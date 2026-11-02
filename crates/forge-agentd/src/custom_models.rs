//! 自定义模型目录(设置·模型页「高级:自定义模型」卡片页的后端面;D-049)。
//!
//! 取代 F8 的单槽 openai-compat 配置:每个目录项是一个用户自配的模型端点——
//! API 格式(OpenAI chat.completions / Anthropic Messages)+ baseUrl + 模型名 +
//! 上下文窗口 + 最大输出 + 是否收图。key 一律进 gend keystore,条目 id 即 keystore id
//! (R-5:密钥不进本文件、不进响应、不进日志)。
//!
//! 兼容旧单槽:`data/llm-custom-models.json` 不存在时,把旧 `data/llm-openai-compat.json`
//! 读成 id = "openai-compat" 的目录项(只读迁移,首次保存才落新文件)。会话
//! `selectedModelId = "openai-compat"` 与 `/api/forge/llm/openai-compat/{config,status}`
//! 照旧可用;新增条目 id 为 `custom:<8 位十六进制>`(与 `cloud:<id>` 同为带前缀的会话模型 id)。
//!
//! REST:
//! - `GET    /api/forge/llm/custom-models`            → {models:[…], defaultId}
//! - `POST   /api/forge/llm/custom-models`            → 新增(无 id)/ 整条更新(带 id);key 省略 = 保留
//! - `DELETE /api/forge/llm/custom-models/{id}`       → 删除条目并清掉其密钥
//! - `POST   /api/forge/llm/custom-models/test`       → 用表单当前值发一次最小请求(测试连接)

use std::collections::HashSet;
use std::path::PathBuf;

use axum::extract::Path;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::llm::{CustomApi, Provider};

/// 旧单槽的目录 id(会话模型 id 与 keystore id 都沿用它)。
pub(crate) const LEGACY_ID: &str = "openai-compat";
/// 新增条目的会话模型 id 前缀。
pub(crate) const ID_PREFIX: &str = "custom:";
/// 模型菜单与快照里自定义模型的分组名。
pub(crate) const GROUP: &str = "自定义模型";
/// 未声明窗口时按旧单槽默认档(modelspec openai-compat 卡的 1m)。
pub(crate) const DEFAULT_CONTEXT_WINDOW: u64 = 1_048_576;
/// Anthropic Messages 的 max_tokens 是必填项;目录项没声明最大输出时发这个值。
pub(crate) const ANTHROPIC_DEFAULT_MAX_TOKENS: u64 = 32_768;
/// 目录上限:菜单里放不下更多,也防误操作刷出几百条。
const MAX_MODELS: usize = 32;
const NAME_MAX_CHARS: usize = 64;
const MIN_CONTEXT_WINDOW: u64 = 1_024;
const MAX_CONTEXT_WINDOW: u64 = 16_777_216;
const MAX_OUTPUT_TOKENS: u64 = 1_048_576;

/// 端点的请求格式。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum ApiFormat {
    /// OpenAI chat.completions(Bearer 鉴权)。
    #[default]
    Openai,
    /// Anthropic Messages(x-api-key + anthropic-version)。
    Anthropic,
}

impl ApiFormat {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            ApiFormat::Openai => "openai",
            ApiFormat::Anthropic => "anthropic",
        }
    }

    /// 错误消息与日志里的渠道标签(不含密钥)。
    pub(crate) fn label(self) -> &'static str {
        match self {
            ApiFormat::Openai => "openai-compat",
            ApiFormat::Anthropic => "anthropic-compat",
        }
    }

    fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "" | "openai" => Some(ApiFormat::Openai),
            "anthropic" => Some(ApiFormat::Anthropic),
            _ => None,
        }
    }
}

/// 一个目录项(落盘形态;绝无 key)。
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CustomModel {
    pub id: String,
    /// 卡片与模型菜单的显示名;空 = 用模型名。
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub api_format: ApiFormat,
    #[serde(default)]
    pub base_url: String,
    #[serde(default)]
    pub model: String,
    /// 上下文窗口(tokens);None = 未声明,按 DEFAULT_CONTEXT_WINDOW 与旧单槽的全档。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context_window: Option<u64>,
    /// 最大输出(tokens);None = OpenAI 格式不发 max_tokens,Anthropic 格式发缺省值。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_output_tokens: Option<u64>,
    /// 端点后面的模型收不收图。端点探不出来(同一个 baseUrl 可以换任意模型),由人声明;
    /// 缺省 false = 不发图,宁可少一项能力也不发出会被拒的请求。
    #[serde(default)]
    pub vision: bool,
}

impl CustomModel {
    /// 显示名:name → 模型名 → 如实「未配置」。
    pub(crate) fn label(&self) -> String {
        let name = self.name.trim();
        if !name.is_empty() {
            name.to_string()
        } else if !self.model.trim().is_empty() {
            self.model.trim().to_string()
        } else {
            "openai-compatible(未配置)".to_string()
        }
    }

    pub(crate) fn context_tokens(&self) -> u64 {
        self.context_window.unwrap_or(DEFAULT_CONTEXT_WINDOW)
    }

    pub(crate) fn endpoint(&self) -> String {
        endpoint_url(&self.base_url, self.api_format)
    }

    fn complete(&self) -> bool {
        !self.base_url.trim().is_empty() && !self.model.trim().is_empty()
    }
}

/// 请求全路径。baseUrl 已写到资源路径(…/chat/completions、…/messages)→ 原样;
/// 以版本段收尾(…/v1、…/api/paas/v4)→ 只补资源名;其余补 `/v1/…`(F8 单槽口径)。
pub(crate) fn endpoint_url(base_url: &str, format: ApiFormat) -> String {
    let base = base_url.trim().trim_end_matches('/');
    let resource = match format {
        ApiFormat::Openai => "chat/completions",
        ApiFormat::Anthropic => "messages",
    };
    if base.ends_with(&format!("/{resource}")) {
        return base.to_string();
    }
    let last = base.rsplit('/').next().unwrap_or("");
    if is_version_segment(last) {
        format!("{base}/{resource}")
    } else {
        format!("{base}/v1/{resource}")
    }
}

fn is_version_segment(s: &str) -> bool {
    s.strip_prefix('v')
        .is_some_and(|rest| !rest.is_empty() && rest.chars().all(|c| c.is_ascii_digit()))
}

/// 会话模型 id 是不是自定义模型(旧单槽 id 或 `custom:<后缀>`)。
pub(crate) fn is_custom_id(id: &str) -> bool {
    if id == LEGACY_ID {
        return true;
    }
    id.strip_prefix(ID_PREFIX).is_some_and(|rest| {
        !rest.is_empty()
            && rest.len() <= 40
            && rest
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    })
}

// ---------- 落盘 ----------

#[derive(Debug, Default, Deserialize)]
struct CatalogFile {
    #[serde(default)]
    models: Vec<CustomModel>,
}

#[derive(Serialize)]
struct CatalogOut<'a> {
    models: &'a [CustomModel],
}

/// 旧单槽文件形态(F8 OpenAiCompatFile 原样,snake_case)。
#[derive(Debug, Default, Deserialize)]
struct LegacyFile {
    #[serde(default)]
    base_url: String,
    #[serde(default)]
    model: String,
    #[serde(default)]
    vision: bool,
}

/// 读-改-写串行锁(整文件原子写,同 gen-backends.json 纪律)。
static FILE_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn catalog_path() -> PathBuf {
    gend::config::data_dir().join("llm-custom-models.json")
}

fn legacy_path() -> PathBuf {
    gend::config::data_dir().join("llm-openai-compat.json")
}

/// 读目录:新文件优先;缺失 → 旧单槽只读迁移。解析失败按空目录处理(打印不含机密)。
pub(crate) fn load() -> Vec<CustomModel> {
    let path = catalog_path();
    match std::fs::read_to_string(&path) {
        Ok(text) => match serde_json::from_str::<CatalogFile>(&text) {
            Ok(file) => {
                let mut seen = HashSet::new();
                file.models
                    .into_iter()
                    .filter(|m| is_custom_id(&m.id) && seen.insert(m.id.clone()))
                    .collect()
            }
            Err(e) => {
                eprintln!(
                    "llm-custom-models.json 解析失败({}): {e},按空目录处理",
                    path.display()
                );
                Vec::new()
            }
        },
        Err(_) => load_legacy(),
    }
}

fn load_legacy() -> Vec<CustomModel> {
    let path = legacy_path();
    let Ok(text) = std::fs::read_to_string(&path) else {
        return Vec::new();
    };
    let legacy: LegacyFile = match serde_json::from_str(&text) {
        Ok(v) => v,
        Err(e) => {
            eprintln!(
                "llm-openai-compat.json 解析失败({}): {e},按未配置处理",
                path.display()
            );
            return Vec::new();
        }
    };
    if legacy.base_url.is_empty() && legacy.model.is_empty() {
        return Vec::new();
    }
    vec![CustomModel {
        id: LEGACY_ID.to_string(),
        base_url: legacy.base_url,
        model: legacy.model,
        vision: legacy.vision,
        ..CustomModel::default()
    }]
}

/// 原子写:tmp 全量写 + rename(调用方须持 FILE_LOCK)。
fn save(models: &[CustomModel]) -> std::io::Result<()> {
    let path = catalog_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let text = serde_json::to_string_pretty(&CatalogOut { models })
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, text)?;
    std::fs::rename(&tmp, &path)
}

fn new_id(existing: &[CustomModel]) -> String {
    use std::hash::{BuildHasher, Hasher};
    loop {
        let mut h = std::collections::hash_map::RandomState::new().build_hasher();
        h.write_u128(
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos(),
        );
        let id = format!("{ID_PREFIX}{:08x}", h.finish() as u32);
        if !existing.iter().any(|m| m.id == id) {
            return id;
        }
    }
}

// ---------- 解析(provider / 快照 / 默认渠道) ----------

/// 目录项 + 密钥有无(一次 keystore 读取供整张清单用)。
pub(crate) struct Entry {
    pub model: CustomModel,
    pub key_configured: bool,
}

impl Entry {
    pub(crate) fn configured(&self) -> bool {
        self.key_configured && self.model.complete()
    }

    fn json(&self, default_id: Option<&str>) -> Value {
        let m = &self.model;
        json!({
            "id": m.id,
            "name": m.name,
            "label": m.label(),
            "apiFormat": m.api_format.as_str(),
            "baseUrl": m.base_url,
            "model": m.model,
            "endpoint": m.endpoint(),
            "contextWindow": m.context_tokens(),
            "contextWindowSet": m.context_window.is_some(),
            "maxOutputTokens": m.max_output_tokens,
            "vision": m.vision,
            "keyConfigured": self.key_configured,
            "configured": self.configured(),
            "isDefault": default_id == Some(m.id.as_str()),
        })
    }
}

fn key_of(ks: &gend::keystore::Keystore, id: &str) -> Option<String> {
    ks.key_for(id).filter(|k| !k.is_empty())
}

pub(crate) fn entries() -> Vec<Entry> {
    let models = load();
    if models.is_empty() {
        return Vec::new();
    }
    let ks = gend::keystore::Keystore::load();
    models
        .into_iter()
        .map(|model| Entry {
            key_configured: key_of(&ks, &model.id).is_some(),
            model,
        })
        .collect()
}

pub(crate) fn find(id: &str) -> Option<CustomModel> {
    load().into_iter().find(|m| m.id == id)
}

fn provider_of(m: CustomModel, ks: &gend::keystore::Keystore) -> Option<Provider> {
    if !m.complete() {
        return None;
    }
    let key = key_of(ks, &m.id)?;
    Some(Provider::OpenAiCompat {
        base_url: m.base_url.trim().to_string(),
        model: m.model.trim().to_string(),
        key,
        api: CustomApi {
            id: m.id,
            format: m.api_format,
            max_output_tokens: m.max_output_tokens,
            vision: m.vision,
        },
    })
}

/// 会话显式选中的自定义模型 → provider;未配齐/已删除 = 显式 NOT_CONFIGURED,不静默回落。
pub(crate) fn provider(id: &str) -> Provider {
    let Some(m) = find(id) else {
        return Provider::OpenAiCompatNotConfigured;
    };
    provider_of(m, &gend::keystore::Keystore::load())
        .unwrap_or(Provider::OpenAiCompatNotConfigured)
}

/// 默认渠道决议用:目录里第一个配齐的条目(旧单槽迁移来的排第一,与 F8「配齐即默认」一致)。
pub(crate) fn default_provider() -> Option<Provider> {
    let models = load();
    if models.is_empty() {
        return None;
    }
    let ks = gend::keystore::Keystore::load();
    models.into_iter().find_map(|m| provider_of(m, &ks))
}

pub(crate) fn any_configured() -> bool {
    entries().iter().any(Entry::configured)
}

/// design-snapshot 的模型菜单条目:能力面同 openai-compat 卡(五档 effort,思考靠
/// reasoning_effort 表达),窗口按目录项声明现算;未声明沿用该卡全档。
pub(crate) fn snapshot_card(entry: &Entry) -> Value {
    let m = &entry.model;
    let availability = if entry.configured() {
        "available"
    } else {
        "needs-key"
    };
    let mut v = match crate::modelspec::card(LEGACY_ID) {
        Some(card) => crate::modelspec::card_json(card, availability, &m.label()),
        None => json!({ "label": m.label(), "availability": availability }),
    };
    v["id"] = json!(m.id);
    v["provider"] = json!(LEGACY_ID);
    v["group"] = json!(GROUP);
    v["apiFormat"] = json!(m.api_format.as_str());
    v["vision"] = json!(m.vision);
    if let Some(window) = m.context_window {
        let options = crate::modelspec::window_options(window);
        v["defaultContext"] = json!(options.last().map(|o| o.id.clone()));
        v["contextOptions"] = json!(options
            .iter()
            .map(|o| json!({ "id": o.id, "label": o.label, "tokens": o.tokens }))
            .collect::<Vec<Value>>());
    }
    v
}

// ---------- REST ----------

fn error(status: StatusCode, code: &str, message: &str) -> Response {
    (
        status,
        Json(json!({ "error": { "code": code, "message": message } })),
    )
        .into_response()
}

fn io_error(e: std::io::Error) -> Response {
    error(StatusCode::INTERNAL_SERVER_ERROR, "FORGE_IO", &e.to_string())
}

/// GET /api/forge/llm/custom-models → {models, defaultId}(无 key,R-5)。
pub async fn list_handler() -> Json<Value> {
    let entries = entries();
    let default_id = entries
        .iter()
        .find(|e| e.configured())
        .map(|e| e.model.id.clone());
    Json(json!({
        "models": entries.iter().map(|e| e.json(default_id.as_deref())).collect::<Vec<Value>>(),
        "defaultId": default_id,
    }))
}

/// 新增/更新/测试共用的表单面。
#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelRequest {
    /// 缺省/空 = 新增;有值 = 更新该条目(测试连接时用来取已存密钥)。
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    name: Option<String>,
    /// "openai" | "anthropic";缺省 openai。
    #[serde(default)]
    api_format: Option<String>,
    #[serde(default)]
    base_url: String,
    #[serde(default)]
    model: String,
    /// 非空 → 写 keystore(永不回显);省略/空 = 保留既有。
    #[serde(default)]
    key: Option<String>,
    /// tokens;null/省略 = 不声明。
    #[serde(default)]
    context_window: Option<u64>,
    /// tokens;null/省略 = 不声明。
    #[serde(default)]
    max_output_tokens: Option<u64>,
    #[serde(default)]
    vision: Option<bool>,
}

/// 校验后的表单(id 由调用方定)。
struct Draft {
    name: String,
    api_format: ApiFormat,
    base_url: String,
    model: String,
    context_window: Option<u64>,
    max_output_tokens: Option<u64>,
    vision: bool,
}

fn validate(req: &ModelRequest) -> Result<Draft, Response> {
    let base_url = req.base_url.trim().to_string();
    if base_url.is_empty() {
        return Err(error(StatusCode::BAD_REQUEST, "EMPTY_BASE_URL", "baseUrl 不可空"));
    }
    let lower = base_url.to_ascii_lowercase();
    if !(lower.starts_with("http://") || lower.starts_with("https://")) {
        return Err(error(
            StatusCode::BAD_REQUEST,
            "INVALID_BASE_URL",
            "baseUrl 须以 http:// 或 https:// 开头",
        ));
    }
    let model = req.model.trim().to_string();
    if model.is_empty() {
        return Err(error(StatusCode::BAD_REQUEST, "EMPTY_MODEL", "model 不可空"));
    }
    let name = req.name.as_deref().unwrap_or("").trim().to_string();
    if name.chars().count() > NAME_MAX_CHARS {
        return Err(error(
            StatusCode::BAD_REQUEST,
            "NAME_TOO_LONG",
            &format!("显示名最多 {NAME_MAX_CHARS} 个字符"),
        ));
    }
    let Some(api_format) = ApiFormat::parse(req.api_format.as_deref().unwrap_or("")) else {
        return Err(error(
            StatusCode::BAD_REQUEST,
            "INVALID_API_FORMAT",
            "apiFormat 只能是 openai 或 anthropic",
        ));
    };
    if let Some(n) = req.context_window {
        if !(MIN_CONTEXT_WINDOW..=MAX_CONTEXT_WINDOW).contains(&n) {
            return Err(error(
                StatusCode::BAD_REQUEST,
                "INVALID_CONTEXT_WINDOW",
                &format!("上下文窗口须在 {MIN_CONTEXT_WINDOW}–{MAX_CONTEXT_WINDOW} tokens 之间"),
            ));
        }
    }
    if let Some(n) = req.max_output_tokens {
        let window = req.context_window.unwrap_or(DEFAULT_CONTEXT_WINDOW);
        if n == 0 || n > MAX_OUTPUT_TOKENS || n > window {
            return Err(error(
                StatusCode::BAD_REQUEST,
                "INVALID_MAX_OUTPUT",
                "最大输出须大于 0,且不超过上下文窗口",
            ));
        }
    }
    Ok(Draft {
        name,
        api_format,
        base_url,
        model,
        context_window: req.context_window,
        max_output_tokens: req.max_output_tokens,
        vision: req.vision.unwrap_or(false),
    })
}

fn request_key(req: &ModelRequest) -> Option<String> {
    req.key
        .as_deref()
        .map(str::trim)
        .filter(|k| !k.is_empty())
        .map(str::to_string)
}

/// POST /api/forge/llm/custom-models:无 id = 新增;有 id = 整条更新(key 省略 = 保留)。
/// 响应 {ok, model}(model 同清单条目形态,绝无 key)。
pub async fn upsert_handler(Json(req): Json<ModelRequest>) -> Response {
    let draft = match validate(&req) {
        Ok(d) => d,
        Err(r) => return r,
    };
    let key = request_key(&req);
    let saved = {
        let _g = FILE_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let mut models = load();
        let requested = req
            .id
            .as_deref()
            .map(str::trim)
            .filter(|id| !id.is_empty());
        let id = match requested {
            Some(id) if models.iter().any(|m| m.id == id) => id.to_string(),
            // 旧单槽 id 允许「不存在即建」(与旧 config 端点同口径)。
            Some(id) if id == LEGACY_ID => LEGACY_ID.to_string(),
            Some(_) => {
                return error(StatusCode::NOT_FOUND, "NOT_FOUND", "该自定义模型不存在或已删除")
            }
            None => {
                if models.len() >= MAX_MODELS {
                    return error(
                        StatusCode::BAD_REQUEST,
                        "TOO_MANY_MODELS",
                        &format!("自定义模型最多 {MAX_MODELS} 个"),
                    );
                }
                new_id(&models)
            }
        };
        let entry = CustomModel {
            id: id.clone(),
            name: draft.name,
            api_format: draft.api_format,
            base_url: draft.base_url,
            model: draft.model,
            context_window: draft.context_window,
            max_output_tokens: draft.max_output_tokens,
            vision: draft.vision,
        };
        match models.iter_mut().find(|m| m.id == id) {
            Some(slot) => *slot = entry.clone(),
            None if id == LEGACY_ID => models.insert(0, entry.clone()),
            None => models.push(entry.clone()),
        }
        if let Err(e) = save(&models) {
            return io_error(e);
        }
        entry
    };
    if let Some(k) = key.as_deref() {
        if let Err(e) = gend::keystore::set_key(&saved.id, k) {
            return io_error(e);
        }
    }
    let entries = entries();
    let default_id = entries
        .iter()
        .find(|e| e.configured())
        .map(|e| e.model.id.clone());
    let json = entries
        .iter()
        .find(|e| e.model.id == saved.id)
        .map(|e| e.json(default_id.as_deref()))
        .unwrap_or(Value::Null);
    Json(json!({ "ok": true, "model": json })).into_response()
}

/// DELETE /api/forge/llm/custom-models/{id}:删条目并清掉其密钥。
pub async fn delete_handler(Path(id): Path<String>) -> Response {
    {
        let _g = FILE_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let mut models = load();
        let before = models.len();
        models.retain(|m| m.id != id);
        if models.len() == before {
            return error(StatusCode::NOT_FOUND, "NOT_FOUND", "该自定义模型不存在或已删除");
        }
        if let Err(e) = save(&models) {
            return io_error(e);
        }
    }
    // 密钥清理失败不回滚目录(条目已不可选,残留密钥无人引用);如实打印,不含密钥。
    if let Err(e) = gend::keystore::remove_key(&id) {
        eprintln!("自定义模型 {id} 的密钥清理失败: {e}");
    }
    Json(json!({ "ok": true })).into_response()
}

/// 测试连接:发一次最小请求,只看端点/鉴权/模型名三者对不对得上(不计入会话)。
fn probe(draft: &Draft, key: &str) -> Result<String, crate::llm::LlmError> {
    let url = endpoint_url(&draft.base_url, draft.api_format);
    let ping = json!([{ "role": "user", "content": "ping" }]);
    let reply = match draft.api_format {
        ApiFormat::Openai => {
            // 不带 max_tokens:推理模型只认 max_completion_tokens,带错反而误报失败。
            let body = json!({ "model": draft.model, "messages": ping, "stream": false });
            let resp =
                crate::llm::post_chat_completions(&url, draft.api_format.label(), key, &body, &[])?;
            resp.pointer("/choices/0/message/content")
                .map(|c| match c {
                    Value::String(s) => s.clone(),
                    other => other.to_string(),
                })
                .unwrap_or_default()
        }
        ApiFormat::Anthropic => {
            let body = json!({ "model": draft.model, "max_tokens": 32, "messages": ping });
            let resp = crate::anthropic_compat::post(&url, key, &body)?;
            resp.get("content")
                .and_then(Value::as_array)
                .map(|blocks| {
                    blocks
                        .iter()
                        .filter_map(|b| b.get("text").and_then(Value::as_str))
                        .collect::<String>()
                })
                .unwrap_or_default()
        }
    };
    Ok(reply.trim().chars().take(80).collect())
}

/// POST /api/forge/llm/custom-models/test(表单面同新增;key 留空且带 id → 用已存密钥)。
/// 校验错 4xx;探测结果一律 200 {ok, latencyMs, endpoint, reply?|error?}。
pub async fn test_handler(Json(req): Json<ModelRequest>) -> Response {
    let draft = match validate(&req) {
        Ok(d) => d,
        Err(r) => return r,
    };
    let key = match request_key(&req) {
        Some(k) => k,
        None => {
            let stored = req
                .id
                .as_deref()
                .map(str::trim)
                .filter(|id| !id.is_empty())
                .and_then(|id| key_of(&gend::keystore::Keystore::load(), id));
            match stored {
                Some(k) => k,
                None => return error(StatusCode::BAD_REQUEST, "MISSING_KEY", "请先填写 API Key"),
            }
        }
    };
    let endpoint = endpoint_url(&draft.base_url, draft.api_format);
    let started = std::time::Instant::now();
    let outcome = tokio::task::spawn_blocking(move || probe(&draft, &key)).await;
    let latency_ms = started.elapsed().as_millis() as u64;
    match outcome {
        Ok(Ok(reply)) => Json(json!({
            "ok": true, "latencyMs": latency_ms, "endpoint": endpoint, "reply": reply,
        }))
        .into_response(),
        Ok(Err(e)) => Json(json!({
            "ok": false, "latencyMs": latency_ms, "endpoint": endpoint, "error": e.to_string(),
        }))
        .into_response(),
        Err(e) => error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "FORGE_INTERNAL",
            &format!("测试任务失败: {e}"),
        ),
    }
}

// ---------- 旧单槽 REST(F8 wave.2 两端点;e2e 脚本仍在用) ----------

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LegacyConfigRequest {
    #[serde(default)]
    pub(crate) base_url: String,
    #[serde(default)]
    pub(crate) model: String,
    #[serde(default)]
    pub(crate) key: Option<String>,
    /// 省略 = 保留既有(免得只改 baseUrl 的请求把它悄悄关掉)。
    #[serde(default)]
    pub(crate) vision: Option<bool>,
}

fn legacy_status() -> Value {
    let ks = gend::keystore::Keystore::load();
    let key_configured = key_of(&ks, LEGACY_ID).is_some();
    let m = load()
        .into_iter()
        .find(|m| m.id == LEGACY_ID)
        .unwrap_or_default();
    json!({
        "configured": key_configured && m.complete(),
        "baseUrl": m.base_url,
        "model": m.model,
        "keyConfigured": key_configured,
        "vision": m.vision,
        "apiFormat": m.api_format.as_str(),
    })
}

/// POST /api/forge/llm/openai-compat/config {baseUrl, model, key?, vision?}:落在 id="openai-compat"
/// 的目录项上;未传的字段(格式/窗口/输出/名称)保留既有值。
pub async fn legacy_config_handler(Json(req): Json<LegacyConfigRequest>) -> Response {
    let base_url = req.base_url.trim().to_string();
    if base_url.is_empty() {
        return error(StatusCode::BAD_REQUEST, "EMPTY_BASE_URL", "baseUrl 不可空");
    }
    let model = req.model.trim().to_string();
    if model.is_empty() {
        return error(StatusCode::BAD_REQUEST, "EMPTY_MODEL", "model 不可空");
    }
    {
        let _g = FILE_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let mut models = load();
        match models.iter_mut().find(|m| m.id == LEGACY_ID) {
            Some(m) => {
                m.base_url = base_url;
                m.model = model;
                if let Some(v) = req.vision {
                    m.vision = v;
                }
            }
            None => models.insert(
                0,
                CustomModel {
                    id: LEGACY_ID.to_string(),
                    base_url,
                    model,
                    vision: req.vision.unwrap_or(false),
                    ..CustomModel::default()
                },
            ),
        }
        if let Err(e) = save(&models) {
            return io_error(e);
        }
    }
    if let Some(k) = req.key.as_deref().map(str::trim).filter(|k| !k.is_empty()) {
        if let Err(e) = gend::keystore::set_key(LEGACY_ID, k) {
            return io_error(e);
        }
    }
    let mut v = legacy_status();
    v["ok"] = json!(true);
    Json(v).into_response()
}

/// GET /api/forge/llm/openai-compat/status → {configured, baseUrl, model, keyConfigured, vision, apiFormat}。
pub async fn legacy_status_handler() -> Json<Value> {
    Json(legacy_status())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// FORGE_GEN_DATA_DIR 指向临时目录(进程级 env,持 llm::TEST_ENV_LOCK 互斥)。
    struct TempData {
        dir: PathBuf,
        _guard: std::sync::MutexGuard<'static, ()>,
    }

    impl TempData {
        fn new(tag: &str) -> Self {
            let guard = crate::llm::TEST_ENV_LOCK
                .lock()
                .unwrap_or_else(|e| e.into_inner());
            std::env::remove_var("FORGE_LLM_API_KEY");
            std::env::remove_var("FORGE_GEN_API_KEY");
            let dir = std::env::temp_dir().join(format!(
                "agentd-custom-{tag}-{}-{}",
                std::process::id(),
                crate::events::new_id("t")
            ));
            std::fs::create_dir_all(&dir).unwrap();
            std::env::set_var("FORGE_GEN_DATA_DIR", &dir);
            TempData { dir, _guard: guard }
        }
    }

    impl Drop for TempData {
        fn drop(&mut self) {
            std::env::remove_var("FORGE_GEN_DATA_DIR");
            std::fs::remove_dir_all(&self.dir).ok();
        }
    }

    async fn body_json(resp: Response) -> (StatusCode, Value) {
        let status = resp.status();
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        (status, serde_json::from_slice(&bytes).unwrap_or(Value::Null))
    }

    fn request(v: Value) -> Json<ModelRequest> {
        Json(serde_json::from_value(v).unwrap())
    }

    #[test]
    fn endpoint_url_respects_version_and_resource_suffixes() {
        use ApiFormat::*;
        assert_eq!(
            endpoint_url("https://api.example.com", Openai),
            "https://api.example.com/v1/chat/completions"
        );
        assert_eq!(
            endpoint_url("https://api.example.com/", Anthropic),
            "https://api.example.com/v1/messages"
        );
        assert_eq!(
            endpoint_url("https://open.bigmodel.cn/api/paas/v4", Openai),
            "https://open.bigmodel.cn/api/paas/v4/chat/completions"
        );
        assert_eq!(
            endpoint_url("https://relay.example.com/v1", Anthropic),
            "https://relay.example.com/v1/messages"
        );
        assert_eq!(
            endpoint_url("https://open.bigmodel.cn/api/anthropic", Anthropic),
            "https://open.bigmodel.cn/api/anthropic/v1/messages"
        );
        assert_eq!(
            endpoint_url("http://127.0.0.1:8000/v1/chat/completions/", Openai),
            "http://127.0.0.1:8000/v1/chat/completions"
        );
        // v1beta 不是纯版本段,照旧补 /v1/…(用户可直接写到资源路径)。
        assert_eq!(
            endpoint_url("https://g.example.com/v1beta/openai", Openai),
            "https://g.example.com/v1beta/openai/v1/chat/completions"
        );
    }

    #[test]
    fn custom_id_shape() {
        assert!(is_custom_id("openai-compat"));
        assert!(is_custom_id("custom:0a1b2c3d"));
        assert!(!is_custom_id("custom:"));
        assert!(!is_custom_id("custom:../x"));
        assert!(!is_custom_id("cloud:gpt-x"));
        assert!(!is_custom_id("deepseek-chat"));
    }

    /// 旧单槽只读迁移:新文件缺失时读旧文件成 openai-compat 条目;首次保存后新文件为准。
    #[tokio::test]
    async fn legacy_file_migrates_on_read_and_save_switches_to_catalog() {
        let t = TempData::new("migrate");
        std::fs::write(
            t.dir.join("llm-openai-compat.json"),
            r#"{"base_url":"https://api.example.com","model":"qwen3","vision":true}"#,
        )
        .unwrap();
        let models = load();
        assert_eq!(models.len(), 1);
        assert_eq!(models[0].id, LEGACY_ID);
        assert_eq!(models[0].model, "qwen3");
        assert!(models[0].vision);
        assert_eq!(models[0].api_format, ApiFormat::Openai);
        assert!(!t.dir.join("llm-custom-models.json").exists(), "读不落盘");
        // 未配 key:不可调用,显式选中 = NOT_CONFIGURED。
        assert!(matches!(provider(LEGACY_ID), Provider::OpenAiCompatNotConfigured));
        assert!(default_provider().is_none());

        let (status, v) = body_json(
            upsert_handler(request(json!({
                "baseUrl": "https://relay.example.com/v1", "model": "claude-sonnet-5-5",
                "apiFormat": "anthropic", "name": "Claude 中转", "key": "sk-test-custom-1",
                "contextWindow": 204800, "maxOutputTokens": 65536, "vision": true,
            })))
            .await,
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{v}");
        let id = v["model"]["id"].as_str().unwrap().to_string();
        assert!(id.starts_with(ID_PREFIX));
        assert_eq!(v["model"]["endpoint"], "https://relay.example.com/v1/messages");
        assert_eq!(v["model"]["configured"], true);
        assert!(t.dir.join("llm-custom-models.json").exists());
        // 迁移来的旧条目随首次保存落进新文件,且排在前面。
        let models = load();
        assert_eq!(models.len(), 2);
        assert_eq!(models[0].id, LEGACY_ID);
        assert_eq!(models[1].id, id);

        match provider(&id) {
            Provider::OpenAiCompat {
                base_url, model, key, api,
            } => {
                assert_eq!(base_url, "https://relay.example.com/v1");
                assert_eq!(model, "claude-sonnet-5-5");
                assert_eq!(key, "sk-test-custom-1");
                assert_eq!(api.id, id);
                assert_eq!(api.format, ApiFormat::Anthropic);
                assert_eq!(api.max_output_tokens, Some(65536));
                assert!(api.vision);
            }
            other => panic!("配齐应可调用: {other:?}"),
        }
        // 旧单槽没 key → 默认渠道取第一个配齐的条目。
        match default_provider() {
            Some(Provider::OpenAiCompat { api, .. }) => assert_eq!(api.id, id),
            other => panic!("默认渠道应为第一个配齐条目: {other:?}"),
        }
    }

    /// 清单/新增/更新/删除全程不回显密钥(R-5);更新时 key 省略 = 保留;删除连带清密钥。
    #[tokio::test]
    async fn crud_never_echoes_key_and_delete_removes_key() {
        let _t = TempData::new("crud");
        let secret = "sk-test-custom-REDLINE";
        let (_, created) = body_json(
            upsert_handler(request(json!({
                "baseUrl": "http://127.0.0.1:8000", "model": "m1", "key": secret,
            })))
            .await,
        )
        .await;
        assert!(!created.to_string().contains(secret));
        let id = created["model"]["id"].as_str().unwrap().to_string();
        assert_eq!(created["model"]["apiFormat"], "openai");
        assert_eq!(created["model"]["contextWindow"], DEFAULT_CONTEXT_WINDOW);
        assert_eq!(created["model"]["contextWindowSet"], false);
        assert_eq!(created["model"]["maxOutputTokens"], Value::Null);
        assert_eq!(created["model"]["isDefault"], true);

        // 更新不带 key:密钥保留,字段整条替换。
        let (status, updated) = body_json(
            upsert_handler(request(json!({
                "id": id, "baseUrl": "http://127.0.0.1:9000", "model": "m2",
                "apiFormat": "anthropic", "contextWindow": 131072, "maxOutputTokens": 8192,
            })))
            .await,
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(updated["model"]["keyConfigured"], true);
        assert_eq!(updated["model"]["model"], "m2");
        assert_eq!(updated["model"]["contextWindowSet"], true);

        let list = list_handler().await.0;
        assert_eq!(list["models"].as_array().unwrap().len(), 1);
        assert_eq!(list["defaultId"], id.as_str());
        assert!(!list.to_string().contains(secret), "清单泄漏密钥(R-5)");

        // 不存在的 id 更新 → 404;删除 → 200 + 密钥清掉;再删 → 404。
        let (status, _) = body_json(
            upsert_handler(request(json!({
                "id": "custom:deadbeef", "baseUrl": "http://x", "model": "m",
            })))
            .await,
        )
        .await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        let (status, _) = body_json(delete_handler(Path(id.clone())).await).await;
        assert_eq!(status, StatusCode::OK);
        assert!(gend::keystore::Keystore::load().secret_for(&id).is_none());
        let (status, _) = body_json(delete_handler(Path(id)).await).await;
        assert_eq!(status, StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn validation_rejects_bad_forms() {
        let _t = TempData::new("validate");
        for (body, code) in [
            (json!({ "baseUrl": "", "model": "m" }), "EMPTY_BASE_URL"),
            (json!({ "baseUrl": "api.example.com", "model": "m" }), "INVALID_BASE_URL"),
            (json!({ "baseUrl": "http://x", "model": " " }), "EMPTY_MODEL"),
            (json!({ "baseUrl": "http://x", "model": "m", "apiFormat": "gemini" }), "INVALID_API_FORMAT"),
            (json!({ "baseUrl": "http://x", "model": "m", "contextWindow": 10 }), "INVALID_CONTEXT_WINDOW"),
            (
                json!({ "baseUrl": "http://x", "model": "m", "contextWindow": 65536, "maxOutputTokens": 131072 }),
                "INVALID_MAX_OUTPUT",
            ),
            (json!({ "baseUrl": "http://x", "model": "m", "maxOutputTokens": 0 }), "INVALID_MAX_OUTPUT"),
        ] {
            let (status, v) = body_json(upsert_handler(request(body.clone())).await).await;
            assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
            assert_eq!(v["error"]["code"], code, "{body}");
        }
        // 测试连接:无 key 且无已存密钥 → MISSING_KEY(不发请求)。
        let (status, v) = body_json(
            test_handler(request(json!({ "baseUrl": "http://127.0.0.1:1", "model": "m" }))).await,
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(v["error"]["code"], "MISSING_KEY");
    }

    /// 旧单槽端点:落在 openai-compat 条目上,保留新字段;status 形态不变且无 key。
    #[tokio::test]
    async fn legacy_endpoints_operate_on_legacy_entry() {
        let _t = TempData::new("legacy");
        let (_, v) = body_json(
            legacy_config_handler(Json(LegacyConfigRequest {
                base_url: "http://127.0.0.1:8000".into(),
                model: "qwen2.5-7b".into(),
                key: Some("sk-test-legacy".into()),
                vision: None,
            }))
            .await,
        )
        .await;
        assert_eq!(v["ok"], true);
        assert_eq!(v["configured"], true);
        assert_eq!(v["vision"], false);
        assert!(!v.to_string().contains("sk-test-legacy"));
        // 新卡片页把它改成 Anthropic + 窗口声明;旧端点再改 baseUrl 不得抹掉这些。
        body_json(
            upsert_handler(request(json!({
                "id": LEGACY_ID, "baseUrl": "http://127.0.0.1:8000", "model": "qwen2.5-7b",
                "apiFormat": "anthropic", "contextWindow": 262144, "vision": true,
            })))
            .await,
        )
        .await;
        body_json(
            legacy_config_handler(Json(LegacyConfigRequest {
                base_url: "http://127.0.0.1:9000".into(),
                model: "qwen2.5-7b".into(),
                key: None,
                vision: None,
            }))
            .await,
        )
        .await;
        let m = find(LEGACY_ID).unwrap();
        assert_eq!(m.base_url, "http://127.0.0.1:9000");
        assert_eq!(m.api_format, ApiFormat::Anthropic);
        assert_eq!(m.context_window, Some(262144));
        assert!(m.vision);
        let st = legacy_status_handler().await.0;
        assert_eq!(st["configured"], true);
        assert_eq!(st["apiFormat"], "anthropic");
    }

    /// 快照条目:声明窗口 → 标准档里更小的各档 + 声明值(默认档);未配齐 needs-key。
    #[test]
    fn snapshot_card_uses_declared_window() {
        let entry = Entry {
            model: CustomModel {
                id: "custom:00000001".into(),
                name: "My Claude".into(),
                api_format: ApiFormat::Anthropic,
                base_url: "https://relay.example.com".into(),
                model: "claude-sonnet-5-5".into(),
                context_window: Some(262_144),
                max_output_tokens: None,
                vision: true,
            },
            key_configured: false,
        };
        let v = snapshot_card(&entry);
        assert_eq!(v["id"], "custom:00000001");
        assert_eq!(v["label"], "My Claude");
        assert_eq!(v["availability"], "needs-key");
        assert_eq!(v["group"], GROUP);
        assert_eq!(v["apiFormat"], "anthropic");
        assert_eq!(v["vision"], true);
        assert_eq!(v["supportsThinking"], true);
        let ids: Vec<&str> = v["contextOptions"]
            .as_array()
            .unwrap()
            .iter()
            .map(|o| o["id"].as_str().unwrap())
            .collect();
        assert_eq!(ids, ["64k", "128k", "200k", "256k"]);
        assert_eq!(v["defaultContext"], "256k");
    }
}
