//! Per-thread MCP transport for the host-owned collaboration service.
//!
//! This process never opens a second agent store. Its opaque capability binds all
//! requests to the participant registered in the running Forge daemon.

use serde_json::{json, Value};
use std::io::{BufRead, Write};
use std::sync::{Arc, Mutex};
use std::time::Duration;

pub const SERVER_NAME: &str = "forge-collaboration";
pub const ENDPOINT_ENV: &str = "FORGE_COLLABORATION_ENDPOINT";
pub const TOKEN_ENV: &str = "FORGE_COLLABORATION_TOKEN";
const MAX_FRAME_BYTES: usize = 1024 * 1024;

fn endpoint(value: &str) -> Result<String, String> {
    let parsed: std::net::SocketAddr = value
        .strip_prefix("http://")
        .unwrap_or(value)
        .trim_end_matches('/')
        .parse()
        .map_err(|_| "协作服务地址必须是回环 IP 与端口".to_string())?;
    if !parsed.ip().is_loopback() || parsed.port() == 0 {
        return Err("协作服务地址必须是有效的回环地址".into());
    }
    Ok(format!("http://{parsed}"))
}

fn request_frame(reader: &mut impl BufRead) -> Result<Option<Value>, String> {
    let mut bytes = Vec::new();
    loop {
        let chunk = reader.fill_buf().map_err(|_| "MCP_READ_FAILED")?;
        if chunk.is_empty() {
            return if bytes.is_empty() {
                Ok(None)
            } else {
                Err("MCP_PARTIAL_FRAME".into())
            };
        }
        let length = chunk
            .iter()
            .position(|b| *b == b'\n')
            .map_or(chunk.len(), |n| n + 1);
        if bytes.len().saturating_add(length) > MAX_FRAME_BYTES {
            return Err("MCP_FRAME_TOO_LARGE".into());
        }
        let complete = chunk[length - 1] == b'\n';
        bytes.extend_from_slice(&chunk[..length]);
        reader.consume(length);
        if complete {
            if bytes.iter().all(u8::is_ascii_whitespace) {
                bytes.clear();
                continue;
            }
            return serde_json::from_slice(&bytes)
                .map(Some)
                .map_err(|_| "MCP_INVALID_JSON".into());
        }
    }
}

fn forward(base: &str, token: &str, method: &str, params: Value) -> Result<Value, String> {
    let suffix = match method {
        "tools/list" => "list",
        "tools/call" => "call",
        _ => return Err("MCP_METHOD_NOT_FOUND".into()),
    };
    let response = ureq::AgentBuilder::new()
        .timeout(Duration::from_secs(2100))
        .try_proxy_from_env(false)
        .redirects(0)
        .build()
        .post(&format!("{base}/api/forge/collaboration/tools/{suffix}"))
        .set("Authorization", &format!("Bearer {token}"))
        .set("Content-Type", "application/json")
        .send_string(&params.to_string())
        // Avoid exposing a capability, HTTP payload, or local environment in errors.
        .map_err(|error| match error {
            ureq::Error::Status(code, _) => format!("COLLABORATION_HOST_HTTP_{code}"),
            _ => "COLLABORATION_HOST_UNAVAILABLE".into(),
        })?;
    use std::io::Read;
    let mut bytes = Vec::new();
    response
        .into_reader()
        .take(MAX_FRAME_BYTES as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| "COLLABORATION_HOST_READ_FAILED")?;
    if bytes.len() > MAX_FRAME_BYTES {
        return Err("COLLABORATION_HOST_RESPONSE_TOO_LARGE".into());
    }
    serde_json::from_slice(&bytes).map_err(|_| "COLLABORATION_HOST_INVALID_JSON".into())
}

fn reply(id: Value, result: Result<Value, String>) -> Value {
    match result {
        Ok(value) => json!({"jsonrpc":"2.0","id":id,"result":value}),
        Err(message) => json!({"jsonrpc":"2.0","id":id,"error":{"code":-32000,"message":message}}),
    }
}

pub async fn run_stdio() {
    let configuration = std::env::var(ENDPOINT_ENV)
        .ok()
        .and_then(|value| endpoint(&value).ok())
        .zip(
            std::env::var(TOKEN_ENV)
                .ok()
                .filter(|value| !value.trim().is_empty()),
        );
    let Some((base, token)) = configuration else {
        eprintln!("COLLABORATION_BRIDGE_CONFIG_INVALID");
        return;
    };
    let output = Arc::new(Mutex::new(std::io::stdout()));
    let (send, mut receive) = tokio::sync::mpsc::channel(32);
    std::thread::spawn(move || {
        let input = std::io::stdin();
        let mut input = input.lock();
        loop {
            match request_frame(&mut input) {
                Ok(Some(request)) => {
                    if send.blocking_send(request).is_err() {
                        break;
                    }
                }
                Ok(None) => break,
                Err(error) => {
                    eprintln!("{error}");
                    break;
                }
            }
        }
    });
    let mut jobs = tokio::task::JoinSet::new();
    let mut handled = std::collections::HashSet::new();
    while let Some(request) = receive.recv().await {
        let Some(id) = request.get("id").filter(|id| !id.is_null()).cloned() else {
            continue;
        };
        if !handled.insert(id.to_string()) {
            continue;
        }
        let method = request["method"].as_str().unwrap_or_default().to_string();
        let params = request.get("params").cloned().unwrap_or_else(|| json!({}));
        let base = base.clone();
        let token = token.clone();
        let output = output.clone();
        // A slow task must not hold stdin or stdout while another call sends a
        // message. Only serialization of one complete response takes this lock.
        jobs.spawn(async move {
            let result = match method.as_str() {
                "initialize" => Ok(
                    json!({"protocolVersion":"2024-11-05","capabilities":{"tools":{}},
                    "serverInfo":{"name":SERVER_NAME,"version":env!("CARGO_PKG_VERSION")}}),
                ),
                "ping" => Ok(json!({})),
                "tools/list" | "tools/call" => {
                    // Detached OS workers do not hold Tokio runtime shutdown open
                    // after the parent closes stdin. The host still owns the job.
                    let (send, receive) = tokio::sync::oneshot::channel();
                    std::thread::spawn(move || {
                        let _ = send.send(forward(&base, &token, &method, params));
                    });
                    receive
                        .await
                        .unwrap_or_else(|_| Err("COLLABORATION_BRIDGE_WORKER_FAILED".into()))
                }
                _ => Err("MCP_METHOD_NOT_FOUND".into()),
            };
            if let Ok(mut output) = output.lock() {
                let _ = writeln!(output, "{}", reply(id, result));
                let _ = output.flush();
            }
        });
        while jobs.try_join_next().is_some() {}
    }
    // Parent closing stdin means the thread has disconnected. Pending work is
    // owned/cancelled by the host participant lifecycle, never by this proxy.
    jobs.abort_all();
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn endpoint_only_accepts_loopback_without_path_or_credentials() {
        assert_eq!(
            endpoint("http://127.0.0.1:8103").unwrap(),
            "http://127.0.0.1:8103"
        );
        assert!(endpoint("http://[::1]:8103/").is_ok());
        for bad in [
            "http://example.com:8103",
            "http://127.0.0.1:0",
            "http://127.0.0.1:8103/path",
            "http://token@127.0.0.1:8103",
            "http://0.0.0.0:8103",
        ] {
            assert!(endpoint(bad).is_err(), "{bad}");
        }
    }
    #[test]
    fn bounded_frames_and_partial_input_fail_without_allocating_unboundedly() {
        let mut input = std::io::Cursor::new(b"\n{\"id\":1}\n".to_vec());
        assert_eq!(request_frame(&mut input).unwrap().unwrap()["id"], 1);
        assert!(request_frame(&mut input).unwrap().is_none());
        assert!(request_frame(&mut std::io::Cursor::new(b"{}".to_vec())).is_err());
        assert!(request_frame(&mut std::io::Cursor::new(vec![b'x'; MAX_FRAME_BYTES + 1])).is_err());
    }
}
