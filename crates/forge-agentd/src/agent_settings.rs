//! Agent defaults and selectable capabilities, shared by settings and local execution.

use axum::{
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};

use crate::AppState;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default, rename_all = "camelCase")]
pub struct AgentConfig {
    pub explore_model: String,
    pub default_permission_mode: String,
}

impl Default for AgentConfig {
    fn default() -> Self {
        Self {
            explore_model: String::new(),
            default_permission_mode: "bypass".into(),
        }
    }
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AgentConfigPatch {
    pub explore_model: Option<String>,
    pub default_permission_mode: Option<String>,
}

fn path(state: &AppState) -> PathBuf {
    state.sessions.path().with_file_name("agent-config.json")
}

fn write_lock() -> &'static Mutex<()> {
    static LOCK: std::sync::OnceLock<Mutex<()>> = std::sync::OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

fn read(path: &Path) -> Result<AgentConfig, String> {
    match std::fs::read_to_string(path) {
        Ok(text) => {
            let config: AgentConfig =
                serde_json::from_str(&text).map_err(|e| format!("Agent 设置解析失败: {e}"))?;
            if !crate::permission::is_known_mode(&config.default_permission_mode) {
                return Err("Agent 设置包含未知执行权限".into());
            }
            Ok(config)
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(AgentConfig::default()),
        Err(e) => Err(format!("Agent 设置读取失败: {e}")),
    }
}

pub fn load(state: &AppState) -> Result<AgentConfig, String> {
    let _guard = write_lock().lock().unwrap_or_else(|e| e.into_inner());
    read(&path(state))
}

fn model_options(state: &AppState) -> Vec<Value> {
    crate::snapshot::models_json(&state.cloud)["models"]
        .as_array()
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .map(|model| {
            json!({
                "id": model["id"], "label": model["label"],
                "group": model["group"], "availability": model["availability"],
            })
        })
        .collect()
}

fn face(state: &AppState, config: AgentConfig) -> Value {
    json!({
        "config": config,
        "options": {
            "exploreModels": model_options(state),
            "permissionModes": crate::permission::mode_options(),
        },
    })
}

pub fn patch(state: &AppState, req: AgentConfigPatch) -> Result<AgentConfig, String> {
    let _guard = write_lock().lock().unwrap_or_else(|e| e.into_inner());
    let destination = path(state);
    let mut config = read(&destination)?;
    if let Some(model) = req.explore_model {
        let model = model.trim();
        if !model.is_empty() && !model_options(state).iter().any(|m| m["id"] == model) {
            return Err(format!("未知本地 Explore 模型: {model}"));
        }
        config.explore_model = model.to_string();
    }
    if let Some(mode) = req.default_permission_mode {
        if !crate::permission::is_known_mode(&mode) {
            return Err(format!("未知执行权限: {mode}"));
        }
        config.default_permission_mode = mode;
    }
    let text = serde_json::to_string_pretty(&config).map_err(|e| e.to_string())?;
    if let Some(parent) = destination.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("Agent 设置目录创建失败: {e}"))?;
    }
    let tmp = destination.with_extension("json.tmp");
    std::fs::write(&tmp, text).map_err(|e| format!("Agent 设置写入失败: {e}"))?;
    crate::permission::replace_persisted_file(&tmp, &destination)
        .map_err(|e| format!("Agent 设置保存失败: {e}"))?;
    Ok(config)
}

/// A role's explicit profile model wins; the Explore default fills only automatic profiles.
pub fn subagent_model(
    kind: Option<&str>,
    profile_model: Option<&str>,
    config: &AgentConfig,
) -> Option<String> {
    let explicit = profile_model
        .map(str::trim)
        .filter(|m| !m.is_empty() && *m != "default");
    explicit.map(str::to_string).or_else(|| {
        (kind == Some("explore") && !config.explore_model.is_empty())
            .then(|| config.explore_model.clone())
    })
}

pub async fn get_config(State(state): State<Arc<AppState>>) -> Response {
    match load(&state) {
        Ok(config) => Json(face(&state, config)).into_response(),
        Err(message) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error":{"code":"CONFIG_READ_FAILED","message":message}})),
        )
            .into_response(),
    }
}

pub async fn patch_config(
    State(state): State<Arc<AppState>>,
    Json(req): Json<AgentConfigPatch>,
) -> Response {
    match patch(&state, req) {
        Ok(config) => Json(face(&state, config)).into_response(),
        Err(message) => (
            StatusCode::BAD_REQUEST,
            Json(json!({"error":{"code":"INVALID_AGENT_CONFIG","message":message}})),
        )
            .into_response(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn explore_default_respects_explicit_profiles_and_other_roles() {
        let config = AgentConfig {
            explore_model: "openai-compat".into(),
            ..Default::default()
        };
        assert_eq!(
            subagent_model(Some("explore"), Some("default"), &config).as_deref(),
            Some("openai-compat")
        );
        assert_eq!(
            subagent_model(Some("explore"), Some(" deepseek-chat "), &config).as_deref(),
            Some("deepseek-chat")
        );
        assert_eq!(
            subagent_model(Some("reviewer"), Some("default"), &config),
            None
        );
        assert_eq!(subagent_model(None, None, &config), None);
        assert_eq!(
            subagent_model(Some("explore"), None, &AgentConfig::default()),
            None
        );
    }

    #[test]
    fn config_roundtrip_rejects_invalid_values_without_losing_previous_settings() {
        let (state, dir) = crate::test_app_state("agent-settings");
        assert_eq!(load(&state).unwrap(), AgentConfig::default());
        patch(
            &state,
            AgentConfigPatch {
                explore_model: Some("deepseek-chat".into()),
                ..Default::default()
            },
        )
        .unwrap();
        patch(
            &state,
            AgentConfigPatch {
                default_permission_mode: Some("plan".into()),
                ..Default::default()
            },
        )
        .unwrap();
        let saved = load(&state).unwrap();
        assert_eq!(saved.explore_model, "deepseek-chat");
        assert_eq!(saved.default_permission_mode, "plan");
        for req in [
            AgentConfigPatch {
                explore_model: Some("codex:unknown".into()),
                ..Default::default()
            },
            AgentConfigPatch {
                default_permission_mode: Some("unrecognized".into()),
                ..Default::default()
            },
        ] {
            assert!(patch(&state, req).is_err());
            assert_eq!(load(&state).unwrap(), saved);
        }
        patch(
            &state,
            AgentConfigPatch {
                explore_model: Some(String::new()),
                ..Default::default()
            },
        )
        .unwrap();
        assert!(load(&state).unwrap().explore_model.is_empty());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[tokio::test]
    async fn defaults_apply_to_new_sessions_and_forks_retain_source_permissions() {
        use axum::{body::Body, http::Request, routing::get, Router};
        use http_body_util::BodyExt;
        use tower::ServiceExt;
        let (state, dir) = crate::test_app_state("agent-default-sessions");
        let old = state
            .sessions
            .create("existing", "coding", None, false, None);
        let app = Router::new()
            .route(
                "/api/forge/agent/config",
                get(get_config).patch(patch_config),
            )
            .route(
                "/api/forge/sessions",
                axum::routing::post(crate::sessions::create_session),
            )
            .route(
                "/api/forge/sessions/{id}/fork",
                axum::routing::post(crate::sessions::fork_session),
            )
            .with_state(state.clone());
        let request = |method: &str, url: &str, body: &str| {
            Request::builder()
                .method(method)
                .uri(url)
                .header("Content-Type", "application/json")
                .body(Body::from(body.to_string()))
                .unwrap()
        };
        let response = app
            .clone()
            .oneshot(request(
                "PATCH",
                "/api/forge/agent/config",
                r#"{"defaultPermissionMode":"plan","exploreModel":"deepseek-chat"}"#,
            ))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let wire: Value =
            serde_json::from_slice(&response.into_body().collect().await.unwrap().to_bytes())
                .unwrap();
        assert_eq!(
            wire["options"]["permissionModes"],
            crate::permission::mode_options()
        );
        assert!(wire["options"]["exploreModels"]
            .as_array()
            .unwrap()
            .iter()
            .all(|m| !m["id"].as_str().unwrap().starts_with("codex:")));
        let response = app
            .clone()
            .oneshot(request("POST", "/api/forge/sessions", "{}"))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let session: Value =
            serde_json::from_slice(&response.into_body().collect().await.unwrap().to_bytes())
                .unwrap();
        let id = session["session"]["id"].as_str().unwrap();
        assert_eq!(state.permissions.mode(id), "plan");
        assert_eq!(
            state.permissions.mode(&old.id),
            "bypass",
            "existing sessions are unchanged"
        );
        patch(
            &state,
            AgentConfigPatch {
                default_permission_mode: Some("auto".into()),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(
            state.permissions.mode(id),
            "plan",
            "changing defaults is not retroactive"
        );
        let response = app
            .oneshot(request(
                "POST",
                &format!("/api/forge/sessions/{id}/fork"),
                "{}",
            ))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let fork: Value =
            serde_json::from_slice(&response.into_body().collect().await.unwrap().to_bytes())
                .unwrap();
        assert_eq!(
            state
                .permissions
                .mode(fork["session"]["id"].as_str().unwrap()),
            "plan"
        );
        let reloaded =
            crate::permission::PermissionService::load(dir.join("agent-sessions/permissions.json"));
        assert_eq!(reloaded.mode(id), "plan");
        std::fs::remove_dir_all(dir).unwrap();
    }
}
