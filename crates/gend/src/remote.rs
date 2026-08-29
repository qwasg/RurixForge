//! remote-openai-compatible 适配器(08 §6.2 真实 HTTP 适配面)。
//!
//! POST {endpoint}/v1/images/generations {model?,prompt,n,size},
//! Authorization: Bearer <keystore key>;响应 data[] 取 b64_json(base64)或 url(二次 GET)。
//! 错误如实映射:429 → GEN_RATE_LIMITED;其余 HTTP 状态/连接失败 → GEN_BACKEND_ERROR,
//! 并带上服务端自述原因(权限/额度/模型不可用等排障全靠它)。
//! 红线 R-5:密钥只进 Authorization 头,错误信息只带状态码与服务端文本,不回显密钥/请求头。

use base64::Engine;
use serde_json::{json, Value};

use crate::backends::{GenBackend, GenCandidate, GenRequest, MAX_BATCH, SIZES};
use crate::config::GenConfig;
use crate::keystore::Keystore;
use crate::{
    GenError, Result, GEN_BACKEND_ERROR, GEN_BACKEND_NOT_CONFIGURED, GEN_BAD_PARAMS,
    GEN_RATE_LIMITED,
};

pub struct RemoteOpenAi;

pub const REMOTE_OPENAI_ID: &str = "remote-openai-compatible";

/// 单次 HTTP 超时:生图模型(gpt-image 等)出图常需 1~3 分钟,原 30s 必然中断。
/// 取 300s,留余量于 agentd 侧 gen-image MCP 调用的 360s 上限之内。
const HTTP_TIMEOUT_SECS: u64 = 300;

/// 向端点请求的原生边长。OpenAI images API(gpt-image-1/2、dall-e-3)最小方形即
/// 1024x1024,不接受 256/512。故一律按 1024 请求,再本地降采样到调用方要的边长——
/// 出图确为高分辨率后缩,不是放大伪装。
const REMOTE_NATIVE_SIZE: u32 = 1024;

impl GenBackend for RemoteOpenAi {
    fn id(&self) -> &str {
        REMOTE_OPENAI_ID
    }

    fn kind(&self) -> &str {
        "remote"
    }

    /// remote 类:enabled + endpoint 非空 + key 存在(env 或 keystore 文件)。
    fn configured(&self, cfg: &GenConfig, keys: &Keystore) -> bool {
        match cfg.entry(REMOTE_OPENAI_ID) {
            Some(e) => {
                e.kind == "remote"
                    && e.enabled
                    && e.endpoint.as_deref().map(|s| !s.trim().is_empty()).unwrap_or(false)
                    && keys.key_for(REMOTE_OPENAI_ID).is_some()
            }
            None => false,
        }
    }

    fn capabilities(&self) -> Value {
        json!({
            // v1 仅 text2img;img2img/styleRef/texture-set 为已知 seam(RD-F5-002)。
            "kinds": ["text2img"],
            "sizes": SIZES,
            "maxBatch": MAX_BATCH,
        })
    }

    fn generate(&self, req: &GenRequest, cfg: &GenConfig, keys: &Keystore) -> Result<Vec<GenCandidate>> {
        if !SIZES.contains(&req.size) {
            return Err(GenError::new(
                GEN_BAD_PARAMS,
                format!("size 须为 {SIZES:?} 之一,实: {}", req.size),
            ));
        }
        if req.n == 0 || req.n > MAX_BATCH {
            return Err(GenError::new(
                GEN_BAD_PARAMS,
                format!("n 须 1..={MAX_BATCH},实: {}", req.n),
            ));
        }
        if !self.configured(cfg, keys) {
            return Err(GenError::new(
                GEN_BACKEND_NOT_CONFIGURED,
                "remote-openai-compatible 未配置(需 enabled + endpoint + key)",
            ));
        }
        let entry = cfg.entry(REMOTE_OPENAI_ID).expect("configured 为真必有条目");
        let endpoint = entry.endpoint.as_deref().expect("configured 为真必有 endpoint");
        // key 只用于拼 Authorization 头;任何分支不得把 key 写入错误信息。
        let key = keys.key_for(REMOTE_OPENAI_ID).expect("configured 为真必有 key");

        let url = format!("{}/v1/images/generations", endpoint.trim_end_matches('/'));
        // 只发 OpenAI images API 的标准字段(model/prompt/n/size)。seed 与 negativePrompt
        // 不属于该 API,发过去会被判 400 unknown_parameter:negative 语义并入 prompt 文本
        // (真实生效),seed 退化为本地候选标识——远程出图本就不可按种子复现,不伪装成可复现。
        let mut prompt = req.prompt.clone();
        if let Some(np) = req
            .negative_prompt
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
        {
            prompt.push_str("\n\nAvoid the following: ");
            prompt.push_str(np);
        }
        let mut body = json!({
            "prompt": prompt,
            "n": req.n,
            "size": format!("{REMOTE_NATIVE_SIZE}x{REMOTE_NATIVE_SIZE}"),
        });
        if let Some(m) = &entry.model {
            body["model"] = json!(m);
        }

        let agent = ureq::AgentBuilder::new()
            .timeout(std::time::Duration::from_secs(HTTP_TIMEOUT_SECS))
            .build();
        let resp = agent
            .post(&url)
            .set("Authorization", &format!("Bearer {key}"))
            .set("Content-Type", "application/json")
            .send_string(&body.to_string());
        let resp = match resp {
            Ok(r) => r,
            Err(ureq::Error::Status(429, r)) => {
                let why = error_detail(r);
                return Err(GenError::new(
                    GEN_RATE_LIMITED,
                    format!("远程后端限流(HTTP 429){why}"),
                ));
            }
            Err(ureq::Error::Status(code, r)) => {
                let why = error_detail(r);
                return Err(GenError::new(
                    GEN_BACKEND_ERROR,
                    format!("远程后端 HTTP {code}{why}"),
                ));
            }
            Err(ureq::Error::Transport(t)) => {
                return Err(GenError::new(
                    GEN_BACKEND_ERROR,
                    format!("远程后端连接失败: {t}"),
                ));
            }
        };
        let bytes = read_body(resp)?;
        let doc: Value = serde_json::from_slice(&bytes).map_err(|e| {
            GenError::new(GEN_BACKEND_ERROR, format!("远程响应非 JSON: {e}"))
        })?;
        let data = doc
            .get("data")
            .and_then(Value::as_array)
            .ok_or_else(|| GenError::new(GEN_BACKEND_ERROR, "远程响应缺 data[]"))?;

        let mut out = Vec::new();
        for (i, item) in data.iter().enumerate() {
            let seed = req.seed.wrapping_add(i as u64);
            if let Some(b64) = item.get("b64_json").and_then(Value::as_str) {
                let raw = base64::engine::general_purpose::STANDARD.decode(b64).map_err(|e| {
                    GenError::new(GEN_BACKEND_ERROR, format!("b64_json 解码失败: {e}"))
                })?;
                out.push(GenCandidate { png_bytes: fit_to_size(raw, req.size)?, seed });
            } else if let Some(u) = item.get("url").and_then(Value::as_str) {
                let raw = fetch_url(&agent, u)?;
                out.push(GenCandidate { png_bytes: fit_to_size(raw, req.size)?, seed });
            } else {
                return Err(GenError::new(
                    GEN_BACKEND_ERROR,
                    format!("远程响应 data[{i}] 无 b64_json/url"),
                ));
            }
        }
        if out.is_empty() {
            return Err(GenError::new(GEN_BACKEND_ERROR, "远程响应 data[] 为空"));
        }
        Ok(out)
    }
}

/// 响应体读全(ureq into_reader,无 charset/json 特性依赖)。
fn read_body(resp: ureq::Response) -> Result<Vec<u8>> {
    use std::io::Read;
    let mut buf = Vec::new();
    resp.into_reader()
        .read_to_end(&mut buf)
        .map_err(|e| GenError::new(GEN_BACKEND_ERROR, format!("读远程响应体失败: {e}")))?;
    Ok(buf)
}

/// 错误响应体里的服务端自述原因(OpenAI 形态 {"error":{"message":..}};非 JSON 则取原文)。
/// 只含服务端自己回的文本,不掺请求头/密钥(R-5);截断 200 字避免长 HTML 错误页灌进事件流。
fn error_detail(resp: ureq::Response) -> String {
    let Ok(bytes) = read_body(resp) else {
        return String::new();
    };
    let text = String::from_utf8_lossy(&bytes);
    let msg = match serde_json::from_str::<Value>(&text) {
        Ok(v) => v
            .get("error")
            .and_then(|e| e.get("message"))
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
        Err(_) => text.trim().to_string(),
    };
    if msg.is_empty() {
        return String::new();
    }
    let msg: String = msg.chars().take(200).collect();
    format!(": {msg}")
}

/// 远程产物归一到调用方要的边长与 PNG 容器(下游 tmpstore/accept 一律按 .png 落盘)。
/// 已是目标尺寸的 PNG 则原样透传,不做无谓重编码。
fn fit_to_size(bytes: Vec<u8>, size: u32) -> Result<Vec<u8>> {
    const PNG_MAGIC: [u8; 8] = [0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a];
    let img = image::load_from_memory(&bytes)
        .map_err(|e| GenError::new(GEN_BACKEND_ERROR, format!("远程产物解码失败: {e}")))?;
    let sized = img.width() == size && img.height() == size;
    if sized && bytes.starts_with(&PNG_MAGIC) {
        return Ok(bytes);
    }
    let img = if sized {
        img
    } else {
        img.resize_exact(size, size, image::imageops::FilterType::Lanczos3)
    };
    let rgba = img.to_rgba8();
    let mut buf = Vec::new();
    use image::ImageEncoder;
    image::codecs::png::PngEncoder::new(&mut buf)
        .write_image(
            rgba.as_raw(),
            rgba.width(),
            rgba.height(),
            image::ExtendedColorType::Rgba8,
        )
        .map_err(|e| GenError::new(GEN_BACKEND_ERROR, format!("PNG 重编码失败: {e}")))?;
    Ok(buf)
}

/// url[] 候选的二次 GET。
fn fetch_url(agent: &ureq::Agent, url: &str) -> Result<Vec<u8>> {
    match agent.get(url).call() {
        Ok(r) => read_body(r),
        Err(ureq::Error::Status(code, _)) => Err(GenError::new(
            GEN_BACKEND_ERROR,
            format!("候选 url 拉取 HTTP {code}"),
        )),
        Err(ureq::Error::Transport(t)) => Err(GenError::new(
            GEN_BACKEND_ERROR,
            format!("候选 url 拉取连接失败: {t}"),
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::BackendEntry;
    use crate::TEST_ENV_LOCK as ENV_LOCK;
    use std::io::{Read, Write};
    use std::net::TcpListener;

    fn env_lock() -> std::sync::MutexGuard<'static, ()> {
        ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// 一次性 HTTP 应答桩:接 1 个连接,读完请求,回固定状态行+体;
    /// 收到的请求体经 channel 回传,供断言发出去的 JSON 形态。
    fn http_stub_capture(
        status_line: &'static str,
        body: Vec<u8>,
    ) -> (String, std::sync::mpsc::Receiver<String>) {
        let listener = TcpListener::bind("127.0.0.1:0").expect("绑定随机端口失败");
        let port = listener.local_addr().unwrap().port();
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            if let Ok((mut s, _)) = listener.accept() {
                let mut req = Vec::new();
                let mut chunk = [0u8; 4096];
                // 读到头体分隔 + Content-Length 指示的体长为止(够测试用)。
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
                            .find_map(|l| l.to_ascii_lowercase().strip_prefix("content-length:").map(str::trim).and_then(|v| v.parse::<usize>().ok()))
                            .unwrap_or(0);
                        if req.len() >= pos + 4 + len {
                            let sent = String::from_utf8_lossy(&req[pos + 4..pos + 4 + len]).to_string();
                            let _ = tx.send(sent);
                            break;
                        }
                    }
                }
                let resp = format!(
                    "HTTP/1.1 {status_line}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    body.len()
                );
                let _ = s.write_all(resp.as_bytes());
                let _ = s.write_all(&body);
            }
        });
        (format!("http://127.0.0.1:{port}"), rx)
    }

    fn http_stub_once(status_line: &'static str, body: &[u8]) -> String {
        http_stub_capture(status_line, body.to_vec()).0
    }

    /// {"data":[{"b64_json": <png>}]} 应答体。
    fn b64_body(png: &[u8]) -> Vec<u8> {
        format!(
            r#"{{"data":[{{"b64_json":"{}"}}]}}"#,
            base64::engine::general_purpose::STANDARD.encode(png)
        )
        .into_bytes()
    }

    fn find_subslice(hay: &[u8], needle: &[u8]) -> Option<usize> {
        hay.windows(needle.len()).position(|w| w == needle)
    }

    fn cfg_with_endpoint(endpoint: &str) -> GenConfig {
        GenConfig {
            backends: vec![BackendEntry {
                id: REMOTE_OPENAI_ID.into(),
                kind: "remote".into(),
                enabled: true,
                endpoint: Some(endpoint.into()),
                model: Some("test-model".into()),
            }],
        }
    }

    fn req() -> GenRequest {
        GenRequest { prompt: "p".into(), negative_prompt: None, size: 256, seed: 7, n: 1 }
    }

    #[test]
    fn http_429_maps_rate_limited() {
        let _g = env_lock();
        std::env::set_var("FORGE_GEN_API_KEY", "sk-test-dummy");
        let ep = http_stub_once("429 Too Many Requests", b"{}");
        let cfg = cfg_with_endpoint(&ep);
        let ks = Keystore::load_from(std::path::Path::new("no-such-ks.json"));
        let err = RemoteOpenAi.generate(&req(), &cfg, &ks).unwrap_err();
        assert_eq!(err.code, GEN_RATE_LIMITED);
        assert!(!err.message.contains("sk-test-dummy"), "错误信息不得含密钥");
        std::env::remove_var("FORGE_GEN_API_KEY");
    }

    #[test]
    fn http_500_maps_backend_error() {
        let _g = env_lock();
        std::env::set_var("FORGE_GEN_API_KEY", "sk-test-dummy");
        let ep = http_stub_once("500 Internal Server Error", b"oops");
        let cfg = cfg_with_endpoint(&ep);
        let ks = Keystore::load_from(std::path::Path::new("no-such-ks.json"));
        let err = RemoteOpenAi.generate(&req(), &cfg, &ks).unwrap_err();
        assert_eq!(err.code, GEN_BACKEND_ERROR);
        assert!(err.message.contains("500"));
        std::env::remove_var("FORGE_GEN_API_KEY");
    }

    #[test]
    fn connection_refused_maps_backend_error() {
        let _g = env_lock();
        std::env::set_var("FORGE_GEN_API_KEY", "sk-test-dummy");
        // 127.0.0.1:1 必连不上(冒烟同形态)。
        let cfg = cfg_with_endpoint("http://127.0.0.1:1");
        let ks = Keystore::load_from(std::path::Path::new("no-such-ks.json"));
        let err = RemoteOpenAi.generate(&req(), &cfg, &ks).unwrap_err();
        assert_eq!(err.code, GEN_BACKEND_ERROR);
        std::env::remove_var("FORGE_GEN_API_KEY");
    }

    #[test]
    fn b64_json_success_roundtrip() {
        let _g = env_lock();
        std::env::set_var("FORGE_GEN_API_KEY", "sk-test-dummy");
        // 应答已是目标边长的 PNG → 原样透传,不重编码。
        let png = crate::mock::render_map("p", "albedo", 256, 1).unwrap();
        let ep = http_stub_once("200 OK", &b64_body(&png));
        let cfg = cfg_with_endpoint(&ep);
        let ks = Keystore::load_from(std::path::Path::new("no-such-ks.json"));
        let out = RemoteOpenAi.generate(&req(), &cfg, &ks).unwrap();
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].png_bytes, png);
        assert_eq!(out[0].seed, 7);
        std::env::remove_var("FORGE_GEN_API_KEY");
    }

    /// 请求体须是 OpenAI images API 的标准字段面:官方端点见到 seed/negativePrompt
    /// 会判 400,且不接受 256/512 边长。
    #[test]
    fn request_body_is_openai_standard() {
        let _g = env_lock();
        std::env::set_var("FORGE_GEN_API_KEY", "sk-test-dummy");
        let png = crate::mock::render_map("p", "albedo", 256, 1).unwrap();
        let (ep, rx) = http_stub_capture("200 OK", b64_body(&png));
        let cfg = cfg_with_endpoint(&ep);
        let ks = Keystore::load_from(std::path::Path::new("no-such-ks.json"));
        let mut r = req();
        r.negative_prompt = Some("blurry, watermark".into());
        RemoteOpenAi.generate(&r, &cfg, &ks).unwrap();
        let sent: Value = serde_json::from_str(&rx.recv().unwrap()).unwrap();

        assert!(sent.get("seed").is_none(), "seed 不是该 API 的参数: {sent}");
        assert!(sent.get("negativePrompt").is_none(), "negativePrompt 非标准: {sent}");
        // 边长按端点原生 1024 请求(调用方要的 256 由本地降采样得到)。
        assert_eq!(sent["size"], "1024x1024");
        assert_eq!(sent["model"], "test-model");
        assert_eq!(sent["n"], 1);
        // negative 语义并入 prompt,不静默丢弃。
        let prompt = sent["prompt"].as_str().unwrap();
        assert!(prompt.starts_with("p"), "{prompt}");
        assert!(prompt.contains("blurry, watermark"), "negative 未并入 prompt: {prompt}");
        std::env::remove_var("FORGE_GEN_API_KEY");
    }

    /// 端点回 1024,调用方要 256 → 本地降采样到 256。
    #[test]
    fn downscales_native_size_to_requested() {
        let _g = env_lock();
        std::env::set_var("FORGE_GEN_API_KEY", "sk-test-dummy");
        let png = crate::mock::render_map("p", "albedo", 1024, 1).unwrap();
        let ep = http_stub_once("200 OK", &b64_body(&png));
        let cfg = cfg_with_endpoint(&ep);
        let ks = Keystore::load_from(std::path::Path::new("no-such-ks.json"));
        let out = RemoteOpenAi.generate(&req(), &cfg, &ks).unwrap();
        let img = image::load_from_memory(&out[0].png_bytes).unwrap();
        assert_eq!((img.width(), img.height()), (256, 256));
        std::env::remove_var("FORGE_GEN_API_KEY");
    }

    /// 服务端自述原因须进错误信息(权限/额度类失败全靠它定位),但密钥仍不得出现。
    #[test]
    fn http_error_carries_server_reason() {
        let _g = env_lock();
        std::env::set_var("FORGE_GEN_API_KEY", "sk-test-dummy");
        let body = br#"{"error":{"message":"Image generation is not enabled for this group","type":"permission_error"}}"#;
        let ep = http_stub_once("403 Forbidden", body);
        let cfg = cfg_with_endpoint(&ep);
        let ks = Keystore::load_from(std::path::Path::new("no-such-ks.json"));
        let err = RemoteOpenAi.generate(&req(), &cfg, &ks).unwrap_err();
        assert_eq!(err.code, GEN_BACKEND_ERROR);
        assert!(err.message.contains("403"), "{}", err.message);
        assert!(
            err.message.contains("Image generation is not enabled"),
            "未带服务端原因: {}",
            err.message
        );
        assert!(!err.message.contains("sk-test-dummy"), "错误信息不得含密钥");
        std::env::remove_var("FORGE_GEN_API_KEY");
    }
}
