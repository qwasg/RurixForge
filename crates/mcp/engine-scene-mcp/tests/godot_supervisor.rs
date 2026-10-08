//! Stage 3:监督器按 forge.toml `[render] backend = "godot"` 拉起 Godot 宿主(02 §6.2、§7.3)。
//! 前提:target/debug/godot_host.dll(cargo build -p godot-host)与 vendor/godot 的官方模板;
//! 运行时目录由 scripts/godot-runtime.ps1 生成到 target/godot-runtime-mcp-test,经 FORGE_GODOT_RUNTIME_DIR 指给监督器。

use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use std::process::{Child, ChildStdin, Command, Stdio};

use serde_json::{json, Value};

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(3)
        .unwrap()
        .to_path_buf()
}

struct Mcp {
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<std::process::ChildStdout>,
    next_id: u64,
    host_pid: Option<u64>,
}

impl Mcp {
    fn request(&mut self, method: &str, params: Value) -> Value {
        let id = self.next_id;
        self.next_id += 1;
        let req = json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params });
        writeln!(self.stdin, "{}", serde_json::to_string(&req).unwrap()).unwrap();
        self.stdin.flush().unwrap();
        let mut line = String::new();
        self.stdout.read_line(&mut line).unwrap();
        let resp: Value = serde_json::from_str(line.trim()).unwrap();
        assert!(resp.get("error").is_none(), "{method}: {resp}");
        resp["result"].clone()
    }

    fn tool(&mut self, name: &str, args: Value) -> Value {
        let r = self.request("tools/call", json!({ "name": name, "arguments": args }));
        assert!(r.get("isError").is_none(), "{name}: {r}");
        serde_json::from_str(r["content"][0]["text"].as_str().unwrap()).unwrap()
    }
}

impl Drop for Mcp {
    fn drop(&mut self) {
        // Stop the whole owned process tree, including a restarted host whose PID
        // may not yet have been observed if an assertion failed.
        let _ = Command::new("taskkill")
            .args(["/PID", &self.child.id().to_string(), "/F", "/T"])
            .stdout(Stdio::null()).stderr(Stdio::null()).status();
        let _ = self.child.kill();
        let _ = self.child.wait();
        // 孙进程不随父进程回收(watchdog_integration 同款坑):按 PID 杀 Godot 主 exe。
        if let Some(pid) = self.host_pid {
            let _ = Command::new("taskkill")
                .args(["/PID", &pid.to_string(), "/F", "/T"])
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status();
        }
    }
}

fn process_name(pid: u64) -> String {
    let out = Command::new("tasklist")
        .args(["/FI", &format!("PID eq {pid}"), "/FO", "CSV", "/NH"])
        .output()
        .unwrap();
    String::from_utf8_lossy(&out.stdout)
        .split(',')
        .next()
        .unwrap_or("")
        .trim_matches('"')
        .to_string()
}

#[test]
fn supervisor_launches_and_restarts_godot_host_from_forge_toml_render_section() {
    let root = workspace_root();
    let dll = root.join("target").join("debug").join("godot_host.dll");
    assert!(
        dll.exists(),
        "缺 {}:先 cargo build -p godot-host",
        dll.display()
    );
    let runtime = root.join("target").join("godot-runtime-mcp-test");
    let st = Command::new("powershell")
        .args(["-NoProfile", "-ExecutionPolicy", "Bypass", "-File"])
        .arg(root.join("scripts").join("godot-runtime.ps1"))
        .arg("-Out")
        .arg(&runtime)
        .status()
        .unwrap();
    assert!(st.success(), "godot-runtime.ps1 失败");
    let project = std::env::temp_dir().join(format!("esm-godot-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&project);
    std::fs::create_dir_all(project.join("Content")).unwrap();
    std::fs::write(
        project.join("forge.toml"),
        "[project]\nname = \"g3\"\n\n[render]\nbackend = \"godot\"\n",
    )
    .unwrap();

    let log_path = project.join("host-events.jsonl");
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_engine-scene-mcp"));
    cmd.env("FORGE_PROJECT_ROOT", &project)
        .env("FORGE_HOST_EVENTS_LOG", &log_path)
        .env("FORGE_GODOT_RUNTIME_DIR", &runtime);
    for k in [
        "FORGE_RENDER_BACKEND",
        "FORGE_RENDER_METHOD",
        "FORGE_RENDER_DRIVER",
        "FORGE_GODOT_GPU_INDEX",
        "FORGE_GODOT_ARGS",
        "FORGE_GAME_SCENE",
        "FORGE_ENGINE_HOST_BIN",
    ] {
        cmd.env_remove(k);
    }
    let mut child = cmd
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let stdin = child.stdin.take().unwrap();
    let stdout = BufReader::new(child.stdout.take().unwrap());
    let mut mcp = Mcp {
        child,
        stdin,
        stdout,
        next_id: 1,
        host_pid: None,
    };
    assert_eq!(
        mcp.request("initialize", json!({}))["serverInfo"]["name"],
        "engine-scene-mcp"
    );
    let ping = mcp.tool("host_ping", json!({}));
    let pid = ping["pid"].as_u64().expect("host.ping 带 pid");
    mcp.host_pid = Some(pid);
    assert_eq!(
        process_name(pid).to_ascii_lowercase(),
        "forge-godot.exe",
        "宿主应是 Godot 运行时:{ping}"
    );
    let f = mcp.tool("viewport_frame", json!({ "width": 64, "height": 48 }));
    assert_eq!(
        (f["width"].clone(), f["height"].clone(), f["format"].clone()),
        (json!(64), json!(48), json!("rgba8")),
        "{f}"
    );
    assert!(f["pixelsB64"].as_str().is_some_and(|s| !s.is_empty()));

    // Exercise the real Godot watchdog edge: kill the host process, then require a fresh
    // host.crashed + host.restarted sequence and a usable replacement RPC endpoint.
    let before = std::fs::read_to_string(&log_path).unwrap_or_default();
    let status = Command::new("taskkill")
        .args(["/PID", &pid.to_string(), "/F", "/T"])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .expect("taskkill Godot host");
    assert!(
        status.success(),
        "Godot host PID {pid} should be terminated"
    );
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(45);
    let mut saw_crashed = false;
    let mut saw_restarted = false;
    while std::time::Instant::now() < deadline {
        let text = std::fs::read_to_string(&log_path).unwrap_or_default();
        let new = text.strip_prefix(&before).unwrap_or(&text);
        saw_crashed = new.lines().any(|line| line.contains("\"host.crashed\""));
        saw_restarted = new.lines().any(|line| line.contains("\"host.restarted\""));
        if saw_crashed && saw_restarted {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(250));
    }
    assert!(
        saw_crashed,
        "Godot kill must append host.crashed within 45 seconds"
    );
    assert!(
        saw_restarted,
        "Godot watchdog must append host.restarted within 45 seconds"
    );
    let ping2 = mcp.tool("host_ping", json!({}));
    let pid2 = ping2["pid"].as_u64().expect("restarted Godot host has PID");
    assert_ne!(pid2, pid, "watchdog restart must spawn a new Godot process");
    mcp.host_pid = Some(pid2);
    assert_eq!(
        process_name(pid2).to_ascii_lowercase(),
        "forge-godot.exe",
        "replacement must be Godot host: {ping2}"
    );
    let restored = mcp.tool("scene_summary", json!({}));
    assert_eq!(
        restored["name"], "restored",
        "watchdog must restore a blank scene after restart"
    );
    assert_eq!(restored["entityCount"], 0);
    let _ = std::fs::remove_dir_all(&project);
}
