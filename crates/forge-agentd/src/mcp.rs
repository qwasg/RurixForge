//! MCP stdio 客户端:engine-scene-mcp 长连接单例(懒加载 + 断线重连)。
//! 协议:newline-delimited JSON-RPC(initialize → notifications/initialized → tools/call)。
//! 长连接是场景状态跨调用持久的前提(02 §3.2);每次独立 spawn 会让 engine-host 实体表丢状态(F1 实测缺陷)。

use serde_json::{json, Value};
use std::collections::HashMap;
use std::fmt;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::{Arc, Mutex as StdMutex, OnceLock};
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
    // F-GAME-3:2D 精灵一步创建(实体 + Sprite 组件组合工具)
    "mcp__engine-scene__sprite_create",
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
    // IDE 三分类:场景实体按 角色/地图/交互 分组索引
    "mcp__engine-scene__scene_index",
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
    // 指针点击注入(归一化视口坐标 → 游戏平面世界坐标 → <action>_x/_y/_z + <action>)
    "mcp__engine-scene__logic_inject_pointer",
    "mcp__engine-scene__viewport_frame",
    "mcp__engine-scene__viewport_pick",
    "mcp__engine-scene__viewport_set_camera",
    "mcp__engine-scene__viewport_get_camera",
    // 视口直连推流通道信息(浏览器 WS 直连帧推送/实时输入;绕开 MCP 轮询链)
    "mcp__engine-scene__viewport_stream_info",
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
    // F10:语义元数据(.meta semantic 段写入)
    "mcp__asset-pipeline__asset_set_description",
    // F2 wave.3:贴图缩略图(原图直出 data URL)
    "mcp__asset-pipeline__asset_thumbnail",
    // F2 wave.4:材质/贴图处理/网格检查
    "mcp__asset-pipeline__material_create",
    "mcp__asset-pipeline__texture_process",
    "mcp__asset-pipeline__mesh_inspect",
    // F2 wave.5:asset-cleanup dryRun 扫描
    "mcp__asset-pipeline__asset_cleanup_scan",
    // F-GAME-4:精灵图集资产面(.rxsprite 创建/读/写 + 自动切帧)
    "mcp__asset-pipeline__sprite_create",
    "mcp__asset-pipeline__sprite_get",
    "mcp__asset-pipeline__sprite_set",
    "mcp__asset-pipeline__sprite_autoslice",
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
    // F10:context(语义索引/RAG 检索六工具;词法/混合档显式标注 I-5)
    "mcp__context__context_index_build",
    "mcp__context__context_search",
    "mcp__context__context_get",
    "mcp__context__context_index_status",
    "mcp__context__asset_describe_batch",
    "mcp__context__asset_set_description",
    // F11(D-025):资产商店(可插拔 registry 分发 + 个人资产库)。
    // 安装/卸载为异步提交制——返 taskId 后经 store_task_status 轮询,
    // 因为包下载 + 校验 + assetd 构建链远超 MCP 的 10s 上限(D-F11-C,同 D-024 对 gen/mesh 的处置)。
    "mcp__store__store_sources_list",
    "mcp__store__store_search",
    "mcp__store__store_info",
    "mcp__store__store_installed_list",
    "mcp__store__store_update_check",
    "mcp__store__store_install",
    "mcp__store__store_uninstall",
    "mcp__store__store_task_status",
    "mcp__store__library_list",
    "mcp__store__library_search",
    "mcp__store__library_add",
    "mcp__store__library_remove",
    "mcp__store__library_install",
];

const SCENE_PREFIX: &str = "mcp__engine-scene__";
const ASSET_PREFIX: &str = "mcp__asset-pipeline__";
const CODE_PREFIX: &str = "mcp__code-forge__";
const GEN_IMAGE_PREFIX: &str = "mcp__gen-image__";
const GEN_MODEL_PREFIX: &str = "mcp__gen-model__";
const CONTEXT_PREFIX: &str = "mcp__context__";
const STORE_PREFIX: &str = "mcp__store__";
/// MCP 调用缺省超时(交互级工具:场景编辑/资产查询/代码检索)。
const CALL_TIMEOUT: Duration = Duration::from_secs(10);
/// 3D 生成:供应商异步任务制,meshy 两阶段各 900s 预算(gend::media 的 MESHY_STAGE_BUDGET_SECS)
/// 加下载余量。真正的控制面是工具参数 timeoutSec,外层只负责不比内层先断。
const GEN_MESH_TIMEOUT: Duration = Duration::from_secs(2000);
/// 图像/贴图组生成:单次远端 HTTP 预算 30s(gend::remote),贴图组逐 map 串行,给足余量。
const GEN_IMAGE_TIMEOUT: Duration = Duration::from_secs(360);
/// rx 工具链:子进程自带 timeoutMs 参数(rx_check 缺省 30s、rx_build/rx_test 缺省 120s),
/// 外层比内层紧会让内层超时永远无法触发,取其上界加余量。
const RX_TIMEOUT: Duration = Duration::from_secs(180);
/// 商店查询面:远端 https 源单次 HTTP 预算 30s(forge-store::source),多源聚合串行,给足余量。
/// 安装/卸载不在此列——它们是异步提交(立即返 taskId),真正的耗时在 REST 长任务侧。
const STORE_TIMEOUT: Duration = Duration::from_secs(90);

/// 按工具选超时。缺省 10s 对交互工具合适,但对「内层自带更长预算」的工具是错的——
/// 外层先断会让内层的超时与错误码永远走不到,调用方只看到一句无信息量的「MCP 调用超时」。
fn call_timeout(tool: &str) -> Duration {
    match tool {
        "mcp__gen-model__gen_mesh" => GEN_MESH_TIMEOUT,
        "mcp__gen-image__gen_image"
        | "mcp__gen-image__gen_texture_set"
        | "mcp__gen-image__gen_variations" => GEN_IMAGE_TIMEOUT,
        "mcp__code-forge__rx_check"
        | "mcp__code-forge__rx_build"
        | "mcp__code-forge__rx_run"
        | "mcp__code-forge__rx_fmt"
        | "mcp__code-forge__rx_test" => RX_TIMEOUT,
        "mcp__store__store_search" | "mcp__store__store_info" | "mcp__store__store_update_check" => {
            STORE_TIMEOUT
        }
        _ => CALL_TIMEOUT,
    }
}

/// MCP 服务标识(七工:engine-scene + asset-pipeline + code-forge + gen-image + gen-model
/// + context + store)。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ServerKind {
    EngineScene,
    AssetPipeline,
    CodeForge,
    GenImage,
    GenModel,
    Context,
    Store,
}

impl ServerKind {
    /// 七工全量(tools/list 遍历用)
    const ALL: [ServerKind; 7] = [
        ServerKind::EngineScene,
        ServerKind::AssetPipeline,
        ServerKind::CodeForge,
        ServerKind::GenImage,
        ServerKind::GenModel,
        ServerKind::Context,
        ServerKind::Store,
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
        } else if tool.starts_with(CONTEXT_PREFIX) {
            Some(ServerKind::Context)
        } else if tool.starts_with(STORE_PREFIX) {
            Some(ServerKind::Store)
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
            ServerKind::Context => CONTEXT_PREFIX,
            ServerKind::Store => STORE_PREFIX,
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

/// context-mcp 二进制路径:env FORGE_CONTEXT_MCP_BIN 优先。
fn context_server_bin() -> PathBuf {
    if let Ok(p) = std::env::var("FORGE_CONTEXT_MCP_BIN") {
        return PathBuf::from(p);
    }
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let root = manifest_dir
        .ancestors()
        .nth(2)
        .expect("CARGO_MANIFEST_DIR 应有上两级(workspace 根)");
    root.join("target").join("debug").join("context-mcp.exe")
}

/// store-mcp 二进制路径:env FORGE_STORE_MCP_BIN 优先。
fn store_server_bin() -> PathBuf {
    if let Ok(p) = std::env::var("FORGE_STORE_MCP_BIN") {
        return PathBuf::from(p);
    }
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let root = manifest_dir
        .ancestors()
        .nth(2)
        .expect("CARGO_MANIFEST_DIR 应有上两级(workspace 根)");
    root.join("target").join("debug").join("store-mcp.exe")
}

/// workspace 根(store-mcp 需要:官方源 registry/ 与 skills/ 落地根都锚在这里)。
fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("CARGO_MANIFEST_DIR 应有上两级(workspace 根)")
        .to_path_buf()
}

/// 缺省资产项目根 = <workspace>/projects/demo(05 §1.2 mcp.json 示例对齐)。
/// 作用域波:这只是「没有工作区可解析」时的兜底,真正的项目根由 scope::project_root_of
/// 按会话工作区算出并逐调用传入,不再全局锚死一个项目。
/// F-TEAM-3:必须 canonicalize 与 scope::canonical 同形态——连接池按路径字符串分池,
/// Windows 上 canonicalize 产生 \\?\ verbatim 前缀,两种形态会为同一目录开两套
/// MCP 子进程 + 两个 engine-host(双真相源:/mcp/call 面与 turn 面各看各的场景)。
pub(crate) fn default_project_root() -> PathBuf {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let root = manifest_dir
        .ancestors()
        .nth(2)
        .expect("CARGO_MANIFEST_DIR 应有上两级(workspace 根)");
    let p = root.join("projects").join("demo");
    p.canonicalize().unwrap_or(p)
}

/// 资产项目根(REST 面遗留调用点:gen/video、gen/audio 产物落项目 tmpstore)。
pub(crate) fn asset_project_root() -> PathBuf {
    default_project_root()
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
    async fn connect(bin: &Path, args: &[String], env: &[(String, String)]) -> Result<Self, McpError> {
        let mut cmd = Command::new(bin);
        for a in args {
            cmd.arg(a);
        }
        for (k, v) in env {
            cmd.env(k, v);
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

/// 长连接池:按 (服务, 项目根) 分槽。
///
/// 此前是七个全局单例,任何会话调 asset_list 都落同一个 projects/demo 子进程——
/// 多项目下「切了工作区仍在读上一个项目」正源于此。改按项目根分槽后,不同项目
/// 各有独立子进程(engine-host 场景态也随之隔离),同项目内仍复用长连接
/// (场景状态跨调用持久是 02 §3.2 的前提,不能退化成每次 spawn)。
type Slot = Arc<Mutex<Option<McpClient>>>;
static POOL: OnceLock<StdMutex<HashMap<(ServerKind, PathBuf), Slot>>> = OnceLock::new();

fn client_slot(kind: ServerKind, project_root: &Path) -> Slot {
    let pool = POOL.get_or_init(|| StdMutex::new(HashMap::new()));
    let key = (kind, project_root.to_path_buf());
    let mut guard = pool.lock().unwrap_or_else(|e| e.into_inner());
    guard.entry(key).or_insert_with(|| Arc::new(Mutex::new(None))).clone()
}

/// 取可用连接:无连接/已退出则重连
async fn ensure_connected<'a>(
    kind: ServerKind,
    project_root: &Path,
    guard: &mut MutexGuard<'a, Option<McpClient>>,
) -> Result<(), McpError> {
    if let Some(c) = guard.as_mut() {
        match c.child.try_wait() {
            Ok(None) => return Ok(()),
            _ => **guard = None,
        }
    }
    let (bin, args) = spawn_spec(kind, project_root);
    if !bin.exists() {
        return Err(McpError(format!(
            "{:?} 二进制不存在: {}",
            kind,
            bin.display()
        )));
    }
    **guard = Some(McpClient::connect(&bin, &args, &spawn_env(kind, project_root)).await?);
    Ok(())
}

/// 各 server spawn 规格(二进制 + 启动参数;连接池与 FreshSession 共用)。
fn spawn_spec(kind: ServerKind, project_root: &Path) -> (PathBuf, Vec<String>) {
    let project = project_root.to_string_lossy().into_owned();
    match kind {
        // engine-scene 与 code-forge 经 env 认项目根(二者无 --project 参数面):
        // 前者是 engine-host 的 FORGE_PROJECT_ROOT,后者是 FORGE_CODE_FORGE_PROJECT。
        ServerKind::EngineScene => (server_bin(), vec![]),
        ServerKind::AssetPipeline => (
            asset_server_bin(),
            vec!["--project".to_string(), project],
        ),
        ServerKind::CodeForge => (code_forge_server_bin(), vec![]),
        ServerKind::GenImage => (gen_image_server_bin(), vec!["--project".to_string(), project]),
        ServerKind::GenModel => (gen_model_server_bin(), vec!["--project".to_string(), project]),
        ServerKind::Context => (
            context_server_bin(),
            vec![
                "--project".to_string(),
                project,
                // 文档腿根 = 项目根:此前锚死仓根,多项目下 A 的 agent 会检索到仓库
                // 全量设计文档而看不见自己项目里的策划案。
                "--docs".to_string(),
                project_root.to_string_lossy().into_owned(),
            ],
        ),
        // store 额外要 --workspace:官方源 registry/ 与 skill 包落地根 skills/ 都锚在仓根,
        // 与 --project(资产落地目标)是两个不同的根,不能合并。
        ServerKind::Store => (
            store_server_bin(),
            vec![
                "--project".to_string(),
                project,
                "--workspace".to_string(),
                workspace_root().to_string_lossy().into_owned(),
            ],
        ),
    }
}

/// 子进程环境注入。
///
/// - 项目根:engine-scene/code-forge 没有 --project 参数面,只认 env。
/// - 私有源令牌:store-mcp 从 `FORGE_STORE_TOKEN_<ID>` 取(见 store-mcp mcp.rs 注释),
///   由本进程从 keystore 取出后逐 spawn 注入;此前没人注入,UI 里配的私有源令牌
///   对 agent 这条腿一直是失效的。令牌只进子进程环境,不进事件/日志/模型上下文(R-5)。
fn spawn_env(kind: ServerKind, project_root: &Path) -> Vec<(String, String)> {
    let project = project_root.to_string_lossy().into_owned();
    match kind {
        ServerKind::EngineScene => vec![("FORGE_PROJECT_ROOT".to_string(), project)],
        ServerKind::CodeForge => vec![("FORGE_CODE_FORGE_PROJECT".to_string(), project)],
        ServerKind::Store => store_token_env(),
        _ => Vec::new(),
    }
}

/// 已配置源的 keystore 令牌 → 环境变量对(无令牌的源不产条目)。
fn store_token_env() -> Vec<(String, String)> {
    let ks = gend::keystore::Keystore::load();
    let mut out = Vec::new();
    for id in crate::store::configured_source_ids() {
        let slot = format!("store:{id}");
        if let Some(tok) = ks.key_for(&slot).filter(|t| !t.is_empty()) {
            let var = format!(
                "FORGE_STORE_TOKEN_{}",
                id.to_uppercase().replace(['-', '.'], "_")
            );
            out.push((var, tok));
        }
    }
    out
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
        let root = default_project_root();
        let (bin, args) = spawn_spec(kind, &root);
        if !bin.exists() {
            return Err(McpError(format!(
                "{:?} 二进制不存在: {}",
                kind,
                bin.display()
            )));
        }
        let client = McpClient::connect(&bin, &args, &spawn_env(kind, &root)).await?;
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

/// 调用 MCP 工具(缺省项目根;REST 遗留调用点用)。
pub async fn call_tool(tool: &str, arguments: Option<Value>) -> Result<Value, McpError> {
    call_tool_in(&default_project_root(), tool, arguments).await
}

/// 调用 MCP 工具:tool 为全名(带前缀),按前缀路由到对应服务,按项目根选连接池槽。
pub async fn call_tool_in(
    project_root: &Path,
    tool: &str,
    arguments: Option<Value>,
) -> Result<Value, McpError> {
    let kind = ServerKind::from_tool(tool)
        .ok_or_else(|| McpError(format!("未知工具前缀: {tool}")))?;
    let name = tool
        .strip_prefix(kind.prefix())
        .ok_or_else(|| McpError(format!("工具名前缀剥离失败: {tool}")))?;
    let budget = call_timeout(tool);
    tokio::time::timeout(
        budget,
        call_with_retry(kind, project_root, name, arguments),
    )
    .await
    .map_err(|_| McpError(format!("MCP 调用超时({}s):{tool}", budget.as_secs())))?
}

async fn call_with_retry(
    kind: ServerKind,
    project_root: &Path,
    name: &str,
    arguments: Option<Value>,
) -> Result<Value, McpError> {
    let slot = client_slot(kind, project_root);
    let mut guard = slot.lock().await;
    let mut last_err: Option<McpError> = None;
    for attempt in 0..2 {
        ensure_connected(kind, project_root, &mut guard).await?;
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

/// 单 server 的工具面拉取结果(部分降级用:一个服务起不来不该让整个工具面归零)。
pub struct ServerTools {
    pub kind: ServerKind,
    pub tools: Vec<Value>,
    /// Some = 该服务不可用的如实原因(tools 为空)。
    pub error: Option<String>,
}

/// 抽出仍可用的工具。某个 server 失败时其余 server 的工具照常留下。
pub fn usable_tools(out: &[ServerTools]) -> Vec<Value> {
    out.iter().flat_map(|s| s.tools.clone()).collect()
}

/// 七 server tools/list 实测拉取(缺省项目根)。
pub async fn list_all_tools() -> Result<Vec<Value>, McpError> {
    let out = list_tools_in(&default_project_root()).await;
    let tools = usable_tools(&out);
    if tools.is_empty() {
        let why = out
            .iter()
            .filter_map(|s| s.error.as_ref().map(|e| format!("{:?}: {e}", s.kind)))
            .collect::<Vec<_>>()
            .join("; ");
        return Err(McpError(if why.is_empty() {
            "MCP 工具面为空".into()
        } else {
            why
        }));
    }
    Ok(tools)
}

/// 按项目根拉取七 server 工具面(懒加载 + 按项目缓存)。
///
/// 与旧实现的关键差异:逐 server 收集结果而非首错即返。任一 server 二进制缺失时,
/// 旧实现让整个 turn 拿不到任何工具(用户看到的就是「agent 不会调用工具」);
/// 现在缺失的服务只是自己那一族不可用,其余照常可调,原因随 ServerTools.error 上报。
pub async fn list_tools_in(project_root: &Path) -> Vec<ServerTools> {
    let cache = TOOLS_CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    {
        let guard = cache.lock().await;
        if let Some(hit) = guard.get(project_root) {
            return hit
                .iter()
                .map(|(kind, tools, error)| ServerTools {
                    kind: *kind,
                    tools: tools.clone(),
                    error: error.clone(),
                })
                .collect();
        }
    }
    let mut out: Vec<ServerTools> = Vec::new();
    for kind in ServerKind::ALL {
        out.push(list_one(kind, project_root).await);
    }
    // 全失败不进缓存:多半是这一刻二进制还没构建出来,下次调用该重试。
    if out.iter().any(|s| !s.tools.is_empty()) {
        let mut guard = cache.lock().await;
        guard.insert(
            project_root.to_path_buf(),
            out.iter()
                .map(|s| (s.kind, s.tools.clone(), s.error.clone()))
                .collect(),
        );
    }
    out
}

async fn list_one(kind: ServerKind, project_root: &Path) -> ServerTools {
    let slot = client_slot(kind, project_root);
    let mut guard = slot.lock().await;
    if let Err(e) = ensure_connected(kind, project_root, &mut guard).await {
        return ServerTools { kind, tools: Vec::new(), error: Some(e.0) };
    }
    let client = guard.as_mut().expect("ensure_connected 后必有连接");
    let result = match client.request("tools/list", json!({})).await {
        Ok(r) => r,
        Err(e) => {
            *guard = None;
            return ServerTools { kind, tools: Vec::new(), error: Some(e.0) };
        }
    };
    if result.get("nextCursor").and_then(Value::as_str).is_some() {
        return ServerTools {
            kind,
            tools: Vec::new(),
            error: Some("tools/list 分页未支持(nextCursor 存在)".into()),
        };
    }
    let Some(tools) = result.get("tools").and_then(Value::as_array) else {
        return ServerTools {
            kind,
            tools: Vec::new(),
            error: Some("tools/list 响应缺 tools 数组".into()),
        };
    };
    let mut out = Vec::new();
    for t in tools {
        let Some(name) = t.get("name").and_then(Value::as_str) else {
            continue;
        };
        out.push(json!({
            "name": format!("{}{}", kind.prefix(), name),
            "description": t.get("description").cloned().unwrap_or(Value::Null),
            "inputSchema": t.get("inputSchema").cloned().unwrap_or_else(|| json!({ "type": "object" })),
        }));
    }
    ServerTools { kind, tools: out, error: None }
}

/// tools/list 进程内缓存(server schema 运行期不变;按项目根分键)。
type ToolsCacheEntry = Vec<(ServerKind, Vec<Value>, Option<String>)>;
static TOOLS_CACHE: OnceLock<Mutex<HashMap<PathBuf, ToolsCacheEntry>>> = OnceLock::new();

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// 外层超时不得比工具内层预算紧,否则内层的超时与错误码永远走不到。
    #[test]
    fn long_running_tools_get_budget_above_their_inner_timeout() {
        // 3D 生成:gend::media 两阶段各 900s 预算 → 外层须 > 1800s。
        assert!(
            call_timeout("mcp__gen-model__gen_mesh").as_secs() > 1800,
            "gen_mesh 外层预算须覆盖适配器两阶段"
        );
        // rx_build/rx_test 子进程缺省 120s。
        for t in ["mcp__code-forge__rx_build", "mcp__code-forge__rx_test"] {
            assert!(call_timeout(t).as_secs() >= 120, "{t} 外层预算不足以覆盖内层");
        }
        // 图像面单次远端 30s,贴图组逐 map 串行。
        assert!(call_timeout("mcp__gen-image__gen_texture_set").as_secs() >= 120);
        // 交互级工具维持 10s:慢就是故障,不该靠拉长超时掩盖。
        for t in [
            "mcp__engine-scene__scene_load",
            "mcp__asset-pipeline__asset_list",
            "mcp__gen-model__gen_accept",
            "mcp__context__context_search",
        ] {
            assert_eq!(call_timeout(t), CALL_TIMEOUT, "{t} 应维持缺省超时");
        }
    }

    /// 超时表里的工具名必须真实存在,否则改名后这张表会静默失效。
    #[test]
    fn timeout_table_names_exist_in_known_tools() {
        for t in [
            "mcp__gen-model__gen_mesh",
            "mcp__gen-image__gen_image",
            "mcp__gen-image__gen_texture_set",
            "mcp__gen-image__gen_variations",
            "mcp__code-forge__rx_check",
            "mcp__code-forge__rx_build",
            "mcp__code-forge__rx_run",
            "mcp__code-forge__rx_fmt",
            "mcp__code-forge__rx_test",
        ] {
            assert!(KNOWN_TOOLS.contains(&t), "超时表引用了不存在的工具名: {t}");
            assert!(ServerKind::from_tool(t).is_some(), "{t} 前缀无法路由");
        }
    }

    #[test]
    fn partial_mcp_failure_keeps_other_servers() {
        let mixed = vec![
            ServerTools {
                kind: ServerKind::Context,
                tools: vec![json!({"function": {"name": "mcp__context__context_search"}})],
                error: None,
            },
            ServerTools {
                kind: ServerKind::Store,
                tools: vec![],
                error: Some("bin missing".into()),
            },
        ];
        let tools = usable_tools(&mixed);
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0]["function"]["name"], "mcp__context__context_search");
        assert!(mixed.iter().any(|s| s.error.is_some()));
    }
}
