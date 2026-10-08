//! D-044:UltraPlan 网页 Demo 的进程级静态托管。
//!
//! 为什么单起一个监听而不是挂在主路由上:Demo 是模型写的任意 HTML/JS,跟 IDE 同源就能直接
//! `fetch('/api/forge/…')` 动会话、调工具。单独的回环端口 + 前端换用**另一个**回环主机名
//! (`127.0.0.1` ↔ `localhost`)让 Demo 与 IDE 跨站,浏览器的同源策略替我们挡住它;这个监听
//! 也只认 `/u/<token>/…` 三种路径,别的一律 404,绝不代理主服务的 API。
//!
//! 不是 AppState 字段(五处 AppState 构造保持不动):进程级单例 [`global`],首次需要时
//! ([`DemoRegistry::ensure_started`])才绑定端口,活到进程退出。端口每次重启都会变,
//! 所以事件里从不写 Demo 地址,前端经 `GET …/ultraplan` 取 `{port, token}` 自己拼。
//!
//! 安全面(每一条都有单测):
//! - 路径:[`resolve_demo_file`] 拒绝空路径、绝对路径、`.`/`..` 段、含 `:` 或 `\` 的段(ADS、
//!   盘符)、尾点尾空格、Windows 设备名;扩展名白名单;canonicalize 后必须仍在 Demo 目录内;
//!   只发普通文件,≤ 16 MiB。
//! - 来源:[`host_allowed`] 只认回环 Host(挡 DNS rebinding),带 Origin 的请求必须来自回环。
//! - 响应头:每个响应(含 404/403)都带 [`DEMO_CSP`] 与 nosniff / no-referrer /
//!   Permissions-Policy / no-cache——Demo 连不出本源、不能被非回环页面嵌入、拿不到摄像头麦克风。

use std::collections::HashMap;
use std::net::{Ipv6Addr, SocketAddr};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU16, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

use axum::{
    body::Body,
    extract::{Path as UrlPath, State},
    http::{header, HeaderMap, HeaderName, HeaderValue, StatusCode},
    response::{IntoResponse, Response},
    routing::get,
    Router,
};

/// 监听地址覆盖(缺省 `127.0.0.1:0`,即系统分配端口)。只接受回环地址。
pub const DEMO_ADDR_ENV: &str = "FORGE_AGENTD_DEMO_ADDR";
/// Demo 入口文件。
pub const ENTRY: &str = "index.html";
/// 单文件上限(字节)。
pub const MAX_FILE_BYTES: u64 = 16 * 1024 * 1024;

/// 每个响应都带的 CSP(D-044)。逐项的用意:
/// - `default-src 'none'` 兜底,下面逐类放开;脚本 / 样式允许内联(模型写的 Demo 普遍内联);
/// - 图片 / 音频 / 字体允许 data: 与 blob:(Canvas 导出、内联 SVG、程序化音效);
/// - `connect-src 'self'`:只能请求自己这个源(读同目录的 JSON),连不到 IDE 的 API 与外网;
/// - `form-action 'none'` / `object-src 'none'` / `frame-src 'none'`:不提交表单、不嵌插件与子框架;
/// - `frame-ancestors` 只许回环页面嵌入(IDE 的端口可配置,所以端口用通配)。
pub const DEMO_CSP: &str = "default-src 'none'; script-src 'self' 'unsafe-inline'; \
style-src 'self' 'unsafe-inline'; img-src 'self' data: blob:; media-src 'self' data: blob:; \
font-src 'self' data:; worker-src 'self' blob:; connect-src 'self'; form-action 'none'; \
base-uri 'self'; object-src 'none'; frame-src 'none'; \
frame-ancestors http://127.0.0.1:* http://localhost:*";
/// 设备权限一律关闭(Demo 用不到,也不该弹授权框)。
pub const PERMISSIONS_POLICY: &str = "camera=(), microphone=(), geolocation=(), usb=(), serial=()";

/// 扩展名白名单(小写)→ Content-Type。不在表里的一律不发(含 .exe / .bat / .wasm 等)。
const ALLOWED_EXT: &[(&str, &str)] = &[
    ("html", "text/html; charset=utf-8"),
    ("css", "text/css; charset=utf-8"),
    ("js", "text/javascript; charset=utf-8"),
    ("mjs", "text/javascript; charset=utf-8"),
    ("json", "application/json"),
    ("png", "image/png"),
    ("jpg", "image/jpeg"),
    ("jpeg", "image/jpeg"),
    ("gif", "image/gif"),
    ("webp", "image/webp"),
    ("svg", "image/svg+xml"),
    ("ico", "image/x-icon"),
    ("wav", "audio/wav"),
    ("mp3", "audio/mpeg"),
    ("ogg", "audio/ogg"),
    ("woff2", "font/woff2"),
    ("ttf", "font/ttf"),
    ("txt", "text/plain; charset=utf-8"),
    ("md", "text/markdown; charset=utf-8"),
];

/// Windows 保留设备名(按「第一个点之前」的部分、不分大小写比较;`con.js` 一样打开的是控制台)。
const DEVICE_NAMES: &[&str] = &[
    "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8",
    "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
];

/// 文件解析失败的原因(全部映射成不泄露细节的 HTTP 状态)。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ServeErr {
    /// 路径形态非法:空、绝对、`.`/`..` 段、含 `:` 或 `\`、尾点尾空格、设备名、控制字符。
    BadPath,
    /// 扩展名不在白名单。
    ExtNotAllowed,
    /// 解析(含符号链接)后落在 Demo 目录之外。
    Outside,
    /// 不存在,或不是普通文件。
    NotFound,
    /// 超过 [`MAX_FILE_BYTES`]。
    TooLarge,
}

impl ServeErr {
    fn status(self) -> StatusCode {
        match self {
            ServeErr::BadPath => StatusCode::BAD_REQUEST,
            ServeErr::ExtNotAllowed => StatusCode::FORBIDDEN,
            ServeErr::Outside | ServeErr::NotFound => StatusCode::NOT_FOUND,
            ServeErr::TooLarge => StatusCode::PAYLOAD_TOO_LARGE,
        }
    }
}

/// 扩展名 → Content-Type(大小写不敏感)。
fn mime_for(ext: &str) -> Option<&'static str> {
    let ext = ext.to_ascii_lowercase();
    ALLOWED_EXT
        .iter()
        .find(|(e, _)| *e == ext)
        .map(|(_, mime)| *mime)
}

/// 单个路径段是否合法(纯词法,不碰文件系统)。
fn segment_ok(seg: &str) -> bool {
    if seg.is_empty() || seg == "." || seg == ".." {
        return false;
    }
    // ':' = 盘符 / NTFS 备用数据流(x.js::$DATA);'\' = Windows 分隔符,会绕过按 '/' 切段的检查。
    if seg.contains([':', '\\']) || seg.chars().any(|c| c.is_control()) {
        return false;
    }
    // Windows 打开文件时会静默去掉尾点尾空格:`index.html.` 与 `index.html` 是同一个文件。
    if seg.ends_with('.') || seg.ends_with(' ') {
        return false;
    }
    let stem = seg.split('.').next().unwrap_or("").trim_end();
    !DEVICE_NAMES.iter().any(|d| stem.eq_ignore_ascii_case(d))
}

/// 把 Demo 目录内的相对路径解析成可发的文件(纯函数,单测直接覆盖)。
///
/// 顺序:词法检查(段、扩展名)→ canonicalize 根与文件、要求 `starts_with`(挡符号链接 /
/// 交接点逃逸)→ 只发普通文件、≤ 16 MiB。返回 (绝对路径, Content-Type)。
pub(crate) fn resolve_demo_file(
    root: &Path,
    rel: &str,
) -> Result<(PathBuf, &'static str), ServeErr> {
    if rel.is_empty() || rel.starts_with('/') || rel.starts_with('\\') {
        return Err(ServeErr::BadPath);
    }
    let segments: Vec<&str> = rel.split('/').collect();
    if !segments.iter().all(|s| segment_ok(s)) {
        return Err(ServeErr::BadPath);
    }
    let last = segments.last().copied().unwrap_or("");
    let ext = match last.rsplit_once('.') {
        Some((stem, ext)) if !stem.is_empty() => ext,
        _ => return Err(ServeErr::ExtNotAllowed),
    };
    if mime_for(ext).is_none() {
        return Err(ServeErr::ExtNotAllowed);
    }
    let root = root.canonicalize().map_err(|_| ServeErr::NotFound)?;
    let file = root
        .join(segments.iter().collect::<PathBuf>())
        .canonicalize()
        .map_err(|_| ServeErr::NotFound)?;
    if !file.starts_with(&root) {
        return Err(ServeErr::Outside);
    }
    // 以解析后的真实文件名定 Content-Type:目录内的符号链接指向别的扩展名时也不会被错标类型。
    let mime = file
        .extension()
        .and_then(|e| e.to_str())
        .and_then(mime_for)
        .ok_or(ServeErr::ExtNotAllowed)?;
    let meta = std::fs::metadata(&file).map_err(|_| ServeErr::NotFound)?;
    if !meta.is_file() {
        return Err(ServeErr::NotFound);
    }
    if meta.len() > MAX_FILE_BYTES {
        return Err(ServeErr::TooLarge);
    }
    Ok((file, mime))
}

/// Host 头只认本监听的三种回环写法(挡 DNS rebinding:恶意域名解析到 127.0.0.1 时,
/// 浏览器发来的 Host 仍是那个域名)。
pub(crate) fn host_allowed(host: Option<&str>, port: u16) -> bool {
    let Some(host) = host else {
        return false;
    };
    [
        format!("127.0.0.1:{port}"),
        format!("localhost:{port}"),
        format!("[::1]:{port}"),
    ]
    .iter()
    .any(|ok| host.eq_ignore_ascii_case(ok))
}

/// 带 Origin 的请求(跨源 fetch、模块脚本的 CORS 请求等)必须来自回环源;没有 Origin 放行
/// (普通导航与同源子资源请求不带它)。`null` 源不是回环,拒绝。
pub(crate) fn origin_allowed(origin: Option<&str>) -> bool {
    let Some(origin) = origin else {
        return true;
    };
    let Some(rest) = origin.strip_prefix("http://") else {
        return false;
    };
    let after_host = ["127.0.0.1", "localhost", "[::1]"]
        .iter()
        .find_map(|h| rest.strip_prefix(h));
    match after_host {
        Some("") => true,
        Some(port) => port
            .strip_prefix(':')
            .is_some_and(|p| !p.is_empty() && p.parse::<u16>().is_ok()),
        None => false,
    }
}

/// 令牌形态(契约是 32 位小写 hex;这里放宽到字母数字与 `-` `_`,只为不让怪字符进 Location 头)。
fn token_ok(token: &str) -> bool {
    !token.is_empty()
        && token.len() <= 64
        && token
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

/// 进程级 Demo 注册表:token → Demo 目录,外加监听端口与「只绑一次」的启动锁。
pub struct DemoRegistry {
    roots: Mutex<HashMap<String, PathBuf>>,
    /// 0 = 尚未启动。
    port: AtomicU16,
    start: tokio::sync::Mutex<()>,
}

impl DemoRegistry {
    fn new() -> Self {
        DemoRegistry {
            roots: Mutex::new(HashMap::new()),
            port: AtomicU16::new(0),
            start: tokio::sync::Mutex::new(()),
        }
    }

    /// 单测用:端口预置为 `port`、不绑定任何端口([`Self::ensure_started`] 直接返回它)。
    #[cfg(test)]
    pub fn for_test(port: u16) -> Arc<Self> {
        let reg = DemoRegistry::new();
        reg.port.store(port, Ordering::SeqCst);
        Arc::new(reg)
    }

    /// 登记(或改指)某个流程的 Demo 目录。
    pub fn register(&self, token: &str, demo_dir: &Path) {
        self.roots
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(token.to_string(), demo_dir.to_path_buf());
    }

    /// 撤销登记(流程重开 / 清除时调用;之后该 token 的所有路径都是 404)。
    #[allow(dead_code)] // D-044 W2b(restart 撤销登记)接线
    pub fn unregister(&self, token: &str) {
        self.roots
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(token);
    }

    fn root(&self, token: &str) -> Option<PathBuf> {
        if !token_ok(token) {
            return None;
        }
        self.roots
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(token)
            .cloned()
    }

    /// 已启动时的端口。
    pub fn port(&self) -> Option<u16> {
        match self.port.load(Ordering::SeqCst) {
            0 => None,
            p => Some(p),
        }
    }

    /// 首次调用时绑定监听并起服务,之后直接返回端口(并发调用由启动锁串行化,只绑一次)。
    ///
    /// 地址:env [`DEMO_ADDR_ENV`],缺省 `127.0.0.1:0`;非回环地址一律拒绝(Host 头可以伪造,
    /// 绑到局域网上就等于把 Demo 目录公开)。IPv4 绑定成功后再在 `[::1]` 的同一端口补一个
    /// 监听——`localhost` 在部分系统上先解析到 ::1;这一步失败不算错,IPv4 照常服务。
    pub async fn ensure_started(self: &Arc<Self>) -> Result<u16, String> {
        if let Some(p) = self.port() {
            return Ok(p);
        }
        let _guard = self.start.lock().await;
        if let Some(p) = self.port() {
            return Ok(p);
        }
        let addr: SocketAddr = match std::env::var(DEMO_ADDR_ENV) {
            Ok(v) if !v.trim().is_empty() => v.trim().parse().map_err(|e| {
                format!("DEMO_HOST_UNAVAILABLE: {DEMO_ADDR_ENV}={v} 不是合法的监听地址: {e}")
            })?,
            _ => SocketAddr::from(([127, 0, 0, 1], 0)),
        };
        if !addr.ip().is_loopback() {
            return Err(format!(
                "DEMO_HOST_UNAVAILABLE: {DEMO_ADDR_ENV}={addr} 不是回环地址,Demo 托管只许绑定回环"
            ));
        }
        let listener = tokio::net::TcpListener::bind(addr)
            .await
            .map_err(|e| format!("DEMO_HOST_UNAVAILABLE: 绑定 {addr} 失败: {e}"))?;
        let port = listener
            .local_addr()
            .map_err(|e| format!("DEMO_HOST_UNAVAILABLE: 读取监听端口失败: {e}"))?
            .port();
        let v6 = if addr.is_ipv4() {
            match tokio::net::TcpListener::bind(SocketAddr::from((Ipv6Addr::LOCALHOST, port))).await
            {
                Ok(l) => Some(l),
                Err(e) => {
                    eprintln!("[demo_host] [::1]:{port} 绑定失败(仅 IPv4 服务): {e}");
                    None
                }
            }
        } else {
            None
        };
        let app = demo_router(self.clone());
        tokio::spawn(async move {
            if let Err(e) = axum::serve(listener, app).await {
                eprintln!("[demo_host] 服务退出: {e}");
            }
        });
        if let Some(l6) = v6 {
            let app = demo_router(self.clone());
            tokio::spawn(async move {
                if let Err(e) = axum::serve(l6, app).await {
                    eprintln!("[demo_host] [::1] 服务退出: {e}");
                }
            });
        }
        self.port.store(port, Ordering::SeqCst);
        Ok(port)
    }
}

/// 进程级单例。
#[allow(dead_code)] // D-044 W2b(Demo 轮次 / GET …/ultraplan)接线
pub fn global() -> Arc<DemoRegistry> {
    static GLOBAL: OnceLock<Arc<DemoRegistry>> = OnceLock::new();
    GLOBAL.get_or_init(|| Arc::new(DemoRegistry::new())).clone()
}

/// 探测脚本访问 Demo 用的地址(服务端自用;前端按契约 §8 自己拼,不经过这里)。
pub fn local_url(port: u16, token: &str) -> String {
    format!("http://127.0.0.1:{port}/u/{token}/")
}

/// Demo 托管的路由:只有 `/u/{token}`(补斜杠重定向)、`/u/{token}/`(入口)、
/// `/u/{token}/{*path}` 三种形态——matchit 的通配段不匹配空余部分,所以入口单列一条。
/// 其余一律 404;每个响应都经 [`security_headers`] 补齐安全头。
pub fn demo_router(reg: Arc<DemoRegistry>) -> Router {
    Router::new()
        .route("/u/{token}", get(redirect_slash))
        .route("/u/{token}/", get(serve_index))
        .route("/u/{token}/{*path}", get(serve_path))
        .fallback(fallback_404)
        .with_state(reg)
        .layer(axum::middleware::map_response(security_headers))
}

async fn security_headers(mut resp: Response) -> Response {
    let h = resp.headers_mut();
    h.insert(
        header::CONTENT_SECURITY_POLICY,
        HeaderValue::from_static(DEMO_CSP),
    );
    h.insert(
        header::X_CONTENT_TYPE_OPTIONS,
        HeaderValue::from_static("nosniff"),
    );
    h.insert(
        header::REFERRER_POLICY,
        HeaderValue::from_static("no-referrer"),
    );
    h.insert(
        HeaderName::from_static("permissions-policy"),
        HeaderValue::from_static(PERMISSIONS_POLICY),
    );
    h.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-cache"));
    resp
}

fn plain(status: StatusCode, msg: &'static str) -> Response {
    (status, msg).into_response()
}

fn request_allowed(reg: &DemoRegistry, headers: &HeaderMap) -> bool {
    let Some(port) = reg.port() else {
        return false;
    };
    let host = headers.get(header::HOST).and_then(|v| v.to_str().ok());
    let origin = headers
        .get(header::ORIGIN)
        .map(|v| v.to_str().unwrap_or("?"));
    host_allowed(host, port) && origin_allowed(origin)
}

async fn fallback_404() -> Response {
    plain(StatusCode::NOT_FOUND, "not found")
}

async fn redirect_slash(
    State(reg): State<Arc<DemoRegistry>>,
    UrlPath(token): UrlPath<String>,
    headers: HeaderMap,
) -> Response {
    if !request_allowed(&reg, &headers) {
        return plain(StatusCode::FORBIDDEN, "forbidden host or origin");
    }
    if reg.root(&token).is_none() {
        return plain(StatusCode::NOT_FOUND, "not found");
    }
    // token 已过 token_ok(只含字母数字 - _),拼进 Location 不会注入头。
    Response::builder()
        .status(StatusCode::PERMANENT_REDIRECT)
        .header(header::LOCATION, format!("/u/{token}/"))
        .body(Body::empty())
        .unwrap_or_else(|_| plain(StatusCode::NOT_FOUND, "not found"))
}

async fn serve_index(
    State(reg): State<Arc<DemoRegistry>>,
    UrlPath(token): UrlPath<String>,
    headers: HeaderMap,
) -> Response {
    serve_file(&reg, &headers, &token, ENTRY.to_string()).await
}

async fn serve_path(
    State(reg): State<Arc<DemoRegistry>>,
    UrlPath((token, path)): UrlPath<(String, String)>,
    headers: HeaderMap,
) -> Response {
    serve_file(&reg, &headers, &token, path).await
}

async fn serve_file(reg: &DemoRegistry, headers: &HeaderMap, token: &str, rel: String) -> Response {
    if !request_allowed(reg, headers) {
        return plain(StatusCode::FORBIDDEN, "forbidden host or origin");
    }
    let Some(root) = reg.root(token) else {
        return plain(StatusCode::NOT_FOUND, "not found");
    };
    let read = tokio::task::spawn_blocking(move || {
        let (path, mime) = resolve_demo_file(&root, &rel)?;
        let bytes = std::fs::read(&path).map_err(|_| ServeErr::NotFound)?;
        Ok::<_, ServeErr>((bytes, mime))
    })
    .await;
    match read {
        Ok(Ok((bytes, mime))) => Response::builder()
            .status(StatusCode::OK)
            .header(header::CONTENT_TYPE, mime)
            .body(Body::from(bytes))
            .unwrap_or_else(|_| plain(StatusCode::INTERNAL_SERVER_ERROR, "response build failed")),
        Ok(Err(e)) => plain(
            e.status(),
            match e {
                ServeErr::BadPath => "bad path",
                ServeErr::ExtNotAllowed => "file type not allowed",
                ServeErr::TooLarge => "file too large",
                ServeErr::Outside | ServeErr::NotFound => "not found",
            },
        ),
        Err(_) => plain(StatusCode::INTERNAL_SERVER_ERROR, "read task failed"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::Request;
    use http_body_util::BodyExt;
    use tower::ServiceExt;

    const PORT: u16 = 47_321;
    const TOKEN: &str = "0123456789abcdef0123456789abcdef";

    fn temp_root(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "d044-demo-host-{tag}-{}-{}",
            std::process::id(),
            crate::events::new_id("t")
        ));
        std::fs::create_dir_all(dir.join("demo").join("js")).unwrap();
        dir
    }

    fn get_req(path: &str, host: Option<&str>, origin: Option<&str>) -> Request<Body> {
        let mut b = Request::builder().method("GET").uri(path);
        if let Some(h) = host {
            b = b.header(header::HOST, h);
        }
        if let Some(o) = origin {
            b = b.header(header::ORIGIN, o);
        }
        b.body(Body::empty()).unwrap()
    }

    fn assert_security_headers(resp: &Response) {
        let h = resp.headers();
        assert_eq!(h[header::CONTENT_SECURITY_POLICY], DEMO_CSP);
        assert_eq!(h[header::X_CONTENT_TYPE_OPTIONS], "nosniff");
        assert_eq!(h[header::REFERRER_POLICY], "no-referrer");
        assert_eq!(h["permissions-policy"], PERMISSIONS_POLICY);
        assert_eq!(h[header::CACHE_CONTROL], "no-cache");
    }

    async fn body_text(resp: Response) -> String {
        let bytes = resp.into_body().collect().await.unwrap().to_bytes();
        String::from_utf8_lossy(&bytes).into_owned()
    }

    /// CSP 逐字(契约与 approved plan 的原文;前端 / 桌面端的 frame 加固按这个串对齐)。
    #[test]
    fn csp_string_is_exact() {
        assert_eq!(
            DEMO_CSP,
            "default-src 'none'; script-src 'self' 'unsafe-inline'; style-src 'self' 'unsafe-inline'; \
img-src 'self' data: blob:; media-src 'self' data: blob:; font-src 'self' data:; worker-src 'self' blob:; \
connect-src 'self'; form-action 'none'; base-uri 'self'; object-src 'none'; frame-src 'none'; \
frame-ancestors http://127.0.0.1:* http://localhost:*"
        );
        assert!(!DEMO_CSP.contains("  "), "续行拼接不得留双空格");
    }

    #[test]
    fn resolve_demo_file_confines_and_filters_ext() {
        let base = temp_root("resolve");
        let root = base.join("demo");
        std::fs::write(root.join("index.html"), "<!doctype html>").unwrap();
        std::fs::write(root.join("js").join("game.js"), "1").unwrap();
        std::fs::write(root.join("LOGO.PNG"), [0x89u8, b'P', b'N', b'G']).unwrap();
        std::fs::write(root.join("tool.exe"), "MZ").unwrap();
        std::fs::write(root.join("noext"), "x").unwrap();
        std::fs::write(base.join("secret.js"), "outside").unwrap();

        // 正常路径。
        let (p, mime) = resolve_demo_file(&root, "index.html").unwrap();
        assert!(p.ends_with("index.html"));
        assert_eq!(mime, "text/html; charset=utf-8");
        assert_eq!(
            resolve_demo_file(&root, "js/game.js").unwrap().1,
            "text/javascript; charset=utf-8"
        );
        // 大写扩展名:按小写查白名单。
        assert_eq!(resolve_demo_file(&root, "LOGO.PNG").unwrap().1, "image/png");

        // 穿越 / 绝对 / 点段 / 空段 / 反斜杠。
        for bad in [
            "",
            "../secret.js",
            "js/../../secret.js",
            "./index.html",
            "js/./game.js",
            "/index.html",
            "\\index.html",
            "js//game.js",
            "js\\game.js",
            "..\\secret.js",
            "C:/Windows/win.ini",
            "C:\\Windows\\win.ini",
        ] {
            assert_eq!(
                resolve_demo_file(&root, bad).unwrap_err(),
                ServeErr::BadPath,
                "{bad:?} 应被词法拒绝"
            );
        }
        // Windows 怪癖:ADS、尾点尾空格、设备名(含带扩展名与大小写变体)。
        for bad in [
            "index.html::$DATA",
            "x.js::$DATA",
            "index.html.",
            "index.html ",
            "CON",
            "con.js",
            "Nul.html",
            "js/aux.css",
            "COM1.png",
            "lpt9.txt",
            "PRN .js",
        ] {
            assert_eq!(
                resolve_demo_file(&root, bad).unwrap_err(),
                ServeErr::BadPath,
                "{bad:?} 应被拒绝"
            );
        }
        // 不是设备名的近似名照常解析(只是不存在)。
        assert_eq!(
            resolve_demo_file(&root, "console.js").unwrap_err(),
            ServeErr::NotFound
        );
        assert_eq!(
            resolve_demo_file(&root, "com10.js").unwrap_err(),
            ServeErr::NotFound
        );
        // 扩展名白名单。
        assert_eq!(
            resolve_demo_file(&root, "tool.exe").unwrap_err(),
            ServeErr::ExtNotAllowed
        );
        assert_eq!(
            resolve_demo_file(&root, "noext").unwrap_err(),
            ServeErr::ExtNotAllowed
        );
        assert_eq!(
            resolve_demo_file(&root, ".html").unwrap_err(),
            ServeErr::ExtNotAllowed
        );
        // 目录不是普通文件。
        std::fs::create_dir_all(root.join("dir.js")).unwrap();
        assert_eq!(
            resolve_demo_file(&root, "dir.js").unwrap_err(),
            ServeErr::NotFound
        );
        // 不存在。
        assert_eq!(
            resolve_demo_file(&root, "missing.js").unwrap_err(),
            ServeErr::NotFound
        );
        // 超过 16 MiB(稀疏文件:set_len 不真写数据)。
        let big = std::fs::File::create(root.join("big.png")).unwrap();
        big.set_len(MAX_FILE_BYTES + 1).unwrap();
        drop(big);
        assert_eq!(
            resolve_demo_file(&root, "big.png").unwrap_err(),
            ServeErr::TooLarge
        );

        // 符号链接逃逸(建链接需要权限的系统上建不出来就跳过,如实打印)。
        #[cfg(windows)]
        let linked =
            std::os::windows::fs::symlink_file(base.join("secret.js"), root.join("link.js"));
        #[cfg(unix)]
        let linked = std::os::unix::fs::symlink(base.join("secret.js"), root.join("link.js"));
        match linked {
            Ok(()) => assert_eq!(
                resolve_demo_file(&root, "link.js").unwrap_err(),
                ServeErr::Outside,
                "指向 Demo 目录外的符号链接必须拒绝"
            ),
            Err(e) => eprintln!("[skip] 本机建不了符号链接,跳过逃逸用例: {e}"),
        }
        std::fs::remove_dir_all(&base).ok();
    }

    #[test]
    fn host_and_origin_checks() {
        assert!(host_allowed(Some("127.0.0.1:4000"), 4000));
        assert!(host_allowed(Some("localhost:4000"), 4000));
        assert!(host_allowed(Some("LOCALHOST:4000"), 4000));
        assert!(host_allowed(Some("[::1]:4000"), 4000));
        for bad in [
            None,
            Some("127.0.0.1:4001"),
            Some("127.0.0.1"),
            Some("evil.example:4000"),
            Some("localhost.evil.example:4000"),
            Some("0.0.0.0:4000"),
            Some("192.168.1.2:4000"),
        ] {
            assert!(!host_allowed(bad, 4000), "{bad:?}");
        }
        assert!(origin_allowed(None));
        assert!(origin_allowed(Some("http://127.0.0.1:3080")));
        assert!(origin_allowed(Some("http://localhost:5173")));
        assert!(origin_allowed(Some("http://[::1]:4000")));
        assert!(origin_allowed(Some("http://localhost")));
        for bad in [
            "null",
            "https://127.0.0.1:3080",
            "http://evil.example",
            "http://localhost.evil.example",
            "http://127.0.0.1.nip.io:80",
            "http://127.0.0.1:",
            "http://127.0.0.1:99999",
        ] {
            assert!(!origin_allowed(Some(bad)), "{bad}");
        }
    }

    #[tokio::test]
    async fn router_serves_index_with_csp_and_rejects_bad_host() {
        let base = temp_root("router");
        let root = base.join("demo");
        std::fs::write(
            root.join("index.html"),
            "<!doctype html><title>demo</title>",
        )
        .unwrap();
        std::fs::write(root.join("js").join("main.js"), "window.__demo = {};").unwrap();
        let reg = DemoRegistry::for_test(PORT);
        reg.register(TOKEN, &root);
        let host = format!("127.0.0.1:{PORT}");

        let resp = demo_router(reg.clone())
            .oneshot(get_req(&format!("/u/{TOKEN}/"), Some(&host), None))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        assert_security_headers(&resp);
        assert_eq!(
            resp.headers()[header::CONTENT_TYPE],
            "text/html; charset=utf-8"
        );
        assert!(body_text(resp).await.contains("<title>demo</title>"));

        // 子路径 + 三种回环 Host 写法 + 回环 Origin。
        for h in [
            format!("127.0.0.1:{PORT}"),
            format!("localhost:{PORT}"),
            format!("[::1]:{PORT}"),
        ] {
            let resp = demo_router(reg.clone())
                .oneshot(get_req(
                    &format!("/u/{TOKEN}/js/main.js"),
                    Some(&h),
                    Some("http://127.0.0.1:3080"),
                ))
                .await
                .unwrap();
            assert_eq!(resp.status(), StatusCode::OK, "Host {h}");
            assert_eq!(
                resp.headers()[header::CONTENT_TYPE],
                "text/javascript; charset=utf-8"
            );
        }

        // 坏 Host / 缺 Host / 端口不符 / 非回环 Origin → 403,同样带全套安全头。
        for (h, o) in [
            (Some("evil.example".to_string()), None),
            (None, None),
            (Some(format!("127.0.0.1:{}", PORT + 1)), None),
            (Some(host.clone()), Some("http://evil.example")),
            (Some(host.clone()), Some("null")),
        ] {
            let resp = demo_router(reg.clone())
                .oneshot(get_req(&format!("/u/{TOKEN}/"), h.as_deref(), o))
                .await
                .unwrap();
            assert_eq!(
                resp.status(),
                StatusCode::FORBIDDEN,
                "Host {h:?} Origin {o:?}"
            );
            assert_security_headers(&resp);
        }

        // 穿越与白名单外扩展名经路由同样被拒(路径参数已 percent-decode)。
        std::fs::write(base.join("secret.js"), "outside").unwrap();
        for (path, status) in [
            (
                format!("/u/{TOKEN}/%2e%2e/secret.js"),
                StatusCode::BAD_REQUEST,
            ),
            (
                format!("/u/{TOKEN}/js/%2e%2e/%2e%2e/secret.js"),
                StatusCode::BAD_REQUEST,
            ),
            (
                format!("/u/{TOKEN}/index.html::$DATA"),
                StatusCode::BAD_REQUEST,
            ),
            (format!("/u/{TOKEN}/index.exe"), StatusCode::FORBIDDEN),
            (format!("/u/{TOKEN}/nope.js"), StatusCode::NOT_FOUND),
        ] {
            let resp = demo_router(reg.clone())
                .oneshot(get_req(&path, Some(&host), None))
                .await
                .unwrap();
            assert_eq!(resp.status(), status, "{path}");
            assert_security_headers(&resp);
        }
        std::fs::remove_dir_all(&base).ok();
    }

    #[tokio::test]
    async fn router_404_for_unregistered_token() {
        let base = temp_root("unreg");
        let root = base.join("demo");
        std::fs::write(root.join("index.html"), "x").unwrap();
        let reg = DemoRegistry::for_test(PORT);
        reg.register(TOKEN, &root);
        let host = format!("127.0.0.1:{PORT}");
        let other = "ffffffffffffffffffffffffffffffff";
        for path in [
            format!("/u/{other}/"),
            format!("/u/{other}/index.html"),
            format!("/u/{other}"),
            "/u/bad%0d%0atoken/".to_string(),
        ] {
            let resp = demo_router(reg.clone())
                .oneshot(get_req(&path, Some(&host), None))
                .await
                .unwrap();
            assert_eq!(resp.status(), StatusCode::NOT_FOUND, "{path}");
            assert_security_headers(&resp);
        }
        // 撤销登记后原 token 也是 404。
        reg.unregister(TOKEN);
        let resp = demo_router(reg.clone())
            .oneshot(get_req(&format!("/u/{TOKEN}/"), Some(&host), None))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
        std::fs::remove_dir_all(&base).ok();
    }

    /// 这个监听只服务 Demo:主服务的 API 路径一律 404(不代理、不转发)。
    #[tokio::test]
    async fn router_never_serves_api_paths() {
        let reg = DemoRegistry::for_test(PORT);
        let host = format!("127.0.0.1:{PORT}");
        for path in [
            "/api/forge/sessions",
            "/api/forge/mcp/call",
            "/health",
            "/",
            "/u/",
            "/index.html",
        ] {
            let resp = demo_router(reg.clone())
                .oneshot(get_req(path, Some(&host), None))
                .await
                .unwrap();
            assert_eq!(resp.status(), StatusCode::NOT_FOUND, "{path}");
            assert_security_headers(&resp);
        }
        // 非 GET 方法同样带安全头(405 由路由层给出)。
        let resp = demo_router(reg.clone())
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/u/{TOKEN}/"))
                    .header(header::HOST, &host)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::METHOD_NOT_ALLOWED);
        assert_security_headers(&resp);
    }

    #[tokio::test]
    async fn redirect_without_trailing_slash() {
        let base = temp_root("redirect");
        let root = base.join("demo");
        std::fs::write(root.join("index.html"), "x").unwrap();
        let reg = DemoRegistry::for_test(PORT);
        reg.register(TOKEN, &root);
        let host = format!("localhost:{PORT}");
        let resp = demo_router(reg.clone())
            .oneshot(get_req(&format!("/u/{TOKEN}"), Some(&host), None))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::PERMANENT_REDIRECT);
        assert_eq!(resp.headers()[header::LOCATION], format!("/u/{TOKEN}/"));
        assert_security_headers(&resp);
        // 坏 Host 不重定向。
        let resp = demo_router(reg.clone())
            .oneshot(get_req(&format!("/u/{TOKEN}"), Some("evil.example"), None))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);
        std::fs::remove_dir_all(&base).ok();
    }

    /// 端口已知(for_test 预置)时 ensure_started 不再绑定,直接返回。
    #[tokio::test]
    async fn ensure_started_is_idempotent_without_binding_in_tests() {
        let reg = DemoRegistry::for_test(PORT);
        assert_eq!(reg.ensure_started().await.unwrap(), PORT);
        assert_eq!(reg.port(), Some(PORT));
        assert_eq!(
            local_url(PORT, TOKEN),
            format!("http://127.0.0.1:{PORT}/u/{TOKEN}/")
        );
    }
}
