//! MCP stdio 客户端:engine-scene-mcp 长连接单例(懒加载 + 断线重连)。
//! 协议:newline-delimited JSON-RPC(initialize → notifications/initialized → tools/call)。
//! 长连接是场景状态跨调用持久的前提(02 §3.2);每次独立 spawn 会让 engine-host 实体表丢状态(F1 实测缺陷)。

use serde_json::{json, Value};
use std::fmt;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::OnceLock;
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader, Lines};
use tokio::process::{Child, ChildStdin, ChildStdout, Command};
use tokio::sync::{Mutex, MutexGuard};

/// 已挂载的 engine-scene 工具面(显式全量清单,与 engine-scene-mcp 工具表一一对应)
pub const KNOWN_TOOLS: [&str; 38] = [
    "mcp__engine-scene__host_ping",
    "mcp__engine-scene__host_events",
    "mcp__engine-scene__scene_new",
    "mcp__engine-scene__scene_summary",
    "mcp__engine-scene__render_once",
    "mcp__engine-scene__entity_create",
    "mcp__engine-scene__entity_destroy",
    "mcp__engine-scene__entity_rename",
    "mcp__engine-scene__entity_get",
    "mcp__engine-scene__entity_list",
    "mcp__engine-scene__entity_batch_apply",
    "mcp__engine-scene__component_add",
    "mcp__engine-scene__component_remove",
    "mcp__engine-scene__component_set",
    "mcp__engine-scene__component_get",
    "mcp__engine-scene__component_list_types",
    "mcp__engine-scene__transform_set",
    "mcp__engine-scene__transform_get",
    "mcp__engine-scene__transform_batch_set",
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
    "mcp__engine-scene__viewport_frame",
    "mcp__engine-scene__viewport_pick",
    "mcp__engine-scene__viewport_set_camera",
    "mcp__engine-scene__viewport_get_camera",
    "mcp__engine-scene__viewport_share_open",
    "mcp__engine-scene__viewport_share_close",
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

/// 长连接句柄:子进程 + stdin/stdout 行流 + 自增 id
struct McpClient {
    child: Child,
    stdin: ChildStdin,
    lines: Lines<BufReader<ChildStdout>>,
    next_id: i64,
}

impl McpClient {
    /// spawn + initialize 握手
    async fn connect(bin: &Path) -> Result<Self, McpError> {
        let mut child = Command::new(bin)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| McpError(format!("spawn {} 失败: {e}", bin.display())))?;
        let stdin = child.stdin.take().ok_or_else(|| McpError("子进程无 stdin".into()))?;
        let stdout = child.stdout.take().ok_or_else(|| McpError("子进程无 stdout".into()))?;
        let mut client = Self {
            child,
            stdin,
            lines: BufReader::new(stdout).lines(),
            next_id: 0,
        };
        client
            .request(
                "initialize",
                json!({
                    "protocolVersion": "2024-11-05",
                    "capabilities": {},
                    "clientInfo": { "name": "forge-agentd", "version": "0.1.0" }
                }),
            )
            .await?;
        client
            .notify("notifications/initialized", json!({}))
            .await?;
        Ok(client)
    }

    /// 发请求并按 id 配对读响应(跳过日志行与其他 id)
    async fn request(&mut self, method: &str, params: Value) -> Result<Value, McpError> {
        self.next_id += 1;
        let id = self.next_id;
        self.send(&json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params }))
            .await?;
        loop {
            let line = self
                .lines
                .next_line()
                .await
                .map_err(|e| McpError(format!("读子进程 stdout 失败: {e}")))?
                .ok_or_else(|| McpError("子进程 stdout 意外关闭(对端退出)".into()))?;
            let Ok(msg) = serde_json::from_str::<Value>(&line) else {
                continue;
            };
            if msg.get("id").and_then(Value::as_i64) == Some(id) {
                if let Some(err) = msg.get("error") {
                    return Err(McpError(format!("JSON-RPC 错误: {err}")));
                }
                return msg
                    .get("result")
                    .cloned()
                    .ok_or_else(|| McpError("响应缺少 result".into()));
            }
        }
    }

    async fn notify(&mut self, method: &str, params: Value) -> Result<(), McpError> {
        self.send(&json!({ "jsonrpc": "2.0", "method": method, "params": params }))
            .await
    }

    async fn send(&mut self, msg: &Value) -> Result<(), McpError> {
        let mut line =
            serde_json::to_string(msg).map_err(|e| McpError(format!("序列化请求失败: {e}")))?;
        line.push('\n');
        self.stdin
            .write_all(line.as_bytes())
            .await
            .map_err(|e| McpError(format!("写子进程 stdin 失败: {e}")))?;
        self.stdin
            .flush()
            .await
            .map_err(|e| McpError(format!("flush stdin 失败: {e}")))
    }
}

/// 全局长连接(OnceLock + Mutex;None = 未连接或已断线)
static CLIENT: OnceLock<Mutex<Option<McpClient>>> = OnceLock::new();

fn client_slot() -> &'static Mutex<Option<McpClient>> {
    CLIENT.get_or_init(|| Mutex::new(None))
}

/// 取可用连接:无连接/已退出则重连
async fn ensure_connected<'a>(
    guard: &mut MutexGuard<'a, Option<McpClient>>,
) -> Result<(), McpError> {
    if let Some(c) = guard.as_mut() {
        // try_wait 探活:已退出则丢弃重连
        match c.child.try_wait() {
            Ok(None) => return Ok(()), // 存活
            _ => **guard = None,       // 已退出或不可查 → 重连
        }
    }
    let bin = server_bin();
    if !bin.exists() {
        return Err(McpError(format!(
            "engine-scene-mcp 二进制不存在: {}",
            bin.display()
        )));
    }
    **guard = Some(McpClient::connect(&bin).await?);
    Ok(())
}

/// 调用 engine-scene 工具:tool 为全名(mcp__engine-scene__X),内部映射为 X。
/// 传输失败时丢弃连接并重试一次(长连接对端可能被看门狗换过)。
pub async fn call_tool(tool: &str, arguments: Option<Value>) -> Result<Value, McpError> {
    let name = tool
        .strip_prefix(TOOL_PREFIX)
        .ok_or_else(|| McpError(format!("未知工具名: {tool}")))?;
    tokio::time::timeout(CALL_TIMEOUT, call_with_retry(name, arguments))
        .await
        .map_err(|_| McpError("MCP 调用超时(10s)".to_string()))?
}

async fn call_with_retry(name: &str, arguments: Option<Value>) -> Result<Value, McpError> {
    let mut guard = client_slot().lock().await;
    let mut last_err: Option<McpError> = None;
    for attempt in 0..2 {
        ensure_connected(&mut guard).await?;
        let client = guard.as_mut().expect("ensure_connected 后必有连接");
        let params = json!({ "name": name, "arguments": arguments.clone().unwrap_or_else(|| json!({})) });
        match client.request("tools/call", params).await {
            Ok(result) => return Ok(result),
            Err(e) => {
                last_err = Some(e);
                *guard = None; // 断线,下一轮重连
                if attempt == 1 {
                    break;
                }
            }
        }
    }
    Err(last_err.unwrap_or_else(|| McpError("MCP 调用失败".into())))
}
