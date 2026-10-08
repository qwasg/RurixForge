//! CloudService 单测:进程内假 forge-cloud(axum,127.0.0.1:0),全程不触外网。

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex as StdMutex};

use axum::extract::{Path, RawQuery, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{delete, get, patch, post};
use axum::{Json, Router};
use serde_json::{json, Value};

use super::*;

pub(crate) const DEVICE_KEY: &str = "sk-rf-DEVICESECRET0123456789abcdef";
pub(crate) const USER_KEY: &str = "sk-rf-USERKEY-plaintext-once";
pub(crate) const PASSWORD: &str = "password123";
pub(crate) const AVATAR: &[u8] = b"\x89PNG\r\n\x1a\nfake-avatar";

/// 假 forge-cloud 的可观测状态。
#[derive(Default)]
pub(crate) struct Fake {
    pub access: StdMutex<String>,
    pub refresh: StdMutex<String>,
    pub refresh_calls: AtomicUsize,
    pub catalog_calls: AtomicUsize,
    pub logout_calls: AtomicUsize,
    /// `GET /me/balance` 次数(会员变更后是否触发了余额 / 订阅重拉)。
    pub balance_calls: AtomicUsize,
    /// 成功的 `POST /me/membership/checkout` 次数。
    pub checkout_calls: AtomicUsize,
    pub fail_refresh: AtomicBool,
    pub last_login: StdMutex<Option<Value>>,
    seq: AtomicUsize,
}

impl Fake {
    fn issue(&self) -> Value {
        let n = self.seq.fetch_add(1, Ordering::SeqCst);
        let access = auth::fake_jwt(&json!({
            "sub": "1", "sid": "sess-1", "role": "user", "n": n,
            "exp": auth::now_unix() + 900
        }));
        let refresh = format!("rt_{n}_secret");
        *self.access.lock().unwrap() = access.clone();
        *self.refresh.lock().unwrap() = refresh.clone();
        json!({
            "accessToken": access,
            "accessExpiresAt": "2099-01-01T00:00:00Z",
            "refreshToken": refresh,
            "refreshExpiresAt": "2099-01-01T00:00:00Z",
        })
    }

    fn authed(&self, h: &HeaderMap) -> bool {
        let want = format!("Bearer {}", self.access.lock().unwrap());
        h.get("authorization").and_then(|v| v.to_str().ok()) == Some(want.as_str())
    }
}

pub(crate) fn user_json() -> Value {
    json!({
        "id": 1, "email": "a@b.c", "nickname": "tester", "role": "user", "status": "active",
        "hasAvatar": true, "avatarVersion": 2, "groupId": 1, "groupName": "default",
        "balanceMicros": 1000, "createdAt": "2026-09-01T00:00:00Z"
    })
}

pub(crate) fn catalog_json() -> Value {
    json!({
        "defaultModel": "gpt-5.5", "currency": "USD", "rateMultiplier": 1.0,
        "models": [
            {
                "id": "gpt-5.5", "displayName": "GPT-5.5", "platform": "openai",
                "capabilities": { "vision": true, "reasoningEfforts": ["low", "medium", "high"],
                    "contextWindow": 400000, "maxOutput": 128000, "tools": true, "responses": true },
                "pricing": { "inputPer1M": 1250000, "outputPer1M": 10000000,
                    "cacheReadPer1M": 125000, "cacheWritePer1M": 0 },
                "available": true
            },
            {
                "id": "claude-x", "displayName": "Claude X", "platform": "anthropic",
                "capabilities": { "vision": false, "reasoningEfforts": [], "contextWindow": 200000,
                    "maxOutput": 64000, "tools": true, "responses": false },
                "pricing": { "inputPer1M": 3000000, "outputPer1M": 15000000,
                    "cacheReadPer1M": 300000, "cacheWritePer1M": 3750000 },
                "available": false
            }
        ]
    })
}

fn err(status: StatusCode, code: &str) -> Response {
    (
        status,
        Json(json!({ "error": { "code": code, "message": code } })),
    )
        .into_response()
}

fn unauthorized() -> Response {
    err(StatusCode::UNAUTHORIZED, "UNAUTHORIZED")
}

async fn login(State(f): State<Arc<Fake>>, Json(body): Json<Value>) -> Response {
    if body["password"] != PASSWORD {
        return err(StatusCode::UNAUTHORIZED, "INVALID_CREDENTIALS");
    }
    if body["issueDeviceKey"] != true || body["device"]["id"].as_str().unwrap_or("").is_empty() {
        return err(StatusCode::BAD_REQUEST, "BAD_DEVICE");
    }
    *f.last_login.lock().unwrap() = Some(body);
    let mut v = f.issue();
    v["user"] = user_json();
    v["deviceKey"] = json!({ "id": 7, "key": DEVICE_KEY, "prefix": "sk-rf-DEVI" });
    Json(v).into_response()
}

async fn register(State(f): State<Arc<Fake>>, Json(body): Json<Value>) -> Response {
    if body["email"] == "taken@example.com" {
        return err(StatusCode::CONFLICT, "EMAIL_TAKEN");
    }
    login(State(f), Json(body)).await
}

async fn refresh(State(f): State<Arc<Fake>>, Json(body): Json<Value>) -> Response {
    f.refresh_calls.fetch_add(1, Ordering::SeqCst);
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    if f.fail_refresh.load(Ordering::SeqCst) {
        return err(StatusCode::UNAUTHORIZED, "REFRESH_REUSED");
    }
    if body["refreshToken"].as_str() != Some(f.refresh.lock().unwrap().as_str()) {
        return err(StatusCode::UNAUTHORIZED, "REFRESH_INVALID");
    }
    Json(f.issue()).into_response()
}

async fn me(State(f): State<Arc<Fake>>, h: HeaderMap) -> Response {
    if !f.authed(&h) {
        return unauthorized();
    }
    Json(json!({ "user": user_json(), "subscriptions": [], "currency": "USD" })).into_response()
}

async fn balance(State(f): State<Arc<Fake>>, h: HeaderMap) -> Response {
    if !f.authed(&h) {
        return unauthorized();
    }
    f.balance_calls.fetch_add(1, Ordering::SeqCst);
    Json(json!({
        "balanceMicros": 777, "currency": "USD",
        "subscriptions": [{ "id": 1, "planName": "Pro", "status": "active" }]
    }))
    .into_response()
}

async fn catalog(State(f): State<Arc<Fake>>, h: HeaderMap) -> Response {
    if !f.authed(&h) {
        return unauthorized();
    }
    f.catalog_calls.fetch_add(1, Ordering::SeqCst);
    Json(catalog_json()).into_response()
}

async fn logout(State(f): State<Arc<Fake>>, h: HeaderMap) -> Response {
    if !f.authed(&h) {
        return unauthorized();
    }
    f.logout_calls.fetch_add(1, Ordering::SeqCst);
    *f.access.lock().unwrap() = "revoked".into();
    *f.refresh.lock().unwrap() = "revoked".into();
    Json(json!({ "ok": true })).into_response()
}

async fn avatar(State(f): State<Arc<Fake>>, h: HeaderMap) -> Response {
    if !f.authed(&h) {
        return unauthorized();
    }
    ([("content-type", "image/png")], AVATAR.to_vec()).into_response()
}

async fn avatar_put(State(f): State<Arc<Fake>>, h: HeaderMap, Json(body): Json<Value>) -> Response {
    if !f.authed(&h) {
        return unauthorized();
    }
    if !body["dataUrl"]
        .as_str()
        .unwrap_or("")
        .starts_with("data:image/")
    {
        return err(StatusCode::BAD_REQUEST, "AVATAR_INVALID");
    }
    let mut u = user_json();
    u["avatarVersion"] = json!(3);
    Json(u).into_response()
}

async fn device_delete(
    State(f): State<Arc<Fake>>,
    h: HeaderMap,
    Path(_id): Path<String>,
) -> Response {
    if !f.authed(&h) {
        return unauthorized();
    }
    Json(json!({ "ok": true })).into_response()
}

async fn password(State(f): State<Arc<Fake>>, h: HeaderMap) -> Response {
    if !f.authed(&h) {
        return unauthorized();
    }
    err(StatusCode::UNAUTHORIZED, "INVALID_CREDENTIALS")
}

async fn api_keys_post(
    State(f): State<Arc<Fake>>,
    h: HeaderMap,
    Json(body): Json<Value>,
) -> Response {
    if !f.authed(&h) {
        return unauthorized();
    }
    Json(json!({
        "apiKey": { "id": 5, "name": body["name"], "kind": "user", "prefix": "sk-rf-USER" },
        "key": USER_KEY
    }))
    .into_response()
}

async fn profile(State(f): State<Arc<Fake>>, h: HeaderMap, Json(body): Json<Value>) -> Response {
    if !f.authed(&h) {
        return unauthorized();
    }
    let mut u = user_json();
    u["nickname"] = body["nickname"].clone();
    Json(u).into_response()
}

async fn redeem(State(f): State<Arc<Fake>>, h: HeaderMap) -> Response {
    if !f.authed(&h) {
        return unauthorized();
    }
    Json(json!({
        "kind": "balance", "valueMicros": 5_000_000, "plan": null,
        "subscription": null, "balanceMicros": 5_001_000
    }))
    .into_response()
}

async fn usage(State(f): State<Arc<Fake>>, h: HeaderMap, RawQuery(q): RawQuery) -> Response {
    if !f.authed(&h) {
        return unauthorized();
    }
    Json(json!({ "items": [], "total": 0, "query": q })).into_response()
}

// ---------- 会员梯度(15 §11.2) ----------

pub(crate) fn membership_json(balance: i64) -> Value {
    json!({
        "currency": "USD",
        "tier": { "planId": 2, "tier": "pro", "name": "Pro", "rank": 10 },
        "subscription": {
            "id": 11, "planId": 2, "planName": "Pro", "status": "active", "tier": "pro",
            "startsAt": "2026-09-01T00:00:00Z", "endsAt": "2026-10-01T00:00:00Z",
            "quotaMicros": 20_000_000, "usedMicros": 5_000_000,
            "forgeQuotaMicros": 60_000_000, "forgeUsedMicros": 0,
            "usageCycle": "month", "billingInterval": "month",
            "valueMicros": 20_000_000, "source": "purchase"
        },
        "scheduled": [], "packs": [],
        "cycle": { "start": "2026-09-01T00:00:00Z", "end": "2026-10-01T00:00:00Z" },
        "pools": {
            "api": { "includedMicros": 20_000_000, "usedMicros": 5_000_000, "remainingMicros": 15_000_000 },
            "forge": { "includedMicros": 60_000_000, "usedMicros": 0, "remainingMicros": 60_000_000 }
        },
        "onDemand": { "enabled": true, "limitMicros": 0, "usedMicros": 0 },
        "balanceMicros": balance,
        "payment": { "enabled": false, "providers": [] },
        "pendingOrder": null
    })
}

async fn tiers() -> Json<Value> {
    Json(json!({
        "currency": "USD",
        "payment": { "enabled": false, "providers": [] },
        "items": [
            { "planId": 1, "tier": "hobby", "name": "Hobby", "priceMonthlyMicros": 0, "priceYearlyMicros": 0, "rank": 0 },
            { "planId": 2, "tier": "pro", "name": "Pro", "priceMonthlyMicros": 20_000_000,
              "priceYearlyMicros": 192_000_000, "highlight": true, "rank": 10 }
        ]
    }))
}

async fn membership(State(f): State<Arc<Fake>>, h: HeaderMap) -> Response {
    if !f.authed(&h) {
        return unauthorized();
    }
    Json(membership_json(1000)).into_response()
}

async fn on_demand(State(f): State<Arc<Fake>>, h: HeaderMap, Json(body): Json<Value>) -> Response {
    if !f.authed(&h) {
        return unauthorized();
    }
    Json(
        json!({ "enabled": body["enabled"], "limitMicros": body["limitMicros"], "usedMicros": 42 }),
    )
    .into_response()
}

async fn membership_usage(
    State(f): State<Arc<Fake>>,
    h: HeaderMap,
    RawQuery(q): RawQuery,
) -> Response {
    if !f.authed(&h) {
        return unauthorized();
    }
    Json(json!({
        "from": "2026-09-01T00:00:00Z", "to": "2026-10-01T00:00:00Z", "currency": "USD",
        "items": [], "totals": { "model": "", "requests": 0 }, "query": q
    }))
    .into_response()
}

async fn quote(State(f): State<Arc<Fake>>, h: HeaderMap, Json(body): Json<Value>) -> Response {
    if !f.authed(&h) {
        return unauthorized();
    }
    Json(json!({
        "planId": body["planId"], "interval": body["interval"], "mode": "upgrade",
        "listPriceMicros": 60_000_000, "creditMicros": 15_000_000, "amountMicros": 45_000_000,
        "refundMicros": 0, "balanceMicros": 1000, "currency": "USD"
    }))
    .into_response()
}

/// planId 99 → 402 余额不足(顶层附 balanceMicros / amountMicros,同 forge-cloud 的 `Extra`)。
async fn checkout(State(f): State<Arc<Fake>>, h: HeaderMap, Json(body): Json<Value>) -> Response {
    if !f.authed(&h) {
        return unauthorized();
    }
    if body["planId"] == 99 {
        return (
            StatusCode::PAYMENT_REQUIRED,
            Json(json!({
                "error": { "code": "INSUFFICIENT_BALANCE", "message": "余额不足" },
                "balanceMicros": 1000, "amountMicros": 20_000_000
            })),
        )
            .into_response();
    }
    f.checkout_calls.fetch_add(1, Ordering::SeqCst);
    Json(json!({
        "order": { "id": 31, "kind": "subscription", "provider": body["provider"], "status": "paid",
                   "amountMicros": 20_000_000, "planId": body["planId"], "interval": body["interval"] },
        "membership": membership_json(4_000_000)
    }))
    .into_response()
}

async fn cancel_scheduled(
    State(f): State<Arc<Fake>>,
    h: HeaderMap,
    Path(id): Path<String>,
) -> Response {
    if !f.authed(&h) {
        return unauthorized();
    }
    if id == "404" {
        return err(StatusCode::NOT_FOUND, "SUBSCRIPTION_NOT_FOUND");
    }
    Json(membership_json(9_000_000)).into_response()
}

async fn orders(State(f): State<Arc<Fake>>, h: HeaderMap, RawQuery(q): RawQuery) -> Response {
    if !f.authed(&h) {
        return unauthorized();
    }
    Json(json!({ "items": [], "total": 0, "query": q })).into_response()
}

async fn cancel_order(
    State(f): State<Arc<Fake>>,
    h: HeaderMap,
    Path(id): Path<String>,
) -> Response {
    if !f.authed(&h) {
        return unauthorized();
    }
    if id == "7" {
        return err(StatusCode::CONFLICT, "ORDER_NOT_PENDING");
    }
    Json(json!({ "id": id.parse::<i64>().unwrap(), "status": "cancelled" })).into_response()
}

async fn auth_config() -> Json<Value> {
    Json(json!({
        "registrationMode": "open", "requireEmailVerify": false, "smtpEnabled": false,
        "siteName": "RurixForge Cloud", "currency": "USD"
    }))
}

/// 运行中的假云端;drop 即停。
pub(crate) struct FakeCloud {
    pub url: String,
    pub state: Arc<Fake>,
    task: tokio::task::JoinHandle<()>,
}

impl FakeCloud {
    pub async fn start() -> Self {
        let state = Arc::new(Fake::default());
        let app = Router::new()
            .route(
                "/healthz",
                get(|| async { Json(json!({ "status": "ok" })) }),
            )
            .route("/api/v1/auth/config", get(auth_config))
            .route("/api/v1/auth/login", post(login))
            .route("/api/v1/auth/register", post(register))
            .route("/api/v1/auth/refresh", post(refresh))
            .route("/api/v1/auth/logout", post(logout))
            .route("/api/v1/me", get(me))
            .route("/api/v1/me/balance", get(balance))
            .route("/api/v1/me/profile", patch(profile))
            .route("/api/v1/me/password", post(password))
            .route("/api/v1/me/avatar", get(avatar).put(avatar_put))
            .route("/api/v1/me/devices/{id}", delete(device_delete))
            .route("/api/v1/me/api-keys", post(api_keys_post))
            .route("/api/v1/me/redeem", post(redeem))
            .route("/api/v1/me/usage", get(usage))
            .route("/api/v1/models/catalog", get(catalog))
            .route("/api/v1/tiers", get(tiers))
            .route("/api/v1/me/membership", get(membership))
            .route("/api/v1/me/membership/on-demand", patch(on_demand))
            .route("/api/v1/me/membership/usage", get(membership_usage))
            .route("/api/v1/me/membership/quote", post(quote))
            .route("/api/v1/me/membership/checkout", post(checkout))
            .route(
                "/api/v1/me/membership/scheduled/{id}",
                delete(cancel_scheduled),
            )
            .route("/api/v1/me/orders", get(orders))
            .route("/api/v1/me/orders/{id}/cancel", post(cancel_order))
            .with_state(Arc::clone(&state));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let task = tokio::spawn(async move {
            axum::serve(listener, app).await.ok();
        });
        FakeCloud { url, state, task }
    }

    /// 服务端吊销当前 access token(模拟过期),refresh token 仍有效。
    pub fn revoke_access(&self) {
        *self.state.access.lock().unwrap() = "server-side-revoked".into();
    }
}

impl Drop for FakeCloud {
    fn drop(&mut self) {
        self.task.abort();
    }
}

/// 指向假云端的内存态 CloudService。
pub(crate) fn service_for(fake: &FakeCloud) -> Arc<CloudService> {
    let svc = Arc::new(CloudService::new());
    svc.update_config(ConfigPatch {
        server_url: Some(fake.url.clone()),
        ..Default::default()
    })
    .unwrap();
    svc
}

pub(crate) async fn logged_in(fake: &FakeCloud) -> Arc<CloudService> {
    let svc = service_for(fake);
    svc.login("a@b.c", PASSWORD).await.expect("login");
    svc
}

fn secrets_absent(text: &str, fake: &FakeCloud) {
    assert!(!text.contains(DEVICE_KEY), "泄漏设备 Key: {text}");
    assert!(!text.contains("_secret"), "泄漏 refresh token: {text}");
    assert!(!text.contains("eyJ"), "泄漏 access token: {text}");
    let access = fake.state.access.lock().unwrap().clone();
    assert!(!text.contains(&access), "泄漏 access token: {text}");
}

#[tokio::test]
async fn login_stores_tokens_and_status_never_contains_them() {
    let fake = FakeCloud::start().await;
    let svc = service_for(&fake);
    assert!(!svc.is_logged_in());
    let gen0 = svc.login_generation();
    svc.login("a@b.c", PASSWORD).await.unwrap();
    assert!(svc.is_logged_in());
    assert_eq!(svc.login_generation(), gen0 + 1);
    assert_eq!(svc.device_key().as_deref(), Some(DEVICE_KEY));
    assert_eq!(
        svc.secret_for_test(DEVICE_KEY_ID).as_deref(),
        Some(DEVICE_KEY)
    );
    assert_eq!(
        svc.secret_for_test(REFRESH_KEY_ID).as_deref(),
        Some("rt_0_secret")
    );
    assert_eq!(svc.user_summary().unwrap()["email"], "a@b.c");
    assert_eq!(svc.current_session_id().as_deref(), Some("sess-1"));
    let login_body = fake.state.last_login.lock().unwrap().clone().unwrap();
    assert_eq!(login_body["device"]["platform"], config::platform());
    assert!(!login_body["device"]["name"].as_str().unwrap().is_empty());

    let status = svc.status().await;
    assert_eq!(status["loggedIn"], true);
    assert_eq!(status["reachable"], true);
    assert_eq!(status["deviceKeyPrefix"], "sk-rf-DEVI");
    assert_eq!(status["user"]["email"], "a@b.c");
    assert_eq!(status["serverUrl"], fake.url.as_str());
    assert!(status["sync"]["memory"].as_bool().unwrap());
    secrets_absent(&status.to_string(), &fake);
    secrets_absent(&svc.account_summary().to_string(), &fake);
    secrets_absent(&svc.config_json().to_string(), &fake);
}

#[tokio::test]
async fn wrong_password_passes_cloud_error_through() {
    let fake = FakeCloud::start().await;
    let svc = service_for(&fake);
    let e = svc.login("a@b.c", "nope").await.unwrap_err();
    assert_eq!((e.status, e.code.as_str()), (401, "INVALID_CREDENTIALS"));
    assert!(!svc.is_logged_in());
    let mut fields = serde_json::Map::new();
    fields.insert("email".into(), json!("taken@example.com"));
    fields.insert("password".into(), json!(PASSWORD));
    let e = svc.register(fields).await.unwrap_err();
    assert_eq!((e.status, e.code.as_str()), (409, "EMAIL_TAKEN"));
}

#[tokio::test]
async fn call_refreshes_once_on_401_and_retries() {
    let fake = FakeCloud::start().await;
    let svc = logged_in(&fake).await;
    fake.revoke_access();
    let me = svc.call("GET", "/api/v1/me", None).await.unwrap();
    assert_eq!(me["user"]["email"], "a@b.c");
    assert_eq!(fake.state.refresh_calls.load(Ordering::SeqCst), 1);
    // 轮换后的 refresh token 已落 keystore(旧的作废)。
    assert_eq!(
        svc.secret_for_test(REFRESH_KEY_ID).as_deref(),
        Some("rt_1_secret")
    );
    assert!(svc.is_logged_in());
}

#[tokio::test]
async fn expired_access_is_refreshed_before_the_call() {
    let fake = FakeCloud::start().await;
    let svc = logged_in(&fake).await;
    svc.expire_access_for_test();
    svc.call("GET", "/api/v1/me/balance", None).await.unwrap();
    assert_eq!(fake.state.refresh_calls.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn concurrent_401s_share_a_single_refresh() {
    let fake = FakeCloud::start().await;
    let svc = logged_in(&fake).await;
    fake.revoke_access();
    let calls = (0..6).map(|_| {
        let s = Arc::clone(&svc);
        tokio::spawn(async move { s.call("GET", "/api/v1/me", None).await })
    });
    for c in calls {
        c.await.unwrap().expect("并发调用都应在一次续期后成功");
    }
    assert_eq!(
        fake.state.refresh_calls.load(Ordering::SeqCst),
        1,
        "续期必须串行化,否则第二次会拿已轮换的旧 token 触发复用检测"
    );
}

#[tokio::test]
async fn refresh_failure_logs_out_locally() {
    let fake = FakeCloud::start().await;
    let svc = logged_in(&fake).await;
    let gen = svc.login_generation();
    fake.revoke_access();
    fake.state.fail_refresh.store(true, Ordering::SeqCst);
    let e = svc.call("GET", "/api/v1/me", None).await.unwrap_err();
    assert_eq!((e.status, e.code.as_str()), (401, "CLOUD_UNAUTHORIZED"));
    assert!(!svc.is_logged_in());
    assert!(svc.device_key().is_none());
    assert!(svc.secret_for_test(REFRESH_KEY_ID).is_none());
    assert!(svc.secret_for_test(DEVICE_KEY_ID).is_none());
    assert!(svc.login_generation() > gen);
    let status = svc.status_json();
    assert_eq!(status["loggedIn"], false);
    assert_eq!(status["lastError"]["code"], "CLOUD_UNAUTHORIZED");
    assert!(status["user"].is_null());
    let e = svc.call("GET", "/api/v1/me", None).await.unwrap_err();
    assert_eq!(e.code, "CLOUD_LOGIN_REQUIRED");
}

#[tokio::test]
async fn business_401_is_not_treated_as_token_expiry() {
    let fake = FakeCloud::start().await;
    let svc = logged_in(&fake).await;
    let e = svc
        .call(
            "POST",
            "/api/v1/me/password",
            Some(json!({ "oldPassword": "x", "newPassword": "y" })),
        )
        .await
        .unwrap_err();
    assert_eq!((e.status, e.code.as_str()), (401, "INVALID_CREDENTIALS"));
    assert_eq!(fake.state.refresh_calls.load(Ordering::SeqCst), 0);
    assert!(svc.is_logged_in());
}

#[tokio::test]
async fn catalog_is_cached_and_hidden_after_logout() {
    let fake = FakeCloud::start().await;
    let svc = service_for(&fake);
    assert!(svc.catalog().await.is_none(), "未登录无目录");
    svc.login("a@b.c", PASSWORD).await.unwrap();
    let c = svc.catalog().await.unwrap();
    assert_eq!(c.default_model, "gpt-5.5");
    assert_eq!(
        c.find("gpt-5.5").unwrap().capabilities.context_window,
        400_000
    );
    svc.catalog().await.unwrap();
    assert_eq!(
        fake.state.catalog_calls.load(Ordering::SeqCst),
        1,
        "5 分钟内命中缓存"
    );
    svc.fetch_catalog(true).await.unwrap();
    assert_eq!(
        fake.state.catalog_calls.load(Ordering::SeqCst),
        2,
        "force 绕过缓存"
    );
    assert!(svc.catalog_cached().is_some());
    svc.logout().await;
    assert_eq!(fake.state.logout_calls.load(Ordering::SeqCst), 1);
    assert!(svc.catalog_cached().is_none());
    assert!(
        svc.last_catalog().is_some(),
        "登出后保留目录作 needs-login 展示"
    );
}

#[tokio::test]
async fn logout_clears_tokens_even_when_cloud_is_down() {
    let fake = FakeCloud::start().await;
    let svc = logged_in(&fake).await;
    let gen = svc.login_generation();
    fake.task.abort();
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    svc.expire_access_for_test();
    svc.logout().await;
    assert!(!svc.is_logged_in());
    assert!(svc.secret_for_test(REFRESH_KEY_ID).is_none());
    assert!(svc.login_generation() > gen);
    assert!(!svc.probe_reachable().await);
    assert_eq!(svc.status_json()["reachable"], false);
}

#[tokio::test]
async fn balance_refresh_updates_cached_status() {
    let fake = FakeCloud::start().await;
    let svc = logged_in(&fake).await;
    assert_eq!(svc.status_json()["balanceMicros"], 1000);
    svc.refresh_balance_soon();
    for _ in 0..60 {
        if svc.status_json()["balanceMicros"] == 777 {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
    let st = svc.status_json();
    assert_eq!(st["balanceMicros"], 777);
    assert_eq!(st["subscriptions"][0]["planName"], "Pro");
    assert_eq!(svc.account_summary()["balanceMicros"], 777);
}

#[tokio::test]
async fn server_url_is_locked_while_logged_in() {
    let fake = FakeCloud::start().await;
    let svc = logged_in(&fake).await;
    let e = svc
        .update_config(ConfigPatch {
            server_url: Some("http://127.0.0.1:1".into()),
            ..Default::default()
        })
        .unwrap_err();
    assert_eq!((e.status, e.code.as_str()), (409, "LOGGED_IN"));
    // 同地址(含尾斜杠)不算修改;设备名与同步开关可改。
    let v = svc
        .update_config(ConfigPatch {
            server_url: Some(format!("{}/", fake.url)),
            device_name: Some(" my-pc ".into()),
            sync: Some(SyncPatch {
                memory: Some(false),
                ..Default::default()
            }),
        })
        .unwrap();
    assert_eq!(v["deviceName"], "my-pc");
    assert_eq!(v["sync"]["memory"], false);
    assert!(!svc.sync_enabled(SyncKind::Memory));
    assert!(svc.sync_enabled(SyncKind::Skills));
    let e = svc
        .update_config(ConfigPatch {
            server_url: Some("ftp://x".into()),
            ..Default::default()
        })
        .unwrap_err();
    assert_eq!(e.code, "INVALID_SERVER_URL");
}

#[tokio::test]
async fn disk_state_survives_reload_without_plaintext_tokens() {
    let fake = FakeCloud::start().await;
    let dir = std::env::temp_dir().join(format!(
        "agentd-cloud-disk-{}-{}",
        std::process::id(),
        crate::events::new_id("t")
    ));
    let paths = Paths {
        data_root: dir.join("agentd"),
        keystore: dir.join("gen").join("keystore.json"),
        gen_data: dir.join("gen"),
    };
    let svc = Arc::new(CloudService::load_from(paths.clone()));
    let device_id = svc.config_json()["deviceId"].as_str().unwrap().to_string();
    svc.update_config(ConfigPatch {
        server_url: Some(fake.url.clone()),
        ..Default::default()
    })
    .unwrap();
    svc.login("a@b.c", PASSWORD).await.unwrap();
    svc.fetch_catalog(false).await.unwrap();
    drop(svc);

    let again = CloudService::load_from(paths.clone());
    assert!(again.is_logged_in(), "重启后登录态应从 keystore 恢复");
    assert_eq!(again.device_key().as_deref(), Some(DEVICE_KEY));
    assert_eq!(
        again.config_json()["deviceId"],
        device_id.as_str(),
        "deviceId 只生成一次"
    );
    assert!(
        again.catalog_cached().is_some(),
        "目录缓存随 cloud-state.json 持久"
    );
    for f in ["agentd/cloud-state.json", "agentd/cloud-config.json"] {
        secrets_absent(&std::fs::read_to_string(dir.join(f)).unwrap(), &fake);
    }
    #[cfg(windows)]
    secrets_absent(&std::fs::read_to_string(&paths.keystore).unwrap(), &fake);
    // 目录里没有 embedding 模型 → 不写回落描述。
    assert!(!paths
        .gen_data
        .join(gend::embed::CLOUD_EMBEDDING_FILE)
        .exists());
    again.logout().await;
    let third = CloudService::load_from(paths);
    assert!(!third.is_logged_in());
    std::fs::remove_dir_all(&dir).ok();
}

// ---------- §11.4 会员面透传(路由级:rest::routes() + 隔离 AppState + 假云端) ----------

/// 路由级测试用 AppState:云端门面由调用方给(指向假云端),其余存贮落隔离临时目录。
fn account_app(cloud: Arc<CloudService>) -> (Router, std::path::PathBuf) {
    let dir = std::env::temp_dir().join(format!(
        "agentd-cloud-rest-{}-{}",
        std::process::id(),
        crate::events::new_id("t")
    ));
    let sessions = dir.join("agent-sessions");
    let state = Arc::new(crate::AppState {
        started: std::time::Instant::now(),
        proposals: crate::proposals::ProposalStore::default(),
        swarm: crate::swarm::SwarmCoordinator::default(),
        events: Arc::new(crate::events::EventBus::new(dir.join("agent-events"), 64)),
        sessions: Arc::new(crate::sessions::SessionStore::load(
            sessions.join("sessions.json"),
        )),
        folders: Arc::new(crate::sessions::ChatFolderStore::load(
            sessions.join("chat-folders.json"),
        )),
        workspaces: Arc::new(crate::workspaces::WorkspaceStore::load(
            sessions.join("workspaces.json"),
        )),
        runs: Arc::new(crate::agent::RunRegistry::default()),
        todos: Arc::new(crate::agent::TodoStore::load(sessions.join("todos.json"))),
        receipts: Arc::new(crate::receipts::ReceiptStore::load(
            sessions.join("receipts.json"),
        )),
        wakes: Arc::new(crate::agent::WakeRegistry::default()),
        collaboration: Arc::new(crate::collaboration::CollaborationStore::load(
            sessions.join("collaboration.json"),
        )),
        team_runtime: Arc::new(crate::collaboration_runtime::TeamRuntime::default()),
        permissions: Arc::new(crate::permission::PermissionService::load(
            sessions.join("permissions.json"),
        )),
        codex: Arc::new(crate::codex::service::CodexService::default()),
        goals: Arc::new(crate::goals::GoalStore::load(sessions.join("goals.json"))),
        cloud,
        memory: Arc::new(crate::memory::MemoryStore::ephemeral()),
        sync: Arc::new(sync::SyncStore::ephemeral()),
    });
    (rest::routes().with_state(state), dir)
}

/// 发一个请求,回 (状态码, JSON 体;空体 = Null)。
async fn hit(app: &Router, method: &str, uri: &str, body: Option<Value>) -> (StatusCode, Value) {
    use tower::ServiceExt;
    let builder = axum::http::Request::builder().method(method).uri(uri);
    let req = match body {
        Some(v) => builder
            .header("content-type", "application/json")
            .body(axum::body::Body::from(v.to_string())),
        None => builder.body(axum::body::Body::empty()),
    }
    .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    let status = resp.status();
    let bytes = axum::body::to_bytes(resp.into_body(), 1 << 20)
        .await
        .unwrap();
    let v = if bytes.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&bytes).unwrap()
    };
    (status, v)
}

/// 等后台余额重拉落地(refresh_balance_soon 先等 800ms 再拉 `/me/balance`,假云端回 777)。
async fn wait_balance(svc: &CloudService, want: i64) {
    for _ in 0..80 {
        if svc.status_json()["balanceMicros"] == want {
            return;
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
    panic!("余额未刷新到 {want}: {}", svc.status_json());
}

#[tokio::test]
async fn membership_routes_proxy_with_jwt_body_and_query() {
    let fake = FakeCloud::start().await;
    let svc = logged_in(&fake).await;
    let (app, dir) = account_app(Arc::clone(&svc));

    // 公开档位梯度。
    let (st, v) = hit(&app, "GET", "/api/forge/account/tiers", None).await;
    assert_eq!(st, StatusCode::OK);
    assert_eq!(v["items"][1]["tier"], "pro");
    assert_eq!(v["payment"]["enabled"], false);

    // JWT 由 agentd 注入(假云端校验 Authorization,不对就 401)。
    let (st, v) = hit(&app, "GET", "/api/forge/account/membership", None).await;
    assert_eq!(st, StatusCode::OK);
    assert_eq!(v["tier"]["tier"], "pro");
    assert_eq!(v["pools"]["api"]["remainingMicros"], 15_000_000);
    secrets_absent(&v.to_string(), &fake);

    // 请求体原样转发。
    let (st, v) = hit(
        &app,
        "PATCH",
        "/api/forge/account/membership/on-demand",
        Some(json!({ "enabled": false, "limitMicros": 5_000_000 })),
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    assert_eq!(
        v,
        json!({ "enabled": false, "limitMicros": 5_000_000, "usedMicros": 42 })
    );
    let (st, v) = hit(
        &app,
        "POST",
        "/api/forge/account/membership/quote",
        Some(json!({ "planId": 3, "interval": "year" })),
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    assert_eq!(
        (v["planId"].as_i64(), v["interval"].as_str()),
        (Some(3), Some("year"))
    );
    assert_eq!(v["amountMicros"], 45_000_000);

    // 查询串:usage 只转 from/to,orders 只转 limit/offset(其余键丢弃)。
    let (st, v) = hit(
        &app,
        "GET",
        "/api/forge/account/membership/usage?from=2026-09-01T00:00:00Z&to=2026-09-30T00:00:00Z&limit=5",
        None,
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    assert_eq!(
        v["query"],
        "from=2026-09-01T00%3A00%3A00Z&to=2026-09-30T00%3A00%3A00Z"
    );
    let (_, v) = hit(&app, "GET", "/api/forge/account/membership/usage", None).await;
    assert!(v["query"].is_null(), "缺省不带查询串 = 当前用量周期");
    let (st, v) = hit(
        &app,
        "GET",
        "/api/forge/account/orders?limit=20&offset=40&from=x",
        None,
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    assert_eq!(v["query"], "limit=20&offset=40");

    // 取消待支付单:不动余额缓存。
    let (st, v) = hit(
        &app,
        "POST",
        "/api/forge/account/orders/12/cancel",
        Some(json!({})),
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    assert_eq!(
        (v["id"].as_i64(), v["status"].as_str()),
        (Some(12), Some("cancelled"))
    );
    assert!(!svc.balance_stale());
    std::fs::remove_dir_all(&dir).ok();
}

#[tokio::test]
async fn checkout_absorbs_new_balance_and_invalidates_cached_status() {
    let fake = FakeCloud::start().await;
    let svc = logged_in(&fake).await;
    let (app, dir) = account_app(Arc::clone(&svc));
    assert_eq!(svc.status_json()["balanceMicros"], 1000);
    assert!(!svc.balance_stale(), "登录刚写过余额缓存");

    let (st, v) = hit(
        &app,
        "POST",
        "/api/forge/account/membership/checkout",
        Some(json!({ "planId": 2, "interval": "month", "provider": "balance" })),
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    assert_eq!(v["order"]["status"], "paid");
    assert_eq!(v["order"]["provider"], "balance");
    assert_eq!(fake.state.checkout_calls.load(Ordering::SeqCst), 1);
    // 回包里的新余额立即进缓存;订阅缓存判过期,后台重拉 /me/balance。
    assert_eq!(svc.status_json()["balanceMicros"], 4_000_000);
    assert_eq!(svc.account_summary()["balanceMicros"], 4_000_000);
    assert!(svc.balance_stale());
    wait_balance(&svc, 777).await;
    assert!(fake.state.balance_calls.load(Ordering::SeqCst) >= 1);
    assert_eq!(svc.status_json()["subscriptions"][0]["planName"], "Pro");
    std::fs::remove_dir_all(&dir).ok();
}

#[tokio::test]
async fn cancel_scheduled_refund_invalidates_cached_status() {
    let fake = FakeCloud::start().await;
    let svc = logged_in(&fake).await;
    let (app, dir) = account_app(Arc::clone(&svc));
    let (st, v) = hit(
        &app,
        "DELETE",
        "/api/forge/account/membership/scheduled/21",
        None,
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    assert_eq!(v["balanceMicros"], 9_000_000);
    assert_eq!(svc.status_json()["balanceMicros"], 9_000_000);
    assert!(svc.balance_stale());
    wait_balance(&svc, 777).await;

    // 业务错误连状态码原样透传,且不作废缓存。
    let (st, v) = hit(
        &app,
        "DELETE",
        "/api/forge/account/membership/scheduled/404",
        None,
    )
    .await;
    assert_eq!(st, StatusCode::NOT_FOUND);
    assert_eq!(v["error"]["code"], "SUBSCRIPTION_NOT_FOUND");
    assert!(!svc.balance_stale());
    let (st, v) = hit(&app, "POST", "/api/forge/account/orders/7/cancel", None).await;
    assert_eq!(st, StatusCode::CONFLICT);
    assert_eq!(v["error"]["code"], "ORDER_NOT_PENDING");
    std::fs::remove_dir_all(&dir).ok();
}

#[tokio::test]
async fn insufficient_balance_402_keeps_top_level_amounts() {
    let fake = FakeCloud::start().await;
    let svc = logged_in(&fake).await;
    let (app, dir) = account_app(Arc::clone(&svc));
    let (st, v) = hit(
        &app,
        "POST",
        "/api/forge/account/membership/checkout",
        Some(json!({ "planId": 99, "interval": "month", "provider": "balance" })),
    )
    .await;
    assert_eq!(st, StatusCode::PAYMENT_REQUIRED);
    assert_eq!(v["error"]["code"], "INSUFFICIENT_BALANCE");
    assert_eq!(v["error"]["message"], "余额不足");
    assert_eq!(v["balanceMicros"], 1000);
    assert_eq!(v["amountMicros"], 20_000_000);
    assert_eq!(fake.state.checkout_calls.load(Ordering::SeqCst), 0);
    // 失败的购买不动缓存。
    assert_eq!(svc.status_json()["balanceMicros"], 1000);
    assert!(!svc.balance_stale());
    std::fs::remove_dir_all(&dir).ok();
}

#[tokio::test]
async fn membership_routes_require_login_and_numeric_ids() {
    let fake = FakeCloud::start().await;
    let svc = service_for(&fake);
    let (app, dir) = account_app(Arc::clone(&svc));
    // 档位梯度公开:未登录也能看。
    let (st, v) = hit(&app, "GET", "/api/forge/account/tiers", None).await;
    assert_eq!(st, StatusCode::OK);
    assert_eq!(v["items"][0]["tier"], "hobby");
    for (method, uri) in [
        ("GET", "/api/forge/account/membership"),
        ("GET", "/api/forge/account/membership/usage"),
        ("GET", "/api/forge/account/orders"),
        ("POST", "/api/forge/account/orders/1/cancel"),
        ("DELETE", "/api/forge/account/membership/scheduled/1"),
    ] {
        let (st, v) = hit(&app, method, uri, None).await;
        assert_eq!(st, StatusCode::UNAUTHORIZED, "{method} {uri}");
        assert_eq!(v["error"]["code"], "CLOUD_LOGIN_REQUIRED", "{method} {uri}");
    }
    let (st, v) = hit(
        &app,
        "POST",
        "/api/forge/account/membership/checkout",
        Some(json!({ "planId": 2, "interval": "month", "provider": "balance" })),
    )
    .await;
    assert_eq!(
        (st, v["error"]["code"].as_str()),
        (StatusCode::UNAUTHORIZED, Some("CLOUD_LOGIN_REQUIRED"))
    );

    // 已登录也只放行纯数字 ID:编码过的路径片段不拼进云端 URL。
    svc.login("a@b.c", PASSWORD).await.unwrap();
    for uri in [
        "/api/forge/account/orders/abc/cancel",
        "/api/forge/account/orders/1%2F..%2F..%2Fadmin/cancel",
        "/api/forge/account/membership/scheduled/-1",
    ] {
        let method = if uri.contains("/orders/") {
            "POST"
        } else {
            "DELETE"
        };
        let (st, v) = hit(&app, method, uri, None).await;
        assert_eq!(st, StatusCode::BAD_REQUEST, "{uri}");
        assert_eq!(v["error"]["code"], "INVALID_INPUT", "{uri}");
    }
    std::fs::remove_dir_all(&dir).ok();
}
