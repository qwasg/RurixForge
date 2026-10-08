//! Website-initiated Google OAuth and a private, supervised subscription adapter.
//! Only account/quota/model metadata crosses the Forge API; tokens stay DPAPI-encrypted.
use axum::{
    extract::{Query, State},
    http::StatusCode,
    response::{Html, IntoResponse, Response},
    routing::get,
    Json, Router,
};
use serde::Deserialize;
use serde_json::{json, Value};
use std::{
    collections::HashMap,
    future::IntoFuture,
    path::PathBuf,
    process::Stdio,
    sync::{Arc, Mutex, OnceLock},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

static GATE: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
const CALLBACK_PORT: u16 = 51121;
const LOGIN_TTL: Duration = Duration::from_secs(300);

struct Adapter {
    child: tokio::process::Child,
    base: String,
    api_key: String,
    management_key: String,
}
#[derive(Default)]
struct Channel {
    adapter: Option<Adapter>,
    ready: bool,
    login: Value,
    callback: Option<tokio::task::JoinHandle<()>>,
    login_started: Option<Instant>,
    quota: Option<(Instant, Value)>,
    models: Vec<Value>,
    installing: bool,
    install_error: Option<String>,
    callback_used: bool,
}
fn states() -> &'static Mutex<HashMap<PathBuf, Channel>> {
    static STATES: OnceLock<Mutex<HashMap<PathBuf, Channel>>> = OnceLock::new();
    STATES.get_or_init(|| Mutex::new(HashMap::new()))
}
fn root() -> PathBuf {
    crate::agent_data_root()
}
fn home() -> PathBuf {
    root().join("antigravity-home")
}
fn executable() -> PathBuf {
    if let Some(path) = std::env::var_os("FORGE_ANTIGRAVITY_BRIDGE_BIN") {
        return path.into();
    }
    let name = if cfg!(windows) {
        "forge-antigravity-bridge.exe"
    } else {
        "forge-antigravity-bridge"
    };
    let local = root().join("antigravity-runtime").join(name);
    if local.is_file() {
        return local;
    }
    // Workspace builds share the binary, while each daemon keeps its own credentials.
    crate::workspace_root()
        .join("data/antigravity-runtime")
        .join(name)
}
fn has_credentials() -> bool {
    std::fs::read_dir(home().join("credentials"))
        .ok()
        .is_some_and(|entries| {
            entries.flatten().any(|entry| {
                entry
                    .path()
                    .extension()
                    .is_some_and(|ext| ext == "credential")
            })
        })
}
fn start_install() {
    let data_root = root();
    {
        let mut map = states().lock().unwrap();
        let state = map.entry(data_root.clone()).or_default();
        if state.installing {
            return;
        }
        state.installing = true;
        state.install_error = None;
    }
    tokio::spawn(async move {
        let task_root = data_root.clone();
        let result = tokio::task::spawn_blocking(move || {
            let runtime_dir = task_root.join("antigravity-runtime");
            let source = runtime_dir.join("build");
            std::fs::create_dir_all(&source).map_err(|_| "无法准备反重力连接组件")?;
            for (name, contents) in [
                (
                    "go.mod",
                    include_str!("../../../tools/antigravity-bridge/go.mod"),
                ),
                (
                    "go.sum",
                    include_str!("../../../tools/antigravity-bridge/go.sum"),
                ),
                (
                    "main.go",
                    include_str!("../../../tools/antigravity-bridge/main.go"),
                ),
                (
                    "store.go",
                    include_str!("../../../tools/antigravity-bridge/store.go"),
                ),
                (
                    "models.go",
                    include_str!("../../../tools/antigravity-bridge/models.go"),
                ),
                (
                    "protect_windows.go",
                    include_str!("../../../tools/antigravity-bridge/protect_windows.go"),
                ),
                (
                    "protect_other.go",
                    include_str!("../../../tools/antigravity-bridge/protect_other.go"),
                ),
            ] {
                std::fs::write(source.join(name), contents)
                    .map_err(|_| "无法准备反重力连接组件")?;
            }
            let mut cmd = std::process::Command::new("go");
            cmd.current_dir(source)
                .args([
                    "build",
                    "-mod=readonly",
                    "-trimpath",
                    "-ldflags=-s -w",
                    "-o",
                ])
                .arg(runtime_dir.join(if cfg!(windows) {
                    "forge-antigravity-bridge.exe"
                } else {
                    "forge-antigravity-bridge"
                }))
                .arg(".")
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null());
            #[cfg(windows)]
            {
                use std::os::windows::process::CommandExt;
                cmd.creation_flags(0x08000000);
            }
            let output = cmd
                .status()
                .map_err(|_| "需要 Go 1.26 或更新版本来准备反重力连接组件")?;
            if !output.success() {
                return Err("反重力连接组件构建失败，请检查 Go 1.26 与网络连接");
            }
            Ok(())
        })
        .await;
        let mut map = states().lock().unwrap();
        let state = map.entry(data_root).or_default();
        state.installing = false;
        state.install_error = match result {
            Ok(Ok(())) => None,
            Ok(Err(msg)) => Some(msg.to_string()),
            Err(_) => Some("反重力连接组件准备失败，请重试".into()),
        };
    });
}
fn secret() -> Result<String, String> {
    let mut bytes = [0u8; 32];
    getrandom::fill(&mut bytes).map_err(|_| "本机授权初始化失败")?;
    Ok(bytes.iter().map(|b| format!("{b:02x}")).collect())
}
fn reply_error(message: &str) -> Response {
    (
        StatusCode::BAD_GATEWAY,
        Json(json!({"error":{"code":"ANTIGRAVITY_AUTH","message":message}})),
    )
        .into_response()
}
fn runtime() -> Option<(String, String, String)> {
    let mut map = states().lock().unwrap();
    let state = map.get_mut(&root())?;
    let adapter = state.adapter.as_mut()?;
    if !matches!(adapter.child.try_wait(), Ok(None)) {
        state.ready = false;
        state.adapter = None;
        return None;
    }
    Some((
        adapter.base.clone(),
        adapter.api_key.clone(),
        adapter.management_key.clone(),
    ))
}
pub(crate) fn resolve() -> Option<(String, String, String)> {
    let ready = states()
        .lock()
        .unwrap()
        .get(&root())
        .is_some_and(|s| s.ready);
    if !ready {
        return None;
    }
    let (base, key, _) = runtime()?;
    let model = states()
        .lock()
        .unwrap()
        .get(&root())
        .and_then(|s| s.models.first())
        .and_then(|v| v["upstreamId"].as_str())
        .unwrap_or(crate::antigravity::DEFAULT_ANTIGRAVITY_MODEL)
        .to_string();
    Some((base, model, key))
}
pub(crate) fn ready() -> bool {
    resolve().is_some()
}
pub(crate) fn models() -> Vec<Value> {
    if !ready() {
        return vec![];
    }
    states()
        .lock()
        .unwrap()
        .get(&root())
        .map(|s| s.models.clone())
        .unwrap_or_default()
}
pub(crate) fn prewarm() {
    if has_credentials() && executable().is_file() {
        tokio::spawn(async {
            let _ = status(false).await;
        });
    }
}
async fn call(
    method: &'static str,
    path: String,
    body: Option<Value>,
    api: bool,
) -> Result<Value, String> {
    let (base, key, management) = runtime().ok_or("本机连接服务未启动")?;
    tokio::task::spawn_blocking(move || {
        let agent = ureq::AgentBuilder::new()
            .timeout(Duration::from_secs(25))
            .redirects(0)
            .build();
        let req = agent
            .request(method, &format!("{base}{path}"))
            .set(
                "Authorization",
                &format!("Bearer {}", if api { key } else { management }),
            )
            .set("Content-Type", "application/json");
        let resp = if let Some(body) = body {
            req.send_string(&body.to_string())
        } else {
            req.call()
        }
        .map_err(|_| "本机连接服务请求失败".to_string())?;
        let text = resp
            .into_string()
            .map_err(|_| "本机连接服务响应无效".to_string())?;
        serde_json::from_str(&text).map_err(|_| "本机连接服务响应无效".to_string())
    })
    .await
    .map_err(|_| "本机连接服务请求失败".to_string())?
}
async fn ensure_runtime() -> Result<(), String> {
    if runtime().is_some() {
        return Ok(());
    }
    if !executable().is_file() {
        return Err("反重力连接组件尚未构建，请运行 pnpm antigravity:build 后重试".into());
    }
    let listener = std::net::TcpListener::bind("127.0.0.1:0").map_err(|_| "本机端口初始化失败")?;
    let port = listener
        .local_addr()
        .map_err(|_| "本机端口初始化失败")?
        .port();
    drop(listener);
    let api_key = secret()?;
    let management_key = secret()?;
    let mut cmd = tokio::process::Command::new(executable());
    cmd.env("FORGE_ANTIGRAVITY_BRIDGE_PORT", port.to_string())
        .env("FORGE_ANTIGRAVITY_BRIDGE_KEY", &api_key)
        .env("FORGE_ANTIGRAVITY_MANAGEMENT_KEY", &management_key)
        .env("FORGE_ANTIGRAVITY_BRIDGE_HOME", home());
    cmd.stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .kill_on_drop(true);
    #[cfg(windows)]
    cmd.creation_flags(0x08000000);
    let child = cmd.spawn().map_err(|_| "本机连接服务启动失败")?;
    states().lock().unwrap().entry(root()).or_default().adapter = Some(Adapter {
        child,
        base: format!("http://127.0.0.1:{port}"),
        api_key,
        management_key,
    });
    for _ in 0..100 {
        if call("GET", "/forge/account".into(), None, false)
            .await
            .is_ok()
        {
            return Ok(());
        }
        if runtime().is_none() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    states().lock().unwrap().entry(root()).or_default().adapter = None;
    Err("本机连接服务启动失败，请重新授权".into())
}

#[derive(Clone)]
struct Callback {
    state: String,
    data_root: PathBuf,
}
#[derive(Deserialize)]
struct CallbackQuery {
    state: Option<String>,
    code: Option<String>,
    error: Option<String>,
}
async fn receive_callback(
    State(callback): State<Arc<Callback>>,
    Query(query): Query<CallbackQuery>,
) -> Response {
    let matches = {
        let mut map = states().lock().unwrap();
        map.get_mut(&callback.data_root).is_some_and(|s| {
            let valid = query.state.as_deref() == Some(&callback.state)
                && !s.callback_used
                && (query.code.as_ref().is_some_and(|c| !c.is_empty()) || query.error.is_some())
                && s.login["state"] == "pending"
                && s.login["loginId"] == callback.state
                && s.login_started.is_some_and(|at| at.elapsed() < LOGIN_TTL);
            if valid {
                s.callback_used = true;
            }
            valid
        })
    };
    if !matches {
        return (
            StatusCode::BAD_REQUEST,
            Html("授权已失效，请返回 Forge 重新点击 Google 网页授权。"),
        )
            .into_response();
    }
    let payload = json!({"provider":"antigravity","state":callback.state,"code":query.code,"error":query.error});
    match call("POST", "/v0/management/oauth-callback".into(), Some(payload), false).await {
        Ok(_) => ([("Cache-Control", "no-store"), ("Content-Security-Policy", "default-src 'none'; style-src 'unsafe-inline'")], Html("<!doctype html><html lang=\"zh-CN\"><meta charset=\"utf-8\"><title>Google 授权 · Forge</title><body style=\"font:16px system-ui;background:#111827;color:#f8fafc;padding:56px\"><h1>已收到 Google 授权</h1><p>正在完成账户连接。请返回 Forge，卡片会自动同步登录结果。</p></body></html>")).into_response(),
        Err(_) => (StatusCode::BAD_GATEWAY, Html("授权回调未完成，请返回 Forge 重试。")).into_response(),
    }
}
async fn cancel_inner() -> Result<(), String> {
    let (id, callback) = {
        let mut map = states().lock().unwrap();
        let state = map.entry(root()).or_default();
        let id = state.login["loginId"].as_str().map(str::to_string);
        state.login = json!({"state":"cancelled"});
        state.login_started = None;
        (id, state.callback.take())
    };
    if let Some(task) = callback {
        task.abort();
        let _ = task.await;
    }
    if let Some(id) = id {
        call("POST", "/forge/cancel-login".into(), Some(json!({})), false).await?;
        let _ = call(
            "DELETE",
            format!("/v0/management/oauth-session?state={id}"),
            None,
            false,
        )
        .await;
        let mut map = states().lock().unwrap();
        let state = map.entry(root()).or_default();
        state.ready = false;
        state.adapter = None;
    }
    Ok(())
}
pub(crate) async fn login() -> Response {
    let _gate = GATE.lock().await;
    if !executable().is_file() {
        start_install();
        return Json(json!({"state":"installing"})).into_response();
    }
    if let Err(message) = ensure_runtime().await {
        return reply_error(&message);
    }
    if let Err(message) = cancel_inner().await {
        return reply_error(&message);
    }
    if let Err(message) = ensure_runtime().await {
        return reply_error(&message);
    }
    let listener =
        match tokio::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, CALLBACK_PORT)).await {
            Ok(listener) => listener,
            Err(_) => {
                return reply_error(
                    "Google 授权回调端口 51121 已被占用，请结束其他反重力授权后重试",
                )
            }
        };
    if let Err(msg) = call("POST", "/forge/begin-login".into(), Some(json!({})), false).await {
        return reply_error(&msg);
    }
    let result = match call(
        "GET",
        "/v0/management/antigravity-auth-url".into(),
        None,
        false,
    )
    .await
    {
        Ok(result) => result,
        Err(message) => return reply_error(&message),
    };
    let id = result["state"].as_str().unwrap_or("").to_string();
    let url = result["url"].as_str().unwrap_or("");
    if id.is_empty()
        || !id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
        || !valid_google_url(url)
    {
        return reply_error("Google 官方授权地址校验失败");
    }
    let login = json!({"state":"pending","authUrl":url,"loginId":id});
    let app = Router::new()
        .route("/oauth-callback", get(receive_callback))
        .with_state(Arc::new(Callback {
            state: id.clone(),
            data_root: root(),
        }));
    let task_root = root();
    let task_id = id.clone();
    let task = tokio::spawn(async move {
        let _ = tokio::time::timeout(LOGIN_TTL, axum::serve(listener, app).into_future()).await;
        let _gate = GATE.lock().await;
        let expired = {
            let mut map = states().lock().unwrap();
            map.get_mut(&task_root).is_some_and(|state| {
                if state.login["loginId"] != task_id || state.login["state"] != "pending" {
                    return false;
                }
                state.login["state"] = json!("failed");
                state.login["error"] = json!("Google 授权已超时，请重试");
                true
            })
        };
        if expired {
            let _ = call("POST", "/forge/cancel-login".into(), Some(json!({})), false).await;
            let _ = call(
                "DELETE",
                format!("/v0/management/oauth-session?state={task_id}"),
                None,
                false,
            )
            .await;
            let mut map = states().lock().unwrap();
            let state = map.entry(task_root).or_default();
            state.ready = false;
            state.adapter = None;
        }
    });
    let mut map = states().lock().unwrap();
    let state = map.entry(root()).or_default();
    state.login = login.clone();
    state.login_started = Some(Instant::now());
    state.callback_used = false;
    state.callback = Some(task);
    Json(login).into_response()
}
fn valid_google_url(raw: &str) -> bool {
    // The SDK emits Google's exact OAuth origin, never a third-party credential page.
    raw.starts_with("https://accounts.google.com/o/oauth2/") && !raw.contains(['\r', '\n'])
}
pub(crate) async fn cancel() -> Response {
    let _gate = GATE.lock().await;
    match cancel_inner().await {
        Ok(()) => Json(json!({"ok":true})).into_response(),
        Err(msg) => reply_error(&msg),
    }
}
pub(crate) async fn logout() -> Response {
    let _gate = GATE.lock().await;
    if let Err(msg) = ensure_runtime().await {
        return reply_error(&msg);
    }
    if let Err(msg) = cancel_inner().await {
        return reply_error(&msg);
    }
    if let Err(msg) = ensure_runtime().await {
        return reply_error(&msg);
    }
    match call("POST", "/forge/logout".into(), Some(json!({})), false).await {
        Ok(_) => {
            let mut map = states().lock().unwrap();
            let state = map.entry(root()).or_default();
            state.ready = false;
            state.quota = None;
            state.models.clear();
            // Drop the supervised adapter so its model/signature registry cannot retain this account.
            state.adapter = None;
            Json(json!({"ok":true})).into_response()
        }
        Err(msg) => reply_error(&msg),
    }
}
fn base_status() -> Value {
    json!({"id":"antigravity","installed":executable().is_file(),"configured":false,"authMode":null,"account":null,"login":{"state":"idle"},"install":{"running":false},"modelId":"antigravity","model":"Google AI 订阅模型","models":[],"quota":{"state":"unknown","windows":[]},"quotaUrl":"https://antigravity.google/docs/plans/"})
}
pub(crate) async fn status(refresh: bool) -> Value {
    let mut body = base_status();
    {
        let map = states().lock().unwrap();
        if let Some(state) = map.get(&root()) {
            body["install"] = json!({"running":state.installing,"error":state.install_error});
            if !state.login.is_null() {
                body["login"] = state.login.clone();
            }
        }
    }
    let should_start = runtime().is_some() || has_credentials();
    if !should_start {
        return body;
    }
    {
        let _gate = GATE.lock().await;
        if let Err(msg) = ensure_runtime().await {
            body["error"] = json!(msg);
            return body;
        }
    }
    let login_id = states().lock().unwrap().get(&root()).and_then(|s| {
        if s.login["state"] == "pending" {
            s.login["loginId"].as_str().map(str::to_string)
        } else {
            None
        }
    });
    if let Some(id) = login_id {
        if let Ok(result) = call(
            "GET",
            format!("/v0/management/get-auth-status?state={id}"),
            None,
            false,
        )
        .await
        {
            let mut map = states().lock().unwrap();
            let state = map.entry(root()).or_default();
            if state.login["loginId"] == id && state.login["state"] == "pending" {
                match result["status"].as_str() {
                    Some("ok") => state.login = json!({"state":"authenticated"}),
                    Some("error") => {
                        state.login["state"] = json!("failed");
                        state.login["error"] = json!("Google 授权未完成，请重新授权");
                    }
                    _ => {}
                }
                if state.login["state"] != "pending" {
                    if let Some(task) = state.callback.take() {
                        task.abort();
                    }
                }
            }
        }
        let failed =
            states().lock().unwrap().get(&root()).is_some_and(|state| {
                state.login["loginId"] == id && state.login["state"] == "failed"
            });
        if failed {
            let _gate = GATE.lock().await;
            let current = states().lock().unwrap().get(&root()).is_some_and(|state| {
                state.login["loginId"] == id && state.login["state"] == "failed"
            });
            if current {
                let _ = cancel_inner().await;
                let failure = json!({"state":"failed","error":"Google 授权未完成，请重新授权"});
                states().lock().unwrap().entry(root()).or_default().login = failure.clone();
                body["login"] = failure;
                return body;
            }
        }
    }
    match call("GET", "/forge/account".into(), None, false).await {
        Ok(account) => {
            let configured = account["configured"] == true;
            {
                let mut map = states().lock().unwrap();
                let state = map.entry(root()).or_default();
                state.ready = configured;
                body["login"] = state.login.clone();
            }
            body["configured"] = json!(configured);
            if configured {
                let _ = call(
                    "POST",
                    "/forge/complete-login".into(),
                    Some(json!({})),
                    false,
                )
                .await;
                body["authMode"] = json!("googleOAuth");
                body["account"] = json!({"email":account["email"]});
            }
            if configured {
                if let Ok(catalog) = call("GET", "/v1/models".into(), None, true).await {
                    let mut models: Vec<Value> = catalog["data"].as_array().into_iter().flatten().filter_map(|model| {
                        let id = model["id"].as_str()?;
                        Some(json!({"id":format!("antigravity:{id}"),"upstreamId":id,"label":model["display_name"].as_str().unwrap_or(id)}))
                    }).collect();
                    models.sort_by_key(|v| {
                        (
                            !v["upstreamId"].as_str().unwrap_or("").contains("flash"),
                            v["upstreamId"].as_str().unwrap_or("").to_string(),
                        )
                    });
                    states().lock().unwrap().entry(root()).or_default().models = models;
                }
                let quota = quota(&account, refresh).await;
                if let Some(plan) = quota["plan"].as_str() {
                    body["account"]["planType"] = json!(plan);
                }
                body["quota"] = quota;
                let models = models();
                body["models"] = json!(models);
                if let Some(model) = models.first() {
                    body["modelId"] = model["id"].clone();
                    body["model"] = model["label"].clone();
                }
            }
        }
        Err(msg) => {
            states().lock().unwrap().entry(root()).or_default().ready = false;
            body["error"] = json!(msg);
        }
    }
    body
}
async fn quota(account: &Value, refresh: bool) -> Value {
    if !refresh {
        if let Some((at, value)) = states()
            .lock()
            .unwrap()
            .get(&root())
            .and_then(|s| s.quota.clone())
        {
            if at.elapsed() < Duration::from_secs(60) {
                return value;
            }
        }
    }
    let payload = json!({"auth_index":account["authIndex"],"method":"POST","url":"https://cloudcode-pa.googleapis.com/v1internal:fetchAvailableModels","header":{"Authorization":"Bearer $TOKEN$","Content-Type":"application/json","User-Agent":account["userAgent"].as_str().unwrap_or("antigravity")},"data":json!({"project":account["projectId"]}).to_string()});
    let response = call(
        "POST",
        "/v0/management/api-call".into(),
        Some(payload),
        false,
    )
    .await
    .ok();
    let data = response
        .filter(|response| response["status_code"] == 200)
        .and_then(|response| {
            response["body"]
                .as_str()
                .and_then(|s| serde_json::from_str::<Value>(s).ok())
        });
    let value = data.as_ref().map(quota_windows).unwrap_or_default();
    // Restore/register the actual account catalog in the SDK, including after restart.
    let catalog: Vec<Value> = if let Some(models) =
        data.as_ref().and_then(|v| v["models"].as_object())
    {
        models.iter().map(|(id, model)| json!({"id":format!("antigravity:{id}"),"upstreamId":id,"label":model["displayName"].as_str().unwrap_or(id)})).collect()
    } else {
        states()
            .lock()
            .unwrap()
            .get(&root())
            .map(|s| s.models.clone())
            .unwrap_or_default()
    };
    if !catalog.is_empty()
        && call(
            "POST",
            "/forge/models".into(),
            Some(json!({"authIndex":account["authIndex"],"models":catalog})),
            false,
        )
        .await
        .is_ok()
    {
        let mut catalog = catalog;
        catalog.sort_by_key(|v| {
            (
                !v["upstreamId"].as_str().unwrap_or("").contains("flash"),
                v["upstreamId"].as_str().unwrap_or("").to_string(),
            )
        });
        states().lock().unwrap().entry(root()).or_default().models = catalog;
    }
    let result = json!({"state":if value.is_empty(){"unavailable"}else{"available"},"windows":value,"source":"Google Antigravity 官方额度","updatedAt":SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_millis() as u64,"error":if value.is_empty(){Some("官方额度暂时无法读取，请稍后刷新")}else{None}});
    states().lock().unwrap().entry(root()).or_default().quota =
        Some((Instant::now(), result.clone()));
    result
}
fn quota_windows(value: &Value) -> Vec<Value> {
    let mut windows = vec![];
    if let Some(models) = value["models"].as_object() {
        for (id, model) in models {
            let Some(left) = model["quotaInfo"]["remainingFraction"]
                .as_f64()
                .filter(|f| f.is_finite())
            else {
                continue;
            };
            let mut window = json!({"id":id,"label":model["displayName"].as_str().unwrap_or(id),"usedPercent":100.0-left.clamp(0.0,1.0)*100.0});
            if let Some(reset) = model["quotaInfo"]["resetTime"]
                .as_str()
                .filter(|value| !value.is_empty())
            {
                window["resetsAt"] = json!(reset);
            }
            windows.push(window);
        }
    }
    windows.sort_by_key(|v| {
        (
            !v["id"].as_str().unwrap_or("").contains("flash"),
            v["id"].as_str().unwrap_or("").to_string(),
        )
    });
    windows
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn google_origin_validation() {
        assert!(valid_google_url(
            "https://accounts.google.com/o/oauth2/v2/auth?state=abc"
        ));
        for url in [
            "https://accounts.google.com.evil.test/o/oauth2/auth",
            "https://accounts.google.com@evil.test/o/oauth2/auth",
            "http://accounts.google.com/o/oauth2/auth",
            "https://accounts.google.com:443/o/oauth2/auth",
        ] {
            assert!(!valid_google_url(url));
        }
    }
    #[test]
    fn quota_uses_official_model_windows_and_unknown_stays_unknown() {
        let windows = quota_windows(
            &json!({"models":{"gemini-flash":{"displayName":"Gemini Flash","quotaInfo":{"remainingFraction":0.7,"resetTime":"2026-10-07T12:00:00Z"}},"claude":{"quotaInfo":{"remainingFraction":0.0}},"unknown":{},"over":{"quotaInfo":{"remainingFraction":2.0}}}}),
        );
        assert_eq!(windows.len(), 3);
        assert_eq!(windows[0]["usedPercent"], 30.0);
        assert_eq!(windows[0]["resetsAt"], "2026-10-07T12:00:00Z");
        assert_eq!(quota_windows(&json!({})).len(), 0);
    }
}
