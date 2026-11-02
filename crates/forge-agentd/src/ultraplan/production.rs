//! Host-owned production handoffs. A model turn ending is not a task receipt.
use std::collections::HashSet;
use std::path::{Component, Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::agent::TodoItem;
use crate::llm::ExecFn;

pub const RESULT_TOOL: &str = "ultraplan_task_result";

#[path = "production_recovery.rs"]
mod recovery;
pub use recovery::{recover_pending_handoff, PendingHandoffRecovery};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ProductionTaskStatus {
    Completed,
    Blocked,
    Failed,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProductionTaskResult {
    pub status: ProductionTaskStatus,
    pub summary: String,
    #[serde(default)]
    pub reason_code: String,
    #[serde(default)]
    pub outputs: Vec<String>,
    #[serde(default)]
    pub evidence: Vec<String>,
    #[serde(default)]
    pub blocked_by: Vec<String>,
    /// Set only by the host after checking the approved handoff contract.
    #[serde(default)]
    pub receipt: Option<Value>,
}

impl ProductionTaskResult {
    pub fn incomplete(code: &str, message: impl Into<String>) -> Self {
        let mut result = Self::failure(code, message);
        result.status = ProductionTaskStatus::Blocked;
        result
    }
    pub fn failure(code: &str, message: impl Into<String>) -> Self {
        Self {
            status: ProductionTaskStatus::Failed,
            summary: message.into(),
            reason_code: code.into(),
            outputs: vec![],
            evidence: vec![],
            blocked_by: vec![],
            receipt: None,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskContract {
    pub id: String,
    pub role: String,
    #[serde(default)]
    pub required_tools: Vec<String>,
    #[serde(default)]
    pub outputs: Vec<String>,
    #[serde(default)]
    pub handoff_checks: Vec<Value>,
}

pub fn contracts(dir: &Path) -> Result<Vec<TaskContract>, String> {
    let bytes = std::fs::read(dir.join("team-plan.json")).map_err(|e| e.to_string())?;
    let tasks: Vec<Value> = serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
    tasks
        .into_iter()
        .map(|task| {
            super::lifecycle::validate_task_contract(&task)?;
            serde_json::from_value(task).map_err(|e| e.to_string())
        })
        .collect()
}

pub fn task_contract(dir: &Path, todo: &TodoItem) -> Result<TaskContract, String> {
    let id = todo
        .plan_todo_id
        .as_deref()
        .and_then(|id| id.rsplit(':').next())
        .ok_or("UP_TASK_NOT_APPROVED: 制作任务缺少批准任务 ID")?;
    contracts(dir)?
        .into_iter()
        .find(|task| task.id == id)
        .ok_or_else(|| format!("UP_TASK_NOT_APPROVED: {id}"))
}

pub fn missing_tools(contract: &TaskContract, tools: &[Value]) -> Vec<String> {
    let actual: HashSet<&str> = tools
        .iter()
        .filter_map(|t| t.pointer("/function/name").and_then(Value::as_str))
        .collect();
    contract
        .required_tools
        .iter()
        .filter(|name| !actual.contains(name.as_str()))
        .cloned()
        .collect()
}

pub fn result_tool() -> Value {
    json!({"type":"function","function":{"name":RESULT_TOOL,
        "description":"提交本任务真实结果。completed只申请交接验证，宿主核对批准产物和局部检查后才放行下游；缺能力/依赖用blocked，产品缺陷用failed。不得自行提交receipt。",
        "parameters":{"type":"object","additionalProperties":false,"properties":{
            "status":{"type":"string","enum":["completed","blocked","failed"]},
            "summary":{"type":"string","minLength":1},"reasonCode":{"type":"string"},
            "outputs":{"type":"array","items":{"type":"string"}},
            "evidence":{"type":"array","items":{"type":"string"}},
            "blockedBy":{"type":"array","items":{"type":"string"}}},
            "required":["status","summary","outputs","evidence"]}}})
}

pub fn submission(args: &Value) -> Result<ProductionTaskResult, String> {
    if args.get("receipt").is_some() {
        return Err("UP_RESULT_INVALID: receipt 只能由宿主生成".into());
    }
    let result: ProductionTaskResult =
        serde_json::from_value(args.clone()).map_err(|e| format!("UP_RESULT_INVALID: {e}"))?;
    if result.summary.trim().is_empty() {
        return Err("UP_RESULT_INVALID: summary 不可为空".into());
    }
    if result.status != ProductionTaskStatus::Completed && result.reason_code.trim().is_empty() {
        return Err("UP_RESULT_INVALID: blocked/failed 必须给出 reasonCode".into());
    }
    Ok(result)
}

pub(crate) fn result_path(dir: &Path, task_id: &str, attempt: u32) -> PathBuf {
    dir.join("task-results")
        .join(forge_util::hashutil::sha256_hex(task_id.as_bytes()))
        .join(format!("{attempt}.json"))
}

pub fn save_task_result(
    dir: &Path,
    task_id: &str,
    attempt: u32,
    result: &ProductionTaskResult,
) -> Result<(), String> {
    super::write_json_atomic(
        &result_path(dir, task_id, attempt),
        &serde_json::to_value(result).map_err(|e| e.to_string())?,
    )
    .map_err(|e| format!("UP_RESULT_PERSIST_FAILED: {e}"))
}

pub fn read_task_result(
    dir: &Path,
    task_id: &str,
    attempt: u32,
) -> Result<Option<ProductionTaskResult>, String> {
    match std::fs::read(result_path(dir, task_id, attempt)) {
        Ok(bytes) => serde_json::from_slice(&bytes)
            .map(Some)
            .map_err(|e| e.to_string()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e.to_string()),
    }
}

fn pending_path(dir: &Path, task_id: &str, attempt: u32) -> PathBuf {
    dir.join("pending-submissions")
        .join(forge_util::hashutil::sha256_hex(task_id.as_bytes()))
        .join(format!("{attempt}.json"))
}

/// Acknowledged model submissions survive a host crash, but are never final
/// receipts. Only a confirmed terminal model turn may trigger their verification.
pub fn save_pending_submission(
    dir: &Path,
    task_id: &str,
    attempt: u32,
    result: &ProductionTaskResult,
) -> Result<(), String> {
    if result.receipt.is_some() {
        return Err("UP_RESULT_INVALID: 待验证申请不可包含宿主凭据".into());
    }
    let mut value = serde_json::to_value(result).map_err(|e| e.to_string())?;
    value.as_object_mut().unwrap().remove("receipt");
    submission(&value)?;
    super::write_json_atomic(&pending_path(dir, task_id, attempt), &value)
        .map_err(|e| format!("UP_RESULT_PERSIST_FAILED: {e}"))
}

pub fn read_pending_submission(
    dir: &Path,
    task_id: &str,
    attempt: u32,
) -> Result<Option<ProductionTaskResult>, String> {
    match std::fs::read(pending_path(dir, task_id, attempt)) {
        Ok(bytes) => {
            let value = serde_json::from_slice(&bytes)
                .map_err(|e| format!("UP_RESULT_PERSIST_FAILED: {e}"))?;
            submission(&value).map(Some)
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(format!("UP_RESULT_PERSIST_FAILED: {e}")),
    }
}

/// Reject aliases/reparse points and workflow-owned evidence as task outputs.
pub fn output_file(root: &Path, relative: &str) -> Result<PathBuf, String> {
    let relative = relative.replace('\\', "/");
    if relative.is_empty()
        || relative.contains(':')
        || relative.starts_with('/')
        || relative
            .split('/')
            .any(|p| p.is_empty() || p == "." || p == ".." || p.ends_with([' ', '.']))
        || relative
            .split('/')
            .next()
            .is_some_and(|s| s.eq_ignore_ascii_case(".forge") || s.eq_ignore_ascii_case(".git"))
    {
        return Err(format!("UP_OUTPUT_INVALID: {relative}"));
    }
    let root = root.canonicalize().map_err(|e| e.to_string())?;
    let mut path = root.clone();
    for component in Path::new(&relative).components() {
        if !matches!(component, Component::Normal(_)) {
            return Err("UP_OUTPUT_INVALID".into());
        }
        path.push(component);
        let meta = std::fs::symlink_metadata(&path)
            .map_err(|e| format!("UP_OUTPUT_MISSING: {relative}: {e}"))?;
        #[cfg(windows)]
        {
            use std::os::windows::fs::MetadataExt;
            if meta.file_attributes() & 0x400 != 0 {
                return Err("UP_OUTPUT_INVALID: reparse point".into());
            }
        }
        if meta.file_type().is_symlink() {
            return Err("UP_OUTPUT_INVALID: symlink".into());
        }
    }
    let path = path.canonicalize().map_err(|e| e.to_string())?;
    if !path.starts_with(root)
        || !path.is_file()
        || path.metadata().map_err(|e| e.to_string())?.len() == 0
    {
        return Err(format!("UP_OUTPUT_MISSING: {relative} 必须为非空普通文件"));
    }
    Ok(path)
}

async fn call(exec: &ExecFn, name: &str, args: Value) -> Result<Value, String> {
    let (ok, feedback) = exec(name.into(), args).await;
    if !ok {
        return Err(feedback.text);
    }
    let value: Value = serde_json::from_str(&feedback.text)
        .map_err(|e| format!("UP_HANDOFF_INVALID_TOOL_RESULT: {name}: {e}"))?;
    if value.get("content").is_some() {
        crate::playtest::unwrap_envelope(&value)
    } else {
        Ok(value)
    }
}

pub async fn verify_handoff(
    root: &Path,
    contract: &TaskContract,
    task_id: &str,
    attempt: u32,
    mut result: ProductionTaskResult,
    exec: &ExecFn,
) -> ProductionTaskResult {
    if result.status != ProductionTaskStatus::Completed {
        return result;
    }
    let _execution = super::verify::EXECUTION_LOCK.lock().await;
    let checked = async {
        let mut outputs = Vec::new();
        for name in &contract.outputs {
            if !result.outputs.contains(name) { return Err(format!("UP_OUTPUT_MISSING: 未提交批准产物 {name}")); }
            let path = output_file(root, name)?;
            let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
            outputs.push(json!({"path":name,"sha256":forge_util::hashutil::sha256_hex(&bytes)}));
            if !contract.handoff_checks.iter().any(|c| c["path"] == *name) {
                return Err(format!("UP_HANDOFF_MISSING: {name} 未配置交接检查"));
            }
        }
        for path in &result.outputs { output_file(root, path)?; }
        for path in &result.evidence { output_file(root, path)?; }
        let mut checks = Vec::new();
        for check in &contract.handoff_checks {
            let relative = check["path"].as_str().ok_or("UP_HANDOFF_INVALID: 缺 path")?;
            if !contract.outputs.iter().any(|p| p == relative) { return Err("UP_HANDOFF_INVALID: 非批准产物".into()); }
            let path = output_file(root, relative)?;
            let bytes = std::fs::read(&path).map_err(|e| e.to_string())?;
            let detail = match check["kind"].as_str() {
                Some("file") => json!({"bytes":bytes.len()}),
                Some("json") => { serde_json::from_slice::<Value>(&bytes).map_err(|e| format!("{relative}: {e}"))?; json!({"parsed":true}) },
                Some("scene") => {
                    let text = std::str::from_utf8(&bytes).map_err(|e| e.to_string())?;
                    let scene = forge_scene::Scene::from_json(text).map_err(|e| e.to_string())?;
                    let mut ids = HashSet::new();
                    for entity in &scene.entities {
                        if !ids.insert(entity.id) { return Err(format!("UP_HANDOFF_FAILED: {relative} 重复实体ID")); }
                        for component in &entity.components { forge_scene::validate_component(component)?; }
                    }
                    json!({"entityCount":scene.entities.len()})
                },
                Some("graph") => {
                    let report = call(exec,"mcp__code-forge__graph_validate",json!({"path":path})).await?;
                    if report["ok"] != true { return Err(format!("UP_HANDOFF_FAILED: {report}")); }
                    report
                },
                Some("rx") => {
                    let report = call(exec,"mcp__code-forge__rx_check",json!({"file":path})).await?;
                    if report["ok"] != true { return Err(format!("UP_HANDOFF_FAILED: {report}")); }
                    report
                },
                Some("matrix") => {
                    let mut matrix: crate::playtest::Matrix = serde_json::from_value(check["matrix"].clone()).map_err(|e| e.to_string())?;
                    if matrix.scene != relative { return Err("UP_HANDOFF_INVALID: matrix.scene 必须等于 path".into()); }
                    matrix.scene = path.to_string_lossy().into_owned();
                    let mut invoke = |name: String, args: Value| async move { call(exec, &name, args).await };
                    let report = crate::playtest::run_matrix(&matrix, &mut invoke).await?.to_json();
                    if report["ok"] != true { return Err(format!("UP_HANDOFF_FAILED: {report}")); }
                    report
                },
                _ => return Err("UP_HANDOFF_INVALID: 未知检查类型".into()),
            };
            checks.push(json!({"check":check,"result":detail}));
        }
        // Verification must describe the files that still exist after engine checks.
        for output in &outputs {
            let path = output_file(root, output["path"].as_str().unwrap())?;
            let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
            if output["sha256"] != forge_util::hashutil::sha256_hex(&bytes) { return Err("UP_HANDOFF_STALE: 验证期间产物发生变化".into()); }
        }
        Ok::<Value,String>(json!({"taskId":task_id,"attempt":attempt,
            "contractDigest":forge_util::hashutil::sha256_hex(&serde_json::to_vec(contract).map_err(|e| e.to_string())?),
            "outputs":outputs,"checks":checks,"verifiedAt":crate::events::now_rfc3339()}))
    }.await;
    match checked {
        Ok(receipt) => result.receipt = Some(receipt),
        Err(error) => {
            result.status = if error.contains("RX_CLI_NOT_FOUND")
                || error.contains("TOOL_FORBIDDEN")
                || error.contains("CAPABILITY_UNAVAILABLE")
            {
                ProductionTaskStatus::Blocked
            } else {
                ProductionTaskStatus::Failed
            };
            result.reason_code = error
                .split(':')
                .next()
                .unwrap_or("UP_HANDOFF_FAILED")
                .into();
            result.summary = format!("{}\n\n宿主交接验证未通过：{error}", result.summary);
            result.receipt = None;
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn pending_submission_survives_restart_without_becoming_a_final_receipt() {
        let dir = std::env::temp_dir().join(crate::events::new_id("up-pending-test"));
        let result = submission(&json!({"status":"blocked","summary":"compiler missing","reasonCode":"RX_CLI_NOT_FOUND"})).unwrap();
        save_pending_submission(&dir, "todo-1", 1, &result).unwrap();
        let recovered = read_pending_submission(&dir, "todo-1", 1).unwrap().unwrap();
        assert_eq!(recovered.status, ProductionTaskStatus::Blocked);
        assert_eq!(recovered.reason_code, "RX_CLI_NOT_FOUND");
        assert!(recovered.receipt.is_none());
        assert!(read_task_result(&dir, "todo-1", 1).unwrap().is_none());
        assert!(read_pending_submission(&dir, "todo-1", 2)
            .unwrap()
            .is_none());
        let mut forged = recovered;
        forged.receipt = Some(json!({"taskId":"todo-1"}));
        assert!(save_pending_submission(&dir, "todo-1", 1, &forged).is_err());
        let bytes = std::fs::read(pending_path(&dir, "todo-1", 1)).unwrap();
        let mut forged_json: Value = serde_json::from_slice(&bytes).unwrap();
        forged_json["receipt"] = json!({});
        super::super::write_json_atomic(&pending_path(&dir, "todo-1", 1), &forged_json).unwrap();
        assert!(read_pending_submission(&dir, "todo-1", 1).is_err());
        std::fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn result_does_not_accept_forged_receipts_or_implicit_success() {
        assert!(submission(&json!({"summary":"completed"})).is_err());
        assert!(submission(&json!({"status":"completed","summary":"ok","receipt":{}})).is_err());
        let result=submission(&json!({"status":"blocked","summary":"compiler missing","reasonCode":"RX_CLI_NOT_FOUND"})).unwrap();
        assert_eq!(result.status, ProductionTaskStatus::Blocked);
    }
    #[tokio::test]
    async fn normal_completion_without_outputs_is_not_a_handoff() {
        let root = std::env::temp_dir();
        let contract = TaskContract {
            id: "T01".into(),
            role: "logic-programmer".into(),
            required_tools: vec![],
            outputs: vec!["missing-production-result.rx".into()],
            handoff_checks: vec![json!({"kind":"file","path":"missing-production-result.rx"})],
        };
        let result =
            submission(&json!({"status":"completed","summary":"done","outputs":[]})).unwrap();
        let exec: Box<ExecFn> =
            Box::new(|_, _| Box::pin(async { panic!("must not call a tool for absent output") }));
        let result = verify_handoff(&root, &contract, "todo-1", 1, result, exec.as_ref()).await;
        assert_eq!(result.status, ProductionTaskStatus::Failed);
        assert!(result.receipt.is_none());
    }
    #[test]
    fn output_paths_cannot_use_workflow_records_or_parent_aliases() {
        for path in [
            ".forge/production.json",
            "../output.json",
            "a/../b",
            "C:/a",
            "a./b",
        ] {
            assert!(output_file(&std::env::temp_dir(), path).is_err());
        }
    }
}
