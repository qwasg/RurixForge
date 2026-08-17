//! MCP stdio 客户端:每次调用独立 spawn engine-scene-mcp 子进程(F0 简单可靠,不追求长连)
//! 协议:newline-delimited JSON-RPC(initialize → notifications/initialized → tools/call)

use serde_json::{json, Value};
use std::fmt;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader, Lines};
use tokio::process::{Child, ChildStdin, ChildStdout, Command};

/// 已挂载的 engine-scene 工具面(声明式;与 engine-scene-mcp tools/list 一一对应)
pub const KNOWN_TOOLS: [&str; 32] = [
    // F0 既有
    "mcp__engine-scene__host_ping",
    "mcp__engine-scene__scene_new",
    "mcp__engine-scene__scene_summary",
    "mcp__engine-scene__render_once",
    "mcp__engine-scene__host_events",
    // F1 entity.*
    "mcp__engine-scene__entity_create",
    "mcp__engine-scene__entity_destroy",
    "mcp__engine-scene__entity_rename",
    "mcp__engine-scene__entity_get",
    "mcp__engine-scene__entity_list",
    "mcp__engine-scene__entity_batch_apply",
    // F1 component.*
    "mcp__engine-scene__component_add",
    "mcp__engine-scene__component_remove",
    "mcp__engine-scene__component_set",
    "mcp__engine-scene__component_get",
    "mcp__engine-scene__component_list_types",
    // F1 transform.*
    "mcp__engine-scene__transform_set",
    "mcp__engine-scene__transform_get",
    "mcp__engine-scene__transform_batch_set",
    // F1 scene.* / edit.* / play.*
    "mcp__engine-scene__scene_save",
    "mcp__engine-scene__scene_load",
    "mcp__engine-scene__scene_diff",
    "mcp__engine-scene__scene_checkpoint",
    "mcp__engine-scene__scene_rollback",
    "mcp__engine-scene__edit_undo",
    "mcp__engine-scene__edit_redo",
    "mcp__engine-scene__play_enter",
    "mcp__engine-scene__play_pause",
    "mcp__engine-scene__play_resume",
    "mcp__engine-scene__play_step",
    "mcp__engine-scene__play_exit",
    "mcp__engine-scene__play_state",
];

const TOOL_PREFIX: &str = "mcp__engine-scene__";
const CALL_TIMEOUT: Duration = Duration::from_secs(10);

/// MCP 调用失败(spawn / 传输 / 超时 / 协议错误)
#[derive(Debug)]
pub struct McpError(String);

impl fmt::Display for McpError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for McpError {}

/// engine-scene-mcp 二进制路径:env FORGE_ENGINE_SCENE_MCP_BIN 优先,
/// 否则 <workspace_root>/target/debug/engine-scene-mcp.exe
/// (workspace_root = CARGO_MANIFEST_DIR 上两级:crates/forge-agentd → 仓根)
pub fn server_bin() -> PathBuf {
    if let Ok(p) = std::env::var("FORGE_ENGINE_SCENE_MCP_BIN") {
        return PathBuf::from(p);
    }
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let root = manifest_dir
        .ancestors()
        .nth(2)
        .expect("CARGO_MANIFEST_DIR 应有上两级(workspace 根)");
    root.join("target").join("debug").join("engine-scene-mcp.exe")
}

/// 调用 engine-scene 工具:tool 为全名(mcp__engine-scene__X),内部映射为 X
pub async fn call_tool(tool: &str, arguments: Option<Value>) -> Result<Value, McpError> {
    let name = tool
        .strip_prefix(TOOL_PREFIX)
        .ok_or_else(|| McpError(format!("未知工具名: {tool}")))?;
    let bin = server_bin();
    if !bin.exists() {
        return Err(McpError(format!(
            "engine-scene-mcp 二进制不存在: {}",
            bin.display()
        )));
    }
    tokio::time::timeout(CALL_TIMEOUT, call_inner(&bin, name, arguments))
        .await
        .map_err(|_| McpError("MCP 调用超时(10s)".to_string()))?
}

async fn call_inner(bin: &Path, name: &str, arguments: Option<Value>) -> Result<Value, McpError> {
    let mut child = Command::new(bin)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| McpError(format!("spawn {} 失败: {e}", bin.display())))?;
    let result = exchange(&mut child, name, arguments).await;
    // exchange 返回时 stdin 已 drop → 对端收到 EOF 走正常关停(其负责清理 engine-host)。
    // 给 3s 优雅退出窗,超时兜底再 kill,避免 engine-host 成孤儿。
    match tokio::time::timeout(Duration::from_secs(3), child.wait()).await {
        Ok(_) => {}
        Err(_) => {
            let _ = child.kill().await;
        }
    }
    result
}

/// initialize → initialized 通知 → tools/call,按 id 配对读响应
async fn exchange(child: &mut Child, name: &str, arguments: Option<Value>) -> Result<Value, McpError> {
    let mut stdin = child.stdin.take().ok_or_else(|| McpError("子进程无 stdin".into()))?;
    let stdout = child.stdout.take().ok_or_else(|| McpError("子进程无 stdout".into()))?;
    let mut lines = BufReader::new(stdout).lines();

    send(
        &mut stdin,
        &json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "initialize",
            "params": {
                "protocolVersion": "2024-11-05",
                "capabilities": {},
                "clientInfo": { "name": "forge-agentd", "version": "0.1.0" }
            }
        }),
    )
    .await?;
    read_response(&mut lines, 1).await?;

    send(&mut stdin, &json!({ "jsonrpc": "2.0", "method": "notifications/initialized" })).await?;

    send(
        &mut stdin,
        &json!({
            "jsonrpc": "2.0",
            "id": 2,
            "method": "tools/call",
            "params": { "name": name, "arguments": arguments.unwrap_or_else(|| json!({})) }
        }),
    )
    .await?;
    let resp = read_response(&mut lines, 2).await?;

    if let Some(err) = resp.get("error") {
        return Err(McpError(format!("engine-scene-mcp 返回 JSON-RPC 错误: {err}")));
    }
    resp.get("result")
        .cloned()
        .ok_or_else(|| McpError("tools/call 响应缺少 result".into()))
}

/// 发送一行 newline-delimited JSON-RPC 消息
async fn send(stdin: &mut ChildStdin, msg: &Value) -> Result<(), McpError> {
    let mut line =
        serde_json::to_string(msg).map_err(|e| McpError(format!("序列化请求失败: {e}")))?;
    line.push('\n');
    stdin
        .write_all(line.as_bytes())
        .await
        .map_err(|e| McpError(format!("写子进程 stdin 失败: {e}")))?;
    stdin
        .flush()
        .await
        .map_err(|e| McpError(format!("flush stdin 失败: {e}")))
}

/// 逐行读取,跳过非 JSON 行(如日志)与其他 id 的消息,直到匹配期望 id
async fn read_response(lines: &mut Lines<BufReader<ChildStdout>>, id: i64) -> Result<Value, McpError> {
    loop {
        let line = lines
            .next_line()
            .await
            .map_err(|e| McpError(format!("读子进程 stdout 失败: {e}")))?
            .ok_or_else(|| McpError("子进程 stdout 意外关闭".into()))?;
        let Ok(msg) = serde_json::from_str::<Value>(&line) else {
            continue;
        };
        if msg.get("id").and_then(Value::as_i64) == Some(id) {
            return Ok(msg);
        }
    }
}
