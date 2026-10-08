//! Codex 引擎的 REST 面(设置·Codex 页与状态栏用)。
//!
//! R-5:任何响应都不回显密钥。API Key 登录时 key 只经请求体转交 codex 子进程,
//! 既不落本仓配置文件也不进事件与日志。

use axum::extract::{Query, State};
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::Deserialize;
use serde_json::{json, Value};
use std::sync::Arc;

use crate::AppState;

fn project_root_for(state: &AppState, workspace_id: Option<&str>) -> std::path::PathBuf {
    crate::scope::project_of(state, workspace_id).project_root
}

fn err(status: axum::http::StatusCode, code: &str, message: &str) -> Response {
    (
        status,
        Json(json!({ "error": { "code": code, "message": message } })),
    )
        .into_response()
}

fn upstream(message: &str) -> Response {
    // 502 而不是 500:失败源在 codex 子进程/上游服务,不是本仓的路由。
    err(
        axum::http::StatusCode::BAD_GATEWAY,
        "CODEX_UPSTREAM",
        message,
    )
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScopeQuery {
    #[serde(default)]
    pub workspace_id: Option<String>,
    #[serde(default)]
    pub session_id: Option<String>,
}

/// GET /api/forge/codex/status —— 安装/连接/账户/额度/模型/MCP 注入状态一次给全。
pub async fn status(
    State(state): State<Arc<AppState>>,
    Query(q): Query<ScopeQuery>,
) -> Json<Value> {
    let ws = q
        .session_id
        .as_deref()
        .and_then(|s| state.sessions.get(s))
        .and_then(|s| s.workspace_id)
        .or(q.workspace_id);
    let root = project_root_for(&state, ws.as_deref());
    Json(state.codex.status_json(&root))
}

/// POST /api/forge/codex/install —— 后台跑 npm 装 codex 与 open-computer-use。
pub async fn install(State(state): State<Arc<AppState>>) -> Response {
    let session_ids = state.sessions.list().into_iter().map(|s| s.id).collect();
    match state
        .codex
        .start_install(Arc::clone(&state.events), session_ids)
    {
        Ok(true) => Json(json!({ "ok": true, "started": true })).into_response(),
        // 已经在装了不算错:前端连点两次不该弹错误。
        Ok(false) => Json(json!({ "ok": true, "started": false, "running": true })).into_response(),
        Err(msg) => err(axum::http::StatusCode::BAD_REQUEST, "NPM_NOT_FOUND", &msg),
    }
}

/// POST /api/forge/codex/config —— 改配置并落盘,返回完整状态。
pub async fn set_config(
    State(state): State<Arc<AppState>>,
    Json(req): Json<super::config::CodexConfigPatch>,
) -> Response {
    // computerUse 一改,生效的 MCP 服务清单就变了:工具面缓存必须清,
    // 否则本地引擎那条腿在下一次重启前仍按旧清单给模型工具面。
    let touches_servers = req.computer_use.is_some();
    let cfg = match super::config::patch(req) {
        Ok(c) => c,
        Err(msg) => {
            return err(
                axum::http::StatusCode::BAD_REQUEST,
                "CODEX_CONFIG_INVALID",
                &msg,
            )
        }
    };
    if touches_servers {
        crate::mcp::invalidate_tools_cache().await;
    }
    let root = project_root_for(&state, None);
    let mut body = state.codex.status_json(&root);
    body["config"] = cfg.to_json();
    Json(body).into_response()
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LoginRequest {
    /// `chatgpt`(订阅额度,浏览器 OAuth)| `deviceCode` | `apiKey`
    #[serde(default = "default_login_kind")]
    pub kind: String,
    /// `kind=apiKey` 时必填。R-5:只转交 codex,不落本仓任何文件。
    #[serde(default)]
    pub api_key: Option<String>,
}

fn default_login_kind() -> String {
    "chatgpt".to_string()
}

/// POST /api/forge/codex/login —— chatgpt 流返回 `authUrl`,由前端新标签打开。
pub async fn login(State(state): State<Arc<AppState>>, Json(req): Json<LoginRequest>) -> Response {
    if !matches!(req.kind.as_str(), "chatgpt" | "deviceCode" | "apiKey") {
        return err(axum::http::StatusCode::BAD_REQUEST, "LOGIN_KIND_INVALID", "未知登录方式");
    }
    if req.kind == "apiKey" && req.api_key.as_deref().map(str::trim).unwrap_or("").is_empty() {
        return err(axum::http::StatusCode::BAD_REQUEST, "KEY_REQUIRED", "API Key 不可空");
    }
    if let Err(message) = super::config::patch(super::config::CodexConfigPatch {
        auth_source: Some(super::config::AUTH_CHATGPT.to_string()),
        ..Default::default()
    }) {
        return err(axum::http::StatusCode::INTERNAL_SERVER_ERROR, "FORGE_IO", &message);
    }
    match state.codex.login(&req.kind, req.api_key.as_deref()).await {
        Ok(v) => {
            let mut body = json!({ "ok": true });
            if let Some(obj) = v.as_object() {
                for (k, val) in obj {
                    body[k] = val.clone();
                }
            }
            // 登录成功后账户态立刻刷一遍,前端不必再轮询一轮才看到 planType。
            let account = state.codex.refresh_account().await.ok();
            if let Some(a) = account {
                body["account"] = json!({
                    "authMode": a.auth_mode, "planType": a.plan_type, "email": a.email,
                });
            }
            Json(body).into_response()
        }
        Err(e) => upstream(&e.0),
    }
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LoginCancelRequest {
    #[serde(default)]
    pub login_id: Option<String>,
}

/// POST /api/forge/codex/login/cancel
pub async fn login_cancel(
    State(state): State<Arc<AppState>>,
    body: Option<Json<LoginCancelRequest>>,
) -> Response {
    let req = body.map(|Json(b)| b).unwrap_or_default();
    match state.codex.cancel_login(req.login_id.as_deref()).await {
        Ok(_) => Json(json!({ "ok": true })).into_response(),
        Err(e) => upstream(&e.0),
    }
}

/// POST /api/forge/codex/logout
pub async fn logout(State(state): State<Arc<AppState>>) -> Response {
    match state.codex.logout().await {
        Ok(()) => Json(json!({ "ok": true })).into_response(),
        Err(e) => upstream(&e.0),
    }
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelsQuery {
    #[serde(default)]
    pub refresh: Option<bool>,
}

/// GET /api/forge/codex/models —— 登录后可用的模型清单(带缓存)。
pub async fn models(State(state): State<Arc<AppState>>, Query(q): Query<ModelsQuery>) -> Response {
    if super::config::effective_auth_source(&state.cloud) == super::config::AUTH_CLOUD {
        let force = q.refresh.unwrap_or(false);
        if force {
            if let Err(e) = state.cloud.fetch_catalog(true).await {
                return err(
                    axum::http::StatusCode::from_u16(e.status)
                        .unwrap_or(axum::http::StatusCode::BAD_GATEWAY),
                    &e.code,
                    &e.message,
                );
            }
        }
        let items = cloud_codex_models(&state.cloud);
        return Json(json!({ "ok": true, "models": items })).into_response();
    }
    match state.codex.models(q.refresh.unwrap_or(false)).await {
        Ok(items) => Json(json!({ "ok": true, "models": items })).into_response(),
        Err(e) => upstream(&e.0),
    }
}

fn cloud_codex_models(cloud: &crate::cloud::CloudService) -> Vec<Value> {
    let Some(catalog) = cloud.catalog_cached() else {
        return Vec::new();
    };
    catalog
        .responses_models()
        .map(|m| {
            json!({
                "id": format!("codex:{}", m.id),
                "displayName": m.display_name,
                "slug": m.id,
                "model": m.id,
            })
        })
        .collect()
}

/// GET /api/forge/codex/rate-limits —— 订阅额度窗口用量。
pub async fn rate_limits(State(state): State<Arc<AppState>>) -> Response {
    match state.codex.refresh_rate_limits().await {
        Ok(v) => Json(json!({ "ok": true, "rateLimits": v })).into_response(),
        Err(e) => upstream(&e.0),
    }
}

/// GET /api/forge/codex/account —— 账户态(未登录 = authMode null,不算错误)。
pub async fn account(State(state): State<Arc<AppState>>) -> Response {
    if super::config::effective_auth_source(&state.cloud) == super::config::AUTH_CLOUD {
        if !state.cloud.is_logged_in() {
            return Json(json!({
                "ok": true,
                "authMode": null,
                "planType": null,
                "email": null,
                "rateLimits": null,
                "lastError": null,
            }))
            .into_response();
        }
        let user = state.cloud.user_summary().unwrap_or(Value::Null);
        let plan_name = state
            .cloud
            .status_json()
            .get("subscriptions")
            .and_then(|s| s.as_array())
            .and_then(|a| a.first())
            .and_then(|s| s.get("planName"))
            .and_then(Value::as_str)
            .map(str::to_string)
            .unwrap_or_else(|| "balance".to_string());
        return Json(json!({
            "ok": true,
            "authMode": "cloud",
            "planType": plan_name,
            "email": user.get("email").cloned().unwrap_or(Value::Null),
            "rateLimits": null,
            "lastError": null,
        }))
        .into_response();
    }
    match state.codex.refresh_account().await {
        Ok(a) => Json(json!({
            "ok": true,
            "authMode": a.auth_mode,
            "planType": a.plan_type,
            "email": a.email,
            "rateLimits": a.rate_limits,
            "lastError": a.last_error,
        }))
        .into_response(),
        Err(e) => upstream(&e.0),
    }
}

/// GET /api/forge/codex/mcp/status —— 静态注入表 + 已运行线程的 app-server 实时状态。
pub async fn mcp_status(
    State(state): State<Arc<AppState>>,
    Query(q): Query<ScopeQuery>,
) -> Json<Value> {
    let session = q.session_id.as_deref().and_then(|s| state.sessions.get(s));
    let ws = session
        .as_ref()
        .and_then(|s| s.workspace_id.clone())
        .or(q.workspace_id);
    let root = project_root_for(&state, ws.as_deref());
    let mut body = super::mcp_config::status_json(&root);
    body["live"] = json!(false);

    let Some(thread_id) = session.and_then(|s| s.codex_thread_id) else {
        return Json(body);
    };
    let Some(client) = state.codex.running_client() else {
        return Json(body);
    };
    match client
        .request(
            "mcpServerStatus/list",
            json!({ "threadId": thread_id, "detail": "toolsAndAuthOnly" }),
        )
        .await
    {
        Ok(runtime) => {
            body["live"] = json!(true);
            body["runtime"] = runtime;
        }
        Err(_) => {
            // 状态页仍可展示静态配置；不把上游错误原文（可能含环境路径）塞进响应。
            body["runtimeError"] = json!("app-server 未能读取 MCP 实时状态");
        }
    }
    Json(body)
}
