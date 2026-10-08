//! `/api/forge/account/*` 路由(15_CLOUD_SERVICE.md §8.2 / §11.4;settings/sync 两组在 [super::sync])。

use std::sync::Arc;

use axum::extract::{Path, Query, State};
use axum::http::{header, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{delete, get, patch, post};
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::{json, Map, Value};

use super::client::Reply;
use super::{CloudError, ConfigPatch};
use crate::AppState;

pub(crate) fn routes() -> Router<Arc<AppState>> {
    Router::new()
        .route("/api/forge/account/status", get(get_status))
        .route(
            "/api/forge/account/config",
            get(get_config).post(post_config),
        )
        .route("/api/forge/account/auth-config", get(get_auth_config))
        .route("/api/forge/account/register", post(post_register))
        .route("/api/forge/account/login", post(post_login))
        .route("/api/forge/account/email-code", post(post_email_code))
        .route("/api/forge/account/logout", post(post_logout))
        .route("/api/forge/account/me", get(get_me))
        .route("/api/forge/account/profile", patch(patch_profile))
        .route("/api/forge/account/password", post(post_password))
        .route(
            "/api/forge/account/avatar",
            get(get_avatar).put(put_avatar).delete(delete_avatar),
        )
        .route("/api/forge/account/devices", get(get_devices))
        .route("/api/forge/account/devices/{id}", delete(delete_device))
        .route(
            "/api/forge/account/api-keys",
            get(get_api_keys).post(post_api_keys),
        )
        .route("/api/forge/account/api-keys/{id}", delete(delete_api_key))
        .route("/api/forge/account/balance", get(get_balance))
        .route("/api/forge/account/subscription", get(get_subscription))
        .route("/api/forge/account/usage", get(get_usage))
        .route("/api/forge/account/usage/daily", get(get_usage_daily))
        .route("/api/forge/account/ledger", get(get_ledger))
        .route("/api/forge/account/redeem", post(post_redeem))
        .route("/api/forge/account/plans", get(get_plans))
        .route("/api/forge/account/models", get(get_models))
        // 会员梯度与额度计费(15 §11.4)。
        .route("/api/forge/account/tiers", get(get_tiers))
        .route("/api/forge/account/membership", get(get_membership))
        .route(
            "/api/forge/account/membership/on-demand",
            patch(patch_on_demand),
        )
        .route(
            "/api/forge/account/membership/usage",
            get(get_membership_usage),
        )
        .route(
            "/api/forge/account/membership/quote",
            post(post_membership_quote),
        )
        .route(
            "/api/forge/account/membership/checkout",
            post(post_membership_checkout),
        )
        .route(
            "/api/forge/account/membership/scheduled/{id}",
            delete(delete_membership_scheduled),
        )
        .route("/api/forge/account/orders", get(get_orders))
        .route(
            "/api/forge/account/orders/{id}/cancel",
            post(post_order_cancel),
        )
}

fn cloud_err(e: CloudError) -> Response {
    let status = StatusCode::from_u16(e.status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
    (
        status,
        Json(json!({
            "error": { "code": e.code, "message": e.message }
        })),
    )
        .into_response()
}

async fn status_body(state: &AppState) -> Value {
    let mut v = state.cloud.status().await;
    if super::byo_allowed()
        && crate::codex::config::effective_auth_source(&state.cloud) == crate::codex::config::AUTH_CHATGPT
        && state.codex.account_snapshot().auth_mode.is_some()
    {
        v["byoConfigured"] = json!(true);
    }
    if let Some(obj) = v.as_object_mut() {
        obj.insert("sync".to_string(), super::sync::sync_status(state));
    }
    v
}

async fn get_status(State(state): State<Arc<AppState>>) -> Json<Value> {
    Json(status_body(&state).await)
}

async fn get_config(State(state): State<Arc<AppState>>) -> Json<Value> {
    Json(state.cloud.config_json())
}

async fn post_config(
    State(state): State<Arc<AppState>>,
    Json(patch): Json<ConfigPatch>,
) -> Response {
    match state.cloud.update_config(patch) {
        Ok(v) => Json(v).into_response(),
        Err(e) => cloud_err(e),
    }
}

async fn get_auth_config(State(state): State<Arc<AppState>>) -> Response {
    match state
        .cloud
        .call_public("GET", "/api/v1/auth/config", None)
        .await
    {
        Ok(reply) if reply.ok() => match reply.json() {
            Ok(v) => Json(v).into_response(),
            Err(e) => cloud_err(e),
        },
        Ok(reply) => cloud_err(reply.to_error()),
        Err(e) => cloud_err(e),
    }
}

#[derive(Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AuthBody {
    #[serde(default)]
    email: String,
    #[serde(default)]
    password: String,
    #[serde(default)]
    nickname: Option<String>,
    #[serde(default)]
    invite_code: Option<String>,
    #[serde(default)]
    email_code: Option<String>,
}

async fn post_login(State(state): State<Arc<AppState>>, Json(body): Json<AuthBody>) -> Response {
    let email = body.email.trim();
    let password = body.password.trim();
    if email.is_empty() || password.is_empty() {
        return cloud_err(CloudError::new(
            400,
            "INVALID_INPUT",
            "email 与 password 不可空",
        ));
    }
    match state.cloud.login(email, password).await {
        Ok(()) => {
            state.sync.nudge();
            Json(status_body(&state).await).into_response()
        }
        Err(e) => cloud_err(e),
    }
}

async fn post_register(State(state): State<Arc<AppState>>, Json(body): Json<AuthBody>) -> Response {
    let email = body.email.trim();
    let password = body.password.trim();
    if email.is_empty() || password.is_empty() {
        return cloud_err(CloudError::new(
            400,
            "INVALID_INPUT",
            "email 与 password 不可空",
        ));
    }
    let mut fields = Map::new();
    fields.insert("email".into(), json!(email));
    fields.insert("password".into(), json!(password));
    if let Some(n) = body.nickname {
        let n = n.trim();
        if !n.is_empty() {
            fields.insert("nickname".into(), json!(n));
        }
    }
    if let Some(v) = body.invite_code.filter(|s| !s.trim().is_empty()) {
        fields.insert("inviteCode".into(), json!(v.trim()));
    }
    if let Some(v) = body.email_code.filter(|s| !s.trim().is_empty()) {
        fields.insert("emailCode".into(), json!(v.trim()));
    }
    match state.cloud.register(fields).await {
        Ok(()) => {
            state.sync.nudge();
            Json(status_body(&state).await).into_response()
        }
        Err(e) => cloud_err(e),
    }
}

#[derive(Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct EmailCodeBody {
    #[serde(default)]
    email: String,
    #[serde(default)]
    purpose: String,
}

async fn post_email_code(
    State(state): State<Arc<AppState>>,
    Json(body): Json<EmailCodeBody>,
) -> Response {
    let email = body.email.trim();
    let purpose = body.purpose.trim();
    if email.is_empty() || purpose.is_empty() {
        return cloud_err(CloudError::new(
            400,
            "INVALID_INPUT",
            "email 与 purpose 不可空",
        ));
    }
    match state
        .cloud
        .call_public(
            "POST",
            "/api/v1/auth/email-code",
            Some(json!({ "email": email, "purpose": purpose })),
        )
        .await
    {
        Ok(reply) if reply.ok() => Json(json!({ "ok": true })).into_response(),
        Ok(reply) => cloud_err(reply.to_error()),
        Err(e) => cloud_err(e),
    }
}

async fn post_logout(State(state): State<Arc<AppState>>) -> Json<Value> {
    state.cloud.logout().await;
    Json(status_body(&state).await)
}

async fn get_me(State(state): State<Arc<AppState>>) -> Response {
    match state.cloud.call("GET", "/api/v1/me", None).await {
        Ok(v) => {
            state.cloud.absorb_me(&v);
            Json(v).into_response()
        }
        Err(e) => cloud_err(e),
    }
}

async fn patch_profile(State(state): State<Arc<AppState>>, Json(body): Json<Value>) -> Response {
    match state
        .cloud
        .call("PATCH", "/api/v1/me/profile", Some(body))
        .await
    {
        Ok(v) => {
            state.cloud.absorb_user(&v);
            Json(v).into_response()
        }
        Err(e) => cloud_err(e),
    }
}

async fn post_password(State(state): State<Arc<AppState>>, Json(body): Json<Value>) -> Response {
    match state
        .cloud
        .call("POST", "/api/v1/me/password", Some(body))
        .await
    {
        Ok(v) => Json(v).into_response(),
        Err(e) => cloud_err(e),
    }
}

async fn get_avatar(State(state): State<Arc<AppState>>) -> Response {
    match state.cloud.call_raw("GET", "/api/v1/me/avatar", None).await {
        Ok(reply) if reply.ok() => {
            let ct = if reply.content_type.is_empty() {
                "application/octet-stream".to_string()
            } else {
                reply.content_type.clone()
            };
            (StatusCode::OK, [(header::CONTENT_TYPE, ct)], reply.body).into_response()
        }
        Ok(reply) => cloud_err(reply.to_error()),
        Err(e) => cloud_err(e),
    }
}

async fn put_avatar(State(state): State<Arc<AppState>>, Json(body): Json<Value>) -> Response {
    match state
        .cloud
        .call("PUT", "/api/v1/me/avatar", Some(body))
        .await
    {
        Ok(v) => {
            state.cloud.absorb_user(&v);
            Json(v).into_response()
        }
        Err(e) => cloud_err(e),
    }
}

async fn delete_avatar(State(state): State<Arc<AppState>>) -> Response {
    match state.cloud.call("DELETE", "/api/v1/me/avatar", None).await {
        Ok(v) => {
            state.cloud.absorb_user(&v);
            Json(v).into_response()
        }
        Err(e) => cloud_err(e),
    }
}

async fn get_devices(State(state): State<Arc<AppState>>) -> Response {
    match state.cloud.call("GET", "/api/v1/me/devices", None).await {
        Ok(v) => Json(v).into_response(),
        Err(e) => cloud_err(e),
    }
}

async fn delete_device(State(state): State<Arc<AppState>>, Path(id): Path<String>) -> Response {
    let path = format!("/api/v1/me/devices/{id}");
    let own = state.cloud.current_session_id().as_deref() == Some(id.as_str());
    match state.cloud.call("DELETE", &path, None).await {
        Ok(v) => {
            if own {
                state.cloud.local_logout(None);
            }
            Json(v).into_response()
        }
        Err(e) => cloud_err(e),
    }
}

async fn get_api_keys(State(state): State<Arc<AppState>>) -> Response {
    match state.cloud.call("GET", "/api/v1/me/api-keys", None).await {
        Ok(v) => Json(strip_user_api_key_plaintext(v)).into_response(),
        Err(e) => cloud_err(e),
    }
}

async fn post_api_keys(State(state): State<Arc<AppState>>, Json(body): Json<Value>) -> Response {
    match state
        .cloud
        .call("POST", "/api/v1/me/api-keys", Some(body))
        .await
    {
        // 用户自建的 API Key 明文仅此一次(§3.2);设备 Key / JWT 永不出 BFF。
        Ok(v) => Json(v).into_response(),
        Err(e) => cloud_err(e),
    }
}

/// 列表响应去掉误带的明文 key 字段(创建响应不在此路径)。
fn strip_user_api_key_plaintext(v: Value) -> Value {
    let mut out = v;
    if let Some(items) = out.get_mut("items").and_then(Value::as_array_mut) {
        for item in items {
            if let Some(obj) = item.as_object_mut() {
                obj.remove("key");
            }
        }
    }
    out
}

async fn delete_api_key(State(state): State<Arc<AppState>>, Path(id): Path<String>) -> Response {
    let path = format!("/api/v1/me/api-keys/{id}");
    match state.cloud.call("DELETE", &path, None).await {
        Ok(v) => Json(v).into_response(),
        Err(e) => cloud_err(e),
    }
}

async fn get_balance(State(state): State<Arc<AppState>>) -> Response {
    match state.cloud.call("GET", "/api/v1/me/balance", None).await {
        Ok(v) => {
            state.cloud.absorb_balance(&v);
            Json(v).into_response()
        }
        Err(e) => cloud_err(e),
    }
}

async fn get_subscription(State(state): State<Arc<AppState>>) -> Response {
    match state
        .cloud
        .call("GET", "/api/v1/me/subscription", None)
        .await
    {
        Ok(v) => {
            state
                .cloud
                .absorb_balance(&json!({ "subscriptions": v.get("items").cloned() }));
            Json(v).into_response()
        }
        Err(e) => cloud_err(e),
    }
}

#[derive(Default, Deserialize)]
struct RawQuery {
    #[serde(default)]
    from: Option<String>,
    #[serde(default)]
    to: Option<String>,
    #[serde(default)]
    limit: Option<String>,
    #[serde(default)]
    offset: Option<String>,
    #[serde(default)]
    days: Option<String>,
}

fn query_path(base: &str, q: &RawQuery) -> String {
    let mut parts = Vec::new();
    if let Some(v) = &q.from {
        if !v.is_empty() {
            parts.push(format!("from={}", urlencoding(v)));
        }
    }
    if let Some(v) = &q.to {
        if !v.is_empty() {
            parts.push(format!("to={}", urlencoding(v)));
        }
    }
    if let Some(v) = &q.limit {
        if !v.is_empty() {
            parts.push(format!("limit={}", urlencoding(v)));
        }
    }
    if let Some(v) = &q.offset {
        if !v.is_empty() {
            parts.push(format!("offset={}", urlencoding(v)));
        }
    }
    if let Some(v) = &q.days {
        if !v.is_empty() {
            parts.push(format!("days={}", urlencoding(v)));
        }
    }
    if parts.is_empty() {
        base.to_string()
    } else {
        format!("{}?{}", base, parts.join("&"))
    }
}

fn urlencoding(s: &str) -> String {
    s.chars()
        .map(|c| match c {
            'A'..='Z' | 'a'..='z' | '0'..='9' | '-' | '_' | '.' | '~' => c.to_string(),
            _ => format!("%{:02X}", c as u32),
        })
        .collect()
}

async fn get_usage(State(state): State<Arc<AppState>>, Query(q): Query<RawQuery>) -> Response {
    let path = query_path("/api/v1/me/usage", &q);
    match state.cloud.call("GET", &path, None).await {
        Ok(v) => Json(v).into_response(),
        Err(e) => cloud_err(e),
    }
}

async fn get_usage_daily(
    State(state): State<Arc<AppState>>,
    Query(q): Query<RawQuery>,
) -> Response {
    let path = query_path("/api/v1/me/usage/daily", &q);
    match state.cloud.call("GET", &path, None).await {
        Ok(v) => Json(v).into_response(),
        Err(e) => cloud_err(e),
    }
}

async fn get_ledger(State(state): State<Arc<AppState>>, Query(q): Query<RawQuery>) -> Response {
    let path = query_path("/api/v1/me/ledger", &q);
    match state.cloud.call("GET", &path, None).await {
        Ok(v) => Json(v).into_response(),
        Err(e) => cloud_err(e),
    }
}

async fn post_redeem(State(state): State<Arc<AppState>>, Json(body): Json<Value>) -> Response {
    match state
        .cloud
        .call("POST", "/api/v1/me/redeem", Some(body))
        .await
    {
        Ok(v) => {
            state.cloud.absorb_balance(&v);
            Json(v).into_response()
        }
        Err(e) => cloud_err(e),
    }
}

async fn get_plans(State(state): State<Arc<AppState>>) -> Response {
    match state.cloud.call_public("GET", "/api/v1/plans", None).await {
        Ok(reply) if reply.ok() => match reply.json() {
            Ok(v) => Json(v).into_response(),
            Err(e) => cloud_err(e),
        },
        Ok(reply) => cloud_err(reply.to_error()),
        Err(e) => cloud_err(e),
    }
}

async fn get_models(State(state): State<Arc<AppState>>) -> Response {
    match state.cloud.fetch_catalog(false).await {
        Ok(c) => Json(c).into_response(),
        Err(e) => cloud_err(e),
    }
}

// ---------- 会员梯度与额度计费(15 §11.2 / §11.4) ----------

/// 云端业务错误原样透传:状态码 + 云端 JSON 体。§11.2 的 402 `INSUFFICIENT_BALANCE` 在顶层附
/// `balanceMicros` / `amountMicros`(客户端据此显示差额),规范化成 `{error:{code,message}}`
/// 会把它们丢掉;回包不是 `{error:{code}}` 形状时才退回 [cloud_err] 的规范形状。
fn cloud_reply_err(reply: &Reply) -> Response {
    let parsed = serde_json::from_slice::<Value>(&reply.body).ok();
    let has_code = parsed
        .as_ref()
        .and_then(|v| v.pointer("/error/code"))
        .and_then(Value::as_str)
        .is_some_and(|c| !c.is_empty());
    match parsed {
        Some(body) if has_code => {
            let status =
                StatusCode::from_u16(reply.status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
            (status, Json(body)).into_response()
        }
        _ => cloud_err(reply.to_error()),
    }
}

/// 会员面透传:agentd 注入 JWT(401 自动续期一次);2xx → JSON,非 2xx → [cloud_reply_err]。
async fn membership_call(
    state: &AppState,
    method: &str,
    path: &str,
    body: Option<Value>,
) -> Result<Value, Response> {
    match state.cloud.call_raw(method, path, body).await {
        Ok(reply) if reply.ok() => reply.json().map_err(cloud_err),
        Ok(reply) => Err(cloud_reply_err(&reply)),
        Err(e) => Err(cloud_err(e)),
    }
}

fn membership_reply(result: Result<Value, Response>) -> Response {
    match result {
        Ok(v) => Json(v).into_response(),
        Err(r) => r,
    }
}

/// 路径里的订阅 / 订单 ID 是整数(15 §1):只放行纯数字,别的形状不拼进云端 URL。
fn numeric_id(raw: &str) -> Result<&str, Response> {
    if !raw.is_empty() && raw.len() <= 20 && raw.bytes().all(|b| b.is_ascii_digit()) {
        Ok(raw)
    } else {
        Err(cloud_err(CloudError::new(
            400,
            "INVALID_INPUT",
            "id 须为整数",
        )))
    }
}

/// `GET /tiers`:公开档位梯度(未登录也可看)。
async fn get_tiers(State(state): State<Arc<AppState>>) -> Response {
    match state.cloud.call_public("GET", "/api/v1/tiers", None).await {
        Ok(reply) if reply.ok() => match reply.json() {
            Ok(v) => Json(v).into_response(),
            Err(e) => cloud_err(e),
        },
        Ok(reply) => cloud_reply_err(&reply),
        Err(e) => cloud_err(e),
    }
}

async fn get_membership(State(state): State<Arc<AppState>>) -> Response {
    membership_reply(membership_call(&state, "GET", "/api/v1/me/membership", None).await)
}

async fn patch_on_demand(State(state): State<Arc<AppState>>, Json(body): Json<Value>) -> Response {
    membership_reply(
        membership_call(
            &state,
            "PATCH",
            "/api/v1/me/membership/on-demand",
            Some(body),
        )
        .await,
    )
}

/// `GET /membership/usage?from=&to=`(缺省 = 当前用量周期)。
async fn get_membership_usage(
    State(state): State<Arc<AppState>>,
    Query(q): Query<RawQuery>,
) -> Response {
    let q = RawQuery {
        from: q.from,
        to: q.to,
        ..Default::default()
    };
    let path = query_path("/api/v1/me/membership/usage", &q);
    membership_reply(membership_call(&state, "GET", &path, None).await)
}

/// `POST /membership/quote`:只读报价。
async fn post_membership_quote(
    State(state): State<Arc<AppState>>,
    Json(body): Json<Value>,
) -> Response {
    membership_reply(
        membership_call(&state, "POST", "/api/v1/me/membership/quote", Some(body)).await,
    )
}

/// `POST /membership/checkout`:余额支付立即扣款生效 → 作废余额 / 订阅缓存。
async fn post_membership_checkout(
    State(state): State<Arc<AppState>>,
    Json(body): Json<Value>,
) -> Response {
    let result =
        membership_call(&state, "POST", "/api/v1/me/membership/checkout", Some(body)).await;
    if let Ok(v) = &result {
        state.cloud.invalidate_balance(v.get("membership"));
    }
    membership_reply(result)
}

/// `DELETE /membership/scheduled/{id}`:取消预约并全额退回余额 → 作废余额 / 订阅缓存。
async fn delete_membership_scheduled(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Response {
    let id = match numeric_id(&id) {
        Ok(id) => id,
        Err(r) => return r,
    };
    let path = format!("/api/v1/me/membership/scheduled/{id}");
    let result = membership_call(&state, "DELETE", &path, None).await;
    if let Ok(v) = &result {
        state.cloud.invalidate_balance(Some(v));
    }
    membership_reply(result)
}

/// `GET /orders?limit=&offset=`。
async fn get_orders(State(state): State<Arc<AppState>>, Query(q): Query<RawQuery>) -> Response {
    let q = RawQuery {
        limit: q.limit,
        offset: q.offset,
        ..Default::default()
    };
    let path = query_path("/api/v1/me/orders", &q);
    membership_reply(membership_call(&state, "GET", &path, None).await)
}

/// `POST /orders/{id}/cancel`:只取消待支付单(不动余额)。
async fn post_order_cancel(State(state): State<Arc<AppState>>, Path(id): Path<String>) -> Response {
    let id = match numeric_id(&id) {
        Ok(id) => id,
        Err(r) => return r,
    };
    let path = format!("/api/v1/me/orders/{id}/cancel");
    membership_reply(membership_call(&state, "POST", &path, None).await)
}
