//! asset-pipeline-mcp 集成测试:真实 spawn + stdio NDJSON 往返。

use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use std::process::{Child, ChildStdin, Command, Stdio};

use serde_json::{json, Value};

struct McpChild {
    child: Child,
    stdin: ChildStdin,
    lines: std::io::Lines<BufReader<std::process::ChildStdout>>,
    next_id: i64,
}

impl McpChild {
    fn spawn(project: &std::path::Path) -> Self {
        let bin = PathBuf::from(env!("CARGO_BIN_EXE_asset-pipeline-mcp"));
        let mut child = Command::new(bin)
            .arg("--project")
            .arg(project)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .expect("spawn asset-pipeline-mcp");
        let stdin = child.stdin.take().unwrap();
        let stdout = child.stdout.take().unwrap();
        let mut c = McpChild { child, stdin, lines: BufReader::new(stdout).lines(), next_id: 0 };
        c.call("initialize", json!({
            "protocolVersion": "2024-11-05",
            "capabilities": {},
            "clientInfo": { "name": "test", "version": "0" }
        }));
        c.notify("notifications/initialized", json!({}));
        c
    }

    fn call(&mut self, method: &str, params: Value) -> Value {
        self.next_id += 1;
        let id = self.next_id;
        let req = json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params });
        writeln!(self.stdin, "{}", serde_json::to_string(&req).unwrap()).unwrap();
        self.stdin.flush().unwrap();
        loop {
            let line = self.lines.next().unwrap().unwrap();
            let msg: Value = serde_json::from_str(&line).unwrap();
            if msg.get("id").and_then(Value::as_i64) == Some(id) {
                return msg;
            }
        }
    }

    fn notify(&mut self, method: &str, params: Value) {
        let req = json!({ "jsonrpc": "2.0", "method": method, "params": params });
        writeln!(self.stdin, "{}", serde_json::to_string(&req).unwrap()).unwrap();
        self.stdin.flush().unwrap();
    }

    fn tool(&mut self, name: &str, args: Value) -> Value {
        let resp = self.call("tools/call", json!({ "name": name, "arguments": args }));
        let text = resp.pointer("/result/content/0/text").and_then(Value::as_str).expect("缺 content text");
        serde_json::from_str(text).expect("工具返回非 JSON")
    }
}

impl Drop for McpChild {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn tmp_project(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("asset_mcp_test_{}_{}", tag, std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn copy_conformance(name: &str, dest_dir: &std::path::Path) -> PathBuf {
    let src = PathBuf::from("H:/rurix/conformance/asset/gltf/accept").join(name);
    let dst = dest_dir.join(name);
    std::fs::copy(&src, &dst).unwrap();
    dst
}

#[test]
fn stdio_import_list_status() {
    let root = tmp_project("stdio");
    let mut c = McpChild::spawn(&root);

    // 导入 tri_min.gltf
    let src = copy_conformance("tri_min.gltf", &root);
    let out = c.tool("asset_import", json!({
        "sourcePaths": [src.to_string_lossy()],
        "destFolder": "Meshes"
    }));
    let imported = out["imported"].as_array().expect("缺 imported");
    assert_eq!(imported.len(), 1);
    let guid = imported[0]["guid"].as_str().expect("缺 guid");
    assert!(!guid.is_empty());
    assert_eq!(imported[0]["cacheHit"], false);

    // asset_list
    let list = c.tool("asset_list", json!({}));
    let assets = list["assets"].as_array().expect("缺 assets");
    assert!(assets.iter().any(|a| a["guid"] == guid), "list 应见新资产");

    // asset_get_meta
    let meta = c.tool("asset_get_meta", json!({ "assetPath": "Meshes/tri_min.gltf" }));
    assert_eq!(meta["meta"]["guid"], guid);
    assert_eq!(meta["meta"]["type"], "mesh");

    // asset_build_status
    let status = c.tool("asset_build_status", json!({ "assetPaths": ["Meshes/tri_min.gltf"] }));
    let items = status["items"].as_array().expect("缺 items");
    assert_eq!(items[0]["state"], "current");

    // 二次导入 → cache hit
    let out2 = c.tool("asset_import", json!({
        "sourcePaths": [src.to_string_lossy()],
        "destFolder": "Meshes"
    }));
    let imported2 = out2["imported"].as_array().unwrap();
    assert_eq!(imported2[0]["cacheHit"], true, "二次导入应命中缓存");

    let _ = std::fs::remove_dir_all(&root);
}
