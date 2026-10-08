//! Turn / TurnItem：一轮用户输入的进度条目，经 [`record_item`] 唯一映射到 EventBus。
//! 语义对齐参考仓 agent-core engine/turn.rs（拷入改接，不引入其 crate）。

use serde_json::{json, Value};

use crate::events::{AgentEventContext, EventBus, EventDraft};

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
    pub agent_id: Option<String>,
    pub agent_run_id: Option<String>,
    pub parent_agent_id: Option<String>,
    pub parent_tool_call_id: Option<String>,
    pub team_id: Option<String>,
    pub task_id: Option<String>,
    pub agent_name: Option<String>,
    pub session_id: String,
    pub run_id: String,
    pub items: Vec<TurnItem>,
}

impl Turn {
    pub fn new(session_id: &str, run_id: &str) -> Self {
        Turn {
            agent_id: None,
            agent_run_id: None,
            parent_agent_id: None,
            parent_tool_call_id: None,
            team_id: None,
            task_id: None,
            agent_name: None,
            session_id: session_id.to_string(),
            run_id: run_id.to_string(),
            items: Vec::new(),
        }
    }

    pub fn event_context(&self) -> AgentEventContext {
        AgentEventContext {
            agent_id: self.agent_id.clone(),
            agent_run_id: self.agent_run_id.clone(),
            parent_agent_id: self.parent_agent_id.clone(),
            parent_tool_call_id: self.parent_tool_call_id.clone(),
            team_id: self.team_id.clone(),
            task_id: self.task_id.clone(),
            agent_name: self.agent_name.clone(),
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
    let mut context = turn.event_context();
    // Per-item legacy grouping takes precedence, allowing nested task tools to
    // retain their immediate parent's card when a turn also has an actor context.
    if let Some(parent) = payload.get("parentToolCallId").and_then(Value::as_str) {
        context.parent_tool_call_id = Some(parent.to_string());
    }
    bus.emit(
        context
            .event(
                &turn.session_id,
                etype,
                domain,
                std::mem::take(&mut payload),
            )
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn member_items_keep_actual_run_and_task_attribution_without_changing_display_run() {
        let dir = std::env::temp_dir().join(crate::events::new_id("forge-turn-attribution"));
        let bus = EventBus::new(dir.clone(), 32);
        let mut turn = Turn::new("session", "leader-run");
        turn.agent_id = Some("member".into());
        turn.agent_run_id = Some("member-run".into());
        turn.parent_agent_id = Some("leader".into());
        turn.parent_tool_call_id = Some("member-card".into());
        turn.team_id = Some("team".into());
        turn.task_id = Some("task".into());
        record_item(
            &bus,
            &mut turn,
            TurnItem::Reasoning {
                text: "analysis".into(),
            },
        );
        record_item(
            &bus,
            &mut turn,
            TurnItem::ToolCall {
                call_id: "call".into(),
                name: "read_file".into(),
                arguments: "{}".into(),
                parent_tool_call_id: None,
            },
        );
        record_item(
            &bus,
            &mut turn,
            TurnItem::ToolResult {
                call_id: "call".into(),
                name: "read_file".into(),
                output: "done".into(),
                is_error: false,
                denied: false,
                duration_ms: 1,
                parent_tool_call_id: Some("nested-card".into()),
            },
        );
        record_item(
            &bus,
            &mut turn,
            TurnItem::AssistantText {
                text: "done".into(),
                provider: "mock".into(),
                degraded: false,
            },
        );
        let events = bus.persisted("session");
        assert_eq!(events.len(), 4);
        for event in &events {
            assert_eq!(event.payload["runId"], "leader-run");
            assert_eq!(event.correlation_id.as_deref(), Some("leader-run"));
            assert_eq!(
                event.source.get("actor").map(String::as_str),
                Some("member")
            );
            for (key, value) in [
                ("agentId", "member"),
                ("agentRunId", "member-run"),
                ("parentAgentId", "leader"),
                ("teamId", "team"),
                ("taskId", "task"),
            ] {
                assert_eq!(event.payload[key], value);
            }
        }
        assert_eq!(events[0].payload["parentToolCallId"], "member-card");
        assert_eq!(events[2].payload["parentToolCallId"], "nested-card");
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn unattributed_legacy_turns_keep_their_wire_shape() {
        let dir = std::env::temp_dir().join(crate::events::new_id("forge-turn-legacy"));
        let bus = EventBus::new(dir.clone(), 16);
        let mut turn = Turn::new("session", "legacy-run");
        record_item(
            &bus,
            &mut turn,
            TurnItem::Reasoning {
                text: "analysis".into(),
            },
        );
        let event = &bus.persisted("session")[0];
        assert_eq!(
            event.payload,
            json!({"text":"analysis","runId":"legacy-run"})
        );
        assert_eq!(event.source.get("actor").map(String::as_str), Some("main"));
        std::fs::remove_dir_all(dir).unwrap();
    }
}
