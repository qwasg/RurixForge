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

struct Pending {
    tx: oneshot::Sender<bool>,
}

pub struct PermissionService {
    path: PathBuf,
    inner: Mutex<HashMap<String, SessionPerm>>,
    pending: Mutex<HashMap<String, Pending>>,
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
                let _ = std::fs::rename(&tmp, &self.path);
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
        self.authorize_with(bus, session_id, run_id, tool, is_write, json!({})).await
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
        let mode = self.mode(session_id);
        if mode == "plan" && is_write {
            return Ok(false);
        }
        if mode != "auto" || !is_write {
            return Ok(true);
        }
        let req_id = new_id("perm");
        let (tx, rx) = oneshot::channel();
        self.pending
            .lock()
            .unwrap()
            .insert(req_id.clone(), Pending { tx });
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
        bus.emit(
            EventDraft::new(session_id, "permission.requested", "agent").payload(payload),
        );
        let ok = tokio::time::timeout(
            std::time::Duration::from_secs(APPROVAL_TIMEOUT_SECS),
            rx,
        )
        .await
        .map_err(|_| "PERMISSION_TIMEOUT".to_string())?
        .unwrap_or(false);
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

    pub fn resolve(&self, id: &str, allow: bool) -> bool {
        if let Some(p) = self.pending.lock().unwrap().remove(id) {
            let _ = p.tx.send(allow);
            return true;
        }
        false
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
        assert!(
            !bus.persisted("s1")
                .iter()
                .any(|e| e.event_type == "permission.requested")
        );
        let _ = std::fs::remove_dir_all(&dir);
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

pub async fn approve_permission(
    axum::extract::State(state): axum::extract::State<Arc<AppState>>,
    axum::extract::Path(id): axum::extract::Path<String>,
) -> axum::Json<Value> {
    let ok = state.permissions.resolve(&id, true);
    axum::Json(json!({ "ok": ok, "id": id, "allowed": true }))
}

pub async fn deny_permission(
    axum::extract::State(state): axum::extract::State<Arc<AppState>>,
    axum::extract::Path(id): axum::extract::Path<String>,
) -> axum::Json<Value> {
    let ok = state.permissions.resolve(&id, false);
    axum::Json(json!({ "ok": ok, "id": id, "allowed": false }))
}
