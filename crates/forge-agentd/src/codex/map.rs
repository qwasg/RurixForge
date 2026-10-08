//! Codex 通知 → Forge 事件词汇。
//!
//! 这是「前端不需要另一套 Codex 界面」的关键:Codex 的 item 生命周期
//! (`item/started` → `item/*Delta` → `item/completed`)与 Forge 的
//! `agent.tool.invoked` → `agent.*.delta` → `agent.tool.completed` 是同一个形状,
//! 于是逐条翻译即可复用既有时间线、工具卡、审批卡与 Plan 页签。
//!
//! 映射是**纯函数式**的:[`Mapper::handle`] 只产出 [`Action`],由 [turn](super::turn)
//! 去 emit/落盘/写 todo。这样全部映射规则都能在全内存单测里断言,不必起子进程。
//!
//! 字段名一律多拼法兼容(`itemType`/`type`、`exitCode`/`exit_code`…):app-server 仍在
//! 演进,认死一种拼法会在某次 codex 升级后变成「Codex 干了活但界面一片空白」。

use serde_json::{json, Value};
use std::collections::HashMap;

/// 翻译产物:交给 turn 执行的副作用。
#[derive(Debug, Clone, PartialEq)]
pub enum Action {
    /// 持久事件(落 JSONL + SSE)。
    Emit {
        event_type: String,
        domain: &'static str,
        payload: Value,
    },
    /// 瞬时流式事件(只广播,不落盘)。
    Stream { event_type: String, payload: Value },
    /// 计划正文落盘(→ `.forge/plans/<slug>.plan.md`)。
    WritePlan {
        name: String,
        overview: String,
        body: String,
    },
    /// `turn/plan/updated` 的步骤 → TodoStore 同步。
    SyncTodos(Vec<PlanStep>),
    /// Codex 原生 goal 的本地持久镜像（避免下一轮用旧 active 状态回推）。
    SyncGoal(Value),
    /// Codex 清除了原生 goal，同步清本地镜像。
    ClearGoal,
    /// Another app-server client resolved a pending server request.
    ResolveServerRequest(Value),
    /// Native image bytes are persisted by the host, never copied into tool text.
    ImageResult { item: Value, payload: Value },
    /// 本轮结束(status:completed|failed|cancelled)。
    TurnEnd {
        status: String,
        error: Option<String>,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub struct PlanStep {
    pub step: String,
    /// pending | in_progress | completed
    pub status: String,
}

/// 工具项在 Forge 侧的门类(前端据此挑渲染块)。
fn tool_kind_of(item_type: &str) -> &'static str {
    match item_type {
        "commandExecution" => "command",
        "fileChange" => "fileChange",
        "webSearch" => "webSearch",
        "mcpToolCall" => "mcp",
        _ => "native",
    }
}

struct ToolTrack {
    name: String,
    kind: &'static str,
    started_ms: u128,
}

/// 一轮的翻译器。持有跨通知的累积态(助手终稿、推理终稿、进行中的工具项)。
pub struct Mapper {
    run_id: String,
    thread_id: Option<String>,
    tools: HashMap<String, ToolTrack>,
    /// 助手正文终稿(以 `item/completed` 的全文为准;没有 completed 才退回 delta 累积)。
    assistant: String,
    assistant_streamed: String,
    /// 计划正文的流式累积。
    plan_stream: String,
}

impl Mapper {
    pub fn new(run_id: &str) -> Self {
        Mapper {
            run_id: run_id.to_string(),
            thread_id: None,
            tools: HashMap::new(),
            assistant: String::new(),
            assistant_streamed: String::new(),
            plan_stream: String::new(),
        }
    }

    pub fn bind_thread(&mut self, thread_id: &str) {
        self.thread_id = Some(thread_id.to_string());
    }

    /// 该通知是否属于本轮线程。账户/额度类通知无 threadId,一律接纳。
    pub fn owns(&self, params: &Value) -> bool {
        match (super::rpc::thread_id_of(params), self.thread_id.as_deref()) {
            (Some(a), Some(b)) => a == b,
            (Some(_), None) => false,
            (None, _) => true,
        }
    }

    /// 助手终稿(turn 收尾时作为 `agent.message` 与 HTTP 返回体的 text)。
    pub fn final_text(&self) -> String {
        if !self.assistant.is_empty() {
            return self.assistant.clone();
        }
        self.assistant_streamed.clone()
    }

    /// 原生 Goal 会在同一个 thread 中连续自动启动多轮。每轮完成后清掉只属于
    /// 本轮的累积，下一轮不能把上一轮正文/工具跟踪重复带进来。
    pub fn reset_turn(&mut self) {
        self.tools.clear();
        self.assistant.clear();
        self.assistant_streamed.clear();
        self.plan_stream.clear();
    }

    fn emit(&self, event_type: &str, domain: &'static str, mut payload: Value) -> Action {
        payload["runId"] = json!(self.run_id);
        Action::Emit {
            event_type: event_type.to_string(),
            domain,
            payload,
        }
    }

    fn stream(&self, event_type: &str, mut payload: Value) -> Action {
        payload["runId"] = json!(self.run_id);
        Action::Stream {
            event_type: event_type.to_string(),
            payload,
        }
    }

    /// 翻译一条通知。返回空 vec = 本仓不关心(如 `item/updated` 的中间态)。
    pub fn handle(&mut self, method: &str, params: &Value) -> Vec<Action> {
        match method {
            // ---- 流式 delta(全部 ephemeral) ----
            "item/agentMessage/delta" => {
                let d = delta_of(params);
                self.assistant_streamed.push_str(&d);
                vec![self.stream("agent.token.stream.delta", json!({ "delta": d }))]
            }
            "item/reasoning/summaryTextDelta"
            | "item/reasoning/textDelta"
            | "item/reasoning/delta" => {
                vec![self.stream(
                    "agent.reasoning.delta",
                    json!({ "delta": delta_of(params) }),
                )]
            }
            "item/commandExecution/outputDelta" => {
                let id = item_id_of(params);
                vec![self.stream(
                    "agent.tool.output.delta",
                    json!({ "toolCallId": id, "delta": delta_of(params) }),
                )]
            }
            "item/fileChange/outputDelta" => {
                vec![self.stream(
                    "agent.tool.output.delta",
                    json!({ "toolCallId": item_id_of(params), "delta": delta_of(params) }),
                )]
            }
            "item/fileChange/patchUpdated" => {
                let changes = changes_of(params);
                if changes.is_empty() {
                    return Vec::new();
                }
                let delta = serde_json::to_string_pretty(&changes).unwrap_or_default();
                vec![self.stream(
                    "agent.tool.output.delta",
                    json!({
                        "toolCallId": item_id_of(params),
                        "delta": delta,
                        "changes": changes,
                    }),
                )]
            }
            "item/mcpToolCall/progress" => vec![self.stream(
                "agent.tool.output.delta",
                json!({ "toolCallId": item_id_of(params), "delta": delta_of(params) }),
            )],
            "item/plan/delta" | "item/planUpdate/delta" => {
                let d = delta_of(params);
                self.plan_stream.push_str(&d);
                vec![self.stream("agent.plan.delta", json!({ "delta": d }))]
            }

            // ---- item 生命周期 ----
            "item/started" => self.on_item_started(params),
            "item/completed" | "item/updated" => self.on_item_done(method, params),

            // ---- turn 级 ----
            "turn/plan/updated" => {
                let steps = plan_steps_of(params);
                if steps.is_empty() {
                    return Vec::new();
                }
                vec![Action::SyncTodos(steps)]
            }
            "turn/diff/updated" => {
                let diff = params
                    .get("diff")
                    .or_else(|| params.get("unifiedDiff"))
                    .cloned()
                    .unwrap_or(Value::Null);
                if diff.is_null() {
                    return Vec::new();
                }
                vec![self.emit("agent.diff.updated", "agent", json!({ "diff": diff }))]
            }
            "thread/tokenUsage/updated" | "turn/tokenUsage/updated" => {
                vec![self.emit("agent.usage", "agent", usage_payload(params))]
            }
            "turn/completed" => {
                if self.final_text().is_empty() {
                    if let Some(text) = params
                        .pointer("/turn/items")
                        .and_then(Value::as_array)
                        .and_then(|items| {
                            items.iter().rev().find_map(|item| {
                                (item_type_of(item) == "agentMessage")
                                    .then(|| text_of(item))
                                    .filter(|text| !text.is_empty())
                            })
                        })
                    {
                        self.assistant = text;
                    }
                }
                let status = params
                    .get("turn")
                    .and_then(|t| t.get("status"))
                    .and_then(Value::as_str)
                    .unwrap_or("completed");
                match status {
                    "failed" => vec![Action::TurnEnd {
                        status: "failed".to_string(),
                        error: Some(error_text(params)),
                    }],
                    "cancelled" | "canceled" | "interrupted" | "aborted" => {
                        vec![Action::TurnEnd {
                            status: "cancelled".to_string(),
                            error: None,
                        }]
                    }
                    _ => vec![Action::TurnEnd {
                        status: "completed".to_string(),
                        error: None,
                    }],
                }
            }
            "turn/failed" => vec![Action::TurnEnd {
                status: "failed".to_string(),
                error: Some(error_text(params)),
            }],
            "turn/aborted" | "turn/interrupted" | "turn/cancelled" => vec![Action::TurnEnd {
                status: "cancelled".to_string(),
                error: None,
            }],
            // Top-level `error` is diagnostic, not a terminal lifecycle event. Even
            // with willRetry=false app-server subsequently emits turn/completed;
            // ending here would drop that terminal event (and native Goal updates).
            "error" => {
                let message = error_text(params);
                let warning = self.emit(
                    "agent.warning",
                    "agent",
                    json!({
                        "code": "CODEX_ERROR",
                        "message": message,
                        "willRetry": params.get("willRetry").cloned().unwrap_or(json!(false)),
                        "codexErrorInfo": params.get("codexErrorInfo").cloned().unwrap_or(Value::Null),
                    }),
                );
                vec![warning]
            }

            // ---- 目标(Codex 原生 goal) ----
            "thread/goal/updated" => {
                let goal = super::normalize_goal_value(
                    params
                        .get("goal")
                        .cloned()
                        .unwrap_or_else(|| params.clone()),
                );
                vec![
                    Action::SyncGoal(goal.clone()),
                    self.emit(
                        "goal.updated",
                        "goal",
                        json!({ "goal": goal, "engine": "codex" }),
                    ),
                ]
            }
            "thread/goal/cleared" => {
                vec![
                    Action::ClearGoal,
                    self.emit("goal.cleared", "goal", json!({ "engine": "codex" })),
                ]
            }

            "serverRequest/resolved" => params
                .get("requestId")
                .cloned()
                .map(Action::ResolveServerRequest)
                .into_iter()
                .collect(),

            // ---- 账户/额度(ephemeral,状态栏与设置页用) ----
            "account/rateLimits/updated" => {
                let limits = params
                    .get("rateLimits")
                    .cloned()
                    .unwrap_or_else(|| params.clone());
                vec![self.stream("codex.rateLimits.updated", json!({ "rateLimits": limits }))]
            }
            "account/updated" | "authStatusChange" => {
                // 账户通知只转展示字段。即使旧版 authStatusChange 携带 token/key，
                // 也绝不能进入事件（ephemeral 仍会经过 SSE，不能视作安全存储）。
                vec![self.stream(
                    "codex.account.updated",
                    json!({ "account": safe_account_payload(params) }),
                )]
            }

            _ => Vec::new(),
        }
    }

    fn on_item_started(&mut self, params: &Value) -> Vec<Action> {
        let outer = item_of(params);
        let item_type = item_type_of(&outer);
        let id = outer
            .get("id")
            .and_then(Value::as_str)
            .map(str::to_string)
            .unwrap_or_else(|| item_id_of(params));
        // 单键包裹形态下负载在包裹里;摊平后下面各分支只看一种形状。
        let item = item_body(&outer, &item_type);
        match item_type.as_str() {
            // 正文/推理没有「开始」这一说,等 delta 与 completed。
            "agentMessage" | "reasoning" => Vec::new(),
            "commandExecution" => {
                let name = "shell";
                self.track(&id, name, "command");
                vec![self.emit(
                    "agent.tool.invoked",
                    "tool",
                    json!({
                        "name": name,
                        "toolKind": "command",
                        "toolCallId": id,
                        "args": {
                            "command": command_text(&item),
                            "cwd": item.get("cwd").cloned().unwrap_or(Value::Null),
                        },
                    }),
                )]
            }
            "fileChange" => {
                let name = "apply_patch";
                self.track(&id, name, "fileChange");
                vec![self.emit(
                    "agent.tool.invoked",
                    "tool",
                    json!({
                        "name": name,
                        "toolKind": "fileChange",
                        "toolCallId": id,
                        "args": { "changes": changes_of(&item) },
                    }),
                )]
            }
            "mcpToolCall" => {
                let name = mcp_tool_name(&item);
                self.track(&id, &name, "mcp");
                vec![self.emit(
                    "agent.tool.invoked",
                    "tool",
                    json!({
                        "name": name,
                        "toolKind": "mcp",
                        "toolCallId": id,
                        "args": item
                            .get("arguments")
                            .or_else(|| item.get("args"))
                            .cloned()
                            .unwrap_or_else(|| json!({})),
                    }),
                )]
            }
            "webSearch" => {
                let name = "web_search";
                self.track(&id, name, "webSearch");
                vec![self.emit(
                    "agent.tool.invoked",
                    "tool",
                    json!({
                        "name": name,
                        "toolKind": "webSearch",
                        "toolCallId": id,
                        "args": { "query": item.get("query").cloned().unwrap_or(Value::Null) },
                    }),
                )]
            }
            "dynamicToolCall" => {
                let name = item
                    .get("tool")
                    .or_else(|| item.get("name"))
                    .and_then(Value::as_str)
                    .unwrap_or("forge_tool")
                    .to_string();
                self.track(&id, &name, "native");
                vec![self.emit(
                    "agent.tool.invoked",
                    "tool",
                    json!({
                        "name": name,
                        "toolKind": "native",
                        "toolCallId": id,
                        "args": item
                            .get("arguments")
                            .cloned()
                            .unwrap_or_else(|| json!({})),
                    }),
                )]
            }
            "imageGeneration" => {
                self.track(&id, "imagegen", "native");
                vec![self.emit(
                    "agent.tool.invoked",
                    "tool",
                    json!({
                        "name": "imagegen", "toolKind": "native", "toolCallId": id,
                        "args": { "prompt": item.get("revisedPrompt"), "transparentBackground": item.get("transparentBackground") },
                    }),
                )]
            }
            // Codex 起子代理:映到本仓既有的 subagent 卡片(前端已有 subagent 块)。
            "collabAgentToolCall" | "collabToolCall" => {
                let title = item
                    .get("description")
                    .or_else(|| item.get("prompt"))
                    .and_then(Value::as_str)
                    .unwrap_or("Codex 子代理")
                    .to_string();
                self.track(&id, "collab", "native");
                vec![self.emit(
                    "subagent.started",
                    "agent",
                    json!({
                        "subagentId": id,
                        "description": title,
                        "subagentType": item
                            .get("agent")
                            .or_else(|| item.get("tool"))
                            .cloned()
                            .unwrap_or(Value::Null),
                        "tool": item.get("tool").cloned().unwrap_or(Value::Null),
                        "senderThreadId": item.get("senderThreadId").cloned().unwrap_or(Value::Null),
                        "receiverThreadIds": receiver_thread_ids(&item),
                        "agentStates": collab_agent_states(&item),
                    }),
                )]
            }
            _ => Vec::new(),
        }
    }

    fn on_item_done(&mut self, method: &str, params: &Value) -> Vec<Action> {
        let outer = item_of(params);
        let item_type = item_type_of(&outer);
        let id = outer
            .get("id")
            .and_then(Value::as_str)
            .map(str::to_string)
            .unwrap_or_else(|| item_id_of(params));
        // 与 on_item_started 同款摊平:status/exitCode 在包裹形态下也在包裹里。
        let item = item_body(&outer, &item_type);
        let completed = method == "item/completed";
        match item_type.as_str() {
            "agentMessage" if completed => {
                // 终稿以 completed 全文为准:delta 可能被限流合并,拼出来的不一定完整。
                let text = text_of(&item);
                if !text.is_empty() {
                    self.assistant = text;
                }
                Vec::new()
            }
            "reasoning" if completed => {
                let text = reasoning_text(&item);
                if text.is_empty() {
                    return Vec::new();
                }
                vec![self.emit("agent.reasoning", "agent", json!({ "text": text }))]
            }
            "plan" | "todoList" | "planUpdate" => {
                if !completed {
                    return Vec::new();
                }
                let body = {
                    let t = text_of(&item);
                    if t.is_empty() {
                        std::mem::take(&mut self.plan_stream)
                    } else {
                        t
                    }
                };
                if body.trim().is_empty() {
                    return Vec::new();
                }
                let name = item
                    .get("name")
                    .or_else(|| item.get("title"))
                    .and_then(Value::as_str)
                    .unwrap_or("Codex 计划")
                    .to_string();
                let overview = item
                    .get("overview")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string();
                vec![Action::WritePlan {
                    name,
                    overview,
                    body,
                }]
            }
            "error" => vec![self.emit(
                "agent.warning",
                "agent",
                json!({
                    "code": "CODEX_ITEM_ERROR",
                    "message": error_text(&item),
                    "itemId": id,
                }),
            )],
            "collabAgentToolCall" | "collabToolCall" if completed => {
                self.tools.remove(&id);
                let status = status_of(&item);
                let etype = if matches!(
                    status.as_str(),
                    "failed" | "interrupted" | "cancelled" | "canceled" | "aborted"
                ) {
                    "subagent.failed"
                } else {
                    "subagent.completed"
                };
                vec![self.emit(
                    etype,
                    "agent",
                    json!({
                        "subagentId": id,
                        "status": item
                            .get("status")
                            .or_else(|| item.get("agentStatus"))
                            .cloned()
                            .unwrap_or(Value::Null),
                        "result": item.get("result").cloned().unwrap_or(Value::Null),
                        "senderThreadId": item.get("senderThreadId").cloned().unwrap_or(Value::Null),
                        "receiverThreadIds": receiver_thread_ids(&item),
                        "agentStates": collab_agent_states(&item),
                    }),
                )]
            }
            // 工具类:只在 completed 收尾(updated 是中间态,重复发会让工具卡闪成两条)。
            "commandExecution" | "fileChange" | "mcpToolCall" | "webSearch" | "dynamicToolCall" | "imageGeneration" => {
                if !completed {
                    return Vec::new();
                }
                let Some(track) = self.tools.remove(&id) else {
                    // 没见过 started 的 completed:补一条 invoked 才不会让前端出现孤儿结果卡。
                    let mut out = self.on_item_started(params);
                    self.tools.remove(&id);
                    out.extend(self.tool_result(
                        &item,
                        &id,
                        &item_type_name(&item_type, &item),
                        tool_kind_of(&item_type),
                        0,
                    ));
                    return out;
                };
                let elapsed = now_ms().saturating_sub(track.started_ms) as u64;
                self.tool_result(&item, &id, &track.name, track.kind, elapsed)
            }
            _ => Vec::new(),
        }
    }

    fn tool_result(
        &self,
        item: &Value,
        id: &str,
        name: &str,
        kind: &'static str,
        duration_ms: u64,
    ) -> Vec<Action> {
        let status = status_of(item);
        let mut payload = json!({
            "name": name,
            "toolKind": kind,
            "toolCallId": id,
            "durationMs": duration_ms,
        });
        if name == "imagegen" {
            if status == "completed" && item.get("failure").is_none_or(Value::is_null) {
                payload["runId"] = json!(self.run_id);
                return vec![Action::ImageResult { item: item.clone(), payload }];
            }
            payload["error"] = json!(super::imagegen::failure_message(item));
            return vec![self.emit("agent.tool.failed", "tool", payload)];
        }
        if let Some(code) = item
            .get("exitCode")
            .or_else(|| item.get("exit_code"))
            .and_then(Value::as_i64)
        {
            payload["exitCode"] = json!(code);
        }
        let changes = changes_of(item);
        if !changes.is_empty() {
            payload["changes"] = json!(changes);
        }
        let etype = match status.as_str() {
            "declined" | "denied" | "rejected" => "agent.tool.denied",
            // 退出码非 0 也算失败:命令跑完但没成功,和「没跑起来」在界面上该同色。
            "failed" | "error" => "agent.tool.failed",
            _ if payload
                .get("exitCode")
                .and_then(Value::as_i64)
                .is_some_and(|c| c != 0) =>
            {
                "agent.tool.failed"
            }
            _ => "agent.tool.completed",
        };
        let output = output_text(item);
        if etype == "agent.tool.completed" {
            payload["ok"] = json!(true);
            payload["output"] = json!(output);
            payload["outputPreview"] = json!(preview(&output));
        } else {
            payload["error"] = json!(if output.is_empty() {
                error_text(item)
            } else {
                output
            });
        }
        vec![self.emit(etype, "tool", payload)]
    }

    fn track(&mut self, id: &str, name: &str, kind: &'static str) {
        self.tools.insert(
            id.to_string(),
            ToolTrack {
                name: name.to_string(),
                kind,
                started_ms: now_ms(),
            },
        );
    }
}

const OUTPUT_PREVIEW_MAX: usize = 2000;

fn preview(s: &str) -> String {
    let n = s.chars().count();
    if n <= OUTPUT_PREVIEW_MAX {
        return s.to_string();
    }
    let head: String = s.chars().take(OUTPUT_PREVIEW_MAX).collect();
    format!("{head}…\n(+{} 字已省略)", n - OUTPUT_PREVIEW_MAX)
}

fn now_ms() -> u128 {
    gend::timeutil::unix_millis()
}

fn item_of(params: &Value) -> Value {
    params
        .get("item")
        .cloned()
        .unwrap_or_else(|| params.clone())
}

/// item 门类。除显式字段外,还认「单键包裹」形态(`{"commandExecution": {...}}`)——
/// 那是 serde 的 externally-tagged 枚举默认序列化形态,codex 用它序列化 item 联合体。
fn item_type_of(item: &Value) -> String {
    for k in ["itemType", "item_type", "type", "kind"] {
        if let Some(v) = item.get(k).and_then(Value::as_str) {
            return v.to_string();
        }
    }
    if let Some(obj) = item.as_object() {
        let wrapped: Vec<&String> = obj.keys().filter(|k| k.as_str() != "id").collect();
        if wrapped.len() == 1 && obj[wrapped[0]].is_object() {
            return wrapped[0].clone();
        }
    }
    String::new()
}

/// 单键包裹形态下,真正的负载在包裹里;这里把两种形态摊平成同一个视图。
fn item_body(item: &Value, item_type: &str) -> Value {
    match item.get(item_type) {
        Some(v) if v.is_object() => v.clone(),
        _ => item.clone(),
    }
}

fn item_type_name(item_type: &str, _item: &Value) -> String {
    match item_type {
        "commandExecution" => "shell".to_string(),
        "fileChange" => "apply_patch".to_string(),
        "webSearch" => "web_search".to_string(),
        "imageGeneration" => "imagegen".to_string(),
        _ => item_type.to_string(),
    }
}

fn item_id_of(params: &Value) -> String {
    params
        .get("itemId")
        .or_else(|| params.get("item_id"))
        .or_else(|| params.get("id"))
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string()
}

fn delta_of(params: &Value) -> String {
    for k in ["delta", "text", "chunk", "output", "message"] {
        if let Some(s) = params.get(k).and_then(Value::as_str) {
            return s.to_string();
        }
    }
    String::new()
}

fn text_of(item: &Value) -> String {
    let body = item_body(item, &item_type_of(item));
    for k in ["text", "content", "plan", "message"] {
        if let Some(s) = body.get(k).and_then(Value::as_str) {
            return s.to_string();
        }
    }
    String::new()
}

fn reasoning_text(item: &Value) -> String {
    let body = item_body(item, &item_type_of(item));
    if let Some(arr) = body
        .get("summary")
        .or_else(|| body.get("summaryText"))
        .and_then(Value::as_array)
    {
        let joined: Vec<String> = arr
            .iter()
            .map(|v| match v.as_str() {
                Some(s) => s.to_string(),
                None => v
                    .get("text")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string(),
            })
            .filter(|s| !s.is_empty())
            .collect();
        if !joined.is_empty() {
            return joined.join("\n\n");
        }
    }
    text_of(item)
}

fn command_text(item: &Value) -> Value {
    let body = item_body(item, &item_type_of(item));
    for k in ["command", "commandLine", "cmd"] {
        if let Some(v) = body.get(k) {
            // 数组形态(argv)拼成一行给人看;字符串原样。
            if let Some(arr) = v.as_array() {
                let parts: Vec<String> = arr
                    .iter()
                    .map(|p| p.as_str().unwrap_or_default().to_string())
                    .collect();
                return json!(parts.join(" "));
            }
            return v.clone();
        }
    }
    Value::Null
}

fn output_text(item: &Value) -> String {
    let body = item_body(item, &item_type_of(item));
    for k in [
        "aggregatedOutput",
        "aggregated_output",
        "output",
        "stdout",
        "result",
        "text",
    ] {
        match body.get(k) {
            Some(Value::String(s)) => return s.clone(),
            Some(v) if !v.is_null() => return v.to_string(),
            _ => {}
        }
    }
    String::new()
}

fn status_of(item: &Value) -> String {
    let body = item_body(item, &item_type_of(item));
    body.get("status")
        .or_else(|| body.get("agentStatus"))
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string()
}

fn receiver_thread_ids(item: &Value) -> Value {
    if let Some(ids) = item.get("receiverThreadIds") {
        return ids.clone();
    }
    item.get("receiverThreadId")
        .or_else(|| item.get("newThreadId"))
        .cloned()
        .map(|id| json!([id]))
        .unwrap_or(Value::Null)
}

fn collab_agent_states(item: &Value) -> Value {
    if let Some(states) = item.get("agentStates") {
        return states.clone();
    }
    let Some(thread_id) = item
        .get("receiverThreadId")
        .or_else(|| item.get("newThreadId"))
        .and_then(Value::as_str)
    else {
        return Value::Null;
    };
    let status = item
        .get("agentStatus")
        .or_else(|| item.get("status"))
        .cloned()
        .unwrap_or(Value::Null);
    let mut states = serde_json::Map::new();
    states.insert(thread_id.to_string(), status);
    Value::Object(states)
}

fn changes_of(item: &Value) -> Vec<Value> {
    let body = item_body(item, &item_type_of(item));
    let raw = body.get("changes").or_else(|| body.get("files"));
    match raw {
        Some(Value::Array(arr)) => arr.clone(),
        // 对象形态 `{ "<path>": { kind, diff } }` → 摊成数组,前端只认一种形状。
        Some(Value::Object(obj)) => obj
            .iter()
            .map(|(path, v)| {
                json!({
                    "path": path,
                    "kind": v.get("kind").or_else(|| v.get("type")).cloned().unwrap_or(Value::Null),
                    "diff": v.get("diff").or_else(|| v.get("unifiedDiff")).cloned().unwrap_or(Value::Null),
                })
            })
            .collect(),
        _ => Vec::new(),
    }
}

fn mcp_tool_name(item: &Value) -> String {
    let body = item_body(item, &item_type_of(item));
    let server = body
        .get("server")
        .or_else(|| body.get("serverName"))
        .and_then(Value::as_str)
        .unwrap_or("unknown");
    let tool = body
        .get("tool")
        .or_else(|| body.get("toolName"))
        .or_else(|| body.get("name"))
        .and_then(Value::as_str)
        .unwrap_or("unknown");
    // 与本仓 TOOL_META 的动词表同名,Codex 调的 MCP 工具在界面上和本地引擎调的长一样。
    format!("mcp__{server}__{tool}")
}

fn plan_steps_of(params: &Value) -> Vec<PlanStep> {
    let raw = params
        .get("plan")
        .or_else(|| params.get("steps"))
        .or_else(|| params.get("items"));
    let Some(arr) = raw.and_then(Value::as_array) else {
        return Vec::new();
    };
    arr.iter()
        .filter_map(|v| {
            let step = v
                .get("step")
                .or_else(|| v.get("title"))
                .or_else(|| v.get("content"))
                .and_then(Value::as_str)
                .or_else(|| v.as_str())?;
            if step.trim().is_empty() {
                return None;
            }
            Some(PlanStep {
                step: step.to_string(),
                status: v
                    .get("status")
                    .and_then(Value::as_str)
                    .unwrap_or("pending")
                    .to_string(),
            })
        })
        .collect()
}

fn usage_payload(params: &Value) -> Value {
    let usage = params
        .get("usage")
        .or_else(|| params.get("tokenUsage"))
        .unwrap_or(params);
    // 正式 schema 是 tokenUsage.total / tokenUsage.last；旧版扁平形态继续兼容。
    let u = usage.get("total").unwrap_or(usage);
    let pick = |keys: &[&str]| -> u64 {
        for k in keys {
            if let Some(n) = u.get(*k).and_then(Value::as_u64) {
                return n;
            }
        }
        0
    };
    let prompt = pick(&["inputTokens", "input_tokens", "promptTokens"]);
    let completion = pick(&["outputTokens", "output_tokens", "completionTokens"]);
    let total = match pick(&["totalTokens", "total_tokens"]) {
        0 => prompt + completion,
        n => n,
    };
    let mut out = json!({
        "provider": "codex",
        "promptTokens": prompt,
        "completionTokens": completion,
        "totalTokens": total,
        "cachedTokens": pick(&["cachedInputTokens", "cached_input_tokens"]),
    });
    if let Some(last) = usage.get("last") {
        out["last"] = normalize_token_counts(last);
    }
    if let Some(window) = usage.get("modelContextWindow") {
        out["modelContextWindow"] = window.clone();
    }
    out
}

fn normalize_token_counts(u: &Value) -> Value {
    let pick = |keys: &[&str]| -> u64 {
        keys.iter()
            .find_map(|key| u.get(*key).and_then(Value::as_u64))
            .unwrap_or(0)
    };
    let input = pick(&["inputTokens", "input_tokens", "promptTokens"]);
    let output = pick(&["outputTokens", "output_tokens", "completionTokens"]);
    let total = match pick(&["totalTokens", "total_tokens"]) {
        0 => input + output,
        n => n,
    };
    json!({
        "promptTokens": input,
        "completionTokens": output,
        "totalTokens": total,
        "cachedTokens": pick(&["cachedInputTokens", "cached_input_tokens"]),
    })
}

fn safe_account_payload(params: &Value) -> Value {
    let root = params.get("account").unwrap_or(params);
    if root.is_null() {
        return Value::Null;
    }
    let mut out = serde_json::Map::new();
    for key in [
        "authMode",
        "auth_mode",
        "planType",
        "plan_type",
        "email",
        "authenticated",
        "requiresOpenaiAuth",
        "type",
    ] {
        if let Some(value) = root.get(key) {
            out.insert(key.to_string(), value.clone());
        }
    }
    Value::Object(out)
}

/// 错误文案。配额耗尽(UsageLimitExceeded)这类必须原文上屏——用户得知道是额度问题
/// 而不是「Codex 出错了」。
fn error_text(v: &Value) -> String {
    let candidates = [v.get("error"), v.get("turn").and_then(|t| t.get("error"))];
    for c in candidates.into_iter().flatten() {
        if let Some(s) = c.as_str() {
            return s.to_string();
        }
        if let Some(s) = c.get("message").and_then(Value::as_str) {
            let code = c
                .get("type")
                .or_else(|| c.get("code"))
                .and_then(Value::as_str);
            return match code {
                Some(t) if !s.contains(t) => format!("{t}: {s}"),
                _ => s.to_string(),
            };
        }
    }
    if let Some(s) = v.get("message").and_then(Value::as_str) {
        return s.to_string();
    }
    "Codex 未给出错误原因".to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mapper() -> Mapper {
        let mut m = Mapper::new("run_1");
        m.bind_thread("th_1");
        m
    }

    fn only_emit(actions: Vec<Action>) -> (String, Value) {
        assert_eq!(actions.len(), 1, "应恰好一条 action: {actions:?}");
        match actions.into_iter().next().unwrap() {
            Action::Emit {
                event_type,
                payload,
                ..
            } => (event_type, payload),
            other => panic!("应是持久事件,实得 {other:?}"),
        }
    }

    fn only_stream(actions: Vec<Action>) -> (String, Value) {
        assert_eq!(actions.len(), 1, "应恰好一条 action: {actions:?}");
        match actions.into_iter().next().unwrap() {
            Action::Stream {
                event_type,
                payload,
            } => (event_type, payload),
            other => panic!("应是瞬时事件,实得 {other:?}"),
        }
    }

    /// 线程归属:别的线程的通知一律不认(多会话并发共用一个 app-server)。
    #[test]
    fn foreign_thread_notifications_are_rejected() {
        let m = mapper();
        assert!(m.owns(&json!({ "threadId": "th_1" })));
        assert!(!m.owns(&json!({ "threadId": "th_other" })));
        // 无 threadId 的账户通知归所有人(状态栏额度要能更新)。
        assert!(m.owns(&json!({ "planType": "pro" })));
    }

    #[test]
    fn native_imagegen_lifecycle_keeps_binary_out_of_tool_text() {
        let mut m = mapper();
        let (kind, payload) = only_emit(m.handle("item/started", &json!({
            "threadId": "th_1", "item": {"type": "imageGeneration", "id": "img_1", "status": "in_progress", "result": ""}
        })));
        assert_eq!(kind, "agent.tool.invoked");
        assert_eq!(payload["name"], "imagegen");
        assert_eq!(payload["toolKind"], "native");
        assert!(m.handle("item/updated", &json!({"threadId":"th_1", "item":{"type":"imageGeneration","id":"img_1"}})).is_empty());
        let actions = m.handle("item/completed", &json!({
            "threadId": "th_1", "item": {"type": "imageGeneration", "id": "img_1", "status": "completed", "result": "png-base64", "savedPath": "C:/native/image.png"}
        }));
        assert_eq!(actions.len(), 1);
        let Action::ImageResult { item, payload } = &actions[0] else { panic!("expected image persistence"); };
        assert_eq!(item["result"], "png-base64");
        assert!(payload.get("output").is_none());
        assert_eq!(payload["toolCallId"], "img_1");
        assert_eq!(payload["runId"], "run_1");
    }

    #[test]
    fn native_imagegen_orphan_and_quota_failure_are_visible() {
        let mut m = mapper();
        let actions = m.handle("item/completed", &json!({
            "threadId":"th_1", "item":{"type":"imageGeneration","id":"img_2","status":"completed","result":"png-base64"}
        }));
        assert!(matches!(&actions[0], Action::Emit { payload, .. } if payload["name"] == "imagegen"));
        assert!(matches!(&actions[1], Action::ImageResult { payload, .. } if payload["name"] == "imagegen"));
        let actions = m.handle("item/completed", &json!({
            "threadId":"th_1", "item":{"type":"imageGeneration","id":"img_3","status":"failed","result":"", "failure":{"type":"usageLimitExceeded","limitId":"imagegen"}}
        }));
        assert!(matches!(&actions[1], Action::Emit { event_type, payload, .. }
            if event_type == "agent.tool.failed" && payload["error"].as_str().unwrap().contains("usageLimitExceeded")));
        assert!(!actions.iter().any(|action| matches!(action, Action::ImageResult { .. })));
    }

    #[test]
    fn resolved_server_request_releases_matching_approval() {
        let mut m = mapper();
        let actions = m.handle(
            "serverRequest/resolved",
            &json!({ "threadId": "th_1", "requestId": "req_7" }),
        );
        assert_eq!(actions, vec![Action::ResolveServerRequest(json!("req_7"))]);
    }

    /// 正文:delta 走 ephemeral,终稿以 completed 全文为准。
    #[test]
    fn agent_message_delta_then_final_text() {
        let mut m = mapper();
        let (t, p) = only_stream(m.handle(
            "item/agentMessage/delta",
            &json!({ "threadId": "th_1", "itemId": "i1", "delta": "你好" }),
        ));
        assert_eq!(t, "agent.token.stream.delta");
        assert_eq!(p["delta"], "你好");
        assert_eq!(p["runId"], "run_1");
        assert_eq!(m.final_text(), "你好");
        assert!(m
            .handle(
                "item/completed",
                &json!({ "threadId": "th_1", "item": {
                    "id": "i1", "itemType": "agentMessage", "text": "你好,已完成三处改动。"
                }}),
            )
            .is_empty());
        // delta 可能被限流合并,拼出来的不完整;终稿必须覆盖它。
        assert_eq!(m.final_text(), "你好,已完成三处改动。");
    }

    #[test]
    fn completed_turn_items_supply_missing_final_message() {
        let mut m = mapper();
        let actions = m.handle(
            "turn/completed",
            &json!({
                "threadId": "th_1",
                "turn": {
                    "status": "completed",
                    "items": [
                        { "id": "r1", "type": "reasoning", "text": "internal" },
                        { "id": "a1", "type": "agentMessage", "text": "最终答复" }
                    ]
                }
            }),
        );
        assert!(matches!(actions[0], Action::TurnEnd { .. }));
        assert_eq!(m.final_text(), "最终答复");
    }

    /// 命令执行:started → invoked(name=shell 对上 TOOL_META),输出 delta,
    /// completed → 带退出码;退出码非 0 判失败。
    #[test]
    fn command_execution_maps_to_shell_tool() {
        let mut m = mapper();
        let (t, p) = only_emit(m.handle(
            "item/started",
            &json!({ "threadId": "th_1", "item": {
                "id": "c1", "itemType": "commandExecution",
                "command": ["cargo", "test", "-p", "forge-agentd"], "cwd": "D:/proj"
            }}),
        ));
        assert_eq!(t, "agent.tool.invoked");
        assert_eq!(p["name"], "shell");
        assert_eq!(p["toolKind"], "command");
        assert_eq!(p["args"]["command"], "cargo test -p forge-agentd");
        assert_eq!(p["args"]["cwd"], "D:/proj");
        assert_eq!(p["toolCallId"], "c1");

        let (t, p) = only_stream(m.handle(
            "item/commandExecution/outputDelta",
            &json!({ "threadId": "th_1", "itemId": "c1", "chunk": "running 3 tests\n" }),
        ));
        assert_eq!(t, "agent.tool.output.delta");
        assert_eq!(p["toolCallId"], "c1");
        assert_eq!(p["delta"], "running 3 tests\n");

        let (t, p) = only_emit(m.handle(
            "item/completed",
            &json!({ "threadId": "th_1", "item": {
                "id": "c1", "itemType": "commandExecution", "status": "completed",
                "exitCode": 101, "aggregatedOutput": "test failed"
            }}),
        ));
        // 命令跑完但没成功,和没跑起来在界面上该同色。
        assert_eq!(t, "agent.tool.failed");
        assert_eq!(p["exitCode"], 101);
        assert_eq!(p["error"], "test failed");
    }

    /// 文件改动 → apply_patch;changes 的对象形态也摊平成数组。
    #[test]
    fn file_change_maps_to_apply_patch_and_flattens_changes() {
        let mut m = mapper();
        let (_, p) = only_emit(m.handle(
            "item/started",
            &json!({ "threadId": "th_1", "item": {
                "id": "f1", "itemType": "fileChange",
                "changes": { "src/a.rs": { "kind": "modify", "diff": "@@ -1 +1 @@" } }
            }}),
        ));
        assert_eq!(p["name"], "apply_patch");
        let ch = p["args"]["changes"].as_array().unwrap();
        assert_eq!(ch.len(), 1);
        assert_eq!(ch[0]["path"], "src/a.rs");
        assert_eq!(ch[0]["kind"], "modify");

        let (t, p) = only_emit(m.handle(
            "item/completed",
            &json!({ "threadId": "th_1", "item": {
                "id": "f1", "itemType": "fileChange", "status": "completed",
                "changes": [{ "path": "src/a.rs", "kind": "modify", "diff": "@@" }]
            }}),
        ));
        assert_eq!(t, "agent.tool.completed");
        assert_eq!(p["changes"][0]["path"], "src/a.rs");

        let (t, p) = only_stream(m.handle(
            "item/fileChange/patchUpdated",
            &json!({
                "threadId": "th_1", "itemId": "f1",
                "changes": { "src/b.rs": { "kind": "add", "diff": "+hello" } }
            }),
        ));
        assert_eq!(t, "agent.tool.output.delta");
        assert_eq!(p["changes"][0]["path"], "src/b.rs");
        assert!(p["delta"].as_str().unwrap().contains("src/b.rs"));
    }

    /// 用户拒批 → agent.tool.denied(前端已有拒绝态,不需要新块)。
    #[test]
    fn declined_command_maps_to_denied() {
        let mut m = mapper();
        m.handle(
            "item/started",
            &json!({ "threadId": "th_1", "item": {
                "id": "c9", "itemType": "commandExecution", "command": "rm -rf /"
            }}),
        );
        let (t, _) = only_emit(m.handle(
            "item/completed",
            &json!({ "threadId": "th_1", "item": {
                "id": "c9", "itemType": "commandExecution", "status": "declined"
            }}),
        ));
        assert_eq!(t, "agent.tool.denied");
    }

    /// MCP 工具名拼成本仓 TOOL_META 的同款全名(界面上与本地引擎无差别)。
    #[test]
    fn mcp_tool_call_uses_forge_tool_naming() {
        let mut m = mapper();
        let (_, p) = only_emit(m.handle(
            "item/started",
            &json!({ "threadId": "th_1", "item": {
                "id": "m1", "itemType": "mcpToolCall",
                "server": "engine-scene", "tool": "entity_create",
                "arguments": { "name": "Player" }
            }}),
        ));
        assert_eq!(p["name"], "mcp__engine-scene__entity_create");
        assert_eq!(p["args"]["name"], "Player");

        let (t, p) = only_stream(m.handle(
            "item/mcpToolCall/progress",
            &json!({ "threadId": "th_1", "itemId": "m1", "message": "loading assets" }),
        ));
        assert_eq!(t, "agent.tool.output.delta");
        assert_eq!(p["delta"], "loading assets");
    }

    /// externally-tagged 单键包裹形态(codex 的枚举默认序列化)也要认。
    #[test]
    fn wrapped_item_shape_is_understood() {
        let mut m = mapper();
        let (_, p) = only_emit(m.handle(
            "item/started",
            &json!({ "threadId": "th_1", "item": {
                "id": "w1",
                "webSearch": { "query": "bevy ecs" }
            }}),
        ));
        assert_eq!(p["name"], "web_search");
        assert_eq!(p["args"]["query"], "bevy ecs");
    }

    /// 没见过 started 的 completed 不能变成孤儿结果卡:补一条 invoked。
    #[test]
    fn orphan_completed_backfills_invoked() {
        let mut m = mapper();
        let actions = m.handle(
            "item/completed",
            &json!({ "threadId": "th_1", "item": {
                "id": "z1", "itemType": "commandExecution",
                "command": "ls", "status": "completed", "exitCode": 0, "output": "a b"
            }}),
        );
        let types: Vec<String> = actions
            .iter()
            .map(|a| match a {
                Action::Emit { event_type, .. } => event_type.clone(),
                other => format!("{other:?}"),
            })
            .collect();
        assert_eq!(
            types,
            vec!["agent.tool.invoked", "agent.tool.completed"],
            "{actions:?}"
        );
    }

    /// 计划 item → 落盘动作;plan 步骤 → todo 同步。
    #[test]
    fn plan_item_writes_file_and_steps_sync_todos() {
        let mut m = mapper();
        m.handle(
            "item/plan/delta",
            &json!({ "threadId": "th_1", "delta": "## 现状\n" }),
        );
        m.handle(
            "item/plan/delta",
            &json!({ "threadId": "th_1", "delta": "缺少存档系统。" }),
        );
        let actions = m.handle(
            "item/completed",
            &json!({ "threadId": "th_1", "item": { "id": "p1", "itemType": "plan" }}),
        );
        match &actions[0] {
            Action::WritePlan { name, body, .. } => {
                assert_eq!(name, "Codex 计划");
                // 没有终稿字段时用流式累积,不能丢正文。
                assert_eq!(body, "## 现状\n缺少存档系统。");
            }
            other => panic!("应落盘计划,实得 {other:?}"),
        }

        let actions = m.handle(
            "turn/plan/updated",
            &json!({ "threadId": "th_1", "plan": [
                { "step": "读现有存档代码", "status": "completed" },
                { "step": "写 SaveGame 组件", "status": "in_progress" }
            ]}),
        );
        match &actions[0] {
            Action::SyncTodos(steps) => {
                assert_eq!(steps.len(), 2);
                assert_eq!(steps[0].step, "读现有存档代码");
                assert_eq!(steps[1].status, "in_progress");
            }
            other => panic!("应同步 todos,实得 {other:?}"),
        }
    }

    /// 用量:codex 的 input/output 命名翻成本仓 prompt/completion;total 缺失自行相加。
    #[test]
    fn token_usage_is_normalized() {
        let mut m = mapper();
        let (t, p) = only_emit(m.handle(
            "thread/tokenUsage/updated",
            &json!({ "threadId": "th_1", "usage": {
                "inputTokens": 1200, "outputTokens": 300, "cachedInputTokens": 900
            }}),
        ));
        assert_eq!(t, "agent.usage");
        assert_eq!(p["promptTokens"], 1200);
        assert_eq!(p["completionTokens"], 300);
        assert_eq!(p["totalTokens"], 1500);
        assert_eq!(p["cachedTokens"], 900);
        assert_eq!(p["provider"], "codex");
    }

    #[test]
    fn nested_token_usage_uses_cumulative_total() {
        let mut m = mapper();
        let (t, p) = only_emit(m.handle(
            "thread/tokenUsage/updated",
            &json!({ "threadId": "th_1", "tokenUsage": {
                "total": {
                    "inputTokens": 1200, "outputTokens": 300,
                    "totalTokens": 1500, "cachedInputTokens": 900
                },
                "last": { "inputTokens": 20, "outputTokens": 5, "totalTokens": 25 },
                "modelContextWindow": 128000
            }}),
        ));
        assert_eq!(t, "agent.usage");
        assert_eq!(p["totalTokens"], 1500);
        assert_eq!(p["last"]["totalTokens"], 25);
        assert_eq!(p["modelContextWindow"], 128000);
    }

    #[test]
    fn formal_collab_agent_item_maps_to_subagent_lifecycle() {
        let mut m = mapper();
        let (started, payload) = only_emit(m.handle(
            "item/started",
            &json!({ "threadId": "th_1", "item": {
                "id": "collab_1", "type": "collabAgentToolCall",
                "tool": "spawn_agent", "prompt": "检查存档逻辑",
                "senderThreadId": "th_1", "receiverThreadIds": ["th_child"],
                "agentStates": { "th_child": "running" }, "status": "inProgress"
            }}),
        ));
        assert_eq!(started, "subagent.started");
        assert_eq!(payload["description"], "检查存档逻辑");
        assert_eq!(payload["receiverThreadIds"], json!(["th_child"]));

        let (completed, payload) = only_emit(m.handle(
            "item/completed",
            &json!({ "threadId": "th_1", "item": {
                "id": "collab_1", "type": "collabAgentToolCall",
                "tool": "spawn_agent", "prompt": "检查存档逻辑",
                "senderThreadId": "th_1", "receiverThreadIds": ["th_child"],
                "agentStates": { "th_child": "completed" }, "status": "completed"
            }}),
        ));
        assert_eq!(completed, "subagent.completed");
        assert_eq!(payload["agentStates"]["th_child"], "completed");

        let (failed, payload) = only_emit(m.handle(
            "item/completed",
            &json!({ "threadId": "th_1", "item": {
                "id": "collab_2", "type": "collabAgentToolCall",
                "newThreadId": "th_new", "agentStatus": "interrupted"
            }}),
        ));
        assert_eq!(failed, "subagent.failed");
        assert_eq!(payload["receiverThreadIds"], json!(["th_new"]));
        assert_eq!(payload["agentStates"]["th_new"], "interrupted");
    }

    #[test]
    fn raw_reasoning_text_delta_is_streamed() {
        let mut m = mapper();
        let (event, payload) = only_stream(m.handle(
            "item/reasoning/textDelta",
            &json!({ "threadId": "th_1", "delta": "checking state" }),
        ));
        assert_eq!(event, "agent.reasoning.delta");
        assert_eq!(payload["delta"], "checking state");
    }

    #[test]
    fn retryable_top_level_error_warns_without_ending_turn() {
        let mut m = mapper();
        let actions = m.handle(
            "error",
            &json!({
                "threadId": "th_1", "willRetry": true,
                "error": { "message": "temporary disconnect", "type": "stream" },
                "codexErrorInfo": { "kind": "stream" }
            }),
        );
        assert_eq!(actions.len(), 1);
        let Action::Emit {
            event_type,
            payload,
            ..
        } = &actions[0]
        else {
            panic!("可重试错误只能产生 warning: {actions:?}");
        };
        assert_eq!(event_type, "agent.warning");
        assert_eq!(payload["willRetry"], true);
        assert_eq!(payload["codexErrorInfo"]["kind"], "stream");

        let fatal = m.handle(
            "error",
            &json!({ "threadId": "th_1", "willRetry": false, "message": "fatal" }),
        );
        assert_eq!(
            fatal.len(),
            1,
            "terminal state must come from turn/completed"
        );
        assert!(
            matches!(fatal[0], Action::Emit { ref event_type, .. } if event_type == "agent.warning")
        );

        let item_error = m.handle(
            "item/completed",
            &json!({ "threadId": "th_1", "item": {
                "id": "err_1", "type": "error", "message": "tool stream failed"
            }}),
        );
        assert_eq!(item_error.len(), 1);
        assert!(
            matches!(item_error[0], Action::Emit { ref event_type, .. } if event_type == "agent.warning")
        );
    }

    #[test]
    fn account_events_never_forward_credentials() {
        let mut m = mapper();
        let (t, p) = only_stream(m.handle(
            "authStatusChange",
            &json!({
                "authMode": "chatgpt", "planType": "pro", "email": "a@b.c",
                "accessToken": "secret-access", "apiKey": "sk-secret"
            }),
        ));
        assert_eq!(t, "codex.account.updated");
        let wire = p.to_string();
        assert!(!wire.contains("secret-access"), "{wire}");
        assert!(!wire.contains("sk-secret"), "{wire}");
        assert_eq!(p["account"]["authMode"], "chatgpt");
    }

    /// 失败原因原文上屏:配额耗尽必须能被用户看懂,不能糊成「Codex 出错了」。
    #[test]
    fn failure_reason_is_verbatim() {
        let mut m = mapper();
        let actions = m.handle(
            "turn/failed",
            &json!({ "threadId": "th_1", "error": {
                "type": "UsageLimitExceeded", "message": "本周期用量已达上限,将于 3 小时后重置"
            }}),
        );
        match &actions[0] {
            Action::TurnEnd { status, error } => {
                assert_eq!(status, "failed");
                let e = error.clone().unwrap();
                assert!(e.contains("UsageLimitExceeded"), "{e}");
                assert!(e.contains("3 小时后重置"), "{e}");
            }
            other => panic!("应结束本轮,实得 {other:?}"),
        }
    }

    /// 中断 → cancelled(不是 failed:用户主动停不该在界面上显示成红色错误)。
    #[test]
    fn interrupt_maps_to_cancelled() {
        let mut m = mapper();
        let actions = m.handle(
            "turn/completed",
            &json!({ "threadId": "th_1", "turn": { "status": "interrupted" }}),
        );
        assert_eq!(
            actions,
            vec![Action::TurnEnd {
                status: "cancelled".to_string(),
                error: None
            }]
        );
    }

    /// 目标与额度:goal 走持久事件,额度走瞬时(高频推送不该把 JSONL 撑爆)。
    #[test]
    fn goal_persists_and_rate_limits_stream() {
        let mut m = mapper();
        let actions = m.handle(
            "thread/goal/updated",
            &json!({ "threadId": "th_1", "goal": { "objective": "做个平台跳跃 demo", "status": "active" }}),
        );
        assert!(matches!(&actions[0], Action::SyncGoal(goal) if goal["status"] == "active"));
        let (t, p) = only_emit(vec![actions[1].clone()]);
        assert_eq!(t, "goal.updated");
        assert_eq!(p["goal"]["objective"], "做个平台跳跃 demo");
        assert_eq!(p["engine"], "codex");

        let cleared = m.handle("thread/goal/cleared", &json!({ "threadId": "th_1" }));
        assert!(matches!(cleared[0], Action::ClearGoal));

        let (t, p) = only_stream(m.handle(
            "account/rateLimits/updated",
            &json!({ "rateLimits": { "primary": { "usedPercent": 12.0 } }}),
        ));
        assert_eq!(t, "codex.rateLimits.updated");
        assert_eq!(p["rateLimits"]["primary"]["usedPercent"], 12.0);
    }
}
