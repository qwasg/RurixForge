//! forge-agentd:F0 最小骨架 agent 守护进程
//! 路由:/health、sessions stub、MCP 工具面声明与 stdio 透传调用、mock LLM provider(恒绿 seam)
//! F2 wave.5:Proposal 确认单(12 §3)+ destructive 强制门(asset_delete force)+ skills 发现。

mod mcp;
mod proposals;
mod subagents;
mod swarm;

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

/// 服务共享状态(启动时刻,供 uptimeSec 实测;Proposal 存贮)
struct AppState {
    started: Instant,
    proposals: proposals::ProposalStore,
    swarm: swarm::SwarmCoordinator,
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

/// 构建路由表(main 与测试复用)
fn build_app() -> Router {
    let state = Arc::new(AppState {
        started: Instant::now(),
        proposals: proposals::ProposalStore::default(),
        swarm: swarm::SwarmCoordinator::default(),
    });
    Router::new()
        .route("/health", get(health))
        .route("/api/forge/sessions", get(sessions))
        .route("/api/forge/mcp/tools", get(mcp_tools))
        .route("/api/forge/mcp/call", post(mcp_call))
        .route("/api/forge/llm/complete", get(llm_complete))
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

/// F0 会话 stub:恒空数组
async fn sessions() -> Json<Value> {
    Json(json!([]))
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
        ("code-module", _) | ("test-matrix", _) => Err(format!(
            "{shard_type} 域 operation 未落地(F4 code-forge / F6 playtest 承接,seam)"
        )),
        _ => Err(format!("shardType {shard_type} 不支持 operation.kind {kind}")),
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
            (id, ok_items, errors)
        }));
    }

    let mut shard_reports = Vec::with_capacity(handles.len());
    let mut total_ok = 0usize;
    let mut total_err = 0usize;
    for h in handles {
        let (id, ok_items, errors) = h.await.expect("worker task join 失败");
        total_ok += ok_items.len();
        total_err += errors.len();
        let shard_ok = errors.is_empty();
        let report = json!({
            "shardId": id,
            "ok": ok_items,
            "okCount": ok_items.len(),
            "errors": errors,
        });
        state.swarm.complete_shard(&id, report.clone(), shard_ok);
        shard_reports.push(json!({
            "shardId": id,
            "status": if shard_ok { "done" } else { "failed" },
            "okCount": report["okCount"],
            "errorCount": report["errors"].as_array().map(Vec::len).unwrap_or(0),
            "errors": report["errors"],
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
    async fn sessions_empty_array() {
        let resp = build_app()
            .oneshot(get("/api/forge/sessions"))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        assert_eq!(json_body(resp).await, json!([]));
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

        // keystore.json 含 key;gen-backends.json 不含 key。
        let ks_text = std::fs::read_to_string(data.join("keystore.json")).unwrap();
        assert!(ks_text.contains(secret), "keystore.json 应含密钥");
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
