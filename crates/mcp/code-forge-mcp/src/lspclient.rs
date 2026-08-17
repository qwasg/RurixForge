//! rurixc `--tooling-server` 常驻 LSP 会话(F4 wave.4;05 §4「code_* 走 rurixc LSP 会话(常驻)」)。
//!
//! 协议事实(以 H:\rurix\src\rurixc\src\tooling\lsp.rs 真实实现为准):
//! - framing:Content-Length 头 + 空行 + JSON-RPC body;上游写出用 `writeln!`(LF 行尾),
//!   本端读侧 \r\n / \n 兼容,写侧发标准 \r\n(上游 read_line + trim 可解)。
//! - 方法面:initialize / textDocument/didOpen(通知,回 publishDiagnostics 通知)/
//!   completion / definition / references / documentHighlight / rename;位置 0 基 line/character。
//! - ToolingSession 单文档语义(session.rs:每 uri 独立 analyze):references 结果限同文件。
//!
//! 常驻纪律(照 agentd mcp.rs 先例语义):单例 + Mutex;child.try_wait 崩溃检测 + 懒重连;
//! 每请求 10s 超时;超时/协议错 → 会话作废(Drop 杀进程),下次调用重连。
//! std-only:读帧跑专用线程经 mpsc 回投,请求侧 recv_timeout 实现超时。

use std::io::{BufRead, BufReader, Read, Write};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc;
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

use serde_json::{json, Value};

use crate::rxtool::{rurixc, TResult, ToolError};

/// 单请求超时(契约:超时/协议错 → 结构化错误,连接作废下次重连)。
const REQUEST_TIMEOUT: Duration = Duration::from_secs(10);

fn terr(code: &str, message: impl Into<String>) -> ToolError {
    ToolError { code: code.to_string(), message: message.into() }
}

/// 写一帧:`Content-Length: {字节数}\r\n\r\n{body}`(泛型 Writer,便于 Vec<u8> 单测)。
pub fn write_frame<W: Write>(w: &mut W, body: &str) -> std::io::Result<()> {
    write!(w, "Content-Length: {}\r\n\r\n", body.len())?;
    w.write_all(body.as_bytes())?;
    w.flush()
}

/// 读一帧:头部逐行至空行(兼容 \n / \r\n),取 Content-Length(大小写不敏感,照上游);
/// 首行即 EOF → Ok(None)(干净关闭);帧中 EOF / 缺头 → Err。
pub fn read_frame<R: BufRead>(r: &mut R) -> std::io::Result<Option<String>> {
    let mut len: Option<usize> = None;
    let mut first = true;
    loop {
        let mut line = String::new();
        let n = r.read_line(&mut line)?;
        if n == 0 {
            if first {
                return Ok(None);
            }
            return Err(std::io::Error::new(
                std::io::ErrorKind::UnexpectedEof,
                "帧头/帧体间 EOF",
            ));
        }
        first = false;
        let t = line.trim();
        if t.is_empty() {
            break;
        }
        if let Some(v) = t.to_lowercase().strip_prefix("content-length:") {
            len = v.trim().parse().ok();
        }
    }
    let len = len.ok_or_else(|| {
        std::io::Error::new(std::io::ErrorKind::InvalidData, "帧头缺 Content-Length")
    })?;
    let mut body = vec![0u8; len];
    r.read_exact(&mut body)?;
    let text = String::from_utf8(body)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, format!("帧体非 UTF-8: {e}")))?;
    Ok(Some(text))
}

/// 读帧线程回投:Msg = 解析成功的 JSON-RPC 消息;Gone = EOF/协议错/JSON 解析失败(会话终)。
enum Frame {
    Msg(Value),
    Gone(String),
}

/// 一条 LSP 连接:子进程 + stdin + 读帧线程通道 + 自增 id。
struct LspConn {
    child: Child,
    stdin: ChildStdin,
    frames: mpsc::Receiver<Frame>,
    next_id: u64,
}

impl LspConn {
    /// spawn rurixc --tooling-server(stdin/stdout piped,stderr null)+ initialize 握手。
    fn spawn() -> TResult<Self> {
        let bin = rurixc()?;
        let mut child = Command::new(&bin)
            .arg("--tooling-server")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| terr("LSP_SPAWN", format!("spawn {} --tooling-server 失败: {e}", bin.display())))?;
        let stdin = child.stdin.take().ok_or_else(|| terr("LSP_SPAWN", "子进程无 stdin"))?;
        let stdout = child.stdout.take().ok_or_else(|| terr("LSP_SPAWN", "子进程无 stdout"))?;
        let (tx, rx) = mpsc::channel::<Frame>();
        std::thread::spawn(move || {
            let mut r = BufReader::new(stdout);
            loop {
                let frame = match read_frame(&mut r) {
                    Ok(Some(body)) => match serde_json::from_str::<Value>(&body) {
                        Ok(v) => Frame::Msg(v),
                        Err(e) => Frame::Gone(format!("LSP 帧 JSON 解析失败: {e};body={body}")),
                    },
                    Ok(None) => Frame::Gone("LSP stdout EOF(对端退出)".to_string()),
                    Err(e) => Frame::Gone(format!("LSP 帧读取失败: {e}")),
                };
                let terminal = matches!(frame, Frame::Gone(_));
                if tx.send(frame).is_err() || terminal {
                    return;
                }
            }
        });
        let mut conn = LspConn { child, stdin, frames: rx, next_id: 0 };
        // initialize 握手(上游回 capabilities;参数面照标准 LSP 最小集)。
        let result = conn.request(
            "initialize",
            json!({
                "protocolVersion": "2024-11-05",
                "capabilities": {},
                "clientInfo": { "name": "code-forge-mcp", "version": env!("CARGO_PKG_VERSION") }
            }),
        )?;
        if result.get("capabilities").is_none() {
            return Err(terr("LSP_PROTO", format!("initialize 响应缺 capabilities: {result}")));
        }
        Ok(conn)
    }

    fn send_json(&mut self, v: &Value) -> TResult<()> {
        let body = serde_json::to_string(v).map_err(|e| terr("LSP_IO", format!("序列化失败: {e}")))?;
        write_frame(&mut self.stdin, &body).map_err(|e| terr("LSP_IO", format!("写 LSP stdin 失败: {e}")))
    }

    /// 发请求并按 id 配对读响应(跳过 publishDiagnostics 等通知与其他 id)。
    fn request(&mut self, method: &str, params: Value) -> TResult<Value> {
        self.next_id += 1;
        let id = self.next_id;
        self.send_json(&json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params }))?;
        loop {
            match self.frames.recv_timeout(REQUEST_TIMEOUT) {
                Ok(Frame::Msg(v)) => {
                    if v.get("id").and_then(Value::as_u64) != Some(id) {
                        continue; // 通知/异 id:跳过
                    }
                    if let Some(e) = v.get("error") {
                        return Err(terr("LSP_RPC", format!("{method} JSON-RPC 错误: {e}")));
                    }
                    return v
                        .get("result")
                        .cloned()
                        .ok_or_else(|| terr("LSP_PROTO", format!("{method} 响应缺 result: {v}")));
                }
                Ok(Frame::Gone(m)) => return Err(terr("LSP_GONE", m)),
                Err(mpsc::RecvTimeoutError::Timeout) => {
                    return Err(terr(
                        "LSP_TIMEOUT",
                        format!("{method} 超时({}ms)", REQUEST_TIMEOUT.as_millis()),
                    ));
                }
                Err(mpsc::RecvTimeoutError::Disconnected) => {
                    return Err(terr("LSP_GONE", "LSP 帧读线程断开"))
                }
            }
        }
    }

    /// didOpen 通知(上游回 publishDiagnostics 通知,由后续 request 读循环跳过)。
    /// 同一 uri 重复 open = 全量替换(上游 session.open 直接 insert 覆盖)。
    fn did_open(&mut self, uri: &str, text: &str) -> TResult<()> {
        self.send_json(&json!({
            "jsonrpc": "2.0",
            "method": "textDocument/didOpen",
            "params": { "textDocument": { "uri": uri, "text": text, "version": 1 } }
        }))
    }
}

impl Drop for LspConn {
    /// std::process::Child drop 不杀进程 —— 显式 kill + wait(会话作废即终结子进程)。
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// 全局常驻会话(OnceLock + Mutex;None = 未连接或已作废)。
static LSP: OnceLock<Mutex<Option<LspConn>>> = OnceLock::new();

/// 在常驻 LSP 会话上执行闭包:无连接/进程已退 → 懒重连;闭包出错 → 会话作废(下次重连)。
pub fn with_lsp<T>(f: impl FnOnce(&mut LspConn) -> TResult<T>) -> TResult<T> {
    let slot = LSP.get_or_init(|| Mutex::new(None));
    let mut guard = slot.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(c) = guard.as_mut() {
        match c.child.try_wait() {
            Ok(None) => {}
            _ => *guard = None, // 已退出/状态异常 → 作废重连
        }
    }
    if guard.is_none() {
        *guard = Some(LspConn::spawn()?);
    }
    let conn = guard.as_mut().expect("spawn 后必有连接");
    match f(conn) {
        Ok(v) => Ok(v),
        Err(e) => {
            *guard = None; // 协议/超时/IO 错 → 会话作废(Drop 杀进程),下次重连
            Err(e)
        }
    }
}

/// 对外最小面:didOpen + references(供 codetool;闭包形态留在本模块内)。
pub fn references_at(uri: &str, text: &str, line: u32, character: u32) -> TResult<Value> {
    with_lsp(|c| {
        c.did_open(uri, text)?;
        c.request(
            "textDocument/references",
            json!({
                "textDocument": { "uri": uri },
                "position": { "line": line, "character": character }
            }),
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn frame_roundtrip_crlf() {
        let body = r#"{"jsonrpc":"2.0","id":1,"result":{"ok":true}}"#;
        let mut buf = Vec::new();
        write_frame(&mut buf, body).unwrap();
        let expect = format!("Content-Length: {}\r\n\r\n{}", body.len(), body);
        assert_eq!(String::from_utf8(buf.clone()).unwrap(), expect);
        let got = read_frame(&mut Cursor::new(buf)).unwrap().unwrap();
        assert_eq!(got, body);
    }

    #[test]
    fn frame_read_lf_only_headers() {
        // 上游 write_message 用 writeln!(LF 行尾):读侧必须兼容。
        let body = "{\"a\":\"中文字段\"}";
        let wire = format!("Content-Length: {}\n\n{}", body.len(), body);
        let got = read_frame(&mut Cursor::new(wire.into_bytes())).unwrap().unwrap();
        assert_eq!(got, body);
    }

    #[test]
    fn frame_read_two_frames_sequential() {
        let mut buf = Vec::new();
        write_frame(&mut buf, "{\"id\":1}").unwrap();
        write_frame(&mut buf, "{\"id\":2}").unwrap();
        let mut c = Cursor::new(buf);
        assert_eq!(read_frame(&mut c).unwrap().unwrap(), "{\"id\":1}");
        assert_eq!(read_frame(&mut c).unwrap().unwrap(), "{\"id\":2}");
        assert_eq!(read_frame(&mut c).unwrap(), None); // 干净 EOF
    }

    #[test]
    fn frame_read_missing_content_length_is_err() {
        let wire = b"Content-Type: x\r\n\r\n{}";
        let r = read_frame(&mut Cursor::new(wire.to_vec()));
        assert!(r.is_err());
    }

    #[test]
    fn frame_read_truncated_body_is_err() {
        let wire = b"Content-Length: 10\r\n\r\n{}";
        let r = read_frame(&mut Cursor::new(wire.to_vec()));
        assert!(r.is_err());
    }

    /// 真 LSP 集成:rurixc 缺失时跳过(照 agentd 集成测试 SKIP 先例)。
    #[test]
    fn lsp_references_real_server() {
        if rurixc().is_err() {
            eprintln!("[SKIP] rurixc 不存在,LSP 集成测试跳过");
            return;
        }
        let src = "fn helper() {}\n\nfn main() {\n    helper();\n}\n";
        let v = references_at("file:///f4w4-ut.rx", src, 0, 3).unwrap();
        let arr = v.as_array().unwrap();
        assert!(arr.len() >= 2, "定义 + 调用点须 >=2 refs: {v}");
        assert!(arr.iter().any(|l| l["range"]["start"]["line"] == 3), "须含调用点(line 3): {v}");
    }
}
