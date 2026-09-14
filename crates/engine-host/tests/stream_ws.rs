//! 视口直连推流通道集成测试:真实 spawn engine-host,经 RPC `viewport.streamInfo`
//! 拿直连地址后 tungstenite 客户端全链路断言:
//! - 坏 token 握手拒绝(403);
//! - subscribe → hello 回执 → 二进制帧(有 GPU)或 error 文本(诚实降级),两者必居其一;
//! - WS input 消息 → input_queue → 60Hz 逻辑帧 → 图解释器 on_input(events.drain 见 logic.input)。

use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpStream;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use serde_json::{json, Value};
use tungstenite::stream::MaybeTlsStream;
use tungstenite::Message;

struct HostProc {
    child: Child,
    port: u16,
}

impl Drop for HostProc {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// spawn engine-host(--port 0),从 stdout 就绪行解析实际端口。
fn spawn_host() -> HostProc {
    let exe = env!("CARGO_BIN_EXE_engine-host");
    let mut child = Command::new(exe)
        .args(["--port", "0"])
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn engine-host 失败");
    let stdout = child.stdout.take().expect("无 stdout 管道");
    let mut reader = BufReader::new(stdout);
    let mut line = String::new();
    reader.read_line(&mut line).expect("读就绪行失败");
    let port: u16 = line
        .trim()
        .strip_prefix("FORGE_HOST_LISTENING port=")
        .and_then(|s| s.parse().ok())
        .unwrap_or_else(|| panic!("就绪行格式非法:{line:?}"));
    HostProc { child, port }
}

struct Client {
    stream: TcpStream,
    next_id: u64,
}

impl Client {
    fn connect(port: u16) -> Self {
        let stream = TcpStream::connect(("127.0.0.1", port)).expect("连接失败");
        stream.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
        Client { stream, next_id: 1 }
    }

    fn call(&mut self, method: &str, params: Value) -> Value {
        let id = self.next_id;
        self.next_id += 1;
        let payload = serde_json::to_vec(&json!({
            "jsonrpc": "2.0", "id": id, "method": method, "params": params,
        }))
        .unwrap();
        let len = u32::try_from(payload.len()).unwrap();
        self.stream.write_all(&len.to_le_bytes()).unwrap();
        self.stream.write_all(&payload).unwrap();
        self.stream.flush().unwrap();
        let mut len_buf = [0u8; 4];
        self.stream.read_exact(&mut len_buf).unwrap();
        let mut buf = vec![0u8; u32::from_le_bytes(len_buf) as usize];
        self.stream.read_exact(&mut buf).unwrap();
        let resp: Value = serde_json::from_slice(&buf).unwrap();
        assert!(resp.get("error").is_none(), "{method} 不应报错:{resp}");
        resp["result"].clone()
    }
}

type Ws = tungstenite::WebSocket<MaybeTlsStream<TcpStream>>;

/// 经 RPC 取 wsUrl 并直连(handshake 内含 token 校验)。
fn connect_stream(rpc: &mut Client) -> (Ws, String) {
    let info = rpc.call("viewport.streamInfo", json!({}));
    let ws_url = info["wsUrl"].as_str().expect("应有 wsUrl").to_string();
    assert_eq!(info["proto"], 1);
    let (ws, _resp) = tungstenite::connect(ws_url.as_str()).expect("WS 直连失败");
    if let MaybeTlsStream::Plain(s) = ws.get_ref() {
        s.set_read_timeout(Some(Duration::from_millis(200))).unwrap();
    }
    (ws, ws_url)
}

fn is_idle(e: &tungstenite::Error) -> bool {
    matches!(
        e,
        tungstenite::Error::Io(io)
            if matches!(io.kind(), std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut)
    )
}

#[test]
fn bad_token_rejected_at_handshake() {
    let host = spawn_host();
    let mut rpc = Client::connect(host.port);
    let info = rpc.call("viewport.streamInfo", json!({}));
    let ws_url = info["wsUrl"].as_str().unwrap();
    let bad_url = format!("{}deadbeef", ws_url); // token 尾部污染 → 校验失败
    match tungstenite::connect(bad_url.as_str()) {
        Err(e) => {
            let msg = format!("{e}");
            assert!(
                msg.contains("403") || msg.to_lowercase().contains("forbidden"),
                "应为 403 拒绝,实为:{msg}"
            );
        }
        Ok(_) => panic!("坏 token 不应握手成功"),
    }
}

#[test]
fn subscribe_gets_hello_then_frame_or_honest_error() {
    let host = spawn_host();
    let mut rpc = Client::connect(host.port);
    let (mut ws, _url) = connect_stream(&mut rpc);
    ws.send(Message::Text(
        json!({ "type": "subscribe", "width": 64, "height": 64, "maxFps": 30 }).to_string(),
    ))
    .unwrap();

    let deadline = Instant::now() + Duration::from_secs(20);
    let mut saw_hello = false;
    let mut outcome: Option<String> = None;
    while Instant::now() < deadline && !(saw_hello && outcome.is_some()) {
        match ws.read() {
            Ok(Message::Text(t)) => {
                let v: Value = serde_json::from_str(&t).expect("文本消息应为 JSON");
                match v["type"].as_str() {
                    Some("hello") => {
                        assert_eq!(v["proto"], 1);
                        assert_eq!(v["width"], 64);
                        assert_eq!(v["height"], 64);
                        saw_hello = true;
                    }
                    Some("error") => {
                        // 诚实降级(无 GPU 环境):必须带原因,绝不伪造帧。
                        assert!(
                            v["message"].as_str().map(|s| !s.is_empty()).unwrap_or(false),
                            "error 消息须带原因"
                        );
                        outcome = Some("error".into());
                    }
                    _ => {} // status 等
                }
            }
            Ok(Message::Binary(b)) => {
                assert!(b.len() >= 20, "帧消息短于头长");
                assert_eq!(&b[0..4], b"FGF1", "帧魔数");
                let w = u16::from_le_bytes([b[8], b[9]]) as usize;
                let h = u16::from_le_bytes([b[10], b[11]]) as usize;
                assert_eq!((w, h), (64, 64), "帧尺寸应与订阅一致");
                assert_eq!(b.len(), 20 + w * h * 4, "负载长度 = 头 + 紧凑 RGBA8");
                outcome = Some("frame".into());
            }
            Ok(_) => {}
            Err(e) if is_idle(&e) => {}
            Err(e) => panic!("WS 读失败:{e}"),
        }
    }
    assert!(saw_hello, "订阅后 20s 内未收到 hello 回执");
    assert!(
        outcome.is_some(),
        "订阅后 20s 内既无二进制帧也无诚实 error 文本"
    );
}

#[test]
fn ws_input_reaches_graph_interpreter() {
    let host = spawn_host();
    let mut rpc = Client::connect(host.port);
    // 带 on_input 探针图的实体(f4w3_probe:on_input → debug.log;项目根 = projects/demo)。
    rpc.call(
        "entity.create",
        json!({
            "name": "probe",
            "components": [{
                "type": "Script",
                "props": { "module": "", "graphRef": "Content/Graphs/f4w3_probe.rxgraph", "props": {} }
            }],
        }),
    );
    rpc.call("play.enter", json!({}));
    let _ = rpc.call("events.drain", json!({})); // 清 play.enter 期间的事件噪声

    let (mut ws, _url) = connect_stream(&mut rpc);
    ws.send(Message::Text(
        json!({ "type": "subscribe", "width": 32, "height": 32, "maxFps": 10 }).to_string(),
    ))
    .unwrap();
    ws.send(Message::Text(
        json!({ "type": "input", "action": "left", "value": -1.0 }).to_string(),
    ))
    .unwrap();

    // WS 入队 → 60Hz 逻辑帧派发 on_input → 事件环出现 logic.input(action=left)。
    let deadline = Instant::now() + Duration::from_secs(5);
    let mut hit = false;
    while Instant::now() < deadline && !hit {
        let drained = rpc.call("events.drain", json!({}));
        if let Some(arr) = drained.as_array() {
            hit = arr.iter().any(|e| {
                e["event"] == "logic.input" && e["action"] == "left" && e["value"] == -1.0
            });
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    assert!(hit, "5s 内 WS 输入未到达图解释器(events.drain 无 logic.input)");
}
