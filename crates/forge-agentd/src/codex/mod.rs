//! Codex 引擎接入。
//!
//! 「本地 / Codex」是同一个会话上的两种执行引擎:本地引擎是本仓自研的工具循环
//! ([agent.rs](crates/forge-agentd/src/agent.rs) `execute_turn` → [llm.rs](crates/forge-agentd/src/llm.rs)
//! `run_tool_loop`),Codex 引擎把同一次 `ask:execute` 交给 `codex app-server` 子进程执行。
//!
//! 两条腿共用会话、事件流、审批面、计划文档与 todo 面板 —— 也就是说前端不需要
//! 「另一套 Codex 界面」,Codex 的 item/审批/计划/目标/用量全部翻译成本仓既有的
//! 事件词汇([map.rs])后走同一条 SSE 上屏。
//!
//! 模块分工:
//! - [bin]:codex / open-computer-use 可执行文件决议与托管安装路径;
//! - [config]:`data/codex-config.json`;
//! - [rpc]:app-server JSON-RPC over stdio 客户端(含单测用的全内存传输);
//! - [service]:进程级门面(客户端生命周期、账户/额度/模型缓存、安装任务);
//! - [mcp_config]:把本仓 7 个 MCP 服务注入 Codex 线程;
//! - [tools]:反向暴露给 Codex 的 Forge 原生工具(dynamicTools);
//! - [map]:Codex 通知 → Forge 事件;
//! - [approvals]:Codex 服务端请求 → Forge 审批面;
//! - [turn]:一整轮的编排(`thread/start` → `turn/start` → 消费通知 → 收尾)。

pub mod approvals;
pub mod bin;
pub mod config;
pub mod map;
pub mod mcp_config;
pub mod rest;
pub mod rpc;
pub mod service;
pub mod tools;
pub mod turn;

use serde_json::{json, Value};
use std::sync::Arc;

use rpc::CodexError;

/// `thread/goal/set` 透传(Codex 自己的 goal 循环据此续跑)。
pub async fn goal_set(
    state: &Arc<crate::AppState>,
    thread_id: &str,
    goal: &crate::goals::Goal,
) -> Result<Value, CodexError> {
    let client = state.codex.ready_client().await?;
    let mut params = json!({
        "threadId": thread_id,
        "goal": { "objective": goal.objective },
    });
    if goal.token_budget > 0 {
        params["goal"]["tokenBudget"] = json!(goal.token_budget);
    }
    client.request("thread/goal/set", params).await
}

pub async fn goal_read(
    state: &Arc<crate::AppState>,
    thread_id: &str,
) -> Result<Value, CodexError> {
    let client = state.codex.ready_client().await?;
    let v = client
        .request("thread/goal/get", json!({ "threadId": thread_id }))
        .await?;
    Ok(v.get("goal").cloned().unwrap_or(v))
}

pub async fn goal_clear(
    state: &Arc<crate::AppState>,
    thread_id: &str,
) -> Result<Value, CodexError> {
    let client = state.codex.ready_client().await?;
    client
        .request("thread/goal/clear", json!({ "threadId": thread_id }))
        .await
}
