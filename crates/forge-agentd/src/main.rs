//! forge-agentd:F0 最小骨架 agent 守护进程
//! 路由:/health、sessions stub、MCP 工具面声明与 stdio 透传调用、mock LLM provider(恒绿 seam)
//! F2 wave.5:Proposal 确认单(12 §3)+ destructive 强制门(asset_delete force)+ skills 发现。
//! F7 wave.1:agent 事件基座(D-F7-A)——events.rs(EventBus:append-only JSONL + 环缓冲 + seq
//! 单调 + emit/emit_ephemeral 二分)、sessions.rs(会话/chat-folders 持久化 + CRUD/fork/revert)、
//! sse.rs(会话事件流 replay+gap+live+keep-alive)、snapshot.rs(design-snapshot 聚合)。

mod agent;
mod events;
mod llm;
mod mcp;
mod playtest;
mod pack;
mod proposals;
mod sessions;
mod snapshot;
mod sse;
mod subagents;
mod swarm;
mod workspace;

use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::{get, patch, post},
    Json, Router,
};
use serde::Deserialize;
use serde_json::{json, Value};
use std::{sync::Arc, time::Instant};

/// 服务共享状态(启动时刻,供 uptimeSec 实测;Proposal 存贮;F7 事件基座三件套 + wave.2 runs/todos)
pub(crate) struct AppState {
    pub(crate) started: Instant,
    pub(crate) proposals: proposals::ProposalStore,
    pub(crate) swarm: swarm::SwarmCoordinator,
    /// F7:事件总线(append-only JSONL data/agent-events/{sessionId}.jsonl + 环缓冲)。
    pub(crate) events: Arc<events::EventBus>,
    /// F7:会话存贮(data/agent-sessions/sessions.json)。
    pub(crate) sessions: Arc<sessions::SessionStore>,
    /// F7:聊天文件夹存贮(data/agent-sessions/chat-folders.json)。
    pub(crate) folders: Arc<sessions::ChatFolderStore>,
    /// F7 wave.2:run 注册表(内存,进程重启即空)+ 取消令牌。
    pub(crate) runs: Arc<agent::RunRegistry>,
    /// F7 wave.2:待办存贮(data/agent-sessions/todos.json 读-改-写)。
    pub(crate) todos: Arc<agent::TodoStore>,
}

#[tokio::main]
async fn main() {
    let addr =
        std::env::var("FORGE_AGENTD_ADDR").unwrap_or_else(|_| "127.0.0.1:8103".to_string());
    let listener = tokio::net::TcpListener::bind(&addr)
        .await
        .unwrap_or_else(|e| panic!("绑定 {addr} 失败: {e}"));
    let local = listener.local_addr().expect("读取本地地址失败");
    println!("forge-agentd listening at http://{local}");
    axum::serve(listener, build_app())
        .await
        .expect("axum serve 失败");
}

/// agent 数据根:env FORGE_AGENTD_DATA_DIR 优先(测试隔离),否则 <workspace>/data(D-F7-E)。
fn agent_data_root() -> std::path::PathBuf {
    if let Ok(p) = std::env::var("FORGE_AGENTD_DATA_DIR") {
        if !p.is_empty() {
            return std::path::PathBuf::from(p);
        }
    }
    workspace_root().join("data")
}

/// 构建路由表(main 与测试复用)
fn build_app() -> Router {
    let data_root = agent_data_root();
    let state = Arc::new(AppState {
        started: Instant::now(),
        proposals: proposals::ProposalStore::default(),
        swarm: swarm::SwarmCoordinator::default(),
        events: Arc::new(events::EventBus::from_env(data_root.join("agent-events"))),
        sessions: Arc::new(sessions::SessionStore::load(
            data_root.join("agent-sessions").join("sessions.json"),
        )),
        folders: Arc::new(sessions::ChatFolderStore::load(
            data_root.join("agent-sessions").join("chat-folders.json"),
        )),
        runs: Arc::new(agent::RunRegistry::default()),
        todos: Arc::new(agent::TodoStore::load(
            data_root.join("agent-sessions").join("todos.json"),
        )),
    });
    Router::new()
        .route("/health", get(health))
        // F7 wave.1:会话事实源(替换 F0 恒空 stub)+ fork/revert 动作 + SSE 事件流。
        .route(
            "/api/forge/sessions",
            get(sessions::list_sessions).post(sessions::create_session),
        )
        .route(
            "/api/forge/sessions/{id}",
            get(sessions::get_session)
                .patch(sessions::patch_session)
                .delete(sessions::delete_session),
        )
        .route("/api/forge/sessions/{id}/fork", post(sessions::fork_session))
        .route(
            "/api/forge/sessions/{id}/revert",
            post(sessions::revert_session),
        )
        .route(
            "/api/forge/sessions/{id}/events/stream",
            get(sse::session_event_stream),
        )
        // F7 wave.2:turn 执行(ask:execute 静态段含冒号,matchit 0.8 仅 {} 为参数语法)+
        // runs 控制 + todos REST。
        .route(
            "/api/forge/sessions/{id}/ask:execute",
            post(agent::ask_execute),
        )
        .route("/api/forge/sessions/{id}/todos", get(agent::list_todos))
        .route("/api/forge/runs/{id}", get(agent::get_run))
        .route("/api/forge/runs/{id}/cancel", post(agent::cancel_run))
        .route("/api/forge/todos", post(agent::create_todo))
        .route("/api/forge/todos/{id}", patch(agent::patch_todo))
        .route(
            "/api/forge/chat-folders",
            get(sessions::list_chat_folders).post(sessions::create_chat_folder),
        )
        .route(
            "/api/forge/chat-folders/{id}",
            patch(sessions::patch_chat_folder).delete(sessions::delete_chat_folder),
        )
        .route("/api/forge/design-snapshot", get(snapshot::design_snapshot))
        .route("/api/forge/mcp/tools", get(mcp_tools))
        .route("/api/forge/mcp/call", post(mcp_call))
        .route("/api/forge/llm/complete", get(llm_complete))
        .route("/api/forge/llm/chat", post(llm::chat))
        // F7 wave.5:deepseek key 配置面(设置·模型页;R-5 不回显)
        .route("/api/forge/llm/key", post(llm::set_llm_key))
        // F7 wave.5:工作区文件树只读面(Inspector 树;confined + 单层 + 截断如实)
        .route("/api/forge/workspace/tree", get(workspace::workspace_tree))
        // F8 wave.1:工作区文件只读文本端点(文件预览;confined + 尺寸上限 + 二进制拒绝)
        .route("/api/forge/workspace/file", get(workspace::workspace_file))
        // F8 wave.2:openai-compatible 通用渠道配置与状态
        .route("/api/forge/llm/openai-compat/config", post(llm::set_openai_compat_config))
        .route("/api/forge/llm/openai-compat/status", get(llm::openai_compat_status_handler))
        .route("/api/forge/playtest/run", post(playtest_run))
        .route("/api/forge/project/pack", post(project_pack))
        .route(
            "/api/forge/proposals",
            get(proposals_list).post(proposals_create),
        )
        .route("/api/forge/proposals/{id}", patch(proposals_patch))
        .route("/api/forge/skills/list", get(skills_list))
        .route("/api/forge/skills/{name}", get(skills_read))
        .route("/api/forge/skills/config/write", post(skills_config_write))
        .route("/api/forge/subagents", get(subagents_list))
        .route("/api/forge/gen/backends", get(gen_backends_list))
        .route(
            "/api/forge/gen/backends/configure",
            post(gen_backends_configure),
        )
        .route("/api/forge/swarm/state", get(swarm_state))
        .route("/api/forge/swarm/seed-demo", post(swarm_seed_demo))
        .route("/api/forge/swarm/execute", post(swarm_execute))
        .fallback(unknown_route)
        .with_state(state)
}

async fn health(State(state): State<Arc<AppState>>) -> Json<Value> {
    Json(json!({
        "service": "forge-agentd",
        "status": "ok",
        "version": "0.1.0",
        "uptimeSec": state.started.elapsed().as_secs_f64(),
    }))
}

/// 已挂载 MCP 工具面(声明式 stub)
async fn mcp_tools() -> Json<Value> {
    Json(json!({ "tools": mcp::KNOWN_TOOLS }))
}

#[derive(Deserialize)]
struct McpCallRequest {
    tool: String,
    #[serde(default)]
    arguments: Option<Value>,
}

/// MCP 工具调用透传:未知工具 404;子进程调用失败 502;成功返回 MCP result 本体。
/// F2 wave.5:destructive 强制门(asset_delete force=true 须 approved Proposal 覆盖,I-6)。
async fn mcp_call(State(state): State<Arc<AppState>>, Json(req): Json<McpCallRequest>) -> Response {
    if !mcp::KNOWN_TOOLS.contains(&req.tool.as_str()) {
        return (
            StatusCode::NOT_FOUND,
            Json(json!({ "error": { "code": "TOOL_NOT_FOUND" } })),
        )
            .into_response();
    }
    if let Some(paths) = forced_asset_delete(&req.tool, req.arguments.as_ref()) {
        if !state
            .proposals
            .has_approved_covering("asset.delete", &paths)
        {
            // 两阶段:先提案(dry-run 影响面 = 待删资产清单),批准后同一调用才放行。
            let id = state.proposals.create(
                "asset.delete",
                format!("asset_delete force=true 删除 {} 个资产(引用阻断已跳过)", paths.len()),
                json!({ "assets": paths }),
                json!({ "sessionId": "http", "tool": req.tool }),
            );
            return (
                StatusCode::CONFLICT,
                Json(json!({
                    "error": {
                        "code": "GOV_PROPOSAL_REQUIRED",
                        "message": "force 删除为 destructive,须先批准 Proposal(I-6)",
                        "proposalId": id,
                    }
                })),
            )
                .into_response();
        }
    }
    match mcp::call_tool(&req.tool, req.arguments).await {
        Ok(result) => Json(result).into_response(),
        Err(err) => (
            StatusCode::BAD_GATEWAY,
            Json(json!({ "error": { "code": "MCP_CALL_FAILED", "message": err.to_string() } })),
        )
            .into_response(),
    }
}

/// asset_delete force=true → Some(待删路径列表);其他 → None。
fn forced_asset_delete(tool: &str, args: Option<&Value>) -> Option<Vec<String>> {
    if tool != "mcp__asset-pipeline__asset_delete" {
        return None;
    }
    let args = args?;
    if !args.get("force").and_then(Value::as_bool).unwrap_or(false) {
        return None;
    }
    let paths: Vec<String> = args
        .get("assetPaths")?
        .as_array()?
        .iter()
        .filter_map(|v| v.as_str().map(String::from))
        .collect();
    if paths.is_empty() {
        None
    } else {
        Some(paths)
    }
}

/// GET /api/forge/proposals:列表(11 §4 DTO)。
async fn proposals_list(State(state): State<Arc<AppState>>) -> Json<Value> {
    let items: Vec<Value> = state.proposals.list().iter().map(|p| p.to_json()).collect();
    Json(json!({ "proposals": items }))
}

#[derive(Deserialize)]
struct ProposalCreateRequest {
    kind: String,
    summary: String,
    #[serde(default)]
    impact: Option<Value>,
    #[serde(default)]
    created_by: Option<Value>,
}

/// POST /api/forge/proposals:创建 pending(agent/UI 主动提案,如 asset-cleanup 整理提案)。
async fn proposals_create(
    State(state): State<Arc<AppState>>,
    Json(req): Json<ProposalCreateRequest>,
) -> Response {
    if req.kind.is_empty() || req.summary.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": { "code": "FORGE_INVALID_ARGS", "message": "kind/summary 不可空" } })),
        )
            .into_response();
    }
    let id = state.proposals.create(
        &req.kind,
        req.summary,
        req.impact.unwrap_or(json!({})),
        req.created_by.unwrap_or(json!({ "sessionId": "http", "tool": "proposals.create" })),
    );
    let p = state.proposals.get(&id).expect("刚创建的 Proposal 必在");
    Json(p.to_json()).into_response()
}

#[derive(Deserialize)]
struct ProposalPatchRequest {
    action: String,
}

/// PATCH /api/forge/proposals/{id}:{action: approve|reject};终态不可逆(12 §3)。
async fn proposals_patch(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    Json(req): Json<ProposalPatchRequest>,
) -> Response {
    match state.proposals.transition(&id, &req.action) {
        Ok(p) => Json(p.to_json()).into_response(),
        Err(e) if e.starts_with("NOT_FOUND:") => (
            StatusCode::NOT_FOUND,
            Json(json!({ "error": { "code": "FORGE_NOT_FOUND", "message": format!("Proposal 不存在: {id}") } })),
        )
            .into_response(),
        Err(e) if e.starts_with("CLOSED:") => (
            StatusCode::CONFLICT,
            Json(json!({ "error": { "code": "GOV_PROPOSAL_CLOSED", "message": format!("Proposal 已终态({})", &e[7..]) } })),
        )
            .into_response(),
        Err(e) => (
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": { "code": "FORGE_INVALID_ARGS", "message": e } })),
        )
            .into_response(),
    }
}

/// workspace 根(CARGO_MANIFEST_DIR 上两级)。
fn workspace_root() -> std::path::PathBuf {
    let manifest_dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    manifest_dir
        .ancestors()
        .nth(2)
        .expect("CARGO_MANIFEST_DIR 应有上两级(workspace 根)")
        .to_path_buf()
}

/// skills 配置(data/skills-config.json):disabled 清单 + extraDirs 追加扫描目录(06 §2 目录配置)。
#[derive(Debug, Default, Clone, serde::Serialize, serde::Deserialize)]
struct SkillsConfig {
    #[serde(default)]
    disabled: Vec<String>,
    #[serde(default, rename = "extraDirs")]
    extra_dirs: Vec<String>,
}

fn skills_config_path() -> std::path::PathBuf {
    workspace_root().join("data").join("skills-config.json")
}

fn skills_config_load() -> SkillsConfig {
    let p = skills_config_path();
    match std::fs::read_to_string(&p) {
        Ok(text) => serde_json::from_str(&text).unwrap_or_else(|e| {
            eprintln!("skills-config.json 损坏({e}),按缺省处理");
            SkillsConfig::default()
        }),
        Err(_) => SkillsConfig::default(),
    }
}

/// skills 扫描目录集:workspace skills/ + config.extraDirs(相对 workspace 根解析)。
fn skills_dirs(cfg: &SkillsConfig) -> Vec<std::path::PathBuf> {
    let root = workspace_root();
    let mut dirs = vec![root.join("skills")];
    for d in &cfg.extra_dirs {
        dirs.push(root.join(d));
    }
    dirs
}

/// GET /api/forge/skills/list:扫 skills/<name>/SKILL.md frontmatter(06 §2);
/// config.disabled 内 skill 标 enabled=false(07 §7.2 skills tab 启用/禁用数据源)。
async fn skills_list() -> Json<Value> {
    let cfg = skills_config_load();
    let mut skills = Vec::new();
    for dir in skills_dirs(&cfg) {
        if let Ok(rd) = std::fs::read_dir(&dir) {
            for ent in rd.flatten() {
                let f = ent.path().join("SKILL.md");
                if !f.is_file() {
                    continue;
                }
                if let Ok(text) = std::fs::read_to_string(&f) {
                    if let Some((name, description)) = parse_skill_frontmatter(&text) {
                        let enabled = !cfg.disabled.iter().any(|d| d == &name);
                        skills.push(json!({ "name": name, "description": description, "enabled": enabled }));
                    }
                }
            }
        }
    }
    skills.sort_by(|a, b| a["name"].as_str().cmp(&b["name"].as_str()));
    skills.dedup_by(|a, b| a["name"].as_str() == b["name"].as_str());
    Json(json!({ "skills": skills }))
}

/// GET /api/forge/skills/{name}:返回 SKILL.md 全文(read_skill 的 HTTP 面,06 §2)。
async fn skills_read(Path(name): Path<String>) -> Response {
    if !name
        .chars()
        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
    {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": { "code": "FORGE_INVALID_ARGS", "message": "skill 名须为小写英文+中划线(06 §1)" } })),
        )
            .into_response();
    }
    let cfg = skills_config_load();
    for dir in skills_dirs(&cfg) {
        let f = dir.join(&name).join("SKILL.md");
        if f.is_file() {
            return match std::fs::read_to_string(&f) {
                Ok(content) => Json(json!({ "name": name, "content": content })).into_response(),
                Err(e) => (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(json!({ "error": { "code": "FORGE_IO", "message": e.to_string() } })),
                )
                    .into_response(),
            };
        }
    }
    (
        StatusCode::NOT_FOUND,
        Json(json!({ "error": { "code": "FORGE_NOT_FOUND", "message": format!("skill 不存在: {name}") } })),
    )
        .into_response()
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SkillsConfigWriteRequest {
    /// 全量覆盖 disabled 清单;缺省 = 不变。
    #[serde(default)]
    disabled: Option<Vec<String>>,
    #[serde(default)]
    extra_dirs: Option<Vec<String>>,
}

/// POST /api/forge/skills/config/write:写 skills 配置(启用/禁用 + 目录配置,06 §2 管理 API)。
/// disabled 名格式校验(小写英文+中划线);写盘后立即生效(list 读盘无缓存)。
async fn skills_config_write(Json(req): Json<SkillsConfigWriteRequest>) -> Response {
    let mut cfg = skills_config_load();
    if let Some(disabled) = req.disabled {
        for d in &disabled {
            if !d
                .chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
            {
                return (
                    StatusCode::BAD_REQUEST,
                    Json(json!({ "error": { "code": "FORGE_INVALID_ARGS", "message": format!("非法 skill 名: {d}") } })),
                )
                    .into_response();
            }
        }
        cfg.disabled = disabled;
    }
    if let Some(dirs) = req.extra_dirs {
        cfg.extra_dirs = dirs;
    }
    let p = skills_config_path();
    if let Some(parent) = p.parent() {
        if let Err(e) = std::fs::create_dir_all(parent) {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": { "code": "FORGE_IO", "message": e.to_string() } })),
            )
                .into_response();
        }
    }
    match std::fs::write(
        &p,
        serde_json::to_string_pretty(&cfg).expect("SkillsConfig 序列化失败"),
    ) {
        Ok(()) => Json(json!({
            "written": true,
            "disabled": cfg.disabled,
            "extraDirs": cfg.extra_dirs,
        }))
        .into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": { "code": "FORGE_IO", "message": e.to_string() } })),
        )
            .into_response(),
    }
}

/// GET /api/forge/subagents:磁盘 profile 热加载清单(04 §6;改文件不重启生效)。
async fn subagents_list() -> Json<Value> {
    let (profiles, errors) = subagents::list_subagents(&subagents::agents_dir());
    Json(json!({
        "subagents": profiles.iter().map(subagents::SubagentProfile::to_json).collect::<Vec<_>>(),
        // 解析失败如实上报,不遮蔽(诚实优先)。
        "errors": errors,
    }))
}

/// 解析 SKILL.md frontmatter(--- 包裹的 name/description 两行)。
fn parse_skill_frontmatter(text: &str) -> Option<(String, String)> {
    let mut lines = text.lines();
    if lines.next()?.trim() != "---" {
        return None;
    }
    let mut name = None;
    let mut desc = None;
    for line in lines {
        let line = line.trim();
        if line == "---" {
            break;
        }
        if let Some(v) = line.strip_prefix("name:") {
            name = Some(v.trim().to_string());
        } else if let Some(v) = line.strip_prefix("description:") {
            desc = Some(v.trim().to_string());
        }
    }
    Some((name?, desc?))
}

/// mock LLM provider seam(诚实标注,F0 恒绿)
async fn llm_complete() -> Json<Value> {
    Json(json!({ "provider": "mock", "text": "mock completion" }))
}

// ---------- F6 wave.1:playtest 矩阵执行器(D-F6-A) ----------

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct PlaytestRunRequest {
    /// 断言矩阵路径(workspace 相对或绝对)。
    matrix_ref: String,
}

/// POST /api/forge/playtest/run:读矩阵 → playtest::run_matrix(mcp 工具面)→ 结构化报告。
/// 报告即诚实工件:红矩阵(ok=false)同样 200 返回;矩阵级错误(读/解析/scene_load)才非 2xx。
async fn playtest_run(Json(req): Json<PlaytestRunRequest>) -> Response {
    let path = playtest::resolve_workspace_path(&req.matrix_ref);
    let text = match std::fs::read_to_string(&path) {
        Ok(t) => t,
        Err(e) => {
            return (
                StatusCode::NOT_FOUND,
                Json(json!({ "error": { "code": "MATRIX_NOT_FOUND", "message": format!("矩阵读取失败 {}: {e}", path.display()) } })),
            )
                .into_response();
        }
    };
    let matrix: playtest::Matrix = match serde_json::from_str(&text) {
        Ok(m) => m,
        Err(e) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({ "error": { "code": "MATRIX_INVALID", "message": format!("矩阵解析失败: {e}") } })),
            )
                .into_response();
        }
    };
    let mut caller = |tool: String, args: Value| async move {
        let r = mcp::call_tool(&tool, Some(args)).await.map_err(|e| e.to_string())?;
        playtest::unwrap_envelope(&r)
    };
    match playtest::run_matrix(&matrix, &mut caller).await {
        Ok(report) => Json(report.to_json()).into_response(),
        Err(e) => (
            StatusCode::BAD_GATEWAY,
            Json(json!({ "error": { "code": "PLAYTEST_TOOL_ERROR", "message": e } })),
        )
            .into_response(),
    }
}

// ---------- F6 wave.4:project-pack 最小打包(D-F6-D) ----------

/// POST /api/forge/project/pack:引用闭包 → Content 源 + 缓存 + 引擎二进制 + 启动脚本。
async fn project_pack(Json(req): Json<pack::PackRequest>) -> Response {
    let scene_abs = playtest::resolve_workspace_path(&req.scene_ref);
    // 项目根推导:自场景向上首个含 Content/ 子目录的祖先。
    let Some(project_root) = scene_abs
        .ancestors()
        .find(|a| a.join("Content").is_dir())
        .map(|p| p.to_path_buf())
    else {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": { "code": "PACK_PROJECT_ROOT_UNRESOLVED", "message": format!("场景路径无法推导项目根: {}", scene_abs.display()) } })),
        )
            .into_response();
    };
    let out_dir = playtest::resolve_workspace_path(&req.out_dir);
    let engine_bin = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("workspace 根")
        .join("target")
        .join("debug")
        .join("engine-host.exe");
    match pack::build_pack(&scene_abs, &project_root, &out_dir, &engine_bin, 17890) {
        Ok(report) => Json(report).into_response(),
        Err(msg) => {
            let (code, status) = if msg.starts_with("PACK_SCENE_NOT_FOUND") {
                ("PACK_SCENE_NOT_FOUND", StatusCode::NOT_FOUND)
            } else if msg.starts_with("PACK_OUTDIR_CONFLICT") {
                ("PACK_OUTDIR_CONFLICT", StatusCode::CONFLICT)
            } else if msg.starts_with("PACK_ENGINE_MISSING") {
                ("PACK_ENGINE_MISSING", StatusCode::INTERNAL_SERVER_ERROR)
            } else {
                ("PACK_ERROR", StatusCode::INTERNAL_SERVER_ERROR)
            };
            (
                status,
                Json(json!({ "error": { "code": code, "message": msg } })),
            )
                .into_response()
        }
    }
}

// ---------- F3 wave.1:swarm 集群(04 §5 / 11 §2.2)+ multitask 确定性执行器(D-F3-E) ----------

/// GET /api/forge/swarm/state:节点 + 分片状态快照。
async fn swarm_state(State(state): State<Arc<AppState>>) -> Json<Value> {
    Json(state.swarm.state_json())
}

/// POST /api/forge/swarm/seed-demo:演示播种(幂等追加逻辑节点)。
async fn swarm_seed_demo(State(state): State<Arc<AppState>>) -> Json<Value> {
    Json(state.swarm.seed_demo())
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SwarmExecuteRequest {
    shard_type: String,
    /// 分片输入集(scene-partition=实体 id;asset-batch=资产路径;数值/字符串均收,统一归一为字符串)。
    items: Vec<Value>,
    #[serde(default = "default_shard_count")]
    shard_count: usize,
    /// 操作:{kind: add_component|remove_component|set_component, type, props?}
    /// 或 {kind: asset_reimport}(asset-batch 域)。
    operation: Value,
}

fn default_shard_count() -> usize {
    4
}

/// 输入集归一:JSON 数值/字符串统一成字符串(实体 id 与资产路径同构处理)。
fn normalize_items(items: &[Value]) -> Result<Vec<String>, String> {
    let mut out = Vec::with_capacity(items.len());
    for it in items {
        match it {
            Value::String(s) if !s.is_empty() => out.push(s.clone()),
            Value::Number(n) => out.push(n.to_string()),
            _ => return Err(format!("items 元素须为非空字符串或数值,实: {it}")),
        }
    }
    Ok(out)
}

/// operation → 单 item 的 MCP 调用(tool, arguments);域不匹配/缺字段 → Err。
fn operation_call(shard_type: &str, operation: &Value, item: &str) -> Result<(String, Value), String> {
    let kind = operation
        .get("kind")
        .and_then(Value::as_str)
        .ok_or_else(|| "operation.kind 缺失".to_string())?;
    match (shard_type, kind) {
        ("scene-partition", "add_component" | "remove_component" | "set_component") => {
            let ctype = operation
                .get("type")
                .and_then(Value::as_str)
                .ok_or_else(|| format!("operation.type 缺失({kind})"))?;
            let id: u64 = item
                .parse()
                .map_err(|_| format!("scene-partition item 须为实体 id 数值,实: {item}"))?;
            let mut args = json!({ "id": id, "type": ctype });
            if kind != "remove_component" {
                args["props"] = operation.get("props").cloned().unwrap_or(json!({}));
            }
            let tool = match kind {
                "add_component" => "mcp__engine-scene__component_add",
                "remove_component" => "mcp__engine-scene__component_remove",
                _ => "mcp__engine-scene__component_set",
            };
            Ok((tool.to_string(), args))
        }
        ("asset-batch", "asset_reimport") => Ok((
            "mcp__asset-pipeline__asset_reimport".to_string(),
            json!({ "assetPath": item }),
        )),
        // F6 wave.2:test-matrix 每分片整体执行(非逐 item 工具映射);此处仅预检 operation 形态。
        ("test-matrix", "test_run") => {
            if operation.get("matrixRef").and_then(Value::as_str).is_none() {
                return Err("test_run operation 缺 matrixRef".to_string());
            }
            Ok(("__test_matrix_shard__".to_string(), json!({})))
        }
        ("code-module", _) => Err(format!(
            "{shard_type} 域 operation 未落地(F4 code-forge 承接,seam)"
        )),
        ("test-matrix", _) => Err(format!(
            "{shard_type} 仅支持 operation.kind=test_run(matrixRef)"
        )),
        _ => Err(format!("shardType {shard_type} 不支持 operation.kind {kind}")),
    }
}

/// F6 wave.2(D-F6-B):test-matrix 并发守卫——FreshSession 全局信号量(显存预算,默认 2)。
static TEST_MATRIX_SEM: std::sync::OnceLock<tokio::sync::Semaphore> = std::sync::OnceLock::new();

/// test-matrix 分片执行:FreshSession 独立 engine-host 跑子矩阵(items = case 名过滤)。
/// 返回 (ok_items, errors, 证据 report:pid/时间窗/聚合),失败 case 如实进 errors 不遮蔽。
async fn test_matrix_shard_run(
    items: &[String],
    operation: &Value,
) -> (Vec<String>, Vec<Value>, Value) {
    let matrix_ref = operation
        .get("matrixRef")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    let path = playtest::resolve_workspace_path(&matrix_ref);
    let matrix: playtest::Matrix = match std::fs::read_to_string(&path)
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
    {
        Some(m) => m,
        None => {
            let errors: Vec<Value> = items
                .iter()
                .map(|i| json!({ "item": i, "error": format!("矩阵读取/解析失败 {}", path.display()) }))
                .collect();
            return (Vec::new(), errors, json!({ "matrixRef": matrix_ref, "fatal": "matrix_unreadable" }));
        }
    };
    // case 过滤:items 逐个匹配;无名匹配 = 如实 error item。
    let mut sub_cases: Vec<playtest::Case> = Vec::new();
    let mut ok_items: Vec<String> = Vec::new();
    let mut errors: Vec<Value> = Vec::new();
    for item in items {
        match matrix.cases.iter().find(|c| c.name == *item) {
            Some(c) => sub_cases.push(playtest::Case {
                name: c.name.clone(),
                assert_: c.assert_.clone(),
            }),
            None => errors.push(json!({ "item": item, "error": "矩阵中无此 case 名" })),
        }
    }
    // 并发守卫:信号量上限 = operation.maxConcurrent(缺省 2)。
    let max_conc = operation
        .get("maxConcurrent")
        .and_then(Value::as_u64)
        .unwrap_or(2) as usize;
    let sem = TEST_MATRIX_SEM.get_or_init(|| tokio::sync::Semaphore::new(max_conc.max(1)));
    let _permit = match sem.acquire().await {
        Ok(p) => p,
        Err(_) => {
            errors.push(json!({ "item": "*", "error": "并发信号量已关闭" }));
            return (ok_items, errors, json!({ "matrixRef": matrix_ref, "fatal": "semaphore_closed" }));
        }
    };
    let mut session = match mcp::FreshSession::spawn(mcp::ServerKind::EngineScene).await {
        Ok(s) => s,
        Err(e) => {
            errors.push(json!({ "item": "*", "error": format!("FreshSession spawn 失败: {e}") }));
            return (ok_items, errors, json!({ "matrixRef": matrix_ref, "fatal": "spawn_failed" }));
        }
    };
    let pid = session.pid();
    // 绝对纪元毫秒时间窗:跨分片可比,重叠即真并发证据(G-F6-2)。
    let epoch_ms = || {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0)
    };
    let started_ms = epoch_ms();
    let session = std::sync::Arc::new(tokio::sync::Mutex::new(session));
    let s2 = session.clone();
    let mut caller = move |tool: String, args: Value| {
        let s = s2.clone();
        async move {
            let name = tool
                .strip_prefix("mcp__engine-scene__")
                .unwrap_or(&tool)
                .to_string();
            let mut g = s.lock().await;
            let r = g.call(&name, Some(args)).await.map_err(|e| e.to_string())?;
            playtest::unwrap_envelope(&r)
        }
    };
    let sub = playtest::Matrix {
        scene: matrix.scene.clone(),
        camera: matrix.camera.clone(),
        enter_play: matrix.enter_play,
        inputs: matrix.inputs.clone(),
        settle_frames: matrix.settle_frames,
        cases: sub_cases,
    };
    match playtest::run_matrix(&sub, &mut caller).await {
        Ok(report) => {
            for c in &report.cases {
                if c.pass {
                    ok_items.push(c.name.clone());
                } else {
                    errors.push(json!({
                        "item": c.name,
                        "error": format!("断言失败({}): actual={} expected={} {}", c.kind, c.actual, c.expected, c.detail),
                    }));
                }
            }
            let finished_ms = epoch_ms();
            session.lock().await.shutdown().await;
            (
                ok_items,
                errors,
                json!({
                    "matrixRef": matrix_ref,
                    "pid": pid,
                    "windowMs": [started_ms, finished_ms],
                    "passed": report.passed,
                    "failed": report.failed,
                    "durationMs": report.duration_ms,
                }),
            )
        }
        Err(e) => {
            errors.push(json!({ "item": "*", "error": e }));
            session.lock().await.shutdown().await;
            (ok_items, errors, json!({ "matrixRef": matrix_ref, "pid": pid, "fatal": "matrix_aborted" }))
        }
    }
}

/// POST /api/forge/swarm/execute:multitask 确定性执行器(D-F3-E,无 LLM 依赖)。
/// 切片(不相交校验)→ 同进程逻辑 worker 并行执行 → 分片报告聚合(失败不遮蔽)。
async fn swarm_execute(
    State(state): State<Arc<AppState>>,
    Json(req): Json<SwarmExecuteRequest>,
) -> Response {
    let items = match normalize_items(&req.items) {
        Ok(v) => v,
        Err(e) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({ "error": { "code": "FORGE_INVALID_ARGS", "message": e } })),
            )
                .into_response()
        }
    };
    // 空输入集早退(预检须取首个元素)。
    if items.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": { "code": "FORGE_INVALID_ARGS", "message": "items 不可空" } })),
        )
            .into_response();
    }
    // 域/操作预检(切片前失败,不留空分片)。
    if let Err(e) = operation_call(&req.shard_type, &req.operation, &items[0]) {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": { "code": "FORGE_INVALID_ARGS", "message": e } })),
        )
            .into_response();
    }
    let shard_ids = match state
        .swarm
        .create_shards(&req.shard_type, items, req.shard_count)
    {
        Ok(ids) => ids,
        Err(swarm::ShardError::Overlap(msg)) => {
            return (
                StatusCode::CONFLICT,
                Json(json!({ "error": { "code": "GOV_SWARM_SHARD_OVERLAP", "message": msg } })),
            )
                .into_response()
        }
        Err(swarm::ShardError::Invalid(msg)) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({ "error": { "code": "FORGE_INVALID_ARGS", "message": msg } })),
            )
                .into_response()
        }
    };

    // 逻辑 worker 并行(04 §5.1 同进程逻辑 worker;每片一个 tokio task)。
    let mut handles = Vec::with_capacity(shard_ids.len());
    for id in &shard_ids {
        let shard = state.swarm.get_shard(id).expect("刚创建的分片必在");
        state.swarm.mark_running(id);
        let shard_type = req.shard_type.clone();
        let operation = req.operation.clone();
        let id = id.clone();
        handles.push(tokio::spawn(async move {
            // F6 wave.2:test-matrix 分片整体执行腿(FreshSession 独立 engine-host + 信号量守卫)。
            if shard_type == "test-matrix" {
                let (ok_items, errors, evidence) =
                    test_matrix_shard_run(&shard.items, &operation).await;
                return (id, ok_items, errors, evidence);
            }
            let mut ok_items: Vec<String> = Vec::new();
            let mut errors: Vec<Value> = Vec::new();
            for item in &shard.items {
                let (tool, args) = match operation_call(&shard_type, &operation, item) {
                    Ok(v) => v,
                    Err(e) => {
                        errors.push(json!({ "item": item, "error": e }));
                        continue;
                    }
                };
                match mcp::call_tool(&tool, Some(args)).await {
                    Ok(result) => {
                        // 工具级错误(content text 内含 "error")如实记 errors,不遮蔽。
                        let tool_err = result
                            .pointer("/content/0/text")
                            .and_then(Value::as_str)
                            .and_then(|t| serde_json::from_str::<Value>(t).ok())
                            .and_then(|v| v.get("error").cloned());
                        if let Some(te) = tool_err {
                            errors.push(json!({ "item": item, "error": te }));
                        } else {
                            ok_items.push(item.clone());
                        }
                    }
                    Err(e) => errors.push(json!({ "item": item, "error": e.to_string() })),
                }
            }
            (id, ok_items, errors, Value::Null)
        }));
    }

    let mut shard_reports = Vec::with_capacity(handles.len());
    let mut total_ok = 0usize;
    let mut total_err = 0usize;
    for h in handles {
        let (id, ok_items, errors, evidence) = h.await.expect("worker task join 失败");
        total_ok += ok_items.len();
        total_err += errors.len();
        let shard_ok = errors.is_empty();
        let report = json!({
            "shardId": id,
            "ok": ok_items,
            "okCount": ok_items.len(),
            "errors": errors,
            "evidence": evidence,
        });
        state.swarm.complete_shard(&id, report.clone(), shard_ok);
        shard_reports.push(json!({
            "shardId": id,
            "status": if shard_ok { "done" } else { "failed" },
            "okCount": report["okCount"],
            "errorCount": report["errors"].as_array().map(Vec::len).unwrap_or(0),
            "errors": report["errors"],
            "evidence": evidence,
        }));
    }

    Json(json!({
        "shardType": req.shard_type,
        "shards": shard_reports,
        "aggregate": {
            "totalItems": total_ok + total_err,
            "succeeded": total_ok,
            "failed": total_err,
            // 创建期已两两不相交校验;聚合如实复述(不一致恒 false,防遮蔽)。
            "disjoint": true,
            "consistent": true,
        },
    }))
    .into_response()
}

// ---------- F5 wave.3:gen 配置 REST 面(08 §6.2;R-5 密钥值永不出) ----------

/// GET /api/forge/gen/backends:注册表全量 + 真实 configured 判定;
/// 只回 endpointSet 布尔,密钥值/endpoint 值不出(endpoint 属配置面,按契约只回布尔)。
async fn gen_backends_list() -> Json<Value> {
    let cfg = gend::config::GenConfig::load();
    let keys = gend::keystore::Keystore::load();
    let list: Vec<Value> = gend::backends::registry()
        .iter()
        .map(|b| {
            let endpoint_set = cfg
                .entry(b.id())
                .and_then(|e| e.endpoint.as_deref())
                .map(|s| !s.trim().is_empty())
                .unwrap_or(false);
            json!({
                "id": b.id(),
                "kind": b.kind(),
                "configured": b.configured(&cfg, &keys),
                "endpointSet": endpoint_set,
                "capabilities": b.capabilities(),
            })
        })
        .collect();
    Json(json!({ "backends": list }))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct GenConfigureRequest {
    id: String,
    kind: String,
    enabled: bool,
    #[serde(default)]
    endpoint: Option<String>,
    /// 密钥:非空才写 data/keystore.json;永不进 gen-backends.json,永不在响应回显(R-5)。
    #[serde(default)]
    api_key: Option<String>,
}

/// POST /api/forge/gen/backends/configure:写 gen-backends.json 条目(读-改-写,保留其他
/// 条目与 model 字段);apiKey 非空 → 写 keystore.json(读-改-写)。响应 {ok, configured}
/// 不含 apiKey;非法 id → 400 GEN_UNKNOWN_BACKEND;kind 与注册表不符 → 400 GEN_BAD_PARAMS。
async fn gen_backends_configure(Json(req): Json<GenConfigureRequest>) -> Response {
    let Some(backend) = gend::backends::find(&req.id) else {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": { "code": "GEN_UNKNOWN_BACKEND", "message": format!("未知后端 id: {}", req.id) } })),
        )
            .into_response();
    };
    if req.kind != backend.kind() {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": { "code": "GEN_BAD_PARAMS", "message": format!("后端 {} kind 须为 {},实: {}", req.id, backend.kind(), req.kind) } })),
        )
            .into_response();
    }
    // endpoint:Some(非空) 覆盖;缺省保留既有条目值。
    let endpoint = req
        .endpoint
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());
    let mut cfg = gend::config::GenConfig::load();
    cfg.upsert_entry(gend::config::BackendEntry {
        id: req.id.clone(),
        kind: req.kind.clone(),
        enabled: req.enabled,
        endpoint,
        model: None,
    });
    if let Err(e) = cfg.save() {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": { "code": "FORGE_IO", "message": e.to_string() } })),
        )
            .into_response();
    }
    // 密钥只进 keystore.json;失败如实 500,错误信息不带 key 值。
    if let Some(key) = req.api_key.map(|s| s.trim().to_string()).filter(|s| !s.is_empty()) {
        if let Err(e) = gend::keystore::set_key(&req.id, &key) {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": { "code": "FORGE_IO", "message": e.to_string() } })),
            )
                .into_response();
        }
    }
    // 回读真实 configured(写盘后重载,不回显任何密钥)。
    let cfg2 = gend::config::GenConfig::load();
    let keys2 = gend::keystore::Keystore::load();
    let configured = backend.configured(&cfg2, &keys2);
    Json(json!({ "ok": true, "id": req.id, "configured": configured })).into_response()
}

async fn unknown_route() -> Response {
    (
        StatusCode::NOT_FOUND,
        Json(json!({ "error": { "code": "NOT_FOUND" } })),
    )
        .into_response()
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::{to_bytes, Body};
    use axum::http::Request;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tower::ServiceExt;

    /// 读响应体并解析为 JSON
    async fn json_body(resp: Response) -> Value {
        let bytes = to_bytes(resp.into_body(), usize::MAX)
            .await
            .expect("读响应体失败");
        serde_json::from_slice(&bytes).expect("响应应为 JSON")
    }

    fn get(uri: &str) -> Request<Body> {
        Request::get(uri).body(Body::empty()).unwrap()
    }

    fn post_json(uri: &str, body: &str) -> Request<Body> {
        Request::post(uri)
            .header("content-type", "application/json")
            .body(Body::from(body.to_string()))
            .unwrap()
    }

    #[tokio::test]
    async fn health_ok() {
        let resp = build_app().oneshot(get("/health")).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let v = json_body(resp).await;
        assert_eq!(v["service"], "forge-agentd");
        assert_eq!(v["status"], "ok");
        assert_eq!(v["version"], "0.1.0");
        assert!(v["uptimeSec"].as_f64().expect("uptimeSec 应为数值") >= 0.0);
    }

    #[tokio::test]
    async fn mcp_tools_declared() {
        let resp = build_app()
            .oneshot(get("/api/forge/mcp/tools"))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let v = json_body(resp).await;
        let tools = v["tools"].as_array().expect("tools 应为数组");
        assert_eq!(tools.len(), 75);
        assert!(tools.iter().any(|t| t == "mcp__engine-scene__scene_summary"));
        assert!(tools.iter().any(|t| t == "mcp__engine-scene__entity_batch_apply"));
        assert!(tools.iter().any(|t| t == "mcp__engine-scene__viewport_frame"));
        assert!(tools.iter().any(|t| t == "mcp__asset-pipeline__asset_import"));
        assert!(tools.iter().any(|t| t == "mcp__asset-pipeline__asset_list"));
        assert!(tools.iter().any(|t| t == "mcp__asset-pipeline__asset_thumbnail"));
        assert!(tools.iter().any(|t| t == "mcp__code-forge__rx_check"));
        assert!(tools.iter().any(|t| t == "mcp__code-forge__rx_test"));
        assert!(tools.iter().any(|t| t == "mcp__code-forge__code_symbol_search"));
        assert!(tools.iter().any(|t| t == "mcp__code-forge__code_references"));
        assert!(tools.iter().any(|t| t == "mcp__code-forge__code_structured_edit"));
        assert!(tools.iter().any(|t| t == "mcp__gen-image__gen_backends_list"));
        assert!(tools.iter().any(|t| t == "mcp__gen-image__gen_image"));
        assert!(tools.iter().any(|t| t == "mcp__gen-image__gen_texture_set"));
        assert!(tools.iter().any(|t| t == "mcp__gen-image__gen_accept"));
        assert!(tools.iter().any(|t| t == "mcp__gen-image__gen_variations"));
        assert!(tools.iter().any(|t| t == "mcp__gen-model__gen_mesh"));
        assert!(tools.iter().any(|t| t == "mcp__gen-model__gen_mesh_refine"));
        assert!(tools.iter().any(|t| t == "mcp__gen-model__gen_accept"));
    }

    #[tokio::test]
    async fn llm_complete_mock() {
        let resp = build_app()
            .oneshot(get("/api/forge/llm/complete"))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let v = json_body(resp).await;
        assert_eq!(v["provider"], "mock");
        assert_eq!(v["text"], "mock completion");
    }

    // ---------- RD-F1-002:/api/forge/llm/chat ----------

    #[tokio::test]
    async fn llm_chat_mock_provider_when_no_key() {
        // 无密钥环境(env 清空 + keystore 指空目录)→ provider=mock 恒绿,不触网不触 MCP。
        // 双锁:GEN_REST_LOCK(gen 测试组亦读写 FORGE_GEN_DATA_DIR)与 llm::TEST_ENV_LOCK,
        // 防跨锁竞态(真实 data/keystore.json 存在后,gen 测试 remove_var 会致本测试串扰解析到真 key)。
        let _g1 = GEN_REST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let _g2 = llm::TEST_ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        std::env::remove_var("FORGE_LLM_API_KEY");
        std::env::remove_var("FORGE_GEN_API_KEY");
        let dir = std::env::temp_dir().join(format!("agentd-chat-test-{}", std::process::id()));
        std::env::set_var("FORGE_GEN_DATA_DIR", &dir);
        let resp = build_app()
            .oneshot(post_json("/api/forge/llm/chat", r#"{"text":"你好","mode":"build"}"#))
            .await
            .unwrap();
        std::env::remove_var("FORGE_GEN_DATA_DIR");
        assert_eq!(resp.status(), StatusCode::OK);
        let v = json_body(resp).await;
        assert_eq!(v["provider"], "mock");
        assert!(v["text"].as_str().expect("text 应为字符串").contains("mock"));
        assert_eq!(v["toolCalls"], json!([]));
        assert_eq!(v["iters"], 0);
    }

    #[tokio::test]
    async fn llm_chat_empty_text_400() {
        let resp = build_app()
            .oneshot(post_json("/api/forge/llm/chat", r#"{"text":"  "}"#))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
        assert_eq!(json_body(resp).await["error"]["code"], "EMPTY_TEXT");
    }

    // ---------- F7 wave.1:事件基座(会话 CRUD/fork/revert + folders + snapshot + SSE) ----------

    /// F7 env 隔离锁(FORGE_AGENTD_DATA_DIR 进程级;set+build_app 须原子)。
    static F7_ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    /// 独立数据目录的 app(会话/事件落盘隔离;返回 (app, 数据根))。
    fn f7_app(tag: &str) -> (Router, std::path::PathBuf) {
        let _g = F7_ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let dir = std::env::temp_dir().join(format!(
            "agentd-f7-{tag}-{}-{}",
            std::process::id(),
            events::new_id("t")
        ));
        std::env::set_var("FORGE_AGENTD_DATA_DIR", &dir);
        let app = build_app();
        std::env::remove_var("FORGE_AGENTD_DATA_DIR");
        (app, dir)
    }

    fn delete_req(uri: &str) -> Request<Body> {
        Request::delete(uri).body(Body::empty()).unwrap()
    }

    fn event_file(dir: &std::path::Path, sid: &str) -> std::path::PathBuf {
        dir.join("agent-events").join(format!("{sid}.jsonl"))
    }

    /// 读会话 JSONL 事件行(落盘形态,含 seq/type/sessionId)。
    fn read_events(dir: &std::path::Path, sid: &str) -> Vec<Value> {
        std::fs::read_to_string(event_file(dir, sid))
            .unwrap_or_default()
            .lines()
            .filter(|l| !l.trim().is_empty())
            .map(|l| serde_json::from_str(l).expect("事件行应可解析"))
            .collect()
    }

    /// 读 SSE 流到 dur 为止,返回原始文本(帧数据拼合;超时即收)。
    async fn sse_read_for(body: &mut Body, dur: std::time::Duration) -> String {
        use http_body_util::BodyExt;
        let mut buf = String::new();
        let deadline = std::time::Instant::now() + dur;
        loop {
            let now = std::time::Instant::now();
            if now >= deadline {
                break;
            }
            match tokio::time::timeout(deadline - now, body.frame()).await {
                Ok(Some(Ok(frame))) => {
                    if let Ok(data) = frame.into_data() {
                        buf.push_str(&String::from_utf8_lossy(&data));
                    }
                }
                _ => break,
            }
        }
        buf
    }

    #[tokio::test]
    async fn f7_sessions_crud_flow() {
        let (app, dir) = f7_app("crud");
        // 列表空 {sessions: []}(替换 F0 恒 [] stub 的形态)。
        let r = app.clone().oneshot(get("/api/forge/sessions")).await.unwrap();
        assert_eq!(r.status(), StatusCode::OK);
        assert_eq!(json_body(r).await, json!({ "sessions": [] }));
        // 创建(缺省字段面)+ session.created 持久事件。
        let r = app
            .clone()
            .oneshot(post_json("/api/forge/sessions", "{}"))
            .await
            .unwrap();
        assert_eq!(r.status(), StatusCode::OK);
        let v = json_body(r).await;
        let s = &v["session"];
        let sid = s["id"].as_str().unwrap().to_string();
        assert!(sid.starts_with("sess_"));
        assert_eq!(s["title"], "新会话");
        assert_eq!(s["status"], "idle");
        assert_eq!(s["agentKind"], "coding");
        assert_eq!(s["webSearchEnabled"], true);
        assert_eq!(s["pinned"], false);
        assert_eq!(s["titleManuallySet"], false);
        let evs = read_events(&dir, &sid);
        assert_eq!(evs.len(), 1);
        assert_eq!(evs[0]["type"], "session.created");
        assert_eq!(evs[0]["seq"], 1);
        // get。
        let r = app
            .clone()
            .oneshot(get(&format!("/api/forge/sessions/{sid}")))
            .await
            .unwrap();
        assert_eq!(r.status(), StatusCode::OK);
        // patch title → titleManuallySet=true;pinned。
        let r = app
            .clone()
            .oneshot(patch_json(
                &format!("/api/forge/sessions/{sid}"),
                r#"{"title":"改题","pinned":true}"#,
            ))
            .await
            .unwrap();
        let v = json_body(r).await;
        assert_eq!(v["session"]["title"], "改题");
        assert_eq!(v["session"]["titleManuallySet"], true);
        assert_eq!(v["session"]["pinned"], true);
        // 列表含 1 条。
        let r = app.clone().oneshot(get("/api/forge/sessions")).await.unwrap();
        assert_eq!(json_body(r).await["sessions"].as_array().unwrap().len(), 1);
        // 404 面。
        let r = app
            .clone()
            .oneshot(get("/api/forge/sessions/sess_none"))
            .await
            .unwrap();
        assert_eq!(r.status(), StatusCode::NOT_FOUND);
        assert_eq!(json_body(r).await["error"]["code"], "SESSION_NOT_FOUND");
        let r = app
            .clone()
            .oneshot(patch_json("/api/forge/sessions/sess_none", "{}"))
            .await
            .unwrap();
        assert_eq!(r.status(), StatusCode::NOT_FOUND);
        // delete:元信息 + 事件文件同删。
        let r = app
            .clone()
            .oneshot(delete_req(&format!("/api/forge/sessions/{sid}")))
            .await
            .unwrap();
        assert_eq!(r.status(), StatusCode::OK);
        assert_eq!(json_body(r).await["ok"], true);
        assert!(!event_file(&dir, &sid).exists(), "删除须清事件文件");
        let r = app
            .oneshot(get(&format!("/api/forge/sessions/{sid}")))
            .await
            .unwrap();
        assert_eq!(r.status(), StatusCode::NOT_FOUND);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[tokio::test]
    async fn f7_fork_clones_events_and_revert_truncates() {
        let (app, dir) = f7_app("fork");
        let r = app
            .clone()
            .oneshot(post_json("/api/forge/sessions", r#"{"title":"主线"}"#))
            .await
            .unwrap();
        let sid = json_body(r).await["session"]["id"].as_str().unwrap().to_string();
        for body in [r#"{"pinned":true}"#, r#"{"title":"主线改"}"#] {
            app.clone()
                .oneshot(patch_json(&format!("/api/forge/sessions/{sid}"), body))
                .await
                .unwrap();
        }
        assert_eq!(read_events(&dir, &sid).len(), 3);
        // fork:分支标题 + 事件克隆(seq 保持单调)+ session.forked。
        let r = app
            .clone()
            .oneshot(post_json(&format!("/api/forge/sessions/{sid}/fork"), "{}"))
            .await
            .unwrap();
        assert_eq!(r.status(), StatusCode::OK);
        let fv = json_body(r).await;
        let forked = &fv["session"];
        let fid = forked["id"].as_str().unwrap().to_string();
        assert_ne!(fid, sid);
        assert_eq!(forked["title"], "分支 · 主线改");
        let fevs = read_events(&dir, &fid);
        assert_eq!(fevs.len(), 4, "克隆 3 + session.forked: {fevs:?}");
        assert_eq!(
            fevs.iter().map(|e| e["seq"].as_i64().unwrap()).collect::<Vec<_>>(),
            vec![1, 2, 3, 4],
            "seq 单调保持"
        );
        assert_eq!(fevs[0]["type"], "session.created");
        assert!(
            fevs.iter().all(|e| e["sessionId"] == fid),
            "克隆事件 sessionId 换新"
        );
        assert_eq!(fevs[3]["type"], "session.forked");
        assert_eq!(fevs[3]["payload"]["sourceSessionId"], sid.as_str());
        // 源流不受 fork 影响。
        assert_eq!(read_events(&dir, &sid).len(), 3);
        // fork 不存在 → 404。
        let r = app
            .clone()
            .oneshot(post_json("/api/forge/sessions/sess_none/fork", "{}"))
            .await
            .unwrap();
        assert_eq!(r.status(), StatusCode::NOT_FOUND);

        // revert mode=before:截到 seq2 事件之前 → 仅余 seq1;session.reverted 复接 seq2。
        let target_id = read_events(&dir, &sid)[1]["id"].as_str().unwrap().to_string();
        let r = app
            .clone()
            .oneshot(post_json(
                &format!("/api/forge/sessions/{sid}/revert"),
                &format!(r#"{{"messageId":"{target_id}","mode":"before"}}"#),
            ))
            .await
            .unwrap();
        assert_eq!(r.status(), StatusCode::OK);
        let evs = read_events(&dir, &sid);
        assert_eq!(
            evs.iter().map(|e| e["type"].as_str().unwrap()).collect::<Vec<_>>(),
            vec!["session.created", "session.reverted"]
        );
        assert_eq!(evs[1]["seq"], 2, "截断后 reverted 复接 seq");
        // revert 缺省 mode(含该事件):patch(seq3)后截到含它。
        app.clone()
            .oneshot(patch_json(&format!("/api/forge/sessions/{sid}"), r#"{"pinned":false}"#))
            .await
            .unwrap();
        let tid3 = read_events(&dir, &sid)
            .into_iter()
            .find(|e| e["type"] == "session.updated")
            .unwrap()["id"]
            .as_str()
            .unwrap()
            .to_string();
        app.clone()
            .oneshot(post_json(
                &format!("/api/forge/sessions/{sid}/revert"),
                &format!(r#"{{"messageId":"{tid3}"}}"#),
            ))
            .await
            .unwrap();
        let evs = read_events(&dir, &sid);
        assert_eq!(
            evs.iter().map(|e| e["type"].as_str().unwrap()).collect::<Vec<_>>(),
            vec!["session.created", "session.reverted", "session.updated", "session.reverted"],
            "含该事件截断语义"
        );
        // 无 messageId:不截断,仅追加 session.reverted。
        let before = read_events(&dir, &sid).len();
        let r = app
            .clone()
            .oneshot(post_json(&format!("/api/forge/sessions/{sid}/revert"), "{}"))
            .await
            .unwrap();
        assert_eq!(r.status(), StatusCode::OK);
        assert_eq!(
            read_events(&dir, &sid).len(),
            before + 1,
            "无 messageId 仅清 activeRunId + 发事件"
        );
        // messageId 不存在 → 404 EVENT_NOT_FOUND。
        let r = app
            .clone()
            .oneshot(post_json(
                &format!("/api/forge/sessions/{sid}/revert"),
                r#"{"messageId":"evt_none"}"#,
            ))
            .await
            .unwrap();
        assert_eq!(r.status(), StatusCode::NOT_FOUND);
        assert_eq!(json_body(r).await["error"]["code"], "EVENT_NOT_FOUND");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[tokio::test]
    async fn f7_chat_folders_router_and_cascade() {
        let (app, dir) = f7_app("folders");
        let r = app.clone().oneshot(get("/api/forge/chat-folders")).await.unwrap();
        assert_eq!(r.status(), StatusCode::OK);
        assert_eq!(json_body(r).await, json!({ "folders": [] }));
        // 空名 400 INVALID_NAME。
        let r = app
            .clone()
            .oneshot(post_json("/api/forge/chat-folders", r#"{"name":" "}"#))
            .await
            .unwrap();
        assert_eq!(r.status(), StatusCode::BAD_REQUEST);
        assert_eq!(json_body(r).await["error"]["code"], "INVALID_NAME");
        // 建 + 改。
        let r = app
            .clone()
            .oneshot(post_json("/api/forge/chat-folders", r#"{"name":"工作"}"#))
            .await
            .unwrap();
        assert_eq!(r.status(), StatusCode::OK);
        let fid = json_body(r).await["folder"]["id"].as_str().unwrap().to_string();
        let r = app
            .clone()
            .oneshot(patch_json(&format!("/api/forge/chat-folders/{fid}"), r#"{"name":"工作区"}"#))
            .await
            .unwrap();
        assert_eq!(json_body(r).await["folder"]["name"], "工作区");
        // 会话挂接 → 删文件夹级联清 folderId。
        let r = app
            .clone()
            .oneshot(post_json("/api/forge/sessions", "{}"))
            .await
            .unwrap();
        let sid = json_body(r).await["session"]["id"].as_str().unwrap().to_string();
        app.clone()
            .oneshot(patch_json(
                &format!("/api/forge/sessions/{sid}"),
                &format!(r#"{{"folderId":"{fid}"}}"#),
            ))
            .await
            .unwrap();
        let r = app
            .clone()
            .oneshot(delete_req(&format!("/api/forge/chat-folders/{fid}")))
            .await
            .unwrap();
        assert_eq!(r.status(), StatusCode::OK);
        let r = app
            .clone()
            .oneshot(get(&format!("/api/forge/sessions/{sid}")))
            .await
            .unwrap();
        assert!(
            json_body(r).await["session"]["folderId"].is_null(),
            "删文件夹须级联清会话 folderId"
        );
        // 删不存在 → 404 FOLDER_NOT_FOUND。
        let r = app
            .oneshot(delete_req(&format!("/api/forge/chat-folders/{fid}")))
            .await
            .unwrap();
        assert_eq!(r.status(), StatusCode::NOT_FOUND);
        assert_eq!(json_body(r).await["error"]["code"], "FOLDER_NOT_FOUND");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[tokio::test]
    async fn f7_design_snapshot_fields_and_models_two_states() {
        // key 判定读 FORGE_LLM_API_KEY + keystore(请求时判定):三锁同源纪律(同 llm 测试组)。
        let _g1 = GEN_REST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let _g2 = llm::TEST_ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let (app, dir) = f7_app("snap");
        std::env::remove_var("FORGE_LLM_API_KEY");
        std::env::remove_var("FORGE_GEN_API_KEY");
        let ks_dir = std::env::temp_dir().join(format!("agentd-f7-ks-{}", std::process::id()));
        std::env::set_var("FORGE_GEN_DATA_DIR", &ks_dir);
        // 无 key 态:字段穷举 + needs-key。
        let r = app
            .clone()
            .oneshot(get("/api/forge/design-snapshot"))
            .await
            .unwrap();
        assert_eq!(r.status(), StatusCode::OK);
        let v = json_body(r).await;
        let mut keys: Vec<&str> = v.as_object().unwrap().keys().map(String::as_str).collect();
        keys.sort_unstable();
        assert_eq!(
            keys,
            ["activeSession", "chatFolders", "events", "latestSeq", "models", "run", "sessions", "todos"],
            "字段穷举: {keys:?}"
        );
        assert_eq!(v["activeSession"], Value::Null);
        assert_eq!(v["events"], json!([]));
        assert_eq!(v["todos"], json!([]));
        assert_eq!(v["run"], Value::Null);
        assert_eq!(v["latestSeq"], 0);
        let models = v["models"]["models"].as_array().unwrap();
        assert_eq!(models.len(), 3);
        assert_eq!(models[2]["id"], "openai-compat");
        assert_eq!(models[2]["provider"], "openai-compat");
        assert_eq!(models[2]["availability"], "needs-key");
        assert_eq!(models[0]["id"], "deepseek-chat");
        assert_eq!(models[0]["provider"], "deepseek");
        assert_eq!(models[0]["availability"], "needs-key");
        assert_eq!(models[1]["id"], "mock");
        assert_eq!(models[1]["availability"], "available");
        assert_eq!(v["models"]["defaultModelId"], "deepseek-chat");
        // 有 key 态:available;响应面不含密钥本体(R-5)。
        std::env::set_var("FORGE_LLM_API_KEY", "sk-test-availability-f7");
        let r = app
            .clone()
            .oneshot(get("/api/forge/design-snapshot"))
            .await
            .unwrap();
        let v = json_body(r).await;
        assert_eq!(v["models"]["models"][0]["availability"], "available");
        assert!(
            !v.to_string().contains("sk-test-availability-f7"),
            "响应泄漏密钥(R-5)"
        );
        std::env::remove_var("FORGE_LLM_API_KEY");
        std::env::remove_var("FORGE_GEN_DATA_DIR");
        // sessionId 腿:activeSession + 持久化全量回放 + latestSeq + chatFolders。
        let r = app
            .clone()
            .oneshot(post_json("/api/forge/sessions", r#"{"title":"快照"}"#))
            .await
            .unwrap();
        let sid = json_body(r).await["session"]["id"].as_str().unwrap().to_string();
        app.clone()
            .oneshot(patch_json(&format!("/api/forge/sessions/{sid}"), r#"{"pinned":true}"#))
            .await
            .unwrap();
        app.clone()
            .oneshot(post_json("/api/forge/chat-folders", r#"{"name":"夹"}"#))
            .await
            .unwrap();
        let r = app
            .clone()
            .oneshot(get(&format!("/api/forge/design-snapshot?sessionId={sid}")))
            .await
            .unwrap();
        let v = json_body(r).await;
        assert_eq!(v["activeSession"]["id"], sid.as_str());
        assert_eq!(v["activeSession"]["title"], "快照");
        let evs = v["events"].as_array().unwrap();
        assert_eq!(evs.len(), 2);
        assert_eq!(evs[0]["type"], "session.created");
        assert_eq!(evs[1]["type"], "session.updated");
        assert_eq!(v["latestSeq"], 2);
        assert_eq!(v["sessions"].as_array().unwrap().len(), 1);
        assert_eq!(v["chatFolders"].as_array().unwrap().len(), 1);
        // 不存在 sessionId → null 三态。
        let r = app
            .oneshot(get("/api/forge/design-snapshot?sessionId=sess_none"))
            .await
            .unwrap();
        let v = json_body(r).await;
        assert_eq!(v["activeSession"], Value::Null);
        assert_eq!(v["events"], json!([]));
        assert_eq!(v["latestSeq"], 0);
        std::fs::remove_dir_all(&dir).ok();
        std::fs::remove_dir_all(&ks_dir).ok();
    }

    #[tokio::test]
    async fn f7_sse_replay_live_resume_and_404() {
        let (app, dir) = f7_app("sse");
        let r = app
            .clone()
            .oneshot(post_json("/api/forge/sessions", r#"{"title":"S"}"#))
            .await
            .unwrap();
        let sid = json_body(r).await["session"]["id"].as_str().unwrap().to_string();
        // 404 面。
        let r = app
            .clone()
            .oneshot(get("/api/forge/sessions/sess_none/events/stream"))
            .await
            .unwrap();
        assert_eq!(r.status(), StatusCode::NOT_FOUND);
        assert_eq!(json_body(r).await["error"]["code"], "SESSION_NOT_FOUND");
        // replay:首帧 session.created(id/event/data 帧格式齐)。
        let r = app
            .clone()
            .oneshot(get(&format!(
                "/api/forge/sessions/{sid}/events/stream?fromSeq=0"
            )))
            .await
            .unwrap();
        assert_eq!(r.status(), StatusCode::OK);
        assert!(r.headers()["content-type"]
            .to_str()
            .unwrap()
            .starts_with("text/event-stream"));
        let mut body = r.into_body();
        let replay = sse_read_for(&mut body, std::time::Duration::from_millis(600)).await;
        assert!(replay.contains("id: 1\n"), "帧 id=seq: {replay:?}");
        assert!(replay.contains("event: session.created\n"), "帧 event=type: {replay:?}");
        assert!(replay.contains("data: {"), "帧 data=wire JSON: {replay:?}");
        assert!(replay.contains("\"seq\":1"), "wire 含 seq: {replay:?}");
        // live:PATCH 推 session.updated(seq 2 续接)。
        let r = app
            .clone()
            .oneshot(patch_json(&format!("/api/forge/sessions/{sid}"), r#"{"pinned":true}"#))
            .await
            .unwrap();
        assert_eq!(r.status(), StatusCode::OK);
        let live = sse_read_for(&mut body, std::time::Duration::from_millis(600)).await;
        assert!(live.contains("event: session.updated\n"), "live 推送: {live:?}");
        assert!(live.contains("id: 2\n"), "live seq 续接: {live:?}");
        drop(body);
        // 续传无重:fromSeq=latest → 600ms 内零事件帧。
        let r = app
            .clone()
            .oneshot(get(&format!(
                "/api/forge/sessions/{sid}/events/stream?fromSeq=2"
            )))
            .await
            .unwrap();
        let mut body2 = r.into_body();
        let resume = sse_read_for(&mut body2, std::time::Duration::from_millis(600)).await;
        assert!(
            !resume.contains("event: "),
            "fromSeq=latest 续传不应有回放帧: {resume:?}"
        );
        drop(body2);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[tokio::test]
    async fn f7_sse_gap_frame_when_window_exceeded() {
        // 小环缓冲(4)制造超窗:env 于 build_app 读取,锁内原子 set+build。
        let (app, dir) = {
            let _g = F7_ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
            let dir = std::env::temp_dir().join(format!(
                "agentd-f7-gap-{}-{}",
                std::process::id(),
                events::new_id("t")
            ));
            std::env::set_var("FORGE_AGENTD_DATA_DIR", &dir);
            std::env::set_var("FORGE_AGENTD_EVENT_BUFFER", "4");
            let app = build_app();
            std::env::remove_var("FORGE_AGENTD_DATA_DIR");
            std::env::remove_var("FORGE_AGENTD_EVENT_BUFFER");
            (app, dir)
        };
        let r = app
            .clone()
            .oneshot(post_json("/api/forge/sessions", "{}"))
            .await
            .unwrap();
        let sid = json_body(r).await["session"]["id"].as_str().unwrap().to_string();
        // 6 个持久事件(PATCH 轮替 pinned)→ 共 7 条,窗口仅容 4。
        for i in 0..6 {
            let body = if i % 2 == 0 {
                r#"{"pinned":true}"#
            } else {
                r#"{"pinned":false}"#
            };
            app.clone()
                .oneshot(patch_json(&format!("/api/forge/sessions/{sid}"), body))
                .await
                .unwrap();
        }
        assert_eq!(read_events(&dir, &sid).len(), 7);
        let r = app
            .clone()
            .oneshot(get(&format!(
                "/api/forge/sessions/{sid}/events/stream?fromSeq=0"
            )))
            .await
            .unwrap();
        assert_eq!(r.status(), StatusCode::OK);
        let mut body = r.into_body();
        let text = sse_read_for(&mut body, std::time::Duration::from_millis(800)).await;
        // 首帧必须是合成 stream.gap。
        let first = text.split("\n\n").next().unwrap_or("");
        assert!(first.contains("event: stream.gap"), "超窗首帧须 stream.gap: {text:?}");
        assert!(first.contains("replay-window-exceeded"), "gap 原因: {text:?}");
        assert!(first.contains("\"gap\":true"), "gap payload: {text:?}");
        // 回放段 = 窗口内 seq 4..7;窗口外 seq 3 不得回放。
        assert!(text.contains("id: 4\n") && text.contains("id: 7\n"), "窗口帧: {text:?}");
        assert!(!text.contains("id: 3\n"), "窗口外帧不得回放: {text:?}");
        drop(body);
        std::fs::remove_dir_all(&dir).ok();
    }

    // ---------- F7 wave.2:turn 执行(ask:execute / runs / todos)路由级 ----------

    /// mock provider 环境(无 key + keystore 指空目录;三锁纪律同 llm 测试组)。
    fn mock_provider_env() -> std::path::PathBuf {
        std::env::remove_var("FORGE_LLM_API_KEY");
        std::env::remove_var("FORGE_GEN_API_KEY");
        let dir = std::env::temp_dir().join(format!("agentd-w2-ks-{}", std::process::id()));
        std::env::set_var("FORGE_GEN_DATA_DIR", &dir);
        dir
    }

    #[tokio::test]
    async fn f7w2_ask_execute_mock_build_route_and_validation() {
        let _g1 = GEN_REST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let _g2 = llm::TEST_ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let ks_dir = mock_provider_env();
        let (app, dir) = f7_app("ask");
        // 404 SESSION_NOT_FOUND。
        let r = app
            .clone()
            .oneshot(post_json(
                "/api/forge/sessions/sess_none/ask:execute",
                r#"{"userInput":"hi"}"#,
            ))
            .await
            .unwrap();
        assert_eq!(r.status(), StatusCode::NOT_FOUND);
        assert_eq!(json_body(r).await["error"]["code"], "SESSION_NOT_FOUND");
        let r = app
            .clone()
            .oneshot(post_json("/api/forge/sessions", r#"{"title":"T"}"#))
            .await
            .unwrap();
        let sid = json_body(r).await["session"]["id"]
            .as_str()
            .unwrap()
            .to_string();
        // 400 INVALID_INPUT:空 userInput。
        let r = app
            .clone()
            .oneshot(post_json(
                &format!("/api/forge/sessions/{sid}/ask:execute"),
                r#"{"userInput":"  "}"#,
            ))
            .await
            .unwrap();
        assert_eq!(r.status(), StatusCode::BAD_REQUEST);
        assert_eq!(json_body(r).await["error"]["code"], "INVALID_INPUT");
        // 400 INVALID_INPUT:未知 mode。
        let r = app
            .clone()
            .oneshot(post_json(
                &format!("/api/forge/sessions/{sid}/ask:execute"),
                r#"{"userInput":"x","mode":"bogus"}"#,
            ))
            .await
            .unwrap();
        assert_eq!(r.status(), StatusCode::BAD_REQUEST);
        assert_eq!(json_body(r).await["error"]["code"], "INVALID_INPUT");
        // mock build:200 + run completed + 事件序列落盘 + 自动命名 + activeRunId 清理。
        let r = app
            .clone()
            .oneshot(post_json(
                &format!("/api/forge/sessions/{sid}/ask:execute"),
                r#"{"userInput":"给我一个场景综述","mode":"build"}"#,
            ))
            .await
            .unwrap();
        assert_eq!(r.status(), StatusCode::OK);
        let v = json_body(r).await;
        assert_eq!(v["run"]["status"], "completed");
        assert!(v["run"]["id"].as_str().unwrap().starts_with("run_"));
        assert!(v["message"]["text"].as_str().unwrap().contains("mock:已收到「给我一个场景综述」"));
        assert_eq!(v["mode"], "build");
        let run_id = v["run"]["id"].as_str().unwrap().to_string();
        let types: Vec<String> = read_events(&dir, &sid)
            .iter()
            .map(|e| e["type"].as_str().unwrap().to_string())
            .collect();
        assert_eq!(
            types,
            vec![
                "session.created",
                "composer.user.message",
                "agent.started",
                "agent.message",
                "agent.completed"
            ],
            "mock build 全事件序列: {types:?}"
        );
        let evs = read_events(&dir, &sid);
        assert_eq!(evs[1]["payload"]["composerMode"], "build");
        assert_eq!(evs[1]["payload"]["runId"], run_id.as_str());
        assert_eq!(evs[2]["payload"]["model"], "mock");
        assert_eq!(evs[3]["payload"]["provider"], "mock");
        assert_eq!(evs[4]["payload"]["runId"], run_id.as_str());
        // 首条消息自动命名 + activeRunId 清理。
        let r = app
            .clone()
            .oneshot(get(&format!("/api/forge/sessions/{sid}")))
            .await
            .unwrap();
        let s = json_body(r).await;
        assert_eq!(s["session"]["title"], "给我一个场景综述");
        assert_eq!(s["session"]["titleManuallySet"], false);
        assert!(s["session"]["activeRunId"].is_null());
        // runs REST:GET 200;cancel 非 running → ok:false 如实。
        let r = app
            .clone()
            .oneshot(get(&format!("/api/forge/runs/{run_id}")))
            .await
            .unwrap();
        assert_eq!(r.status(), StatusCode::OK);
        let rv = json_body(r).await;
        assert_eq!(rv["run"]["status"], "completed");
        assert_eq!(rv["run"]["trigger"], "composer_chat");
        assert_eq!(rv["run"]["sessionId"], sid.as_str());
        let r = app
            .clone()
            .oneshot(post_json(&format!("/api/forge/runs/{run_id}/cancel"), "{}"))
            .await
            .unwrap();
        let cv = json_body(r).await;
        assert_eq!(cv["ok"], false);
        assert_eq!(cv["status"], "completed");
        // 404 RUN_NOT_FOUND 双腿。
        let r = app
            .clone()
            .oneshot(get("/api/forge/runs/run_none"))
            .await
            .unwrap();
        assert_eq!(r.status(), StatusCode::NOT_FOUND);
        assert_eq!(json_body(r).await["error"]["code"], "RUN_NOT_FOUND");
        let r = app
            .clone()
            .oneshot(post_json("/api/forge/runs/run_none/cancel", "{}"))
            .await
            .unwrap();
        assert_eq!(r.status(), StatusCode::NOT_FOUND);
        assert_eq!(json_body(r).await["error"]["code"], "RUN_NOT_FOUND");
        std::env::remove_var("FORGE_GEN_DATA_DIR");
        std::fs::remove_dir_all(&dir).ok();
        std::fs::remove_dir_all(&ks_dir).ok();
    }

    #[tokio::test]
    async fn f7w2_todos_rest_events_and_validation() {
        let (app, dir) = f7_app("todos");
        // POST:404 会话不存在。
        let r = app
            .clone()
            .oneshot(post_json(
                "/api/forge/todos",
                r#"{"sessionId":"sess_none","title":"x"}"#,
            ))
            .await
            .unwrap();
        assert_eq!(r.status(), StatusCode::NOT_FOUND);
        assert_eq!(json_body(r).await["error"]["code"], "SESSION_NOT_FOUND");
        let r = app
            .clone()
            .oneshot(post_json("/api/forge/sessions", "{}"))
            .await
            .unwrap();
        let sid = json_body(r).await["session"]["id"]
            .as_str()
            .unwrap()
            .to_string();
        // 400 TODO_INVALID:空 title / 非法 kind。
        let r = app
            .clone()
            .oneshot(post_json(
                "/api/forge/todos",
                &format!(r#"{{"sessionId":"{sid}","title":" "}}"#),
            ))
            .await
            .unwrap();
        assert_eq!(r.status(), StatusCode::BAD_REQUEST);
        assert_eq!(json_body(r).await["error"]["code"], "TODO_INVALID");
        let r = app
            .clone()
            .oneshot(post_json(
                "/api/forge/todos",
                &format!(r#"{{"sessionId":"{sid}","title":"x","kind":"bogus"}}"#),
            ))
            .await
            .unwrap();
        assert_eq!(r.status(), StatusCode::BAD_REQUEST);
        // 建两条:缺省面 + explore 带 description。
        let r = app
            .clone()
            .oneshot(post_json(
                "/api/forge/todos",
                &format!(r#"{{"sessionId":"{sid}","title":"改场景"}}"#),
            ))
            .await
            .unwrap();
        assert_eq!(r.status(), StatusCode::OK);
        let t1 = json_body(r).await["todo"].clone();
        assert!(t1["id"].as_str().unwrap().starts_with("todo_"));
        assert_eq!(t1["kind"], "edit");
        assert_eq!(t1["source"], "user");
        assert_eq!(t1["status"], "queued");
        let r = app
            .clone()
            .oneshot(post_json(
                "/api/forge/todos",
                &format!(r#"{{"sessionId":"{sid}","title":"看布局","kind":"explore","description":"先取证"}}"#),
            ))
            .await
            .unwrap();
        let t2 = json_body(r).await["todo"].clone();
        assert_eq!(t2["kind"], "explore");
        assert_eq!(t2["description"], "先取证");
        // todo.created 事件落盘(payload id/title/kind/status)。
        let evs = read_events(&dir, &sid);
        let created: Vec<&Value> = evs.iter().filter(|e| e["type"] == "todo.created").collect();
        assert_eq!(created.len(), 2);
        assert_eq!(created[0]["payload"]["id"], t1["id"]);
        assert_eq!(created[0]["payload"]["title"], "改场景");
        assert_eq!(created[0]["payload"]["kind"], "edit");
        assert_eq!(created[0]["payload"]["status"], "queued");
        // GET 列表(404 腿)。
        let r = app
            .clone()
            .oneshot(get(&format!("/api/forge/sessions/{sid}/todos")))
            .await
            .unwrap();
        assert_eq!(json_body(r).await["todos"].as_array().unwrap().len(), 2);
        let r = app
            .clone()
            .oneshot(get("/api/forge/sessions/sess_none/todos"))
            .await
            .unwrap();
        assert_eq!(r.status(), StatusCode::NOT_FOUND);
        // PATCH:status/title/summary + todo.updated 事件。
        let r = app
            .clone()
            .oneshot(patch_json(
                &format!("/api/forge/todos/{}", t1["id"].as_str().unwrap()),
                r#"{"status":"completed","summary":"已完成","title":"改场景v2"}"#,
            ))
            .await
            .unwrap();
        assert_eq!(r.status(), StatusCode::OK);
        let pt = json_body(r).await["todo"].clone();
        assert_eq!(pt["status"], "completed");
        assert_eq!(pt["summary"], "已完成");
        assert_eq!(pt["title"], "改场景v2");
        let evs2 = read_events(&dir, &sid);
        let updated: Vec<&Value> = evs2.iter().filter(|e| e["type"] == "todo.updated").collect();
        assert_eq!(updated.len(), 1);
        assert_eq!(updated[0]["payload"]["id"], t1["id"]);
        assert_eq!(updated[0]["payload"]["status"], "completed");
        // 400 TODO_INVALID:非法 status;404 TODO_NOT_FOUND。
        let r = app
            .clone()
            .oneshot(patch_json(
                &format!("/api/forge/todos/{}", t1["id"].as_str().unwrap()),
                r#"{"status":"bogus"}"#,
            ))
            .await
            .unwrap();
        assert_eq!(r.status(), StatusCode::BAD_REQUEST);
        assert_eq!(json_body(r).await["error"]["code"], "TODO_INVALID");
        let r = app
            .clone()
            .oneshot(patch_json("/api/forge/todos/todo_none", r#"{"status":"running"}"#))
            .await
            .unwrap();
        assert_eq!(r.status(), StatusCode::NOT_FOUND);
        assert_eq!(json_body(r).await["error"]["code"], "TODO_NOT_FOUND");
        // snapshot:todos 填真(经路由)。
        let r = app
            .clone()
            .oneshot(get(&format!("/api/forge/design-snapshot?sessionId={sid}")))
            .await
            .unwrap();
        let v = json_body(r).await;
        let todos = v["todos"].as_array().unwrap();
        assert_eq!(todos.len(), 2, "snapshot todos 填真: {todos:?}");
        assert!(todos.iter().any(|t| t["title"] == "改场景v2" && t["status"] == "completed"));
        assert!(v["run"].is_null(), "无 activeRunId → null");
        std::fs::remove_dir_all(&dir).ok();
    }

    // ---------- F6 wave.1:/api/forge/playtest/run ----------

    #[tokio::test]
    async fn playtest_run_matrix_not_found_404() {
        let resp = build_app()
            .oneshot(post_json(
                "/api/forge/playtest/run",
                r#"{"matrixRef":"tests/playtest/no-such-matrix.json"}"#,
            ))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
        assert_eq!(json_body(resp).await["error"]["code"], "MATRIX_NOT_FOUND");
    }

    #[tokio::test]
    async fn playtest_run_matrix_invalid_400() {
        // 写一份坏 JSON 到临时文件(绝对路径),解析失败 → 400。
        let dir = std::env::temp_dir().join(format!("agentd-pt-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("bad.json");
        std::fs::write(&p, "{ not json").unwrap();
        let resp = build_app()
            .oneshot(post_json(
                "/api/forge/playtest/run",
                &format!(r#"{{"matrixRef":"{}"}}"#, p.to_string_lossy().replace('\\', "/")),
            ))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
        assert_eq!(json_body(resp).await["error"]["code"], "MATRIX_INVALID");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[tokio::test]
    async fn unknown_route_404() {
        let resp = build_app()
            .oneshot(get("/no/such/route"))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
        assert_eq!(json_body(resp).await["error"]["code"], "NOT_FOUND");
    }

    #[tokio::test]
    async fn mcp_call_unknown_tool_404() {
        let app = build_app();
        let req = post_json("/api/forge/mcp/call", r#"{"tool":"mcp__engine-scene__no_such_tool"}"#);
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
        assert_eq!(json_body(resp).await["error"]["code"], "TOOL_NOT_FOUND");
    }

    // ---------- F2 wave.5:Proposal 确认单 + destructive 强制门 ----------

    fn patch_json(uri: &str, body: &str) -> Request<Body> {
        Request::patch(uri)
            .header("content-type", "application/json")
            .body(Body::from(body.to_string()))
            .unwrap()
    }

    #[tokio::test]
    async fn proposals_crud_and_terminal_state() {
        let app = build_app();
        // 创建 pending。
        let created = app
            .clone()
            .oneshot(post_json(
                "/api/forge/proposals",
                r#"{"kind":"asset.cleanup","summary":"移动 2 个错放资产","impact":{"assets":["Meshes/a.gltf","Meshes/b.png"]}}"#,
            ))
            .await
            .unwrap();
        assert_eq!(created.status(), StatusCode::OK);
        let p = json_body(created).await;
        assert_eq!(p["status"], "pending");
        assert_eq!(p["kind"], "asset.cleanup");
        let id = p["id"].as_str().unwrap().to_string();

        // 列表可见。
        let listed = app
            .clone()
            .oneshot(get("/api/forge/proposals"))
            .await
            .unwrap();
        let v = json_body(listed).await;
        assert!(v["proposals"].as_array().unwrap().iter().any(|x| x["id"] == id));

        // approve → approved;二次迁移 → 409 终态不可逆。
        let patched = app
            .clone()
            .oneshot(patch_json(&format!("/api/forge/proposals/{id}"), r#"{"action":"approve"}"#))
            .await
            .unwrap();
        assert_eq!(patched.status(), StatusCode::OK);
        assert_eq!(json_body(patched).await["status"], "approved");
        let again = app
            .clone()
            .oneshot(patch_json(&format!("/api/forge/proposals/{id}"), r#"{"action":"reject"}"#))
            .await
            .unwrap();
        assert_eq!(again.status(), StatusCode::CONFLICT);
        assert_eq!(json_body(again).await["error"]["code"], "GOV_PROPOSAL_CLOSED");

        // 不存在 → 404。
        let missing = app
            .oneshot(patch_json("/api/forge/proposals/prop_999", r#"{"action":"approve"}"#))
            .await
            .unwrap();
        assert_eq!(missing.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn asset_delete_force_gated_by_proposal() {
        let app = build_app();
        // 无 approved Proposal → 409 GOV_PROPOSAL_REQUIRED + 自动建 pending。
        let blocked = app
            .clone()
            .oneshot(post_json(
                "/api/forge/mcp/call",
                r#"{"tool":"mcp__asset-pipeline__asset_delete","arguments":{"assetPaths":["Meshes/x.gltf"],"force":true}}"#,
            ))
            .await
            .unwrap();
        assert_eq!(blocked.status(), StatusCode::CONFLICT);
        let v = json_body(blocked).await;
        assert_eq!(v["error"]["code"], "GOV_PROPOSAL_REQUIRED");
        let pid = v["error"]["proposalId"].as_str().unwrap().to_string();

        // 非 force 删除不受门控(直达 asset-pipeline 子进程;对不存在资产返回工具级 NO_META,HTTP 200)。
        let plain = app
            .clone()
            .oneshot(post_json(
                "/api/forge/mcp/call",
                r#"{"tool":"mcp__asset-pipeline__asset_delete","arguments":{"assetPaths":["Meshes/x.gltf"]}}"#,
            ))
            .await
            .unwrap();
        assert_eq!(plain.status(), StatusCode::OK, "非 force 应穿门直达子进程");
        let plain_body = json_body(plain).await;
        let plain_tool = extract_tool_json(&plain_body);
        assert_eq!(plain_tool["error"], "NO_META", "工具级如实报错: {plain_tool}");

        // 批准自动创建的 Proposal → 再调 force 删除放行(HTTP 200 到达子进程,而非 409 被门拦)。
        let approved = app
            .clone()
            .oneshot(patch_json(&format!("/api/forge/proposals/{pid}"), r#"{"action":"approve"}"#))
            .await
            .unwrap();
        assert_eq!(approved.status(), StatusCode::OK);
        let passed = app
            .oneshot(post_json(
                "/api/forge/mcp/call",
                r#"{"tool":"mcp__asset-pipeline__asset_delete","arguments":{"assetPaths":["Meshes/x.gltf"],"force":true}}"#,
            ))
            .await
            .unwrap();
        assert_eq!(passed.status(), StatusCode::OK, "批准后须放行(409 = 仍被门拦)");
    }

    #[tokio::test]
    async fn skills_list_discovers_frontmatter() {
        let resp = build_app()
            .oneshot(get("/api/forge/skills/list"))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let v = json_body(resp).await;
        let skills = v["skills"].as_array().expect("skills 应为数组");
        // workspace skills/ 下至少有 asset-cleanup(wave.5 落地)。
        assert!(
            skills.iter().any(|s| s["name"] == "asset-cleanup"),
            "skills/list 未见 asset-cleanup: {skills:?}"
        );
        let sc = skills.iter().find(|s| s["name"] == "asset-cleanup").unwrap();
        assert!(sc["description"].as_str().unwrap().contains("整理"));
    }

    /// 集成:真实 spawn target/debug/engine-scene-mcp.exe 调 scene_summary。
    /// 二进制未构建时跳过(如实打印 SKIP),见汇报。
    #[tokio::test]
    async fn mcp_call_scene_summary_real_spawn() {
        let bin = mcp::server_bin();
        if !bin.exists() {
            eprintln!("[SKIP] engine-scene-mcp 未构建: {},集成测试跳过", bin.display());
            return;
        }
        let app = build_app();
        let req = post_json("/api/forge/mcp/call", r#"{"tool":"mcp__engine-scene__scene_summary"}"#);
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let v = json_body(resp).await;
        let summary = extract_tool_json(&v);
        assert!(summary.get("name").is_some(), "scene_summary 缺 name 字段: {summary}");
        assert!(
            summary.get("entityCount").is_some(),
            "scene_summary 缺 entityCount 字段: {summary}"
        );
    }

    // ---------- F3 wave.1:swarm state / seed-demo / execute ----------

    #[tokio::test]
    async fn swarm_state_default_node_and_empty_shards() {
        let resp = build_app()
            .oneshot(get("/api/forge/swarm/state"))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let v = json_body(resp).await;
        assert_eq!(v["nodes"].as_array().unwrap().len(), 1);
        assert_eq!(v["nodes"][0]["kind"], "logical");
        assert_eq!(v["shards"].as_array().unwrap().len(), 0);
    }

    #[tokio::test]
    async fn swarm_seed_demo_adds_nodes() {
        let resp = build_app()
            .oneshot(post_json("/api/forge/swarm/seed-demo", "{}"))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let v = json_body(resp).await;
        assert_eq!(v["seeded"], 2);
        assert_eq!(v["nodesAfter"], 3);
    }

    #[tokio::test]
    async fn swarm_execute_overlap_rejected_409() {
        let app = build_app();
        // 输入集自身重复 → 相交。
        let dup = app
            .clone()
            .oneshot(post_json(
                "/api/forge/swarm/execute",
                r#"{"shardType":"scene-partition","items":[1,1],"operation":{"kind":"add_component","type":"RigidBody","props":{"kind":"static","mass":0}}}"#,
            ))
            .await
            .unwrap();
        assert_eq!(dup.status(), StatusCode::CONFLICT);
        assert_eq!(
            json_body(dup).await["error"]["code"],
            "GOV_SWARM_SHARD_OVERLAP"
        );
    }

    #[tokio::test]
    async fn swarm_execute_invalid_domain_and_empty_items_400() {
        let app = build_app();
        // code-module 域 operation seam(F4 承接)→ 400 如实报未落地。
        let seam = app
            .clone()
            .oneshot(post_json(
                "/api/forge/swarm/execute",
                r#"{"shardType":"code-module","items":["a.rx"],"operation":{"kind":"rx_fmt"}}"#,
            ))
            .await
            .unwrap();
        assert_eq!(seam.status(), StatusCode::BAD_REQUEST);
        assert!(json_body(seam).await["error"]["message"]
            .as_str()
            .unwrap()
            .contains("未落地"));
        // 空 items。
        let empty = app
            .oneshot(post_json(
                "/api/forge/swarm/execute",
                r#"{"shardType":"scene-partition","items":[],"operation":{"kind":"add_component","type":"RigidBody"}}"#,
            ))
            .await
            .unwrap();
        assert_eq!(empty.status(), StatusCode::BAD_REQUEST);
    }

    /// 40 关卡块碰撞体全链:scene_new → 40 entity → /swarm/execute 4 片 add RigidBody →
    /// 聚合 40 全覆盖无失败 → component_get 抽验 → state 分片全 done。
    /// 二进制未构建时跳过(如实 SKIP)。
    #[tokio::test]
    async fn swarm_execute_40_blocks_add_rigidbody() {
        let bin = mcp::server_bin();
        if !bin.exists() {
            eprintln!("[SKIP] engine-scene-mcp 未构建: {},集成测试跳过", bin.display());
            return;
        }
        let _serial = SCENE_TEST_LOCK.lock().await;
        let app = build_app();
        let call = |app: &Router, body: &str| {
            let app = app.clone();
            let body = body.to_string();
            async move {
                let resp = app
                    .oneshot(post_json("/api/forge/mcp/call", &body))
                    .await
                    .unwrap();
                assert_eq!(resp.status(), StatusCode::OK);
                json_body(resp).await
            }
        };
        call(&app, r#"{"tool":"mcp__engine-scene__scene_new","arguments":{"name":"f3-swarm-ut"}}"#).await;
        // 造 40 个关卡块,收实体 id。
        let mut entity_ids: Vec<u64> = Vec::new();
        for i in 1..=40 {
            let v = call(
                &app,
                &format!(r#"{{"tool":"mcp__engine-scene__entity_create","arguments":{{"name":"block-{i}"}}}}"#),
            )
            .await;
            let created = extract_tool_json(&v);
            entity_ids.push(created["id"].as_u64().expect("entity_create 应返回 id"));
        }
        assert_eq!(entity_ids.len(), 40);

        // multitask 执行:4 片 add RigidBody。
        let items = entity_ids
            .iter()
            .map(u64::to_string)
            .collect::<Vec<_>>()
            .join(",");
        let resp = app
            .clone()
            .oneshot(post_json(
                "/api/forge/swarm/execute",
                &format!(r#"{{"shardType":"scene-partition","items":[{items}],"shardCount":4,"operation":{{"kind":"add_component","type":"RigidBody","props":{{"kind":"static","mass":0}}}}}}"#),
            ))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let v = json_body(resp).await;
        assert_eq!(v["aggregate"]["totalItems"], 40);
        assert_eq!(v["aggregate"]["succeeded"], 40, "分片报告: {v}");
        assert_eq!(v["aggregate"]["failed"], 0);
        assert_eq!(v["aggregate"]["disjoint"], true);
        assert_eq!(v["shards"].as_array().unwrap().len(), 4);
        for s in v["shards"].as_array().unwrap() {
            assert_eq!(s["status"], "done", "分片失败不遮蔽: {s}");
            assert_eq!(s["okCount"], 10);
        }

        // 抽验首/末实体 RigidBody 真实落上。
        for id in [entity_ids[0], entity_ids[39]] {
            let got = call(
                &app,
                &format!(r#"{{"tool":"mcp__engine-scene__component_get","arguments":{{"id":{id},"type":"RigidBody"}}}}"#),
            )
            .await;
            let comp = extract_tool_json(&got);
            assert_eq!(comp["props"]["kind"], "static", "实体 {id} RigidBody: {comp}");
        }

        // state 可见 4 个 done 分片。
        let st = app
            .oneshot(get("/api/forge/swarm/state"))
            .await
            .unwrap();
        let sv = json_body(st).await;
        let done = sv["shards"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|s| s["status"] == "done")
            .count();
        assert_eq!(done, 4);
    }

    // ---------- F3 wave.2:subagents 热加载 + skills/{name} + skills/config/write ----------

    /// subagents 目录读写测试互斥(F4 wave.3 修复):hot_reload 写真实 data/agents 临时文件,
    /// 与 list 断言「恰好 5 个」存在并发竞争窗口(cargo test 同进程并行)——两测试同锁串行。
    static SUBAGENTS_DIR_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    #[tokio::test]
    async fn subagents_list_five_builtin_profiles() {
        let _dir_guard = SUBAGENTS_DIR_LOCK.lock().unwrap();
        let resp = build_app()
            .oneshot(get("/api/forge/subagents"))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let v = json_body(resp).await;
        assert_eq!(v["errors"].as_array().unwrap().len(), 0, "profile 解析错误: {v}");
        let list = v["subagents"].as_array().unwrap();
        assert_eq!(list.len(), 5, "内建五 profile(04 §6): {list:?}");
        for name in ["asset-wrangler", "logic-programmer", "material-smith", "qa-tester", "scene-builder"] {
            let p = list.iter().find(|p| p["name"] == name).unwrap_or_else(|| panic!("缺 profile {name}"));
            assert!(p["description"].as_str().unwrap().len() > 4);
            assert!(p["tools"].as_array().unwrap().len() >= 2);
            assert!(p["maxSteps"].as_u64().unwrap() >= 16);
            assert!(p["prompt"].as_str().unwrap().contains("必须遵守"));
        }
        // 逐字白名单抽查(04 §6):logic-programmer 含 component.* 族;qa-tester 16 步。
        let lp = list.iter().find(|p| p["name"] == "logic-programmer").unwrap();
        assert!(lp["tools"].as_array().unwrap().iter().any(|t| t == "mcp__engine-scene__component.*"));
        let qa = list.iter().find(|p| p["name"] == "qa-tester").unwrap();
        assert_eq!(qa["maxSteps"], 16);
    }

    #[tokio::test]
    async fn subagents_hot_reload_without_restart() {
        let _dir_guard = SUBAGENTS_DIR_LOCK.lock().unwrap();
        let app = build_app();
        let dir = subagents::agents_dir();
        std::fs::create_dir_all(&dir).unwrap();
        let f = dir.join("zz-test-hot.md");
        let body = |steps: u32| {
            format!("---\nname: zz-test-hot\ndescription: 热加载测试\ntools: [\"read_file\"]\nmodel: default\nmaxSteps: {steps}\n---\n临时 profile,测试后删除。\n")
        };
        std::fs::write(&f, body(7)).unwrap();
        let r1 = app.clone().oneshot(get("/api/forge/subagents")).await.unwrap();
        let v1 = json_body(r1).await;
        let p1 = v1["subagents"].as_array().unwrap().iter().find(|p| p["name"] == "zz-test-hot").expect("新 profile 应即现");
        assert_eq!(p1["maxSteps"], 7);
        // 改文件(不重启)后再查应反映新值。
        std::fs::write(&f, body(42)).unwrap();
        let r2 = app.oneshot(get("/api/forge/subagents")).await.unwrap();
        let v2 = json_body(r2).await;
        let p2 = v2["subagents"].as_array().unwrap().iter().find(|p| p["name"] == "zz-test-hot").unwrap();
        assert_eq!(p2["maxSteps"], 42, "热加载失效?");
        std::fs::remove_file(&f).ok();
    }

    #[tokio::test]
    async fn skills_read_full_text_and_guards() {
        let app = build_app();
        let ok = app
            .clone()
            .oneshot(get("/api/forge/skills/asset-cleanup"))
            .await
            .unwrap();
        assert_eq!(ok.status(), StatusCode::OK);
        let v = json_body(ok).await;
        let content = v["content"].as_str().unwrap();
        // 与磁盘逐字节一致。
        let disk = std::fs::read_to_string(
            workspace_root().join("skills").join("asset-cleanup").join("SKILL.md"),
        )
        .unwrap();
        assert_eq!(content, disk);
        assert!(content.starts_with("---"));

        let missing = app
            .clone()
            .oneshot(get("/api/forge/skills/no-such-skill"))
            .await
            .unwrap();
        assert_eq!(missing.status(), StatusCode::NOT_FOUND);
        let bad = app
            .oneshot(get("/api/forge/skills/Bad_Name"))
            .await
            .unwrap();
        assert_eq!(bad.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn skills_config_write_disable_then_restore() {
        let cfg_path = skills_config_path();
        let backup = std::fs::read_to_string(&cfg_path).ok();
        let app = build_app();
        // 禁用 asset-cleanup → list 反映 enabled=false。
        let w = app
            .clone()
            .oneshot(post_json(
                "/api/forge/skills/config/write",
                r#"{"disabled":["asset-cleanup"]}"#,
            ))
            .await
            .unwrap();
        assert_eq!(w.status(), StatusCode::OK);
        let listed = app
            .clone()
            .oneshot(get("/api/forge/skills/list"))
            .await
            .unwrap();
        let v = json_body(listed).await;
        let sc = v["skills"].as_array().unwrap().iter().find(|s| s["name"] == "asset-cleanup").unwrap();
        assert_eq!(sc["enabled"], false, "禁用未生效: {v}");
        // 还原(恢复原文件内容或删除)。
        let restore = app
            .clone()
            .oneshot(post_json("/api/forge/skills/config/write", r#"{"disabled":[]}"#))
            .await
            .unwrap();
        assert_eq!(restore.status(), StatusCode::OK);
        let listed2 = app
            .oneshot(get("/api/forge/skills/list"))
            .await
            .unwrap();
        let v2 = json_body(listed2).await;
        let sc2 = v2["skills"].as_array().unwrap().iter().find(|s| s["name"] == "asset-cleanup").unwrap();
        assert_eq!(sc2["enabled"], true);
        // 非法名 400。
        let bad = build_app()
            .oneshot(post_json(
                "/api/forge/skills/config/write",
                r#"{"disabled":["Bad_Name"]}"#,
            ))
            .await
            .unwrap();
        assert_eq!(bad.status(), StatusCode::BAD_REQUEST);
        // 物理还原(避免测试改写开发态配置)。
        match backup {
            Some(text) => std::fs::write(&cfg_path, text).unwrap(),
            None => {
                std::fs::remove_file(&cfg_path).ok();
            }
        }
    }

    // ---------- F5 wave.3:gen 配置 REST 面 ----------

    /// FORGE_GEN_DATA_DIR / FORGE_GEN_API_KEY 进程级,gen REST 测试串行。
    static GEN_REST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    fn gen_temp_dir(tag: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("agentd-gen-{tag}-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[tokio::test]
    async fn gen_backends_list_unconfigured_default() {
        let _g = GEN_REST_LOCK.lock().unwrap();
        let data = gen_temp_dir("list");
        std::env::set_var("FORGE_GEN_DATA_DIR", &data);
        std::env::remove_var("FORGE_GEN_API_KEY");
        let resp = build_app()
            .oneshot(get("/api/forge/gen/backends"))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let v = json_body(resp).await;
        let bs = v["backends"].as_array().unwrap();
        assert_eq!(bs.len(), 2, "注册表两条目: {bs:?}");
        for id in ["local-mock", "remote-openai-compatible"] {
            let b = bs.iter().find(|b| b["id"] == id).unwrap_or_else(|| panic!("缺 {id}"));
            assert_eq!(b["configured"], false);
            assert_eq!(b["endpointSet"], false);
            assert!(b["capabilities"].is_object());
            // 响应面无任何密钥/endpoint 值字段。
            assert!(b.get("apiKey").is_none());
            assert!(b.get("endpoint").is_none());
        }
        std::env::remove_var("FORGE_GEN_DATA_DIR");
        std::fs::remove_dir_all(&data).ok();
    }

    #[tokio::test]
    async fn gen_configure_writes_config_and_keystore_redline() {
        let _g = GEN_REST_LOCK.lock().unwrap();
        let data = gen_temp_dir("cfg");
        std::env::set_var("FORGE_GEN_DATA_DIR", &data);
        std::env::remove_var("FORGE_GEN_API_KEY");
        let app = build_app();

        // 1) local-mock enabled → ok+configured;gen-backends.json 写盘可回读。
        let r = app
            .clone()
            .oneshot(post_json(
                "/api/forge/gen/backends/configure",
                r#"{"id":"local-mock","kind":"local","enabled":true}"#,
            ))
            .await
            .unwrap();
        assert_eq!(r.status(), StatusCode::OK);
        let v = json_body(r).await;
        assert_eq!(v["ok"], true);
        assert_eq!(v["configured"], true);
        let cfg_text = std::fs::read_to_string(data.join("gen-backends.json")).unwrap();
        let cfg: Value = serde_json::from_str(&cfg_text).unwrap();
        assert_eq!(cfg["backends"][0]["id"], "local-mock");
        assert_eq!(cfg["backends"][0]["enabled"], true);

        // GET 回读 configured=true。
        let listed = app
            .clone()
            .oneshot(get("/api/forge/gen/backends"))
            .await
            .unwrap();
        let lv = json_body(listed).await;
        let lm = lv["backends"].as_array().unwrap().iter().find(|b| b["id"] == "local-mock").unwrap();
        assert_eq!(lm["configured"], true);

        // 2) remote + apiKey:configured=true(endpoint+key 齐);密钥只进 keystore.json。
        let secret = "sk-test-REDLINE-9f8e7d";
        let r2 = app
            .clone()
            .oneshot(post_json(
                "/api/forge/gen/backends/configure",
                &format!(r#"{{"id":"remote-openai-compatible","kind":"remote","enabled":true,"endpoint":"https://api.example.com","apiKey":"{secret}"}}"#),
            ))
            .await
            .unwrap();
        assert_eq!(r2.status(), StatusCode::OK);
        let v2 = json_body(r2).await;
        assert_eq!(v2["ok"], true);
        assert_eq!(v2["configured"], true);
        assert!(!v2.to_string().contains(secret), "响应回显密钥(R-5): {v2}");

        // keystore.json 可取回 key;gen-backends.json 不含 key。
        // RD-F5-001:Windows 为 DPAPI 加密形态(文件无明文,经 Keystore 解密读回);非 Windows 明文 fallback。
        let ks_path = data.join("keystore.json");
        let ks_text = std::fs::read_to_string(&ks_path).unwrap();
        #[cfg(windows)]
        {
            assert!(ks_text.contains("\"dpapi\""), "Windows 应为 DPAPI 加密形态: {ks_text}");
            assert!(!ks_text.contains(secret), "DPAPI 密文文件含明文密钥(RD-F5-001): {ks_text}");
        }
        #[cfg(not(windows))]
        {
            assert!(ks_text.contains(secret), "keystore.json 应含密钥(非 Windows 明文 fallback)");
        }
        let ks = gend::keystore::Keystore::load_from(&ks_path);
        assert_eq!(ks.key_for("remote-openai-compatible").as_deref(), Some(secret));
        let cfg_text2 = std::fs::read_to_string(data.join("gen-backends.json")).unwrap();
        assert!(!cfg_text2.contains(secret), "gen-backends.json 泄漏密钥(R-5): {cfg_text2}");
        let cfg2: Value = serde_json::from_str(&cfg_text2).unwrap();
        // 读-改-写保留 local-mock 条目。
        assert!(cfg2["backends"].as_array().unwrap().iter().any(|b| b["id"] == "local-mock"));
        let remote = cfg2["backends"].as_array().unwrap().iter().find(|b| b["id"] == "remote-openai-compatible").unwrap();
        assert_eq!(remote["endpoint"], "https://api.example.com");
        assert!(remote.get("apiKey").is_none());

        // 3) 非法 id → 400 GEN_UNKNOWN_BACKEND;kind 不符 → 400 GEN_BAD_PARAMS。
        let bad = app
            .clone()
            .oneshot(post_json(
                "/api/forge/gen/backends/configure",
                r#"{"id":"no-such-backend","kind":"local","enabled":true}"#,
            ))
            .await
            .unwrap();
        assert_eq!(bad.status(), StatusCode::BAD_REQUEST);
        assert_eq!(json_body(bad).await["error"]["code"], "GEN_UNKNOWN_BACKEND");
        let bad_kind = app
            .oneshot(post_json(
                "/api/forge/gen/backends/configure",
                r#"{"id":"local-mock","kind":"remote","enabled":true}"#,
            ))
            .await
            .unwrap();
        assert_eq!(bad_kind.status(), StatusCode::BAD_REQUEST);
        assert_eq!(json_body(bad_kind).await["error"]["code"], "GEN_BAD_PARAMS");

        std::env::remove_var("FORGE_GEN_DATA_DIR");
        std::fs::remove_dir_all(&data).ok();
    }

    // ---------- F7 wave.5:workspace/tree + llm/key ----------

    /// FORGE_AGENTD_WORKSPACE_ROOT 进程级,workspace 树测试串行(F4 wave.3 教训:共享目录读写互斥)。
    static WS_ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    /// 独立 workspace 根(造目录树;返回 (根, guard 落盘即清))。
    fn ws_temp_root(tag: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "agentd-w5-ws-{tag}-{}-{}",
            std::process::id(),
            events::new_id("t")
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[tokio::test]
    async fn f7w5_workspace_tree_single_level_sorted_and_hidden() {
        let _g = WS_ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let root = ws_temp_root("tree");
        // 造树:两目录 + 三文件(含 .hidden)+ 嵌套(单层面不应下钻)。
        std::fs::create_dir_all(root.join("beta_dir")).unwrap();
        std::fs::create_dir_all(root.join("Alpha dir")).unwrap();
        std::fs::write(root.join("zeta.txt"), "z").unwrap();
        std::fs::write(root.join("alpha.txt"), "aa").unwrap();
        std::fs::write(root.join(".hidden"), "h").unwrap();
        std::fs::write(root.join("beta_dir").join("inner.txt"), "i").unwrap();
        std::env::set_var("FORGE_AGENTD_WORKSPACE_ROOT", &root);
        let app = build_app();
        // 根单层。
        let r = app
            .clone()
            .oneshot(get("/api/forge/workspace/tree"))
            .await
            .unwrap();
        assert_eq!(r.status(), StatusCode::OK);
        let v = json_body(r).await;
        assert_eq!(v["path"], "");
        assert_eq!(v["truncated"], false);
        let names: Vec<&str> = v["entries"]
            .as_array()
            .unwrap()
            .iter()
            .map(|e| e["name"].as_str().unwrap())
            .collect();
        // 目录优先 + 名称小写排序:.hidden(h) < Alpha dir < beta dir?否——hidden 不参与排前,
        // 统一按 (dir 优先, name 小写):Alpha dir / beta_dir 目录在前;.hidden/alpha.txt/zeta.txt 文件在后。
        assert_eq!(names, vec!["Alpha dir", "beta_dir", ".hidden", "alpha.txt", "zeta.txt"], "排序: {names:?}");
        let entries = v["entries"].as_array().unwrap();
        assert_eq!(entries[0]["kind"], "dir");
        assert_eq!(entries[0]["size"], 0);
        assert_eq!(entries[0]["relPath"], "Alpha dir");
        assert_eq!(entries[0]["hidden"], false);
        assert!(entries[0]["modifiedAt"].as_str().unwrap().contains('T'));
        assert_eq!(entries[2]["hidden"], true, ". 前缀隐藏标记");
        assert_eq!(entries[3]["size"], 2, "alpha.txt 两字节");
        // 单层:未见嵌套 inner.txt。
        assert!(!names.contains(&"inner.txt"));
        // 子目录层(正斜杠 relPath 下钻)。
        let r = app
            .clone()
            .oneshot(get("/api/forge/workspace/tree?path=beta_dir"))
            .await
            .unwrap();
        let v = json_body(r).await;
        assert_eq!(v["path"], "beta_dir");
        let names: Vec<&str> = v["entries"]
            .as_array()
            .unwrap()
            .iter()
            .map(|e| e["name"].as_str().unwrap())
            .collect();
        assert_eq!(names, vec!["inner.txt"]);
        assert_eq!(v["entries"][0]["relPath"], "beta_dir/inner.txt");
        std::env::remove_var("FORGE_AGENTD_WORKSPACE_ROOT");
        std::fs::remove_dir_all(&root).ok();
    }

    #[tokio::test]
    async fn f7w5_workspace_tree_confined_and_not_found() {
        let _g = WS_ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let root = ws_temp_root("confined");
        std::fs::create_dir_all(root.join("sub")).unwrap();
        std::fs::write(root.join("f.txt"), "x").unwrap();
        std::env::set_var("FORGE_AGENTD_WORKSPACE_ROOT", &root);
        let app = build_app();
        // .. 逃逸 → 400 PATH_OUTSIDE_ROOT。
        let r = app
            .clone()
            .oneshot(get("/api/forge/workspace/tree?path=.."))
            .await
            .unwrap();
        assert_eq!(r.status(), StatusCode::BAD_REQUEST);
        assert_eq!(json_body(r).await["error"]["code"], "PATH_OUTSIDE_ROOT");
        // 绝对路径出根 → 400(root.join(带盘符/根的路径)= 整体替换,canon 后出根)。
        // 用固定盘符/根路径(ASCII 安全;http::Uri 不接受非 ASCII query)。
        #[cfg(windows)]
        let abs = "C:/";
        #[cfg(not(windows))]
        let abs = "/";
        let r = app
            .clone()
            .oneshot(get(&format!("/api/forge/workspace/tree?path={abs}")))
            .await
            .unwrap();
        assert_eq!(r.status(), StatusCode::BAD_REQUEST, "绝对出根: {abs}");
        assert_eq!(json_body(r).await["error"]["code"], "PATH_OUTSIDE_ROOT");
        // 不存在 → 404 PATH_NOT_FOUND;文件当目录 → 404。
        let r = app
            .clone()
            .oneshot(get("/api/forge/workspace/tree?path=no_such_dir"))
            .await
            .unwrap();
        assert_eq!(r.status(), StatusCode::NOT_FOUND);
        assert_eq!(json_body(r).await["error"]["code"], "PATH_NOT_FOUND");
        let r = app
            .oneshot(get("/api/forge/workspace/tree?path=f.txt"))
            .await
            .unwrap();
        assert_eq!(r.status(), StatusCode::NOT_FOUND);
        std::env::remove_var("FORGE_AGENTD_WORKSPACE_ROOT");
        std::fs::remove_dir_all(&root).ok();
    }

    #[tokio::test]
    async fn f7w5_workspace_tree_truncation_over_500() {
        let _g = WS_ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let root = ws_temp_root("trunc");
        for i in 0..505 {
            std::fs::write(root.join(format!("f{i:03}.txt")), "x").unwrap();
        }
        std::env::set_var("FORGE_AGENTD_WORKSPACE_ROOT", &root);
        let app = build_app();
        let r = app
            .oneshot(get("/api/forge/workspace/tree"))
            .await
            .unwrap();
        assert_eq!(r.status(), StatusCode::OK);
        let v = json_body(r).await;
        assert_eq!(v["total"], 505);
        assert_eq!(v["truncated"], true, "超 500 如实截断标记");
        assert_eq!(v["entries"].as_array().unwrap().len(), 500);
        // 截断后仍按排序前缀(f000..f499)。
        assert_eq!(v["entries"][0]["name"], "f000.txt");
        assert_eq!(v["entries"][499]["name"], "f499.txt");
        std::env::remove_var("FORGE_AGENTD_WORKSPACE_ROOT");
        std::fs::remove_dir_all(&root).ok();
    }

    #[tokio::test]
    async fn f7w5_llm_key_write_flips_availability_redline() {
        // 三锁同源纪律(gen REST + llm env + F7 data dir 互不串扰)。
        let _g1 = GEN_REST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let _g2 = llm::TEST_ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        std::env::remove_var("FORGE_LLM_API_KEY");
        std::env::remove_var("FORGE_GEN_API_KEY");
        let data = gen_temp_dir("llmkey");
        std::env::set_var("FORGE_GEN_DATA_DIR", &data);
        let app = build_app();
        // 前置:无 key → needs-key。
        let r = app
            .clone()
            .oneshot(get("/api/forge/design-snapshot"))
            .await
            .unwrap();
        assert_eq!(
            json_body(r).await["models"]["models"][0]["availability"],
            "needs-key"
        );
        // 空 key → 400 EMPTY_KEY。
        let r = app
            .clone()
            .oneshot(post_json("/api/forge/llm/key", r#"{"apiKey":"  "}"#))
            .await
            .unwrap();
        assert_eq!(r.status(), StatusCode::BAD_REQUEST);
        assert_eq!(json_body(r).await["error"]["code"], "EMPTY_KEY");
        // 写 key → {ok, configured};响应面不回显(R-5);availability 翻转。
        let secret = "sk-test-REDLINE-w5-llmkey";
        let r = app
            .clone()
            .oneshot(post_json(
                "/api/forge/llm/key",
                &format!(r#"{{"apiKey":"{secret}"}}"#),
            ))
            .await
            .unwrap();
        assert_eq!(r.status(), StatusCode::OK);
        let v = json_body(r).await;
        assert_eq!(v, json!({ "ok": true, "configured": true }), "响应面仅 ok+configured: {v}");
        assert!(!v.to_string().contains(secret), "响应回显密钥(R-5): {v}");
        let r = app
            .clone()
            .oneshot(get("/api/forge/design-snapshot"))
            .await
            .unwrap();
        let v = json_body(r).await;
        assert_eq!(v["models"]["models"][0]["availability"], "available");
        assert!(!v.to_string().contains(secret), "snapshot 泄漏密钥(R-5)");
        // keystore 回读 = 写入值(经 Keystore 面;Windows DPAPI 密文文件无明文)。
        let ks = gend::keystore::Keystore::load_from(&data.join("keystore.json"));
        assert_eq!(ks.key_for("deepseek").as_deref(), Some(secret));
        // 覆盖写:新值替换。
        let secret2 = "sk-test-REDLINE-w5-overwrite";
        let r = app
            .oneshot(post_json(
                "/api/forge/llm/key",
                &format!(r#"{{"apiKey":"{secret2}"}}"#),
            ))
            .await
            .unwrap();
        assert_eq!(r.status(), StatusCode::OK);
        let ks2 = gend::keystore::Keystore::load_from(&data.join("keystore.json"));
        assert_eq!(ks2.key_for("deepseek").as_deref(), Some(secret2), "覆盖写未生效");
        std::env::remove_var("FORGE_GEN_DATA_DIR");
        std::fs::remove_dir_all(&data).ok();
    }

    /// 从 MCP tools/call result 提取工具返回 JSON
    /// (兼容 content[0].text 内嵌 JSON / structuredContent / result 本体三种形态)
    fn extract_tool_json(result: &Value) -> Value {
        if let Some(text) = result.pointer("/content/0/text").and_then(Value::as_str) {
            return serde_json::from_str(text)
                .unwrap_or_else(|_| panic!("工具 text 内容非 JSON: {text}"));
        }
        if let Some(sc) = result.get("structuredContent") {
            return sc.clone();
        }
        result.clone()
    }

    /// 场景态测试串行锁:scene_new 会切换全局长连接的当前场景,
    /// 多测试线程交错会互踩(F3 wave.1 40 实体测试引入后 hazard 现实化)。
    static SCENE_TEST_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

    /// 长连接持久性回归:同一会话内 create 后 list 必须可见(F1 实测缺陷修复门)。
    #[tokio::test]
    async fn mcp_call_entity_persists_across_calls() {
        let bin = mcp::server_bin();
        if !bin.exists() {
            eprintln!("[SKIP] engine-scene-mcp 未构建: {},集成测试跳过", bin.display());
            return;
        }
        let _serial = SCENE_TEST_LOCK.lock().await;
        let app = build_app();
        // 独立场景,避免与其他测试互串
        let _ = app
            .clone()
            .oneshot(
                post_json(
                    "/api/forge/mcp/call",
                    r#"{"tool":"mcp__engine-scene__scene_new","arguments":{"name":"persist-check"}}"#,
                ),
            )
            .await
            .unwrap();
        let created = app
            .clone()
            .oneshot(
                post_json(
                    "/api/forge/mcp/call",
                    r#"{"tool":"mcp__engine-scene__entity_create","arguments":{"name":"cube-1"}}"#,
                ),
            )
            .await
            .unwrap();
        assert_eq!(created.status(), StatusCode::OK);
        let listed = app
            .oneshot(
                post_json(
                    "/api/forge/mcp/call",
                    r#"{"tool":"mcp__engine-scene__entity_list"}"#,
                ),
            )
            .await
            .unwrap();
        assert_eq!(listed.status(), StatusCode::OK);
        let v = json_body(listed).await;
        let list = extract_tool_json(&v);
        let names = list.to_string();
        assert!(
            names.contains("cube-1"),
            "跨调用实体未持久(长连接失效?): {names}"
        );
    }

    /// 真实 bind 127.0.0.1:0 的集成测试:原始 TCP 发 HTTP/1.1(避免新增 HTTP 客户端依赖)
    #[tokio::test]
    async fn real_bind_health() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            axum::serve(listener, build_app()).await.unwrap();
        });

        let mut stream = tokio::net::TcpStream::connect(addr).await.unwrap();
        stream
            .write_all(
                format!("GET /health HTTP/1.1\r\nHost: {addr}\r\nConnection: close\r\n\r\n")
                    .as_bytes(),
            )
            .await
            .unwrap();
        let mut raw = Vec::new();
        stream.read_to_end(&mut raw).await.unwrap();
        let text = String::from_utf8(raw).expect("响应应为 UTF-8");
        let (head, body) = text.split_once("\r\n\r\n").expect("HTTP 响应应含头体分隔");
        assert!(head.starts_with("HTTP/1.1 200"), "状态行异常: {head}");
        let v: Value = serde_json::from_str(body).expect("body 应为 JSON");
        assert_eq!(v["service"], "forge-agentd");
        assert_eq!(v["status"], "ok");
        server.abort();
    }
}
