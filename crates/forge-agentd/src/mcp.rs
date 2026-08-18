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
pub const KNOWN_TOOLS: &[&str] = &[
    "mcp__engine-scene__host_ping",
    "mcp__engine-scene__host_events",
    // F3 wave.3:debug 三件套——内存事件环排空(场景域事件)
    "mcp__engine-scene__host_events_drain",
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
    // F3 wave.3:debug 三件套——场景图全量转储
    "mcp__engine-scene__scene_graph_dump",
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
    // F4 wave.3:逻辑输入注入(play 态驱动图解释器 on_input)
    "mcp__engine-scene__logic_inject_input",
    "mcp__engine-scene__viewport_frame",
    "mcp__engine-scene__viewport_pick",
    "mcp__engine-scene__viewport_set_camera",
    "mcp__engine-scene__viewport_get_camera",
    "mcp__engine-scene__viewport_share_open",
    "mcp__engine-scene__viewport_share_close",
    // F2 wave.1+2:asset-pipeline(资产管线)
    "mcp__asset-pipeline__asset_import",
    "mcp__asset-pipeline__asset_list",
    "mcp__asset-pipeline__asset_get_meta",
    "mcp__asset-pipeline__asset_build_status",
    "mcp__asset-pipeline__asset_refs",
    "mcp__asset-pipeline__asset_delete",
    "mcp__asset-pipeline__asset_move",
    "mcp__asset-pipeline__asset_fix_redirectors",
    "mcp__asset-pipeline__asset_reimport",
    "mcp__asset-pipeline__asset_set_meta",
    // F2 wave.3:贴图缩略图(原图直出 data URL)
    "mcp__asset-pipeline__asset_thumbnail",
    // F2 wave.4:材质/贴图处理/网格检查
    "mcp__asset-pipeline__material_create",
    "mcp__asset-pipeline__texture_process",
    "mcp__asset-pipeline__mesh_inspect",
    // F2 wave.5:asset-cleanup dryRun 扫描
    "mcp__asset-pipeline__asset_cleanup_scan",
    // F4 wave.1:code-forge(rx 工具链五工具,子进程包上游 rx CLI/rurixc)
    "mcp__code-forge__rx_check",
    "mcp__code-forge__rx_build",
    "mcp__code-forge__rx_run",
    "mcp__code-forge__rx_fmt",
    "mcp__code-forge__rx_test",
    // F4 wave.2:code-forge graph 三工具(.rxgraph 校验/创建/读取,10 §6)
    "mcp__code-forge__graph_validate",
    "mcp__code-forge__graph_create",
    "mcp__code-forge__graph_get",
    // F4 wave.4:code-forge code_* 三工具(符号搜索/LSP 引用/结构化编辑,05 §4)
    "mcp__code-forge__code_symbol_search",
    "mcp__code-forge__code_references",
    "mcp__code-forge__code_structured_edit",
    // F5 wave.1:gen-image 五工具(05 §7;GEN_BACKEND_NOT_CONFIGURED 门 + keystore R-5)
    "mcp__gen-image__gen_backends_list",
    "mcp__gen-image__gen_image",
    "mcp__gen-image__gen_texture_set",
    "mcp__gen-image__gen_accept",
    "mcp__gen-image__gen_variations",
    // F5 wave.2:gen-model 三工具(05 §8;text2mesh/refine 无后端显式 NOT_CONFIGURED,
    // gen_accept 走 asset_import 同一构建链)
    "mcp__gen-model__gen_mesh",
    "mcp__gen-model__gen_mesh_refine",
    "mcp__gen-model__gen_accept",
];

const SCENE_PREFIX: &str = "mcp__engine-scene__";
const ASSET_PREFIX: &str = "mcp__asset-pipeline__";
const CODE_PREFIX: &str = "mcp__code-forge__";
const GEN_IMAGE_PREFIX: &str = "mcp__gen-image__";
const GEN_MODEL_PREFIX: &str = "mcp__gen-model__";
const CALL_TIMEOUT: Duration = Duration::from_secs(10);

/// MCP 服务标识(五工:engine-scene + asset-pipeline + code-forge + gen-image + gen-model)。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ServerKind {
    EngineScene,
    AssetPipeline,
    CodeForge,
    GenImage,
    GenModel,
}

impl ServerKind {
    /// 五工全量(tools/list 遍历用)
    const ALL: [ServerKind; 5] = [
        ServerKind::EngineScene,
        ServerKind::AssetPipeline,
        ServerKind::CodeForge,
        ServerKind::GenImage,
        ServerKind::GenModel,
    ];

    fn from_tool(tool: &str) -> Option<Self> {
        if tool.starts_with(SCENE_PREFIX) {
            Some(ServerKind::EngineScene)
        } else if tool.starts_with(ASSET_PREFIX) {
            Some(ServerKind::AssetPipeline)
        } else if tool.starts_with(CODE_PREFIX) {
            Some(ServerKind::CodeForge)
        } else if tool.starts_with(GEN_IMAGE_PREFIX) {
            Some(ServerKind::GenImage)
        } else if tool.starts_with(GEN_MODEL_PREFIX) {
            Some(ServerKind::GenModel)
        } else {
            None
        }
    }

    fn prefix(self) -> &'static str {
        match self {
            ServerKind::EngineScene => SCENE_PREFIX,
            ServerKind::AssetPipeline => ASSET_PREFIX,
            ServerKind::CodeForge => CODE_PREFIX,
            ServerKind::GenImage => GEN_IMAGE_PREFIX,
            ServerKind::GenModel => GEN_MODEL_PREFIX,
        }
    }
}

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

/// asset-pipeline-mcp 二进制路径:env FORGE_ASSET_PIPELINE_MCP_BIN 优先。
fn asset_server_bin() -> PathBuf {
    if let Ok(p) = std::env::var("FORGE_ASSET_PIPELINE_MCP_BIN") {
        return PathBuf::from(p);
    }
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let root = manifest_dir
        .ancestors()
        .nth(2)
        .expect("CARGO_MANIFEST_DIR 应有上两级(workspace 根)");
    root.join("target").join("debug").join("asset-pipeline-mcp.exe")
}

/// code-forge-mcp 二进制路径:env FORGE_CODE_FORGE_MCP_BIN 优先。
fn code_forge_server_bin() -> PathBuf {
    if let Ok(p) = std::env::var("FORGE_CODE_FORGE_MCP_BIN") {
        return PathBuf::from(p);
    }
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let root = manifest_dir
        .ancestors()
        .nth(2)
        .expect("CARGO_MANIFEST_DIR 应有上两级(workspace 根)");
    root.join("target").join("debug").join("code-forge-mcp.exe")
}

/// gen-image-mcp 二进制路径:env FORGE_GEN_IMAGE_MCP_BIN 优先。
fn gen_image_server_bin() -> PathBuf {
    if let Ok(p) = std::env::var("FORGE_GEN_IMAGE_MCP_BIN") {
        return PathBuf::from(p);
    }
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let root = manifest_dir
        .ancestors()
        .nth(2)
        .expect("CARGO_MANIFEST_DIR 应有上两级(workspace 根)");
    root.join("target").join("debug").join("gen-image-mcp.exe")
}

/// gen-model-mcp 二进制路径:env FORGE_GEN_MODEL_MCP_BIN 优先。
fn gen_model_server_bin() -> PathBuf {
    if let Ok(p) = std::env::var("FORGE_GEN_MODEL_MCP_BIN") {
        return PathBuf::from(p);
    }
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let root = manifest_dir
        .ancestors()
        .nth(2)
        .expect("CARGO_MANIFEST_DIR 应有上两级(workspace 根)");
    root.join("target").join("debug").join("gen-model-mcp.exe")
}

/// 资产项目根 = <workspace>/projects/demo(05 §1.2 mcp.json 示例对齐)。
fn asset_project_root() -> PathBuf {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let root = manifest_dir
        .ancestors()
        .nth(2)
        .expect("CARGO_MANIFEST_DIR 应有上两级(workspace 根)");
    root.join("projects").join("demo")
}

/// 长连接句柄:子进程 + stdin/stdout 行流 + 自增 id
struct McpClient {
    child: Child,
    stdin: ChildStdin,
    lines: Lines<BufReader<ChildStdout>>,
    next_id: i64,
}

impl McpClient {
    /// spawn + initialize 握手(args 为空 = 无额外参数)。
    async fn connect(bin: &Path, args: &[String]) -> Result<Self, McpError> {
        let mut cmd = Command::new(bin);
        for a in args {
            cmd.arg(a);
        }
        let mut child = cmd
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
static SCENE_CLIENT: OnceLock<Mutex<Option<McpClient>>> = OnceLock::new();
static ASSET_CLIENT: OnceLock<Mutex<Option<McpClient>>> = OnceLock::new();
static CODE_CLIENT: OnceLock<Mutex<Option<McpClient>>> = OnceLock::new();
static GEN_IMAGE_CLIENT: OnceLock<Mutex<Option<McpClient>>> = OnceLock::new();
static GEN_MODEL_CLIENT: OnceLock<Mutex<Option<McpClient>>> = OnceLock::new();

fn client_slot(kind: ServerKind) -> &'static Mutex<Option<McpClient>> {
    match kind {
        ServerKind::EngineScene => SCENE_CLIENT.get_or_init(|| Mutex::new(None)),
        ServerKind::AssetPipeline => ASSET_CLIENT.get_or_init(|| Mutex::new(None)),
        ServerKind::CodeForge => CODE_CLIENT.get_or_init(|| Mutex::new(None)),
        ServerKind::GenImage => GEN_IMAGE_CLIENT.get_or_init(|| Mutex::new(None)),
        ServerKind::GenModel => GEN_MODEL_CLIENT.get_or_init(|| Mutex::new(None)),
    }
}

/// 取可用连接:无连接/已退出则重连
async fn ensure_connected<'a>(
    kind: ServerKind,
    guard: &mut MutexGuard<'a, Option<McpClient>>,
) -> Result<(), McpError> {
    if let Some(c) = guard.as_mut() {
        match c.child.try_wait() {
            Ok(None) => return Ok(()),
            _ => **guard = None,
        }
    }
    let (bin, args) = spawn_spec(kind);
    if !bin.exists() {
        return Err(McpError(format!(
            "{:?} 二进制不存在: {}",
            kind,
            bin.display()
        )));
    }
    **guard = Some(McpClient::connect(&bin, &args).await?);
    Ok(())
}

/// 各 server spawn 规格(二进制 + 启动参数;单例与 FreshSession 共用)。
fn spawn_spec(kind: ServerKind) -> (PathBuf, Vec<String>) {
    match kind {
        ServerKind::EngineScene => (server_bin(), vec![]),
        ServerKind::AssetPipeline => (
            asset_server_bin(),
            vec!["--project".to_string(), asset_project_root().to_string_lossy().into_owned()],
        ),
        ServerKind::CodeForge => (code_forge_server_bin(), vec![]),
        ServerKind::GenImage => (
            gen_image_server_bin(),
            vec!["--project".to_string(), asset_project_root().to_string_lossy().into_owned()],
        ),
        ServerKind::GenModel => (
            gen_model_server_bin(),
            vec!["--project".to_string(), asset_project_root().to_string_lossy().into_owned()],
        ),
    }
}

/// F6 wave.2(D-F6-B):非单例短连接——test-matrix 每 shard 独立 spawn
/// (独立 engine-scene-mcp 子进程 → 独立 engine-host 看门狗实例,场景态互不影响)。
/// 用毕须 shutdown(显式 kill;Drop 兜底,防孤儿继承 stdout 句柄挂管道——实测坑)。
pub struct FreshSession {
    client: McpClient,
}

impl FreshSession {
    /// spawn 新实例(二进制缺失如实 Err)。
    pub async fn spawn(kind: ServerKind) -> Result<Self, McpError> {
        let (bin, args) = spawn_spec(kind);
        if !bin.exists() {
            return Err(McpError(format!(
                "{:?} 二进制不存在: {}",
                kind,
                bin.display()
            )));
        }
        let client = McpClient::connect(&bin, &args).await?;
        Ok(Self { client })
    }

    /// 调用工具(name 不带前缀;本腿首发仅 EngineScene 用,调用方负责全名剥离)。
    pub async fn call(&mut self, name: &str, arguments: Option<Value>) -> Result<Value, McpError> {
        let params = json!({ "name": name, "arguments": arguments.unwrap_or_else(|| json!({})) });
        self.client.request("tools/call", params).await
    }

    /// 显式关停子进程(kill + wait 收尸)。
    pub async fn shutdown(&mut self) {
        let _ = self.client.child.kill().await;
        let _ = self.client.child.wait().await;
    }

    /// 子进程(engine-scene-mcp)PID——并发证据(每 shard 独立实例,PID 互异即非单例)。
    pub fn pid(&self) -> Option<u32> {
        self.client.child.id()
    }
}

impl Drop for FreshSession {
    fn drop(&mut self) {
        // 兜底:异步 kill 不可用时至少发起 kill(tokio Child::start_kill 同步可调用)。
        let _ = self.client.child.start_kill();
    }
}

/// 调用 MCP 工具:tool 为全名(带前缀),按前缀路由到对应服务。
pub async fn call_tool(tool: &str, arguments: Option<Value>) -> Result<Value, McpError> {
    let kind = ServerKind::from_tool(tool)
        .ok_or_else(|| McpError(format!("未知工具前缀: {tool}")))?;
    let name = tool
        .strip_prefix(kind.prefix())
        .ok_or_else(|| McpError(format!("工具名前缀剥离失败: {tool}")))?;
    tokio::time::timeout(CALL_TIMEOUT, call_with_retry(kind, name, arguments))
        .await
        .map_err(|_| McpError("MCP 调用超时(10s)".to_string()))?
}

async fn call_with_retry(kind: ServerKind, name: &str, arguments: Option<Value>) -> Result<Value, McpError> {
    let mut guard = client_slot(kind).lock().await;
    let mut last_err: Option<McpError> = None;
    for attempt in 0..2 {
        ensure_connected(kind, &mut guard).await?;
        let client = guard.as_mut().expect("ensure_connected 后必有连接");
        let params = json!({ "name": name, "arguments": arguments.clone().unwrap_or_else(|| json!({})) });
        match client.request("tools/call", params).await {
            Ok(result) => return Ok(result),
            Err(e) => {
                last_err = Some(e);
                *guard = None;
                if attempt == 1 {
                    break;
                }
            }
        }
    }
    Err(last_err.unwrap_or_else(|| McpError("MCP 调用失败".into())))
}

/// RD-F1-002:五 server tools/list 实测拉取(懒加载 + 进程内缓存)。
/// 返回 [{ name(带前缀), description, inputSchema }],供 LLM provider 转 OpenAI tools 格式。
/// 分页:各 server 工具量远小于单页,若响应带 nextCursor 如实报错(不静默截断)。
pub async fn list_all_tools() -> Result<Vec<Value>, McpError> {
    let cache = TOOLS_CACHE.get_or_init(|| Mutex::new(None));
    {
        let guard = cache.lock().await;
        if let Some(tools) = guard.as_ref() {
            return Ok(tools.clone());
        }
    }
    let mut out: Vec<Value> = Vec::new();
    for kind in ServerKind::ALL {
        let mut guard = client_slot(kind).lock().await;
        ensure_connected(kind, &mut guard).await?;
        let client = guard.as_mut().expect("ensure_connected 后必有连接");
        let result = client.request("tools/list", json!({})).await?;
        if result.get("nextCursor").and_then(Value::as_str).is_some() {
            return Err(McpError(format!(
                "{:?} tools/list 分页未支持(nextCursor 存在)",
                kind
            )));
        }
        let tools = result
            .get("tools")
            .and_then(Value::as_array)
            .ok_or_else(|| McpError(format!("{:?} tools/list 响应缺 tools 数组", kind)))?;
        for t in tools {
            let name = t
                .get("name")
                .and_then(Value::as_str)
                .ok_or_else(|| McpError(format!("{:?} tools/list 条目缺 name", kind)))?;
            out.push(json!({
                "name": format!("{}{}", kind.prefix(), name),
                "description": t.get("description").cloned().unwrap_or(Value::Null),
                "inputSchema": t.get("inputSchema").cloned().unwrap_or_else(|| json!({ "type": "object" })),
            }));
        }
    }
    let mut guard = cache.lock().await;
    *guard = Some(out.clone());
    Ok(out)
}

/// tools/list 进程内缓存(server schema 运行期不变;spawn 失败重连不影响 schema 正确性)
static TOOLS_CACHE: OnceLock<Mutex<Option<Vec<Value>>>> = OnceLock::new();
