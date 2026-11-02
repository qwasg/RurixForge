//! Durable scheduler identity and retry authorization. Verification owns
//! production.json; this module only writes execution.json.
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::path::Path;
use std::sync::Mutex;

static JOURNAL_LOCK: Mutex<()> = Mutex::new(());

#[derive(Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Attempt {
    number: u32,
    status: String,
    prompt_digest: String,
    #[serde(default)]
    retry_authorized: bool,
    #[serde(default)]
    report: String,
    #[serde(default)]
    history: Vec<Value>,
    #[serde(default)]
    capability_digest: Option<String>,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Journal {
    schema_version: u32,
    identity: Value,
    fix_rounds: usize,
    tasks: BTreeMap<String, Attempt>,
    qa: BTreeMap<String, String>,
    #[serde(default)]
    evidence_recollections: std::collections::BTreeSet<String>,
    #[serde(default)]
    previous_plans: Vec<Value>,
}

fn read(dir: &Path) -> Result<Journal, String> {
    let production: Value = serde_json::from_slice(
        &std::fs::read(dir.join("production.json")).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    let identity = json!({"flowId":production["flowId"],"planRev":production["planRev"],"planHash":production["planHash"]});
    if !identity["flowId"].is_string()
        || !identity["planRev"].is_number()
        || !identity["planHash"].is_string()
    {
        return Err("ULTRAPLAN_EXECUTION_IDENTITY: 缺少批准计划身份".into());
    }
    match std::fs::read(dir.join("execution.json")) {
        Ok(bytes) => {
            let journal: Journal = serde_json::from_slice(&bytes)
                .map_err(|e| format!("ULTRAPLAN_EXECUTION_JOURNAL: {e}"))?;
            if journal.schema_version != 1 || journal.identity != identity {
                return Err(
                    "ULTRAPLAN_EXECUTION_IDENTITY: 执行记录对应其他计划，不能重置重试预算".into(),
                );
            }
            Ok(journal)
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Journal {
            schema_version: 1,
            identity,
            fix_rounds: 0,
            tasks: BTreeMap::new(),
            qa: BTreeMap::new(),
            evidence_recollections: Default::default(),
            previous_plans: Vec::new(),
        }),
        Err(e) => Err(e.to_string()),
    }
}

fn save(dir: &Path, journal: &Journal) -> Result<(), String> {
    let bytes = serde_json::to_vec_pretty(journal).map_err(|e| e.to_string())?;
    let tmp = dir.join("execution.json.tmp");
    std::fs::write(&tmp, bytes).map_err(|e| e.to_string())?;
    std::fs::rename(tmp, dir.join("execution.json")).map_err(|e| e.to_string())
}

pub(super) fn fix_rounds(dir: &Path) -> Result<usize, String> {
    let _lock = JOURNAL_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    Ok(read(dir)?.fix_rounds)
}

pub(super) fn active_attempt(dir: &Path, id: &str) -> Result<Option<u32>, String> {
    let _lock = JOURNAL_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    Ok(read(dir)?
        .tasks
        .get(id)
        .filter(|task| task.status == "running" && task.number > 0)
        .map(|task| task.number))
}

/// An explicit coordinator resume may retry a resolved environmental blocker.
/// It is not a product repair and cannot erase or replenish the repair budget.
pub(super) fn authorize_blocked_retry(dir: &Path, ids: &[String]) -> Result<(), String> {
    let _lock = JOURNAL_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let mut journal = read(dir)?;
    if ids.is_empty() {
        return Err("必须明确指定待恢复的阻塞任务".into());
    }
    for id in ids {
        let Some(task) = journal
            .tasks
            .get(id)
            .filter(|task| task.status == "blocked" && task.number > 0)
        else {
            return Err(format!(
                "UP_RESUME_NOT_AUTHORIZED: {id} 不是已确认的阻塞终态"
            ));
        };
        let result = crate::ultraplan::production::read_task_result(dir, id, task.number)?;
        let reason = result
            .as_ref()
            .map(|result| result.reason_code.as_str())
            .unwrap_or_else(|| task.report.split(':').next().unwrap_or_default());
        if !(reason.starts_with("UP_CAPABILITY")
            || reason.starts_with("RX_CLI")
            || matches!(reason, "TOOL_FORBIDDEN" | "CAPABILITY_UNAVAILABLE"))
        {
            return Err(format!(
                "UP_RESUME_NOT_AUTHORIZED: {id} 阻塞原因 {reason} 不能由能力恢复自动重试"
            ));
        }
        let current = capability_digest(dir);
        if current.is_none()
            || task.capability_digest.is_none()
            || current == task.capability_digest
        {
            return Err(format!(
                "UP_CAPABILITY_UNCHANGED: {id} 能力状态未确认变化，保持阻塞，不重复派遣"
            ));
        }
    }
    for id in ids {
        journal.tasks.get_mut(id).unwrap().retry_authorized = true;
    }
    save(dir, &journal)
}

/// Only the lifecycle accepting a newer plan may change journal identity.
/// Historical attempts stay available and the flow's repair budget carries on.
pub(super) fn advance_plan(dir: &Path) -> Result<(), String> {
    let _lock = JOURNAL_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let path = dir.join("execution.json");
    let bytes = match std::fs::read(&path) {
        Ok(bytes) => bytes,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            let journal = read(dir)?;
            return save(dir, &journal);
        }
        Err(e) => return Err(e.to_string()),
    };
    let mut old: Journal = serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
    let production: Value = serde_json::from_slice(
        &std::fs::read(dir.join("production.json")).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    let next = json!({"flowId":production["flowId"],"planRev":production["planRev"],"planHash":production["planHash"]});
    if old.identity == next {
        return Ok(());
    }
    if old.schema_version != 1
        || old.identity["flowId"] != next["flowId"]
        || !next["planHash"].is_string()
        || next["planRev"]
            .as_u64()
            .zip(old.identity["planRev"].as_u64())
            .is_none_or(|(new, old)| new <= old)
        || old.tasks.values().any(|task| task.status == "running")
    {
        return Err(
            "UP_PLAN_ADVANCE_FORBIDDEN: 只允许没有未知执行的同流程新批准版本继承预算".into(),
        );
    }
    old.previous_plans.push(
        json!({"identity":old.identity,"tasks":old.tasks,"qa":old.qa,"fixRounds":old.fix_rounds}),
    );
    old.identity = next;
    old.tasks.clear();
    old.qa.clear();
    old.evidence_recollections.clear();
    save(dir, &old)
}

/// Replaying a still-pending authorization does not spend another repair round.
pub(super) fn reserve_retry(dir: &Path, ids: &[String], maximum: usize) -> Result<(), String> {
    let _lock = JOURNAL_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let mut journal = read(dir)?;
    if ids.is_empty() {
        return Err("修复轮没有明确重试的已有任务".into());
    }
    if ids
        .iter()
        .all(|id| journal.tasks.get(id).is_some_and(|a| a.retry_authorized))
    {
        return Ok(());
    }
    if journal.fix_rounds >= maximum {
        return Err(format!("修复轮已达持久上限 {maximum}"));
    }
    for id in ids {
        if journal.tasks.get(id).is_some_and(|a| a.status == "running") {
            return Err(format!(
                "ULTRAPLAN_RECOVERY_REQUIRED: 任务{id}没有可靠终态，禁止重放"
            ));
        }
    }
    journal.fix_rounds += 1;
    for id in ids {
        journal
            .tasks
            .entry(id.clone())
            .or_default()
            .retry_authorized = true;
    }
    save(dir, &journal)
}

fn capability_digest(dir: &Path) -> Option<String> {
    let value: Value =
        serde_json::from_slice(&std::fs::read(dir.join("preflight.json")).ok()?).ok()?;
    value["capabilityDigest"]
        .as_str()
        .filter(|digest| !digest.is_empty())
        .map(str::to_owned)
}

fn begin_locked(dir: &Path, id: &str, prompt: &str, mut journal: Journal) -> Result<u32, String> {
    let task = journal.tasks.entry(id.into()).or_default();
    if task.status == "running" {
        return Err(format!(
            "ULTRAPLAN_RECOVERY_REQUIRED: 任务{id}仍有未确认终态的执行，未派发"
        ));
    }
    if task.number != 0 && !task.retry_authorized {
        return Err(format!(
            "ULTRAPLAN_RETRY_NOT_AUTHORIZED: 任务{id}未获单次重试授权"
        ));
    }
    if task.number != 0 {
        task.history.push(json!({"attempt":task.number,"status":task.status,"promptDigest":task.prompt_digest,"report":task.report,"capabilityDigest":task.capability_digest}));
    }
    task.number = task.number.checked_add(1).ok_or("任务次数溢出")?;
    task.status = "running".into();
    task.prompt_digest = forge_util::hashutil::sha256_hex(prompt.as_bytes());
    task.retry_authorized = false;
    task.report.clear();
    task.capability_digest = capability_digest(dir);
    let number = task.number;
    save(dir, &journal)?;
    Ok(number)
}

pub(super) fn begin_attempt(dir: &Path, id: &str, prompt: &str) -> Result<u32, String> {
    let _lock = JOURNAL_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    begin_locked(dir, id, prompt, read(dir)?)
}

/// Adopt existing legacy artifacts through host verification, never by replaying
/// a worker or replacing an already recorded/unknown attempt.
pub(super) fn legacy_revalidation_attempt(
    dir: &Path,
    id: &str,
    prompt: &str,
) -> Result<Option<u32>, String> {
    let _lock = JOURNAL_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let journal = read(dir)?;
    if journal
        .tasks
        .get(id)
        .is_some_and(|task| task.number > 0 || task.status == "running")
    {
        return Ok(None);
    }
    begin_locked(dir, id, prompt, journal).map(Some)
}

pub(super) fn reserve_evidence_recollection(dir: &Path, candidate: &str) -> Result<bool, String> {
    let _lock = JOURNAL_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let mut journal = read(dir)?;
    if !journal.evidence_recollections.insert(candidate.into()) {
        return Ok(false);
    }
    save(dir, &journal)?;
    Ok(true)
}

pub(super) fn finish_attempt(
    dir: &Path,
    id: &str,
    number: u32,
    status: &str,
    report: &str,
) -> Result<(), String> {
    let _lock = JOURNAL_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let mut journal = read(dir)?;
    let task = journal.tasks.get_mut(id).ok_or("任务执行记录不存在")?;
    if task.number != number || task.status != "running" {
        return Err("ULTRAPLAN_EXECUTION_STALE: 拒绝旧执行回执".into());
    }
    task.status = status.into();
    task.report = report.into();
    save(dir, &journal)
}

pub(super) fn terminal_result(dir: &Path, id: &str) -> Result<Option<(String, String)>, String> {
    let _lock = JOURNAL_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let journal = read(dir)?;
    Ok(journal
        .tasks
        .get(id)
        .filter(|a| a.number > 0 && !a.retry_authorized && a.status != "running")
        .map(|a| (a.status.clone(), a.report.clone())))
}

fn project_root(dir: &Path) -> Result<std::path::PathBuf, String> {
    let target: Value =
        serde_json::from_slice(&std::fs::read(dir.join("target.json")).map_err(|e| e.to_string())?)
            .map_err(|e| e.to_string())?;
    let workspace = dir
        .ancestors()
        .nth(3)
        .ok_or("流程目录非法")?
        .canonicalize()
        .map_err(|e| e.to_string())?;
    let relative = target["projectRoot"]
        .as_str()
        .ok_or("制作目标缺少projectRoot")?;
    let project = workspace
        .join(relative)
        .canonicalize()
        .map_err(|e| e.to_string())?;
    if !project.starts_with(&workspace) {
        return Err("制作目标不属于工作区".into());
    }
    Ok(project)
}

pub(super) fn candidate(dir: &Path) -> Result<String, String> {
    crate::ultraplan::evidence::project_fingerprint(&project_root(dir)?)
}

pub(super) fn completed_handoff(
    dir: &Path,
    todo: &crate::agent::TodoItem,
    all: &[crate::agent::TodoItem],
) -> Result<Value, String> {
    validate_completed(dir, todo, all)?;
    let attempt = {
        let _lock = JOURNAL_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        read(dir)?
            .tasks
            .get(&todo.id)
            .map(|task| task.number)
            .ok_or("交接任务执行记录不存在")?
    };
    let result = crate::ultraplan::production::read_task_result(dir, &todo.id, attempt)?
        .ok_or("交接结果不存在")?;
    Ok(json!({
        "taskId":todo.id,"title":todo.title,"attempt":attempt,
        "resultPath":crate::ultraplan::production::result_path(dir,&todo.id,attempt),
        "outputs":result.outputs,"receipt":result.receipt,
        "summary":todo.summary,"resultSummary":result.summary,"evidence":result.evidence
    }))
}

/// Recovery may reuse completed work only when its current approved contract and
/// bytes still match a host-issued receipt. A legacy completed todo is not proof.
pub(super) fn validate_completed(
    dir: &Path,
    todo: &crate::agent::TodoItem,
    all: &[crate::agent::TodoItem],
) -> Result<(), String> {
    let (number, completed_owners) = {
        let _lock = JOURNAL_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let journal = read(dir)?;
        let task = journal
            .tasks
            .get(&todo.id)
            .filter(|t| t.status == "completed")
            .ok_or_else(|| {
                format!(
                    "UP_HANDOFF_RECOVERY_REQUIRED: {} 没有可靠执行交接，保留产物先验证",
                    todo.id
                )
            })?;
        let owners: BTreeMap<String, u32> = journal
            .tasks
            .iter()
            .filter_map(|(id, task)| {
                let completed = if task.status == "completed" {
                    Some(task.number)
                } else {
                    task.history
                        .iter()
                        .rev()
                        .find(|attempt| attempt["status"] == "completed")
                        .and_then(|attempt| attempt["attempt"].as_u64())
                        .and_then(|number| u32::try_from(number).ok())
                };
                completed.map(|number| (id.clone(), number))
            })
            .collect();
        (task.number, owners)
    };
    let mut superseded = std::collections::HashSet::new();
    for later in all
        .iter()
        .filter(|t| t.id != todo.id && completed_owners.contains_key(&t.id))
    {
        let mut pending = later.deps.clone();
        let mut seen = std::collections::HashSet::new();
        let mut descendant = false;
        while let Some(dep) = pending.pop() {
            if !seen.insert(dep.clone()) {
                continue;
            }
            if dep == todo.id || dep == todo.title {
                descendant = true;
                break;
            }
            if let Some(parent) = all
                .iter()
                .find(|t| t.id == dep)
                .or_else(|| all.iter().find(|t| t.title == dep))
            {
                pending.extend(parent.deps.iter().cloned());
            }
        }
        if descendant {
            let outputs: std::collections::HashSet<_> =
                crate::ultraplan::production::task_contract(dir, later)?
                    .outputs
                    .into_iter()
                    .collect();
            // Retrying the most recent owner does not make its ancestor the
            // current owner again. Validate the historical handoff identity;
            // the retry must establish fresh bytes before releasing dependents.
            validate_receipt(dir, later, completed_owners[&later.id], &outputs)?;
            superseded.extend(outputs);
        }
    }
    validate_receipt(dir, todo, number, &superseded)
}

fn validate_receipt(
    dir: &Path,
    todo: &crate::agent::TodoItem,
    number: u32,
    superseded: &std::collections::HashSet<String>,
) -> Result<(), String> {
    use crate::ultraplan::production;
    let result = production::read_task_result(dir, &todo.id, number)?
        .filter(|r| r.status == production::ProductionTaskStatus::Completed)
        .ok_or("UP_HANDOFF_RECOVERY_REQUIRED: 缺少完成任务的结构化结果")?;
    let receipt = result
        .receipt
        .ok_or("UP_HANDOFF_RECOVERY_REQUIRED: 缺少宿主交接凭据")?;
    let contract = production::task_contract(dir, todo)?;
    let digest = forge_util::hashutil::sha256_hex(
        &serde_json::to_vec(&contract).map_err(|e| e.to_string())?,
    );
    if receipt["taskId"] != todo.id
        || receipt["attempt"] != number
        || receipt["contractDigest"] != digest
    {
        return Err("UP_HANDOFF_STALE: 任务交接身份或批准契约已经变化".into());
    }
    let outputs = receipt["outputs"]
        .as_array()
        .filter(|rows| !rows.is_empty())
        .ok_or("UP_HANDOFF_RECOVERY_REQUIRED: 缺少产物指纹")?;
    let project = project_root(dir)?;
    for name in &contract.outputs {
        let output = outputs
            .iter()
            .find(|output| output["path"] == *name)
            .ok_or("UP_HANDOFF_RECOVERY_REQUIRED: 凭据未覆盖全部批准产物")?;
        // A verified descendant intentionally owns the newer bytes. Keep the
        // ancestor receipt as history instead of reconstructing its old scene.
        if superseded.contains(name) {
            continue;
        }
        let path = production::output_file(&project, name)?;
        let digest = forge_util::hashutil::sha256_file(&path).map_err(|e| e.to_string())?;
        if output["sha256"] != digest {
            return Err(format!("UP_HANDOFF_STALE: {name} 已修改，需重新验证交接"));
        }
    }
    Ok(())
}

/// Reconcile a host result persisted immediately before a coordinator crash;
/// an attempt without such a result must never be replayed automatically.
pub(super) fn recover_result(
    dir: &Path,
    todo: &crate::agent::TodoItem,
    all: &[crate::agent::TodoItem],
) -> Result<Option<(String, String)>, String> {
    let pending = {
        let _lock = JOURNAL_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let journal = read(dir)?;
        journal
            .tasks
            .get(&todo.id)
            .filter(|a| a.status == "running")
            .map(|a| a.number)
    };
    if let Some(number) = pending {
        use crate::ultraplan::production;
        if let Some(result) = production::read_task_result(dir, &todo.id, number)? {
            let status = match result.status {
                production::ProductionTaskStatus::Completed => {
                    validate_receipt(dir, todo, number, &Default::default())?;
                    "completed"
                }
                production::ProductionTaskStatus::Blocked => "blocked",
                production::ProductionTaskStatus::Failed => "failed",
            };
            finish_attempt(dir, &todo.id, number, status, &result.summary)?;
        }
    }
    let terminal = terminal_result(dir, &todo.id)?;
    if terminal
        .as_ref()
        .is_some_and(|(status, _)| status == "completed")
    {
        validate_completed(dir, todo, all)?;
    }
    Ok(terminal)
}

pub(super) fn qa_result(dir: &Path, candidate: &str) -> Result<Option<String>, String> {
    let _lock = JOURNAL_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    Ok(read(dir)?.qa.get(candidate).cloned())
}

pub(super) fn record_qa(dir: &Path, candidate: &str, report: &str) -> Result<(), String> {
    let _lock = JOURNAL_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let mut journal = read(dir)?;
    journal.qa.insert(candidate.into(), report.into());
    save(dir, &journal)
}

pub(super) fn invalidate_qa(dir: &Path, candidate: &str) -> Result<(), String> {
    let _lock = JOURNAL_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let mut journal = read(dir)?;
    journal.qa.remove(candidate);
    save(dir, &journal)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture() -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(crate::events::new_id("execution-ledger"));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("production.json"),
            r#"{"flowId":"flow","planRev":1,"planHash":"hash"}"#,
        )
        .unwrap();
        dir
    }
    #[test]
    fn attempts_and_budget_survive_resume_and_repeated_authorization() {
        let dir = fixture();
        assert_eq!(begin_attempt(&dir, "task", "build").unwrap(), 1);
        assert!(begin_attempt(&dir, "task", "build").is_err());
        finish_attempt(&dir, "task", 1, "failed", "complete failure report").unwrap();
        assert!(begin_attempt(&dir, "task", "changed prompt").is_err());
        reserve_retry(&dir, &["task".into()], 1).unwrap();
        reserve_retry(&dir, &["task".into()], 1).unwrap();
        assert_eq!(fix_rounds(&dir).unwrap(), 1);
        assert_eq!(begin_attempt(&dir, "task", "targeted repair").unwrap(), 2);
        assert!(finish_attempt(&dir, "task", 1, "completed", "stale").is_err());
        finish_attempt(&dir, "task", 2, "completed", "verified").unwrap();
        assert!(reserve_retry(&dir, &["task".into()], 1).is_err());
        assert_eq!(
            terminal_result(&dir, "task").unwrap(),
            Some(("completed".into(), "verified".into()))
        );
        std::fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn qa_receipts_are_candidate_bound_and_plan_changes_fail_closed() {
        let dir = fixture();
        record_qa(&dir, "version-a", "QA_RESULT: PASS").unwrap();
        assert_eq!(
            qa_result(&dir, "version-a").unwrap().as_deref(),
            Some("QA_RESULT: PASS")
        );
        assert!(qa_result(&dir, "version-b").unwrap().is_none());
        std::fs::write(
            dir.join("production.json"),
            r#"{"flowId":"flow","planRev":2,"planHash":"other"}"#,
        )
        .unwrap();
        assert!(fix_rounds(&dir).is_err());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn blocked_resume_is_explicit_and_does_not_spend_or_reset_product_budget() {
        let dir = fixture();
        assert!(authorize_blocked_retry(&dir, &["absent".into()]).is_err());
        std::fs::write(
            dir.join("preflight.json"),
            r#"{"capabilityDigest":"compiler-missing"}"#,
        )
        .unwrap();
        begin_attempt(&dir, "blocked", "write game").unwrap();
        assert_eq!(active_attempt(&dir, "blocked").unwrap(), Some(1));
        assert!(authorize_blocked_retry(&dir, &["blocked".into()]).is_err());
        finish_attempt(
            &dir,
            "blocked",
            1,
            "blocked",
            "RX_CLI_NOT_FOUND: compiler missing",
        )
        .unwrap();
        assert_eq!(active_attempt(&dir, "blocked").unwrap(), None);
        assert!(authorize_blocked_retry(&dir, &["blocked".into()])
            .unwrap_err()
            .contains("UNCHANGED"));
        std::fs::write(
            dir.join("preflight.json"),
            r#"{"capabilityDigest":"compiler-installed"}"#,
        )
        .unwrap();
        authorize_blocked_retry(&dir, &["blocked".into()]).unwrap();
        authorize_blocked_retry(&dir, &["blocked".into()]).unwrap();
        assert_eq!(fix_rounds(&dir).unwrap(), 0);
        assert_eq!(begin_attempt(&dir, "blocked", "write game").unwrap(), 2);
        assert!(begin_attempt(&dir, "blocked", "write game").is_err());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn legacy_adoption_and_evidence_recollection_are_not_replayed() {
        let dir = fixture();
        assert_eq!(
            legacy_revalidation_attempt(&dir, "old", "existing outputs").unwrap(),
            Some(1)
        );
        assert_eq!(
            legacy_revalidation_attempt(&dir, "old", "existing outputs").unwrap(),
            None
        );
        finish_attempt(&dir, "old", 1, "completed", "host revalidated").unwrap();
        assert_eq!(
            legacy_revalidation_attempt(&dir, "old", "existing outputs").unwrap(),
            None
        );
        assert!(reserve_evidence_recollection(&dir, "candidate-a").unwrap());
        assert!(!reserve_evidence_recollection(&dir, "candidate-a").unwrap());
        assert!(reserve_evidence_recollection(&dir, "candidate-b").unwrap());
        assert_eq!(fix_rounds(&dir).unwrap(), 0);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn newer_approved_plan_retains_fix_budget_and_archives_task_history() {
        let dir = fixture();
        begin_attempt(&dir, "task", "initial").unwrap();
        finish_attempt(&dir, "task", 1, "failed", "failure evidence").unwrap();
        reserve_retry(&dir, &["task".into()], 5).unwrap();
        std::fs::write(
            dir.join("production.json"),
            r#"{"flowId":"flow","planRev":2,"planHash":"new"}"#,
        )
        .unwrap();
        assert!(fix_rounds(&dir).is_err());
        advance_plan(&dir).unwrap();
        assert_eq!(fix_rounds(&dir).unwrap(), 1);
        let journal = read(&dir).unwrap();
        assert!(journal.tasks.is_empty());
        assert_eq!(journal.previous_plans.len(), 1);
        assert_eq!(
            journal.previous_plans[0]["tasks"]["task"]["report"],
            "failure evidence"
        );
        advance_plan(&dir).unwrap();
        assert_eq!(read(&dir).unwrap().previous_plans.len(), 1);
        std::fs::write(
            dir.join("production.json"),
            r#"{"flowId":"flow","planRev":2,"planHash":"unapproved-change"}"#,
        )
        .unwrap();
        assert!(advance_plan(&dir).is_err());
        std::fs::remove_dir_all(dir).unwrap();
    }

    fn handoff_fixture() -> (
        std::path::PathBuf,
        std::path::PathBuf,
        crate::agent::TodoStore,
    ) {
        let workspace = std::env::temp_dir().join(crate::events::new_id("handoff-resume"));
        let dir = workspace.join(".forge/ultraplan/flow");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("production.json"),
            r#"{"flowId":"flow","planRev":1,"planHash":"hash"}"#,
        )
        .unwrap();
        std::fs::write(dir.join("target.json"), r#"{"projectRoot":"."}"#).unwrap();
        std::fs::write(dir.join("team-plan.json"), serde_json::to_vec(&json!([
            {"id":"base","role":"scene-builder","requiredTools":["write_file"],"outputs":["scene.json"],"handoffChecks":[{"kind":"json","path":"scene.json"}]},
            {"id":"consumer","role":"logic-programmer","requiredTools":["write_file"],"outputs":["scene.json"],"handoffChecks":[{"kind":"json","path":"scene.json"}]}
        ])).unwrap()).unwrap();
        let todos = crate::agent::TodoStore::load(workspace.join("todos.json"));
        (workspace, dir, todos)
    }

    fn task(
        todos: &crate::agent::TodoStore,
        id: &str,
        deps: Vec<String>,
    ) -> crate::agent::TodoItem {
        todos
            .create(
                "s",
                crate::agent::NewTodo {
                    title: id.into(),
                    role: Some("scene-builder".into()),
                    deps,
                    prompt: Some(format!("Implement {id}")),
                    plan_todo_id: Some(format!("flow:1:{id}")),
                    source: Some("ultraplan:flow:1".into()),
                    ..Default::default()
                },
            )
            .unwrap()
    }

    fn receipt(dir: &Path, todo: &crate::agent::TodoItem, content: &str) {
        use crate::ultraplan::production;
        let contract = production::task_contract(dir, todo).unwrap();
        let result = production::ProductionTaskResult {
            status: production::ProductionTaskStatus::Completed,
            summary: "host verified".into(),
            reason_code: String::new(),
            outputs: vec!["scene.json".into()],
            evidence: vec![],
            blocked_by: vec![],
            receipt: Some(json!({"taskId":todo.id,"attempt":1,
                "contractDigest":forge_util::hashutil::sha256_hex(&serde_json::to_vec(&contract).unwrap()),
                "outputs":[{"path":"scene.json","sha256":forge_util::hashutil::sha256_hex(content.as_bytes())}]})),
        };
        production::save_task_result(dir, &todo.id, 1, &result).unwrap();
    }

    #[test]
    fn confirmed_handoff_recovers_crash_without_replaying_and_rejects_changed_bytes() {
        let (workspace, dir, todos) = handoff_fixture();
        let todo = task(&todos, "base", vec![]);
        begin_attempt(&dir, &todo.id, todo.prompt.as_deref().unwrap()).unwrap();
        std::fs::write(workspace.join("scene.json"), "original").unwrap();
        receipt(&dir, &todo, "original");
        assert_eq!(
            recover_result(&dir, &todo, std::slice::from_ref(&todo))
                .unwrap()
                .unwrap()
                .0,
            "completed"
        );
        assert_eq!(read(&dir).unwrap().tasks[&todo.id].number, 1);
        assert!(begin_attempt(&dir, &todo.id, "same").is_err());
        std::fs::write(workspace.join("scene.json"), "changed").unwrap();
        assert!(validate_completed(&dir, &todo, std::slice::from_ref(&todo))
            .unwrap_err()
            .contains("STALE"));
        std::fs::remove_dir_all(workspace).unwrap();
    }

    #[test]
    fn completed_descendant_owns_shared_output_without_invalidating_ancestor_history() {
        let (workspace, dir, todos) = handoff_fixture();
        let mut base = task(&todos, "base", vec![]);
        let mut consumer = task(&todos, "consumer", vec![base.id.clone()]);
        for (todo, content) in [(&base, "base-scene"), (&consumer, "final-scene")] {
            begin_attempt(&dir, &todo.id, todo.prompt.as_deref().unwrap()).unwrap();
            std::fs::write(workspace.join("scene.json"), content).unwrap();
            receipt(&dir, todo, content);
            finish_attempt(&dir, &todo.id, 1, "completed", "verified").unwrap();
        }
        base.status = "completed".into();
        consumer.status = "completed".into();
        let all = [base.clone(), consumer.clone()];
        validate_completed(&dir, &base, &all).unwrap();
        validate_completed(&dir, &consumer, &all).unwrap();
        reserve_retry(&dir, &[consumer.id.clone()], 5).unwrap();
        consumer.status = "queued".into();
        validate_completed(&dir, &base, &[base.clone(), consumer.clone()]).unwrap();
        begin_attempt(&dir, &consumer.id, "repair final scene").unwrap();
        std::fs::write(workspace.join("scene.json"), "partial repair").unwrap();
        finish_attempt(&dir, &consumer.id, 2, "failed", "repair incomplete").unwrap();
        consumer.status = "failed".into();
        validate_completed(&dir, &base, &[base.clone(), consumer.clone()]).unwrap();
        std::fs::write(workspace.join("scene.json"), "unverified change").unwrap();
        assert!(validate_completed(&dir, &consumer, &all).is_err());
        std::fs::remove_dir_all(workspace).unwrap();
    }

    #[tokio::test]
    async fn dependent_receives_full_verified_handoff_and_receipt_path() {
        use crate::agent::PatchTodoRequest;
        use crate::plan::{DispatchOutcome, DispatchReq, TeamFlowCtx};
        let (workspace, dir, todos) = handoff_fixture();
        let base = task(&todos, "base", vec![]);
        let consumer = task(&todos, "consumer", vec![base.id.clone()]);
        std::fs::write(
            dir.join("production.json"),
            serde_json::to_vec(&json!({
                "flowId":"flow","planRev":1,"planHash":"hash","activeTodoIds":[base.id,consumer.id]
            }))
            .unwrap(),
        )
        .unwrap();
        let summary = format!("{}CRITICAL_MARKER_AFTER_500", "完整前文".repeat(600));
        begin_attempt(&dir, &base.id, "build base").unwrap();
        std::fs::write(workspace.join("scene.json"), "verified scene").unwrap();
        receipt(&dir, &base, "verified scene");
        finish_attempt(&dir, &base.id, 1, "completed", &summary).unwrap();
        todos
            .patch(
                &base.id,
                &PatchTodoRequest {
                    status: Some("completed".into()),
                    summary: Some(summary),
                    ..Default::default()
                },
            )
            .unwrap();
        let events = crate::events::EventBus::new(workspace.join("events"), 64);
        let cancelled = || false;
        let ctx = TeamFlowCtx {
            todos: &todos,
            events: &events,
            session_id: "s",
            user_goal: "game",
            cancelled: &cancelled,
            max_fix_rounds: 5,
            source_filter: Some("ultraplan:flow:1"),
            serialize_engine: true,
            validate: None,
            production_dir: Some(&dir),
            verification_issues: None,
        };
        let seen = Mutex::new(String::new());
        let dispatch = |request: DispatchReq| {
            assert_eq!(request.task_id.as_deref(), Some(consumer.id.as_str()));
            *seen.lock().unwrap() = request.prompt;
            async { DispatchOutcome::Completed("consumer done".into()) }
        };
        let report = crate::plan::run_ready_waves(&ctx, &dispatch).await;
        assert!(report.stalled.is_none(), "{:?}", report.stalled);
        let prompt = seen.lock().unwrap();
        assert!(prompt.starts_with(consumer.prompt.as_deref().unwrap()));
        assert!(prompt.contains("CRITICAL_MARKER_AFTER_500"));
        let context: Value =
            serde_json::from_str(prompt.split("[上游已验证交接资料]\n").nth(1).unwrap()).unwrap();
        assert_eq!(
            context[0]["resultPath"],
            json!(crate::ultraplan::production::result_path(&dir, &base.id, 1))
        );
        assert_eq!(context[0]["receipt"]["taskId"], base.id);
        assert_eq!(context[0]["outputs"], json!(["scene.json"]));
        std::fs::remove_dir_all(workspace).unwrap();
    }
}
