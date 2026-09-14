//! 会话工具权限：bypass / plan / auto + 规则（参考仓 permission.rs 语义改接）。

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use tokio::sync::oneshot;

use crate::events::{new_id, EventDraft};
use crate::AppState;

const DEFAULT_MODE: &str = "bypass";
const APPROVAL_TIMEOUT_SECS: u64 = 60;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PermissionRule {
    pub action: String,
    pub pattern: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct SessionPerm {
    #[serde(default = "default_mode")]
    mode: String,
    #[serde(default)]
    rules: Vec<PermissionRule>,
}

fn default_mode() -> String {
    DEFAULT_MODE.to_string()
}

/// 待答审批。
///
/// 两种形态并存的原因:本地引擎只需要「放行 / 拒绝」一个布尔;Codex 的审批是**有档次的**
/// —— accept(这一次)/ acceptForSession(本会话都行)/ decline,`item/tool/requestUserInput`
/// 还要带回一份答卷。把 Codex 那套硬塞进 bool 会把「本会话都行」降级成「就这一次」,
/// 于是保留两种 sender,由回答端(REST)按实际待答形态决定送什么。
enum Pending {
    Bool(oneshot::Sender<bool>),
    Decision(oneshot::Sender<Value>),
}

struct PendingEntry {
    run_id: String,
    upstream_request_id: Option<String>,
    sender: Pending,
}

pub struct PermissionService {
    path: PathBuf,
    inner: Mutex<HashMap<String, SessionPerm>>,
    pending: Mutex<HashMap<String, PendingEntry>>,
}

impl PermissionService {
    pub fn load(path: PathBuf) -> Self {
        let map = read_file(&path);
        PermissionService {
            path,
            inner: Mutex::new(map),
            pending: Mutex::new(HashMap::new()),
        }
    }

    fn persist_locked(&self, inner: &HashMap<String, SessionPerm>) {
        let doc = json!({ "sessions": inner });
        if let Ok(text) = serde_json::to_string_pretty(&doc) {
            if let Some(parent) = self.path.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            let tmp = self.path.with_extension("json.tmp");
            if std::fs::write(&tmp, text).is_ok() {
                if let Err(e) = replace_persisted_file(&tmp, &self.path) {
                    eprintln!("[permission] 持久化失败: {e}");
                }
            }
        }
    }

    pub fn mode(&self, session_id: &str) -> String {
        self.inner
            .lock()
            .unwrap()
            .get(session_id)
            .map(|s| s.mode.clone())
            .unwrap_or_else(|| DEFAULT_MODE.to_string())
    }

    pub fn set_mode(&self, session_id: &str, mode: &str) -> Result<String, String> {
        if !matches!(mode, "auto" | "plan" | "bypass") {
            return Err("invalid permission mode".into());
        }
        let mut inner = self.inner.lock().unwrap();
        let entry = inner.entry(session_id.to_string()).or_default();
        entry.mode = mode.to_string();
        self.persist_locked(&inner);
        Ok(mode.to_string())
    }

    pub fn snapshot(&self, session_id: &str) -> Value {
        let inner = self.inner.lock().unwrap();
        let p = inner.get(session_id).cloned().unwrap_or_default();
        json!({ "mode": p.mode, "rules": p.rules })
    }

    /// 写工具在 plan 下拒绝；auto 下走审批。返回 Ok(true) 放行 / Ok(false) 拒绝。
    pub async fn authorize(
        &self,
        bus: &crate::events::EventBus,
        session_id: &str,
        run_id: &str,
        tool: &str,
        is_write: bool,
    ) -> Result<bool, String> {
        self.authorize_with(bus, session_id, run_id, tool, is_write, json!({}))
            .await
    }

    /// 带目标项目 / 参数摘要的审批(素材创作与普通聊天共用)。
    pub async fn authorize_with(
        &self,
        bus: &crate::events::EventBus,
        session_id: &str,
        run_id: &str,
        tool: &str,
        is_write: bool,
        extra: Value,
    ) -> Result<bool, String> {
        self.authorize_with_timeout(
            bus,
            session_id,
            run_id,
            tool,
            is_write,
            extra,
            std::time::Duration::from_secs(APPROVAL_TIMEOUT_SECS),
        )
        .await
    }

    async fn authorize_with_timeout(
        &self,
        bus: &crate::events::EventBus,
        session_id: &str,
        run_id: &str,
        tool: &str,
        is_write: bool,
        extra: Value,
        approval_timeout: std::time::Duration,
    ) -> Result<bool, String> {
        let mode = self.mode(session_id);
        if mode == "plan" && is_write {
            return Ok(false);
        }
        if mode != "auto" || !is_write {
            return Ok(true);
        }
        let req_id = new_id("perm");
        let (tx, rx) = oneshot::channel();
        self.pending.lock().unwrap().insert(
            req_id.clone(),
            PendingEntry {
                run_id: run_id.to_string(),
                upstream_request_id: None,
                sender: Pending::Bool(tx),
            },
        );
        let mut payload = json!({
            "id": req_id,
            "runId": run_id,
            "tool": tool,
        });
        if let Some(obj) = extra.as_object() {
            if let Some(p) = payload.as_object_mut() {
                for (k, v) in obj {
                    p.insert(k.clone(), v.clone());
                }
            }
        }
        bus.emit(EventDraft::new(session_id, "permission.requested", "agent").payload(payload));
        let ok = match tokio::time::timeout(approval_timeout, rx).await {
            Ok(result) => result.unwrap_or(false),
            Err(_) => {
                // `timeout` drops the receiver. Remove its sender as well; otherwise a
                // later click removes a dead entry and incorrectly reports ok:true,
                // while no waiter remains to emit permission.resolved for the UI.
                self.pending.lock().unwrap().remove(&req_id);
                bus.emit(
                    EventDraft::new(session_id, "permission.resolved", "agent").payload(json!({
                        "id": req_id,
                        "runId": run_id,
                        "tool": tool,
                        "allowed": false,
                        "decision": "decline",
                        "reason": "approval_timeout",
                        "error": "PERMISSION_TIMEOUT",
                    })),
                );
                return Err("PERMISSION_TIMEOUT".to_string());
            }
        };
        bus.emit(
            EventDraft::new(session_id, "permission.resolved", "agent").payload(json!({
                "id": req_id,
                "runId": run_id,
                "tool": tool,
                "allowed": ok,
            })),
        );
        Ok(ok)
    }

    /// Codex 侧审批:发 `permission.requested` 后等前端回话,返回决定原文。
    ///
    /// 与 [`Self::authorize_with`] 的关键差别是**不设超时**:那边 60s 到点自动判失败是
    /// 因为本地工具循环还有后续步骤要跑;Codex 这边一个待批命令就是整轮的堵点,
    /// 到点自动拒绝只会让用户看到一轮莫名失败的 turn。turn 挂着等人是对的,
    /// 用户不想批就点停止(走 `turn/interrupt`)。
    pub async fn request_decision(
        &self,
        bus: &crate::events::EventBus,
        session_id: &str,
        run_id: &str,
        kind: &str,
        extra: Value,
        upstream_request_id: &Value,
    ) -> Result<Value, String> {
        let req_id = new_id("perm");
        let (tx, rx) = oneshot::channel();
        self.pending.lock().unwrap().insert(
            req_id.clone(),
            PendingEntry {
                run_id: run_id.to_string(),
                upstream_request_id: Some(request_id_key(upstream_request_id)),
                sender: Pending::Decision(tx),
            },
        );
        let mut payload = json!({
            "id": req_id,
            "runId": run_id,
            "kind": kind,
            "engine": "codex",
        });
        if let (Some(obj), Some(p)) = (extra.as_object(), payload.as_object_mut()) {
            for (k, v) in obj {
                p.insert(k.clone(), v.clone());
            }
        }
        bus.emit(EventDraft::new(session_id, "permission.requested", "agent").payload(payload));
        let decision = rx
            .await
            .map_err(|_| "PERMISSION_ABANDONED: 审批通道已关闭".to_string())?;
        bus.emit(
            EventDraft::new(session_id, "permission.resolved", "agent").payload(json!({
                "id": req_id,
                "runId": run_id,
                "kind": kind,
                "decision": decision.get("decision").cloned().unwrap_or(Value::Null),
                "allowed": is_allow(&decision),
            })),
        );
        Ok(decision)
    }

    pub fn resolve(&self, id: &str, allow: bool) -> bool {
        self.resolve_with(
            id,
            json!({ "decision": if allow { "accept" } else { "decline" } }),
        )
    }

    /// 带决定原文的回话(Codex 的 acceptForSession / 答卷经此路)。
    pub fn resolve_with(&self, id: &str, decision: Value) -> bool {
        match self.pending.lock().unwrap().remove(id) {
            Some(PendingEntry {
                sender: Pending::Decision(tx),
                ..
            }) => tx.send(decision).is_ok(),
            // 本地引擎那条腿只认布尔:任何非 decline 的决定都当放行。
            Some(PendingEntry {
                sender: Pending::Bool(tx),
                ..
            }) => tx.send(is_allow(&decision)).is_ok(),
            None => false,
        }
    }

    /// run 已结束/中止时统一拒绝仍悬挂的审批，释放 Codex server-request 回话任务。
    /// request_decision/authorize_with 收到值后会走原有路径发 permission.resolved。
    pub fn abandon_run(&self, run_id: &str) -> usize {
        let mut pending = self.pending.lock().unwrap();
        let ids = pending
            .iter()
            .filter_map(|(id, entry)| (entry.run_id == run_id).then(|| id.clone()))
            .collect::<Vec<_>>();
        for id in &ids {
            let Some(entry) = pending.remove(id) else {
                continue;
            };
            match entry.sender {
                Pending::Bool(tx) => {
                    let _ = tx.send(false);
                }
                Pending::Decision(tx) => {
                    let _ = tx.send(json!({ "decision": "decline", "abandoned": true }));
                }
            }
        }
        ids.len()
    }

    /// app-server can resolve an approval from another client. Release the matching
    /// Forge waiter immediately so the inline card does not remain actionable until
    /// the whole turn ends.
    pub fn abandon_upstream(&self, upstream_request_id: &Value) -> usize {
        let key = request_id_key(upstream_request_id);
        let mut pending = self.pending.lock().unwrap();
        let ids = pending
            .iter()
            .filter_map(|(id, entry)| {
                (entry.upstream_request_id.as_deref() == Some(key.as_str())).then(|| id.clone())
            })
            .collect::<Vec<_>>();
        for id in &ids {
            let Some(entry) = pending.remove(id) else {
                continue;
            };
            match entry.sender {
                Pending::Bool(tx) => {
                    let _ = tx.send(false);
                }
                Pending::Decision(tx) => {
                    let _ = tx.send(json!({ "decision": "decline", "abandoned": true }));
                }
            }
        }
        ids.len()
    }
}

fn request_id_key(value: &Value) -> String {
    serde_json::to_string(value).unwrap_or_else(|_| "null".to_string())
}

fn replace_persisted_file(
    tmp: &std::path::Path,
    destination: &std::path::Path,
) -> std::io::Result<()> {
    if !destination.exists() {
        return std::fs::rename(tmp, destination);
    }
    let backup = destination.with_extension("json.bak");
    if backup.exists() {
        std::fs::remove_file(&backup)?;
    }
    std::fs::rename(destination, &backup)?;
    if let Err(error) = std::fs::rename(tmp, destination) {
        let _ = std::fs::rename(&backup, destination);
        return Err(error);
    }
    let _ = std::fs::remove_file(backup);
    Ok(())
}

/// 决定是否为放行。缺省(没写 decision)按放行——这条路径只由「点了允许」的按钮走到。
fn is_allow(decision: &Value) -> bool {
    match decision.get("decision").and_then(Value::as_str) {
        Some("decline") | Some("deny") | Some("reject") => false,
        _ => true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mode_roundtrip_and_reject_invalid() {
        let dir = std::env::temp_dir().join(format!(
            "forge-perm-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let _ = std::fs::create_dir_all(&dir);
        let svc = PermissionService::load(dir.join("permissions.json"));
        assert_eq!(svc.mode("s1"), "bypass");
        assert_eq!(svc.set_mode("s1", "auto").unwrap(), "auto");
        assert_eq!(svc.mode("s1"), "auto");
        assert_eq!(svc.set_mode("s1", "plan").unwrap(), "plan");
        let reloaded = PermissionService::load(dir.join("permissions.json"));
        assert_eq!(reloaded.mode("s1"), "plan", "第二次覆盖写也必须持久化");
        assert!(svc.set_mode("s1", "nope").is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn auto_read_skips_approval_event() {
        let dir = std::env::temp_dir().join(format!(
            "forge-perm-read-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let _ = std::fs::create_dir_all(&dir);
        let bus = crate::events::EventBus::new(dir.join("ev"), 16);
        let svc = PermissionService::load(dir.join("permissions.json"));
        svc.set_mode("s1", "auto").unwrap();
        let ok = svc
            .authorize_with(&bus, "s1", "run_1", "resource_search", false, json!({}))
            .await
            .unwrap();
        assert!(ok);
        assert!(!bus
            .persisted("s1")
            .iter()
            .any(|e| e.event_type == "permission.requested"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn upstream_resolution_releases_pending_codex_decision() {
        let dir = std::env::temp_dir().join(format!(
            "forge-perm-abandon-{}-{}",
            std::process::id(),
            gend::timeutil::unix_millis()
        ));
        let svc = PermissionService::load(dir.join("permissions.json"));
        let (tx, rx) = oneshot::channel();
        svc.pending.lock().unwrap().insert(
            "perm_1".into(),
            PendingEntry {
                run_id: "run_1".into(),
                upstream_request_id: Some(request_id_key(&json!("up_1"))),
                sender: Pending::Decision(tx),
            },
        );
        assert_eq!(svc.abandon_upstream(&json!("up_1")), 1);
        let decision = rx.await.unwrap();
        assert_eq!(decision["decision"], "decline");
        assert_eq!(decision["abandoned"], true);
        assert_eq!(svc.abandon_run("run_1"), 0);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[tokio::test]
    async fn abandon_run_immediately_rejects_local_and_codex_waiters() {
        let dir = std::env::temp_dir().join(format!(
            "forge-perm-cancel-{}-{}",
            std::process::id(),
            gend::timeutil::unix_millis()
        ));
        let svc = PermissionService::load(dir.join("permissions.json"));
        let (bool_tx, bool_rx) = oneshot::channel();
        let (decision_tx, decision_rx) = oneshot::channel();
        {
            let mut pending = svc.pending.lock().unwrap();
            pending.insert(
                "perm_bool".into(),
                PendingEntry {
                    run_id: "run_1".into(),
                    upstream_request_id: None,
                    sender: Pending::Bool(bool_tx),
                },
            );
            pending.insert(
                "perm_decision".into(),
                PendingEntry {
                    run_id: "run_1".into(),
                    upstream_request_id: Some(request_id_key(&json!("up_1"))),
                    sender: Pending::Decision(decision_tx),
                },
            );
        }

        assert_eq!(svc.abandon_run("run_1"), 2);
        assert!(!bool_rx.await.unwrap());
        let decision = decision_rx.await.unwrap();
        assert_eq!(decision["decision"], "decline");
        assert_eq!(decision["abandoned"], true);
        assert!(svc.pending.lock().unwrap().is_empty());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn resolution_reports_false_when_waiter_is_already_gone() {
        let dir = std::env::temp_dir().join(format!(
            "forge-perm-dead-waiter-{}-{}",
            std::process::id(),
            gend::timeutil::unix_millis()
        ));
        let svc = PermissionService::load(dir.join("permissions.json"));

        let (bool_tx, bool_rx) = oneshot::channel();
        drop(bool_rx);
        svc.pending.lock().unwrap().insert(
            "perm_bool".into(),
            PendingEntry {
                run_id: "run_1".into(),
                upstream_request_id: None,
                sender: Pending::Bool(bool_tx),
            },
        );
        assert!(!svc.resolve("perm_bool", true));

        let (decision_tx, decision_rx) = oneshot::channel();
        drop(decision_rx);
        svc.pending.lock().unwrap().insert(
            "perm_decision".into(),
            PendingEntry {
                run_id: "run_1".into(),
                upstream_request_id: Some(request_id_key(&json!("up_1"))),
                sender: Pending::Decision(decision_tx),
            },
        );
        assert!(!svc.resolve_with("perm_decision", json!({ "decision": "accept" })));
        assert!(svc.pending.lock().unwrap().is_empty());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[tokio::test]
    async fn auto_write_emits_target_and_honors_deny() {
        let dir = std::env::temp_dir().join(format!(
            "forge-perm-write-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let _ = std::fs::create_dir_all(&dir);
        let bus = std::sync::Arc::new(crate::events::EventBus::new(dir.join("ev"), 16));
        let svc = std::sync::Arc::new(PermissionService::load(dir.join("permissions.json")));
        svc.set_mode("s1", "auto").unwrap();
        let svc2 = svc.clone();
        let bus2 = bus.clone();
        let handle = tokio::spawn(async move {
            svc2.authorize_with(
                &bus2,
                "s1",
                "run_1",
                "mcp__store__store_install",
                true,
                json!({ "targetProjectId": "ws_a", "argsSummary": "{\"packageId\":\"p\"}" }),
            )
            .await
        });
        let mut req_id = None;
        for _ in 0..50 {
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
            if let Some(ev) = bus
                .persisted("s1")
                .into_iter()
                .find(|e| e.event_type == "permission.requested")
            {
                assert_eq!(ev.payload["tool"], "mcp__store__store_install");
                assert_eq!(ev.payload["targetProjectId"], "ws_a");
                req_id = ev.payload["id"].as_str().map(str::to_string);
                break;
            }
        }
        let req_id = req_id.expect("permission.requested");
        assert!(svc.resolve(&req_id, false));
        let allowed = handle.await.expect("join").expect("authorize");
        assert!(!allowed);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn approval_timeout_clears_pending_and_emits_decline_resolution() {
        let dir = std::env::temp_dir().join(format!(
            "forge-perm-timeout-{}-{}",
            std::process::id(),
            gend::timeutil::unix_millis()
        ));
        let bus = crate::events::EventBus::new(dir.join("events"), 16);
        let svc = PermissionService::load(dir.join("permissions.json"));
        svc.set_mode("s1", "auto").unwrap();

        let result = svc
            .authorize_with_timeout(
                &bus,
                "s1",
                "run_1",
                "write_file",
                true,
                json!({}),
                std::time::Duration::ZERO,
            )
            .await;

        assert_eq!(result, Err("PERMISSION_TIMEOUT".to_string()));
        assert!(svc.pending.lock().unwrap().is_empty());
        let events = bus.persisted("s1");
        let requested = events
            .iter()
            .find(|event| event.event_type == "permission.requested")
            .expect("须先发出审批请求");
        let resolved = events
            .iter()
            .find(|event| event.event_type == "permission.resolved")
            .expect("超时须收束审批卡");
        assert_eq!(resolved.payload["id"], requested.payload["id"]);
        assert_eq!(resolved.payload["allowed"], false);
        assert_eq!(resolved.payload["decision"], "decline");
        assert_eq!(resolved.payload["reason"], "approval_timeout");
        assert_eq!(resolved.payload["error"], "PERMISSION_TIMEOUT");
        let _ = std::fs::remove_dir_all(dir);
    }
}

fn read_file(path: &std::path::Path) -> HashMap<String, SessionPerm> {
    let Ok(text) = std::fs::read_to_string(path) else {
        return HashMap::new();
    };
    let Ok(v) = serde_json::from_str::<Value>(&text) else {
        return HashMap::new();
    };
    v.get("sessions")
        .and_then(|s| serde_json::from_value(s.clone()).ok())
        .unwrap_or_default()
}

pub async fn get_permission(
    axum::extract::State(state): axum::extract::State<Arc<AppState>>,
    axum::extract::Path(id): axum::extract::Path<String>,
) -> axum::response::Response {
    use axum::response::IntoResponse;
    if state.sessions.get(&id).is_none() {
        return (
            axum::http::StatusCode::NOT_FOUND,
            axum::Json(json!({ "error": { "code": "SESSION_NOT_FOUND" } })),
        )
            .into_response();
    }
    axum::Json(state.permissions.snapshot(&id)).into_response()
}

#[derive(Deserialize)]
pub struct SetModeReq {
    mode: String,
}

pub async fn set_permission(
    axum::extract::State(state): axum::extract::State<Arc<AppState>>,
    axum::extract::Path(id): axum::extract::Path<String>,
    axum::Json(req): axum::Json<SetModeReq>,
) -> axum::response::Response {
    use axum::response::IntoResponse;
    if state.sessions.get(&id).is_none() {
        return (
            axum::http::StatusCode::NOT_FOUND,
            axum::Json(json!({ "error": { "code": "SESSION_NOT_FOUND" } })),
        )
            .into_response();
    }
    match state.permissions.set_mode(&id, &req.mode) {
        Ok(mode) => axum::Json(json!({ "mode": mode })).into_response(),
        Err(m) => (
            axum::http::StatusCode::BAD_REQUEST,
            axum::Json(json!({ "error": { "code": "INVALID_INPUT", "message": m } })),
        )
            .into_response(),
    }
}

/// 审批回话体(全可选;不带 = 就这一次的普通允许/拒绝,与旧前端完全兼容)。
#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApproveRequest {
    /// `accept` | `acceptForSession` | `decline`(Codex 档次;本地引擎只看是否 decline)。
    #[serde(default)]
    pub decision: Option<String>,
    /// `item/tool/requestUserInput` 的答卷。
    #[serde(default)]
    pub answers: Option<Value>,
}

pub async fn approve_permission(
    axum::extract::State(state): axum::extract::State<Arc<AppState>>,
    axum::extract::Path(id): axum::extract::Path<String>,
    body: Option<axum::Json<ApproveRequest>>,
) -> axum::Json<Value> {
    let req = body.map(|axum::Json(b)| b).unwrap_or_default();
    let decision = req
        .decision
        .filter(|d| d == "accept" || d == "acceptForSession")
        .unwrap_or_else(|| "accept".to_string());
    let mut payload = json!({ "decision": decision });
    if let Some(a) = req.answers {
        payload["answers"] = a;
    }
    let ok = state.permissions.resolve_with(&id, payload);
    axum::Json(json!({ "ok": ok, "id": id, "allowed": true, "decision": decision }))
}

pub async fn deny_permission(
    axum::extract::State(state): axum::extract::State<Arc<AppState>>,
    axum::extract::Path(id): axum::extract::Path<String>,
) -> axum::Json<Value> {
    let ok = state.permissions.resolve(&id, false);
    axum::Json(json!({ "ok": ok, "id": id, "allowed": false, "decision": "decline" }))
}
