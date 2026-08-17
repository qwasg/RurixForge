//! forge-agentd:F0 最小骨架 agent 守护进程
//! 路由:/health、sessions stub、MCP 工具面声明与 stdio 透传调用、mock LLM provider(恒绿 seam)
//! F2 wave.5:Proposal 确认单(12 §3)+ destructive 强制门(asset_delete force)+ skills 发现。

mod mcp;
mod proposals;

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

/// GET /api/forge/skills/list:扫 workspace skills/<name>/SKILL.md frontmatter(06 §2 最小落地)。
async fn skills_list() -> Json<Value> {
    let manifest_dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let root = manifest_dir
        .ancestors()
        .nth(2)
        .expect("CARGO_MANIFEST_DIR 应有上两级(workspace 根)");
    let dir = root.join("skills");
    let mut skills = Vec::new();
    if let Ok(rd) = std::fs::read_dir(&dir) {
        for ent in rd.flatten() {
            let f = ent.path().join("SKILL.md");
            if !f.is_file() {
                continue;
            }
            if let Ok(text) = std::fs::read_to_string(&f) {
                if let Some((name, description)) = parse_skill_frontmatter(&text) {
                    skills.push(json!({ "name": name, "description": description }));
                }
            }
        }
    }
    skills.sort_by(|a, b| a["name"].as_str().cmp(&b["name"].as_str()));
    Json(json!({ "skills": skills }))
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
        assert_eq!(tools.len(), 53);
        assert!(tools.iter().any(|t| t == "mcp__engine-scene__scene_summary"));
        assert!(tools.iter().any(|t| t == "mcp__engine-scene__entity_batch_apply"));
        assert!(tools.iter().any(|t| t == "mcp__engine-scene__viewport_frame"));
        assert!(tools.iter().any(|t| t == "mcp__asset-pipeline__asset_import"));
        assert!(tools.iter().any(|t| t == "mcp__asset-pipeline__asset_list"));
        assert!(tools.iter().any(|t| t == "mcp__asset-pipeline__asset_thumbnail"));
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

    /// 长连接持久性回归:同一会话内 create 后 list 必须可见(F1 实测缺陷修复门)。
    #[tokio::test]
    async fn mcp_call_entity_persists_across_calls() {
        let bin = mcp::server_bin();
        if !bin.exists() {
            eprintln!("[SKIP] engine-scene-mcp 未构建: {},集成测试跳过", bin.display());
            return;
        }
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
