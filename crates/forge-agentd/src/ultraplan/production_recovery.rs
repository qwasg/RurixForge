//! Reconcile a durable submission with its exact provider job before host checks.
use super::{read_pending_submission, read_task_result, save_task_result, ProductionTaskResult};
use crate::agent_job::{AgentJobRecord, AgentJobStatus};
use crate::codex::rpc::CodexClient;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

#[derive(Debug)]
pub enum PendingHandoffRecovery {
    /// The provider finished; this is still only a request for host verification.
    Verify(ProductionTaskResult),
    /// A confirmed stop/failure (or missing submission) is already persisted.
    Terminal(ProductionTaskResult),
}

fn recovery_required(message: impl std::fmt::Display) -> String {
    format!("UP_RECOVERY_REQUIRED: {message}")
}

fn job_path(dir: &Path, job_id: &str) -> PathBuf {
    dir.join("jobs").join(format!(
        "{}.json",
        forge_util::hashutil::sha256_hex(job_id.as_bytes())
    ))
}

fn confirmed_terminal(record: &AgentJobRecord) -> bool {
    record.terminal_confirmed
        && matches!(
            record.result.status,
            AgentJobStatus::Completed | AgentJobStatus::Cancelled | AgentJobStatus::Failed
        )
}

/// No worker is started or replayed. A supplied, already running Codex client
/// may read the original thread; no thread/start, thread/resume or turn/start is
/// sent. Without exact terminal evidence, retain the active attempt as unknown.
/// Existing final results are left to the scheduler's receipt reconciliation.
pub async fn recover_pending_handoff(
    dir: &Path,
    flow_id: &str,
    plan_rev: u64,
    task_id: &str,
    attempt: u32,
    codex_client: Option<&CodexClient>,
) -> Result<Option<PendingHandoffRecovery>, String> {
    if read_task_result(dir, task_id, attempt)?.is_some() {
        return Ok(None);
    }
    if attempt == 0 {
        return Err(recovery_required("无有效任务 attempt"));
    }
    let expected_id = format!("{flow_id}-p{plan_rev}-{task_id}-attempt-{attempt}");
    let path = job_path(dir, &expected_id);
    let original = std::fs::read(&path)
        .map_err(|e| recovery_required(format!("无法读取原任务账本 {}: {e}", path.display())))?;
    let mut record: AgentJobRecord =
        serde_json::from_slice(&original).map_err(recovery_required)?;
    if record.schema_version != 1
        || record.generation == 0
        || record.job_id != expected_id
        || record.metadata.flow_id.as_deref() != Some(flow_id)
        || record.metadata.revision != Some(plan_rev)
        || record.metadata.phase.as_deref() != Some(crate::ultraplan::RUNNING_PRODUCTION)
        || !matches!(record.engine.as_str(), "codex" | "local")
    {
        return Err(recovery_required(
            "原任务账本与当前流程、计划版本或 attempt 不匹配",
        ));
    }
    if record.engine == "codex"
        && (record.thread_id.as_deref().is_none_or(str::is_empty)
            || record.turn_id.as_deref().is_none_or(str::is_empty))
    {
        return Err(recovery_required(
            "原 Codex 工作缺少精确 thread/turn，禁止猜测终态",
        ));
    }
    if !confirmed_terminal(&record) {
        let client = codex_client
            .filter(|client| record.engine == "codex" && client.running())
            .ok_or_else(|| {
                recovery_required("原任务终态尚未确认，保留产物与待验证申请，未派发新任务")
            })?;
        let thread = record.thread_id.as_deref().unwrap();
        let turn = record.turn_id.as_deref().unwrap();
        let response = client
            .request_with_timeout(
                "thread/read",
                json!({"threadId":thread,"includeTurns":true}),
                std::time::Duration::from_secs(10),
            )
            .await
            .map_err(recovery_required)?;
        if response.pointer("/thread/id").and_then(Value::as_str) != Some(thread) {
            return Err(recovery_required("thread/read 返回其他线程，未采用其终态"));
        }
        let saved = response
            .pointer("/thread/turns")
            .and_then(Value::as_array)
            .and_then(|turns| turns.iter().find(|value| value["id"] == turn))
            .ok_or_else(|| recovery_required("thread/read 未包含原 turn，未采用其他轮次的终态"))?;
        let status = saved["status"].as_str().unwrap_or_default();
        record.result.status = match status {
            "completed" => AgentJobStatus::Completed,
            "interrupted" => AgentJobStatus::Cancelled,
            "failed" => AgentJobStatus::Failed,
            _ => {
                return Err(recovery_required(format!(
                    "原 turn 仍为 {status:?}，禁止派发替代任务"
                )))
            }
        };
        record.terminal_confirmed = true;
        record.result.error = match status {
            "completed" => None,
            "interrupted" => Some("CODEX_JOB_CANCELLED: 原 turn 的 interrupted 终态已确认".into()),
            _ => Some("CODEX_JOB_FAILED: 原 turn 的 failed 终态已确认".into()),
        };
        // Preserve a compact, host-owned proof without copying private messages.
        let evidence = dir
            .join("recovery-evidence")
            .join(forge_util::hashutil::sha256_hex(expected_id.as_bytes()))
            .join(format!("g{}.json", record.generation));
        let proof = json!({"jobId":expected_id,"generation":record.generation,"method":"thread/read",
            "threadId":thread,"turnId":turn,"status":status,"terminalConfirmed":true,"at":crate::events::now_rfc3339()});
        if std::fs::read(&path).map_err(recovery_required)? != original {
            return Err(recovery_required("读取终态期间任务账本已变化，请重新核对"));
        }
        crate::ultraplan::write_json_atomic(&evidence, &proof).map_err(recovery_required)?;
        record
            .result
            .evidence
            .push(evidence.to_string_lossy().into_owned());
        crate::ultraplan::write_json_atomic(
            &path,
            &serde_json::to_value(&record).map_err(recovery_required)?,
        )
        .map_err(recovery_required)?;
    }
    // Another recovery may already have finished verification while reading RPC.
    if read_task_result(dir, task_id, attempt)?.is_some() {
        return Ok(None);
    }
    let result = match record.result.status {
        AgentJobStatus::Completed => match read_pending_submission(dir, task_id, attempt)? {
            Some(pending) => return Ok(Some(PendingHandoffRecovery::Verify(pending))),
            None => ProductionTaskResult::incomplete(
                "UP_RESULT_MISSING",
                "原模型轮次已确认完成，但没有持久化的结构化任务结果；未重新派工",
            ),
        },
        AgentJobStatus::Cancelled => ProductionTaskResult::incomplete(
            "UP_TASK_CANCELLED",
            format!(
                "原任务已确认取消；待验证申请不能当作成功，不重新派工。{}",
                record.result.error.as_deref().unwrap_or_default()
            ),
        ),
        AgentJobStatus::Failed => ProductionTaskResult::incomplete(
            "UP_TASK_FAILED",
            format!(
                "原任务已确认失败；待验证申请不能当作成功，不重新派工。{}",
                record.result.error.as_deref().unwrap_or_default()
            ),
        ),
        _ => return Err(recovery_required("任务终态尚未确认")),
    };
    save_task_result(dir, task_id, attempt, &result)?;
    Ok(Some(PendingHandoffRecovery::Terminal(result)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent_job::{AgentJobMetadata, AgentJobSpec};
    use std::sync::{Arc, Mutex};

    fn fixture(engine: &str, status: AgentJobStatus, confirmed: bool) -> (PathBuf, AgentJobRecord) {
        let dir = std::env::temp_dir().join(crate::events::new_id("up-pending-recovery"));
        let mut spec = AgentJobSpec::new("flow-p3-todo-1-attempt-1", dir.clone());
        spec.metadata = AgentJobMetadata {
            flow_id: Some("flow".into()),
            revision: Some(3),
            phase: Some("production".into()),
            role: Some("logic-programmer".into()),
            ..Default::default()
        };
        let mut record = AgentJobRecord::starting(&spec, engine, "fixture".into());
        record.result.status = status;
        record.terminal_confirmed = confirmed;
        if engine == "codex" {
            record.thread_id = Some("original-thread".into());
            record.turn_id = Some("original-turn".into());
        }
        crate::ultraplan::write_json_atomic(
            &job_path(&dir, &record.job_id),
            &serde_json::to_value(&record).unwrap(),
        )
        .unwrap();
        let pending=super::super::submission(&json!({"status":"completed","summary":"ready for checks","outputs":["output.txt"],"evidence":[]})).unwrap();
        super::super::save_pending_submission(&dir, "todo-1", 1, &pending).unwrap();
        (dir, record)
    }

    #[tokio::test]
    async fn confirmed_provider_completion_restores_pending_without_issuing_receipt() {
        for engine in ["local", "codex"] {
            let (dir, _) = fixture(engine, AgentJobStatus::Completed, true);
            let recovered = recover_pending_handoff(&dir, "flow", 3, "todo-1", 1, None)
                .await
                .unwrap();
            let Some(PendingHandoffRecovery::Verify(result)) = recovered else {
                panic!("must request host verification")
            };
            assert!(result.receipt.is_none());
            assert_eq!(result.outputs, vec!["output.txt"]);
            assert!(read_task_result(&dir, "todo-1", 1).unwrap().is_none());
            std::fs::remove_dir_all(dir).unwrap();
        }
    }

    #[tokio::test]
    async fn confirmed_stops_override_completed_pending_and_are_durable() {
        for (status, reason) in [
            (AgentJobStatus::Cancelled, "UP_TASK_CANCELLED"),
            (AgentJobStatus::Failed, "UP_TASK_FAILED"),
        ] {
            let (dir, _) = fixture("local", status, true);
            let Some(PendingHandoffRecovery::Terminal(result)) =
                recover_pending_handoff(&dir, "flow", 3, "todo-1", 1, None)
                    .await
                    .unwrap()
            else {
                panic!("must persist terminal failure")
            };
            assert_eq!(result.reason_code, reason);
            assert_eq!(result.status, super::super::ProductionTaskStatus::Blocked);
            assert!(result.receipt.is_none());
            assert_eq!(
                read_task_result(&dir, "todo-1", 1)
                    .unwrap()
                    .unwrap()
                    .reason_code,
                reason
            );
            assert!(recover_pending_handoff(&dir, "flow", 3, "todo-1", 1, None)
                .await
                .unwrap()
                .is_none());
            assert!(super::super::read_pending_submission(&dir, "todo-1", 1)
                .unwrap()
                .is_some());
            std::fs::remove_dir_all(dir).unwrap();
        }
    }

    #[tokio::test]
    async fn completed_provider_without_submission_persists_missing_result_failure() {
        let (dir, _) = fixture("local", AgentJobStatus::Completed, true);
        std::fs::remove_file(super::super::pending_path(&dir, "todo-1", 1)).unwrap();
        let Some(PendingHandoffRecovery::Terminal(result)) =
            recover_pending_handoff(&dir, "flow", 3, "todo-1", 1, None)
                .await
                .unwrap()
        else {
            panic!("provider completion alone must not complete a task")
        };
        assert_eq!(result.reason_code, "UP_RESULT_MISSING");
        assert!(result.receipt.is_none());
        assert_eq!(
            read_task_result(&dir, "todo-1", 1)
                .unwrap()
                .unwrap()
                .reason_code,
            "UP_RESULT_MISSING"
        );
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[tokio::test]
    async fn pending_and_unconfirmed_completed_are_not_terminal_evidence() {
        for (engine, status) in [
            ("local", AgentJobStatus::Completed),
            ("local", AgentJobStatus::Running),
            ("local", AgentJobStatus::Cancelled),
            ("codex", AgentJobStatus::Completed),
            ("codex", AgentJobStatus::Running),
            ("codex", AgentJobStatus::Cancelled),
        ] {
            let (dir, record) = fixture(engine, status, false);
            let before = std::fs::read(job_path(&dir, &record.job_id)).unwrap();
            assert!(recover_pending_handoff(&dir, "flow", 3, "todo-1", 1, None)
                .await
                .unwrap_err()
                .starts_with("UP_RECOVERY_REQUIRED"));
            assert!(read_task_result(&dir, "todo-1", 1).unwrap().is_none());
            assert_eq!(
                std::fs::read(job_path(&dir, &record.job_id)).unwrap(),
                before
            );
            assert!(recover_pending_handoff(&dir, "flow", 3, "todo-1", 2, None)
                .await
                .is_err());
            std::fs::remove_dir_all(dir).unwrap();
        }
    }

    #[tokio::test]
    async fn read_original_turn_can_confirm_terminal_without_starting_a_turn() {
        for (returned_thread, returned_turn, status, success) in [
            ("original-thread", "original-turn", "completed", true),
            ("original-thread", "original-turn", "interrupted", true),
            ("wrong-thread", "original-turn", "completed", false),
            ("original-thread", "wrong-turn", "completed", false),
            ("original-thread", "original-turn", "inProgress", false),
        ] {
            let (dir, record) = fixture("codex", AgentJobStatus::Running, false);
            let methods = Arc::new(Mutex::new(Vec::new()));
            let captured = methods.clone();
            let transport = Arc::new(crate::codex::rpc::ScriptedTransport::new(move |message| {
                let method = message["method"].as_str().unwrap();
                captured.lock().unwrap().push(method.to_string());
                match method {
                    "initialize" => vec![json!({"id":message["id"],"result":{}})],
                    "initialized" => vec![],
                    "thread/read" => {
                        assert_eq!(message["params"]["threadId"], "original-thread");
                        vec![
                            json!({"id":message["id"],"result":{"thread":{"id":returned_thread,"turns":[{"id":returned_turn,"status":status}]}}}),
                        ]
                    }
                    _ => panic!("recovery must not start or resume a task: {method}"),
                }
            }));
            let client = CodexClient::new(transport);
            client.ensure_started().await.unwrap();
            let recovered =
                recover_pending_handoff(&dir, "flow", 3, "todo-1", 1, Some(&client)).await;
            assert_eq!(
                recovered.is_ok(),
                success,
                "{returned_thread}/{returned_turn}/{status}"
            );
            assert_eq!(
                *methods.lock().unwrap(),
                vec!["initialize", "initialized", "thread/read"]
            );
            let saved: AgentJobRecord =
                serde_json::from_slice(&std::fs::read(job_path(&dir, &record.job_id)).unwrap())
                    .unwrap();
            assert_eq!(saved.terminal_confirmed, success);
            if success && status == "completed" {
                assert!(matches!(
                    recovered.unwrap(),
                    Some(PendingHandoffRecovery::Verify(_))
                ));
            }
            if success && status == "interrupted" {
                assert_eq!(
                    read_task_result(&dir, "todo-1", 1)
                        .unwrap()
                        .unwrap()
                        .reason_code,
                    "UP_TASK_CANCELLED"
                );
            }
            if !success {
                assert!(read_task_result(&dir, "todo-1", 1).unwrap().is_none());
            }
            std::fs::remove_dir_all(dir).unwrap();
        }
    }

    #[tokio::test]
    async fn malformed_job_identity_never_reuses_a_submission() {
        let (dir, mut record) = fixture("local", AgentJobStatus::Completed, true);
        record.metadata.revision = Some(4);
        crate::ultraplan::write_json_atomic(
            &job_path(&dir, &record.job_id),
            &serde_json::to_value(&record).unwrap(),
        )
        .unwrap();
        assert!(recover_pending_handoff(&dir, "flow", 3, "todo-1", 1, None)
            .await
            .is_err());
        assert!(read_task_result(&dir, "todo-1", 1).unwrap().is_none());
        std::fs::remove_dir_all(dir).unwrap();
    }
}
