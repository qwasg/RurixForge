//! 阿里云百炼 MiniMax-H3:单次建任务 → 轮询 → 下载 MP4。
//! https://help.aliyun.com/zh/model-studio/minimax-video-generation-api-reference
//! endpoint 为北京业务空间域名根。参考图支持 HTTP(S) URL 和官方私有临时存储
//! oss:// 引用，后者显式添加 OssResourceResolve；本地/data URI 须先真实上传。

use super::{
    read_body, redact_key, remote_configured, remote_conn, MediaArtifact, MediaBackend, MediaKind,
    MediaRequest,
};
use crate::config::GenConfig;
use crate::keystore::Keystore;
use crate::{GenError, Result, GEN_BACKEND_ERROR, GEN_BAD_PARAMS, GEN_RATE_LIMITED};
use serde_json::{json, Value};
use std::time::{Duration, Instant};

pub const ALIYUN_MINIMAX_VIDEO_ID: &str = "aliyun-minimax-video";
const MODEL: &str = "MiniMax/MiniMax-H3";
const CREATE_PATH: &str = "/api/v1/services/aigc/video-generation/video-synthesis";
const TASK_PATH: &str = "/api/v1/tasks";
const ASPECTS: [&str; 6] = ["16:9", "9:16", "1:1", "4:3", "3:4", "21:9"];
const HTTP_TIMEOUT: Duration = Duration::from_secs(120);
const POLL_INTERVAL: Duration = Duration::from_secs(15);
const POLL_BUDGET: Duration = Duration::from_secs(900);

pub struct AliyunMiniMaxVideo;

impl MediaBackend for AliyunMiniMaxVideo {
    fn id(&self) -> &str {
        ALIYUN_MINIMAX_VIDEO_ID
    }
    fn kind(&self) -> &str {
        "remote"
    }
    fn configured(&self, cfg: &GenConfig, keys: &Keystore) -> bool {
        remote_configured(self.id(), cfg, keys)
    }
    fn capabilities(&self) -> Value {
        json!({
            "kinds": ["text2video", "image2video"],
            "aspects": ASPECTS,
            "resolutions": ["768p", "2k"],
            "defaultResolution": "768p",
            "minDurationSec": 4,
            "maxDurationSec": 15,
            "maxBatch": 1,
            "formats": ["mp4"],
            "maxPromptChars": 7000,
            "imageInputTypes": ["url", "oss"],
            "defaultModel": MODEL,
        })
    }
    fn generate(
        &self,
        req: &MediaRequest,
        cfg: &GenConfig,
        keys: &Keystore,
    ) -> Result<Vec<MediaArtifact>> {
        generate_with_policy(req, cfg, keys, POLL_INTERVAL, POLL_BUDGET)
    }
}

fn bad(message: impl Into<String>) -> GenError {
    GenError::new(GEN_BAD_PARAMS, message)
}
fn failure(message: impl Into<String>) -> GenError {
    GenError::new(GEN_BACKEND_ERROR, message)
}

/// 限制媒体/端点 URL 协议与非空 authority;不接受带凭据的 URL。
fn is_http_url(value: &str) -> bool {
    let Some(rest) = value
        .strip_prefix("https://")
        .or_else(|| value.strip_prefix("http://"))
    else {
        return false;
    };
    let authority = rest.split(['/', '?', '#']).next().unwrap_or("");
    !authority.is_empty() && !authority.contains('@') && !value.chars().any(char::is_whitespace)
}

/// Model Studio's authenticated temporary storage, scoped to this account/model.
/// https://help.aliyun.com/zh/model-studio/get-temporary-file-url
fn is_private_upload_url(value: &str) -> bool {
    value.strip_prefix("oss://").is_some_and(|path| {
        !path.is_empty() && !path.starts_with('/') && !path.contains(['?', '#', '@'])
            && !path.split('/').any(|part| part == "..")
            && !path.chars().any(char::is_whitespace)
    })
}

fn build_body(req: &MediaRequest, model: Option<&str>) -> Result<Value> {
    if req.kind != MediaKind::Video {
        return Err(bad("MiniMax-H3 仅支持视频生成"));
    }
    let prompt = req.prompt.trim();
    if prompt.is_empty() || prompt.chars().count() > 7000 {
        return Err(bad("MiniMax-H3 prompt 必须为 1..=7000 个字符"));
    }
    let model = model.filter(|m| !m.trim().is_empty()).unwrap_or(MODEL);
    if model != MODEL {
        return Err(bad(format!("阿里云 MiniMax 视频模型须为 {MODEL}")));
    }
    let resolution = match req.params.get("resolution") {
        None | Some(Value::Null) => "768P",
        Some(Value::String(s)) if s.eq_ignore_ascii_case("768p") => "768P",
        Some(Value::String(s)) if s.eq_ignore_ascii_case("2k") => "2K",
        _ => return Err(bad("MiniMax-H3 resolution 仅支持 768p / 2k")),
    };
    let duration = match req.params.get("durationSec") {
        None | Some(Value::Null) => 5,
        Some(v) => v
            .as_u64()
            .filter(|d| (4..=15).contains(d))
            .ok_or_else(|| bad("MiniMax-H3 durationSec 须为 4..=15 的整数"))?,
    };
    let image = match req
        .params
        .get("imageDataUrl")
        .or_else(|| req.params.get("imageUrl"))
    {
        None | Some(Value::Null) => None,
        Some(Value::String(s)) if s.trim().is_empty() => None,
        Some(Value::String(s)) if is_http_url(s.trim()) || is_private_upload_url(s.trim()) => Some(s.trim()),
        _ => {
            return Err(bad(
                "MiniMax-H3 首帧图片须为 HTTP(S) URL 或本账号上传的 oss:// 临时引用;请先上传本地图片",
            ))
        }
    };
    let aspect = match req.params.get("aspect") {
        None | Some(Value::Null) => "16:9",
        Some(Value::String(s))
            if ASPECTS.contains(&s.as_str()) || (image.is_some() && s == "adaptive") =>
        {
            s
        }
        _ => {
            return Err(bad(
                "MiniMax-H3 视频比例须为 16:9 / 9:16 / 1:1 / 4:3 / 3:4 / 21:9",
            ))
        }
    };
    let mut body = json!({
        "model": MODEL,
        "input": { "prompt": prompt },
        "parameters": {
            "resolution": resolution,
            "duration": duration,
            "ratio": if image.is_some() { "adaptive" } else { aspect },
        },
    });
    if let Some(url) = image {
        body["input"]["media"] = json!([{ "type": "first_frame", "url": url }]);
    }
    Ok(body)
}

fn http_error(status: u16, bytes: &[u8], key: &str) -> GenError {
    let doc: Value = serde_json::from_slice(bytes).unwrap_or(Value::Null);
    let code = doc.get("code").and_then(Value::as_str).unwrap_or("");
    let detail = doc.get("message").and_then(Value::as_str).unwrap_or("");
    let safe: String = redact_key(&format!("{code} {detail}"), key)
        .chars()
        .take(400)
        .collect();
    GenError::new(
        if status == 429 {
            GEN_RATE_LIMITED
        } else {
            GEN_BACKEND_ERROR
        },
        format!("阿里云 MiniMax HTTP {status}: {}", safe.trim()),
    )
}

fn api_json(
    method: &str,
    url: &str,
    key: &str,
    body: Option<&Value>,
    timeout: Duration,
) -> Result<Value> {
    let agent = ureq::AgentBuilder::new()
        .timeout(timeout)
        .redirects(0)
        .build();
    let mut request = agent
        .request(method, url)
        .set("Authorization", &format!("Bearer {key}"));
    if body.and_then(|b| b.pointer("/input/media")).and_then(Value::as_array)
        .is_some_and(|media| media.iter().any(|m| m.get("url").and_then(Value::as_str).is_some_and(is_private_upload_url))) {
        request = request.set("X-DashScope-OssResourceResolve", "enable");
    }
    let response = if let Some(body) = body {
        request
            .set("Content-Type", "application/json")
            .set("X-DashScope-Async", "enable")
            .send_string(&body.to_string())
    } else {
        request.call()
    };
    let response = match response {
        Ok(r) => r,
        Err(ureq::Error::Status(status, r)) => {
            return Err(http_error(status, &read_body(r).unwrap_or_default(), key))
        }
        Err(ureq::Error::Transport(_)) => return Err(failure("阿里云 MiniMax 请求连接失败或超时")),
    };
    if !(200..300).contains(&response.status()) {
        return Err(failure(format!(
            "阿里云 MiniMax 非预期 HTTP {}",
            response.status()
        )));
    }
    let bytes = read_body(response)?;
    let doc: Value =
        serde_json::from_slice(&bytes).map_err(|_| failure("阿里云 MiniMax 响应非 JSON"))?;
    if doc
        .get("code")
        .and_then(Value::as_str)
        .is_some_and(|c| !c.is_empty())
    {
        return Err(http_error(200, &bytes, key));
    }
    Ok(doc)
}

fn poll(
    endpoint: &str,
    task_id: &str,
    key: &str,
    interval: Duration,
    budget: Duration,
) -> Result<Value> {
    let deadline = Instant::now() + budget;
    let url = format!("{endpoint}{TASK_PATH}/{task_id}");
    loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Err(failure(format!("阿里云 MiniMax 任务 {task_id} 等待超时;任务可能仍在运行，请使用该任务 ID 查询，勿重复创建")));
        }
        let doc = match api_json("GET", &url, key, None, HTTP_TIMEOUT.min(remaining)) {
            Ok(doc) => doc,
            Err(err) if err.code == GEN_RATE_LIMITED => {
                // 查询可安全重试;绝不借查询失败重新创建付费任务。
                std::thread::sleep(
                    interval.min(deadline.saturating_duration_since(Instant::now())),
                );
                continue;
            }
            Err(err) => return Err(err),
        };
        match doc.pointer("/output/task_status").and_then(Value::as_str) {
            Some("SUCCEEDED") => return Ok(doc),
            Some("PENDING" | "RUNNING") => {}
            Some(status @ ("FAILED" | "CANCELED" | "UNKNOWN")) => {
                let code = doc
                    .pointer("/output/code")
                    .and_then(Value::as_str)
                    .unwrap_or("");
                let message = doc
                    .pointer("/output/message")
                    .and_then(Value::as_str)
                    .unwrap_or("");
                let detail: String = redact_key(&format!("{code} {message}"), key)
                    .chars()
                    .take(400)
                    .collect();
                return Err(failure(format!(
                    "阿里云 MiniMax 任务 {task_id} {status}: {}",
                    detail.trim()
                )));
            }
            _ => {
                return Err(failure(format!(
                    "阿里云 MiniMax 任务 {task_id} 返回无效任务状态"
                )))
            }
        }
        std::thread::sleep(interval.min(deadline.saturating_duration_since(Instant::now())));
    }
}

fn download_mp4(url: &str) -> Result<Vec<u8>> {
    if !is_http_url(url) {
        return Err(failure("阿里云 MiniMax 视频下载地址不是 HTTP(S) URL"));
    }
    // 下载地址为签名 URL,绝不附加供应商密钥。
    let response = ureq::AgentBuilder::new()
        .timeout(HTTP_TIMEOUT)
        .build()
        .get(url)
        .call()
        .map_err(|_| failure("阿里云 MiniMax 视频下载失败"))?;
    let bytes = read_body(response)?;
    if bytes.len() < 12 || &bytes[4..8] != b"ftyp" {
        return Err(failure(
            "阿里云 MiniMax 下载产物不是 MP4（缺少 ftyp 文件头）",
        ));
    }
    Ok(bytes)
}

fn generate_with_policy(
    req: &MediaRequest,
    cfg: &GenConfig,
    keys: &Keystore,
    interval: Duration,
    budget: Duration,
) -> Result<Vec<MediaArtifact>> {
    // 参数验证先于网络。模型从通用配置取得,但不接受误填到该适配器的其他模型。
    let entry = cfg.entry(ALIYUN_MINIMAX_VIDEO_ID);
    let body = build_body(req, entry.and_then(|e| e.model.as_deref()))?;
    let (endpoint, key, _) = remote_conn(ALIYUN_MINIMAX_VIDEO_ID, cfg, keys)?;
    if !is_http_url(&endpoint) || endpoint.contains('?') || endpoint.contains('#') {
        return Err(bad("阿里云 MiniMax endpoint 须为业务空间 HTTP(S) 根地址"));
    }
    // 兼容控制台给出的 /api/v1 BaseURL,避免重复拼接。
    let endpoint = endpoint.strip_suffix("/api/v1").unwrap_or(&endpoint);
    let created = api_json(
        "POST",
        &format!("{endpoint}{CREATE_PATH}"),
        &key,
        Some(&body),
        HTTP_TIMEOUT,
    )?;
    let task_id = created
        .pointer("/output/task_id")
        .and_then(Value::as_str)
        .filter(|s| {
            !s.is_empty()
                && s.bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
        })
        .ok_or_else(|| failure("阿里云 MiniMax 创建响应缺少有效 task_id"))?;
    // 只创建一次;后续查询或下载失败不会自动再次提交付费任务。
    let result = (|| {
        let completed = poll(endpoint, task_id, &key, interval, budget)?;
        let url = completed
            .pointer("/output/video_url")
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
            .ok_or_else(|| failure("阿里云 MiniMax 任务成功但缺少 video_url"))?;
        let bytes = download_mp4(url)?;
        Ok(vec![MediaArtifact::plain(
            bytes,
            "mp4",
            json!({
                "provider": ALIYUN_MINIMAX_VIDEO_ID,
                "model": MODEL,
                "taskId": task_id,
                "requestId": completed.get("request_id"),
                "mode": if body["input"].get("media").is_some() { "image2video" } else { "text2video" },
                "aspect": body["parameters"]["ratio"],
                "resolution": body["parameters"]["resolution"],
                "durationSec": body["parameters"]["duration"],
                "usage": completed.get("usage"),
            }),
        )])
    })();
    result.map_err(|err: GenError| {
        GenError::new(
            err.code,
            format!(
                "{};任务 ID: {task_id}，请查询现有任务，勿重复创建",
                redact_key(&err.message, &key),
            ),
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::BackendEntry;
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::sync::{Arc, Mutex};

    const KEY: &str = "unit-test-aliyun-key-never-leak";
    const MP4: &[u8] = b"\x00\x00\x00\x18ftypisom\x00\x00\x02\x00isommp42";
    #[derive(Clone)]
    struct Seen {
        method: String,
        path: String,
        headers: String,
        body: Value,
    }

    // 与 media.rs 多请求桩同形态,另保留 headers 以验证 API 鉴权/异步头与无密钥下载。
    fn server<F>(handler: F) -> (String, Arc<Mutex<Vec<Seen>>>)
    where
        F: Fn(&str, &Seen) -> (u16, Vec<u8>) + Send + 'static,
    {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let base_thread = base.clone();
        let seen = Arc::new(Mutex::new(Vec::new()));
        let seen_thread = Arc::clone(&seen);
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut stream) = stream else { break };
                stream
                    .set_read_timeout(Some(Duration::from_secs(5)))
                    .unwrap();
                let mut bytes = Vec::new();
                let mut chunk = [0; 4096];
                let (head_end, length) = loop {
                    if let Some(end) = bytes.windows(4).position(|w| w == b"\r\n\r\n") {
                        let head = String::from_utf8_lossy(&bytes[..end]);
                        let length = head
                            .lines()
                            .find_map(|line| {
                                line.to_ascii_lowercase()
                                    .strip_prefix("content-length:")
                                    .and_then(|v| v.trim().parse::<usize>().ok())
                            })
                            .unwrap_or(0);
                        if bytes.len() >= end + 4 + length {
                            break (end, length);
                        }
                    }
                    let n = stream.read(&mut chunk).unwrap();
                    assert!(n > 0);
                    bytes.extend_from_slice(&chunk[..n]);
                };
                let headers = String::from_utf8_lossy(&bytes[..head_end]).into_owned();
                let mut first = headers.lines().next().unwrap().split_whitespace();
                let request = Seen {
                    method: first.next().unwrap().into(),
                    path: first.next().unwrap().into(),
                    body: serde_json::from_slice(&bytes[head_end + 4..head_end + 4 + length])
                        .unwrap_or(Value::Null),
                    headers,
                };
                seen_thread.lock().unwrap().push(request.clone());
                let (status, body) = handler(&base_thread, &request);
                write!(
                    stream,
                    "HTTP/1.1 {status} Test\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    body.len()
                )
                .unwrap();
                stream.write_all(&body).unwrap();
            }
        });
        (base, seen)
    }

    fn config(base: &str) -> GenConfig {
        GenConfig {
            backends: vec![BackendEntry {
                id: ALIYUN_MINIMAX_VIDEO_ID.into(),
                kind: "remote".into(),
                enabled: true,
                endpoint: Some(base.into()),
                model: Some(MODEL.into()),
            }],
        }
    }
    fn request(params: Value) -> MediaRequest {
        MediaRequest {
            kind: MediaKind::Video,
            prompt: "A still lake at dawn".into(),
            params,
        }
    }
    fn with_key(test: impl FnOnce(&Keystore)) {
        let _guard = crate::TEST_ENV_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let old = std::env::var_os("FORGE_GEN_API_KEY");
        struct Restore(Option<std::ffi::OsString>);
        impl Drop for Restore {
            fn drop(&mut self) {
                if let Some(value) = self.0.take() {
                    std::env::set_var("FORGE_GEN_API_KEY", value);
                } else {
                    std::env::remove_var("FORGE_GEN_API_KEY");
                }
            }
        }
        let _restore = Restore(old);
        std::env::set_var("FORGE_GEN_API_KEY", KEY);
        test(&Keystore::load_from(std::path::Path::new(
            "missing-aliyun-test-keystore.json",
        )));
    }

    #[test]
    fn aliyun_parameters_match_h3_without_resolution_upgrades() {
        let defaults = build_body(&request(json!({})), None).unwrap();
        assert_eq!(defaults["model"], MODEL);
        assert_eq!(
            defaults["parameters"],
            json!({"resolution":"768P","duration":5,"ratio":"16:9"})
        );
        assert!(defaults["input"].get("media").is_none());
        for (input, wire) in [
            ("768p", "768P"),
            ("768P", "768P"),
            ("2k", "2K"),
            ("2K", "2K"),
        ] {
            assert_eq!(
                build_body(&request(json!({"resolution":input})), None).unwrap()["parameters"]
                    ["resolution"],
                wire
            );
        }
        for params in [
            json!({"resolution":"720p"}),
            json!({"resolution":"1080p"}),
            json!({"durationSec":3}),
            json!({"durationSec":16}),
            json!({"durationSec":4.5}),
            json!({"aspect":"adaptive"}),
            json!({"imageDataUrl":"data:image/png;base64,eA=="}),
        ] {
            assert_eq!(
                build_body(&request(params), None).unwrap_err().code,
                GEN_BAD_PARAMS
            );
        }
        for duration in [4, 15] {
            assert_eq!(
                build_body(&request(json!({"durationSec":duration})), None).unwrap()["parameters"]
                    ["duration"],
                duration
            );
        }
        for aspect in ASPECTS {
            assert_eq!(
                build_body(&request(json!({"aspect":aspect})), None).unwrap()["parameters"]
                    ["ratio"],
                aspect
            );
        }
        let image = build_body(
            &request(json!({"imageDataUrl":"https://images.example/first.png", "aspect":"1:1"})),
            None,
        )
        .unwrap();
        assert_eq!(
            image["input"]["media"],
            json!([{"type":"first_frame","url":"https://images.example/first.png"}])
        );
        assert_eq!(image["parameters"]["ratio"], "adaptive");
        let mut long = request(json!({}));
        long.prompt = "图".repeat(7000);
        assert!(build_body(&long, None).is_ok());
        long.prompt.push('图');
        assert_eq!(build_body(&long, None).unwrap_err().code, GEN_BAD_PARAMS);
        long.prompt = "  ".into();
        assert_eq!(build_body(&long, None).unwrap_err().code, GEN_BAD_PARAMS);
        assert_eq!(
            build_body(&request(json!({})), Some("MiniMax/MiniMax-M3"))
                .unwrap_err()
                .code,
            GEN_BAD_PARAMS
        );
    }

    #[test]
    fn aliyun_roundtrip_creates_once_polls_and_downloads_without_key() {
        with_key(|keys| {
            let count = Arc::new(Mutex::new(0));
            let count_server = Arc::clone(&count);
            let (base, seen) = server(move |base, req| {
                match (req.method.as_str(), req.path.as_str()) {
                    ("POST", CREATE_PATH) => (
                        200,
                        br#"{"output":{"task_id":"t1","task_status":"PENDING"}}"#.to_vec(),
                    ),
                    ("GET", "/api/v1/tasks/t1") => {
                        let mut count = count_server.lock().unwrap();
                        *count += 1;
                        if *count == 1 {
                            return (
                                429,
                                br#"{"code":"Throttling","message":"retry query"}"#.to_vec(),
                            );
                        }
                        let doc = if *count == 2 {
                            json!({"output":{"task_status":"RUNNING"}})
                        } else {
                            json!({"output":{"task_status":"SUCCEEDED","video_url":format!("{base}/video.mp4")},"usage":{"output_seconds":5}})
                        };
                        (200, doc.to_string().into_bytes())
                    }
                    ("GET", "/video.mp4") => (200, MP4.to_vec()),
                    _ => (404, b"{}".to_vec()),
                }
            });
            let out = generate_with_policy(
                &request(json!({})),
                &config(&format!("{base}/api/v1/")),
                keys,
                Duration::from_millis(1),
                Duration::from_secs(3),
            )
            .unwrap();
            assert_eq!(out[0].bytes, MP4);
            assert_eq!(out[0].meta["taskId"], "t1");
            assert_eq!(out[0].meta["usage"]["output_seconds"], 5);
            assert!(!out[0].meta.to_string().contains(KEY));
            let calls = seen.lock().unwrap();
            assert_eq!(calls.iter().filter(|r| r.method == "POST").count(), 1);
            assert_eq!(
                calls
                    .iter()
                    .filter(|r| r.path == "/api/v1/tasks/t1")
                    .count(),
                3
            );
            let create = &calls[0];
            assert_eq!(create.body["parameters"]["duration"], 5);
            assert!(create
                .headers
                .to_ascii_lowercase()
                .contains("x-dashscope-async: enable"));
            for call in calls.iter().filter(|r| r.path != "/video.mp4") {
                assert!(call.headers.contains(&format!("Bearer {KEY}")));
                assert!(!call.body.to_string().contains(KEY));
            }
            assert!(!calls
                .last()
                .unwrap()
                .headers
                .to_ascii_lowercase()
                .contains("authorization:"));
        });
    }

    #[test]
    fn aliyun_private_reference_requires_authenticated_resolution_header() {
        with_key(|keys| {
            let (base, seen) = server(|base, req| {
                match (req.method.as_str(), req.path.as_str()) {
                    ("POST", CREATE_PATH) => (200, br#"{"output":{"task_id":"private-ref"}}"#.to_vec()),
                    ("GET", "/api/v1/tasks/private-ref") => (200, json!({"output":{"task_status":"SUCCEEDED","video_url":format!("{base}/video.mp4")}}).to_string().into_bytes()),
                    ("GET", "/video.mp4") => (200, MP4.to_vec()),
                    _ => (404, b"{}".to_vec()),
                }
            });
            let uri = "oss://dashscope-instant/account/date/character.png";
            let out = generate_with_policy(&request(json!({"imageDataUrl":uri})), &config(&base), keys,
                Duration::from_millis(1), Duration::from_secs(3)).unwrap();
            assert_eq!(out[0].meta["mode"], "image2video");
            let calls = seen.lock().unwrap();
            assert_eq!(calls[0].body["input"]["media"][0]["url"], uri);
            assert!(calls[0].headers.to_ascii_lowercase().contains("x-dashscope-ossresourceresolve: enable"));
            assert!(!calls.last().unwrap().headers.to_ascii_lowercase().contains("authorization:"));
            for invalid in ["oss://", "oss:///etc/file", "oss://bucket/../file", "oss://bucket/file?secret", "oss://bucket/my file"] {
                assert!(!is_private_upload_url(invalid));
            }
        });
    }

    #[test]
    fn aliyun_terminal_errors_and_timeout_do_not_resubmit() {
        with_key(|keys| {
            for status in ["FAILED", "CANCELED", "UNKNOWN"] {
                let (base, seen) = server(move |_base, req| {
                    let doc = if req.method == "POST" {
                        json!({"output":{"task_id":"task-failed"}})
                    } else {
                        json!({"output":{"task_status":status,"code":"InvalidParameter","message":format!("rejected {KEY}")}})
                    };
                    (200, doc.to_string().into_bytes())
                });
                let e = generate_with_policy(
                    &request(json!({})),
                    &config(&base),
                    keys,
                    Duration::from_millis(1),
                    Duration::from_secs(1),
                )
                .unwrap_err();
                assert!(e.message.contains(status));
                assert!(!e.message.contains(KEY));
                assert_eq!(
                    seen.lock()
                        .unwrap()
                        .iter()
                        .filter(|r| r.method == "POST")
                        .count(),
                    1
                );
            }
            let (base, seen) = server(|_base, req| {
                (
                    200,
                    if req.method == "POST" {
                        br#"{"output":{"task_id":"task-timeout"}}"#.to_vec()
                    } else {
                        br#"{"output":{"task_status":"PENDING"}}"#.to_vec()
                    },
                )
            });
            let e = generate_with_policy(
                &request(json!({})),
                &config(&base),
                keys,
                Duration::from_millis(1),
                Duration::from_millis(10),
            )
            .unwrap_err();
            assert!(e.message.contains("task-timeout") && e.message.contains("超时"));
            assert_eq!(
                seen.lock()
                    .unwrap()
                    .iter()
                    .filter(|r| r.method == "POST")
                    .count(),
                1
            );
        });
    }

    #[test]
    fn aliyun_http_and_artifact_errors_are_honest_and_redacted() {
        with_key(|keys| {
            for status in [400, 401, 429, 500] {
                let (base, _) = server(move |_base, _req| {
                    (
                        status,
                        json!({"code":"ProviderError","message":format!("reason {KEY}")})
                            .to_string()
                            .into_bytes(),
                    )
                });
                let e = generate_with_policy(
                    &request(json!({})),
                    &config(&base),
                    keys,
                    Duration::ZERO,
                    Duration::from_secs(1),
                )
                .unwrap_err();
                assert_eq!(
                    e.code,
                    if status == 429 {
                        GEN_RATE_LIMITED
                    } else {
                        GEN_BACKEND_ERROR
                    }
                );
                assert!(e.message.contains("reason"));
                assert!(!e.message.contains(KEY));
            }
            for kind in [
                "missing-url",
                "not-mp4",
                "bad-status",
                "missing-id",
                "bad-json",
                "poll-error",
                "download-error",
            ] {
                let (base, _) = server(move |base, req| {
                    if req.method == "POST" {
                        return (
                            200,
                            match kind {
                                "missing-id" => b"{}".to_vec(),
                                "bad-json" => b"not JSON".to_vec(),
                                _ => br#"{"output":{"task_id":"t1"}}"#.to_vec(),
                            },
                        );
                    }
                    if kind == "poll-error" {
                        return (503, b"{}".to_vec());
                    }
                    if req.path == "/video.mp4" && kind == "download-error" {
                        return (403, b"{}".to_vec());
                    }
                    if req.path == "/video.mp4" {
                        return (200, b"<html>upstream failure</html>".to_vec());
                    }
                    let doc = match kind {
                        "missing-url" => json!({"output":{"task_status":"SUCCEEDED"}}),
                        "bad-status" => json!({"output":{"task_status":"NEW_UNKNOWN_VALUE"}}),
                        _ => {
                            json!({"output":{"task_status":"SUCCEEDED","video_url":format!("{base}/video.mp4")}})
                        }
                    };
                    (200, doc.to_string().into_bytes())
                });
                let e = generate_with_policy(
                    &request(json!({})),
                    &config(&base),
                    keys,
                    Duration::ZERO,
                    Duration::from_secs(1),
                )
                .unwrap_err();
                assert_eq!(e.code, GEN_BACKEND_ERROR, "{kind}");
                if kind != "missing-id" && kind != "bad-json" {
                    assert!(
                        e.message.contains("任务 ID: t1") && e.message.contains("勿重复创建"),
                        "{kind}: {}",
                        e.message
                    );
                }
            }
        });
    }
}
