//! forge-agentd:F0 最小骨架 agent 守护进程
//! 路由:/health、sessions stub、MCP 工具面声明与 stdio 透传调用、mock LLM provider(恒绿 seam)

mod mcp;

use axum::{
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use serde::Deserialize;
use serde_json::{json, Value};
use std::{sync::Arc, time::Instant};

/// 服务共享状态(启动时刻,供 uptimeSec 实测)
struct AppState {
    started: Instant,
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
    });
    Router::new()
        .route("/health", get(health))
        .route("/api/forge/sessions", get(sessions))
        .route("/api/forge/mcp/tools", get(mcp_tools))
        .route("/api/forge/mcp/call", post(mcp_call))
        .route("/api/forge/llm/complete", get(llm_complete))
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

/// MCP 工具调用透传:未知工具 404;子进程调用失败 502;成功返回 MCP result 本体
async fn mcp_call(Json(req): Json<McpCallRequest>) -> Response {
    if !mcp::KNOWN_TOOLS.contains(&req.tool.as_str()) {
        return (
            StatusCode::NOT_FOUND,
            Json(json!({ "error": { "code": "TOOL_NOT_FOUND" } })),
        )
            .into_response();
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
        assert_eq!(tools.len(), 32);
        assert!(tools.iter().any(|t| t == "mcp__engine-scene__scene_summary"));
        assert!(tools.iter().any(|t| t == "mcp__engine-scene__entity_batch_apply"));
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
