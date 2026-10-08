//! Engine-neutral job settings for the existing host-owned tool loop.
//!
//! A job does not own a Forge session/run: the parent coordinator does. Each
//! engine supplies a `StepFn`, while Forge continues to execute and authorize
//! tools, record evidence, and decide workflow transitions.

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashSet;
use std::path::Path;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, OnceLock};

pub type CancelCheck = Arc<dyn Fn() -> bool + Send + Sync>;

#[derive(Clone)]
pub struct AgentJobSpec {
    pub job_id: String,
    pub cwd: PathBuf,
    pub model: Option<String>,
    pub effort: Option<String>,
    pub cancelled: CancelCheck,
    /// Per-flow journal directory, never the ordinary chat thread mapping.
    pub state_dir: Option<PathBuf>,
    pub metadata: AgentJobMetadata,
    /// Only a coordinator recovery may reuse a completed result. Ordinary
    /// calls (including QA and requested repairs) start another generation.
    pub resume_completed: bool,
    /// Explicit coordinator authorization to retry a confirmed terminal job.
    pub retry_failed: bool,
}

/// Coordination context is engine-neutral and persists with the job identity.
/// The host still enforces scopes; storing them is not an authorization grant.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentJobMetadata {
    pub flow_id: Option<String>,
    pub revision: Option<u64>,
    pub phase: Option<String>,
    pub role: Option<String>,
    pub requirement_pack: Option<String>,
    pub requirement_digest: Option<String>,
    pub backend: Option<String>,
    #[serde(default)]
    pub read_scope: Vec<String>,
    #[serde(default)]
    pub write_scope: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AgentJobStatus {
    Starting,
    Running,
    Completed,
    Failed,
    Cancelled,
    RecoveryRequired,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentJobResult {
    pub status: AgentJobStatus,
    pub text: String,
    pub error: Option<String>,
    #[serde(default)]
    pub evidence: Vec<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentJobRecord {
    pub schema_version: u32,
    pub job_id: String,
    pub engine: String,
    pub cwd: PathBuf,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub effort: Option<String>,
    pub metadata: AgentJobMetadata,
    pub tool_signature: String,
    #[serde(default = "first_generation")]
    pub generation: u64,
    #[serde(default)]
    pub terminal_confirmed: bool,
    pub thread_id: Option<String>,
    pub turn_id: Option<String>,
    pub result: AgentJobResult,
}

impl AgentJobRecord {
    pub fn starting(spec: &AgentJobSpec, engine: &str, tool_signature: String) -> Self {
        Self {
            schema_version: 1,
            job_id: spec.job_id.clone(),
            engine: engine.into(),
            cwd: spec.cwd.clone(),
            model: spec.model.clone(),
            effort: spec.effort.clone(),
            metadata: spec.metadata.clone(),
            tool_signature,
            generation: 1,
            terminal_confirmed: false,
            thread_id: None,
            turn_id: None,
            result: AgentJobResult {
                status: AgentJobStatus::Starting,
                text: String::new(),
                error: None,
                evidence: Vec::new(),
            },
        }
    }
}

fn first_generation() -> u64 {
    1
}

pub(crate) fn journal_path(spec: &AgentJobSpec) -> Option<PathBuf> {
    spec.state_dir.as_ref().map(|dir| {
        dir.join(format!(
            "{}.json",
            forge_util::hashutil::sha256_hex(spec.job_id.as_bytes())
        ))
    })
}

impl AgentJobSpec {
    pub fn new(job_id: impl Into<String>, cwd: PathBuf) -> Self {
        Self {
            job_id: job_id.into(),
            cwd,
            model: None,
            effort: None,
            cancelled: Arc::new(|| false),
            state_dir: None,
            metadata: AgentJobMetadata::default(),
            resume_completed: false,
            retry_failed: false,
        }
    }
}

/// Local and Codex jobs share the durable identity contract. Local jobs do not
/// invent an upstream thread/turn or replay work whose terminal state is unknown.
pub async fn run_local_job(
    spec: Option<&AgentJobSpec>,
    system: &str,
    user: &str,
    cfg: crate::llm::ToolLoopCfg<'_>,
) -> Result<crate::llm::ToolLoopOutcome, crate::llm::LlmError> {
    let Some(spec) = spec.filter(|s| s.state_dir.is_some()) else {
        return crate::llm::run_tool_loop(system, user, cfg).await;
    };
    let path = journal_path(spec).expect("state_dir checked");
    let _claim = LocalJobClaim::acquire(&path)?;
    let signature = local_tool_signature(&cfg.tools)?;
    let previous = match std::fs::read_to_string(&path) {
        Ok(text) => Some(
            serde_json::from_str::<AgentJobRecord>(&text)
                .map_err(|e| local_error("JOURNAL_INVALID", e))?,
        ),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
        Err(e) => return Err(local_error("JOURNAL_IO", e)),
    };
    let mut record = AgentJobRecord::starting(spec, "local", signature.clone());
    if let Some(mut old) = previous {
        if old.schema_version != 1
            || old.job_id != spec.job_id
            || old.engine != "local"
            || old.cwd != spec.cwd
            || old.metadata != spec.metadata
            || old.tool_signature != signature
        {
            return Err(local_error(
                "IDENTITY_MISMATCH",
                "已有任务的需求、目录或工具集与本次不同",
            ));
        }
        if matches!(
            old.result.status,
            AgentJobStatus::Starting | AgentJobStatus::Running
        ) {
            old.result.status = AgentJobStatus::RecoveryRequired;
            old.result.error =
                Some("任务没有可靠终态；请检查已有产物后显式重试，禁止自动重放".into());
            old.terminal_confirmed = false;
            persist_local_record(&path, &old)?;
            return Err(local_error(
                "RECOVERY_REQUIRED",
                "发现上次未收束任务，已保留记录；此次不执行任何工具",
            ));
        }
        if matches!(
            old.result.status,
            AgentJobStatus::Failed | AgentJobStatus::Cancelled | AgentJobStatus::RecoveryRequired
        ) && !spec.retry_failed
        {
            return Err(local_error(
                "RECOVERY_REQUIRED",
                "请由阶段控制器显式授权重试已有失败或不确定任务",
            ));
        }
        if old.result.status == AgentJobStatus::Completed && spec.resume_completed {
            if !old.terminal_confirmed {
                return Err(local_error("RECOVERY_REQUIRED", "已有完成记录没有可靠终态"));
            }
            return read_local_outcome(&path, &old);
        }
        record.generation = old
            .generation
            .checked_add(1)
            .ok_or_else(|| local_error("JOURNAL_INVALID", "任务代次溢出"))?;
    }
    persist_local_record(&path, &record)?;
    record.result.status = AgentJobStatus::Running;
    persist_local_record(&path, &record)?;

    let prior_cancelled = cfg.cancelled;
    let cancelled = || (spec.cancelled)() || prior_cancelled.is_some_and(|check| check());
    let outcome = crate::llm::run_tool_loop(
        system,
        user,
        crate::llm::ToolLoopCfg {
            cancelled: Some(&cancelled),
            ..cfg
        },
    )
    .await;
    let mut terminal_error = None;
    let evidence = match &outcome {
        Ok(out) => {
            let exhausted = out.exhausted;
            record.result.status = if out.cancelled {
                AgentJobStatus::Cancelled
            } else if exhausted {
                AgentJobStatus::Failed
            } else {
                AgentJobStatus::Completed
            };
            record.result.text = out.text.clone();
            if exhausted {
                let error = local_error("INCOMPLETE", "工具循环达到上限但没有收束");
                record.result.error = Some(error.to_string());
                terminal_error = Some(error);
            }
            json!({"schemaVersion":1,"jobId":spec.job_id,"generation":record.generation,
                "outcome":{"text":out.text,"iters":out.iters,"cancelled":out.cancelled,"exhausted":out.exhausted,
                    "records":out.records.iter().map(|r|json!({"name":r.name,"ok":r.ok,"summary":r.summary})).collect::<Vec<_>>()}})
        }
        Err(error) => {
            record.result.status = if cancelled() {
                AgentJobStatus::Cancelled
            } else {
                AgentJobStatus::Failed
            };
            record.result.error = Some(error.to_string());
            json!({"schemaVersion":1,"jobId":spec.job_id,"generation":record.generation,"error":error.to_string()})
        }
    };
    let evidence_path = local_evidence_path(&path, record.generation);
    let body =
        serde_json::to_string_pretty(&evidence).map_err(|e| local_error("JOURNAL_INVALID", e))?;
    // Each generation owns one immutable evidence file. A collision signals an
    // inconsistent journal and must not overwrite the older execution evidence.
    use std::io::Write;
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&evidence_path)
        .map_err(|e| local_error("JOURNAL_IO", e))?;
    file.write_all(body.as_bytes())
        .and_then(|_| file.sync_all())
        .map_err(|e| local_error("JOURNAL_IO", e))?;
    record.result.evidence = vec![evidence_path.to_string_lossy().into_owned()];
    record.terminal_confirmed = true;
    persist_local_record(&path, &record)?;
    if let Some(error) = terminal_error {
        return Err(error);
    }
    outcome
}

fn local_error(code: &str, error: impl std::fmt::Display) -> crate::llm::LlmError {
    crate::llm::LlmError::new(format!("LOCAL_JOB_{code}: {error}"))
}

fn local_tool_signature(tools: &[Value]) -> Result<String, crate::llm::LlmError> {
    let bytes = serde_json::to_vec(tools).map_err(|e| local_error("JOURNAL_INVALID", e))?;
    Ok(forge_util::hashutil::sha256_hex(&bytes))
}

fn persist_local_record(path: &Path, record: &AgentJobRecord) -> Result<(), crate::llm::LlmError> {
    let text =
        serde_json::to_string_pretty(record).map_err(|e| local_error("JOURNAL_INVALID", e))?;
    crate::sessions::write_atomic(path, &text).map_err(|e| local_error("JOURNAL_IO", e))
}

fn local_evidence_path(journal: &Path, generation: u64) -> PathBuf {
    journal.with_file_name(format!(
        "{}.g{generation}.records.json",
        journal.file_stem().unwrap_or_default().to_string_lossy()
    ))
}

fn read_local_outcome(
    path: &Path,
    record: &AgentJobRecord,
) -> Result<crate::llm::ToolLoopOutcome, crate::llm::LlmError> {
    let evidence = local_evidence_path(path, record.generation);
    if record.result.evidence != vec![evidence.to_string_lossy().into_owned()] {
        return Err(local_error("JOURNAL_INVALID", "任务证据索引不匹配"));
    }
    let text = std::fs::read_to_string(evidence).map_err(|e| local_error("JOURNAL_IO", e))?;
    let doc: Value = serde_json::from_str(&text).map_err(|e| local_error("JOURNAL_INVALID", e))?;
    if doc["schemaVersion"] != 1
        || doc["jobId"] != record.job_id
        || doc["generation"] != record.generation
    {
        return Err(local_error("JOURNAL_INVALID", "任务证据身份不匹配"));
    }
    let out = &doc["outcome"];
    let invalid = || local_error("JOURNAL_INVALID", "任务证据缺少完整的工具循环结果");
    let records = out["records"]
        .as_array()
        .ok_or_else(invalid)?
        .iter()
        .map(|row| {
            Ok(crate::llm::ToolCallRecord {
                name: row["name"].as_str().ok_or_else(invalid)?.into(),
                ok: row["ok"].as_bool().ok_or_else(invalid)?,
                summary: row["summary"].as_str().ok_or_else(invalid)?.into(),
            })
        })
        .collect::<Result<Vec<_>, crate::llm::LlmError>>()?;
    let result = crate::llm::ToolLoopOutcome {
        text: out["text"].as_str().ok_or_else(invalid)?.into(),
        records,
        iters: out["iters"]
            .as_u64()
            .and_then(|n| usize::try_from(n).ok())
            .ok_or_else(invalid)?,
        cancelled: out["cancelled"].as_bool().ok_or_else(invalid)?,
        exhausted: out["exhausted"].as_bool().ok_or_else(invalid)?,
    };
    if result.cancelled || result.exhausted || result.text != record.result.text {
        return Err(invalid());
    }
    Ok(result)
}

static LOCAL_JOBS: OnceLock<Mutex<HashSet<PathBuf>>> = OnceLock::new();
struct LocalJobClaim(PathBuf);
impl LocalJobClaim {
    fn acquire(path: &Path) -> Result<Self, crate::llm::LlmError> {
        let parent = path
            .parent()
            .ok_or_else(|| local_error("JOURNAL_INVALID", "任务目录缺失"))?;
        std::fs::create_dir_all(parent).map_err(|e| local_error("JOURNAL_IO", e))?;
        let key = parent
            .canonicalize()
            .map_err(|e| local_error("JOURNAL_IO", e))?
            .join(path.file_name().unwrap());
        if !LOCAL_JOBS
            .get_or_init(|| Mutex::new(HashSet::new()))
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(key.clone())
        {
            return Err(local_error("ALREADY_RUNNING", "同一任务正在执行"));
        }
        Ok(Self(key))
    }
}
impl Drop for LocalJobClaim {
    fn drop(&mut self) {
        if let Some(jobs) = LOCAL_JOBS.get() {
            jobs.lock()
                .unwrap_or_else(|e| e.into_inner())
                .remove(&self.0);
        }
    }
}

#[cfg(test)]
mod local_tests {
    use super::*;
    use crate::llm::{ExecFn, LlmError, StepFn, StepOutcome, ToolLoopCfg};
    use std::collections::VecDeque;
    use std::sync::atomic::{AtomicUsize, Ordering};

    struct Fixture(PathBuf);
    impl Fixture {
        fn new() -> Self {
            let root = std::env::temp_dir().join(format!(
                "forge-local-job-test-{}",
                crate::events::new_id("t")
            ));
            std::fs::create_dir_all(&root).unwrap();
            Self(root)
        }
        fn spec(&self, id: &str) -> AgentJobSpec {
            let mut spec = AgentJobSpec::new(id, self.0.clone());
            spec.state_dir = Some(self.0.join("jobs"));
            spec.metadata = AgentJobMetadata {
                flow_id: Some("flow-1".into()),
                revision: Some(2),
                requirement_digest: Some("requirements-v2".into()),
                ..Default::default()
            };
            spec
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            if let (Ok(root), Ok(temp)) =
                (self.0.canonicalize(), std::env::temp_dir().canonicalize())
            {
                if root.parent() == Some(temp.as_path())
                    && root
                        .file_name()
                        .is_some_and(|s| s.to_string_lossy().starts_with("forge-local-job-test-"))
                {
                    let _ = std::fs::remove_dir_all(root);
                }
            }
        }
    }
    fn load(spec: &AgentJobSpec) -> AgentJobRecord {
        serde_json::from_str(&std::fs::read_to_string(journal_path(spec).unwrap()).unwrap())
            .unwrap()
    }
    fn scripted(
        messages: Vec<Result<Value, &'static str>>,
        calls: Arc<AtomicUsize>,
        path: Option<PathBuf>,
    ) -> Box<StepFn> {
        let queue = Arc::new(Mutex::new(VecDeque::from(messages)));
        Box::new(move |_, _, _| {
            calls.fetch_add(1, Ordering::SeqCst);
            if let Some(path) = &path {
                let record: AgentJobRecord =
                    serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
                assert_eq!(record.result.status, AgentJobStatus::Running);
                assert!(!record.terminal_confirmed);
            }
            let next = queue
                .lock()
                .unwrap()
                .pop_front()
                .expect("unexpected model replay");
            Box::pin(async move {
                next.map(|message| StepOutcome {
                    message,
                    usage: None,
                })
                .map_err(LlmError::new)
            })
        })
    }
    fn final_message(text: &str) -> Value {
        json!({"role":"assistant","content":text})
    }
    fn call_message() -> Value {
        json!({"role":"assistant","tool_calls":[{"id":"call1","type":"function","function":{"name":"fixture_read","arguments":"{}"}}]})
    }
    fn tools() -> Vec<Value> {
        vec![
            json!({"type":"function","function":{"name":"fixture_read","parameters":{"type":"object","properties":{}}}}),
        ]
    }
    fn cfg<'a>(step: &'a StepFn, execute: &'a ExecFn) -> ToolLoopCfg<'a> {
        ToolLoopCfg {
            tools: tools(),
            step,
            execute,
            vision: false,
            sink: None,
            forbidden: None,
            cancelled: None,
            stream: None,
            preamble: None,
            max_iters: None,
            inbox: None,
            history: vec![],
        }
    }
    fn execute() -> Box<ExecFn> {
        Box::new(|_, _| Box::pin(async { (true, "actual fixture tool result".into()) }))
    }

    #[tokio::test]
    async fn local_completed_generations_preserve_actual_records_and_identity() {
        let fixture = Fixture::new();
        let mut spec = fixture.spec("completed");
        spec.model = Some("fixture-model".into());
        spec.effort = Some("high".into());
        let calls = Arc::new(AtomicUsize::new(0));
        let execute = execute();
        let step = scripted(
            vec![Ok(call_message()), Ok(final_message("finished"))],
            calls.clone(),
            journal_path(&spec),
        );
        let out = run_local_job(
            Some(&spec),
            "system",
            "user",
            cfg(step.as_ref(), execute.as_ref()),
        )
        .await
        .unwrap();
        assert_eq!(out.records.len(), 1);
        assert_eq!(out.records[0].summary, "actual fixture tool result");
        let first = load(&spec);
        assert_eq!(first.model, spec.model);
        assert_eq!(first.effort, spec.effort);
        assert_eq!(first.result.status, AgentJobStatus::Completed);
        assert!(first.terminal_confirmed);
        assert!(first.thread_id.is_none() && first.turn_id.is_none());
        let evidence = std::fs::read_to_string(&first.result.evidence[0]).unwrap();
        let parsed: Value = serde_json::from_str(&evidence).unwrap();
        assert_eq!(
            parsed["outcome"]["records"],
            json!([{"name":"fixture_read","ok":true,"summary":"actual fixture tool result"}])
        );
        spec.resume_completed = true;
        let restored = run_local_job(
            Some(&spec),
            "system",
            "user",
            cfg(step.as_ref(), execute.as_ref()),
        )
        .await
        .unwrap();
        assert_eq!(restored.records, out.records);
        assert_eq!(calls.load(Ordering::SeqCst), 2);
        assert_eq!(load(&spec).generation, 1);
        spec.resume_completed = false;
        let next = scripted(
            vec![Ok(final_message("next generation"))],
            calls.clone(),
            journal_path(&spec),
        );
        run_local_job(
            Some(&spec),
            "system",
            "user",
            cfg(next.as_ref(), execute.as_ref()),
        )
        .await
        .unwrap();
        let second = load(&spec);
        assert_eq!(second.generation, 2);
        assert_ne!(first.result.evidence, second.result.evidence);
        assert_eq!(
            std::fs::read_to_string(&first.result.evidence[0]).unwrap(),
            evidence
        );
        for changed in ["metadata", "cwd", "tools"] {
            let mut wrong = spec.clone();
            let mut config = cfg(next.as_ref(), execute.as_ref());
            match changed {
                "metadata" => wrong.metadata.revision = Some(3),
                "cwd" => wrong.cwd = fixture.0.join("other"),
                _ => config.tools.clear(),
            }
            let error = run_local_job(Some(&wrong), "system", "user", config)
                .await
                .err()
                .unwrap();
            assert!(error.to_string().contains("IDENTITY_MISMATCH"));
        }
        assert_eq!(calls.load(Ordering::SeqCst), 3);
        assert_eq!(load(&spec).generation, 2);
    }

    #[tokio::test]
    async fn local_failed_cancelled_and_exhausted_are_true_terminal_states() {
        let fixture = Fixture::new();
        let execute = execute();
        let failed = fixture.spec("failed");
        let calls = Arc::new(AtomicUsize::new(0));
        let step = scripted(
            vec![Err("HTTP 400: FIXTURE_NON_TRANSIENT_FAILURE")],
            calls.clone(),
            journal_path(&failed),
        );
        assert!(run_local_job(
            Some(&failed),
            "s",
            "u",
            cfg(step.as_ref(), execute.as_ref())
        )
        .await
        .is_err());
        let record = load(&failed);
        assert_eq!(record.result.status, AgentJobStatus::Failed);
        assert!(record.terminal_confirmed);
        assert!(record
            .result
            .error
            .unwrap()
            .contains("FIXTURE_NON_TRANSIENT_FAILURE"));
        assert!(run_local_job(
            Some(&failed),
            "s",
            "u",
            cfg(step.as_ref(), execute.as_ref())
        )
        .await
        .err()
        .unwrap()
        .to_string()
        .contains("RECOVERY_REQUIRED"));
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        let mut cancelled = fixture.spec("cancelled");
        cancelled.cancelled = Arc::new(|| true);
        let no_calls = Arc::new(AtomicUsize::new(0));
        let cancel_step = scripted(vec![], no_calls.clone(), None);
        let out = run_local_job(
            Some(&cancelled),
            "s",
            "u",
            cfg(cancel_step.as_ref(), execute.as_ref()),
        )
        .await
        .unwrap();
        assert!(out.cancelled);
        assert_eq!(no_calls.load(Ordering::SeqCst), 0);
        assert_eq!(load(&cancelled).result.status, AgentJobStatus::Cancelled);
        assert!(load(&cancelled).terminal_confirmed);
        let exhausted = fixture.spec("exhausted");
        let step = scripted(
            vec![Ok(call_message())],
            Arc::new(AtomicUsize::new(0)),
            journal_path(&exhausted),
        );
        let mut config = cfg(step.as_ref(), execute.as_ref());
        config.max_iters = Some(1);
        assert!(run_local_job(Some(&exhausted), "s", "u", config)
            .await
            .err()
            .unwrap()
            .to_string()
            .contains("INCOMPLETE"));
        let record = load(&exhausted);
        assert_eq!(record.result.status, AgentJobStatus::Failed);
        let evidence: Value =
            serde_json::from_str(&std::fs::read_to_string(&record.result.evidence[0]).unwrap())
                .unwrap();
        assert_eq!(evidence["outcome"]["exhausted"], true);
        assert_eq!(evidence["outcome"]["records"].as_array().unwrap().len(), 1);
        let stateless = AgentJobSpec::new("stateless", fixture.0.clone());
        let step = scripted(
            vec![
                Ok(final_message("ordinary loop")),
                Ok(final_message("ordinary loop")),
            ],
            Arc::new(AtomicUsize::new(0)),
            None,
        );
        run_local_job(
            Some(&stateless),
            "s",
            "u",
            cfg(step.as_ref(), execute.as_ref()),
        )
        .await
        .unwrap();
        run_local_job(None, "s", "u", cfg(step.as_ref(), execute.as_ref()))
            .await
            .unwrap();
        assert!(journal_path(&stateless).is_none());
    }

    #[tokio::test]
    async fn local_crash_requires_a_second_explicit_retry_without_replaying() {
        let fixture = Fixture::new();
        let execute = execute();
        for status in [AgentJobStatus::Starting, AgentJobStatus::Running] {
            let mut spec = fixture.spec(&format!("crashed-{status:?}"));
            spec.retry_failed = true;
            let mut old =
                AgentJobRecord::starting(&spec, "local", local_tool_signature(&tools()).unwrap());
            old.generation = 7;
            old.result.status = status;
            persist_local_record(&journal_path(&spec).unwrap(), &old).unwrap();
            let calls = Arc::new(AtomicUsize::new(0));
            let step = scripted(
                vec![Ok(final_message("explicit retry completed"))],
                calls.clone(),
                journal_path(&spec),
            );
            assert!(
                run_local_job(Some(&spec), "s", "u", cfg(step.as_ref(), execute.as_ref()))
                    .await
                    .err()
                    .unwrap()
                    .to_string()
                    .contains("RECOVERY_REQUIRED")
            );
            assert_eq!(load(&spec).result.status, AgentJobStatus::RecoveryRequired);
            assert_eq!(load(&spec).generation, 7);
            assert!(!load(&spec).terminal_confirmed);
            assert_eq!(calls.load(Ordering::SeqCst), 0);
            spec.retry_failed = false;
            assert!(
                run_local_job(Some(&spec), "s", "u", cfg(step.as_ref(), execute.as_ref()))
                    .await
                    .is_err()
            );
            assert_eq!(calls.load(Ordering::SeqCst), 0);
            spec.retry_failed = true;
            run_local_job(Some(&spec), "s", "u", cfg(step.as_ref(), execute.as_ref()))
                .await
                .unwrap();
            assert_eq!(load(&spec).generation, 8);
            assert_eq!(load(&spec).result.status, AgentJobStatus::Completed);
            assert_eq!(calls.load(Ordering::SeqCst), 1);
        }
    }
}
