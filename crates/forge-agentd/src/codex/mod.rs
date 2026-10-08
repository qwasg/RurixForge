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
pub mod imagegen;
pub mod map;
pub mod managed;
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
    let params = json!({
        "threadId": thread_id,
        "objective": goal.objective,
        "status": codex_goal_status(&goal.status),
        // Forge 用 0 表示不限；Codex 用 null 清除预算。省略会错误地保留旧预算。
        "tokenBudget": if goal.token_budget == 0 { Value::Null } else { json!(goal.token_budget) },
    });
    client.request("thread/goal/set", params).await
}

pub async fn goal_read(state: &Arc<crate::AppState>, thread_id: &str) -> Result<Value, CodexError> {
    let client = state.codex.ready_client().await?;
    let v = client
        .request("thread/goal/get", json!({ "threadId": thread_id }))
        .await?;
    Ok(normalize_goal_value(v.get("goal").cloned().unwrap_or(v)))
}

/// 只改已有原生 Goal 的状态。pause/resume 必须透传；仅改本地镜像会让 app-server
/// 继续在后台自动起 turn。
pub async fn goal_status_set(
    state: &Arc<crate::AppState>,
    thread_id: &str,
    status: &str,
) -> Result<Value, CodexError> {
    let client = state.codex.ready_client().await?;
    client
        .request(
            "thread/goal/set",
            json!({ "threadId": thread_id, "status": codex_goal_status(status) }),
        )
        .await
}

fn codex_goal_status(status: &str) -> &str {
    match status {
        // app-server 的终态枚举是 `complete`，Forge 的统一 UI/本地引擎使用 `completed`。
        crate::goals::STATUS_COMPLETED => "complete",
        other => other,
    }
}

pub(crate) fn normalize_goal_value(mut goal: Value) -> Value {
    if goal.get("tokenBudget") == Some(&Value::Null) {
        // Forge Goal UI/持久层用 0 表示不限；不让 nullable 上游字段穿透成不兼容形态。
        goal["tokenBudget"] = json!(0);
    }
    match goal.get("status").and_then(Value::as_str) {
        Some("complete") => goal["status"] = json!(crate::goals::STATUS_COMPLETED),
        Some(status @ ("usageLimited" | "budgetLimited")) => {
            // Forge UI 只有 active/paused/blocked/completed 四态；额度终止是可恢复暂停，
            // 同时保留 Codex 原始状态，供 GoalBar 给出准确原因。
            let codex_status = status.to_string();
            goal["status"] = json!(crate::goals::STATUS_PAUSED);
            goal["codexStatus"] = json!(codex_status);
            goal["budgetExhausted"] = json!(true);
        }
        _ => {}
    }
    goal
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn goal_status_vocabulary_is_normalized() {
        assert_eq!(codex_goal_status("completed"), "complete");
        assert_eq!(codex_goal_status("paused"), "paused");
        assert_eq!(
            normalize_goal_value(json!({ "status": "complete" }))["status"],
            "completed"
        );
        let limited = normalize_goal_value(json!({ "status": "budgetLimited" }));
        assert_eq!(limited["status"], "paused");
        assert_eq!(limited["codexStatus"], "budgetLimited");
        assert_eq!(limited["budgetExhausted"], true);
        assert_eq!(
            normalize_goal_value(json!({ "status": "active", "tokenBudget": null }))["tokenBudget"],
            0
        );
    }
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
