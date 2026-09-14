//! engine-host F1 wave.4 H.264 流腿集成测试(G-F1-13):
//! 真实 spawn 进程 + JSON-RPC;format=h264 返回 Annex B 码流(起始码/SPS/PPS/IDR 断言),
//! rgba8 档 0-byte 回归。三态诚实:无 vulkan 设备 → viewport.frame 返回 DEV_ENV_DEGRADE,SKIP。

use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpStream;
use std::process::{Child, Command, Stdio};
use std::time::Duration;

use base64::Engine as _;
use serde_json::{json, Value};

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

fn spawn_host() -> HostProc {
    let exe = env!("CARGO_BIN_EXE_engine-host");
    let mut child = Command::new(exe)
        .args(["--port", "0"])
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
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
        stream
            .set_read_timeout(Some(Duration::from_secs(30)))
            .unwrap();
        Client { stream, next_id: 1 }
    }

    fn call(&mut self, method: &str, params: Value) -> Value {
        let id = self.next_id;
        self.next_id += 1;
        let req = json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params });
        let payload = serde_json::to_vec(&req).unwrap();
        let len = u32::try_from(payload.len()).unwrap();
        self.stream.write_all(&len.to_le_bytes()).unwrap();
        self.stream.write_all(&payload).unwrap();
        self.stream.flush().unwrap();
        let mut len_buf = [0u8; 4];
        self.stream.read_exact(&mut len_buf).unwrap();
        let n = u32::from_le_bytes(len_buf) as usize;
        let mut buf = vec![0u8; n];
        self.stream.read_exact(&mut buf).unwrap();
        let resp: Value = serde_json::from_slice(&buf).unwrap();
        if let Some(e) = resp.get("error") {
            panic!("{method} 失败:{}", e.get("message").and_then(Value::as_str).unwrap_or("?"));
        }
        resp.get("result").cloned().unwrap_or(Value::Null)
    }

    fn call_raw(&mut self, method: &str, params: Value) -> Result<Value, String> {
        let id = self.next_id;
        self.next_id += 1;
        let req = json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params });
        let payload = serde_json::to_vec(&req).unwrap();
        let len = u32::try_from(payload.len()).unwrap();
        self.stream.write_all(&len.to_le_bytes()).unwrap();
        self.stream.write_all(&payload).unwrap();
        self.stream.flush().unwrap();
        let mut len_buf = [0u8; 4];
        self.stream.read_exact(&mut len_buf).unwrap();
        let n = u32::from_le_bytes(len_buf) as usize;
        let mut buf = vec![0u8; n];
        self.stream.read_exact(&mut buf).unwrap();
        let resp: Value = serde_json::from_slice(&buf).unwrap();
        if let Some(e) = resp.get("error") {
            Err(e.get("message").and_then(Value::as_str).unwrap_or("?").to_string())
        } else {
            Ok(resp.get("result").cloned().unwrap_or(Value::Null))
        }
    }
}

const W: u32 = 320;
const H: u32 = 240;

#[test]
fn f1_w4_h264_stream_leg() {
    let host = spawn_host();
    let mut c = Client::connect(host.port);
    c.call("scene.new", json!({ "name": "h264" }));
    c.call(
        "entity.create",
        json!({
            "name": "cube",
            "translation": [0.0, 0.5, 0.0],
            "components": [{ "type": "MeshRenderer", "props": { "mesh": "cube", "material": "m" } }]
        }),
    );

    // 设备门:首帧探设备(h264 档)。
    let first = c.call_raw("viewport.frame", json!({ "width": W, "height": H, "format": "h264" }));
    let first = match first {
        Ok(v) => v,
        Err(e) if e.contains("DEV_ENV_DEGRADE") => {
            eprintln!("[f1_h264] SKIP DEV_ENV_DEGRADE: {e}");
            return;
        }
        Err(e) => panic!("viewport.frame(h264) 非降级错误:{e}"),
    };

    // G-F1-13:Annex B 起始码 + SPS/PPS/IDR + 关键帧标记。
    assert_eq!(first.get("format").and_then(Value::as_str), Some("h264"));
    let nal_b64 = first.get("nalB64").and_then(Value::as_str).expect("缺 nalB64");
    let nal = base64::engine::general_purpose::STANDARD.decode(nal_b64).expect("base64 解码失败");
    assert!(!nal.is_empty(), "h264 码流为空");
    assert_eq!(first.get("keyframe").and_then(Value::as_bool), Some(true), "首帧应为关键帧");
    // Annex B 起始码 00 00 00 01。
    let has_start_code = nal.windows(4).any(|w| w == [0u8, 0, 0, 1]);
    assert!(has_start_code, "缺 Annex B 起始码");
    // SPS (NAL type 7) / PPS (NAL type 8) / IDR (NAL type 5)。
    let nal_types: Vec<u8> = nal
        .windows(4)
        .enumerate()
        .filter(|(_, w)| *w == [0u8, 0, 0, 1])
        .filter_map(|(i, _)| nal.get(i + 4).map(|b| b & 0x1F))
        .collect();
    assert!(nal_types.contains(&7), "缺 SPS(NAL type 7),实际: {nal_types:?}");
    assert!(nal_types.contains(&8), "缺 PPS(NAL type 8),实际: {nal_types:?}");
    assert!(nal_types.contains(&5), "缺 IDR(NAL type 5),实际: {nal_types:?}");

    // 第 2 帧非关键帧,码流非空。
    let second = c.call("viewport.frame", json!({ "width": W, "height": H, "format": "h264" }));
    assert_eq!(second.get("keyframe").and_then(Value::as_bool), Some(false), "第 2 帧不应为关键帧");
    let nal2 = base64::engine::general_purpose::STANDARD
        .decode(second.get("nalB64").and_then(Value::as_str).unwrap())
        .unwrap();
    assert!(!nal2.is_empty(), "第 2 帧码流为空");

    // rgba8 档 0-byte 回归。
    let rgba = c.call("viewport.frame", json!({ "width": W, "height": H }));
    assert_eq!(rgba.get("format").and_then(Value::as_str), Some("rgba8"));
    assert!(rgba.get("pixelsB64").and_then(Value::as_str).is_some(), "rgba8 档缺 pixelsB64");

    eprintln!("[f1_h264] H.264 流腿 PASS(Annex B/SPS/PPS/IDR/关键帧周期/rgba8 回归)");
}
