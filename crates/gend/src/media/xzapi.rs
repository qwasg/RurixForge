//! StarFrame / xzapi H3: upload a local reference, create once, poll, download.
//! Protocol: https://docs.xzapi.vip/index.html

use super::{redact_key, remote_configured, remote_conn, MediaArtifact, MediaBackend, MediaKind, MediaRequest};
use crate::config::{data_dir, GenConfig};
use crate::keystore::Keystore;
use crate::{GenError, Result, GEN_BACKEND_ERROR, GEN_BACKEND_NOT_CONFIGURED, GEN_BAD_PARAMS, GEN_RATE_LIMITED};
use base64::Engine;
use serde_json::{json, Value};
use std::io::{Read, Write};
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

pub const XZAPI_VIDEO_ID: &str = "xzapi-video";
const MODEL: &str = "ch1007-minimax-h3-2k";
const UPLOAD_KEY_ID: &str = "xzapi-upload-sign";
const UPLOAD_ENDPOINT: &str = "https://web.xzapi.vip/api/upload-sign";
const IMAGE_LIMIT: usize = 20 * 1024 * 1024;
const JSON_LIMIT: usize = 2 * 1024 * 1024;
const VIDEO_LIMIT: usize = 512 * 1024 * 1024;
static SEQUENCE: AtomicU64 = AtomicU64::new(0);

pub struct XzapiVideo;

impl MediaBackend for XzapiVideo {
    fn id(&self) -> &str { XZAPI_VIDEO_ID }
    fn kind(&self) -> &str { "remote" }
    fn configured(&self, cfg: &GenConfig, keys: &Keystore) -> bool {
        remote_configured(self.id(), cfg, keys)
    }
    fn capabilities(&self) -> Value {
        json!({
            "kinds": ["text2video", "image2video"],
            "aspects": ["16:9", "9:16"],
            "resolutions": ["2k"], "defaultResolution": "2k",
            "minDurationSec": 15, "maxDurationSec": 15,
            "minPromptChars": 10, "maxPromptChars": 5000,
            "maxBatch": 1, "formats": ["mp4"],
            "imageInputTypes": ["url", "data"],
            "defaultEndpoint": "https://api.xzapi.vip", "defaultModel": MODEL,
        })
    }
    fn generate(&self, req: &MediaRequest, cfg: &GenConfig, keys: &Keystore) -> Result<Vec<MediaArtifact>> {
        generate(req, cfg, keys, UPLOAD_ENDPOINT, Duration::from_secs(10),
            Duration::from_secs(1800), &data_dir().join("xzapi-tasks"))
    }
}

fn bad(message: impl Into<String>) -> GenError { GenError::new(GEN_BAD_PARAMS, message) }
fn failure(message: impl Into<String>) -> GenError { GenError::new(GEN_BACKEND_ERROR, message) }

fn http_url(value: &str) -> bool {
    let Some(rest) = value.strip_prefix("https://").or_else(|| value.strip_prefix("http://")) else { return false };
    let authority = rest.split(['/', '?', '#']).next().unwrap_or("");
    !authority.is_empty() && !authority.contains('@') && !value.chars().any(char::is_whitespace)
}

fn client_id() -> String {
    let nanos = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_nanos();
    format!("forge-{nanos:x}-{:x}-{:x}", std::process::id(), SEQUENCE.fetch_add(1, Ordering::Relaxed))
}

fn body(req: &MediaRequest, model: Option<&str>, client_id: &str) -> Result<Value> {
    if req.kind != MediaKind::Video { return Err(bad("xzapi H3 only supports video generation")); }
    let prompt = req.prompt.trim();
    if !(10..=5000).contains(&prompt.chars().count()) {
        return Err(bad("xzapi H3 prompt must contain 10..=5000 characters"));
    }
    if model.is_some_and(|m| !m.is_empty() && m != MODEL) {
        return Err(bad(format!("xzapi H3 model must be {MODEL}")));
    }
    let aspect = req.params.get("aspect").and_then(Value::as_str).unwrap_or("16:9");
    if !["16:9", "9:16"].contains(&aspect) { return Err(bad("xzapi H3 aspect must be 16:9 or 9:16")); }
    if req.params.get("durationSec").is_some_and(|v| v.as_u64() != Some(15)) {
        return Err(bad("xzapi H3 durationSec must be 15"));
    }
    if req.params.get("resolution").is_some_and(|v| !v.as_str().is_some_and(|s| s.eq_ignore_ascii_case("2k"))) {
        return Err(bad("xzapi H3 resolution must be 2k"));
    }
    let mut body = json!({"model": MODEL, "prompt": prompt, "mode": "references",
        "aspect_ratio": aspect, "duration": 15, "resolution": "2k", "client_task_id": client_id});
    if let Some(image) = req.params.get("imageDataUrl").or_else(|| req.params.get("imageUrl")) {
        let image = image.as_str().ok_or_else(|| bad("xzapi reference must be a URL or image data URI"))?.trim();
        if !image.is_empty() {
            if !http_url(image) && !image.starts_with("data:image/") {
                return Err(bad("xzapi reference must be HTTP(S) or an image data URI"));
            }
            body["references"] = json!({"image": image});
        }
    }
    Ok(body)
}

fn read_limited(response: ureq::Response, limit: usize) -> Result<Vec<u8>> {
    let mut bytes = Vec::new();
    response.into_reader().take(limit as u64 + 1).read_to_end(&mut bytes)
        .map_err(|_| failure("xzapi response read failed"))?;
    if bytes.len() > limit { return Err(failure("xzapi response exceeds size limit")); }
    Ok(bytes)
}

fn response(result: std::result::Result<ureq::Response, ureq::Error>, secret: &str) -> Result<ureq::Response> {
    match result {
        Ok(r) => Ok(r),
        Err(ureq::Error::Status(status, r)) => {
            let doc = read_limited(r, JSON_LIMIT).ok().and_then(|b| serde_json::from_slice::<Value>(&b).ok());
            let message = doc.as_ref().and_then(|v| v.pointer("/error/message").or_else(|| v.get("message")))
                .and_then(Value::as_str).unwrap_or("request rejected");
            Err(GenError::new(if status == 429 { GEN_RATE_LIMITED } else { GEN_BACKEND_ERROR },
                format!("xzapi HTTP {status}: {}", redact_key(message, secret).chars().take(600).collect::<String>())))
        }
        Err(ureq::Error::Transport(t)) => Err(failure(format!("xzapi network request failed ({:?}); no automatic resubmission", t.kind()))),
    }
}

fn agent() -> ureq::Agent {
    // Never forward API credentials through a redirect. Downloads handle Location separately.
    ureq::AgentBuilder::new().timeout(Duration::from_secs(120))
        .timeout_connect(Duration::from_secs(15)).try_proxy_from_env(true).redirects(0).build()
}

fn api_json(method: &str, url: &str, header: &str, secret: &str, body: Option<&Value>) -> Result<Value> {
    let request = agent().request(method, url).set(header, secret).set("Content-Type", "application/json");
    let result = match body { Some(b) => request.send_string(&b.to_string()), None => request.call() };
    let bytes = read_limited(response(result, secret.strip_prefix("Bearer ").unwrap_or(secret))?, JSON_LIMIT)?;
    serde_json::from_slice(&bytes).map_err(|_| failure("xzapi returned invalid JSON"))
}

/// Optional, host-scoped direct storage route for Windows machines using a global VPN.
/// Keep API traffic and the user's global proxy settings unchanged. Resolve the current
/// interface address on every transfer so DHCP changes do not leave a stale bind address.
fn storage_bytes(method: &str, url: &str, mime: &str, payload: &[u8], limit: usize) -> Result<Vec<u8>> {
    let host = url.strip_prefix("https://").and_then(|s| s.split('/').next()).unwrap_or("");
    let network = std::fs::read(data_dir().join("xzapi-network.json")).ok()
        .and_then(|b| serde_json::from_slice::<Value>(&b).ok());
    let interface = network.as_ref().and_then(|v| v.get("storageInterface")).and_then(Value::as_str);
    if cfg!(windows) && host == "starframe-sh.tos-s3-cn-shanghai.volces.com" {
        if let Some(interface) = interface.filter(|s| !s.trim().is_empty()) {
            return direct_storage(method, url, host, interface, mime, payload, limit);
        }
    }
    let request = agent().request(method, url).set("Content-Type", mime);
    let result = if method == "PUT" { request.send_bytes(payload) } else { request.call() };
    read_limited(response(result, "")?, limit)
}

fn hidden_command(program: impl AsRef<std::ffi::OsStr>) -> std::process::Command {
    let command = std::process::Command::new(program);
    #[cfg(windows)]
    let command = {
        use std::os::windows::process::CommandExt;
        let mut command = command;
        command.creation_flags(0x08000000);
        command
    };
    command
}

fn curl_quote(value: &str) -> String {
    format!("\"{}\"", value.replace('\\', "\\\\").replace('"', "\\\""))
}

fn direct_storage(method: &str, url: &str, host: &str, interface: &str,
    mime: &str, payload: &[u8], limit: usize) -> Result<Vec<u8>> {
    use std::process::Stdio;
    let address = hidden_command("powershell.exe").args(["-NoProfile", "-NonInteractive", "-Command",
        "(Get-NetIPAddress -InterfaceAlias $env:FORGE_XZAPI_INTERFACE -AddressFamily IPv4 -ErrorAction Stop | Where-Object {$_.AddressState -eq 'Preferred'} | Select-Object -First 1).IPAddress"])
        .env("FORGE_XZAPI_INTERFACE", interface).output()
        .map_err(|_| failure("xzapi could not inspect the configured storage network interface"))?;
    let local = String::from_utf8_lossy(&address.stdout).trim().to_string();
    if !address.status.success() || local.parse::<std::net::Ipv4Addr>().is_err() {
        return Err(failure("xzapi storageInterface has no usable IPv4 address"));
    }
    // The TUN resolver supplies synthetic 198.18.x.x addresses. Preserve the original
    // hostname for both TLS verification and the storage request signature.
    let dns = api_json("GET", &format!("https://cloudflare-dns.com/dns-query?name={host}&type=A"),
        "Accept", "application/dns-json", None)?;
    let ips: Vec<String> = dns.get("Answer").and_then(Value::as_array).into_iter().flatten()
        .filter(|v| v["type"] == 1).filter_map(|v| v["data"].as_str())
        .filter(|ip| ip.parse::<std::net::Ipv4Addr>().is_ok()).map(str::to_owned).collect();
    if ips.is_empty() { return Err(failure("xzapi storage DNS returned no IPv4 addresses")); }
    let dir = data_dir().join("xzapi-transfers");
    std::fs::create_dir_all(&dir)?;
    let id = client_id();
    let input = dir.join(format!("{id}.input"));
    let output = dir.join(format!("{id}.output"));
    struct TempFiles(Vec<std::path::PathBuf>);
    impl Drop for TempFiles { fn drop(&mut self) { for p in &self.0 { let _ = std::fs::remove_file(p); } } }
    let _cleanup = TempFiles(vec![input.clone(), output.clone()]);
    if method == "PUT" {
        std::fs::OpenOptions::new().create_new(true).write(true).open(&input)?.write_all(payload)?;
    }
    let mut config = format!("url = {}\nrequest = {}\ninterface = {}\nnoproxy = \"*\"\nresolve = {}\noutput = {}\nmax-time = 120\nconnect-timeout = 10\nmax-filesize = {}\nwrite-out = \"%{{http_code}}\"\n",
        curl_quote(url), curl_quote(method), curl_quote(&local),
        curl_quote(&format!("{host}:443:{}", ips.join(","))), curl_quote(&output.to_string_lossy()), limit);
    if method == "PUT" {
        config.push_str(&format!("header = {}\nupload-file = {}\n", curl_quote(&format!("Content-Type: {mime}")), curl_quote(&input.to_string_lossy())));
    }
    let curl = std::path::PathBuf::from(std::env::var_os("SystemRoot").unwrap_or_else(|| "C:\\Windows".into()))
        .join("System32").join("curl.exe");
    // Signed URLs stay on stdin, never in command-line arguments or logs.
    let mut child = hidden_command(curl).args(["--silent", "--config", "-"])
        .stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn()
        .map_err(|_| failure("xzapi direct storage transfer requires Windows curl.exe"))?;
    child.stdin.take().ok_or_else(|| failure("xzapi storage pipe unavailable"))?.write_all(config.as_bytes())?;
    let result = child.wait_with_output().map_err(|_| failure("xzapi direct storage transfer failed"))?;
    let status = String::from_utf8_lossy(&result.stdout).trim().parse::<u16>().unwrap_or(0);
    if !result.status.success() || !(200..300).contains(&status) {
        return Err(failure(format!("xzapi direct storage {method} failed (HTTP {status}, exit {:?})", result.status.code())));
    }
    let mut bytes = Vec::new();
    std::fs::File::open(&output)?.take(limit as u64 + 1).read_to_end(&mut bytes)?;
    if bytes.len() > limit { return Err(failure("xzapi storage response exceeds size limit")); }
    Ok(bytes)
}

fn upload(image: &str, signer: &str, token: &str) -> Result<String> {
    let (prefix, encoded) = image.split_once(",").ok_or_else(|| bad("invalid reference data URI"))?;
    let (mime, ext) = match prefix {
        "data:image/png;base64" => ("image/png", "png"),
        "data:image/jpeg;base64" | "data:image/jpg;base64" => ("image/jpeg", "jpg"),
        _ => return Err(bad("local xzapi references must be PNG or JPEG")),
    };
    if encoded.len() > (IMAGE_LIMIT + 2) / 3 * 4 { return Err(bad("reference image exceeds 20 MiB")); }
    let bytes = base64::engine::general_purpose::STANDARD.decode(encoded).map_err(|_| bad("invalid image base64"))?;
    let format = image::guess_format(&bytes).map_err(|_| bad("reference is not an image"))?;
    if bytes.is_empty() || bytes.len() > IMAGE_LIMIT || !matches!((ext, format),
        ("png", image::ImageFormat::Png) | ("jpg", image::ImageFormat::Jpeg)) {
        return Err(bad("invalid reference image format or size"));
    }
    let signed = api_json("POST", signer, "X-Upload-Sign-Token", token, Some(&json!({
        "file_name": format!("{}.{ext}", client_id()), "content_type": mime, "file_size": bytes.len(),
    }))).map_err(|e| GenError::new(e.code, format!("reference signing: {}", e.message)))?;
    let upload_url = signed.get("upload_url").and_then(Value::as_str).filter(|s| http_url(s))
        .ok_or_else(|| failure("xzapi upload signature has no valid upload_url"))?;
    let public_url = signed.get("file_url").and_then(Value::as_str).filter(|s| http_url(s))
        .ok_or_else(|| failure("xzapi upload signature has no valid file_url"))?;
    storage_bytes("PUT", upload_url, mime, &bytes, JSON_LIMIT)
        .map_err(|e| GenError::new(e.code, format!("reference PUT upload: {}", e.message)))?;
    // Verify the uploaded object is accessible and contains the exact submitted bytes.
    let downloaded = storage_bytes("GET", public_url, mime, &[], IMAGE_LIMIT)
        .map_err(|e| GenError::new(e.code, format!("reference read-back: {}", e.message)))?;
    if downloaded != bytes { return Err(failure("xzapi uploaded reference failed read-back verification")); }
    Ok(public_url.to_owned())
}

fn download(endpoint: &str, task_id: &str, key: &str) -> Result<Vec<u8>> {
    let url = format!("{endpoint}/v1/videos/{task_id}/content");
    let r = response(agent().get(&url).set("Authorization", &format!("Bearer {key}")).call(), key)?;
    let bytes = if (300..400).contains(&r.status()) {
        let location = r.header("Location").filter(|s| http_url(s))
            .ok_or_else(|| failure("xzapi content redirect has no valid public URL"))?;
        // Signed storage/CDN URLs receive no API key.
        storage_bytes("GET", location, "video/mp4", &[], VIDEO_LIMIT)?
    } else { read_limited(r, VIDEO_LIMIT)? };
    if bytes.len() < 12 || &bytes[4..8] != b"ftyp" {
        return Err(failure("xzapi downloaded content is not an MP4"));
    }
    Ok(bytes)
}

fn save_receipt(path: &Path, receipt: &Value) -> Result<()> {
    std::fs::write(path, serde_json::to_vec_pretty(receipt).map_err(|_| failure("receipt encoding failed"))?)?;
    Ok(())
}

fn generate(req: &MediaRequest, cfg: &GenConfig, keys: &Keystore, signer: &str,
    interval: Duration, budget: Duration, receipt_dir: &Path) -> Result<Vec<MediaArtifact>> {
    let id = client_id();
    let mut request = body(req, cfg.entry(XZAPI_VIDEO_ID).and_then(|e| e.model.as_deref()), &id)?;
    let (endpoint, key, _) = remote_conn(XZAPI_VIDEO_ID, cfg, keys)?;
    let endpoint = endpoint.strip_suffix("/v1").unwrap_or(&endpoint);
    if !http_url(endpoint) || endpoint.contains(['?', '#']) {
        return Err(bad("xzapi endpoint must be an HTTP(S) API base URL"));
    }
    let mut uploaded = false;
    if let Some(image) = request.pointer("/references/image").and_then(Value::as_str) {
        if image.starts_with("data:") {
            let token = keys.key_for(UPLOAD_KEY_ID).ok_or_else(|| GenError::new(GEN_BACKEND_NOT_CONFIGURED,
                "xzapi local image upload is not configured (xzapi-upload-sign)"))?;
            request["references"]["image"] = json!(upload(image, signer, &token)?);
            uploaded = true;
        }
    }
    let mode = if request.get("references").is_some() { "image2video" } else { "text2video" };
    std::fs::create_dir_all(receipt_dir)?;
    let receipt_path = receipt_dir.join(format!("{id}.json"));
    let mut receipt = json!({"provider": XZAPI_VIDEO_ID, "clientTaskId": id, "model": MODEL,
        "mode": mode, "imageUploaded": uploaded, "status": "submitting", "createdAt": crate::timeutil::utc_now_iso8601()});
    // Persist before the only paid POST. Never delete receipts or retry task creation.
    std::fs::OpenOptions::new().write(true).create_new(true).open(&receipt_path)?
        .write_all(receipt.to_string().as_bytes())?;
    let result = (|| {
        let auth = format!("Bearer {key}");
        let created = api_json("POST", &format!("{endpoint}/v1/videos"), "Authorization", &auth, Some(&request))?;
        let task_id = created.get("id").and_then(Value::as_str).filter(|s| !s.is_empty()
            && s.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_'))
            .ok_or_else(|| failure("xzapi create response has no valid task ID"))?.to_owned();
        receipt["taskId"] = json!(task_id);
        receipt["status"] = created["status"].clone();
        save_receipt(&receipt_path, &receipt)?;
        let started = Instant::now();
        let mut task = created;
        loop {
            let status = task.get("status").and_then(Value::as_str).unwrap_or("");
            receipt["status"] = json!(status);
            receipt["progress"] = task["progress"].clone();
            save_receipt(&receipt_path, &receipt)?;
            match status {
                "completed" => break,
                "failed" | "cancelled" | "canceled" => {
                    let reason = task.pointer("/metadata/fail_reason").and_then(Value::as_str).unwrap_or("generation failed");
                    return Err(failure(format!("xzapi task failed: {}", redact_key(reason, &key))));
                }
                "queued" | "pending" | "processing" | "in_progress" => (),
                _ => return Err(failure("xzapi task returned an unknown status; query the existing task")),
            }
            if started.elapsed() >= budget { return Err(failure("xzapi polling timed out; task may still be running")); }
            std::thread::sleep(interval);
            task = api_json("GET", &format!("{endpoint}/v1/videos/{task_id}"), "Authorization", &auth, None)?;
        }
        let bytes = download(endpoint, &task_id, &key)?;
        receipt["downloadedBytes"] = json!(bytes.len());
        receipt["completedAt"] = json!(crate::timeutil::utc_now_iso8601());
        save_receipt(&receipt_path, &receipt)?;
        Ok(vec![MediaArtifact::plain(bytes, "mp4", json!({"provider": XZAPI_VIDEO_ID,
            "model": MODEL, "taskId": task_id, "clientTaskId": id, "mode": mode,
            "imageUploaded": uploaded, "aspect": request["aspect_ratio"], "resolution": "2k", "durationSec": 15}))])
    })();
    result.map_err(|error: GenError| {
        receipt["error"] = json!(redact_key(&error.message, &key));
        let _ = save_receipt(&receipt_path, &receipt);
        GenError::new(error.code, format!("{}; client_task_id: {id}; task_id: {}; receipt: {}; do not create a new task",
            redact_key(&error.message, &key), receipt["taskId"].as_str().unwrap_or("unknown"), receipt_path.display()))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::BackendEntry;
    use std::net::TcpListener;
    use std::sync::{Arc, Mutex};

    #[test]
    #[ignore = "Live upload only; requires an explicitly selected reference and configured credentials"]
    fn live_reference_upload_probe() {
        let _guard = crate::TEST_ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let path = std::env::var("FORGE_XZAPI_PROBE_IMAGE").expect("set FORGE_XZAPI_PROBE_IMAGE");
        let bytes = std::fs::read(path).unwrap();
        let data = format!("data:image/png;base64,{}", base64::engine::general_purpose::STANDARD.encode(bytes));
        let key = Keystore::load().key_for(UPLOAD_KEY_ID).expect("upload signing key");
        let started = Instant::now();
        match upload(&data, UPLOAD_ENDPOINT, &key) {
            Ok(_) => eprintln!("Reference upload and byte-for-byte read-back passed in {:?}", started.elapsed()),
            Err(e) => panic!("{} (elapsed {:?})", e.message, started.elapsed()),
        }
    }

    #[test]
    fn rejects_wrong_h3_channel_parameters_before_network() {
        for params in [json!({"durationSec": 5}), json!({"resolution": "720p"}), json!({"aspect": "1:1"})] {
            let req = MediaRequest {kind: MediaKind::Video, prompt: "Animate the reference image".into(), params};
            assert_eq!(body(&req, None, "test").unwrap_err().code, GEN_BAD_PARAMS);
        }
    }

    #[test]
    fn local_reference_upload_create_poll_download_and_failure_receipt() {
        let _guard = crate::TEST_ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let old = std::env::var_os("FORGE_GEN_API_KEY");
        std::env::set_var("FORGE_GEN_API_KEY", "xzapi-test-key");
        struct Restore(Option<std::ffi::OsString>);
        impl Drop for Restore { fn drop(&mut self) {
            match &self.0 { Some(v) => std::env::set_var("FORGE_GEN_API_KEY", v), None => std::env::remove_var("FORGE_GEN_API_KEY") }
        }}
        let _restore = Restore(old);
        for fail in [false, true] {
            let listener = TcpListener::bind("127.0.0.1:0").unwrap();
            let endpoint = format!("http://{}", listener.local_addr().unwrap());
            let base = endpoint.clone();
            let calls = Arc::new(Mutex::new(Vec::<(String, String, Vec<u8>)>::new()));
            let seen = calls.clone();
            let png = b"\x89PNG\r\n\x1a\nreference".to_vec();
            let image = png.clone();
            let mp4 = b"\0\0\0\x18ftypisomtest-video".to_vec();
            let video = mp4.clone();
            std::thread::spawn(move || {
                for stream in listener.incoming() {
                    let mut stream = stream.unwrap();
                    stream.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
                    let mut raw = Vec::new();
                    let mut chunk = [0; 4096];
                    let (end, len) = loop {
                        if let Some(end) = raw.windows(4).position(|w| w == b"\r\n\r\n") {
                            let head = String::from_utf8_lossy(&raw[..end]);
                            let len = head.lines().find_map(|l| l.to_ascii_lowercase().strip_prefix("content-length:")
                                .and_then(|s| s.trim().parse::<usize>().ok())).unwrap_or(0);
                            if raw.len() >= end + 4 + len { break (end, len); }
                        }
                        let n = stream.read(&mut chunk).unwrap();
                        if n == 0 { return; }
                        raw.extend_from_slice(&chunk[..n]);
                    };
                    let head = String::from_utf8_lossy(&raw[..end]).into_owned();
                    let path = head.lines().next().unwrap().split_whitespace().nth(1).unwrap().to_string();
                    let payload = raw[end + 4..end + 4 + len].to_vec();
                    seen.lock().unwrap().push((path.clone(), head.clone(), payload));
                    let bytes = match path.as_str() {
                        "/sign" => { assert!(head.contains("X-Upload-Sign-Token: xzapi-test-key"));
                            json!({"upload_url": format!("{base}/put"), "file_url": format!("{base}/image")}).to_string().into_bytes() },
                        "/put" | "/image" => { assert!(!head.to_lowercase().contains("authorization:")); image.clone() },
                        "/v1/videos" => { assert!(head.contains("Authorization: Bearer xzapi-test-key"));
                            json!({"id": "task_test", "status": "queued"}).to_string().into_bytes() },
                        "/v1/videos/task_test" => json!({"status": if fail {"failed"} else {"completed"},
                            "metadata": {"fail_reason": "rejected xzapi-test-key"}}).to_string().into_bytes(),
                        "/v1/videos/task_test/content" => video.clone(),
                        _ => panic!("unexpected path {path}"),
                    };
                    write!(stream, "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", bytes.len()).unwrap();
                    stream.write_all(&bytes).unwrap();
                }
            });
            let cfg = GenConfig { backends: vec![BackendEntry {id: XZAPI_VIDEO_ID.into(), kind: "remote".into(),
                enabled: true, endpoint: Some(endpoint.clone()), model: Some(MODEL.into())}] };
            let req = MediaRequest {kind: MediaKind::Video, prompt: "Animate the reference image".into(),
                params: json!({"imageDataUrl": format!("data:image/png;base64,{}", base64::engine::general_purpose::STANDARD.encode(png))})};
            let dir = std::env::temp_dir().join(client_id());
            let result = generate(&req, &cfg, &Keystore::load(), &format!("{endpoint}/sign"), Duration::ZERO, Duration::from_secs(2), &dir);
            if fail {
                let e = result.unwrap_err();
                assert!(e.message.contains("task_test"));
                assert!(!e.message.contains("xzapi-test-key"));
            } else {
                let out = result.unwrap();
                assert_eq!(out[0].bytes, mp4);
                assert_eq!(out[0].meta["imageUploaded"], true);
                assert_eq!(out[0].meta["mode"], "image2video");
            }
            let seen = calls.lock().unwrap();
            let posts: Vec<_> = seen.iter().filter(|(p, _, _)| p == "/v1/videos").collect();
            assert_eq!(posts.len(), 1);
            let submitted: Value = serde_json::from_slice(&posts[0].2).unwrap();
            assert_eq!(submitted["references"]["image"], format!("{endpoint}/image"));
            assert_eq!(submitted["duration"], 15);
            let file = std::fs::read_dir(&dir).unwrap().next().unwrap().unwrap().path();
            let receipt = std::fs::read_to_string(file).unwrap();
            assert!(receipt.contains("task_test"));
            assert!(!receipt.contains("xzapi-test-key"));
        }
    }
}
