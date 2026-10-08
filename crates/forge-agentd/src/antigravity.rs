//! Antigravity (Google AI / Gemini) 订阅反代配置面与额度探针 (R1 & R2)。
//! 持久化端点 baseUrl/model/enabled 至 data/llm-antigravity.json;
//! 访问 Token 写入本地 keystore (红线 R-5: 密钥永不回显、不进事件与日志、Debug 脱敏)。
//! 提供 status/config/probe REST 路由及 native agent step 工厂。

use axum::extract::Json;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::path::PathBuf;
use std::sync::{Mutex, RwLock};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

pub const ANTIGRAVITY_KEYSTORE_ID: &str = "antigravity";
#[allow(dead_code)]
pub const ANTIGRAVITY_MODEL_ID: &str = "antigravity";
pub const ANTIGRAVITY_NOT_CONFIGURED: &str = "ANTIGRAVITY_NOT_CONFIGURED";
pub const DEFAULT_ANTIGRAVITY_MODEL: &str = "gemini-3.8-flash";

static ANTIGRAVITY_FILE_LOCK: Mutex<()> = Mutex::new(());
static PROBE_CACHE: RwLock<Option<ProbeCacheEntry>> = RwLock::new(None);

/// 非敏感配置文件: <workspace>/data/llm-antigravity.json (R-5: 绝无密钥)
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct AntigravityFile {
    #[serde(alias = "baseUrl", alias = "base_url", default)]
    pub base_url: String,
    #[serde(default = "default_model")]
    pub model: String,
    #[serde(default = "default_true")]
    pub enabled: bool,
}

fn default_model() -> String {
    DEFAULT_ANTIGRAVITY_MODEL.to_string()
}

fn default_true() -> bool {
    true
}

impl Default for AntigravityFile {
    fn default() -> Self {
        Self {
            base_url: String::new(),
            model: DEFAULT_ANTIGRAVITY_MODEL.to_string(),
            enabled: true,
        }
    }
}

/// 配额时间窗口 (对齐 Codex LimitBucket 架构)
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct RateLimitWindow {
    pub used_percent: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resets_at: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub window_duration_mins: Option<u32>,
}

/// 额度限流容器 (primary / secondary 窗口)
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct AntigravityRateLimits {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub primary: Option<RateLimitWindow>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub secondary: Option<RateLimitWindow>,
}

/// 探针缓存项
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct ProbeCacheEntry {
    pub probed_at: Instant,
    pub probed_timestamp_ms: u64,
    pub connected: bool,
    pub latency_ms: u64,
    pub rate_limits: Option<AntigravityRateLimits>,
    pub upstream_models: Vec<String>,
    pub error: Option<String>,
}

/// POST /api/forge/llm/antigravity/config 请求体
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AntigravityConfigRequest {
    #[serde(alias = "baseUrl", alias = "base_url", default)]
    pub base_url: String,
    #[serde(default)]
    pub model: String,
    #[serde(default)]
    pub key: Option<String>,
    #[serde(default)]
    pub enabled: Option<bool>,
}

/// POST /api/forge/llm/antigravity/probe 请求体
#[derive(Debug, Clone, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct AntigravityProbeRequest {
    #[serde(alias = "baseUrl", alias = "base_url", default)]
    pub base_url: Option<String>,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub key: Option<String>,
}

pub fn antigravity_config_path() -> PathBuf {
    gend::config::data_dir().join("llm-antigravity.json")
}

pub fn load_antigravity_file() -> AntigravityFile {
    let path = antigravity_config_path();
    match std::fs::read_to_string(&path) {
        Ok(text) => serde_json::from_str(&text).unwrap_or_else(|e| {
            eprintln!("llm-antigravity.json parse failed: {e}, using default");
            AntigravityFile::default()
        }),
        Err(_) => AntigravityFile::default(),
    }
}

pub fn save_antigravity_file(cfg: &AntigravityFile) -> std::io::Result<()> {
    let path = antigravity_config_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let text = serde_json::to_string_pretty(cfg)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, text)?;
    std::fs::rename(&tmp, &path)
}

/// 读取本地 Keystore 中存储的 Token (R-5: 仅用于外呼 Authorization 头组装)
pub fn antigravity_key() -> Option<String> {
    if let Ok(v) = std::env::var("FORGE_ANTIGRAVITY_API_KEY") {
        let trimmed = v.trim();
        if !trimmed.is_empty() {
            return Some(trimmed.to_string());
        }
    }
    let ks = gend::keystore::Keystore::load();
    ks.secret_for(ANTIGRAVITY_KEYSTORE_ID)
        .or_else(|| ks.key_for(ANTIGRAVITY_KEYSTORE_ID))
        .map(|k| k.trim().to_string())
        .filter(|k| !k.is_empty())
}

/// 决议三元组: baseUrl + model + key 全齐且 enabled → Some
pub fn resolve_antigravity() -> Option<(String, String, String)> {
    if let Some(managed) = crate::antigravity_oauth::resolve() { return Some(managed); }
    let file = load_antigravity_file();
    if !file.enabled || file.base_url.trim().is_empty() || file.model.trim().is_empty() {
        return None;
    }
    antigravity_key().map(|k| (file.base_url.trim().to_string(), file.model.trim().to_string(), k))
}

pub fn clear_probe_cache() {
    let mut cache = PROBE_CACHE.write().unwrap();
    *cache = None;
}

#[allow(dead_code)]
pub fn cached_probe() -> Option<ProbeCacheEntry> {
    PROBE_CACHE.read().unwrap().clone()
}

/// 渠道可用性判定 (design-snapshot 与 status 统一事实源):
/// - "needs-config": 未配置端点/模型/密钥或未启用
/// - "available": 配置齐备且最近探测成功(或新配置未探测)
/// - "disconnected": 配置齐备但最近探测失败(失联/4xx/5xx)
pub fn availability() -> &'static str {
    if crate::antigravity_oauth::ready() { return "available"; }
    let file = load_antigravity_file();
    let has_key = antigravity_key().is_some();
    if !file.enabled || file.base_url.trim().is_empty() || file.model.trim().is_empty() || !has_key {
        return "needs-config";
    }
    let cache = PROBE_CACHE.read().unwrap();
    if let Some(entry) = cache.as_ref() {
        if entry.connected {
            "available"
        } else {
            "disconnected"
        }
    } else {
        "available"
    }
}

/// 从 HTTP 响应头提取 rate limit 信息
pub fn extract_rate_limits_from_headers(resp: &ureq::Response) -> Option<AntigravityRateLimits> {
    // Primary window
    let primary_used = resp
        .header("x-ratelimit-primary-used")
        .or_else(|| resp.header("x-ratelimit-used-percent"))
        .and_then(|v| v.trim().parse::<f64>().ok());

    let primary_limit = resp
        .header("x-ratelimit-limit-requests")
        .and_then(|v| v.trim().parse::<f64>().ok());
    let primary_remaining = resp
        .header("x-ratelimit-remaining-requests")
        .and_then(|v| v.trim().parse::<f64>().ok());

    let primary_resets_at = resp
        .header("x-ratelimit-primary-resets-at")
        .or_else(|| resp.header("x-ratelimit-reset-requests"))
        .and_then(|v| v.trim().parse::<u64>().ok());

    let primary_window_mins = resp
        .header("x-ratelimit-primary-window-mins")
        .or_else(|| resp.header("x-ratelimit-window-mins"))
        .and_then(|v| v.trim().parse::<u32>().ok());

    let primary_percent = primary_used.or_else(|| {
        if let (Some(limit), Some(rem)) = (primary_limit, primary_remaining) {
            if limit > 0.0 {
                Some(((limit - rem).max(0.0) / limit) * 100.0)
            } else {
                None
            }
        } else {
            None
        }
    });

    let primary = primary_percent.map(|used_percent| RateLimitWindow {
        used_percent,
        resets_at: primary_resets_at,
        window_duration_mins: primary_window_mins,
    });

    // Secondary window
    let sec_used = resp
        .header("x-ratelimit-secondary-used")
        .and_then(|v| v.trim().parse::<f64>().ok());

    let sec_limit = resp
        .header("x-ratelimit-limit-tokens")
        .and_then(|v| v.trim().parse::<f64>().ok());
    let sec_remaining = resp
        .header("x-ratelimit-remaining-tokens")
        .and_then(|v| v.trim().parse::<f64>().ok());

    let sec_resets_at = resp
        .header("x-ratelimit-secondary-resets-at")
        .or_else(|| resp.header("x-ratelimit-reset-tokens"))
        .and_then(|v| v.trim().parse::<u64>().ok());

    let sec_window_mins = resp
        .header("x-ratelimit-secondary-window-mins")
        .and_then(|v| v.trim().parse::<u32>().ok());

    let sec_percent = sec_used.or_else(|| {
        if let (Some(limit), Some(rem)) = (sec_limit, sec_remaining) {
            if limit > 0.0 {
                Some(((limit - rem).max(0.0) / limit) * 100.0)
            } else {
                None
            }
        } else {
            None
        }
    });

    let secondary = sec_percent.map(|used_percent| RateLimitWindow {
        used_percent,
        resets_at: sec_resets_at,
        window_duration_mins: sec_window_mins,
    });

    if primary.is_some() || secondary.is_some() {
        Some(AntigravityRateLimits { primary, secondary })
    } else {
        None
    }
}

/// 从 JSON 响应体提取 rate limit 信息
pub fn extract_rate_limits_from_body(body: &Value) -> Option<AntigravityRateLimits> {
    let source = if body.get("rateLimits").is_some() {
        body.get("rateLimits")
    } else if body.get("rate_limits").is_some() {
        body.get("rate_limits")
    } else {
        Some(body)
    }?;

    let parse_window = |v: &Value| -> Option<RateLimitWindow> {
        let used_percent = v
            .get("usedPercent")
            .or_else(|| v.get("used_percent"))
            .and_then(Value::as_f64)
            .or_else(|| {
                let limit = v.get("limit").and_then(Value::as_f64);
                let remaining = v.get("remaining").and_then(Value::as_f64);
                let used = v.get("used").and_then(Value::as_f64);
                if let (Some(lim), Some(u)) = (limit, used) {
                    if lim > 0.0 {
                        Some((u / lim) * 100.0)
                    } else {
                        None
                    }
                } else if let (Some(lim), Some(rem)) = (limit, remaining) {
                    if lim > 0.0 {
                        Some(((lim - rem).max(0.0) / lim) * 100.0)
                    } else {
                        None
                    }
                } else {
                    None
                }
            })?;
        let resets_at = v
            .get("resetsAt")
            .or_else(|| v.get("resets_at"))
            .and_then(Value::as_u64);
        let window_duration_mins = v
            .get("windowDurationMins")
            .or_else(|| v.get("window_duration_mins"))
            .and_then(Value::as_u64)
            .map(|m| m as u32);
        Some(RateLimitWindow {
            used_percent,
            resets_at,
            window_duration_mins,
        })
    };

    let primary = source.get("primary").and_then(parse_window);
    let secondary = source.get("secondary").and_then(parse_window);

    if primary.is_some() || secondary.is_some() {
        Some(AntigravityRateLimits { primary, secondary })
    } else {
        None
    }
}

#[derive(Debug, Clone)]
pub struct ProbeResult {
    pub latency_ms: u64,
    pub rate_limits: Option<AntigravityRateLimits>,
    pub upstream_models: Vec<String>,
}

/// 构建额度探针请求 URL (探测 /v1/models)
/// 若用户输入的 baseUrl 带有 /chat/completions 或 /v1/chat/completions，自动剥离后规整为 /v1/models
pub fn probe_url_for_base(base_url: &str) -> String {
    let mut clean = base_url.trim().trim_end_matches('/');
    if let Some(stripped) = clean.strip_suffix("/v1/chat/completions") {
        clean = stripped.trim_end_matches('/');
        format!("{clean}/v1/models")
    } else if let Some(stripped) = clean.strip_suffix("/chat/completions") {
        clean = stripped.trim_end_matches('/');
        if clean.ends_with("/v1") {
            format!("{clean}/models")
        } else {
            format!("{clean}/v1/models")
        }
    } else if clean.ends_with("/v1/models") {
        clean.to_string()
    } else if clean.ends_with("/v1") {
        format!("{clean}/models")
    } else {
        format!("{clean}/v1/models")
    }
}

/// 实际向端点发起额度探针与连通探测
pub fn execute_probe(base_url: &str, key: &str, _model: &str) -> Result<ProbeResult, String> {
    let start = Instant::now();
    let url = probe_url_for_base(base_url);

    let agent = ureq::AgentBuilder::new()
        .timeout_connect(Duration::from_secs(10))
        .timeout_read(Duration::from_secs(10))
        .build();

    let resp = agent
        .get(&url)
        .set("Authorization", &format!("Bearer {key}"))
        .set("Accept", "application/json")
        .call();

    let latency_ms = start.elapsed().as_millis() as u64;

    match resp {
        Ok(r) => {
            let mut rate_limits = extract_rate_limits_from_headers(&r);

            let body_text = r.into_string().unwrap_or_default();
            let body_json: Value = serde_json::from_str(&body_text).unwrap_or(Value::Null);

            let mut upstream_models = Vec::new();
            if let Some(arr) = body_json.get("data").and_then(Value::as_array) {
                for item in arr {
                    if let Some(id) = item.get("id").and_then(Value::as_str) {
                        upstream_models.push(id.to_string());
                    }
                }
            } else if let Some(arr) = body_json.get("models").and_then(Value::as_array) {
                for item in arr {
                    if let Some(id) = item.get("id").and_then(Value::as_str) {
                        upstream_models.push(id.to_string());
                    } else if let Some(id) = item.as_str() {
                        upstream_models.push(id.to_string());
                    }
                }
            }

            if rate_limits.is_none() {
                rate_limits = extract_rate_limits_from_body(&body_json);
            }

            Ok(ProbeResult {
                latency_ms,
                rate_limits,
                upstream_models,
            })
        }
        Err(ureq::Error::Status(code, r)) => {
            let mut msg = format!("HTTP {code}");
            if let Ok(text) = r.into_string() {
                if let Ok(v) = serde_json::from_str::<Value>(&text) {
                    if let Some(m) = v.pointer("/error/message").and_then(Value::as_str) {
                        msg = format!("HTTP {code}: {m}");
                    }
                }
            }
            Err(msg)
        }
        Err(ureq::Error::Transport(t)) => Err(format!("连接失败: {t}")),
    }
}

// ---------- Axum REST 路由 Handlers ----------

/// POST /api/forge/llm/antigravity/config
pub async fn set_antigravity_config(Json(req): Json<AntigravityConfigRequest>) -> Response {
    let base_url = req.base_url.trim().to_string();
    if base_url.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({
                "error": {
                    "code": "EMPTY_BASE_URL",
                    "message": "baseUrl 不可空"
                }
            })),
        )
            .into_response();
    }
    let model = req.model.trim().to_string();
    if model.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({
                "error": {
                    "code": "EMPTY_MODEL",
                    "message": "model 不可空"
                }
            })),
        )
            .into_response();
    }

    {
        let _g = ANTIGRAVITY_FILE_LOCK.lock().unwrap();
        let file = AntigravityFile {
            base_url: base_url.clone(),
            model: model.clone(),
            enabled: req.enabled.unwrap_or(true),
        };
        if let Err(e) = save_antigravity_file(&file) {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({
                    "error": {
                        "code": "FORGE_IO",
                        "message": e.to_string()
                    }
                })),
            )
                .into_response();
        }
    }

    if let Some(k) = req.key.as_deref().map(str::trim).filter(|k| !k.is_empty()) {
        if let Err(e) = gend::keystore::set_key(ANTIGRAVITY_KEYSTORE_ID, k) {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({
                    "error": {
                        "code": "FORGE_IO",
                        "message": e.to_string()
                    }
                })),
            )
                .into_response();
        }
    }

    clear_probe_cache();

    let file = load_antigravity_file();
    let key_conf = antigravity_key().is_some();
    Json(json!({
        "ok": true,
        "configured": !file.base_url.is_empty() && !file.model.is_empty() && key_conf,
        "baseUrl": file.base_url,
        "model": file.model,
        "enabled": file.enabled,
        "keyConfigured": key_conf,
    }))
    .into_response()
}

/// GET /api/forge/llm/antigravity/status
pub async fn antigravity_status_handler() -> Json<Value> {
    let file = load_antigravity_file();
    let key_conf = antigravity_key().is_some();
    let configured = !file.base_url.is_empty() && !file.model.is_empty() && key_conf && file.enabled;
    let avail = availability();
    let cache = PROBE_CACHE.read().unwrap();
    let (rate_limits, upstream_models, latency_ms, last_probed_at) = if let Some(e) = cache.as_ref() {
        (
            serde_json::to_value(&e.rate_limits).unwrap_or(Value::Null),
            serde_json::to_value(&e.upstream_models).unwrap_or_else(|_| json!([])),
            Some(e.latency_ms),
            Some(e.probed_timestamp_ms),
        )
    } else {
        (Value::Null, json!([]), None, None)
    };
    Json(json!({
        "ok": true,
        "configured": configured,
        "baseUrl": file.base_url,
        "model": file.model,
        "enabled": file.enabled,
        "keyConfigured": key_conf,
        "availability": avail,
        "rateLimits": rate_limits,
        "models": upstream_models,
        "latencyMs": latency_ms,
        "lastProbedAt": last_probed_at,
    }))
}

/// POST /api/forge/llm/antigravity/probe
pub async fn antigravity_probe_handler(
    Json(req): Json<AntigravityProbeRequest>,
) -> Response {
    let file = load_antigravity_file();
    let base_url = req
        .base_url
        .filter(|s| !s.trim().is_empty())
        .unwrap_or(file.base_url);
    let model = req
        .model
        .filter(|s| !s.trim().is_empty())
        .unwrap_or(file.model);
    let key = req
        .key
        .filter(|s| !s.trim().is_empty())
        .or_else(antigravity_key);

    if base_url.trim().is_empty() || key.is_none() {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({
                "ok": false,
                "status": "needs-config",
                "error": "baseUrl 或 API Key 未配置",
            })),
        )
            .into_response();
    }
    let key = key.unwrap();
    let base_url_clone = base_url.clone();
    let model_clone = model.clone();
    let key_clone = key.clone();

    let result = tokio::task::spawn_blocking(move || {
        execute_probe(&base_url_clone, &key_clone, &model_clone)
    })
    .await;

    let (ok, status_str, latency_ms, rate_limits, upstream_models, error) = match result {
        Ok(Ok(probe_ok)) => (
            true,
            "available",
            probe_ok.latency_ms,
            probe_ok.rate_limits,
            probe_ok.upstream_models,
            None,
        ),
        Ok(Err(err)) => (
            false,
            "disconnected",
            0,
            None,
            Vec::new(),
            Some(err),
        ),
        Err(join_err) => (
            false,
            "disconnected",
            0,
            None,
            Vec::new(),
            Some(format!("Probe task failed: {join_err}")),
        ),
    };

    let now_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);

    {
        let mut cache = PROBE_CACHE.write().unwrap();
        *cache = Some(ProbeCacheEntry {
            probed_at: Instant::now(),
            probed_timestamp_ms: now_ms,
            connected: ok,
            latency_ms,
            rate_limits: rate_limits.clone(),
            upstream_models: upstream_models.clone(),
            error: error.clone(),
        });
    }

    Json(json!({
        "ok": ok,
        "status": status_str,
        "latencyMs": latency_ms,
        "rateLimits": rate_limits,
        "models": upstream_models,
        "error": error,
    }))
    .into_response()
}

// ---------- Native Agent Step 工厂 ----------

/// 未配置步进: 首轮第 1 迭代即显式失败 (抛出 ANTIGRAVITY_NOT_CONFIGURED, 绝不重试 10 次拖延 13 分钟)
pub fn antigravity_not_configured_step() -> Box<crate::llm::StepFn> {
    Box::new(|_m, _t, _s| {
        Box::pin(async move {
            Err(crate::llm::LlmError::new(format!(
                "{ANTIGRAVITY_NOT_CONFIGURED}: Antigravity 渠道未配齐(baseUrl/model/key 缺一);请在 设置→模型 页配置"
            )))
        })
    })
}

/// Antigravity 步进工厂: 阻塞 HTTP 走 spawn_blocking; Bearer Token 入头, 闭包不外泄
pub fn antigravity_step(
    base_url: &str,
    model: &str,
    key: &str,
    spec: &crate::llm::RequestSpec,
) -> Box<crate::llm::StepFn> {
    let base_url = base_url.to_string();
    let model = model.to_string();
    let key = key.to_string();
    let spec = spec.clone();
    Box::new(move |messages, tools, stream| {
        let base_url = base_url.clone();
        let model = model.clone();
        let key = key.clone();
        let spec = spec.clone();
        Box::pin(async move {
            let resp = tokio::task::spawn_blocking(move || {
                let clean = base_url.trim_end_matches('/');
                let url = if clean.ends_with("/chat/completions") {
                    clean.to_string()
                } else if clean.ends_with("/v1") {
                    format!("{clean}/chat/completions")
                } else {
                    format!("{clean}/v1/chat/completions")
                };

                if let Some(sink) = stream {
                    crate::llm::chat_completions_stream(
                        &url,
                        "antigravity",
                        &model,
                        &key,
                        &messages,
                        &tools,
                        &spec,
                        Some(&sink),
                        &[],
                    )
                } else {
                    let body = crate::llm::build_request_body(&model, &messages, &tools, &spec);
                    crate::llm::post_chat_completions(&url, "antigravity", &key, &body, &[])
                }
            })
            .await
            .map_err(|e| crate::llm::LlmError::new(format!("spawn_blocking join 失败: {e}")))??;

            crate::llm::parse_step_response(&resp, "antigravity")
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TestDirGuard {
        dir: PathBuf,
    }

    impl TestDirGuard {
        fn new(tag: &str) -> Self {
            let dir = std::env::temp_dir().join(format!(
                "agentd-ag-test-{tag}-{}-{}",
                std::process::id(),
                SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .map(|d| d.as_nanos())
                    .unwrap_or(0)
            ));
            std::fs::create_dir_all(&dir).unwrap();
            std::env::remove_var("FORGE_ANTIGRAVITY_API_KEY");
            std::env::remove_var("FORGE_LLM_API_KEY");
            std::env::remove_var("FORGE_GEN_API_KEY");
            std::env::set_var("FORGE_GEN_DATA_DIR", &dir);
            clear_probe_cache();
            TestDirGuard { dir }
        }
    }

    impl Drop for TestDirGuard {
        fn drop(&mut self) {
            std::env::remove_var("FORGE_GEN_DATA_DIR");
            std::env::remove_var("FORGE_ANTIGRAVITY_API_KEY");
            std::fs::remove_dir_all(&self.dir).ok();
            clear_probe_cache();
        }
    }

    #[tokio::test]
    async fn antigravity_config_save_load_roundtrip_and_redline_r5() {
        let _g = crate::llm::TEST_ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let _guard = TestDirGuard::new("roundtrip");

        // 1. 初始状态未配置
        let initial_status = antigravity_status_handler().await.0;
        assert_eq!(initial_status["configured"], false);
        assert_eq!(initial_status["keyConfigured"], false);
        assert_eq!(initial_status["availability"], "needs-config");

        // 2. 调用 POST config 写入配置与密钥
        let secret = "sk-antigravity-super-secret-key-12345";
        let resp = set_antigravity_config(Json(AntigravityConfigRequest {
            base_url: "https://proxy.antigravity.test:8080".to_string(),
            model: "gemini-3.8-flash".to_string(),
            key: Some(secret.to_string()),
            enabled: Some(true),
        }))
        .await;

        let (parts, body) = resp.into_parts();
        assert_eq!(parts.status, StatusCode::OK);
        let bytes = axum::body::to_bytes(body, 1 << 20).await.unwrap();
        let resp_json: Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(resp_json["ok"], true);
        assert_eq!(resp_json["configured"], true);
        assert_eq!(resp_json["baseUrl"], "https://proxy.antigravity.test:8080");
        assert_eq!(resp_json["model"], "gemini-3.8-flash");
        assert_eq!(resp_json["keyConfigured"], true);

        // 红线 R-5 断言: HTTP 响应体绝不包含明文密钥
        let resp_str = resp_json.to_string();
        assert!(!resp_str.contains(secret), "POST config 响应体泄漏密钥 (红线 R-5 违规)");

        // 3. 配置文件 data/llm-antigravity.json 绝无密钥明文
        let file_path = antigravity_config_path();
        let file_content = std::fs::read_to_string(&file_path).unwrap();
        assert!(!file_content.contains(secret), "llm-antigravity.json 泄漏密钥 (红线 R-5 违规)");
        let saved_file = load_antigravity_file();
        assert_eq!(saved_file.base_url, "https://proxy.antigravity.test:8080");
        assert_eq!(saved_file.model, "gemini-3.8-flash");

        // 4. Keystore 安全保存并能读回
        let loaded_key = antigravity_key();
        assert_eq!(loaded_key.as_deref(), Some(secret));

        // 5. GET status 查询验证
        let status = antigravity_status_handler().await.0;
        assert_eq!(status["ok"], true);
        assert_eq!(status["configured"], true);
        assert_eq!(status["keyConfigured"], true);
        assert_eq!(status["availability"], "available");
        let status_str = status.to_string();
        assert!(!status_str.contains(secret), "GET status 响应体泄漏密钥 (红线 R-5 违规)");

        // 6. resolve_antigravity 正确产出三元组
        let resolved = resolve_antigravity();
        assert_eq!(
            resolved,
            Some((
                "https://proxy.antigravity.test:8080".to_string(),
                "gemini-3.8-flash".to_string(),
                secret.to_string()
            ))
        );
    }

    #[tokio::test]
    async fn antigravity_config_validation_rejections() {
        let _g = crate::llm::TEST_ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let _guard = TestDirGuard::new("validation");

        // 空 baseUrl 拒绝
        let resp_empty_url = set_antigravity_config(Json(AntigravityConfigRequest {
            base_url: "   ".to_string(),
            model: "gemini-3.8-flash".to_string(),
            key: Some("key".to_string()),
            enabled: Some(true),
        }))
        .await;
        let (parts, body) = resp_empty_url.into_parts();
        assert_eq!(parts.status, StatusCode::BAD_REQUEST);
        let bytes = axum::body::to_bytes(body, 1 << 20).await.unwrap();
        let v: Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(v["error"]["code"], "EMPTY_BASE_URL");

        // 空 model 拒绝
        let resp_empty_model = set_antigravity_config(Json(AntigravityConfigRequest {
            base_url: "https://proxy.example.com".to_string(),
            model: "   ".to_string(),
            key: Some("key".to_string()),
            enabled: Some(true),
        }))
        .await;
        let (parts2, body2) = resp_empty_model.into_parts();
        assert_eq!(parts2.status, StatusCode::BAD_REQUEST);
        let bytes2 = axum::body::to_bytes(body2, 1 << 20).await.unwrap();
        let v2: Value = serde_json::from_slice(&bytes2).unwrap();
        assert_eq!(v2["error"]["code"], "EMPTY_MODEL");
    }

    #[test]
    fn antigravity_rate_limits_parsing_from_body() {
        let body = json!({
            "primary": {
                "usedPercent": 35.5,
                "resetsAt": 1799999999u64,
                "windowDurationMins": 300
            },
            "secondary": {
                "usedPercent": 12.0,
                "resetsAt": 1800000000u64,
                "windowDurationMins": 1440
            }
        });
        let parsed = extract_rate_limits_from_body(&body).expect("should parse rate limits");
        let primary = parsed.primary.expect("primary window");
        assert_eq!(primary.used_percent, 35.5);
        assert_eq!(primary.resets_at, Some(1799999999));
        assert_eq!(primary.window_duration_mins, Some(300));

        let secondary = parsed.secondary.expect("secondary window");
        assert_eq!(secondary.used_percent, 12.0);
        assert_eq!(secondary.resets_at, Some(1800000000));
        assert_eq!(secondary.window_duration_mins, Some(1440));
    }

    #[tokio::test]
    async fn antigravity_not_configured_step_fails_fast() {
        let step = antigravity_not_configured_step();
        let res = (step)(vec![], vec![], None).await;
        assert!(res.is_err());
        let err = res.err().unwrap().to_string();
        assert!(err.contains(ANTIGRAVITY_NOT_CONFIGURED));
    }

    #[tokio::test]
    async fn antigravity_probe_validation_needs_config() {
        let _g = crate::llm::TEST_ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let _guard = TestDirGuard::new("probe_needs_config");

        let resp = antigravity_probe_handler(Json(AntigravityProbeRequest {
            base_url: None,
            model: None,
            key: None,
        }))
        .await;

        let (parts, body) = resp.into_parts();
        assert_eq!(parts.status, StatusCode::BAD_REQUEST);
        let bytes = axum::body::to_bytes(body, 1 << 20).await.unwrap();
        let val: Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(val["status"], "needs-config");
    }

    #[test]
    fn antigravity_probe_cache_lifecycle() {
        let _g = crate::llm::TEST_ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        clear_probe_cache();
        assert!(PROBE_CACHE.read().unwrap().is_none());

        {
            let mut cache = PROBE_CACHE.write().unwrap();
            *cache = Some(ProbeCacheEntry {
                probed_at: Instant::now(),
                probed_timestamp_ms: 1700000000000,
                connected: true,
                latency_ms: 120,
                rate_limits: Some(AntigravityRateLimits {
                    primary: Some(RateLimitWindow {
                        used_percent: 25.0,
                        resets_at: Some(1700000000),
                        window_duration_mins: Some(60),
                    }),
                    secondary: None,
                }),
                upstream_models: vec!["gemini-3.8-flash".to_string(), "gemini-3.8-pro".to_string()],
                error: None,
            });
        }

        {
            let cache = PROBE_CACHE.read().unwrap();
            let entry = cache.as_ref().expect("cache entry exists");
            assert_eq!(entry.latency_ms, 120);
            assert_eq!(entry.upstream_models.len(), 2);
            assert_eq!(
                entry.rate_limits.as_ref().unwrap().primary.as_ref().unwrap().used_percent,
                25.0
            );
        }

        clear_probe_cache();
        assert!(PROBE_CACHE.read().unwrap().is_none());
    }

    #[tokio::test]
    async fn adversarial_input_validation_empty_whitespace_and_nulls() {
        let _g = crate::llm::TEST_ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let _guard = TestDirGuard::new("adv_validation");

        // 1. Whitespace variations in base_url
        for bad_url in &["", " ", "\t", "\r\n", "  \t \n  "] {
            let resp = set_antigravity_config(Json(AntigravityConfigRequest {
                base_url: bad_url.to_string(),
                model: "gemini-3.8-flash".to_string(),
                key: Some("key".to_string()),
                enabled: Some(true),
            }))
            .await;
            let (parts, body) = resp.into_parts();
            assert_eq!(parts.status, StatusCode::BAD_REQUEST, "bad_url={bad_url:?} should fail 400");
            let bytes = axum::body::to_bytes(body, 1 << 20).await.unwrap();
            let v: Value = serde_json::from_slice(&bytes).unwrap();
            assert_eq!(v["error"]["code"], "EMPTY_BASE_URL");
        }

        // 2. Whitespace variations in model
        for bad_model in &["", " ", "\t", "\r\n", "  \t \n  "] {
            let resp = set_antigravity_config(Json(AntigravityConfigRequest {
                base_url: "https://proxy.example.com".to_string(),
                model: bad_model.to_string(),
                key: Some("key".to_string()),
                enabled: Some(true),
            }))
            .await;
            let (parts, body) = resp.into_parts();
            assert_eq!(parts.status, StatusCode::BAD_REQUEST, "bad_model={bad_model:?} should fail 400");
            let bytes = axum::body::to_bytes(body, 1 << 20).await.unwrap();
            let v: Value = serde_json::from_slice(&bytes).unwrap();
            assert_eq!(v["error"]["code"], "EMPTY_MODEL");
        }

        // 3. Deserialized from empty JSON: missing fields
        let req_empty: AntigravityConfigRequest = serde_json::from_str("{}").unwrap();
        let resp = set_antigravity_config(Json(req_empty)).await;
        let (parts, body) = resp.into_parts();
        assert_eq!(parts.status, StatusCode::BAD_REQUEST);
        let bytes = axum::body::to_bytes(body, 1 << 20).await.unwrap();
        let v: Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(v["error"]["code"], "EMPTY_BASE_URL");
    }

    #[test]
    fn adversarial_rate_limits_body_parsing_all_flavors() {
        // Flavor A: snake_case rate_limits with limit and used
        let body_a = json!({
            "rate_limits": {
                "primary": {
                    "limit": 1000.0,
                    "used": 250.0,
                    "resets_at": 123456u64,
                    "window_duration_mins": 60
                }
            }
        });
        let parsed_a = extract_rate_limits_from_body(&body_a).expect("should parse snake_case limit/used");
        let p_a = parsed_a.primary.expect("primary window");
        assert_eq!(p_a.used_percent, 25.0);
        assert_eq!(p_a.resets_at, Some(123456));
        assert_eq!(p_a.window_duration_mins, Some(60));
        assert!(parsed_a.secondary.is_none());

        // Flavor B: top-level object with limit and remaining
        let body_b = json!({
            "primary": {
                "limit": 200.0,
                "remaining": 50.0
            }
        });
        let parsed_b = extract_rate_limits_from_body(&body_b).expect("should parse limit/remaining");
        let p_b = parsed_b.primary.expect("primary window");
        assert_eq!(p_b.used_percent, 75.0); // (200 - 50) / 200 * 100 = 75%

        // Flavor C: limit == 0.0 (no division by zero / NaN)
        let body_c = json!({
            "primary": {
                "limit": 0.0,
                "used": 0.0
            }
        });
        let parsed_c = extract_rate_limits_from_body(&body_c);
        assert!(parsed_c.is_none(), "limit=0 should not produce a window");

        // Flavor D: remaining > limit (remaining 120, limit 100) -> 0% used, no negative
        let body_d = json!({
            "primary": {
                "limit": 100.0,
                "remaining": 120.0
            }
        });
        let parsed_d = extract_rate_limits_from_body(&body_d).expect("should parse over-remaining");
        let p_d = parsed_d.primary.expect("primary window");
        assert_eq!(p_d.used_percent, 0.0);

        // Flavor E: completely empty JSON
        let body_e = json!({});
        assert!(extract_rate_limits_from_body(&body_e).is_none());

        // Flavor F: secondary only
        let body_f = json!({
            "rateLimits": {
                "secondary": {
                    "usedPercent": 88.5
                }
            }
        });
        let parsed_f = extract_rate_limits_from_body(&body_f).expect("should parse secondary only");
        assert!(parsed_f.primary.is_none());
        assert_eq!(parsed_f.secondary.unwrap().used_percent, 88.5);
    }

    #[test]
    fn adversarial_rate_limits_header_parsing_with_mock_server() {
        use std::io::Write;
        use std::net::TcpListener;

        let listener = TcpListener::bind("127.0.0.1:0").expect("bind free port");
        let port = listener.local_addr().unwrap().port();

        let handle = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut buf = [0u8; 1024];
            let _ = std::io::Read::read(&mut stream, &mut buf);
            let response = concat!(
                "HTTP/1.1 200 OK\r\n",
                "Content-Type: application/json\r\n",
                "x-ratelimit-primary-used: 42.5\r\n",
                "x-ratelimit-primary-resets-at: 1780000000\r\n",
                "x-ratelimit-primary-window-mins: 120\r\n",
                "x-ratelimit-limit-tokens: 50000\r\n",
                "x-ratelimit-remaining-tokens: 10000\r\n",
                "x-ratelimit-secondary-resets-at: 1780003600\r\n",
                "x-ratelimit-secondary-window-mins: 60\r\n",
                "Content-Length: 2\r\n\r\n",
                "{}"
            );
            stream.write_all(response.as_bytes()).unwrap();
            stream.flush().unwrap();
        });

        let resp = ureq::get(&format!("http://127.0.0.1:{port}/v1/models")).call().unwrap();
        let limits = extract_rate_limits_from_headers(&resp).expect("headers must parse rate limits");
        let primary = limits.primary.expect("primary window from headers");
        assert_eq!(primary.used_percent, 42.5);
        assert_eq!(primary.resets_at, Some(1780000000));
        assert_eq!(primary.window_duration_mins, Some(120));

        let secondary = limits.secondary.expect("secondary window from headers");
        // limit 50000, rem 10000 -> used = 40000/50000 = 80.0%
        assert_eq!(secondary.used_percent, 80.0);
        assert_eq!(secondary.resets_at, Some(1780003600));
        assert_eq!(secondary.window_duration_mins, Some(60));

        handle.join().unwrap();
    }

    #[test]
    fn adversarial_snapshot_availability_three_states_state_machine() {
        let _g = crate::llm::TEST_ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let _guard = TestDirGuard::new("adv_snap");

        // State 1: 未配置态 -> needs-config
        assert_eq!(availability(), "needs-config");

        // State 2: 配置齐备但未探测 -> available
        let file = AntigravityFile {
            base_url: "https://proxy.example.com".to_string(),
            model: "gemini-3.8-flash".to_string(),
            enabled: true,
        };
        save_antigravity_file(&file).unwrap();
        gend::keystore::set_key(ANTIGRAVITY_KEYSTORE_ID, "sk-test-ag-key").unwrap();
        clear_probe_cache();
        assert_eq!(availability(), "available");

        // State 3: 探测失败 (connected = false) -> disconnected
        {
            let mut cache = PROBE_CACHE.write().unwrap();
            *cache = Some(ProbeCacheEntry {
                probed_at: Instant::now(),
                probed_timestamp_ms: 1000,
                connected: false,
                latency_ms: 0,
                rate_limits: None,
                upstream_models: vec![],
                error: Some("HTTP 401 Unauthorized".to_string()),
            });
        }
        assert_eq!(availability(), "disconnected");

        // State 4: 探测恢复 (connected = true) -> available
        {
            let mut cache = PROBE_CACHE.write().unwrap();
            *cache = Some(ProbeCacheEntry {
                probed_at: Instant::now(),
                probed_timestamp_ms: 2000,
                connected: true,
                latency_ms: 150,
                rate_limits: None,
                upstream_models: vec!["gemini-3.8-flash".to_string()],
                error: None,
            });
        }
        assert_eq!(availability(), "available");

        // State 5: enabled = false -> needs-config
        let disabled_file = AntigravityFile {
            base_url: "https://proxy.example.com".to_string(),
            model: "gemini-3.8-flash".to_string(),
            enabled: false,
        };
        save_antigravity_file(&disabled_file).unwrap();
        assert_eq!(availability(), "needs-config");
    }

    #[test]
    fn adversarial_modelspec_aliased_id_check() {
        use crate::modelspec::resolve;

        // 1. Direct model ID gemini-3.8-flash resolves to 1M tokens
        let res_plain = resolve(Some("gemini-3.8-flash"), true, Some("high"), Some("1m"));
        assert_eq!(res_plain.context_tokens, 1_048_576);
        assert_eq!(res_plain.reasoning_effort.as_deref(), Some("high"));

        // 2. Direct model ID gemini-3.8-pro resolves to 1M tokens
        let res_pro = resolve(Some("gemini-3.8-pro"), true, Some("high"), Some("1m"));
        assert_eq!(res_pro.context_tokens, 1_048_576);

        // 3. antigravity/ 前缀解析
        let res_slash_flash = resolve(Some("antigravity/gemini-3.8-flash"), true, Some("high"), Some("1m"));
        assert_eq!(res_slash_flash.context_tokens, 1_048_576);
        assert_eq!(res_slash_flash.reasoning_effort.as_deref(), Some("high"));

        let res_slash_pro = resolve(Some("antigravity/gemini-3.8-pro"), true, Some("medium"), Some("200k"));
        assert_eq!(res_slash_pro.context_tokens, 204_800);
        assert_eq!(res_slash_pro.reasoning_effort.as_deref(), Some("medium"));

        // 4. antigravity: 前缀解析
        let res_colon_flash = resolve(Some("antigravity:gemini-3.8-flash"), true, Some("high"), Some("1m"));
        assert_eq!(res_colon_flash.context_tokens, 1_048_576);
        assert_eq!(res_colon_flash.reasoning_effort.as_deref(), Some("high"));

        let res_colon_pro = resolve(Some("antigravity:gemini-3.8-pro"), true, Some("low"), Some("300k"));
        assert_eq!(res_colon_pro.context_tokens, 307_200);
        assert_eq!(res_colon_pro.reasoning_effort.as_deref(), Some("low"));
    }

    #[test]
    fn test_probe_url_for_base_trimming() {
        // 标准 baseURL 测试
        assert_eq!(
            probe_url_for_base("http://127.0.0.1:8080"),
            "http://127.0.0.1:8080/v1/models"
        );
        assert_eq!(
            probe_url_for_base("http://127.0.0.1:8080/"),
            "http://127.0.0.1:8080/v1/models"
        );
        assert_eq!(
            probe_url_for_base("http://127.0.0.1:8080/v1"),
            "http://127.0.0.1:8080/v1/models"
        );
        assert_eq!(
            probe_url_for_base("http://127.0.0.1:8080/v1/"),
            "http://127.0.0.1:8080/v1/models"
        );
        assert_eq!(
            probe_url_for_base("http://127.0.0.1:8080/v1/models"),
            "http://127.0.0.1:8080/v1/models"
        );

        // 用户输入 chat/completions 甚至 /v1/chat/completions 尾缀时的容错裁剪
        assert_eq!(
            probe_url_for_base("http://127.0.0.1:8080/chat/completions"),
            "http://127.0.0.1:8080/v1/models"
        );
        assert_eq!(
            probe_url_for_base("http://127.0.0.1:8080/chat/completions/"),
            "http://127.0.0.1:8080/v1/models"
        );
        assert_eq!(
            probe_url_for_base("http://127.0.0.1:8080/v1/chat/completions"),
            "http://127.0.0.1:8080/v1/models"
        );
        assert_eq!(
            probe_url_for_base("http://127.0.0.1:8080/v1/chat/completions/"),
            "http://127.0.0.1:8080/v1/models"
        );

        // 带自定义路径前缀时的裁剪
        assert_eq!(
            probe_url_for_base("https://proxy.example.com/prefix/v1/chat/completions"),
            "https://proxy.example.com/prefix/v1/models"
        );
        assert_eq!(
            probe_url_for_base("https://proxy.example.com/prefix/chat/completions"),
            "https://proxy.example.com/prefix/v1/models"
        );
    }
}


