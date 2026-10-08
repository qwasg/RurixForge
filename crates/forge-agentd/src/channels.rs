//! Official subscription channels. Secrets stay in CLI storage / the encrypted keystore.
use axum::extract::{Path, Query};
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::path::PathBuf;
use std::process::Stdio;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

use crate::llm::{self, LlmError, RequestSpec, StepFn, StreamDelta, StreamSink};

const KIMI_VERSION: &str = "1.52.0";
const GLM_CHAT_URL: &str = "https://open.bigmodel.cn/api/coding/paas/v4/chat/completions";
pub const GLM_CONSOLE: &str = "https://bigmodel.cn/coding-plan/personal/overview";
const BRIDGE: &str = include_str!("kimi_bridge.py");
static LOGIN_GATE: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
static CONFIG_GATE: Mutex<()> = Mutex::new(());

#[derive(Default)]
struct RuntimeState {
    installing: bool,
    install_error: Option<String>,
    login: Value,
    task: Option<tokio::task::JoinHandle<()>>,
    ready: bool,
    quota: Option<(Instant, Value)>,
    glm_quota: Option<(Instant, Value)>,
}
fn states() -> &'static Mutex<HashMap<PathBuf, RuntimeState>> {
    static STATES: OnceLock<Mutex<HashMap<PathBuf, RuntimeState>>> = OnceLock::new();
    STATES.get_or_init(|| Mutex::new(HashMap::new()))
}
fn root() -> PathBuf {
    crate::agent_data_root()
}
fn python() -> PathBuf {
    root().join("kimi-runtime").join(if cfg!(windows) {
        "Scripts/python.exe"
    } else {
        "bin/python"
    })
}
fn installed() -> bool {
    let packages = root().join("kimi-runtime").join(if cfg!(windows) {
        "Lib/site-packages/kimi_cli/__init__.py"
    } else {
        "lib/python3.13/site-packages/kimi_cli/__init__.py"
    });
    python().is_file() && packages.is_file()
}
pub(crate) fn ready(channel: &str) -> bool {
    match channel {
        "antigravity" => crate::antigravity_oauth::ready(),
        "glm" => glm_key().is_some(),
        "kimi" => {
            states()
                .lock()
                .unwrap()
                .get(&root())
                .is_some_and(|s| s.ready)
                && installed()
        }
        _ => false,
    }
}
fn glm_key() -> Option<String> {
    gend::keystore::Keystore::load().secret_for("channel:glm")
}
fn glm_model() -> String {
    std::fs::read(root().join("official-channels.json"))
        .ok()
        .and_then(|bytes| serde_json::from_slice::<Value>(&bytes).ok())
        .and_then(|v| v["glmModel"].as_str().map(str::to_string))
        .unwrap_or_else(|| "glm-5.3".to_string())
}
pub(crate) fn model_label(channel: &str) -> String {
    if channel == "glm" {
        glm_model()
    } else {
        "Kimi Code".to_string()
    }
}

pub(crate) fn prewarm() {
    if installed() {
        tokio::spawn(async { let _ = channel_status("kimi", false).await; });
    }
}
fn error(status: axum::http::StatusCode, code: &str, message: &str) -> Response {
    (
        status,
        Json(json!({"error":{"code":code,"message":message}})),
    )
        .into_response()
}
fn upstream(message: &str) -> Response {
    error(
        axum::http::StatusCode::BAD_GATEWAY,
        "CHANNEL_UPSTREAM",
        message,
    )
}
fn bridge_command(mode: &str) -> tokio::process::Command {
    let mut command = tokio::process::Command::new(python());
    command
        .args(["-u", "-c", BRIDGE, mode])
        .env("KIMI_SHARE_DIR", root().join("kimi-home"))
        .env("PYTHONIOENCODING", "utf-8")
        .env_remove("KIMI_BASE_URL")
        .env_remove("KIMI_API_KEY")
        .env_remove("KIMI_MODEL_NAME")
        .env_remove("KIMI_CODE_BASE_URL")
        .env_remove("KIMI_CODE_OAUTH_HOST")
        .env_remove("KIMI_OAUTH_HOST")
        .env_remove("KIMI_CONFIG_FILE")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true);
    #[cfg(windows)]
    command.creation_flags(0x08000000);
    command
}

async fn run_bridge(mode: &str, payload: Value, sink: Option<StreamSink>) -> Result<Value, String> {
    let mut child = bridge_command(mode)
        .spawn()
        .map_err(|_| "Kimi CLI 运行时无法启动".to_string())?;
    let mut input = child.stdin.take().ok_or("Kimi CLI 无 stdin")?;
    input
        .write_all(format!("{payload}\n").as_bytes())
        .await
        .map_err(|_| "Kimi CLI 输入失败")?;
    drop(input);
    let mut lines = BufReader::new(child.stdout.take().ok_or("Kimi CLI 无 stdout")?).lines();
    let mut output = Value::Null;
    while let Some(line) = lines.next_line().await.map_err(|_| "Kimi CLI 输出失败")? {
        let value: Value = serde_json::from_str(&line).map_err(|_| "Kimi CLI 协议错误")?;
        if let Some(msg) = value["error"].as_str() {
            return Err(msg.to_string());
        }
        if let Some(kind) = value["delta"].as_str() {
            if let Some(sink) = &sink {
                let text = value["text"].as_str().unwrap_or("").to_string();
                match kind {
                    "text" => sink(StreamDelta::Text(text)),
                    "reasoning" => sink(StreamDelta::Reasoning(text)),
                    "tool" => sink(StreamDelta::ToolArgs {
                        index: value["index"].as_u64().unwrap_or(0) as u32,
                        tool_call_id: value["id"].as_str().unwrap_or("").to_string(),
                        name: value["name"].as_str().unwrap_or("").to_string(),
                        delta: text,
                    }),
                    _ => {}
                }
            }
        } else {
            output = value;
        }
    }
    if !child
        .wait()
        .await
        .map_err(|_| "Kimi CLI 退出失败")?
        .success()
        || output.is_null()
    {
        return Err("Kimi CLI 未返回有效结果".to_string());
    }
    Ok(output)
}

async fn start_install() -> Result<(), String> {
    let task_root = root();
    {
        let mut map = states().lock().unwrap();
        let state = map.entry(task_root.clone()).or_default();
        if state.installing || installed() {
            return Ok(());
        }
        state.installing = true;
        state.install_error = None;
    }
    tokio::spawn(async move {
        let runtime = task_root.join("kimi-runtime");
        let executable = runtime.join(if cfg!(windows) {
            "Scripts/python.exe"
        } else {
            "bin/python"
        });
        let mut steps: Vec<Vec<String>> = Vec::new();
        if !executable.is_file() {
            steps.push(vec![
                "venv".into(),
                runtime.to_string_lossy().into_owned(),
                "--python".into(),
                "3.13".into(),
            ]);
        }
        steps.push(vec![
            "pip".into(),
            "install".into(),
            "--python".into(),
            executable.to_string_lossy().into_owned(),
            "--link-mode".into(),
            "copy".into(),
            format!("kimi-cli=={KIMI_VERSION}"),
        ]);
        let mut failure = None;
        for arguments in steps {
            let mut command = tokio::process::Command::new("uv");
            command
                .args(arguments)
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .kill_on_drop(true);
            #[cfg(windows)]
            command.creation_flags(0x08000000);
            match tokio::time::timeout(Duration::from_secs(600), command.status()).await {
                Ok(Ok(status)) if status.success() => {}
                _ => {
                    failure = Some("Kimi CLI 安装失败；请确认 uv 可用及网络连接后重试".to_string());
                    break;
                }
            }
        }
        let mut map = states().lock().unwrap();
        let state = map.entry(task_root).or_default();
        state.installing = false;
        state.install_error = failure;
    });
    Ok(())
}

#[derive(Default, Deserialize)]
pub struct StatusQuery {
    #[serde(default)]
    refresh: bool,
}

fn glm_quota_windows(payload: &Value) -> Vec<Value> {
    let data = payload.get("data").unwrap_or(payload);
    data["limits"]
        .as_array()
        .into_iter()
        .flatten()
        .enumerate()
        .filter_map(|(i, item)| {
            let used = item["percentage"].as_f64().or_else(|| {
                let total = item["usage"].as_f64()?;
                let used = item["currentValue"].as_f64()?;
                (total > 0.0).then_some(used * 100.0 / total)
            })?;
            if !used.is_finite() {
                return None;
            }
            let label = match item["type"].as_str() {
                Some("TOKENS_LIMIT") => "编程额度",
                Some("TIME_LIMIT") => "MCP · 月度",
                _ => item["type"].as_str().unwrap_or("使用窗口"),
            };
            Some(json!({"id":format!("glm-{i}"),"label":label,"usedPercent":used.clamp(0.0,100.0)}))
        })
        .collect()
}

async fn glm_quota(refresh: bool) -> Value {
    let Some(key) = glm_key() else {
        return json!({"state":"unknown","windows":[]});
    };
    let task_root = root();
    if !refresh {
        if let Some((at, value)) = states()
            .lock()
            .unwrap()
            .get(&task_root)
            .and_then(|s| s.glm_quota.as_ref())
        {
            if at.elapsed() < Duration::from_secs(60) {
                return value.clone();
            }
        }
    }
    let result = tokio::task::spawn_blocking(move || {
        let agent = ureq::AgentBuilder::new().timeout(Duration::from_secs(12)).redirects(0).build();
        let response = agent.get("https://open.bigmodel.cn/api/monitor/usage/quota/limit")
            .set("Authorization", &key).set("Content-Type", "application/json").call()
            .map_err(|_| "官方额度暂时无法读取")?;
        let text = response.into_string().map_err(|_| "官方额度响应无效")?;
        let value: Value = serde_json::from_str(&text).map_err(|_| "官方额度响应无效")?;
        let windows = glm_quota_windows(&value);
        Ok::<_, &str>(json!({"state":if windows.is_empty(){"unavailable"}else{"available"},"windows":windows,
            "source":"GLM 官方用量接口", "updatedAt":std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_millis() as u64}))
    }).await;
    let quota = match result {
        Ok(Ok(value)) => value,
        _ => {
            json!({"state":"unavailable","windows":[],"error":"官方额度暂时无法读取，可前往官网查看","source":"GLM 官方用量接口"})
        }
    };
    states()
        .lock()
        .unwrap()
        .entry(task_root)
        .or_default()
        .glm_quota = Some((Instant::now(), quota.clone()));
    quota
}

async fn channel_status(id: &str, refresh: bool) -> Value {
    if id == "antigravity" { return crate::antigravity_oauth::status(refresh).await; }
    if id == "glm" {
        // GLM's public integration uses a Coding Plan key; web cookies are never imported.
        let quota = glm_quota(refresh).await;
        return json!({"id":"glm","installed":true,"configured":ready("glm"),"authMode":if ready("glm") {Some("apiKey")} else {None},
            "modelId":"glm-coding","model":glm_model(),"login":{"state":"idle"},"account":null,
            "quota":quota,"quotaUrl":GLM_CONSOLE, "install":{"running":false}});
    }
    let task_root = root();
    let mut body = json!({"id":"kimi","installed":installed(),"configured":false,"authMode":null,
        "modelId":"kimi-code","model":"Kimi Code","account":null,
        "quota":{"state":"unknown","windows":[]},"quotaUrl":"https://www.kimi.com/code/console"});
    if installed() {
        match tokio::time::timeout(
            Duration::from_secs(30),
            run_bridge("status", json!({"usage":refresh}), None),
        )
        .await
        {
            Ok(Ok(value)) => {
                body["configured"] = value["configured"].clone();
                body["authMode"] = value["authMode"].clone();
                body["account"] = value["account"].clone();
                body["error"] = value["error"].clone();
                let mut map = states().lock().unwrap();
                let state = map.entry(task_root.clone()).or_default();
                state.ready = value["configured"] == true;
                if !state.ready {
                    state.quota = None;
                }
                if refresh {
                    state.quota = Some((Instant::now(), value["quota"].clone()));
                }
            }
            _ => {
                body["error"] = json!("Kimi CLI 状态读取失败，请重试");
            }
        }
    }
    let mut map = states().lock().unwrap();
    let state = map.entry(task_root).or_default();
    body["login"] = if state.login.is_null() {
        json!({"state":"idle"})
    } else {
        state.login.clone()
    };
    body["install"] = json!({"running":state.installing,"error":state.install_error});
    if let Some((at, value)) = &state.quota {
        if at.elapsed() < Duration::from_secs(300) {
            body["quota"] = value.clone();
            if at.elapsed() > Duration::from_secs(60) {
                body["quota"]["stale"] = json!(true);
            }
        }
    }
    body
}

pub async fn list(Query(q): Query<StatusQuery>) -> Json<Value> {
    let (kimi, glm, antigravity) = tokio::join!(
        channel_status("kimi", q.refresh),
        channel_status("glm", q.refresh),
        channel_status("antigravity", q.refresh)
    );
    Json(json!({"channels":[antigravity,kimi,glm]}))
}
pub async fn status(Path(id): Path<String>, Query(q): Query<StatusQuery>) -> Response {
    if !matches!(id.as_str(), "antigravity" | "kimi" | "glm") {
        return error(
            axum::http::StatusCode::NOT_FOUND,
            "CHANNEL_UNKNOWN",
            "未知渠道",
        );
    }
    Json(channel_status(&id, q.refresh).await).into_response()
}

pub async fn login(Path(id): Path<String>) -> Response {
    if id == "antigravity" { return crate::antigravity_oauth::login().await; }
    if id == "glm" {
        return Json(json!({"state":"key_required","authUrl":GLM_CONSOLE})).into_response();
    }
    if id != "kimi" {
        return error(
            axum::http::StatusCode::NOT_FOUND,
            "CHANNEL_UNKNOWN",
            "未知渠道",
        );
    }
    let _gate = LOGIN_GATE.lock().await;
    if !installed() {
        return match start_install().await {
            Ok(()) => Json(json!({"state":"installing"})).into_response(),
            Err(msg) => upstream(&msg),
        };
    }
    let task_root = root();
    {
        let map = states().lock().unwrap();
        if let Some(s) = map.get(&task_root) {
            if s.login["state"] == "pending" {
                return Json(s.login.clone()).into_response();
            }
        }
    }
    let mut child = match bridge_command("login").spawn() {
        Ok(child) => child,
        Err(_) => return upstream("Kimi CLI 运行时无法启动"),
    };
    if let Some(mut input) = child.stdin.take() {
        let _ = input.write_all(b"{}\n").await;
    }
    let Some(stdout) = child.stdout.take() else {
        return upstream("Kimi CLI 无 stdout");
    };
    let mut lines = BufReader::new(stdout).lines();
    let first = match tokio::time::timeout(Duration::from_secs(30), lines.next_line()).await {
        Ok(Ok(Some(line))) => serde_json::from_str::<Value>(&line).unwrap_or(Value::Null),
        _ => return upstream("官方授权请求超时，请重试"),
    };
    if first["state"] != "pending" && first["state"] != "authenticated" {
        return upstream("Kimi 官方授权失败，请重试");
    }
    let login_id = format!(
        "kimi-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
    );
    let mut view = first;
    view["loginId"] = json!(login_id);
    let authenticated = view["state"] == "authenticated";
    let mut map = states().lock().unwrap();
    let state = map.entry(task_root.clone()).or_default();
    if let Some(old) = state.task.take() {
        old.abort();
    }
    state.login = view.clone();
    state.task = Some(tokio::spawn(async move {
        let mut terminal = if authenticated {
            json!({"state":"authenticated","loginId":login_id})
        } else {
            json!({"state":"failed","error":"Kimi 授权未完成，请重试","loginId":login_id})
        };
        let completed = tokio::time::timeout(Duration::from_secs(600), async {
            while let Ok(Some(line)) = lines.next_line().await {
                if let Ok(value) = serde_json::from_str::<Value>(&line) {
                    if value["state"] == "authenticated" { terminal = json!({"state":"authenticated","loginId":login_id}); }
                    if value["error"].is_string() { terminal = json!({"state":"failed","error":"Kimi 官方授权失败或链接已过期，请重试","loginId":login_id}); }
                }
            }
            child.wait().await
        }).await;
        if !matches!(completed, Ok(Ok(status)) if status.success()) {
            let _ = child.kill().await;
            terminal = json!({"state":"failed","error":"Kimi 授权未完成或已超时，请重试","loginId":login_id});
        }
        let mut map = states().lock().unwrap();
        let state = map.entry(task_root).or_default();
        if state.login["loginId"] == login_id {
            state.ready = terminal["state"] == "authenticated";
            state.login = terminal;
            state.quota = None;
        }
    }));
    Json(view).into_response()
}

pub async fn cancel(Path(id): Path<String>) -> Response {
    if id == "antigravity" { return crate::antigravity_oauth::cancel().await; }
    if id != "kimi" {
        return error(
            axum::http::StatusCode::NOT_FOUND,
            "CHANNEL_UNKNOWN",
            "未知渠道",
        );
    }
    let _gate = LOGIN_GATE.lock().await;
    cancel_pending().await;
    Json(json!({"ok":true})).into_response()
}

async fn cancel_pending() {
    let task = {
        let mut map = states().lock().unwrap();
        let state = map.entry(root()).or_default();
        state.login = json!({"state":"cancelled"});
        state.task.take()
    };
    if let Some(task) = task {
        task.abort();
        let _ = task.await;
    }
}

pub async fn logout(Path(id): Path<String>) -> Response {
    match id.as_str() {
        "antigravity" => crate::antigravity_oauth::logout().await,
        "glm" => {
            let _gate = CONFIG_GATE.lock().unwrap();
            match gend::keystore::remove_key("channel:glm") {
                Ok(_) => {
                    states()
                        .lock()
                        .unwrap()
                        .entry(root())
                        .or_default()
                        .glm_quota = None;
                    Json(json!({"ok":true})).into_response()
                }
                Err(_) => upstream("解除绑定失败"),
            }
        }
        "kimi" => {
            let _gate = LOGIN_GATE.lock().await;
            cancel_pending().await;
            if installed() {
                match tokio::time::timeout(
                    Duration::from_secs(15),
                    run_bridge("logout", json!({}), None),
                )
                .await
                {
                    Ok(Ok(_)) => {}
                    _ => return upstream("Kimi 官方退出失败，请重试"),
                }
            }
            let mut map = states().lock().unwrap();
            let state = map.entry(root()).or_default();
            state.ready = false;
            state.quota = None;
            Json(json!({"ok":true})).into_response()
        }
        _ => error(
            axum::http::StatusCode::NOT_FOUND,
            "CHANNEL_UNKNOWN",
            "未知渠道",
        ),
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConfigRequest {
    api_key: Option<String>,
    model: Option<String>,
}
pub async fn configure(Path(id): Path<String>, Json(req): Json<ConfigRequest>) -> Response {
    if id != "glm" {
        return error(
            axum::http::StatusCode::BAD_REQUEST,
            "CHANNEL_CONFIG_INVALID",
            "此渠道使用官方 CLI 授权",
        );
    }
    {
        let _gate = CONFIG_GATE.lock().unwrap();
        let model = req.model.unwrap_or_else(glm_model);
        if !model.starts_with("glm-")
            || model.len() > 80
            || !model
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '.')
        {
            return error(
                axum::http::StatusCode::BAD_REQUEST,
                "MODEL_INVALID",
                "请输入官方 GLM 模型 ID",
            );
        }
        if let Some(key) = req.api_key {
            if key.trim().is_empty() || key.chars().any(|c| c.is_control()) {
                return error(
                    axum::http::StatusCode::BAD_REQUEST,
                    "KEY_REQUIRED",
                    "请输入 Coding Plan Key",
                );
            }
            if gend::keystore::set_key("channel:glm", key.trim()).is_err() {
                return upstream("密钥保存失败");
            }
        } else if glm_key().is_none() {
            return error(
                axum::http::StatusCode::BAD_REQUEST,
                "KEY_REQUIRED",
                "请先绑定 Coding Plan Key",
            );
        }
        let path = root().join("official-channels.json");
        let tmp = path.with_extension("json.tmp");
        if std::fs::create_dir_all(root())
            .and_then(|_| std::fs::write(&tmp, json!({"glmModel":model}).to_string()))
            .and_then(|_| std::fs::rename(&tmp, &path))
            .is_err()
        {
            return upstream("渠道配置保存失败");
        }
        states()
            .lock()
            .unwrap()
            .entry(root())
            .or_default()
            .glm_quota = None;
    }
    Json(channel_status("glm", false).await).into_response()
}

pub(crate) fn step(channel: &'static str, spec: &RequestSpec) -> Box<StepFn> {
    let spec = spec.clone();
    Box::new(move |messages, tools, sink| {
        let spec = spec.clone();
        Box::pin(async move {
            if channel == "kimi" {
                if !installed() {
                    return Err(LlmError::new(
                        "CHANNEL_LOGIN_REQUIRED: 请先安装并登录 Kimi Code",
                    ));
                }
                let value = run_bridge("chat", json!({"messages":messages,"tools":tools,"thinking":spec.reasoning_effort.is_some(),"effort":spec.reasoning_effort}), sink)
                    .await.map_err(LlmError::new)?;
                return llm::parse_step_response(&value, "Kimi Code");
            }
            let key = glm_key().ok_or_else(|| {
                LlmError::new("CHANNEL_LOGIN_REQUIRED: 请先绑定 GLM Coding Plan Key")
            })?;
            let model = glm_model();
            let result = tokio::task::spawn_blocking(move || {
                let mut glm_spec = spec;
                glm_spec.model = None;
                glm_spec.reasoning_effort = None;
                let mut body = llm::build_request_body(&model, &messages, &tools, &glm_spec);
                body.as_object_mut().unwrap().remove("reasoning_effort");
                if let Some(sink) = sink {
                    llm::chat_completions_stream(
                        GLM_CHAT_URL,
                        "GLM Coding Plan",
                        &model,
                        &key,
                        &messages,
                        &tools,
                        &glm_spec,
                        Some(&sink),
                        &[],
                    )
                } else {
                    llm::post_chat_completions(GLM_CHAT_URL, "GLM Coding Plan", &key, &body, &[])
                }
            })
            .await
            .map_err(|_| LlmError::new("GLM 调用任务中断"))??;
            llm::parse_step_response(&result, "GLM Coding Plan")
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::{to_bytes, Body};
    use axum::http::{Request, StatusCode};
    use axum::routing::{get, post};
    use tower::ServiceExt;

    #[test]
    fn official_glm_quota_does_not_invent_allowance_or_reset_time() {
        assert!(
            glm_quota_windows(&json!({"data":{"limits":[{"type":"TOKENS_LIMIT"}]}})).is_empty()
        );
        let windows = glm_quota_windows(&json!({"data":{"limits":[
            {"type":"TOKENS_LIMIT","percentage":25},
            {"type":"TIME_LIMIT","currentValue":18,"usage":20},
            {"type":"TOKENS_LIMIT","percentage":104},
            {"type":"TIME_LIMIT","currentValue":4,"usage":0}
        ]}}));
        assert_eq!(windows.len(), 3);
        assert_eq!(windows[0]["usedPercent"], 25.0);
        assert_eq!(windows[1]["usedPercent"], 90.0);
        assert_eq!(windows[1]["label"], "MCP · 月度");
        assert_eq!(windows[2]["usedPercent"], 100.0);
        assert!(windows.iter().all(|w| w.get("resetsAt").is_none()));
    }

    #[tokio::test]
    async fn official_channel_routes_reject_unknown_channels_and_invalid_configuration() {
        let app = axum::Router::new()
            .route("/channels/{id}", get(status))
            .route("/channels/{id}/login", post(login))
            .route("/channels/{id}/config", post(configure));
        for (method, path, payload, expected) in [
            ("GET", "/channels/unknown", json!({}), StatusCode::NOT_FOUND),
            (
                "POST",
                "/channels/unknown/login",
                json!({}),
                StatusCode::NOT_FOUND,
            ),
            (
                "POST",
                "/channels/kimi/config",
                json!({"apiKey":"test-only"}),
                StatusCode::BAD_REQUEST,
            ),
            (
                "POST",
                "/channels/glm/config",
                json!({"model":"gpt-other","apiKey":"test-only"}),
                StatusCode::BAD_REQUEST,
            ),
            (
                "POST",
                "/channels/glm/config",
                json!({"model":"glm-5.3","apiKey":"\n"}),
                StatusCode::BAD_REQUEST,
            ),
        ] {
            let request = Request::builder()
                .method(method)
                .uri(path)
                .header("Content-Type", "application/json")
                .body(Body::from(payload.to_string()))
                .unwrap();
            let response = app.clone().oneshot(request).await.unwrap();
            assert_eq!(response.status(), expected, "{path}");
            let bytes = to_bytes(response.into_body(), 8192).await.unwrap();
            assert!(!String::from_utf8_lossy(&bytes).contains("test-only"));
        }
        let response = app
            .oneshot(
                Request::post("/channels/glm/login")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let value: Value =
            serde_json::from_slice(&to_bytes(response.into_body(), 8192).await.unwrap()).unwrap();
        assert_eq!(value["state"], "key_required");
        assert_eq!(value["authUrl"], GLM_CONSOLE);
    }

    #[tokio::test]
    async fn official_missing_credentials_fail_before_any_model_request() {
        let _guard = crate::llm::TEST_ENV_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let previous_agent = std::env::var_os("FORGE_AGENTD_DATA_DIR");
        let previous_gen = std::env::var_os("FORGE_GEN_DATA_DIR");
        let directory = std::env::temp_dir().join(format!(
            "forge-official-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&directory).unwrap();
        std::env::set_var("FORGE_AGENTD_DATA_DIR", &directory);
        std::env::set_var("FORGE_GEN_DATA_DIR", &directory);
        for channel in ["kimi", "glm"] {
            assert!(!ready(channel));
            let step = step(channel, &RequestSpec::default());
            let result = step(vec![json!({"role":"user","content":"hello"})], vec![], None).await;
            assert!(result
                .err().expect("missing credentials must fail")
                .to_string()
                .contains("CHANNEL_LOGIN_REQUIRED"));
        }
        for (key, previous) in [
            ("FORGE_AGENTD_DATA_DIR", previous_agent),
            ("FORGE_GEN_DATA_DIR", previous_gen),
        ] {
            match previous {
                Some(value) => std::env::set_var(key, value),
                None => std::env::remove_var(key),
            }
        }
        std::fs::remove_dir(&directory).unwrap();
    }
}
