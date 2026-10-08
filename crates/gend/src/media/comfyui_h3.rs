//! Local MiniMax H3 Base, using ComfyUI's native API and a versioned 8-step graph.
//! Configuration is distinct from readiness. Every generation checks node schemas and
//! installed model names, submits exactly once, and fetches only its SaveVideo output.

use super::{MediaArtifact, MediaBackend, MediaKind, MediaRequest};
use crate::config::{data_dir, GenConfig};
use crate::keystore::Keystore;
use crate::{GenError, Result, GEN_BACKEND_ERROR, GEN_BACKEND_NOT_CONFIGURED, GEN_BAD_PARAMS};
use base64::Engine;
use serde_json::{json, Value};
use std::collections::BTreeSet;
use std::io::{Read, Write};
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

pub const COMFYUI_MINIMAX_H3_ID: &str = "comfyui-minimax-h3";
const MODEL: &str = "MiniMax-H3";
const WORKFLOW_VERSION: &str = "comfyui-h3-base-int8-v1";
const WORKFLOW: &str = include_str!("comfyui_h3_v1.json");
const JSON_LIMIT: usize = 2 * 1024 * 1024;
const IMAGE_LIMIT: usize = 20 * 1024 * 1024;
const VIDEO_LIMIT: usize = 512 * 1024 * 1024;
const FPS: u64 = 24;
static SEQUENCE: AtomicU64 = AtomicU64::new(0);

pub struct ComfyuiMiniMaxH3;

impl MediaBackend for ComfyuiMiniMaxH3 {
    fn id(&self) -> &str {
        COMFYUI_MINIMAX_H3_ID
    }
    fn kind(&self) -> &str {
        "local"
    }
    fn configured(&self, cfg: &GenConfig, _keys: &Keystore) -> bool {
        connection(cfg).is_ok()
    }
    fn capabilities(&self) -> Value {
        json!({
            "kinds": ["text2video", "image2video"], "aspects": ["16:9", "9:16", "1:1"],
            "resolutions": ["352p", "480p", "768p"], "defaultResolution": "352p",
            "minDurationSec": 2, "maxDurationSec": 15, "defaultDurationSec": 2,
            "maxBatch": 1, "formats": ["mp4"], "maxPromptChars": 7000,
            "imageInputTypes": ["dataUrl"], "defaultEndpoint": "http://127.0.0.1:8188",
            "defaultModel": MODEL, "requiresKey": false,
            "workflowVersion": WORKFLOW_VERSION, "fps": FPS, "audio": true,
        })
    }
    fn generate(
        &self,
        req: &MediaRequest,
        cfg: &GenConfig,
        _keys: &Keystore,
    ) -> Result<Vec<MediaArtifact>> {
        generate(
            req,
            cfg,
            Duration::from_secs(2),
            Duration::from_secs(1800),
            &data_dir().join("comfyui-h3-tasks"),
        )
    }
}

impl ComfyuiMiniMaxH3 {
    /// Read-only probe; success means the server advertises the required native nodes
    /// and exact five model filenames. It does not claim the GPU has loaded them.
    pub fn readiness(&self, cfg: &GenConfig) -> Result<Value> {
        let endpoint = connection(cfg)?;
        let graph = template()?;
        check_ready(&endpoint, &graph, true)?;
        Ok(json!({"ready": true, "endpoint": endpoint, "model": MODEL,
            "workflowVersion": WORKFLOW_VERSION, "check": "node-schemas-and-model-filenames",
            "checkedAt": crate::timeutil::utc_now_iso8601()}))
    }
}

fn bad(message: impl Into<String>) -> GenError {
    GenError::new(GEN_BAD_PARAMS, message)
}
fn failure(message: impl Into<String>) -> GenError {
    GenError::new(GEN_BACKEND_ERROR, message)
}

/// Keep this provider on the user's machine. Credentials, proxies, redirects,
/// arbitrary paths, remote IPs and URL query strings have no role in this API.
fn loopback_endpoint(endpoint: &str) -> bool {
    let Some(authority) = endpoint
        .strip_prefix("http://")
        .or_else(|| endpoint.strip_prefix("https://"))
    else {
        return false;
    };
    if authority.is_empty()
        || authority.contains(['/', '?', '#', '@'])
        || authority.chars().any(char::is_whitespace)
    {
        return false;
    }
    let (host, port) = if let Some(rest) = authority.strip_prefix('[') {
        let Some((host, suffix)) = rest.split_once(']') else {
            return false;
        };
        let port = if suffix.is_empty() {
            None
        } else {
            suffix.strip_prefix(':')
        };
        if !suffix.is_empty() && port.is_none() {
            return false;
        }
        (host, port)
    } else {
        match authority.split_once(':') {
            Some((host, port)) => (host, Some(port)),
            None => (authority, None),
        }
    };
    if port.is_some_and(|p| p.parse::<u16>().map_or(true, |n| n == 0)) {
        return false;
    }
    host.eq_ignore_ascii_case("localhost")
        || host
            .parse::<std::net::IpAddr>()
            .is_ok_and(|ip| ip.is_loopback())
}

fn connection(cfg: &GenConfig) -> Result<String> {
    let entry = cfg
        .entry(COMFYUI_MINIMAX_H3_ID)
        .filter(|e| e.enabled && e.kind == "local")
        .ok_or_else(|| {
            GenError::new(
                GEN_BACKEND_NOT_CONFIGURED,
                "本地 H3 须显式启用 kind=local 的后端条目，无需 API key",
            )
        })?;
    let endpoint = entry
        .endpoint
        .as_deref()
        .unwrap_or("")
        .trim()
        .trim_end_matches('/');
    if !loopback_endpoint(endpoint) {
        return Err(GenError::new(
            GEN_BACKEND_NOT_CONFIGURED,
            "本地 H3 endpoint 须为本机回环 HTTP(S) 地址，例如 http://127.0.0.1:8188",
        ));
    }
    if entry
        .model
        .as_deref()
        .is_some_and(|m| !m.trim().is_empty() && m != MODEL)
    {
        return Err(bad(format!("本地 H3 model 须为 {MODEL}")));
    }
    Ok(endpoint.to_owned())
}

fn template() -> Result<Value> {
    serde_json::from_str(WORKFLOW).map_err(|_| failure("内置 H3 工作流 JSON 无效"))
}

fn client_id() -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    format!(
        "forge-h3-{nanos:x}-{:x}-{:x}",
        std::process::id(),
        SEQUENCE.fetch_add(1, Ordering::Relaxed)
    )
}

struct Input {
    graph: Value,
    image: Option<(Vec<u8>, &'static str, &'static str)>,
    aspect: String,
    resolution: String,
    requested_duration: u64,
    frames: u64,
    width: u64,
    height: u64,
    seed: u64,
}

fn input(req: &MediaRequest, id: &str) -> Result<Input> {
    if req.kind != MediaKind::Video {
        return Err(bad("本地 H3 仅支持 text2video / image2video"));
    }
    if !req.params.is_object() && !req.params.is_null() {
        return Err(bad("本地 H3 params 须为对象"));
    }
    if req.prompt.trim().chars().count() > 7000 {
        return Err(bad("本地 H3 prompt 最多 7000 字符"));
    }
    let image = match req
        .params
        .get("imageDataUrl")
        .or_else(|| req.params.get("imageUrl"))
    {
        None | Some(Value::Null) => None,
        Some(Value::String(s)) if s.trim().is_empty() => None,
        Some(Value::String(s)) => Some(decode_image(s.trim())?),
        _ => return Err(bad("本地 H3 参考图须为 PNG/JPEG base64 data URI")),
    };
    if req.prompt.trim().is_empty() && image.is_none() {
        return Err(bad("本地 H3 prompt 与参考图至少其一非空"));
    }
    let aspect = match req.params.get("aspect") {
        None | Some(Value::Null) => "16:9",
        Some(Value::String(s)) if ["16:9", "9:16", "1:1"].contains(&s.as_str()) => s,
        _ => return Err(bad("本地 H3 aspect 须为 16:9 / 9:16 / 1:1")),
    };
    let resolution = match req.params.get("resolution") {
        None | Some(Value::Null) => "352p",
        Some(Value::String(s)) if ["352p", "480p", "768p"].contains(&s.as_str()) => s,
        _ => return Err(bad("本地 H3 resolution 须为 352p / 480p / 768p")),
    };
    let duration = integer(&req.params, "durationSec", 2, 2, 15)?;
    integer(&req.params, "n", 1, 1, 1)?;
    let default_seed = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos() as u64
        ^ SEQUENCE.fetch_add(1, Ordering::Relaxed);
    let seed = integer(&req.params, "seed", default_seed, 0, u64::MAX)?;
    let (long, short) = match resolution {
        "352p" => (608, 352),
        "480p" => (864, 480),
        _ => (1344, 768),
    };
    let (width, height) = match aspect {
        "9:16" => (short, long),
        "1:1" => (short, short),
        _ => (long, short),
    };
    // Native temporal_shape snaps UP to 17k+5. Two requested seconds are 56/24s.
    let frames = (duration * FPS - 5).div_ceil(17) * 17 + 5;
    let mut graph = template()?;
    graph["6"]["inputs"]["prompt"] = json!(req.prompt.trim());
    graph["6"]["inputs"]["width"] = json!(width);
    graph["6"]["inputs"]["height"] = json!(height);
    graph["6"]["inputs"]["length"] = json!(frames);
    graph["10"]["inputs"]["noise_seed"] = json!(seed);
    graph["15"]["inputs"]["filename_prefix"] = json!(format!("video/RurixForge_H3_{id}"));
    Ok(Input {
        graph,
        image,
        aspect: aspect.into(),
        resolution: resolution.into(),
        requested_duration: duration,
        frames,
        width,
        height,
        seed,
    })
}

fn integer(params: &Value, name: &str, default: u64, min: u64, max: u64) -> Result<u64> {
    match params.get(name) {
        None | Some(Value::Null) => Ok(default),
        Some(v) => v
            .as_u64()
            .filter(|v| (min..=max).contains(v))
            .ok_or_else(|| bad(format!("本地 H3 {name} 须为 {min}..={max} 的整数"))),
    }
}

fn decode_image(data: &str) -> Result<(Vec<u8>, &'static str, &'static str)> {
    let (prefix, encoded) = data
        .split_once(',')
        .ok_or_else(|| bad("本地 H3 只接受 PNG/JPEG base64 data URI 参考图"))?;
    let (mime, ext, expected) = match prefix {
        "data:image/png;base64" => ("image/png", "png", image::ImageFormat::Png),
        "data:image/jpeg;base64" | "data:image/jpg;base64" => {
            ("image/jpeg", "jpg", image::ImageFormat::Jpeg)
        }
        _ => return Err(bad("本地 H3 参考图须为 PNG/JPEG base64 data URI")),
    };
    if encoded.len() > IMAGE_LIMIT.div_ceil(3) * 4 {
        return Err(bad("本地 H3 参考图超过 20 MiB"));
    }
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(encoded)
        .map_err(|_| bad("参考图 base64 无效"))?;
    if bytes.is_empty()
        || bytes.len() > IMAGE_LIMIT
        || image::guess_format(&bytes).ok() != Some(expected)
    {
        return Err(bad("本地 H3 参考图格式或大小无效"));
    }
    let reader = image::ImageReader::with_format(std::io::Cursor::new(&bytes), expected);
    let (width, height) = reader
        .into_dimensions()
        .map_err(|_| bad("本地 H3 参考图文件损坏"))?;
    if width == 0 || height == 0 || width > 16384 || height > 16384 {
        return Err(bad("本地 H3 参考图每边须为 1..=16384 像素"));
    }
    Ok((bytes, mime, ext))
}

fn agent() -> ureq::Agent {
    ureq::AgentBuilder::new()
        .timeout(Duration::from_secs(60))
        .timeout_connect(Duration::from_secs(3))
        .try_proxy_from_env(false)
        .redirects(0)
        .build()
}

fn read_limited(response: ureq::Response, limit: usize) -> Result<Vec<u8>> {
    let mut bytes = Vec::new();
    response
        .into_reader()
        .take(limit as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| failure("本地 ComfyUI 响应读取失败"))?;
    if bytes.len() > limit {
        return Err(failure("本地 ComfyUI 响应超过大小限制"));
    }
    Ok(bytes)
}

fn response(result: std::result::Result<ureq::Response, ureq::Error>) -> Result<ureq::Response> {
    match result {
        Ok(r) if (200..300).contains(&r.status()) => Ok(r),
        Ok(r) => Err(failure(format!(
            "本地 ComfyUI HTTP {}；不跟随重定向",
            r.status()
        ))),
        Err(ureq::Error::Status(status, r)) => {
            let doc = read_limited(r, JSON_LIMIT)
                .ok()
                .and_then(|b| serde_json::from_slice::<Value>(&b).ok());
            // Do not echo the node_errors payload: it may contain the prompt/reference.
            let detail = doc
                .as_ref()
                .and_then(|v| v.pointer("/error/message").or_else(|| v.get("message")))
                .and_then(Value::as_str)
                .unwrap_or("请求被拒绝");
            Err(failure(format!(
                "本地 ComfyUI HTTP {status}: {}",
                detail.chars().take(400).collect::<String>()
            )))
        }
        Err(ureq::Error::Transport(t)) => Err(failure(format!(
            "本地 ComfyUI 连接或读取失败 ({:?})；请检查本地服务，不会自动重新提交",
            t.kind()
        ))),
    }
}

fn api_json(method: &str, endpoint: &str, path: &str, body: Option<&Value>) -> Result<Value> {
    let request = agent().request(method, &format!("{endpoint}{path}"));
    let result = match body {
        Some(body) => request
            .set("Content-Type", "application/json")
            .send_string(&body.to_string()),
        None => request.call(),
    };
    let bytes = read_limited(response(result)?, JSON_LIMIT)?;
    serde_json::from_slice(&bytes).map_err(|_| failure("本地 ComfyUI 返回无效 JSON"))
}

fn check_ready(endpoint: &str, graph: &Value, image: bool) -> Result<()> {
    let nodes = graph
        .as_object()
        .ok_or_else(|| failure("内置 H3 工作流格式无效"))?;
    let mut classes: BTreeSet<&str> = nodes
        .values()
        .filter_map(|n| n["class_type"].as_str())
        .collect();
    if image {
        classes.insert("LoadImage");
    }
    for class in classes {
        let info = api_json("GET", endpoint, &format!("/object_info/{class}"), None)?;
        let info = info
            .get(class)
            .filter(|v| v.is_object())
            .ok_or_else(|| failure(format!("本地 ComfyUI 缺少节点 {class}，请更新本地 H3 安装")))?;
        if class == "MiniMaxH3ImageToVideo"
            && image
            && info.pointer("/input/optional/first_frame").is_none()
        {
            return Err(failure("本地 ComfyUI H3 节点不支持 first_frame 参考图"));
        }
        for node in nodes.values().filter(|n| n["class_type"] == class) {
            for field in ["unet_name", "clip_name", "vae_name", "lora_name"] {
                if let Some(model) = node["inputs"][field].as_str() {
                    let available = info
                        .pointer(&format!("/input/required/{field}/0"))
                        .and_then(Value::as_array);
                    if !available.is_some_and(|a| a.iter().any(|v| v.as_str() == Some(model))) {
                        return Err(failure(format!("本地 ComfyUI 未发现 H3 权重 {model}")));
                    }
                }
            }
        }
    }
    Ok(())
}

fn upload_image(endpoint: &str, image: &(Vec<u8>, &str, &str), id: &str) -> Result<String> {
    let boundary = format!("{id}-boundary");
    let mut body = format!("--{boundary}\r\nContent-Disposition: form-data; name=\"type\"\r\n\r\ninput\r\n\
        --{boundary}\r\nContent-Disposition: form-data; name=\"subfolder\"\r\n\r\nRurixForge\r\n\
        --{boundary}\r\nContent-Disposition: form-data; name=\"overwrite\"\r\n\r\nfalse\r\n\
        --{boundary}\r\nContent-Disposition: form-data; name=\"image\"; filename=\"{id}.{}\"\r\nContent-Type: {}\r\n\r\n", image.2, image.1).into_bytes();
    body.extend_from_slice(&image.0);
    body.extend_from_slice(format!("\r\n--{boundary}--\r\n").as_bytes());
    let bytes = read_limited(
        response(
            agent()
                .post(&format!("{endpoint}/upload/image"))
                .set(
                    "Content-Type",
                    &format!("multipart/form-data; boundary={boundary}"),
                )
                .send_bytes(&body),
        )?,
        JSON_LIMIT,
    )?;
    let doc: Value =
        serde_json::from_slice(&bytes).map_err(|_| failure("ComfyUI 图片上传返回无效 JSON"))?;
    let name = doc
        .get("name")
        .and_then(Value::as_str)
        .filter(|s| safe_name(s))
        .ok_or_else(|| failure("ComfyUI 图片上传缺少有效 name"))?;
    let folder = doc.get("subfolder").and_then(Value::as_str).unwrap_or("");
    if !safe_folder(folder) || doc.get("type").and_then(Value::as_str) != Some("input") {
        return Err(failure("ComfyUI 图片上传未返回 input 目录引用"));
    }
    Ok(if folder.is_empty() {
        name.into()
    } else {
        format!("{folder}/{name}")
    })
}

fn safe_name(name: &str) -> bool {
    !name.is_empty()
        && name != "."
        && name != ".."
        && !name.contains(['/', '\\', ':'])
        && !name.chars().any(char::is_control)
}
fn safe_folder(folder: &str) -> bool {
    folder.is_empty() || folder.split('/').all(safe_name)
}
fn prompt_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

fn download(endpoint: &str, task: &Value) -> Result<(Vec<u8>, Value)> {
    // Never search history for a recent or unrelated file. The sole output node in
    // our graph is 15, and this task is already selected by the exact prompt_id.
    let outputs = task
        .pointer("/outputs/15/images")
        .and_then(Value::as_array)
        .ok_or_else(|| failure("本地 H3 已完成但 SaveVideo 节点 15 没有 MP4 产物"))?;
    let files: Vec<_> = outputs
        .iter()
        .filter(|f| {
            f["filename"].as_str().is_some_and(|s| s.ends_with(".mp4")) && f["type"] == "output"
        })
        .collect();
    if files.len() != 1 {
        return Err(failure("本地 H3 SaveVideo 须返回恰好一个 output MP4"));
    }
    let file = files[0];
    let name = file["filename"].as_str().unwrap_or("");
    let folder = file["subfolder"].as_str().unwrap_or("");
    if !safe_name(name) || !safe_folder(folder) {
        return Err(failure("ComfyUI 产物路径无效"));
    }
    let bytes = read_limited(
        response(
            agent()
                .get(&format!("{endpoint}/view"))
                .query("filename", name)
                .query("subfolder", folder)
                .query("type", "output")
                .call(),
        )?,
        VIDEO_LIMIT,
    )?;
    if bytes.len() < 12 || &bytes[4..8] != b"ftyp" {
        return Err(failure("本地 H3 返回的产物不是 MP4"));
    }
    Ok((bytes, file.clone()))
}

fn save_receipt(path: &Path, receipt: &Value) -> Result<()> {
    std::fs::write(
        path,
        serde_json::to_vec_pretty(receipt).map_err(|_| failure("H3 回执序列化失败"))?,
    )?;
    Ok(())
}

fn generate(
    req: &MediaRequest,
    cfg: &GenConfig,
    interval: Duration,
    budget: Duration,
    receipts: &Path,
) -> Result<Vec<MediaArtifact>> {
    let id = client_id();
    let mut input = input(req, &id)?;
    let endpoint = connection(cfg)?;
    check_ready(&endpoint, &input.graph, input.image.is_some())?;
    if let Some(image) = &input.image {
        let path = upload_image(&endpoint, image, &id)?;
        input.graph["16"] = json!({"class_type": "LoadImage", "inputs": {"image": path}});
        input.graph["6"]["inputs"]["first_frame"] = json!(["16", 0]);
    }
    let mode = if input.image.is_some() {
        "image2video"
    } else {
        "text2video"
    };
    std::fs::create_dir_all(receipts)?;
    let receipt_path = receipts.join(format!("{id}.json"));
    let mut receipt = json!({"provider": COMFYUI_MINIMAX_H3_ID, "clientTaskId": id,
        "model": MODEL, "mode": mode, "workflowVersion": WORKFLOW_VERSION, "status": "submitting",
        "createdAt": crate::timeutil::utc_now_iso8601()});
    std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&receipt_path)?
        .write_all(receipt.to_string().as_bytes())?;
    let mut job_id = String::new();
    let result = (|| {
        // Exactly one POST. Connection failure may mean accepted; preserve a receipt
        // and client ID so the user can inspect ComfyUI instead of resubmitting.
        let created = api_json(
            "POST",
            &endpoint,
            "/prompt",
            Some(&json!({"prompt": input.graph, "client_id": id})),
        )?;
        job_id = created["prompt_id"]
            .as_str()
            .filter(|s| prompt_id(s))
            .ok_or_else(|| failure("ComfyUI 提交响应缺少有效 prompt_id；请检查队列，不要重复提交"))?
            .to_owned();
        receipt["promptId"] = json!(job_id);
        receipt["taskId"] = json!(job_id);
        receipt["status"] = json!("submitted");
        save_receipt(&receipt_path, &receipt)?;
        let started = Instant::now();
        let task = loop {
            if started.elapsed() >= budget {
                return Err(failure(
                    "本地 H3 等待超时，任务可能仍在运行；请按 prompt_id 查看 ComfyUI",
                ));
            }
            let history = api_json("GET", &endpoint, &format!("/history/{job_id}"), None)?;
            if let Some(task) = history.get(&job_id) {
                let status = task
                    .pointer("/status/status_str")
                    .and_then(Value::as_str)
                    .unwrap_or("");
                let messages = task.pointer("/status/messages").and_then(Value::as_array);
                let execution_error = messages.into_iter().flatten().find(|m| {
                    matches!(
                        m[0].as_str(),
                        Some("execution_error" | "execution_interrupted")
                    )
                });
                if status == "error" || execution_error.is_some() {
                    let detail = execution_error
                        .and_then(|e| e[1]["exception_message"].as_str())
                        .unwrap_or("执行失败或已中断");
                    return Err(failure(format!(
                        "本地 H3 生成失败: {}",
                        detail.chars().take(500).collect::<String>()
                    )));
                }
                if task.pointer("/status/completed").and_then(Value::as_bool) == Some(true) {
                    if status != "success" {
                        return Err(failure("本地 H3 任务已结束但未成功"));
                    }
                    break task.clone();
                }
            }
            std::thread::sleep(interval.min(budget.saturating_sub(started.elapsed())));
        };
        receipt["status"] = json!("downloading");
        save_receipt(&receipt_path, &receipt)?;
        let (bytes, output) = download(&endpoint, &task)?;
        receipt["status"] = json!("completed");
        receipt["output"] = output.clone();
        receipt["bytes"] = json!(bytes.len());
        save_receipt(&receipt_path, &receipt)?;
        Ok(vec![MediaArtifact::plain(
            bytes,
            "mp4",
            json!({
                "provider": COMFYUI_MINIMAX_H3_ID, "model": MODEL, "mode": mode,
                "taskId": job_id, "promptId": job_id, "clientTaskId": id,
                "workflowVersion": WORKFLOW_VERSION, "aspect": input.aspect, "resolution": input.resolution,
                "requestedDurationSec": input.requested_duration, "durationSec": input.frames as f64 / FPS as f64,
                "effectiveDurationSec": input.frames as f64 / FPS as f64,
                "width": input.width, "height": input.height, "frames": input.frames, "frameCount": input.frames, "numFrames": input.frames,
                "fps": FPS, "seed": input.seed, "steps": 8,
                "audio": {"generated": true, "channels": 2}, "metadataSource": "submitted-native-workflow",
                "imageUploaded": input.image.is_some(), "output": output, "receipt": receipt_path.display().to_string(),
            }),
        )])
    })();
    result.map_err(|error: GenError| {
        receipt["status"] = json!("error");
        receipt["error"] = json!(error.message);
        let _ = save_receipt(&receipt_path, &receipt);
        GenError::new(
            error.code,
            format!(
                "{}; prompt_id: {}; client_task_id: {id}; receipt: {}; 不会自动重新提交或切换云端",
                error.message,
                if job_id.is_empty() {
                    "unknown (submission may have been accepted)"
                } else {
                    &job_id
                },
                receipt_path.display()
            ),
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::BackendEntry;
    use std::net::TcpListener;
    use std::sync::atomic::AtomicBool;
    use std::sync::{Arc, Mutex};

    fn cfg(endpoint: Option<&str>) -> GenConfig {
        GenConfig {
            backends: vec![BackendEntry {
                id: COMFYUI_MINIMAX_H3_ID.into(),
                kind: "local".into(),
                enabled: true,
                endpoint: endpoint.map(str::to_owned),
                model: Some(MODEL.into()),
            }],
        }
    }
    fn req(params: Value) -> MediaRequest {
        MediaRequest {
            kind: MediaKind::Video,
            prompt: "A plant moves gently in the breeze.".into(),
            params,
        }
    }
    fn keys() -> Keystore {
        Keystore::load_from(&std::env::temp_dir().join(client_id()))
    }
    fn png() -> Vec<u8> {
        let mut bytes = std::io::Cursor::new(Vec::new());
        image::DynamicImage::new_rgb8(2, 2)
            .write_to(&mut bytes, image::ImageFormat::Png)
            .unwrap();
        bytes.into_inner()
    }
    fn data_image() -> String {
        format!(
            "data:image/png;base64,{}",
            base64::engine::general_purpose::STANDARD.encode(png())
        )
    }

    #[test]
    fn configured_requires_explicit_local_entry_and_loopback_but_no_key() {
        assert!(!ComfyuiMiniMaxH3.configured(&GenConfig::default(), &keys()));
        assert!(!ComfyuiMiniMaxH3.configured(&cfg(None), &keys()));
        for endpoint in [
            "http://127.0.0.1:8188",
            "http://localhost:8188/",
            "http://[::1]:8188",
            "http://127.2.3.4",
        ] {
            assert!(
                ComfyuiMiniMaxH3.configured(&cfg(Some(endpoint)), &keys()),
                "{endpoint}"
            );
        }
        for endpoint in [
            "https://example.com",
            "http://192.168.1.2:8188",
            "http://127.0.0.1.evil.test",
            "http://127.0.0.1:0",
            "http://127.0.0.1/api",
            "http://user@127.0.0.1:8188",
            "http://127.0.0.1:8188?x=1",
            "http://[::1]evil",
        ] {
            assert!(
                !ComfyuiMiniMaxH3.configured(&cfg(Some(endpoint)), &keys()),
                "{endpoint}"
            );
        }
        let mut disabled = cfg(Some("http://127.0.0.1:8188"));
        disabled.backends[0].enabled = false;
        assert!(!ComfyuiMiniMaxH3.configured(&disabled, &keys()));
        disabled.backends[0].enabled = true;
        disabled.backends[0].kind = "remote".into();
        assert!(!ComfyuiMiniMaxH3.configured(&disabled, &keys()));
        let configured = cfg(Some("http://127.0.0.1:8188"));
        let resolved =
            super::super::resolve_backend(MediaKind::Video, None, &configured, &keys()).unwrap();
        assert_eq!(resolved.id(), COMFYUI_MINIMAX_H3_ID);
        assert_eq!(resolved.kind(), "local");
        assert_eq!(resolved.capabilities()["defaultResolution"], "352p");
        assert_eq!(
            resolved.capabilities()["imageInputTypes"],
            json!(["dataUrl"])
        );
    }

    #[test]
    fn input_validation_precedes_network_and_alignment_matches_native_h3() {
        for params in [
            json!({"durationSec": 1}),
            json!({"durationSec": 16}),
            json!({"durationSec": 2.5}),
            json!({"durationSec": "2"}),
            json!({"resolution": "2k"}),
            json!({"aspect": "adaptive"}),
            json!({"n": 2}),
            json!({"seed": -1}),
            json!({"imageUrl": "https://example.com/image.png"}),
            json!({"imageDataUrl": "data:image/png;base64,invalid"}),
            json!({"imageDataUrl": "data:image/png;base64,aGVsbG8="}),
        ] {
            let err = ComfyuiMiniMaxH3
                .generate(&req(params), &cfg(Some("http://127.0.0.1:1")), &keys())
                .unwrap_err();
            assert_eq!(err.code, GEN_BAD_PARAMS, "{err}");
        }
        let mut blank = req(json!({}));
        blank.prompt.clear();
        assert!(input(&blank, "test").is_err());
        blank.params = json!({"imageDataUrl": data_image()});
        assert!(input(&blank, "test").is_ok());
        for (resolution, dimensions) in [
            ("352p", (608, 352)),
            ("480p", (864, 480)),
            ("768p", (1344, 768)),
        ] {
            let parsed = input(
                &req(json!({"resolution": resolution, "aspect": "9:16", "seed": 42})),
                "test",
            )
            .unwrap();
            assert_eq!((parsed.width, parsed.height), (dimensions.1, dimensions.0));
            assert_eq!(parsed.frames, 56);
            assert_eq!(parsed.graph["6"]["inputs"]["length"], 56);
            assert_eq!(parsed.graph["10"]["inputs"]["noise_seed"], 42);
        }
        assert_eq!(
            input(&req(json!({"durationSec":15})), "test")
                .unwrap()
                .frames,
            362
        );
    }

    #[derive(Clone)]
    struct Call {
        path: String,
        headers: String,
        body: Vec<u8>,
    }
    struct Server {
        endpoint: String,
        calls: Arc<Mutex<Vec<Call>>>,
        stop: Arc<AtomicBool>,
        thread: Option<std::thread::JoinHandle<()>>,
    }
    impl Drop for Server {
        fn drop(&mut self) {
            self.stop.store(true, Ordering::Relaxed);
            if let Some(thread) = self.thread.take() {
                thread.join().unwrap();
            }
        }
    }
    fn object_info(class: &str) -> Value {
        let mut node = json!({"input": {"required": {}, "optional": {}}});
        for graph_node in template()
            .unwrap()
            .as_object()
            .unwrap()
            .values()
            .filter(|n| n["class_type"] == class)
        {
            for key in ["unet_name", "clip_name", "vae_name", "lora_name"] {
                if let Some(value) = graph_node["inputs"][key].as_str() {
                    if node["input"]["required"][key].is_null() {
                        node["input"]["required"][key] = json!([[]]);
                    }
                    node["input"]["required"][key][0]
                        .as_array_mut()
                        .unwrap()
                        .push(json!(value));
                }
            }
        }
        if class == "MiniMaxH3ImageToVideo" {
            node["input"]["optional"]["first_frame"] = json!(["IMAGE", {}]);
        }
        json!({class: node})
    }
    fn server(scenario: &'static str) -> Server {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let endpoint = format!("http://{}", listener.local_addr().unwrap());
        let calls = Arc::new(Mutex::new(Vec::<Call>::new()));
        let seen = calls.clone();
        let stop = Arc::new(AtomicBool::new(false));
        let stopped = stop.clone();
        let thread = std::thread::spawn(move || {
            let mut polls = 0;
            while !stopped.load(Ordering::Relaxed) {
                let mut stream = match listener.accept() {
                    Ok((stream, _)) => stream,
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        std::thread::sleep(Duration::from_millis(1));
                        continue;
                    }
                    Err(e) => panic!("{e}"),
                };
                stream.set_nonblocking(false).unwrap();
                stream
                    .set_read_timeout(Some(Duration::from_secs(3)))
                    .unwrap();
                let mut raw = Vec::new();
                let mut chunk = [0; 4096];
                let (end, len) = loop {
                    if let Some(end) = raw.windows(4).position(|w| w == b"\r\n\r\n") {
                        let head = String::from_utf8_lossy(&raw[..end]);
                        let len = head
                            .lines()
                            .find_map(|line| {
                                line.to_ascii_lowercase()
                                    .strip_prefix("content-length:")
                                    .and_then(|s| s.trim().parse::<usize>().ok())
                            })
                            .unwrap_or(0);
                        if raw.len() >= end + 4 + len {
                            break (end, len);
                        }
                    }
                    let n = stream.read(&mut chunk).unwrap();
                    assert_ne!(n, 0);
                    raw.extend_from_slice(&chunk[..n]);
                };
                let head = String::from_utf8_lossy(&raw[..end]).into_owned();
                let path = head
                    .lines()
                    .next()
                    .unwrap()
                    .split_whitespace()
                    .nth(1)
                    .unwrap()
                    .to_owned();
                seen.lock().unwrap().push(Call {
                    path: path.clone(),
                    headers: head,
                    body: raw[end + 4..end + 4 + len].to_vec(),
                });
                let mut status = 200;
                let body = if let Some(class) = path.strip_prefix("/object_info/") {
                    if scenario == "missing_model" && class == "UNETLoader" {
                        json!({class: {"input": {"required": {"unet_name": [[]]}}}})
                            .to_string()
                            .into_bytes()
                    } else {
                        object_info(class).to_string().into_bytes()
                    }
                } else if path == "/upload/image" {
                    json!({"name": "uploaded reference.png", "subfolder": "RurixForge", "type": "input"}).to_string().into_bytes()
                } else if path == "/prompt" {
                    json!({"prompt_id": "our_prompt", "number": 1, "node_errors": {}})
                        .to_string()
                        .into_bytes()
                } else if path == "/history/our_prompt" {
                    polls += 1;
                    if scenario == "history_http" {
                        status = 500;
                        b"{}".to_vec()
                    } else if scenario == "pending" || (scenario == "success" && polls == 1) {
                        json!({"other_prompt": {"status": {"completed": true, "status_str": "success"}, "outputs": {"15": {"images": [{"filename": "other.mp4", "type": "output"}]}}}}).to_string().into_bytes()
                    } else {
                        let mut task = json!({"status": {"completed": true, "status_str": "success"},
                            "outputs": {"15": {"images": [{"filename": "our output.mp4", "subfolder": "video", "type": "output"}]}}});
                        if scenario == "execution_error" {
                            task["status"] = json!({"completed": false, "status_str": "error",
                            "messages": [["execution_error", {"exception_message": "CUDA out of memory", "prompt_id": "our_prompt"}]]});
                        }
                        if scenario == "missing_output" {
                            task["outputs"] = json!({"999": {"images": [{"filename": "unrelated.mp4", "type": "output"}]}});
                        }
                        json!({"our_prompt": task}).to_string().into_bytes()
                    }
                } else if path.starts_with("/view?") {
                    if scenario == "bad_mp4" {
                        b"not a video".to_vec()
                    } else {
                        b"\0\0\0\x18ftypisommock-video".to_vec()
                    }
                } else {
                    panic!("Unexpected path {path}")
                };
                write!(
                    stream,
                    "HTTP/1.1 {status} Test\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    body.len()
                )
                .unwrap();
                stream.write_all(&body).unwrap();
            }
        });
        Server {
            endpoint,
            calls,
            stop,
            thread: Some(thread),
        }
    }

    #[test]
    fn live_readiness_is_distinct_from_configuration_and_missing_model_never_submits() {
        let server = server("missing_model");
        let config = cfg(Some(&server.endpoint));
        assert!(ComfyuiMiniMaxH3.configured(&config, &keys()));
        let error = ComfyuiMiniMaxH3.readiness(&config).unwrap_err();
        assert!(error
            .message
            .contains("minimax_h3_fl2va_pruned_int8_convrot.safetensors"));
        let directory = std::env::temp_dir().join(client_id());
        assert!(generate(
            &req(json!({})),
            &config,
            Duration::ZERO,
            Duration::from_secs(1),
            &directory
        )
        .is_err());
        assert!(!server
            .calls
            .lock()
            .unwrap()
            .iter()
            .any(|c| c.path == "/prompt"));
        assert!(
            !directory.exists(),
            "Readiness failure must not create a submitted receipt"
        );
    }

    #[test]
    fn t2v_and_i2v_submit_once_poll_own_task_and_download_exact_output() {
        for image in [false, true] {
            let server = server("success");
            let directory = std::env::temp_dir().join(client_id());
            let mut params =
                json!({"durationSec": 2, "resolution": "352p", "aspect": "16:9", "seed": 42});
            if image {
                params["imageDataUrl"] = json!(data_image());
            }
            let out = generate(
                &req(params),
                &cfg(Some(&server.endpoint)),
                Duration::ZERO,
                Duration::from_secs(2),
                &directory,
            )
            .unwrap();
            assert_eq!(out.len(), 1);
            assert_eq!(out[0].bytes, b"\0\0\0\x18ftypisommock-video");
            assert_eq!(out[0].meta["taskId"], "our_prompt");
            assert_eq!(
                out[0].meta["mode"],
                if image { "image2video" } else { "text2video" }
            );
            assert_eq!(out[0].meta["numFrames"], 56);
            assert_eq!(out[0].meta["durationSec"], json!(56.0 / 24.0));
            assert_eq!(out[0].meta["requestedDurationSec"], 2);
            assert_eq!(out[0].meta["audio"]["channels"], 2);
            let calls = server.calls.lock().unwrap();
            let posts: Vec<_> = calls.iter().filter(|c| c.path == "/prompt").collect();
            assert_eq!(posts.len(), 1);
            assert!(!calls
                .iter()
                .any(|c| c.headers.to_ascii_lowercase().contains("authorization:")));
            let submitted: Value = serde_json::from_slice(&posts[0].body).unwrap();
            assert_eq!(
                submitted["prompt"]["6"]["inputs"]["prompt"],
                "A plant moves gently in the breeze."
            );
            assert_eq!(submitted["prompt"]["6"]["inputs"]["width"], 608);
            assert_eq!(submitted["prompt"]["6"]["inputs"]["length"], 56);
            assert_eq!(submitted["prompt"]["10"]["inputs"]["noise_seed"], 42);
            if image {
                let uploads: Vec<_> = calls.iter().filter(|c| c.path == "/upload/image").collect();
                assert_eq!(uploads.len(), 1);
                assert!(uploads[0]
                    .headers
                    .contains("multipart/form-data; boundary="));
                assert!(uploads[0].body.windows(png().len()).any(|w| w == png()));
                let multipart = String::from_utf8_lossy(&uploads[0].body);
                assert!(multipart.contains("name=\"image\"; filename=\"forge-h3-"));
                assert!(multipart.contains("name=\"overwrite\"\r\n\r\nfalse"));
                assert_eq!(submitted["prompt"]["16"]["class_type"], "LoadImage");
                assert_eq!(
                    submitted["prompt"]["16"]["inputs"]["image"],
                    "RurixForge/uploaded reference.png"
                );
                assert_eq!(
                    submitted["prompt"]["6"]["inputs"]["first_frame"],
                    json!(["16", 0])
                );
            } else {
                assert!(!calls.iter().any(|c| c.path == "/upload/image"));
                assert!(submitted["prompt"]["6"]["inputs"]
                    .get("first_frame")
                    .is_none());
            }
            assert_eq!(
                calls
                    .iter()
                    .filter(|c| c.path == "/history/our_prompt")
                    .count(),
                2
            );
            let downloads: Vec<_> = calls
                .iter()
                .filter(|c| c.path.starts_with("/view?"))
                .collect();
            assert_eq!(downloads.len(), 1);
            assert!(
                downloads[0].path.contains("filename=our%20output.mp4")
                    || downloads[0].path.contains("filename=our+output.mp4")
            );
            assert!(downloads[0].path.contains("subfolder=video"));
            assert!(downloads[0].path.contains("type=output"));
            assert!(!downloads[0].path.contains("other"));
            let receipt: Value = serde_json::from_slice(
                &std::fs::read(out[0].meta["receipt"].as_str().unwrap()).unwrap(),
            )
            .unwrap();
            assert_eq!(receipt["status"], "completed");
            assert_eq!(receipt["promptId"], "our_prompt");
        }
    }

    #[test]
    fn every_post_submit_failure_retains_prompt_id_and_never_resubmits() {
        for scenario in [
            "execution_error",
            "history_http",
            "missing_output",
            "bad_mp4",
            "pending",
        ] {
            let server = server(scenario);
            let directory = std::env::temp_dir().join(client_id());
            let budget = if scenario == "pending" {
                Duration::from_millis(1)
            } else {
                Duration::from_secs(2)
            };
            let error = generate(
                &req(json!({})),
                &cfg(Some(&server.endpoint)),
                Duration::ZERO,
                budget,
                &directory,
            )
            .unwrap_err();
            assert_eq!(error.code, GEN_BACKEND_ERROR);
            assert!(
                error.message.contains("prompt_id: our_prompt"),
                "{scenario}: {error}"
            );
            if scenario == "execution_error" {
                assert!(error.message.contains("CUDA out of memory"));
            }
            assert_eq!(
                server
                    .calls
                    .lock()
                    .unwrap()
                    .iter()
                    .filter(|c| c.path == "/prompt")
                    .count(),
                1
            );
            let receipt_path = std::fs::read_dir(directory)
                .unwrap()
                .next()
                .unwrap()
                .unwrap()
                .path();
            let receipt: Value =
                serde_json::from_slice(&std::fs::read(receipt_path).unwrap()).unwrap();
            assert_eq!(receipt["promptId"], "our_prompt");
            assert_eq!(receipt["status"], "error");
        }
    }
}
