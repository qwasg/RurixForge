//! engine-scene-mcp 集成测试:真实 spawn MCP(autoStart engine-host)→
//! scene_summary 断言 → taskkill 强杀 host → 10s 内断言 host.crashed 与
//! host.restarted 落盘且 scene_summary 恢复可用。

use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use std::process::{Child, ChildStdin, Command, Stdio};
use std::time::{Duration, Instant};

use serde_json::{json, Value};

/// workspace 根:测试 crate CARGO_MANIFEST_DIR(…/crates/mcp/engine-scene-mcp)上三级。
fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(3)
        .unwrap()
        .to_path_buf()
}

fn host_events_path() -> PathBuf {
    workspace_root().join("data").join("host-events.jsonl")
}

struct McpProc {
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<std::process::ChildStdout>,
    next_id: u64,
}

impl Drop for McpProc {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

impl McpProc {
    fn spawn() -> Self {
        let exe = env!("CARGO_BIN_EXE_engine-scene-mcp");
        let host_bin = workspace_root().join("target").join("debug").join("engine-host.exe");
        assert!(
            host_bin.exists(),
            "engine-host.exe 不存在({});请先 cargo build --workspace",
            host_bin.display()
        );
        let mut child = Command::new(exe)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .expect("spawn engine-scene-mcp 失败");
        let stdin = child.stdin.take().unwrap();
        let stdout = BufReader::new(child.stdout.take().unwrap());
        McpProc {
            child,
            stdin,
            stdout,
            next_id: 1,
        }
    }

    /// 发一行请求,读一行响应,断言无协议层 error,返回 result。
    fn request(&mut self, method: &str, params: Value) -> Value {
        let id = self.next_id;
        self.next_id += 1;
        let req = json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params });
        writeln!(self.stdin, "{}", serde_json::to_string(&req).unwrap()).unwrap();
        self.stdin.flush().unwrap();
        let mut line = String::new();
        self.stdout.read_line(&mut line).expect("读 MCP 响应失败");
        let resp: Value = serde_json::from_str(line.trim()).expect("MCP 响应须为 JSON");
        assert_eq!(resp["id"], id, "响应 id 须回显");
        assert!(resp.get("error").is_none(), "{method} 协议层不应报错:{resp}");
        resp["result"].clone()
    }

    /// 调工具并把 content[0].text 解析为 JSON;断言非 isError。
    fn call_tool(&mut self, name: &str, args: Value) -> Value {
        let result = self.request("tools/call", json!({ "name": name, "arguments": args }));
        assert!(
            result.get("isError").is_none(),
            "工具 {name} 不应 isError:{result}"
        );
        let text = result["content"][0]["text"].as_str().expect("缺 content text");
        serde_json::from_str(text).expect("工具结果 text 须为 JSON")
    }
}

/// taskkill 强杀指定 pid。
fn kill_pid(pid: u64) {
    let status = Command::new("taskkill")
        .args(["/PID", &pid.to_string(), "/F"])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .expect("taskkill 调用失败");
    assert!(status.success(), "taskkill /PID {pid} /F 应成功");
}

#[test]
fn watchdog_restarts_host_after_kill() {
    // 记录测试前日志内容,只断言本测试新增的 crashed/restarted 行。
    let log_path = host_events_path();
    let before = std::fs::read_to_string(&log_path).unwrap_or_default();

    let mut mcp = McpProc::spawn();

    // initialize
    let init = mcp.request("initialize", json!({}));
    assert_eq!(init["serverInfo"]["name"], "engine-scene-mcp");

    // tools/list:F0 5 个 + F1 27 个 = 32 个工具齐全
    let tools = mcp.request("tools/list", json!({}));
    let names: Vec<&str> = tools["tools"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|t| t["name"].as_str())
        .collect();
    assert_eq!(names.len(), 32, "tools/list 须为 32 个工具:{names:?}");
    for want in [
        "host_ping", "scene_new", "scene_summary", "render_once", "host_events",
        "entity_create", "entity_destroy", "entity_rename", "entity_get", "entity_list",
        "entity_batch_apply", "component_add", "component_remove", "component_set",
        "component_get", "component_list_types", "transform_set", "transform_get",
        "transform_batch_set", "scene_save", "scene_load", "scene_diff",
        "scene_checkpoint", "scene_rollback", "edit_undo", "edit_redo",
        "play_enter", "play_pause", "play_resume", "play_step", "play_exit", "play_state",
    ] {
        assert!(names.contains(&want), "tools/list 缺 {want}:{names:?}");
    }

    // F1 透传实测:component_list_types + entity_create + entity_list + play_state。
    let types = mcp.call_tool("component_list_types", json!({}));
    assert_eq!(types.as_array().unwrap().len(), 4, "注册表须 4 类型");
    let created = mcp.call_tool(
        "entity_create",
        json!({
            "name": "MCP立方体",
            "components": [{ "type": "MeshRenderer", "props": { "mesh": "cube.fbx", "material": "m.mat" } }],
            "translation": [1.0, 2.0, 3.0]
        }),
    );
    let eid = created["id"].as_u64().unwrap();
    let list = mcp.call_tool("entity_list", json!({}));
    assert_eq!(list["entities"].as_array().unwrap().len(), 1);
    assert_eq!(list["entities"][0]["transform"]["translation"][1], 2.0);
    assert_eq!(mcp.call_tool("play_state", json!({}))["state"], "edit");
    // 撤销:create → undo → list 为空。
    mcp.call_tool("edit_undo", json!({}));
    assert_eq!(mcp.call_tool("entity_list", json!({}))["entities"].as_array().unwrap().len(), 0);
    let _ = eid;

    // autoStart 后 scene_summary 可用
    let sum = mcp.call_tool("scene_summary", json!({}));
    let backend = sum["physics"]["backend"].as_str().unwrap().to_string();
    assert!(["jolt", "rapier"].contains(&backend.as_str()), "backend 非法:{backend}");

    // 取 host pid 并强杀
    let ping = mcp.call_tool("host_ping", json!({}));
    let pid = ping["pid"].as_u64().expect("host_ping 应含 pid");
    kill_pid(pid);

    // 10s 内断言 host.crashed 与 host.restarted 落盘(看门狗 500ms 一拍)
    let deadline = Instant::now() + Duration::from_secs(10);
    let (mut saw_crashed, mut saw_restarted) = (false, false);
    while Instant::now() < deadline {
        let text = std::fs::read_to_string(&log_path).unwrap_or_default();
        let new = text.strip_prefix(&before).unwrap_or(&text);
        saw_crashed = new.lines().any(|l| l.contains("\"host.crashed\""));
        saw_restarted = new.lines().any(|l| l.contains("\"host.restarted\""));
        if saw_crashed && saw_restarted {
            break;
        }
        std::thread::sleep(Duration::from_millis(200));
    }
    assert!(saw_crashed, "10s 内未见 host.crashed 落盘");
    assert!(saw_restarted, "10s 内未见 host.restarted 落盘");

    // 崩溃记录须含 reason;host_events 工具应能读回这些行
    let events = mcp.call_tool("host_events", json!({}));
    let arr = events.as_array().unwrap();
    let crashed = arr
        .iter()
        .find(|e| e["event"] == "host.crashed")
        .expect("host_events 应含 host.crashed");
    assert!(crashed["reason"].as_str().is_some(), "host.crashed 须含 reason");
    assert!(crashed["ts"].as_str().unwrap().ends_with('Z'));

    // 重启后 scene_summary 恢复可用(看门狗已 scene.new 恢复,名为 restored)
    let sum2 = mcp.call_tool("scene_summary", json!({}));
    assert_eq!(sum2["name"], "restored", "重启后应恢复为新场景");
    assert_eq!(sum2["entityCount"], 0);
    assert_eq!(sum2["physics"]["backend"].as_str().unwrap(), backend);

    // 清理:先杀 MCP(停看门狗,避免杀 host 后又被拉起),再杀其拉起的 host。
    let ping2 = mcp.call_tool("host_ping", json!({}));
    let pid2 = ping2["pid"].as_u64();
    let _ = mcp.child.kill();
    let _ = mcp.child.wait();
    if let Some(pid2) = pid2 {
        kill_pid(pid2);
    }
}
