//! remote-openai-compatible 适配器(08 §6.2 真实 HTTP 适配面)。
//!
//! POST {endpoint}/v1/images/generations {model?,prompt,n,size:"{s}x{s}",seed?},
//! Authorization: Bearer <keystore key>;响应 data[] 取 b64_json(base64)或 url(二次 GET)。
//! 错误如实映射:429 → GEN_RATE_LIMITED;其余 HTTP 状态/连接失败 → GEN_BACKEND_ERROR。
//! 红线 R-5:密钥只进 Authorization 头,错误信息只带状态码,不回显密钥/请求头。

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
        let mut body = json!({
            "prompt": req.prompt,
            "n": req.n,
            "size": format!("{0}x{0}", req.size),
        });
        if let Some(m) = &entry.model {
            body["model"] = json!(m);
        }
        // openai 兼容端点对 seed 支持不一;有 seed 如实带上(不支持的端点自行忽略)。
        body["seed"] = json!(req.seed);
        if let Some(np) = &req.negative_prompt {
            body["negativePrompt"] = json!(np);
        }

        let agent = ureq::AgentBuilder::new()
            .timeout(std::time::Duration::from_secs(30))
            .build();
        let resp = agent
            .post(&url)
            .set("Authorization", &format!("Bearer {key}"))
            .set("Content-Type", "application/json")
            .send_string(&body.to_string());
        let resp = match resp {
            Ok(r) => r,
            Err(ureq::Error::Status(429, _)) => {
                return Err(GenError::new(GEN_RATE_LIMITED, "远程后端限流(HTTP 429)"));
            }
            Err(ureq::Error::Status(code, _)) => {
                return Err(GenError::new(
                    GEN_BACKEND_ERROR,
                    format!("远程后端 HTTP {code}"),
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
                let png = base64::engine::general_purpose::STANDARD.decode(b64).map_err(|e| {
                    GenError::new(GEN_BACKEND_ERROR, format!("b64_json 解码失败: {e}"))
                })?;
                out.push(GenCandidate { png_bytes: png, seed });
            } else if let Some(u) = item.get("url").and_then(Value::as_str) {
                let png = fetch_url(&agent, u)?;
                out.push(GenCandidate { png_bytes: png, seed });
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

    /// 一次性 HTTP 应答桩:接 1 个连接,读完请求,回固定状态行+体。
    fn http_stub_once(status_line: &'static str, body: &'static [u8]) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").expect("绑定随机端口失败");
        let port = listener.local_addr().unwrap().port();
        std::thread::spawn(move || {
            if let Ok((mut s, _)) = listener.accept() {
                let mut req = Vec::new();
                let mut chunk = [0u8; 4096];
                // 读到头体分隔 + Content-Length 指示的体长为止(够测试用)。
                let need = loop {
                    let n = s.read(&mut chunk).unwrap_or(0);
                    if n == 0 {
                        break 0usize;
                    }
                    req.extend_from_slice(&chunk[..n]);
                    if let Some(pos) = find_subslice(&req, b"\r\n\r\n") {
                        let head = String::from_utf8_lossy(&req[..pos]).to_string();
                        let len = head
                            .lines()
                            .find_map(|l| l.to_ascii_lowercase().strip_prefix("content-length:").map(str::trim).and_then(|v| v.parse::<usize>().ok()))
                            .unwrap_or(0);
                        if req.len() >= pos + 4 + len {
                            break 0;
                        }
                    }
                };
                let _ = need;
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
        let png = crate::mock::render_map("p", "albedo", 256, 1).unwrap();
        let body = format!(
            r#"{{"data":[{{"b64_json":"{}"}}]}}"#,
            base64::engine::general_purpose::STANDARD.encode(&png)
        );
        let body: &'static [u8] = Box::leak(body.into_bytes().into_boxed_slice());
        let ep = http_stub_once("200 OK", body);
        let cfg = cfg_with_endpoint(&ep);
        let ks = Keystore::load_from(std::path::Path::new("no-such-ks.json"));
        let out = RemoteOpenAi.generate(&req(), &cfg, &ks).unwrap();
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].png_bytes, png);
        assert_eq!(out[0].seed, 7);
        std::env::remove_var("FORGE_GEN_API_KEY");
    }
}
