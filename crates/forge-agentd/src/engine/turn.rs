//! Turn / TurnItem：一轮用户输入的进度条目，经 [`record_item`] 唯一映射到 EventBus。
//! 语义对齐参考仓 agent-core engine/turn.rs（拷入改接，不引入其 crate）。

use serde_json::{json, Value};

use crate::events::{EventBus, EventDraft};

/// 一轮内的单一进度项。
#[derive(Debug, Clone)]
pub enum TurnItem {
    Reasoning {
        text: String,
    },
    AssistantText {
        text: String,
        provider: String,
        degraded: bool,
    },
    ToolCall {
        call_id: String,
        name: String,
        arguments: String,
        parent_tool_call_id: Option<String>,
    },
    ToolResult {
        call_id: String,
        name: String,
        output: String,
        is_error: bool,
        denied: bool,
        duration_ms: u64,
        parent_tool_call_id: Option<String>,
    },
    SteeredUser {
        text: String,
    },
}

pub struct Turn {
    pub session_id: String,
    pub run_id: String,
    pub items: Vec<TurnItem>,
}

impl Turn {
    pub fn new(session_id: &str, run_id: &str) -> Self {
        Turn {
            session_id: session_id.to_string(),
            run_id: run_id.to_string(),
            items: Vec::new(),
        }
    }
}

const OUTPUT_PREVIEW_MAX: usize = 2000;

fn preview(s: &str) -> String {
    let n = s.chars().count();
    if n <= OUTPUT_PREVIEW_MAX {
        return s.to_string();
    }
    let head: String = s.chars().take(OUTPUT_PREVIEW_MAX).collect();
    format!("{head}…\n（+{} 字已省略）", n - OUTPUT_PREVIEW_MAX)
}

fn parse_args(arguments: &str) -> Value {
    serde_json::from_str(arguments).unwrap_or_else(|_| json!({}))
}

/// 持久事件唯一出口：写入 turn 并 emit。
pub fn record_item(bus: &EventBus, turn: &mut Turn, item: TurnItem) {
    let (etype, domain, mut payload) = match &item {
        TurnItem::Reasoning { text } => (
            "agent.reasoning",
            "agent",
            json!({ "text": text, "runId": turn.run_id }),
        ),
        TurnItem::AssistantText {
            text,
            provider,
            degraded,
        } => (
            "agent.message",
            "agent",
            json!({
                "text": text,
                "runId": turn.run_id,
                "provider": provider,
                "degraded": degraded,
            }),
        ),
        TurnItem::ToolCall {
            call_id,
            name,
            arguments,
            parent_tool_call_id,
        } => {
            let mut p = json!({
                "name": name,
                "args": parse_args(arguments),
                "toolCallId": call_id,
                "runId": turn.run_id,
            });
            if let Some(parent) = parent_tool_call_id {
                p["parentToolCallId"] = json!(parent);
            }
            ("agent.tool.invoked", "tool", p)
        }
        TurnItem::ToolResult {
            call_id,
            name,
            output,
            is_error,
            denied,
            duration_ms,
            parent_tool_call_id,
        } => {
            let etype = if *denied {
                "agent.tool.denied"
            } else if *is_error {
                "agent.tool.failed"
            } else {
                "agent.tool.completed"
            };
            let mut p = json!({
                "name": name,
                "toolCallId": call_id,
                "runId": turn.run_id,
                "durationMs": duration_ms,
            });
            if *denied || *is_error {
                p["error"] = json!(output);
            } else {
                p["ok"] = json!(true);
                let prev = preview(output);
                p["output"] = json!(prev);
                p["outputPreview"] = json!(preview(&prev));
            }
            if let Some(parent) = parent_tool_call_id {
                p["parentToolCallId"] = json!(parent);
            }
            (etype, "tool", p)
        }
        TurnItem::SteeredUser { text } => (
            "agent.steered",
            "agent",
            json!({ "text": text, "runId": turn.run_id, "phase": "injected" }),
        ),
    };
    let _ = domain;
    let _ = etype;
    bus.emit(
        EventDraft::new(&turn.session_id, etype, domain)
            .payload(std::mem::take(&mut payload))
            .correlation(Some(turn.run_id.clone())),
    );
    turn.items.push(item);
}

/// 瞬时流式 delta（不落盘）。
pub fn emit_stream_delta(
    bus: &EventBus,
    session_id: &str,
    run_id: &str,
    event_type: &str,
    payload: Value,
) {
    bus.emit_ephemeral(
        EventDraft::new(session_id, event_type, "agent")
            .payload(payload)
            .correlation(Some(run_id.to_string())),
    );
}
