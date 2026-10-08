//! forge-cloud HTTP 传输:ureq(阻塞)包进 `spawn_blocking`。
//! 只负责「发出去、收回来」;鉴权与续期在 [super::CloudService]。
//! 错误消息只带 URL 路径与传输原因,绝不含请求头/请求体(令牌在头里)。

use std::io::Read;
use std::time::Duration;

use serde_json::Value;

use super::CloudError;

/// 建连超时。
const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
/// 单请求总时长上限。
pub(crate) const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);
/// 响应体上限(头像 ≤512 KiB,列表类 JSON 远小于此)。
const BODY_MAX: u64 = 16 * 1024 * 1024;

/// 一次请求。
pub(crate) struct Request {
    pub method: String,
    pub url: String,
    /// Bearer 令牌(access token);None = 公开接口。
    pub bearer: Option<String>,
    pub json: Option<Value>,
    pub timeout: Duration,
}

impl Request {
    pub fn new(method: &str, url: String) -> Self {
        Request {
            method: method.to_ascii_uppercase(),
            url,
            bearer: None,
            json: None,
            timeout: REQUEST_TIMEOUT,
        }
    }

    pub fn bearer(mut self, token: Option<String>) -> Self {
        self.bearer = token;
        self
    }

    pub fn json(mut self, body: Option<Value>) -> Self {
        self.json = body;
        self
    }

    pub fn timeout(mut self, t: Duration) -> Self {
        self.timeout = t;
        self
    }
}

/// 云端回包(任意状态码;非 2xx 也走这里,由调用方决定怎么解释)。
#[derive(Debug, Clone)]
pub(crate) struct Reply {
    pub status: u16,
    pub content_type: String,
    pub body: Vec<u8>,
}

impl Reply {
    pub fn ok(&self) -> bool {
        (200..300).contains(&self.status)
    }

    /// 2xx 的 JSON 体(空体 = `{}`)。
    pub fn json(&self) -> Result<Value, CloudError> {
        if self.body.iter().all(u8::is_ascii_whitespace) {
            return Ok(Value::Object(Default::default()));
        }
        serde_json::from_slice(&self.body).map_err(|e| {
            CloudError::new(502, "CLOUD_BAD_RESPONSE", format!("云端响应不是 JSON:{e}"))
        })
    }

    /// `{error:{code,message}}` 的 code(无则 None)。
    pub fn error_code(&self) -> Option<String> {
        serde_json::from_slice::<Value>(&self.body)
            .ok()?
            .pointer("/error/code")
            .and_then(Value::as_str)
            .map(str::to_string)
    }

    /// 非 2xx → CloudError(云端业务码原样透传;无结构体则按状态码兜底)。
    pub fn to_error(&self) -> CloudError {
        let parsed = serde_json::from_slice::<Value>(&self.body).ok();
        let err = parsed.as_ref().and_then(|v| v.get("error"));
        let code = err
            .and_then(|e| e.get("code"))
            .and_then(Value::as_str)
            .filter(|c| !c.is_empty())
            .map(str::to_string)
            .unwrap_or_else(|| "CLOUD_HTTP_ERROR".to_string());
        let message = err
            .and_then(|e| e.get("message"))
            .and_then(Value::as_str)
            .map(str::to_string)
            .unwrap_or_else(|| format!("云端返回 HTTP {}", self.status));
        CloudError::new(self.status, code, message)
    }
}

/// 发请求。传输层失败(DNS/建连/超时/TLS)→ `CLOUD_UNREACHABLE`;任何 HTTP 状态码都算「可达」。
pub(crate) async fn send(req: Request) -> Result<Reply, CloudError> {
    tokio::task::spawn_blocking(move || send_blocking(req))
        .await
        .map_err(|e| CloudError::new(500, "CLOUD_INTERNAL", format!("spawn_blocking 失败:{e}")))?
}

fn send_blocking(req: Request) -> Result<Reply, CloudError> {
    let agent = ureq::AgentBuilder::new()
        .timeout_connect(CONNECT_TIMEOUT.min(req.timeout))
        .timeout(req.timeout)
        .build();
    let mut r = agent
        .request(&req.method, &req.url)
        .set("Accept", "application/json")
        .set(
            "User-Agent",
            concat!("forge-agentd/", env!("CARGO_PKG_VERSION")),
        );
    if let Some(t) = &req.bearer {
        r = r.set("Authorization", &format!("Bearer {t}"));
    }
    let result = match &req.json {
        Some(body) => r
            .set("Content-Type", "application/json")
            .send_string(&body.to_string()),
        None => r.call(),
    };
    let resp = match result {
        Ok(resp) => resp,
        Err(ureq::Error::Status(_, resp)) => resp,
        Err(ureq::Error::Transport(t)) => {
            let detail = t.message().map(|m| format!(": {m}")).unwrap_or_default();
            return Err(CloudError::unreachable(format!(
                "{} {}{detail}",
                path_of(&req.url),
                t.kind()
            )));
        }
    };
    let status = resp.status();
    let content_type = resp.header("Content-Type").unwrap_or("").to_string();
    let mut body = Vec::new();
    resp.into_reader()
        .take(BODY_MAX)
        .read_to_end(&mut body)
        .map_err(|e| CloudError::unreachable(format!("读响应体失败:{e}")))?;
    Ok(Reply {
        status,
        content_type,
        body,
    })
}

/// URL → 路径段(错误消息只报路径,不报主机/查询串)。
fn path_of(url: &str) -> &str {
    let rest = url.split_once("://").map(|(_, r)| r).unwrap_or(url);
    let path = rest.find('/').map(|i| &rest[i..]).unwrap_or("/");
    path.split('?').next().unwrap_or(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn error_body_passthrough_and_fallback() {
        let r = Reply {
            status: 409,
            content_type: "application/json".into(),
            body: br#"{"error":{"code":"EMAIL_TAKEN","message":"x"}}"#.to_vec(),
        };
        let e = r.to_error();
        assert_eq!(
            (e.status, e.code.as_str(), e.message.as_str()),
            (409, "EMAIL_TAKEN", "x")
        );
        let r = Reply {
            status: 503,
            content_type: "text/plain".into(),
            body: b"down".to_vec(),
        };
        assert_eq!(r.to_error().code, "CLOUD_HTTP_ERROR");
        assert_eq!(r.error_code(), None);
    }

    #[test]
    fn empty_body_is_empty_object() {
        let r = Reply {
            status: 200,
            content_type: String::new(),
            body: Vec::new(),
        };
        assert_eq!(r.json().unwrap(), serde_json::json!({}));
    }

    #[test]
    fn path_of_strips_host_and_query() {
        assert_eq!(path_of("http://h:1/api/v1/me?x=1"), "/api/v1/me");
        assert_eq!(path_of("http://h:1"), "/");
    }

    #[tokio::test]
    async fn unreachable_maps_to_cloud_unreachable() {
        let e = send(
            Request::new("GET", "http://127.0.0.1:9/healthz".into())
                .timeout(Duration::from_secs(3)),
        )
        .await
        .unwrap_err();
        assert_eq!(e.status, 502);
        assert_eq!(e.code, "CLOUD_UNREACHABLE");
    }
}
