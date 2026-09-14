//! media — 素材创作媒体生成适配器层(素材创作波:视频/音频/3D 网格)。
//!
//! 平行于 backends.rs 的 text2img 面,不动现有 GenBackend trait(不波及 gen-image-mcp)。
//! 本地 ComfyUI H3、真实远程供应商及 OpenAI 兼容风格适配器:
//! - comfyui-minimax-h3: 本机 ComfyUI 原生 H3 工作流，单次提交 → 轮询 → 下载 MP4；无需 key。
//! - meshy(3D 默认供应商,真实 API):POST {endpoint}/openapi/v2/text-to-3d(preview→refine
//!   两阶段)/ POST {endpoint}/openapi/v1/image-to-3d,建任务 → 轮询 status → 下载 model_urls.glb
//! - remote-video-compatible: POST {endpoint}/v1/videos/generations → data[] b64_json/url(mp4);
//!   params.imageDataUrl 非空 = 图生视频(body 增 image 字段,照 meshy image-to-3d 的参考图处置)
//! - remote-audio-compatible: tts POST {endpoint}/v1/audio/speech(OpenAI 真实格式,原始音频字节);
//!   music POST {endpoint}/v1/music/generations → data[](mp3)
//! - remote-mesh-compatible: POST {endpoint}/v1/meshes/generations → data[](glb),自建/兼容端点兜底
//! 配置复用 gen-backends.json(BackendEntry)与 keystore(键 = 适配器 id);
//! 条目缺失或所需连接信息未配 → configured=false,调用显式 GEN_BACKEND_NOT_CONFIGURED
//! 本地 H3 configured 仅表示已启用且地址有效，生成前另行检查服务节点和权重文件名。
//! (诚实占位,不伪造产物)。错误映射照 remote.rs:429 → GEN_RATE_LIMITED,其余 → GEN_BACKEND_ERROR。
//! 红线 R-5:密钥只进 Authorization 头,错误信息不回显密钥/请求头(供应商错误体回显前经 redact_key)。

use base64::Engine;
use serde_json::{json, Value};

mod aliyun_minimax;
pub use aliyun_minimax::{AliyunMiniMaxVideo, ALIYUN_MINIMAX_VIDEO_ID};
mod xzapi;
pub use xzapi::{XzapiVideo, XZAPI_VIDEO_ID};
mod comfyui_h3;
pub use comfyui_h3::{ComfyuiMiniMaxH3, COMFYUI_MINIMAX_H3_ID};

use crate::config::GenConfig;
use crate::keystore::Keystore;
use crate::{
    GenError, Result, GEN_BACKEND_ERROR, GEN_BACKEND_NOT_CONFIGURED, GEN_BAD_PARAMS,
    GEN_RATE_LIMITED,
};

/// 媒体生成种类(能力面字符串与 capabilities.kinds 一致)。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MediaKind {
    Video,
    Tts,
    Music,
    Mesh,
}

impl MediaKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            MediaKind::Video => "text2video",
            MediaKind::Tts => "tts",
            MediaKind::Music => "music",
            MediaKind::Mesh => "text2mesh",
        }
    }

    /// REST 参数字符串 → kind(未知 → None,调用方回 GEN_BAD_PARAMS)。
    /// image2video 与 text2video 同为 Video 通道:有无参考图由 params.imageDataUrl 决定,
    /// 不因此分裂出第二个 kind(否则 capabilities/路由/白名单三处都要各记一份同义词)。
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "text2video" | "image2video" | "video" => Some(MediaKind::Video),
            "tts" => Some(MediaKind::Tts),
            "music" => Some(MediaKind::Music),
            "text2mesh" | "mesh" => Some(MediaKind::Mesh),
            _ => None,
        }
    }
}

/// 媒体生成请求。专有参数进 params(video: aspect/durationSec/resolution/n;
/// tts: voice/format;music: lyrics/instrumental;mesh: style),适配器如实透传。
#[derive(Debug, Clone)]
pub struct MediaRequest {
    pub kind: MediaKind,
    pub prompt: String,
    pub params: Value,
}

/// 产物附带的预览图(供应商侧渲染的 PNG)。3D 产物在浏览器里无内联渲染面,
/// 这是「看见生成结果」的唯一诚实来源——不是本仓画的,是供应商对同一产物的渲染。
/// label 即视角名(front/right/back/left/alpha)。
#[derive(Debug, Clone)]
pub struct MediaPreview {
    pub label: String,
    pub png: Vec<u8>,
}

/// 媒体产物(字节 + 扩展名 + 元数据;ext 不带点:mp4/mp3/wav/glb)。
#[derive(Debug, Clone)]
pub struct MediaArtifact {
    pub bytes: Vec<u8>,
    pub ext: String,
    pub meta: Value,
    /// 预览图(无则空;签名 URL 会过期,故此处存的是已下载的字节)。
    pub previews: Vec<MediaPreview>,
}

impl MediaArtifact {
    /// 无预览图的产物(视频/音频/兼容端点骨架)。
    pub fn plain(bytes: Vec<u8>, ext: &str, meta: Value) -> Self {
        MediaArtifact { bytes, ext: ext.to_string(), meta, previews: Vec::new() }
    }
}

/// 媒体后端适配器接口(平行 GenBackend;capabilities.kinds 如实)。
pub trait MediaBackend {
    fn id(&self) -> &str;
    /// local | remote。
    fn kind(&self) -> &str;
    fn configured(&self, cfg: &GenConfig, keys: &Keystore) -> bool;
    fn capabilities(&self) -> Value;
    fn generate(
        &self,
        req: &MediaRequest,
        cfg: &GenConfig,
        keys: &Keystore,
    ) -> Result<Vec<MediaArtifact>>;
}

pub const REMOTE_VIDEO_ID: &str = "remote-video-compatible";
pub const REMOTE_AUDIO_ID: &str = "remote-audio-compatible";
pub const REMOTE_MESH_ID: &str = "remote-mesh-compatible";

/// media 注册表。顺序即优先级:resolve_backend 无显式 id 时取首个已配置者,
/// 故 meshy 排在 remote-mesh-compatible 之前 = 3D 生成默认供应商。
pub fn media_registry() -> Vec<Box<dyn MediaBackend>> {
    vec![Box::new(ComfyuiMiniMaxH3), Box::new(XzapiVideo), Box::new(AliyunMiniMaxVideo), Box::new(RemoteVideo), Box::new(RemoteAudio), Box::new(MeshyMesh), Box::new(RemoteMesh)]
}

/// 按 id 查 media 适配器。
pub fn find_media(id: &str) -> Option<Box<dyn MediaBackend>> {
    media_registry().into_iter().find(|b| b.id() == id)
}

/// 支持某 kind 的适配器清单(顺序 = 注册表序)。
pub fn backends_for_kind(kind: MediaKind) -> Vec<Box<dyn MediaBackend>> {
    media_registry()
        .into_iter()
        .filter(|b| {
            b.capabilities()
                .get("kinds")
                .and_then(Value::as_array)
                .map(|arr| arr.iter().any(|k| k.as_str() == Some(kind.as_str())))
                .unwrap_or(false)
        })
        .collect()
}

/// 解析请求后端:显式 backend id 优先(须支持该 kind),否则取第一个已配置的;
/// 全未配置 → GEN_BACKEND_NOT_CONFIGURED(错误信息列出可配置条目 id,便于设置页引导)。
pub fn resolve_backend(
    kind: MediaKind,
    explicit: Option<&str>,
    cfg: &GenConfig,
    keys: &Keystore,
) -> Result<Box<dyn MediaBackend>> {
    let candidates = backends_for_kind(kind);
    if let Some(id) = explicit {
        let b = candidates
            .into_iter()
            .find(|b| b.id() == id)
            .ok_or_else(|| {
                GenError::new(GEN_BAD_PARAMS, format!("后端 {id} 不存在或不支持 {}", kind.as_str()))
            })?;
        if !b.configured(cfg, keys) {
            return Err(GenError::new(
                GEN_BACKEND_NOT_CONFIGURED,
                format!("后端 {id} 未配置(需启用并完成该后端的连接设置)"),
            ));
        }
        return Ok(b);
    }
    let ids: Vec<String> = candidates.iter().map(|b| b.id().to_string()).collect();
    candidates
        .into_iter()
        .find(|b| b.configured(cfg, keys))
        .ok_or_else(|| {
            GenError::new(
                GEN_BACKEND_NOT_CONFIGURED,
                format!("无已配置的 {} 生成后端(可在设置页配置: {})", kind.as_str(), ids.join(" / ")),
            )
        })
}

// ---------- 共享:remote 条目判定 + HTTP ----------

/// remote 类配置判定(照 remote.rs:enabled + endpoint 非空 + key 存在)。
fn remote_configured(id: &str, cfg: &GenConfig, keys: &Keystore) -> bool {
    match cfg.entry(id) {
        Some(e) => {
            e.kind == "remote"
                && e.enabled
                && e.endpoint.as_deref().map(|s| !s.trim().is_empty()).unwrap_or(false)
                && keys.key_for(id).is_some()
        }
        None => false,
    }
}

/// 未配置守卫 + 取 endpoint/key/model(configured 为真后调用)。
fn remote_conn(
    id: &str,
    cfg: &GenConfig,
    keys: &Keystore,
) -> Result<(String, String, Option<String>)> {
    if !remote_configured(id, cfg, keys) {
        return Err(GenError::new(
            GEN_BACKEND_NOT_CONFIGURED,
            format!("{id} 未配置(需 enabled + endpoint + key)"),
        ));
    }
    let entry = cfg.entry(id).expect("configured 为真必有条目");
    let endpoint = entry
        .endpoint
        .as_deref()
        .expect("configured 为真必有 endpoint")
        .trim_end_matches('/')
        .to_string();
    // key 只用于拼 Authorization 头;任何分支不得把 key 写入错误信息。
    let key = keys.key_for(id).expect("configured 为真必有 key");
    Ok((endpoint, key, entry.model.clone()))
}

/// POST JSON,返回原始响应体字节。429 → GEN_RATE_LIMITED,其余错误 → GEN_BACKEND_ERROR。
fn post_json(url: &str, key: &str, body: &Value, timeout_secs: u64) -> Result<Vec<u8>> {
    let agent = ureq::AgentBuilder::new()
        .timeout(std::time::Duration::from_secs(timeout_secs))
        .build();
    let resp = agent
        .post(url)
        .set("Authorization", &format!("Bearer {key}"))
        .set("Content-Type", "application/json")
        .send_string(&body.to_string());
    match resp {
        Ok(r) => read_body(r),
        Err(ureq::Error::Status(429, _)) => {
            Err(GenError::new(GEN_RATE_LIMITED, "远程后端限流(HTTP 429)"))
        }
        Err(ureq::Error::Status(code, _)) => {
            Err(GenError::new(GEN_BACKEND_ERROR, format!("远程后端 HTTP {code}")))
        }
        Err(ureq::Error::Transport(t)) => {
            Err(GenError::new(GEN_BACKEND_ERROR, format!("远程后端连接失败: {t}")))
        }
    }
}

/// 响应体读全。
fn read_body(resp: ureq::Response) -> Result<Vec<u8>> {
    use std::io::Read;
    let mut buf = Vec::new();
    resp.into_reader()
        .read_to_end(&mut buf)
        .map_err(|e| GenError::new(GEN_BACKEND_ERROR, format!("读远程响应体失败: {e}")))?;
    Ok(buf)
}

/// url 候选的二次 GET(images API 同款双形态)。
fn fetch_url(url: &str, timeout_secs: u64) -> Result<Vec<u8>> {
    let agent = ureq::AgentBuilder::new()
        .timeout(std::time::Duration::from_secs(timeout_secs))
        .build();
    match agent.get(url).call() {
        Ok(r) => read_body(r),
        Err(ureq::Error::Status(code, _)) => {
            Err(GenError::new(GEN_BACKEND_ERROR, format!("产物 url 拉取 HTTP {code}")))
        }
        Err(ureq::Error::Transport(t)) => {
            Err(GenError::new(GEN_BACKEND_ERROR, format!("产物 url 拉取连接失败: {t}")))
        }
    }
}

/// 解析 images API 同构响应:{data:[{b64_json|url}]} → 产物字节列表。
fn parse_data_artifacts(bytes: &[u8], ext: &str, timeout_secs: u64) -> Result<Vec<Vec<u8>>> {
    let doc: Value = serde_json::from_slice(bytes)
        .map_err(|e| GenError::new(GEN_BACKEND_ERROR, format!("远程响应非 JSON: {e}")))?;
    let data = doc
        .get("data")
        .and_then(Value::as_array)
        .ok_or_else(|| GenError::new(GEN_BACKEND_ERROR, "远程响应缺 data[]"))?;
    let mut out = Vec::new();
    for (i, item) in data.iter().enumerate() {
        if let Some(b64) = item.get("b64_json").and_then(Value::as_str) {
            let raw = base64::engine::general_purpose::STANDARD
                .decode(b64)
                .map_err(|e| GenError::new(GEN_BACKEND_ERROR, format!("b64_json 解码失败: {e}")))?;
            out.push(raw);
        } else if let Some(u) = item.get("url").and_then(Value::as_str) {
            out.push(fetch_url(u, timeout_secs)?);
        } else {
            return Err(GenError::new(
                GEN_BACKEND_ERROR,
                format!("远程响应 data[{i}] 无 b64_json/url({ext})"),
            ));
        }
    }
    if out.is_empty() {
        return Err(GenError::new(GEN_BACKEND_ERROR, "远程响应 data[] 为空"));
    }
    Ok(out)
}

fn str_param<'a>(params: &'a Value, key: &str) -> Option<&'a str> {
    params.get(key).and_then(Value::as_str).filter(|s| !s.trim().is_empty())
}

fn bool_param(params: &Value, key: &str) -> Option<bool> {
    params.get(key).and_then(Value::as_bool)
}

fn u64_param(params: &Value, key: &str) -> Option<u64> {
    params.get(key).and_then(Value::as_u64)
}

/// 供应商错误体回显前的密钥擦除(R-5 兜底:响应体理论上可能回显请求内容)。
fn redact_key(text: &str, key: &str) -> String {
    if key.is_empty() {
        return text.to_string();
    }
    text.replace(key, "***")
}

// ---------- remote-video-compatible ----------

/// 视频生成远程骨架(预留 API 接口面):
/// `POST {endpoint}/v1/videos/generations {model?, prompt, image?, aspect, resolution,
/// durationSec, n}` → `{data:[{b64_json|url}]}`(mp4)。
/// image = 参考图(公网 URL 或 base64 data URI);给了即图生视频,prompt 转作动作引导。
pub struct RemoteVideo;

/// 视频比例白名单。
pub const VIDEO_ASPECTS: [&str; 3] = ["16:9", "9:16", "1:1"];
/// 视频分辨率档白名单。
pub const VIDEO_RESOLUTIONS: [&str; 3] = ["720p", "1080p", "2k"];
/// 时长上限(秒;预留面,端点各自再钳)。
pub const VIDEO_MAX_DURATION_SEC: u64 = 10;
/// 视频生成 HTTP 超时(出片慢,给足)。
const VIDEO_TIMEOUT_SECS: u64 = 300;

impl MediaBackend for RemoteVideo {
    fn id(&self) -> &str {
        REMOTE_VIDEO_ID
    }

    fn kind(&self) -> &str {
        "remote"
    }

    fn configured(&self, cfg: &GenConfig, keys: &Keystore) -> bool {
        remote_configured(REMOTE_VIDEO_ID, cfg, keys)
    }

    fn capabilities(&self) -> Value {
        json!({
            "kinds": ["text2video", "image2video"],
            "aspects": VIDEO_ASPECTS,
            "resolutions": VIDEO_RESOLUTIONS,
            "maxDurationSec": VIDEO_MAX_DURATION_SEC,
            "maxBatch": 1,
            "formats": ["mp4"],
        })
    }

    fn generate(
        &self,
        req: &MediaRequest,
        cfg: &GenConfig,
        keys: &Keystore,
    ) -> Result<Vec<MediaArtifact>> {
        if req.kind != MediaKind::Video {
            return Err(GenError::new(
                GEN_BAD_PARAMS,
                "remote-video-compatible 仅支持 text2video / image2video",
            ));
        }
        let image = str_param(&req.params, "imageDataUrl")
            .or_else(|| str_param(&req.params, "imageUrl"));
        if req.prompt.trim().is_empty() && image.is_none() {
            return Err(GenError::new(GEN_BAD_PARAMS, "prompt 与 imageDataUrl 至少其一非空"));
        }
        let aspect = str_param(&req.params, "aspect").unwrap_or("16:9");
        if !VIDEO_ASPECTS.contains(&aspect) {
            return Err(GenError::new(
                GEN_BAD_PARAMS,
                format!("aspect 须为 {VIDEO_ASPECTS:?} 之一,实: {aspect}"),
            ));
        }
        let resolution = str_param(&req.params, "resolution").unwrap_or("720p");
        if !VIDEO_RESOLUTIONS.contains(&resolution) {
            return Err(GenError::new(
                GEN_BAD_PARAMS,
                format!("resolution 须为 {VIDEO_RESOLUTIONS:?} 之一,实: {resolution}"),
            ));
        }
        let duration = req.params.get("durationSec").and_then(Value::as_u64).unwrap_or(5);
        if duration == 0 || duration > VIDEO_MAX_DURATION_SEC {
            return Err(GenError::new(
                GEN_BAD_PARAMS,
                format!("durationSec 须 1..={VIDEO_MAX_DURATION_SEC},实: {duration}"),
            ));
        }
        let (endpoint, key, model) = remote_conn(REMOTE_VIDEO_ID, cfg, keys)?;
        let url = format!("{endpoint}/v1/videos/generations");
        let mut body = json!({
            "prompt": req.prompt,
            "aspect": aspect,
            "resolution": resolution,
            "durationSec": duration,
            "n": 1,
        });
        if let Some(img) = image {
            body["image"] = json!(img);
        }
        if let Some(m) = model {
            body["model"] = json!(m);
        }
        let mode = if image.is_some() { "image2video" } else { "text2video" };
        let resp = post_json(&url, &key, &body, VIDEO_TIMEOUT_SECS)?;
        let arts = parse_data_artifacts(&resp, "mp4", VIDEO_TIMEOUT_SECS)?;
        Ok(arts
            .into_iter()
            .map(|bytes| {
                MediaArtifact::plain(
                    bytes,
                    "mp4",
                    json!({
                        "mode": mode,
                        "aspect": aspect,
                        "resolution": resolution,
                        "durationSec": duration,
                    }),
                )
            })
            .collect())
    }
}

// ---------- remote-audio-compatible ----------

/// 音频生成远程骨架:tts 走 OpenAI 真实格式 /v1/audio/speech(响应 = 原始音频字节),
/// music 走 /v1/music/generations(images API 同构 data[])。
pub struct RemoteAudio;

/// TTS 输出格式白名单(响应即该格式原始字节)。
pub const AUDIO_FORMATS: [&str; 2] = ["mp3", "wav"];
const AUDIO_TIMEOUT_SECS: u64 = 120;

impl MediaBackend for RemoteAudio {
    fn id(&self) -> &str {
        REMOTE_AUDIO_ID
    }

    fn kind(&self) -> &str {
        "remote"
    }

    fn configured(&self, cfg: &GenConfig, keys: &Keystore) -> bool {
        remote_configured(REMOTE_AUDIO_ID, cfg, keys)
    }

    fn capabilities(&self) -> Value {
        json!({
            "kinds": ["tts", "music"],
            "formats": AUDIO_FORMATS,
            "maxBatch": 1,
        })
    }

    fn generate(
        &self,
        req: &MediaRequest,
        cfg: &GenConfig,
        keys: &Keystore,
    ) -> Result<Vec<MediaArtifact>> {
        match req.kind {
            MediaKind::Tts => self.generate_tts(req, cfg, keys),
            MediaKind::Music => self.generate_music(req, cfg, keys),
            _ => Err(GenError::new(GEN_BAD_PARAMS, "remote-audio-compatible 仅支持 tts/music")),
        }
    }
}

impl RemoteAudio {
    /// OpenAI TTS 真实格式:POST /v1/audio/speech {model,input,voice,response_format},
    /// 成功响应体 = 音频原始字节。
    fn generate_tts(
        &self,
        req: &MediaRequest,
        cfg: &GenConfig,
        keys: &Keystore,
    ) -> Result<Vec<MediaArtifact>> {
        let format = str_param(&req.params, "format").unwrap_or("mp3");
        if !AUDIO_FORMATS.contains(&format) {
            return Err(GenError::new(
                GEN_BAD_PARAMS,
                format!("format 须为 {AUDIO_FORMATS:?} 之一,实: {format}"),
            ));
        }
        let voice = str_param(&req.params, "voice").unwrap_or("alloy");
        let (endpoint, key, model) = remote_conn(REMOTE_AUDIO_ID, cfg, keys)?;
        let url = format!("{endpoint}/v1/audio/speech");
        let mut body = json!({
            "input": req.prompt,
            "voice": voice,
            "response_format": format,
        });
        if let Some(m) = model {
            body["model"] = json!(m);
        }
        let bytes = post_json(&url, &key, &body, AUDIO_TIMEOUT_SECS)?;
        if bytes.is_empty() {
            return Err(GenError::new(GEN_BACKEND_ERROR, "TTS 响应体为空"));
        }
        Ok(vec![MediaArtifact::plain(
            bytes,
            format,
            json!({ "mode": "tts", "voice": voice }),
        )])
    }

    /// 音乐生成(预留约定,images API 同构):POST /v1/music/generations
    /// {model?,prompt,lyrics?,instrumental?} → data[] b64_json/url。
    fn generate_music(
        &self,
        req: &MediaRequest,
        cfg: &GenConfig,
        keys: &Keystore,
    ) -> Result<Vec<MediaArtifact>> {
        let (endpoint, key, model) = remote_conn(REMOTE_AUDIO_ID, cfg, keys)?;
        let url = format!("{endpoint}/v1/music/generations");
        let mut body = json!({ "prompt": req.prompt });
        if let Some(lyrics) = str_param(&req.params, "lyrics") {
            body["lyrics"] = json!(lyrics);
        }
        if let Some(inst) = req.params.get("instrumental").and_then(Value::as_bool) {
            body["instrumental"] = json!(inst);
        }
        if let Some(m) = model {
            body["model"] = json!(m);
        }
        let resp = post_json(&url, &key, &body, AUDIO_TIMEOUT_SECS)?;
        let arts = parse_data_artifacts(&resp, "mp3", AUDIO_TIMEOUT_SECS)?;
        Ok(arts
            .into_iter()
            .map(|bytes| MediaArtifact::plain(bytes, "mp3", json!({ "mode": "music" })))
            .collect())
    }
}

// ---------- meshy(3D 生成默认供应商) ----------

/// Meshy 适配器 id(注册表中排在 remote-mesh-compatible 之前 = mesh 类默认供应商)。
pub const MESHY_ID: &str = "meshy";
/// Meshy 官方 API 根。条目未填 endpoint 时用此缺省——供应商端点是固定事实,
/// 不该逼用户在设置页手抄一遍;填了则以条目为准(自建代理/镜像)。
pub const MESHY_DEFAULT_ENDPOINT: &str = "https://api.meshy.ai";
/// 缺省 ai_model(官方 latest 别名,当前指向 Meshy 7)。
pub const MESHY_DEFAULT_AI_MODEL: &str = "latest";
/// ai_model 白名单(standard 面 + smart-topology 面)。
pub const MESHY_AI_MODELS: [&str; 5] = ["latest", "meshy-7", "meshy-6", "meshy-5", "meshy-t2"];
/// model_type 白名单(官方 lowpoly 已弃用,不进白名单——smart-topology 是其替代)。
pub const MESHY_MODEL_TYPES: [&str; 2] = ["standard", "smart-topology"];
/// 贴图分辨率档。
pub const MESHY_TEXTURE_RESOLUTIONS: [&str; 3] = ["2k", "4k", "8k"];
/// 拓扑档(smart-topology 仅 triangle)。
pub const MESHY_TOPOLOGIES: [&str; 2] = ["triangle", "quad"];
/// pose_mode 档(空串 = 不指定姿势)。
pub const MESHY_POSE_MODES: [&str; 3] = ["", "a-pose", "t-pose"];
/// prompt 字符上限(官方 600)。
const MESHY_PROMPT_MAX: usize = 600;
/// 单次 HTTP 超时(建任务 / 查询 / 产物下载各自独立计时)。
const MESHY_HTTP_TIMEOUT_SECS: u64 = 120;
/// 单阶段(preview / refine / image-to-3d)轮询总预算;params.timeoutSec 可覆盖。
const MESHY_STAGE_BUDGET_SECS: u64 = 900;
/// 轮询间隔。
const MESHY_POLL_INTERVAL_SECS: u64 = 4;
/// standard(remesh 面)target_polycount 区间。
const MESHY_POLYCOUNT_STANDARD: (u64, u64) = (100, 300_000);
/// smart-topology(直出面)target_polycount 区间。
const MESHY_POLYCOUNT_SMART: (u64, u64) = (100, 15_000);

/// Meshy 3D 生成适配器(text-to-3d v2 两阶段 / image-to-3d v1 单阶段)。
///
/// 与其余 media 骨架的本质差异:Meshy 是**异步任务制**——建任务只回 task id,
/// 产物须轮询 status 到 SUCCEEDED 后从签名 URL 下载。整条轮询在 generate() 内阻塞完成,
/// 以维持 MediaBackend「一次调用出产物」的同步契约(调用方 agentd 已用 spawn_blocking 包裹)。
pub struct MeshyMesh;

/// Meshy 密钥:env MESHY_API_KEY 优先(官方文档惯例,便于本地临时覆盖)→
/// keystore["meshy"](其内部再让全局 FORGE_GEN_API_KEY 兜底,沿用 gend 既有约定)。
fn meshy_key(keys: &Keystore) -> Option<String> {
    if let Ok(v) = std::env::var("MESHY_API_KEY") {
        let v = v.trim().to_string();
        if !v.is_empty() {
            return Some(v);
        }
    }
    keys.key_for(MESHY_ID)
}

/// 未配置守卫 + 取 endpoint(缺省官方)/ key / 条目 model。
fn meshy_conn(cfg: &GenConfig, keys: &Keystore) -> Result<(String, String, Option<String>)> {
    let entry = cfg.entry(MESHY_ID).filter(|e| e.enabled).ok_or_else(|| {
        GenError::new(
            GEN_BACKEND_NOT_CONFIGURED,
            "meshy 未启用(设置页 → 模型 → 生成后端 meshy:开启并填 API Key)",
        )
    })?;
    let key = meshy_key(keys).ok_or_else(|| {
        GenError::new(
            GEN_BACKEND_NOT_CONFIGURED,
            "meshy 缺 API Key(设置页填入,或设环境变量 MESHY_API_KEY)",
        )
    })?;
    let endpoint = entry
        .endpoint
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or(MESHY_DEFAULT_ENDPOINT)
        .trim_end_matches('/')
        .to_string();
    Ok((endpoint, key, entry.model.clone()))
}

/// 一次生成的 Meshy 请求档位(白名单校验后的形态;非法值一律 GEN_BAD_PARAMS 前置拒绝,
/// 不把用户的错拖到远端才报)。
struct MeshyOptions {
    model_type: String,
    ai_model: String,
    topology: Option<String>,
    target_polycount: Option<u64>,
    /// false = 只出 preview 几何(不跑 refine 贴图阶段,省额度)。
    texture: bool,
    enable_pbr: bool,
    texture_resolution: String,
    texture_prompt: Option<String>,
    texture_image_url: Option<String>,
    pose_mode: Option<String>,
    /// 求四向预览图。仅 image-to-3d 真出图(实测 text-to-3d 收下该参数但不产
    /// thumbnail_urls),故 text 面不发,免得在请求里立个兑现不了的旗子。
    multi_view: bool,
    budget: std::time::Duration,
}

/// 白名单校验小工具(值不在集合内 → GEN_BAD_PARAMS,消息列出可选项)。
fn pick<'a>(params: &'a Value, key: &str, allowed: &[&str]) -> Result<Option<&'a str>> {
    match params.get(key).and_then(Value::as_str) {
        None => Ok(None),
        Some(v) => {
            if allowed.contains(&v) {
                Ok(Some(v))
            } else {
                Err(GenError::new(
                    GEN_BAD_PARAMS,
                    format!("{key} 须为 {allowed:?} 之一,实: {v}"),
                ))
            }
        }
    }
}

impl MeshyOptions {
    fn parse(params: &Value, cfg_model: Option<&str>) -> Result<Self> {
        let model_type =
            pick(params, "modelType", &MESHY_MODEL_TYPES)?.unwrap_or("standard").to_string();
        let smart = model_type == "smart-topology";
        // ai_model 取值序:显式参数 > 条目 model > 档位缺省(smart-topology 只认 meshy-t2)。
        let requested = str_param(params, "aiModel").or(cfg_model.map(str::trim).filter(|s| !s.is_empty()));
        let ai_model = match requested {
            Some(m) => {
                if !MESHY_AI_MODELS.contains(&m) {
                    return Err(GenError::new(
                        GEN_BAD_PARAMS,
                        format!("aiModel 须为 {MESHY_AI_MODELS:?} 之一,实: {m}"),
                    ));
                }
                m.to_string()
            }
            None if smart => "meshy-t2".to_string(),
            None => MESHY_DEFAULT_AI_MODEL.to_string(),
        };
        if smart && ai_model != "meshy-t2" {
            return Err(GenError::new(
                GEN_BAD_PARAMS,
                format!("modelType=smart-topology 只接受 aiModel=meshy-t2,实: {ai_model}"),
            ));
        }
        let topology = pick(params, "topology", &MESHY_TOPOLOGIES)?.map(str::to_string);
        if smart && topology.as_deref() == Some("quad") {
            return Err(GenError::new(
                GEN_BAD_PARAMS,
                "modelType=smart-topology 只出三角面,不接受 topology=quad",
            ));
        }
        let target_polycount = u64_param(params, "targetPolycount");
        if let Some(n) = target_polycount {
            let (lo, hi) = if smart { MESHY_POLYCOUNT_SMART } else { MESHY_POLYCOUNT_STANDARD };
            if n < lo || n > hi {
                return Err(GenError::new(
                    GEN_BAD_PARAMS,
                    format!("targetPolycount 须 {lo}..={hi}({model_type} 档),实: {n}"),
                ));
            }
        }
        let texture_prompt = str_param(params, "texturePrompt").map(str::to_string);
        if let Some(tp) = &texture_prompt {
            if tp.chars().count() > MESHY_PROMPT_MAX {
                return Err(GenError::new(
                    GEN_BAD_PARAMS,
                    format!("texturePrompt 上限 {MESHY_PROMPT_MAX} 字符,实 {}", tp.chars().count()),
                ));
            }
        }
        Ok(MeshyOptions {
            model_type,
            ai_model,
            topology,
            target_polycount,
            texture: bool_param(params, "texture").unwrap_or(true),
            enable_pbr: bool_param(params, "pbr").unwrap_or(true),
            texture_resolution: pick(params, "textureResolution", &MESHY_TEXTURE_RESOLUTIONS)?
                .unwrap_or("2k")
                .to_string(),
            texture_prompt,
            texture_image_url: str_param(params, "textureImageUrl").map(str::to_string),
            pose_mode: pick(params, "poseMode", &MESHY_POSE_MODES)?
                .filter(|s| !s.is_empty())
                .map(str::to_string),
            multi_view: bool_param(params, "multiView").unwrap_or(true),
            budget: std::time::Duration::from_secs(
                u64_param(params, "timeoutSec").unwrap_or(MESHY_STAGE_BUDGET_SECS),
            ),
        })
    }

    /// 几何档写入建任务 body(preview / image-to-3d 共用)。
    /// standard 档的 target_polycount 只在 remesh 阶段生效,故须同时开 should_remesh;
    /// smart-topology 档直接按面数出模,不跑 remesh。
    fn apply_geometry(&self, body: &mut Value) {
        if self.model_type != "standard" {
            body["model_type"] = json!(self.model_type);
        }
        if let Some(t) = &self.topology {
            body["topology"] = json!(t);
        }
        if let Some(n) = self.target_polycount {
            body["target_polycount"] = json!(n);
            if self.model_type == "standard" {
                body["should_remesh"] = json!(true);
            }
        }
    }

    /// 贴图档写入 body(refine 阶段 / image-to-3d 单阶段共用)。
    /// 官方约定:texture_prompt 与 texture_image_url 同时给时前者生效,故此处二选一。
    fn apply_texture(&self, body: &mut Value) {
        body["enable_pbr"] = json!(self.enable_pbr);
        body["texture_resolution"] = json!(self.texture_resolution);
        if let Some(tp) = &self.texture_prompt {
            body["texture_prompt"] = json!(tp);
        } else if let Some(ti) = &self.texture_image_url {
            body["texture_image_url"] = json!(ti);
        }
    }
}

fn meshy_agent() -> ureq::Agent {
    ureq::AgentBuilder::new()
        .timeout(std::time::Duration::from_secs(MESHY_HTTP_TIMEOUT_SECS))
        .build()
}

/// Meshy HTTP 失败 → GenError。供应商错误体带可诉诸的原因(缺参/额度/模型不匹配),
/// 值得回显,但先经 redact_key 擦除并截断。
fn meshy_http_error(code: u16, body: String, key: &str) -> GenError {
    match code {
        401 | 403 => GenError::new(
            GEN_BACKEND_ERROR,
            format!("Meshy 鉴权失败(HTTP {code}):API Key 无效或已撤销"),
        ),
        402 => GenError::new(GEN_BACKEND_ERROR, "Meshy 额度不足(HTTP 402):账户 credits 已用尽"),
        429 => GenError::new(GEN_RATE_LIMITED, "Meshy 限流(HTTP 429)"),
        _ => {
            let brief: String = redact_key(body.trim(), key).chars().take(300).collect();
            if brief.is_empty() {
                GenError::new(GEN_BACKEND_ERROR, format!("Meshy HTTP {code}"))
            } else {
                GenError::new(GEN_BACKEND_ERROR, format!("Meshy HTTP {code}: {brief}"))
            }
        }
    }
}

fn meshy_post(url: &str, key: &str, body: &Value) -> Result<Vec<u8>> {
    let resp = meshy_agent()
        .post(url)
        .set("Authorization", &format!("Bearer {key}"))
        .set("Content-Type", "application/json")
        .send_string(&body.to_string());
    match resp {
        Ok(r) => read_body(r),
        Err(ureq::Error::Status(code, r)) => {
            Err(meshy_http_error(code, r.into_string().unwrap_or_default(), key))
        }
        Err(ureq::Error::Transport(t)) => {
            Err(GenError::new(GEN_BACKEND_ERROR, format!("Meshy 连接失败: {t}")))
        }
    }
}

fn meshy_get(url: &str, key: &str) -> Result<Vec<u8>> {
    match meshy_agent().get(url).set("Authorization", &format!("Bearer {key}")).call() {
        Ok(r) => read_body(r),
        Err(ureq::Error::Status(code, r)) => {
            Err(meshy_http_error(code, r.into_string().unwrap_or_default(), key))
        }
        Err(ureq::Error::Transport(t)) => {
            Err(GenError::new(GEN_BACKEND_ERROR, format!("Meshy 连接失败: {t}")))
        }
    }
}

fn meshy_json(bytes: &[u8]) -> Result<Value> {
    serde_json::from_slice(bytes)
        .map_err(|e| GenError::new(GEN_BACKEND_ERROR, format!("Meshy 响应非 JSON: {e}")))
}

/// 建任务:POST → {"result": "<task id>"}。
fn meshy_create_task(url: &str, key: &str, body: &Value) -> Result<String> {
    let doc = meshy_json(&meshy_post(url, key, body)?)?;
    doc.get("result")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .ok_or_else(|| GenError::new(GEN_BACKEND_ERROR, "Meshy 建任务响应缺 result(任务 id)"))
}

/// 轮询任务直到 SUCCEEDED;FAILED/CANCELED 与超时都如实报错(不返回半成品)。
fn meshy_poll(base_url: &str, key: &str, task_id: &str, budget: std::time::Duration) -> Result<Value> {
    let url = format!("{base_url}/{task_id}");
    let interval = std::time::Duration::from_secs(MESHY_POLL_INTERVAL_SECS);
    let deadline = std::time::Instant::now() + budget;
    loop {
        let doc = meshy_json(&meshy_get(&url, key)?)?;
        match doc.get("status").and_then(Value::as_str).unwrap_or("") {
            "SUCCEEDED" => return Ok(doc),
            st @ ("FAILED" | "CANCELED") => {
                let msg = doc
                    .pointer("/task_error/message")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .trim();
                let detail = if msg.is_empty() { String::new() } else { format!(": {msg}") };
                return Err(GenError::new(
                    GEN_BACKEND_ERROR,
                    format!("Meshy 任务 {task_id} {st}{detail}"),
                ));
            }
            _ => {}
        }
        let remaining = deadline.saturating_duration_since(std::time::Instant::now());
        if remaining.is_zero() {
            let progress = doc.get("progress").and_then(Value::as_u64).unwrap_or(0);
            return Err(GenError::new(
                GEN_BACKEND_ERROR,
                format!(
                    "Meshy 任务 {task_id} 超出等待预算 {}s(停在 progress={progress}%);可加大 timeoutSec 后重试",
                    budget.as_secs()
                ),
            ));
        }
        std::thread::sleep(interval.min(remaining));
    }
}

/// 从已完成任务下载 glb。签名 URL 无需鉴权头;首 4 字节校验挡住错误页伪装成产物。
fn meshy_download_glb(task: &Value) -> Result<Vec<u8>> {
    let url = task
        .pointer("/model_urls/glb")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| GenError::new(GEN_BACKEND_ERROR, "Meshy 任务完成但无 model_urls.glb"))?;
    let bytes = fetch_url(url, MESHY_HTTP_TIMEOUT_SECS)?;
    if bytes.len() < 12 || &bytes[..4] != b"glTF" {
        return Err(GenError::new(
            GEN_BACKEND_ERROR,
            format!("Meshy 产物不是 glb(首 4 字节非 glTF,共 {} 字节)", bytes.len()),
        ));
    }
    Ok(bytes)
}

/// 视角顺序(供应商 thumbnail_urls 是无序对象,固定顺序才能稳定呈现与断言)。
pub const MESHY_VIEWS: [&str; 4] = ["front", "right", "back", "left"];
/// 单张预览图上限(官方 512×512 PNG,给足余量)。
const MESHY_PREVIEW_MAX_BYTES: usize = 4 * 1024 * 1024;

/// 从完成任务里挑出预览图地址,按固定视角序返回 (label, url)。
/// image-to-3d 开 multi_view_thumbnails 时给四向 thumbnail_urls;
/// text-to-3d 只有单张 thumbnail_url(实测该接口静默忽略 multi_view_thumbnails)。
fn meshy_preview_urls(task: &Value) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = Vec::new();
    if let Some(map) = task.get("thumbnail_urls").and_then(Value::as_object) {
        for view in MESHY_VIEWS {
            if let Some(u) = map.get(view).and_then(Value::as_str).filter(|s| !s.is_empty()) {
                out.push((view.to_string(), u.to_string()));
            }
        }
    }
    if out.is_empty() {
        if let Some(u) = task.get("thumbnail_url").and_then(Value::as_str).filter(|s| !s.is_empty())
        {
            out.push(("front".to_string(), u.to_string()));
        }
    }
    out
}

/// 下载预览图。**失败不阻断生成**——glb 才是产物,预览图是附赠;
/// 为一张缩略图把已付费的模型丢掉是荒谬的。失败视角如实缺席,不占位。
///
/// 单次重试:实测过 TLS 瞬断丢掉一个视角(其余三个正常),而签名 URL 会过期、
/// 丢了就再也补不回来。GET 幂等,重试无副作用。
fn meshy_download_previews(task: &Value) -> Vec<MediaPreview> {
    let mut out = Vec::new();
    for (label, url) in meshy_preview_urls(task) {
        let mut last_err = None;
        for attempt in 0..2 {
            match fetch_url(&url, MESHY_HTTP_TIMEOUT_SECS) {
                Ok(png) if !png.is_empty() && png.len() <= MESHY_PREVIEW_MAX_BYTES => {
                    out.push(MediaPreview { label: label.clone(), png });
                    last_err = None;
                    break;
                }
                Ok(png) => {
                    last_err = Some(format!("尺寸异常({} 字节)", png.len()));
                    break;
                }
                Err(e) => {
                    last_err = Some(e.message);
                    if attempt == 0 {
                        std::thread::sleep(std::time::Duration::from_secs(1));
                    }
                }
            }
        }
        if let Some(e) = last_err {
            eprintln!("[gend] meshy 预览图 {label} 两次拉取均失败({e}),该视角缺席");
        }
    }
    out
}

/// 产物元数据:够 provenance 追溯(任务 id 可在 Meshy 控制台复查),且不含任何密钥。
fn meshy_artifact(
    bytes: Vec<u8>,
    mode: &str,
    task: &Value,
    task_id: &str,
    preview_task_id: Option<&str>,
    opts: &MeshyOptions,
) -> MediaArtifact {
    let mut meta = json!({
        "provider": MESHY_ID,
        "mode": mode,
        "taskId": task_id,
        "aiModel": opts.ai_model,
        "modelType": opts.model_type,
        "textured": opts.texture,
    });
    if let Some(p) = preview_task_id {
        meta["previewTaskId"] = json!(p);
    }
    if let Some(n) = opts.target_polycount {
        meta["targetPolycount"] = json!(n);
    }
    if let Some(t) = task.get("thumbnail_url").and_then(Value::as_str) {
        meta["thumbnailUrl"] = json!(t);
    }
    if let Some(c) = task.get("consumed_credits").and_then(Value::as_u64) {
        meta["consumedCredits"] = json!(c);
    }
    let previews = meshy_download_previews(task);
    meta["previewViews"] = json!(previews.iter().map(|p| p.label.clone()).collect::<Vec<_>>());
    MediaArtifact { bytes, ext: "glb".into(), meta, previews }
}

impl MeshyMesh {
    /// 文生 3D(v2 两阶段):preview 出无贴图几何 → refine 按 preview_task_id 上贴图。
    /// texture=false 时止于 preview(省额度,适合只要形体的占位资产)。
    fn text_to_3d(
        &self,
        endpoint: &str,
        key: &str,
        prompt: &str,
        opts: &MeshyOptions,
    ) -> Result<Vec<MediaArtifact>> {
        let url = format!("{endpoint}/openapi/v2/text-to-3d");
        let mut body = json!({
            "mode": "preview",
            "prompt": prompt,
            "ai_model": opts.ai_model,
            "target_formats": ["glb"],
        });
        opts.apply_geometry(&mut body);
        if let Some(pose) = &opts.pose_mode {
            body["pose_mode"] = json!(pose);
        }
        let preview_id = meshy_create_task(&url, key, &body)?;
        let preview = meshy_poll(&url, key, &preview_id, opts.budget)?;
        if !opts.texture {
            let bytes = meshy_download_glb(&preview)?;
            return Ok(vec![meshy_artifact(bytes, "text-to-3d", &preview, &preview_id, None, opts)]);
        }
        let mut refine = json!({
            "mode": "refine",
            "preview_task_id": preview_id,
            "target_formats": ["glb"],
        });
        // smart-topology 的 preview 用 meshy-t2,而 refine 面不收该值;省略让服务端配对,
        // 避免「模型不匹配」400。standard 档则显式回传,保证 preview/refine 同模型。
        if opts.model_type == "standard" {
            refine["ai_model"] = json!(opts.ai_model);
        }
        opts.apply_texture(&mut refine);
        let refine_id = meshy_create_task(&url, key, &refine)?;
        let done = meshy_poll(&url, key, &refine_id, opts.budget)?;
        let bytes = meshy_download_glb(&done)?;
        Ok(vec![meshy_artifact(bytes, "text-to-3d", &done, &refine_id, Some(&preview_id), opts)])
    }

    /// 图生 3D(v1 单阶段):image_url 收公网 URL 或 base64 data URI。
    fn image_to_3d(
        &self,
        endpoint: &str,
        key: &str,
        image: &str,
        opts: &MeshyOptions,
    ) -> Result<Vec<MediaArtifact>> {
        let url = format!("{endpoint}/openapi/v1/image-to-3d");
        let mut body = json!({
            "image_url": image,
            "ai_model": opts.ai_model,
            "should_texture": opts.texture,
            "target_formats": ["glb"],
        });
        opts.apply_geometry(&mut body);
        if let Some(pose) = &opts.pose_mode {
            body["pose_mode"] = json!(pose);
        }
        if opts.texture {
            opts.apply_texture(&mut body);
        }
        if opts.multi_view {
            body["multi_view_thumbnails"] = json!(true);
        }
        let task_id = meshy_create_task(&url, key, &body)?;
        let done = meshy_poll(&url, key, &task_id, opts.budget)?;
        let bytes = meshy_download_glb(&done)?;
        Ok(vec![meshy_artifact(bytes, "image-to-3d", &done, &task_id, None, opts)])
    }
}

impl MediaBackend for MeshyMesh {
    fn id(&self) -> &str {
        MESHY_ID
    }

    fn kind(&self) -> &str {
        "remote"
    }

    /// endpoint 不参与判定(官方地址有缺省);enabled + key 即可用。
    fn configured(&self, cfg: &GenConfig, keys: &Keystore) -> bool {
        cfg.entry(MESHY_ID).map(|e| e.kind == "remote" && e.enabled).unwrap_or(false)
            && meshy_key(keys).is_some()
    }

    fn capabilities(&self) -> Value {
        json!({
            "kinds": ["text2mesh", "image2mesh"],
            "formats": ["glb"],
            "maxBatch": 1,
            "provider": MESHY_ID,
            "defaultEndpoint": MESHY_DEFAULT_ENDPOINT,
            // 预览图视角数如实分面:图生 3D 供应商渲四向,文生 3D 只给正面。
            "previewViews": { "image2mesh": MESHY_VIEWS, "text2mesh": ["front"] },
            "aiModels": MESHY_AI_MODELS,
            "modelTypes": MESHY_MODEL_TYPES,
            "topologies": MESHY_TOPOLOGIES,
            "textureResolutions": MESHY_TEXTURE_RESOLUTIONS,
            "poseModes": MESHY_POSE_MODES,
            "promptMaxChars": MESHY_PROMPT_MAX,
            "pbr": true,
            "async": true,
        })
    }

    fn generate(
        &self,
        req: &MediaRequest,
        cfg: &GenConfig,
        keys: &Keystore,
    ) -> Result<Vec<MediaArtifact>> {
        if req.kind != MediaKind::Mesh {
            return Err(GenError::new(GEN_BAD_PARAMS, "meshy 仅支持 text2mesh / image2mesh"));
        }
        let (endpoint, key, cfg_model) = meshy_conn(cfg, keys)?;
        let prompt = req.prompt.trim();
        let image = str_param(&req.params, "imageDataUrl")
            .or_else(|| str_param(&req.params, "imageUrl"));
        if prompt.is_empty() && image.is_none() {
            return Err(GenError::new(GEN_BAD_PARAMS, "prompt 与 imageDataUrl 至少其一非空"));
        }
        if prompt.chars().count() > MESHY_PROMPT_MAX {
            return Err(GenError::new(
                GEN_BAD_PARAMS,
                format!("prompt 上限 {MESHY_PROMPT_MAX} 字符,实 {}", prompt.chars().count()),
            ));
        }
        let mut opts = MeshyOptions::parse(&req.params, cfg_model.as_deref())?;
        // 图生 3D 面:prompt 不进几何请求(官方无该参数),转作贴图引导,不白丢用户输入。
        if image.is_some() && opts.texture_prompt.is_none() && !prompt.is_empty() {
            opts.texture_prompt = Some(prompt.to_string());
        }
        match image {
            Some(img) => self.image_to_3d(&endpoint, &key, img, &opts),
            None => self.text_to_3d(&endpoint, &key, prompt, &opts),
        }
    }
}

// ---------- remote-mesh-compatible ----------

/// 3D 网格生成远程骨架(预留约定,images API 同构):
/// POST {endpoint}/v1/meshes/generations {model?,prompt,style?} → data[] b64_json/url(glb)。
/// meshy 之外的自建 / OpenAI 风格兼容端点兜底位。
pub struct RemoteMesh;

const MESH_TIMEOUT_SECS: u64 = 300;

impl MediaBackend for RemoteMesh {
    fn id(&self) -> &str {
        REMOTE_MESH_ID
    }

    fn kind(&self) -> &str {
        "remote"
    }

    fn configured(&self, cfg: &GenConfig, keys: &Keystore) -> bool {
        remote_configured(REMOTE_MESH_ID, cfg, keys)
    }

    fn capabilities(&self) -> Value {
        json!({
            "kinds": ["text2mesh"],
            "formats": ["glb"],
            "maxBatch": 1,
        })
    }

    fn generate(
        &self,
        req: &MediaRequest,
        cfg: &GenConfig,
        keys: &Keystore,
    ) -> Result<Vec<MediaArtifact>> {
        if req.kind != MediaKind::Mesh {
            return Err(GenError::new(GEN_BAD_PARAMS, "remote-mesh-compatible 仅支持 text2mesh"));
        }
        let (endpoint, key, model) = remote_conn(REMOTE_MESH_ID, cfg, keys)?;
        let url = format!("{endpoint}/v1/meshes/generations");
        let mut body = json!({ "prompt": req.prompt });
        if let Some(style) = str_param(&req.params, "style") {
            body["style"] = json!(style);
        }
        if let Some(m) = model {
            body["model"] = json!(m);
        }
        let resp = post_json(&url, &key, &body, MESH_TIMEOUT_SECS)?;
        let arts = parse_data_artifacts(&resp, "glb", MESH_TIMEOUT_SECS)?;
        Ok(arts
            .into_iter()
            .map(|bytes| MediaArtifact::plain(bytes, "glb", json!({})))
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::BackendEntry;
    use crate::TEST_ENV_LOCK as ENV_LOCK;
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::sync::{Arc, Mutex};

    fn env_lock() -> std::sync::MutexGuard<'static, ()> {
        ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// 一次性 HTTP 应答桩(照 remote.rs tests)。
    fn http_stub_once(status_line: &'static str, body: &'static [u8]) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").expect("绑定随机端口失败");
        let port = listener.local_addr().unwrap().port();
        std::thread::spawn(move || {
            if let Ok((mut s, _)) = listener.accept() {
                let mut req = Vec::new();
                let mut chunk = [0u8; 4096];
                loop {
                    let n = s.read(&mut chunk).unwrap_or(0);
                    if n == 0 {
                        break;
                    }
                    req.extend_from_slice(&chunk[..n]);
                    if let Some(pos) = find_subslice(&req, b"\r\n\r\n") {
                        let head = String::from_utf8_lossy(&req[..pos]).to_string();
                        let len = head
                            .lines()
                            .find_map(|l| {
                                l.to_ascii_lowercase()
                                    .strip_prefix("content-length:")
                                    .map(str::trim)
                                    .and_then(|v| v.parse::<usize>().ok())
                            })
                            .unwrap_or(0);
                        if req.len() >= pos + 4 + len {
                            break;
                        }
                    }
                }
                let resp = format!(
                    "HTTP/1.1 {status_line}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    body.len()
                );
                let _ = s.write_all(resp.as_bytes());
                let _ = s.write_all(body);
            }
        });
        format!("http://127.0.0.1:{port}")
    }

    fn find_subslice(hay: &[u8], needle: &[u8]) -> Option<usize> {
        hay.windows(needle.len()).position(|w| w == needle)
    }

    fn content_length(head: &str) -> usize {
        head.lines()
            .find_map(|l| {
                l.to_ascii_lowercase()
                    .strip_prefix("content-length:")
                    .map(str::trim)
                    .and_then(|v| v.parse::<usize>().ok())
            })
            .unwrap_or(0)
    }

    /// 多请求 HTTP 桩(Meshy 异步任务链需要建任务/轮询/下载多次往返,
    /// 一次性桩不够用)。handler(base, method, path, body) → (status, payload);
    /// base 是桩自身根地址,供应答体里回填产物下载地址用。
    fn http_stub_server<F>(handler: F) -> String
    where
        F: Fn(&str, &str, &str, &[u8]) -> (u16, Vec<u8>) + Send + 'static,
    {
        let listener = TcpListener::bind("127.0.0.1:0").expect("绑定随机端口失败");
        let port = listener.local_addr().unwrap().port();
        let base = format!("http://127.0.0.1:{port}");
        let base_thread = base.clone();
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut s) = stream else { break };
                let mut req = Vec::new();
                let mut chunk = [0u8; 4096];
                loop {
                    if let Some(pos) = find_subslice(&req, b"\r\n\r\n") {
                        let head = String::from_utf8_lossy(&req[..pos]).to_string();
                        if req.len() >= pos + 4 + content_length(&head) {
                            break;
                        }
                    }
                    let n = s.read(&mut chunk).unwrap_or(0);
                    if n == 0 {
                        break;
                    }
                    req.extend_from_slice(&chunk[..n]);
                }
                let head_end = find_subslice(&req, b"\r\n\r\n").unwrap_or(req.len());
                let head = String::from_utf8_lossy(&req[..head_end]).to_string();
                let mut parts = head.lines().next().unwrap_or("").split_whitespace();
                let method = parts.next().unwrap_or("").to_string();
                let path = parts.next().unwrap_or("").to_string();
                let body = if head_end + 4 <= req.len() { &req[head_end + 4..] } else { &[][..] };
                let (status, payload) = handler(&base_thread, &method, &path, body);
                let resp = format!(
                    "HTTP/1.1 {status} S\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    payload.len()
                );
                let _ = s.write_all(resp.as_bytes());
                let _ = s.write_all(&payload);
            }
        });
        base
    }

    /// 最小合法 glb 头(首 4 字节 glTF + 够 12 字节,过 meshy_download_glb 校验)。
    const FAKE_GLB: &[u8] = b"glTF\x02\x00\x00\x00\x18\x00\x00\x00fake-payload";

    fn meshy_cfg(endpoint: Option<&str>) -> GenConfig {
        GenConfig {
            backends: vec![BackendEntry {
                id: MESHY_ID.into(),
                kind: "remote".into(),
                enabled: true,
                endpoint: endpoint.map(str::to_string),
                model: None,
            }],
        }
    }

    fn cfg_for(id: &str, endpoint: &str) -> GenConfig {
        GenConfig {
            backends: vec![BackendEntry {
                id: id.into(),
                kind: "remote".into(),
                enabled: true,
                endpoint: Some(endpoint.into()),
                model: Some("test-model".into()),
            }],
        }
    }

    fn empty_ks() -> Keystore {
        Keystore::load_from(std::path::Path::new("no-such-ks.json"))
    }

    #[test]
    fn unconfigured_maps_not_configured() {
        let _g = env_lock();
        std::env::remove_var("FORGE_GEN_API_KEY");
        std::env::remove_var("MESHY_API_KEY");
        let cfg = GenConfig::default();
        let ks = empty_ks();
        for b in media_registry() {
            assert!(!b.configured(&cfg, &ks), "{} 空配置应为未配置", b.id());
        }
        let req = MediaRequest { kind: MediaKind::Video, prompt: "p".into(), params: json!({}) };
        let err = RemoteVideo.generate(&req, &cfg, &ks).unwrap_err();
        assert_eq!(err.code, GEN_BACKEND_NOT_CONFIGURED);
        let err = resolve_backend(MediaKind::Mesh, None, &cfg, &ks)
            .err()
            .expect("空配置应为未配置错误");
        assert_eq!(err.code, GEN_BACKEND_NOT_CONFIGURED);
        assert!(err.message.contains(REMOTE_MESH_ID), "错误应引导可配置条目: {}", err.message);
    }

    #[test]
    fn kinds_routing_matches_capabilities() {
        let ids = |kind| {
            backends_for_kind(kind).iter().map(|b| b.id().to_string()).collect::<Vec<_>>()
        };
        assert_eq!(
            ids(MediaKind::Video),
            vec![COMFYUI_MINIMAX_H3_ID.to_string(), XZAPI_VIDEO_ID.to_string(), ALIYUN_MINIMAX_VIDEO_ID.to_string(), REMOTE_VIDEO_ID.to_string()]
        );
        assert_eq!(ids(MediaKind::Tts), vec![REMOTE_AUDIO_ID.to_string()]);
        assert_eq!(ids(MediaKind::Music), vec![REMOTE_AUDIO_ID.to_string()]);
        // meshy 在前 = mesh 类默认供应商(resolve_backend 取首个已配置者)。
        assert_eq!(
            ids(MediaKind::Mesh),
            vec![MESHY_ID.to_string(), REMOTE_MESH_ID.to_string()]
        );
        assert_eq!(MediaKind::parse("text2video"), Some(MediaKind::Video));
        assert_eq!(MediaKind::parse("image2video"), Some(MediaKind::Video));
        assert_eq!(MediaKind::parse("tts"), Some(MediaKind::Tts));
        assert_eq!(MediaKind::parse("nope"), None);
        // 图生视频与文生视频同后端(参考图只是 params 分支,不另设适配器)。
        assert!(RemoteVideo
            .capabilities()
            .get("kinds")
            .and_then(Value::as_array)
            .is_some_and(|a| a.iter().any(|k| k.as_str() == Some("image2video"))));
    }

    // ---------- meshy(3D 默认供应商) ----------

    #[test]
    fn meshy_configured_needs_enabled_and_key_but_not_endpoint() {
        let _g = env_lock();
        std::env::remove_var("FORGE_GEN_API_KEY");
        std::env::remove_var("MESHY_API_KEY");
        let ks = empty_ks();
        // 条目缺失 → 未配置(D-F5-A)。
        assert!(!MeshyMesh.configured(&GenConfig::default(), &ks));
        // 有条目 + enabled,但无 key → 未配置。
        assert!(!MeshyMesh.configured(&meshy_cfg(None), &ks));
        // env 供 key → 已配置,且 endpoint 缺省官方地址(不逼用户手抄)。
        std::env::set_var("MESHY_API_KEY", "msy_unit_test");
        assert!(MeshyMesh.configured(&meshy_cfg(None), &ks));
        let (ep, key, _) = meshy_conn(&meshy_cfg(None), &ks).unwrap();
        assert_eq!(ep, MESHY_DEFAULT_ENDPOINT);
        assert_eq!(key, "msy_unit_test");
        // 条目 endpoint 覆盖缺省(自建代理),尾斜杠归一。
        let (ep2, _, _) = meshy_conn(&meshy_cfg(Some("https://proxy.example.com/")), &ks).unwrap();
        assert_eq!(ep2, "https://proxy.example.com");
        // 停用 → 未配置(key 仍在,但档位事实如实)。
        let mut off = meshy_cfg(None);
        off.backends[0].enabled = false;
        assert!(!MeshyMesh.configured(&off, &ks));
        std::env::remove_var("MESHY_API_KEY");
    }

    #[test]
    fn meshy_is_default_for_mesh_when_configured() {
        let _g = env_lock();
        std::env::set_var("MESHY_API_KEY", "msy_unit_test");
        // FORGE_GEN_API_KEY 是全后端通配 key,借它把兜底端点也拉成已配置,
        // 这样「两家都可用」才是真场景而非只有 meshy 一家可选。
        std::env::set_var("FORGE_GEN_API_KEY", "sk-test-dummy");
        let mut cfg = meshy_cfg(None);
        cfg.backends.push(BackendEntry {
            id: REMOTE_MESH_ID.into(),
            kind: "remote".into(),
            enabled: true,
            endpoint: Some("https://self-host.example.com".into()),
            model: None,
        });
        let ks = empty_ks();
        assert!(MeshyMesh.configured(&cfg, &ks) && RemoteMesh.configured(&cfg, &ks));
        let b = resolve_backend(MediaKind::Mesh, None, &cfg, &ks).unwrap();
        assert_eq!(b.id(), MESHY_ID, "两家都可用时须默认走 meshy");
        // 显式指名仍能切回兜底端点(默认不等于锁死)。
        let b2 = resolve_backend(MediaKind::Mesh, Some(REMOTE_MESH_ID), &cfg, &ks).unwrap();
        assert_eq!(b2.id(), REMOTE_MESH_ID);
        std::env::remove_var("MESHY_API_KEY");
        std::env::remove_var("FORGE_GEN_API_KEY");
    }

    #[test]
    fn meshy_bad_params_rejected_before_network() {
        let _g = env_lock();
        std::env::remove_var("FORGE_GEN_API_KEY");
        std::env::set_var("MESHY_API_KEY", "msy_unit_test");
        // endpoint 指向不可达地址:参数校验若真前置,就不该走到连接失败。
        let cfg = meshy_cfg(Some("http://127.0.0.1:1"));
        let ks = empty_ks();
        let bad = |params: Value| {
            let req = MediaRequest { kind: MediaKind::Mesh, prompt: "a chair".into(), params };
            MeshyMesh.generate(&req, &cfg, &ks).unwrap_err()
        };
        assert_eq!(bad(json!({ "modelType": "lowpoly" })).code, GEN_BAD_PARAMS);
        assert_eq!(bad(json!({ "aiModel": "gpt-4" })).code, GEN_BAD_PARAMS);
        assert_eq!(bad(json!({ "topology": "ngon" })).code, GEN_BAD_PARAMS);
        assert_eq!(bad(json!({ "textureResolution": "16k" })).code, GEN_BAD_PARAMS);
        assert_eq!(bad(json!({ "targetPolycount": 42 })).code, GEN_BAD_PARAMS);
        assert_eq!(bad(json!({ "targetPolycount": 999_999 })).code, GEN_BAD_PARAMS);
        // smart-topology 面区间更窄,且不收 quad。
        let smart_over =
            bad(json!({ "modelType": "smart-topology", "targetPolycount": 100_000 }));
        assert_eq!(smart_over.code, GEN_BAD_PARAMS);
        let smart_quad = bad(json!({ "modelType": "smart-topology", "topology": "quad" }));
        assert_eq!(smart_quad.code, GEN_BAD_PARAMS);
        // 双空(无 prompt 无图)。
        let req = MediaRequest { kind: MediaKind::Mesh, prompt: "  ".into(), params: json!({}) };
        assert_eq!(MeshyMesh.generate(&req, &cfg, &ks).unwrap_err().code, GEN_BAD_PARAMS);
        // prompt 超 600 字。
        let long = MediaRequest {
            kind: MediaKind::Mesh,
            prompt: "x".repeat(MESHY_PROMPT_MAX + 1),
            params: json!({}),
        };
        assert_eq!(MeshyMesh.generate(&long, &cfg, &ks).unwrap_err().code, GEN_BAD_PARAMS);
        std::env::remove_var("MESHY_API_KEY");
    }

    #[test]
    fn meshy_text_to_3d_two_stage_roundtrip() {
        let _g = env_lock();
        std::env::remove_var("FORGE_GEN_API_KEY");
        std::env::set_var("MESHY_API_KEY", "msy_unit_test");
        let seen: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
        let bodies: Arc<Mutex<Vec<Value>>> = Arc::new(Mutex::new(Vec::new()));
        let seen_h = Arc::clone(&seen);
        let bodies_h = Arc::clone(&bodies);
        let ep = http_stub_server(move |base, method, path, body| {
            seen_h.lock().unwrap().push(format!("{method} {path}"));
            match (method, path) {
                ("POST", "/openapi/v2/text-to-3d") => {
                    let doc: Value = serde_json::from_slice(body).unwrap_or(Value::Null);
                    let stage = doc.get("mode").and_then(Value::as_str).unwrap_or("").to_string();
                    bodies_h.lock().unwrap().push(doc);
                    let id = if stage == "preview" { "task-preview" } else { "task-refine" };
                    (200, format!(r#"{{"result":"{id}"}}"#).into_bytes())
                }
                ("GET", p) if p.starts_with("/openapi/v2/text-to-3d/task-") => {
                    let id = p.rsplit('/').next().unwrap_or("");
                    // preview 先回一次 IN_PROGRESS,证明轮询确实在跑而非一次 GET 撞运气。
                    let first =
                        seen_h.lock().unwrap().iter().filter(|s| s.contains(id)).count() == 1;
                    if id == "task-preview" && first {
                        return (200, br#"{"status":"IN_PROGRESS","progress":40}"#.to_vec());
                    }
                    let doc = json!({
                        "status": "SUCCEEDED",
                        "progress": 100,
                        "id": id,
                        "consumed_credits": 20,
                        "thumbnail_url": "https://x/t.png",
                        "model_urls": { "glb": format!("{base}/dl/{id}.glb") },
                    });
                    (200, doc.to_string().into_bytes())
                }
                ("GET", p) if p.starts_with("/dl/") => (200, FAKE_GLB.to_vec()),
                _ => (404, b"{}".to_vec()),
            }
        });
        let cfg = meshy_cfg(Some(&ep));
        let ks = empty_ks();
        let req = MediaRequest {
            kind: MediaKind::Mesh,
            prompt: "a wooden chair".into(),
            // 轮询间隔固定 4s,给足预算避免慢机误判超时。
            params: json!({ "targetPolycount": 5000, "textureResolution": "4k", "timeoutSec": 60 }),
        };
        let out = match MeshyMesh.generate(&req, &cfg, &ks) {
            Ok(v) => v,
            Err(e) => panic!("meshy 全链应成功,实: {e}"),
        };
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].ext, "glb");
        assert_eq!(out[0].bytes, FAKE_GLB);
        assert_eq!(out[0].meta["provider"], MESHY_ID);
        assert_eq!(out[0].meta["mode"], "text-to-3d");
        assert_eq!(out[0].meta["taskId"], "task-refine");
        assert_eq!(out[0].meta["previewTaskId"], "task-preview");
        assert_eq!(out[0].meta["targetPolycount"], 5000);
        assert_eq!(out[0].meta["consumedCredits"], 20);
        // 两阶段 body:preview 带几何档 + should_remesh;refine 带 preview_task_id + 贴图档。
        let bs = bodies.lock().unwrap();
        assert_eq!(bs.len(), 2, "须建 preview + refine 两个任务");
        assert_eq!(bs[0]["mode"], "preview");
        assert_eq!(bs[0]["prompt"], "a wooden chair");
        assert_eq!(bs[0]["target_polycount"], 5000);
        assert_eq!(bs[0]["should_remesh"], true);
        assert_eq!(bs[0]["target_formats"], json!(["glb"]));
        assert_eq!(bs[1]["mode"], "refine");
        assert_eq!(bs[1]["preview_task_id"], "task-preview");
        assert_eq!(bs[1]["texture_resolution"], "4k");
        assert_eq!(bs[1]["enable_pbr"], true);
        // 轮询确有多轮(preview 的 IN_PROGRESS 那次)。
        let calls = seen.lock().unwrap();
        assert!(
            calls.iter().filter(|s| s.contains("task-preview")).count() >= 2,
            "preview 应至少轮询两次: {calls:?}"
        );
        std::env::remove_var("MESHY_API_KEY");
    }

    #[test]
    fn meshy_preview_only_skips_refine() {
        let _g = env_lock();
        std::env::remove_var("FORGE_GEN_API_KEY");
        std::env::set_var("MESHY_API_KEY", "msy_unit_test");
        let posts = Arc::new(Mutex::new(0usize));
        let posts_h = Arc::clone(&posts);
        let ep = http_stub_server(move |base, method, path, _body| match (method, path) {
            ("POST", "/openapi/v2/text-to-3d") => {
                *posts_h.lock().unwrap() += 1;
                (200, br#"{"result":"task-preview"}"#.to_vec())
            }
            ("GET", p) if p.contains("/text-to-3d/task-preview") => (
                200,
                json!({
                    "status": "SUCCEEDED",
                    "progress": 100,
                    "model_urls": { "glb": format!("{base}/dl/m.glb") },
                })
                .to_string()
                .into_bytes(),
            ),
            ("GET", p) if p.starts_with("/dl/") => (200, FAKE_GLB.to_vec()),
            _ => (404, b"{}".to_vec()),
        });
        let cfg = meshy_cfg(Some(&ep));
        let req = MediaRequest {
            kind: MediaKind::Mesh,
            prompt: "a rock".into(),
            params: json!({ "texture": false, "timeoutSec": 60 }),
        };
        let out = MeshyMesh.generate(&req, &cfg, &empty_ks()).expect("preview-only 应成功");
        assert_eq!(out[0].meta["textured"], false);
        assert!(out[0].meta.get("previewTaskId").is_none(), "无 refine 阶段不应有 previewTaskId");
        assert_eq!(*posts.lock().unwrap(), 1, "texture=false 只建一个任务");
        std::env::remove_var("MESHY_API_KEY");
    }

    #[test]
    fn meshy_task_failure_and_http_errors_map_honestly() {
        let _g = env_lock();
        std::env::remove_var("FORGE_GEN_API_KEY");
        std::env::set_var("MESHY_API_KEY", "msy_secret_key_do_not_leak");
        // 任务级失败:status=FAILED + task_error.message 如实带出。
        let ep = http_stub_server(|_base, method, path, _b| match (method, path) {
            ("POST", "/openapi/v2/text-to-3d") => (200, br#"{"result":"t1"}"#.to_vec()),
            ("GET", p) if p.contains("/t1") => (
                200,
                br#"{"status":"FAILED","task_error":{"message":"prompt rejected"}}"#.to_vec(),
            ),
            _ => (404, b"{}".to_vec()),
        });
        let req = MediaRequest {
            kind: MediaKind::Mesh,
            prompt: "x".into(),
            params: json!({ "timeoutSec": 30 }),
        };
        let e = MeshyMesh.generate(&req, &meshy_cfg(Some(&ep)), &empty_ks()).unwrap_err();
        assert_eq!(e.code, GEN_BACKEND_ERROR);
        assert!(e.message.contains("prompt rejected"), "{}", e.message);
        // HTTP 402/429/400 各自的可诉诸消息 + 错误体不回显密钥。
        let ep402 = http_stub_server(|_base, _m, _p, _b| {
            (402, br#"{"message":"insufficient credits for msy_secret_key_do_not_leak"}"#.to_vec())
        });
        let e = MeshyMesh.generate(&req, &meshy_cfg(Some(&ep402)), &empty_ks()).unwrap_err();
        assert_eq!(e.code, GEN_BACKEND_ERROR);
        assert!(e.message.contains("额度不足"), "{}", e.message);
        let ep429 = http_stub_server(|_base, _m, _p, _b| (429, b"{}".to_vec()));
        let e = MeshyMesh.generate(&req, &meshy_cfg(Some(&ep429)), &empty_ks()).unwrap_err();
        assert_eq!(e.code, GEN_RATE_LIMITED);
        let ep400 = http_stub_server(|_base, _m, _p, _b| {
            (400, br#"{"message":"bad request with key msy_secret_key_do_not_leak inside"}"#.to_vec())
        });
        let e = MeshyMesh.generate(&req, &meshy_cfg(Some(&ep400)), &empty_ks()).unwrap_err();
        assert!(e.message.contains("bad request"), "供应商原因应可见: {}", e.message);
        assert!(
            !e.message.contains("msy_secret_key_do_not_leak"),
            "错误体回显前须擦除密钥: {}",
            e.message
        );
        std::env::remove_var("MESHY_API_KEY");
    }

    #[test]
    fn meshy_image_to_3d_uses_v1_and_carries_prompt_as_texture_hint() {
        let _g = env_lock();
        std::env::remove_var("FORGE_GEN_API_KEY");
        std::env::set_var("MESHY_API_KEY", "msy_unit_test");
        let body_seen: Arc<Mutex<Option<Value>>> = Arc::new(Mutex::new(None));
        let bs = Arc::clone(&body_seen);
        let ep = http_stub_server(move |base, method, path, body| match (method, path) {
            ("POST", "/openapi/v1/image-to-3d") => {
                *bs.lock().unwrap() = serde_json::from_slice(body).ok();
                (200, br#"{"result":"img1"}"#.to_vec())
            }
            ("GET", p) if p.contains("/image-to-3d/img1") => (
                200,
                json!({
                    "status": "SUCCEEDED",
                    "progress": 100,
                    "model_urls": { "glb": format!("{base}/dl/m.glb") },
                })
                .to_string()
                .into_bytes(),
            ),
            ("GET", p) if p.starts_with("/dl/") => (200, FAKE_GLB.to_vec()),
            _ => (404, b"{}".to_vec()),
        });
        let req = MediaRequest {
            kind: MediaKind::Mesh,
            prompt: "weathered bronze".into(),
            params: json!({
                "imageDataUrl": "data:image/png;base64,aGVsbG8=",
                "timeoutSec": 60,
            }),
        };
        let out = MeshyMesh.generate(&req, &meshy_cfg(Some(&ep)), &empty_ks()).expect("图生 3D 应成功");
        assert_eq!(out[0].meta["mode"], "image-to-3d");
        let b = body_seen.lock().unwrap().clone().expect("须收到建任务 body");
        assert_eq!(b["image_url"], "data:image/png;base64,aGVsbG8=");
        assert_eq!(b["should_texture"], true);
        // 图生面无 prompt 参数,用户输入转作贴图引导而非丢弃。
        assert_eq!(b["texture_prompt"], "weathered bronze");
        assert!(b.get("prompt").is_none(), "image-to-3d 不该带 prompt 字段");
        std::env::remove_var("MESHY_API_KEY");
    }

    #[test]
    fn video_bad_params_rejected() {
        let _g = env_lock();
        std::env::set_var("FORGE_GEN_API_KEY", "sk-test-dummy");
        let cfg = cfg_for(REMOTE_VIDEO_ID, "http://127.0.0.1:1");
        let ks = empty_ks();
        let req = MediaRequest {
            kind: MediaKind::Video,
            prompt: "p".into(),
            params: json!({ "aspect": "21:9" }),
        };
        let err = RemoteVideo.generate(&req, &cfg, &ks).unwrap_err();
        assert_eq!(err.code, GEN_BAD_PARAMS);
        let req = MediaRequest {
            kind: MediaKind::Video,
            prompt: "p".into(),
            params: json!({ "durationSec": 99 }),
        };
        let err = RemoteVideo.generate(&req, &cfg, &ks).unwrap_err();
        assert_eq!(err.code, GEN_BAD_PARAMS);
        std::env::remove_var("FORGE_GEN_API_KEY");
    }

    #[test]
    fn video_b64_success_roundtrip() {
        let _g = env_lock();
        std::env::set_var("FORGE_GEN_API_KEY", "sk-test-dummy");
        let fake_mp4 = b"\x00\x00\x00\x18ftypmp42fake-bytes";
        let body = format!(
            r#"{{"data":[{{"b64_json":"{}"}}]}}"#,
            base64::engine::general_purpose::STANDARD.encode(fake_mp4)
        );
        let body: &'static [u8] = Box::leak(body.into_bytes().into_boxed_slice());
        let ep = http_stub_once("200 OK", body);
        let cfg = cfg_for(REMOTE_VIDEO_ID, &ep);
        let ks = empty_ks();
        let req = MediaRequest { kind: MediaKind::Video, prompt: "p".into(), params: json!({}) };
        let out = RemoteVideo.generate(&req, &cfg, &ks).unwrap();
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].bytes, fake_mp4);
        assert_eq!(out[0].ext, "mp4");
        assert_eq!(out[0].meta["mode"], "text2video");
        std::env::remove_var("FORGE_GEN_API_KEY");
    }

    #[test]
    fn video_image2video_sends_image_field() {
        let _g = env_lock();
        std::env::set_var("FORGE_GEN_API_KEY", "sk-test-dummy");
        let fake_mp4 = b"\x00\x00\x00\x18ftypmp42ref-driven";
        let b64 = base64::engine::general_purpose::STANDARD.encode(fake_mp4);
        let bodies: Arc<Mutex<Vec<Value>>> = Arc::new(Mutex::new(Vec::new()));
        let bodies_h = Arc::clone(&bodies);
        let ep = http_stub_server(move |_base, method, path, body| {
            assert_eq!((method, path), ("POST", "/v1/videos/generations"));
            bodies_h.lock().unwrap().push(serde_json::from_slice(body).unwrap_or(Value::Null));
            (200, format!(r#"{{"data":[{{"b64_json":"{b64}"}}]}}"#).into_bytes())
        });
        let cfg = cfg_for(REMOTE_VIDEO_ID, &ep);
        let ks = empty_ks();
        let req = MediaRequest {
            kind: MediaKind::Video,
            prompt: "walk cycle to the right".into(),
            params: json!({ "imageDataUrl": "data:image/png;base64,aGVsbG8=", "aspect": "1:1" }),
        };
        let out = RemoteVideo.generate(&req, &cfg, &ks).unwrap();
        assert_eq!(out[0].bytes, fake_mp4);
        assert_eq!(out[0].meta["mode"], "image2video");
        let sent = bodies.lock().unwrap();
        let b = sent.first().expect("须发出一次建任务请求");
        assert_eq!(b["image"], "data:image/png;base64,aGVsbG8=");
        assert_eq!(b["prompt"], "walk cycle to the right");
        assert_eq!(b["aspect"], "1:1");
        std::env::remove_var("FORGE_GEN_API_KEY");
    }

    #[test]
    fn video_needs_prompt_or_image() {
        let _g = env_lock();
        std::env::set_var("FORGE_GEN_API_KEY", "sk-test-dummy");
        // endpoint 不可达:双空若真前置拦下,就不该走到连接失败。
        let cfg = cfg_for(REMOTE_VIDEO_ID, "http://127.0.0.1:1");
        let ks = empty_ks();
        let req = MediaRequest { kind: MediaKind::Video, prompt: "  ".into(), params: json!({}) };
        assert_eq!(RemoteVideo.generate(&req, &cfg, &ks).unwrap_err().code, GEN_BAD_PARAMS);
        std::env::remove_var("FORGE_GEN_API_KEY");
    }

    #[test]
    fn tts_raw_bytes_response() {
        let _g = env_lock();
        std::env::set_var("FORGE_GEN_API_KEY", "sk-test-dummy");
        let ep = http_stub_once("200 OK", b"ID3fake-mp3-bytes");
        let cfg = cfg_for(REMOTE_AUDIO_ID, &ep);
        let ks = empty_ks();
        let req = MediaRequest {
            kind: MediaKind::Tts,
            prompt: "你好".into(),
            params: json!({ "voice": "alloy" }),
        };
        let out = RemoteAudio.generate(&req, &cfg, &ks).unwrap();
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].bytes, b"ID3fake-mp3-bytes");
        assert_eq!(out[0].ext, "mp3");
        std::env::remove_var("FORGE_GEN_API_KEY");
    }

    #[test]
    fn http_429_maps_rate_limited_and_no_key_leak() {
        let _g = env_lock();
        std::env::set_var("FORGE_GEN_API_KEY", "sk-test-dummy");
        let ep = http_stub_once("429 Too Many Requests", b"{}");
        let cfg = cfg_for(REMOTE_MESH_ID, &ep);
        let ks = empty_ks();
        let req = MediaRequest { kind: MediaKind::Mesh, prompt: "p".into(), params: json!({}) };
        let err = RemoteMesh.generate(&req, &cfg, &ks).unwrap_err();
        assert_eq!(err.code, GEN_RATE_LIMITED);
        assert!(!err.message.contains("sk-test-dummy"), "错误信息不得含密钥");
        std::env::remove_var("FORGE_GEN_API_KEY");
    }
}
