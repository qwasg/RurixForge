//! engine-host 集成测试:真实 spawn 进程、真实 bind 127.0.0.1:0(随机端口),
//! 按 4 字节小端长度前缀帧协议全链路断言。

use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpStream;
use std::process::{Child, Command, Stdio};
use std::time::Duration;

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
    assert!(port != 0, "应拿到 OS 分配的随机端口");
    HostProc { child, port }
}

/// 帧协议客户端:写请求、读响应、断言无 error,返回 result。
struct Client {
    stream: TcpStream,
    next_id: u64,
}

impl Client {
    fn connect(port: u16) -> Self {
        let stream = TcpStream::connect(("127.0.0.1", port)).expect("连接失败");
        stream
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        Client {
            stream,
            next_id: 1,
        }
    }

    fn write_frame(&mut self, v: &Value) {
        let payload = serde_json::to_vec(v).unwrap();
        let len = u32::try_from(payload.len()).unwrap();
        self.stream.write_all(&len.to_le_bytes()).unwrap();
        self.stream.write_all(&payload).unwrap();
        self.stream.flush().unwrap();
    }

    fn read_frame(&mut self) -> Value {
        let mut len_buf = [0u8; 4];
        self.stream.read_exact(&mut len_buf).unwrap();
        let len = u32::from_le_bytes(len_buf) as usize;
        let mut buf = vec![0u8; len];
        self.stream.read_exact(&mut buf).unwrap();
        serde_json::from_slice(&buf).unwrap()
    }

    /// 正常调用:断言 result 存在且无 error。
    fn call(&mut self, method: &str, params: Value) -> Value {
        let id = self.next_id;
        self.next_id += 1;
        self.write_frame(&json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": method,
            "params": params,
        }));
        let resp = self.read_frame();
        assert_eq!(resp["id"], id, "响应 id 须回显");
        assert!(resp.get("error").is_none(), "{method} 不应报错:{}", resp);
        resp["result"].clone()
    }

    /// 错误调用:返回 error.code。
    fn call_err(&mut self, method: &str, params: Value) -> i64 {
        let id = self.next_id;
        self.next_id += 1;
        self.write_frame(&json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": method,
            "params": params,
        }));
        let resp = self.read_frame();
        resp["error"]["code"].as_i64().expect("应有 error.code")
    }
}

#[test]
fn full_rpc_chain_over_length_prefixed_frames() {
    let host = spawn_host();
    let mut c = Client::connect(host.port);

    // host.ping
    let ping = c.call("host.ping", json!({}));
    assert_eq!(ping["pong"], true);
    assert_eq!(ping["version"], "0.1.0");
    assert!(ping["uptimeSec"].as_u64().is_some());
    assert!(
        ["jolt", "rapier"].contains(&ping["backend"].as_str().unwrap()),
        "backend 须为真实后端名:{}",
        ping["backend"]
    );

    // scene.new(默认名)
    let s0 = c.call("scene.new", json!({}));
    assert_eq!(s0["entityCount"], 0);

    // scene.new(自定义名)
    let s1 = c.call("scene.new", json!({"name": "集成测试场景"}));
    assert_eq!(s1["name"], "集成测试场景");
    assert_eq!(s1["entityCount"], 0);

    // scene.summary
    let sum = c.call("scene.summary", json!(null));
    assert_eq!(sum["name"], "集成测试场景");
    assert_eq!(sum["entityCount"], 0);
    assert_eq!(sum["physics"]["backend"], ping["backend"]);
    assert!((sum["physics"]["dtFixed"].as_f64().unwrap() - 1.0 / 60.0).abs() < 1e-9);
    assert!(sum["physics"]["steps"].as_u64().is_some());
    assert_eq!(sum["render"]["frames"], 0);
    assert!(sum["events"].as_u64().unwrap() >= 2, "至少有启动+两次建场景事件");

    // render.once ×2:frames 单调增,非零像素 > 0
    let r1 = c.call("render.once", json!({}));
    assert_eq!(r1["frames"], 1);
    assert_eq!(r1["tris"], 1);
    assert!(r1["nonZeroPixels"].as_u64().unwrap() > 0, "三角形应有非零像素");
    let r2 = c.call("render.once", json!({}));
    assert_eq!(r2["frames"], 2);

    // events.drain:含 host.started / scene.created / render.frame,且 drain 后清空
    let evs = c.call("events.drain", json!({}));
    let names: Vec<&str> = evs
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|e| e["event"].as_str())
        .collect();
    assert!(names.contains(&"host.started"), "缺 host.started:{names:?}");
    assert_eq!(names.iter().filter(|n| **n == "scene.created").count(), 2);
    assert_eq!(names.iter().filter(|n| **n == "render.frame").count(), 2);
    for e in evs.as_array().unwrap() {
        assert!(e["ts"].as_str().unwrap().ends_with('Z'), "事件须带 UTC ISO8601 ts");
    }
    let evs2 = c.call("events.drain", json!({}));
    assert_eq!(evs2.as_array().unwrap().len(), 0, "drain 须清空 ring");

    // 错误面:未知方法 -32601;坏参数 -32602;坏 JSON -32700
    assert_eq!(c.call_err("no.such.method", json!({})), -32601);
    assert_eq!(c.call_err("scene.new", json!({"name": 123})), -32602);
    let bad = b"{ not json ]";
    let len = u32::try_from(bad.len()).unwrap();
    c.stream.write_all(&len.to_le_bytes()).unwrap();
    c.stream.write_all(bad).unwrap();
    c.stream.flush().unwrap();
    let resp = c.read_frame();
    assert_eq!(resp["error"]["code"], -32700, "坏 JSON 须回 -32700");

    // 连接在 -32700 后仍可用
    let ping2 = c.call("host.ping", json!({}));
    assert_eq!(ping2["pong"], true);
}
