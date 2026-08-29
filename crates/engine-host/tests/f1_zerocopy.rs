//! engine-host F1 wave.3 零拷贝集成测试(G-F1-10/G-F1-11 引擎侧):
//! 真实 spawn 进程 + JSON-RPC;share_open 目标 = 自进程(DuplicateHandle 自身合法)。
//!
//! 三态诚实:无 vulkan 设备 → viewport.frame 返回 DEV_ENV_DEGRADE,打印 SKIP 通过退出。
//! zero_copy 档要求设备有 VK_KHR_external_memory_win32;无扩展时引擎回退 readback_upload,
//! 本测试如实 FAIL(不充绿)——RTX 4070 Ti 实测扩展在位。

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
        match self.call_raw(method, params) {
            (Ok(v), _) => v,
            (Err(e), _) => panic!("{method} 失败:{e}"),
        }
    }

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
const BG: [u8; 4] = [23, 24, 29, 255];

fn center_px(frame: &[u8], w: u32, h: u32) -> [u8; 4] {
    let x = w / 2;
    let y = h / 2;
    let i = ((y * w + x) * 4) as usize;
    [frame[i], frame[i + 1], frame[i + 2], frame[i + 3]]
}

fn frame_bytes(c: &mut Client, w: u32, h: u32) -> (Vec<u8>, Value) {
    let r = c.call("viewport.frame", json!({ "width": w, "height": h }));
    let b64 = r.get("pixelsB64").and_then(Value::as_str).expect("缺 pixelsB64");
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(b64)
        .expect("base64 解码失败");
    assert_eq!(bytes.len(), (w * h * 4) as usize, "帧字节数不符");
    (bytes, r)
}

#[test]
fn f1_w3_zero_copy_leg() {
    let host = spawn_host();
    let mut c = Client::connect(host.port);
    c.call("scene.new", json!({ "name": "zc" }));
    c.call(
        "entity.create",
        json!({
            "name": "cube",
            "translation": [0.0, 0.5, 0.0],
            "components": [{ "type": "MeshRenderer", "props": { "mesh": "cube", "material": "m" } }]
        }),
    );

    // 设备门:首帧探设备(share 未开 → no_share 档)。
    let (first, _) = c.call_raw("viewport.frame", json!({ "width": W, "height": H }));
    let first = match first {
        Ok(v) => v,
        Err(e) if e.contains("DEV_ENV_DEGRADE") => {
            eprintln!("[f1_zerocopy] SKIP DEV_ENV_DEGRADE: {e}");
            return;
        }
        Err(e) => panic!("viewport.frame 非降级错误:{e}"),
    };
    assert_eq!(
        first.get("framePath").and_then(Value::as_str),
        Some("no_share"),
        "share 未开应为 no_share 档"
    );

    // share_open 目标 = 自进程(DuplicateHandle 自身合法)。
    let self_pid = std::process::id();
    c.call(
        "viewport.shareOpen",
        json!({ "pid": self_pid, "width": W, "height": H }),
    );

    // G-F1-10:零拷贝档 — framePath=zero_copy 且 cpuUploads 零增量。
    let (f1, r1) = frame_bytes(&mut c, W, H);
    let path1 = r1.get("framePath").and_then(Value::as_str).unwrap_or("?");
    assert_eq!(
        path1, "zero_copy",
        "share 开启后应零拷贝直渲(无 external memory 扩展则如实 FAIL,不充绿)"
    );
    assert_eq!(r1.get("cpuUploads").and_then(Value::as_u64), Some(0));
    assert_ne!(center_px(&f1, W, H), BG, "零拷贝帧中心应为立方体像素(非底色)");

    // 确定性 + 计数器冻结。
    let (f2, r2) = frame_bytes(&mut c, W, H);
    assert_eq!(f1, f2, "零拷贝同场景两帧应逐字节一致");
    assert_eq!(r2.get("cpuUploads").and_then(Value::as_u64), Some(0));

    // 移动实体 → 帧变(同一帧源语义不变)。
    let list = c.call("entity.list", json!({}));
    let cube_id = list["entities"][0]["id"].as_u64().expect("缺实体 id");
    c.call(
        "transform.set",
        json!({ "id": cube_id, "translation": [100.0, 0.5, 0.0] }),
    );
    let (f3, _) = frame_bytes(&mut c, W, H);
    assert_ne!(f1, f3, "实体移出后零拷贝帧应变化");
    c.call(
        "transform.set",
        json!({ "id": cube_id, "translation": [0.0, 0.5, 0.0] }),
    );

    // G-F1-11:尺寸变化 → share 重建 + import 重建无错。
    c.call("viewport.shareClose", json!({}));
    c.call(
        "viewport.shareOpen",
        json!({ "pid": self_pid, "width": 64, "height": 64 }),
    );
    let (f4, r4) = frame_bytes(&mut c, 64, 64);
    assert_eq!(
        r4.get("framePath").and_then(Value::as_str),
        Some("zero_copy"),
        "重建后仍应零拷贝档"
    );
    assert_ne!(center_px(&f4, 64, 64), BG, "重建后帧中心非底色");

    // 关闭幂等 + 回退档复原。
    c.call("viewport.shareClose", json!({}));
    c.call("viewport.shareClose", json!({}));
    let (_, r5) = frame_bytes(&mut c, W, H);
    assert_eq!(
        r5.get("framePath").and_then(Value::as_str),
        Some("no_share"),
        "share 关闭后应回 no_share 档"
    );

    // 句柄生命周期:open/close ×10 循环无错(ShareTex::Drop 统一收口)。
    for _ in 0..10 {
        c.call(
            "viewport.shareOpen",
            json!({ "pid": self_pid, "width": W, "height": H }),
        );
        c.call("viewport.shareClose", json!({}));
    }
    c.call("viewport.shareClose", json!({})); // 尾幂等
    eprintln!("[f1_zerocopy] zero_copy 全链 PASS(确定性/重建/幂等/×10 开合)");
}

/// G-F1-11 编辑器实尺回归:960x540(桌面视口实尺)零拷贝首帧。
/// 2026-08-17 实测:该尺寸下 import 会话曾静默失败回退 readback_upload
/// (128x96 小尺寸正常),本测试锁死实尺行为。
#[test]
fn f1_w3_zero_copy_editor_size_leg() {
    const EW: u32 = 960;
    const EH: u32 = 540;
    let host = spawn_host();
    let mut c = Client::connect(host.port);
    c.call("scene.new", json!({ "name": "zc-large" }));
    c.call(
        "entity.create",
        json!({
            "name": "cube",
            "translation": [0.0, 0.5, 0.0],
            "components": [{ "type": "MeshRenderer", "props": { "mesh": "cube", "material": "m" } }]
        }),
    );

    let (first, _) = c.call_raw("viewport.frame", json!({ "width": EW, "height": EH }));
    match first {
        Ok(v) => assert_eq!(
            v.get("framePath").and_then(Value::as_str),
            Some("no_share"),
            "share 未开应为 no_share 档"
        ),
        Err(e) if e.contains("DEV_ENV_DEGRADE") => {
            eprintln!("[f1_zerocopy] SKIP DEV_ENV_DEGRADE: {e}");
            return;
        }
        Err(e) => panic!("viewport.frame 非降级错误:{e}"),
    }

    let self_pid = std::process::id();
    c.call(
        "viewport.shareOpen",
        json!({ "pid": self_pid, "width": EW, "height": EH }),
    );
    let (f1, r1) = frame_bytes(&mut c, EW, EH);
    assert_eq!(
        r1.get("framePath").and_then(Value::as_str),
        Some("zero_copy"),
        "编辑器实尺(960x540)应零拷贝直渲"
    );
    assert_eq!(r1.get("cpuUploads").and_then(Value::as_u64), Some(0));
    assert_ne!(center_px(&f1, EW, EH), BG, "实尺零拷贝帧中心应为立方体像素");
    eprintln!("[f1_zerocopy] 960x540 实尺零拷贝 PASS");
}

// RD-F1-003 的 `probe_image_mem_req` 探针随共享体由纹理改为 buffer 一并退役:
// 线性 buffer 两侧字节数逐字一致,不再需要「VK 图像需求 vs D3D12 分配」对账。
// 共享体的行距/尺寸契约改由 share.rs 内联单测 shared_layout_contract 看守。
