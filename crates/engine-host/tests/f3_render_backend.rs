//! 02 §4.6:render.backendInfo / render.capabilities 的线上格式(rurix 后端),经真实进程与 4 字节小端长度前缀帧协议。
//! 三态诚实:无 vulkan 设备时 viewport.frame 返回 DEV_ENV_DEGRADE,deviceName 那一段打印 SKIP 并通过。

use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpStream;
use std::process::{Child, Command, Stdio};
use std::time::Duration;

use serde_json::{json, Value};

struct HostProc(Child);

impl Drop for HostProc {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

/// spawn engine-host(--port 0),从 stdout 就绪行解析端口并连上。
fn spawn_host() -> (HostProc, TcpStream) {
    let mut child = Command::new(env!("CARGO_BIN_EXE_engine-host"))
        .args(["--port", "0"])
        .env_remove("FORGE_GPU_PARTICLES")
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn engine-host 失败");
    let mut line = String::new();
    BufReader::new(child.stdout.take().expect("无 stdout 管道")).read_line(&mut line).expect("读就绪行失败");
    let port: u16 = line
        .trim()
        .strip_prefix("FORGE_HOST_LISTENING port=")
        .and_then(|s| s.parse().ok())
        .unwrap_or_else(|| panic!("就绪行格式非法:{line:?}"));
    let stream = TcpStream::connect(("127.0.0.1", port)).expect("连接失败");
    stream.set_read_timeout(Some(Duration::from_secs(60))).unwrap();
    (HostProc(child), stream)
}

fn rpc(s: &mut TcpStream, method: &str, params: Option<Value>) -> Value {
    let mut req = json!({ "jsonrpc": "2.0", "id": 7, "method": method });
    if let Some(p) = params {
        req["params"] = p;
    }
    let body = serde_json::to_vec(&req).unwrap();
    s.write_all(&(body.len() as u32).to_le_bytes()).unwrap();
    s.write_all(&body).unwrap();
    let mut len = [0u8; 4];
    s.read_exact(&mut len).unwrap();
    let mut buf = vec![0u8; u32::from_le_bytes(len) as usize];
    s.read_exact(&mut buf).unwrap();
    serde_json::from_slice(&buf).unwrap()
}

fn result(v: Value) -> Value {
    assert!(v.get("error").is_none(), "不应报错:{v}");
    v["result"].clone()
}

fn expected_info(device: Value) -> Value {
    json!({
        "renderBackend": "rurix", "method": null, "driver": null, "source": "default", "ready": true,
        "deviceName": device,
        "versions": { "engineHost": env!("CARGO_PKG_VERSION"), "godot": null, "gdext": null },
    })
}

#[test]
fn backend_info_and_capabilities_wire_format() {
    let (_host, mut s) = spawn_host();
    let caps = json!({
        "renderBackend": "rurix",
        "pipelined": false,
        "legs": ["sprite_mesh", "model", "sentinels_v6"],
        "preview": true,
        "particles": false,
        "frameExits": { "cpuRgba8": true, "sharedD3d12": cfg!(windows), "zeroCopy": cfg!(windows) },
        "stats": { "nonzero": true, "triangles": true, "truncated": true, "meshFallbacks": true, "meshClasses": true },
        "maxDraws": { "spriteMesh": 256, "model": 2048, "sentinelsV6": 1536 },
        "maxSize": { "rpc": [1920, 1080], "stream": [1280, 720] },
    });
    // 不接受参数:缺省、{}、多余字段都返回同一结果。
    for params in [None, Some(json!({})), Some(json!({ "ignored": [1, 2] }))] {
        assert_eq!(result(rpc(&mut s, "render.backendInfo", params.clone())), expected_info(Value::Null));
        assert_eq!(result(rpc(&mut s, "render.capabilities", params)), caps);
    }
    let unknown = rpc(&mut s, "render.nope", Some(json!({})));
    assert_eq!(unknown["error"]["code"], -32601, "{unknown}");
    // 顶层键 renderBackend 与 host.ping 的物理后端键 backend 互不影响。
    let ping = result(rpc(&mut s, "host.ping", None));
    assert!(ping["backend"].is_string() && ping.get("renderBackend").is_none(), "{ping}");
}

#[test]
fn device_name_is_null_until_the_first_frame() {
    let (_host, mut s) = spawn_host();
    assert_eq!(result(rpc(&mut s, "render.backendInfo", None))["deviceName"], Value::Null);
    let frame = rpc(&mut s, "viewport.frame", Some(json!({ "width": 64, "height": 64 })));
    if let Some(e) = frame["error"]["message"].as_str() {
        assert!(e.contains("DEV_ENV_DEGRADE"), "非降级错误:{frame}");
        eprintln!("[f3_render_backend] SKIP DEV_ENV_DEGRADE: {e}");
        assert_eq!(result(rpc(&mut s, "render.backendInfo", None))["deviceName"], Value::Null, "无帧不得伪造设备名");
        return;
    }
    let device = frame["result"]["deviceName"].clone();
    assert!(device.as_str().is_some_and(|d| !d.is_empty()), "{frame}");
    assert_eq!(result(rpc(&mut s, "render.backendInfo", None)), expected_info(device.clone()));
    result(rpc(&mut s, "viewport.frame", Some(json!({ "width": 64, "height": 64 }))));
    assert_eq!(result(rpc(&mut s, "render.backendInfo", Some(json!({}))))["deviceName"], device, "首帧设备名不变");
}
