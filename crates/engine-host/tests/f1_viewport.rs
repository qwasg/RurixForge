//! engine-host F1 wave.2 Viewport 集成测试(G-F1-6/7/8 引擎侧):
//! 真实 spawn 进程 + 长度前缀帧,经 JSON-RPC 驱动 viewport.frame/pick/setCamera。
//!
//! 三态诚实:无 vulkan 设备 → viewport.frame 返回 DEV_ENV_DEGRADE 错误,本测试打印
//! SKIP 并以通过退出(非 fake pass;设备真跑由 smoke/CI 裁决)。host 数学腿见
//! src/viewport.rs 内联单测(恒跑)。

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
        stream
            .set_read_timeout(Some(Duration::from_secs(30)))
            .unwrap();
        Client { stream, next_id: 1 }
    }

    /// 调用并取 result;RPC 错误原样 panic 出 message。
    fn call(&mut self, method: &str, params: Value) -> Value {
        match self.call_raw(method, params) {
            (Ok(v), _) => v,
            (Err(e), _) => panic!("{method} 失败:{e}"),
        }
    }

    /// 调用并区分 result/error。
    fn call_raw(&mut self, method: &str, params: Value) -> (Result<Value, String>, Value) {
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
            (Err(e.get("message").and_then(Value::as_str).unwrap_or("?").to_string()), resp.clone())
        } else {
            (Ok(resp.get("result").cloned().unwrap_or(Value::Null)), resp)
        }
    }
}

const W: u32 = 128;
const H: u32 = 96;
/// 与 viewport.rs CLEAR_RGBA 经 UNORM 就近取整一致。
const BG: [u8; 4] = [23, 24, 29, 255];

fn px(frame: &[u8], x: u32, y: u32) -> [u8; 4] {
    let i = ((y * W + x) * 4) as usize;
    [frame[i], frame[i + 1], frame[i + 2], frame[i + 3]]
}

fn frame_bytes(c: &mut Client, extra: Value) -> (Vec<u8>, Value) {
    let mut params = json!({ "width": W, "height": H });
    if let Value::Object(m) = extra {
        for (k, v) in m {
            params[k] = v;
        }
    }
    let r = c.call("viewport.frame", params);
    let b64 = r.get("pixelsB64").and_then(Value::as_str).expect("缺 pixelsB64");
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(b64)
        .expect("base64 解码失败");
    assert_eq!(bytes.len(), (W * H * 4) as usize, "帧字节数不符");
    (bytes, r)
}

#[test]
fn f1_w2_viewport_device_leg() {
    let host = spawn_host();
    let mut c = Client::connect(host.port);
    c.call("scene.new", json!({ "name": "vp" }));
    let created = c.call(
        "entity.create",
        json!({
            "name": "cube",
            "translation": [0.0, 0.5, 0.0],
            "components": [{ "type": "MeshRenderer", "props": { "mesh": "cube", "material": "m" } }]
        }),
    );
    let cube_id = created.get("id").and_then(Value::as_u64).expect("create 应回 id");

    // 三态门:首帧探设备。
    let (first, _) = c.call_raw("viewport.frame", json!({ "width": W, "height": H }));
    let first = match first {
        Ok(v) => v,
        Err(e) if e.contains("DEV_ENV_DEGRADE") => {
            eprintln!("[f1_viewport] SKIP DEV_ENV_DEGRADE: {e}");
            return;
        }
        Err(e) => panic!("viewport.frame 非降级错误:{e}"),
    };
    let dev = first.get("deviceName").and_then(Value::as_str).unwrap_or("");
    assert!(!dev.is_empty(), "deviceName 应非空");
    assert_eq!(first.get("draws").and_then(Value::as_u64), Some(1), "1 实体应 1 draw");
    eprintln!("[f1_viewport] device leg on: {dev}");

    // G-F1-6:锚点像素(中心非底/四角底色)+ 两帧逐字节一致 + 移动后帧变。
    let (f1, _) = frame_bytes(&mut c, json!({}));
    assert_ne!(px(&f1, W / 2, H / 2), BG, "中心应为立方体像素(非底色)");
    for (x, y) in [(1, 1), (W - 2, 1), (1, H - 2), (W - 2, H - 2)] {
        assert_eq!(px(&f1, x, y), BG, "四角应为底色 ({x},{y})");
    }
    let (f2, _) = frame_bytes(&mut c, json!({}));
    assert_eq!(f1, f2, "同场景两帧应逐字节一致(确定性)");

    c.call(
        "transform.set",
        json!({ "id": cube_id, "translation": [100.0, 0.5, 0.0] }),
    );
    let (f3, _) = frame_bytes(&mut c, json!({}));
    assert_ne!(f1, f3, "实体移出后帧应变化");
    assert_eq!(px(&f3, W / 2, H / 2), BG, "移走后中心应回底色");

    // G-F1-7:pick 命中/未命中(实体移回中央)。
    c.call(
        "transform.set",
        json!({ "id": cube_id, "translation": [0.0, 0.5, 0.0] }),
    );
    let pick = c.call(
        "viewport.pick",
        json!({ "x": (W / 2) as f32, "y": (H / 2) as f32, "width": W, "height": H }),
    );
    assert_eq!(pick.get("hit").and_then(Value::as_bool), Some(true), "中心应命中");
    assert_eq!(pick.get("entityId").and_then(Value::as_u64), Some(cube_id));
    let miss = c.call(
        "viewport.pick",
        json!({ "x": 2.0, "y": 2.0, "width": W, "height": H }),
    );
    assert_eq!(miss.get("hit").and_then(Value::as_bool), Some(false), "角落应未命中");

    // G-F1-8:相机 orbit 后帧变(同场景)。
    let cam0 = c.call("viewport.getCamera", json!({}));
    let yaw0 = cam0.get("yaw").and_then(Value::as_f64).unwrap();
    c.call("viewport.setCamera", json!({ "yaw": yaw0 + 90.0 }));
    let (f4, _) = frame_bytes(&mut c, json!({}));
    assert_ne!(f1, f4, "orbit 90° 后帧应变化");
    let cam1 = c.call("viewport.getCamera", json!({}));
    assert_eq!(cam1.get("yaw").and_then(Value::as_f64), Some(yaw0 + 90.0));
}
