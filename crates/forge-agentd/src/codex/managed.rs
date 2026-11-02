//! Codex app-server as a step provider for Forge-owned jobs.
//!
//! A dynamic-tool request stays pending while `run_managed_tool_loop` executes
//! the existing Forge callback. The next step sends the real result back to
//! Codex. Thus both engines share permissions, stage exits, task scheduling,
//! tool events and visual evidence; Codex does not acquire a second coordinator.

use std::collections::{BTreeSet, HashMap, HashSet, VecDeque};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use serde_json::{json, Value};
use tokio::sync::{mpsc, Mutex};

use super::map::{ReasoningEvent, ReasoningTracker};
use super::rpc::{CodexClient, CodexError, Inbound};
use super::service::CodexService;
use crate::agent_job::{AgentJobRecord, AgentJobSpec, AgentJobStatus};
use crate::llm::{LlmError, StepFn, StepOutcome, StreamDelta, StreamSink, Usage};

const CAPABILITY_ERROR: &str = "CODEX_ULTRAPLAN_CAPABILITY_UNAVAILABLE";
const CANCELLED: &str = "CODEX_JOB_CANCELLED";
const POLL: Duration = Duration::from_millis(100);
const TOOL_BATCH_WINDOW: Duration = Duration::from_millis(30);
const INTERRUPT_BUDGET: Duration = Duration::from_secs(3);

/// One isolated process for an active flow. Jobs each get their own thread.
/// Dropping the last job/flow retires only this process, not ordinary Codex chats.
pub struct ManagedCodexFlow {
    runtime: Arc<ManagedCodexRuntime>,
    participants: std::sync::Mutex<HashMap<String, Arc<Mutex<Bridge>>>>,
}

/// Bridges own the transport, not their cache owner. This avoids a reference
/// cycle when a persistent member retains its conversation between tasks.
struct ManagedCodexRuntime {
    client: Arc<CodexClient>,
    ready: Mutex<bool>,
    closed: AtomicBool,
    unscoped_requests: std::sync::Mutex<HashSet<String>>,
    jobs: std::sync::Mutex<HashSet<String>>,
}

impl ManagedCodexFlow {
    pub fn new(service: &CodexService) -> Result<Arc<Self>, CodexError> {
        Ok(Self::from_client(service.isolated_client()?))
    }

    fn from_client(client: Arc<CodexClient>) -> Arc<Self> {
        Arc::new(Self {
            runtime: Arc::new(ManagedCodexRuntime {
                client,
                ready: Mutex::new(false),
                closed: AtomicBool::new(false),
                unscoped_requests: std::sync::Mutex::new(HashSet::new()),
                jobs: std::sync::Mutex::new(HashSet::new()),
            }),
            participants: std::sync::Mutex::new(HashMap::new()),
        })
    }

    pub fn cancel(&self) {
        self.runtime.cancel();
    }

    pub fn is_closed(&self) -> bool {
        self.runtime.closed.load(Ordering::Acquire)
    }

    /// Persistent team members keep one native thread across separate tasks.
    /// Callers pass only new task input/history, never replay the member's full
    /// local transcript into this already persistent Codex conversation.
    pub fn participant_step(self: &Arc<Self>, spec: AgentJobSpec) -> Box<StepFn> {
        let bridge = self
            .participants
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .entry(spec.job_id.clone())
            .or_insert_with(|| {
                Arc::new(Mutex::new(Bridge::new(self.runtime.clone(), spec.clone())))
            })
            .clone();
        let owner = self.clone();
        Box::new(move |messages, tools, stream| {
            let bridge = bridge.clone();
            let spec = spec.clone();
            let owner = owner.clone();
            Box::pin(async move {
                let _owner = owner;
                let mut bridge = bridge.lock().await;
                if bridge.spec.cwd != spec.cwd
                    || bridge.spec.model != spec.model
                    || bridge.spec.metadata != spec.metadata
                    || bridge.spec.state_dir != spec.state_dir
                {
                    return Err(capability("持久成员的目录、模型与身份不能在任务间改变"));
                }
                bridge.spec.cancelled = spec.cancelled;
                bridge.spec.retry_failed = spec.retry_failed;
                bridge.spec.resume_completed = false;
                let result = bridge.step(messages, tools, stream).await;
                if let Err(error) = &result {
                    bridge.abort().await;
                    bridge.record_failure(error);
                }
                result
            })
        })
    }

    /// Use the returned step exclusively with `llm::run_managed_tool_loop`.
    /// A subsequent leader repair loop may reuse it: after a completed turn it
    /// starts a new turn on this job's thread with the new complete input.
    pub fn step(self: &Arc<Self>, spec: AgentJobSpec) -> Box<StepFn> {
        let bridge = Arc::new(Mutex::new(Bridge::new(self.runtime.clone(), spec)));
        let owner = self.clone();
        Box::new(move |messages, tools, stream| {
            let bridge = bridge.clone();
            let owner = owner.clone();
            Box::pin(async move {
                let _owner = owner;
                let mut bridge = bridge.lock().await;
                let result = bridge.step(messages, tools, stream).await;
                if let Err(error) = &result {
                    bridge.abort().await;
                    bridge.record_failure(error);
                }
                result
            })
        })
    }
}

impl ManagedCodexRuntime {
    async fn ensure_ready(&self) -> Result<(), LlmError> {
        if self.closed.load(Ordering::Acquire) {
            return Err(LlmError::new(
                "CODEX_FLOW_STOPPED: 流程执行器已停止，请重试当前阶段",
            ));
        }
        let mut ready = self.ready.lock().await;
        if !*ready {
            self.client.ensure_started().await.map_err(as_llm)?;
            *ready = true;
        }
        Ok(())
    }

    pub fn cancel(&self) {
        self.closed.store(true, Ordering::Release);
        self.client.suspend();
    }
}

impl Drop for ManagedCodexFlow {
    fn drop(&mut self) {
        self.runtime.cancel();
    }
}

struct PendingTool {
    request_id: Value,
    call_id: String,
    name: String,
    arguments: Value,
}

struct Bridge {
    flow: Arc<ManagedCodexRuntime>,
    spec: AgentJobSpec,
    inbox: Option<mpsc::UnboundedReceiver<Inbound>>,
    queued_events: VecDeque<Inbound>,
    thread_id: Option<String>,
    turn_id: Option<String>,
    tools: HashMap<String, String>,
    tool_signature: Option<String>,
    pending: Vec<PendingTool>,
    seen_requests: HashSet<String>,
    last_message_count: usize,
    completed_messages: Vec<Value>,
    serial: usize,
    final_text: String,
    stream_text: String,
    reasoning: ReasoningTracker,
    usage: Option<Usage>,
    usage_seen: (u64, u64),
    claimed: bool,
    journal_checked: bool,
    record: Option<AgentJobRecord>,
    recovered: bool,
    terminal_confirmed: bool,
}

impl Bridge {
    fn new(flow: Arc<ManagedCodexRuntime>, spec: AgentJobSpec) -> Self {
        let reasoning = ReasoningTracker::new(&spec.job_id);
        Self {
            flow,
            spec,
            inbox: None,
            queued_events: VecDeque::new(),
            thread_id: None,
            turn_id: None,
            tools: HashMap::new(),
            tool_signature: None,
            pending: Vec::new(),
            seen_requests: HashSet::new(),
            last_message_count: 0,
            completed_messages: Vec::new(),
            serial: 0,
            final_text: String::new(),
            stream_text: String::new(),
            reasoning,
            usage: None,
            usage_seen: (0, 0),
            claimed: false,
            journal_checked: false,
            record: None,
            recovered: false,
            terminal_confirmed: false,
        }
    }

    fn check_cancelled(&self) -> Result<(), LlmError> {
        if (self.spec.cancelled)() || self.flow.closed.load(Ordering::Acquire) {
            Err(LlmError::new(CANCELLED))
        } else {
            Ok(())
        }
    }

    async fn cancellable<T>(
        &self,
        future: impl std::future::Future<Output = Result<T, LlmError>>,
    ) -> Result<T, LlmError> {
        tokio::pin!(future);
        loop {
            tokio::select! {
                result = &mut future => return result,
                _ = tokio::time::sleep(POLL) => {
                    if let Err(error) = self.check_cancelled() {
                        // During startup no turn id may exist yet. Retire the
                        // flow process so a late start cannot run unobserved.
                        self.flow.cancel();
                        return Err(error);
                    }
                }
            }
        }
    }

    async fn request(&self, method: &str, params: Value) -> Result<Value, LlmError> {
        self.cancellable(async {
            self.flow
                .client
                .request(method, params)
                .await
                .map_err(as_llm)
        })
        .await
    }

    async fn step(
        &mut self,
        messages: Vec<Value>,
        tools: Vec<Value>,
        stream: Option<StreamSink>,
    ) -> Result<StepOutcome, LlmError> {
        self.check_cancelled()?;
        let (declarations, mapping) = declarations(&tools)?;
        let signature =
            serde_json::to_string(&declarations).map_err(|e| LlmError::new(e.to_string()))?;
        if self
            .tool_signature
            .as_ref()
            .is_some_and(|previous| previous != &signature)
        {
            return Err(capability("同一个受管任务的工具集不能在轮次中途改变"));
        }
        self.load_journal(&signature)?;
        if self.recovered {
            if let Some(record) = &self.record {
                if record.result.status == AgentJobStatus::Completed && self.spec.resume_completed {
                    return Ok(StepOutcome {
                        message: json!({ "role": "assistant", "content": record.result.text }),
                        usage: None,
                    });
                }
                if matches!(
                    record.result.status,
                    AgentJobStatus::Failed
                        | AgentJobStatus::Cancelled
                        | AgentJobStatus::RecoveryRequired
                ) && !self.spec.retry_failed
                {
                    return Err(recovery(
                        "此任务已有终态；请由阶段控制器显式创建重试任务",
                        record,
                    ));
                }
            }
        }
        self.restart_known_startup_abort()?;
        self.cancellable(self.flow.ensure_ready()).await?;
        if self.thread_id.is_none() {
            self.tools = mapping;
            self.tool_signature = Some(signature);
            // Subscribe first: the server may emit notifications before the
            // thread/start or turn/start response reaches the caller.
            self.inbox = Some(self.flow.client.subscribe());
            let effective = self
                .request(
                    "config/read",
                    json!({ "includeLayers": false, "cwd": self.spec.cwd.to_string_lossy() }),
                )
                .await
                .map_err(|e| capability(&format!("无法读取受管线程配置: {e}")))?;
            let config = effective
                .get("config")
                .filter(|v| v.is_object())
                .ok_or_else(|| capability("config/read 未返回有效配置，无法隔离继承工具"))?;
            let catalog = self
                .request(
                    "skills/list",
                    json!({ "cwds": [self.spec.cwd.to_string_lossy()], "forceReload": true }),
                )
                .await
                .map_err(|e| capability(&format!("无法完整枚举受管线程技能，未降级: {e}")))?;
            let isolation = isolated_config(config, &catalog, &self.spec.cwd)?;
            let params = thread_params(&self.spec, &messages, declarations, isolation);
            if self.recovered {
                if let Some(result) = self.recover_existing(params).await? {
                    return Ok(result);
                }
            } else {
                self.persist_status(AgentJobStatus::Starting, None, None)?;
                let started = self.request("thread/start", params).await.map_err(|e| {
                    capability(&format!("动态工具与只读隔离为必需能力，未降级: {e}"))
                })?;
                self.thread_id = Some(
                    response_id(&started, "thread")
                        .ok_or_else(|| capability("thread/start 缺少 thread.id"))?,
                );
                self.persist_status(AgentJobStatus::Starting, None, None)?;
            }
        }
        // thread/start can succeed while reporting ignored config keys. Do not
        // start inference on a thread whose required isolation was rejected.
        while let Some(event) = self.inbox.as_mut().and_then(|inbox| inbox.try_recv().ok()) {
            self.check_isolation_event(&event)?;
            self.queued_events.push_back(event);
        }
        self.check_cancelled()?;
        if self.turn_id.is_none() {
            self.final_text.clear();
            self.stream_text.clear();
            self.reasoning.reset_turn();
            self.usage = None;
            self.seen_requests.clear();
            self.terminal_confirmed = false;
            if let Some(record) = &mut self.record {
                if !matches!(
                    record.result.status,
                    AgentJobStatus::Starting | AgentJobStatus::Running
                ) {
                    record.generation += 1;
                    record.result.text.clear();
                }
                record.turn_id = None;
            }
            self.persist_status(AgentJobStatus::Starting, None, None)?;
            let mut input = continuation_input(&messages, &self.completed_messages);
            if let Some(grant) = write_grant(&self.spec, self.tools.keys().map(String::as_str)) {
                input.insert(0, json!({ "type": "text", "text": grant }));
            }
            let mut params = json!({
                "threadId": self.thread_id,
                "input": input,
                "cwd": self.spec.cwd.to_string_lossy(),
                "approvalPolicy": "never",
                "sandboxPolicy": { "type": "readOnly" },
                "summary": "auto",
            });
            if let Some(model) = model_id(&self.spec) {
                params["model"] = json!(model);
            }
            if let Some(effort) = &self.spec.effort {
                params["effort"] = json!(effort);
            }
            let started = self.request("turn/start", params).await?;
            self.turn_id = Some(
                response_id(&started, "turn")
                    .ok_or_else(|| capability("turn/start 缺少 turn.id，无法安全中止"))?,
            );
            self.persist_status(AgentJobStatus::Running, None, None)?;
        } else {
            self.reply_tools(&messages)?;
        }
        self.last_message_count = messages.len();

        // Return a batch of outstanding tool calls to the ordinary Forge loop.
        // Subsequent dynamic calls from this same model response are allowed a
        // short batching window, preserving parallel explore dispatch.
        let mut batch_until = None;
        loop {
            self.check_cancelled()?;
            let wait = batch_until
                .map(|until: tokio::time::Instant| {
                    until.saturating_duration_since(tokio::time::Instant::now())
                })
                .unwrap_or(POLL);
            if batch_until.is_some() && wait.is_zero() {
                return Ok(self.tool_outcome());
            }
            let inbox = self
                .inbox
                .as_mut()
                .ok_or_else(|| capability("任务缺少事件订阅"))?;
            let next = match self.queued_events.pop_front() {
                Some(event) => Ok(Some(event)),
                None => tokio::time::timeout(wait, inbox.recv()).await,
            };
            match next {
                Err(_) if !self.pending.is_empty() => return Ok(self.tool_outcome()),
                Err(_) => continue,
                Ok(None) => {
                    return Err(LlmError::new("CODEX_JOB_CONNECTION_LOST: Codex 连接已断开"))
                }
                Ok(Some(msg)) => {
                    self.check_isolation_event(&msg)?;
                    if let Inbound::ServerRequest { id, params, .. } = &msg {
                        if super::rpc::thread_id_of(params).is_none() {
                            let first = self
                                .flow
                                .unscoped_requests
                                .lock()
                                .unwrap_or_else(|e| e.into_inner())
                                .insert(id.to_string());
                            if first {
                                let _ = self.flow.client.respond_error(
                                    id,
                                    -32602,
                                    "受管工具请求必须带 threadId",
                                );
                            }
                            return Err(capability("反向请求缺少线程归属，已拒绝执行"));
                        }
                        if super::rpc::thread_id_of(params) == self.thread_id.as_deref() {
                            if !self.flow.client.claim_server_request(id) {
                                continue;
                            }
                            let requested_turn = params
                                .get("turnId")
                                .and_then(Value::as_str)
                                .or_else(|| params.pointer("/turn/id").and_then(Value::as_str));
                            if requested_turn.is_none() || requested_turn != self.turn_id.as_deref()
                            {
                                let _ = self.flow.client.respond_error(
                                    id,
                                    -32602,
                                    "受管工具请求不属于当前活动轮次",
                                );
                                continue;
                            }
                        }
                    }
                    if !self.owns(&msg) {
                        continue;
                    }
                    match msg {
                        Inbound::ServerRequest { id, method, params } => {
                            if !self.seen_requests.insert(id.to_string()) {
                                continue;
                            }
                            if method != "item/tool/call" && method != "tool/call" {
                                self.flow.client.respond_error(&id, -32601, "受管任务仅可调用 Forge 动态工具；原生审批和用户输入不可绕过流程").map_err(as_llm)?;
                                return Err(capability(&format!("收到非受管工具请求 {method}")));
                            }
                            let remote = params
                                .get("tool")
                                .or_else(|| params.get("name"))
                                .and_then(Value::as_str)
                                .unwrap_or_default();
                            let Some(name) = self.tools.get(remote).cloned() else {
                                self.flow
                                    .client
                                    .respond(&id, tool_error("TOOL_FORBIDDEN: 不在本任务工具集中"))
                                    .map_err(as_llm)?;
                                continue;
                            };
                            self.serial += 1;
                            let call_id = format!("{}-{}", self.spec.job_id, self.serial);
                            let arguments = params
                                .get("arguments")
                                .or_else(|| params.get("args"))
                                .cloned()
                                .unwrap_or(json!({}));
                            self.pending.push(PendingTool {
                                request_id: id,
                                call_id,
                                name,
                                arguments,
                            });
                            batch_until
                                .get_or_insert(tokio::time::Instant::now() + TOOL_BATCH_WINDOW);
                        }
                        Inbound::Notification { method, params } => {
                            if let Some(out) =
                                self.notification(&method, &params, stream.as_ref())?
                            {
                                if !self.pending.is_empty() {
                                    return Err(capability("Codex 在工具结果返回前结束了轮次"));
                                }
                                self.completed_messages = messages.clone();
                                self.completed_messages.push(out.message.clone());
                                return Ok(out);
                            }
                        }
                    }
                }
            }
        }
    }

    fn check_isolation_event(&self, event: &Inbound) -> Result<(), LlmError> {
        if let Inbound::Notification { method, params } = event {
            if (method == "configWarning" && !disabled_project_config_warning(params))
                || method == "skills/changed"
            {
                // These notifications are process scoped; checking threadId
                // first would silently discard the isolation failure.
                self.flow.cancel();
                return Err(capability(&format!(
                    "受管配置未获接受或技能目录已变更，已停止隔离进程: {method}: {params}"
                )));
            }
        }
        Ok(())
    }

    fn owns(&self, msg: &Inbound) -> bool {
        let params = msg.params();
        // Unscoped account events are handled by the ordinary global watcher.
        // Never broadcast an unscoped reverse request to every concurrent job.
        if super::rpc::thread_id_of(params) != self.thread_id.as_deref() {
            return false;
        }
        let event_turn = params
            .get("turnId")
            .and_then(Value::as_str)
            .or_else(|| params.pointer("/turn/id").and_then(Value::as_str));
        !matches!((event_turn, self.turn_id.as_deref()), (Some(a), Some(b)) if a != b)
    }

    fn reply_tools(&mut self, messages: &[Value]) -> Result<(), LlmError> {
        if self.pending.is_empty() {
            return Err(capability("活动 Codex 轮次没有等待宿主的工具结果"));
        }
        let tail = messages.get(self.last_message_count..).unwrap_or_default();
        let context: Vec<Value> = tail
            .iter()
            .filter(|m| m["role"] == "user")
            .flat_map(dynamic_context)
            .collect();
        let count = self.pending.len();
        // Validate the complete batch before responding to any request.
        let results = self.pending.iter().enumerate().map(|(index, call)| {
            let result = tail.iter().find(|m| m["role"] == "tool" && m["tool_call_id"] == call.call_id)
                .ok_or_else(|| capability("外层工具循环未提供对应的工具结果"))?;
            let success = result.get("forgeToolSuccess").and_then(Value::as_bool)
                .ok_or_else(|| capability("Codex 受管任务必须使用 run_managed_tool_loop 保留真实工具状态"))?;
            let text = result.get("content").and_then(Value::as_str).unwrap_or_default();
            let mut content = vec![json!({ "type": "inputText", "text": text })];
            // The common loop groups images and new team inbox messages after
            // the batch. Preserve their source labels and any user steering,
            // once, on the final result while the native turn awaits tools.
            if index + 1 == count && !context.is_empty() {
                content.push(json!({ "type": "inputText", "text": "以下是宿主本批次工具证据和新增收件上下文：" }));
                content.extend(context.clone());
            }
            Ok((call.request_id.clone(), json!({ "success": success, "contentItems": content })))
        }).collect::<Result<Vec<_>, LlmError>>()?;
        self.check_cancelled()?;
        for (id, result) in results {
            self.flow.client.respond(&id, result).map_err(as_llm)?;
        }
        self.pending.clear();
        Ok(())
    }

    fn tool_outcome(&mut self) -> StepOutcome {
        StepOutcome {
            message: json!({ "role": "assistant", "content": null, "tool_calls": self.pending.iter().map(|p| json!({
                "id": p.call_id, "type": "function", "function": { "name": p.name, "arguments": match &p.arguments {
                    Value::String(s) => s.clone(), other => other.to_string(),
                } }
            })).collect::<Vec<_>>() }),
            usage: self.usage.take(),
        }
    }

    fn notification(
        &mut self,
        method: &str,
        params: &Value,
        stream: Option<&StreamSink>,
    ) -> Result<Option<StepOutcome>, LlmError> {
        if let Some(events) = self.reasoning.handle(method, params) {
            if let Some(sink) = stream {
                for event in events {
                    sink(match event {
                        ReasoningEvent::Started { item_id } => {
                            StreamDelta::ReasoningStarted { item_id }
                        }
                        ReasoningEvent::Delta { item_id, delta } => {
                            StreamDelta::ReasoningDelta { item_id, delta }
                        }
                        ReasoningEvent::Completed { item_id, text } => {
                            StreamDelta::ReasoningCompleted { item_id, text }
                        }
                    });
                }
            }
            return Ok(None);
        }
        match method {
            "item/agentMessage/delta" => {
                let delta = params["delta"].as_str().unwrap_or_default();
                self.stream_text.push_str(delta);
                if let Some(sink) = stream {
                    sink(StreamDelta::Text(delta.to_string()));
                }
            }
            "item/started" | "item/completed" => {
                let item = params.get("item").unwrap_or(params);
                let kind = item
                    .get("type")
                    .or_else(|| item.get("itemType"))
                    .and_then(Value::as_str)
                    .unwrap_or_default();
                if matches!(
                    kind,
                    "commandExecution"
                        | "imageGeneration"
                        | "fileChange"
                        | "mcpToolCall"
                        | "collabAgentToolCall"
                        | "collabToolCall"
                ) {
                    self.flow.cancel();
                    return Err(capability(&format!(
                        "检测到绕过宿主工具桥的原生能力 {kind}，已停止流程进程"
                    )));
                }
                if method == "item/completed" && kind == "agentMessage" {
                    if let Some(text) = item.get("text").and_then(Value::as_str) {
                        self.final_text = text.to_string();
                    }
                }
            }
            "thread/tokenUsage/updated" => {
                let total = params
                    .pointer("/tokenUsage/total")
                    .or_else(|| params.pointer("/usage/total"));
                if let Some(u) = total {
                    let input = u["inputTokens"].as_u64().unwrap_or(0);
                    let output = u["outputTokens"].as_u64().unwrap_or(0);
                    let fresh_input = input.saturating_sub(self.usage_seen.0);
                    let fresh_output = output.saturating_sub(self.usage_seen.1);
                    self.usage_seen = (input, output);
                    let prior = self.usage.take().unwrap_or(Usage {
                        prompt_tokens: 0,
                        completion_tokens: 0,
                        total_tokens: 0,
                    });
                    self.usage = Some(Usage {
                        prompt_tokens: prior.prompt_tokens + fresh_input,
                        completion_tokens: prior.completion_tokens + fresh_output,
                        total_tokens: prior.total_tokens + fresh_input + fresh_output,
                    });
                }
            }
            "thread/goal/updated"
                if params.pointer("/goal/status").and_then(Value::as_str) == Some("active") =>
            {
                self.flow.cancel();
                return Err(capability("受管任务不得启动 Codex 原生 Goal 续跑"));
            }
            "turn/completed" => {
                let turn = params.get("turn").unwrap_or(params);
                let status = turn["status"].as_str().unwrap_or("failed");
                self.terminal_confirmed = matches!(status, "completed" | "failed" | "interrupted");
                self.turn_id = None;
                if status != "completed" {
                    return Err(LlmError::new(format!(
                        "CODEX_JOB_FAILED: {}",
                        turn.get("error")
                            .filter(|v| !v.is_null())
                            .map(Value::to_string)
                            .unwrap_or_else(|| status.to_string())
                    )));
                }
                let text = if self.final_text.is_empty() {
                    self.stream_text.clone()
                } else {
                    self.final_text.clone()
                };
                self.persist_status(AgentJobStatus::Completed, Some(text.clone()), None)?;
                return Ok(Some(StepOutcome {
                    message: json!({ "role": "assistant", "content": text }),
                    usage: self.usage.take(),
                }));
            }
            "error" if params.get("willRetry") != Some(&Value::Bool(true)) => {
                return Err(LlmError::new(format!(
                    "CODEX_JOB_FAILED: {}",
                    params
                        .get("message")
                        .and_then(Value::as_str)
                        .unwrap_or("Codex 运行失败")
                )));
            }
            _ => {}
        }
        Ok(None)
    }

    fn load_journal(&mut self, signature: &str) -> Result<(), LlmError> {
        if self.journal_checked {
            return Ok(());
        }
        if !self
            .flow
            .jobs
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(self.spec.job_id.clone())
        {
            return Err(LlmError::new(
                "CODEX_JOB_ALREADY_RUNNING: 同一流程任务正在执行",
            ));
        }
        self.claimed = true;
        let record = if let Some(path) = crate::agent_job::journal_path(&self.spec) {
            match std::fs::read_to_string(&path) {
                Ok(text) => {
                    let record: AgentJobRecord = serde_json::from_str(&text)
                        .map_err(|e| LlmError::new(format!("CODEX_JOB_JOURNAL_INVALID: {e}")))?;
                    if record.schema_version != 1
                        || record.job_id != self.spec.job_id
                        || record.engine != "codex"
                        || record.cwd != self.spec.cwd
                        || record.metadata != self.spec.metadata
                        || record.tool_signature != signature
                    {
                        return Err(LlmError::new(
                            "CODEX_JOB_IDENTITY_MISMATCH: 已有任务的需求、范围或工具集与本次不同",
                        ));
                    }
                    self.recovered = true;
                    record
                }
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                    AgentJobRecord::starting(&self.spec, "codex", signature.to_string())
                }
                Err(e) => return Err(LlmError::new(format!("CODEX_JOB_JOURNAL_IO: {e}"))),
            }
        } else {
            AgentJobRecord::starting(&self.spec, "codex", signature.to_string())
        };
        self.record = Some(record);
        self.journal_checked = true;
        Ok(())
    }

    fn restart_known_startup_abort(&mut self) -> Result<(), LlmError> {
        let Some(old) = self.record.as_ref().filter(|old| {
            self.recovered
                && self.spec.retry_failed
                && old.result.status == AgentJobStatus::Failed
                && old.thread_id.is_some()
                && old.turn_id.is_none()
        }) else {
            return Ok(());
        };
        let prefix = format!(
            "{CAPABILITY_ERROR}: 受管配置未获接受或技能目录已变更，已停止隔离进程: configWarning: "
        );
        let known = old
            .result
            .error
            .as_deref()
            .and_then(|error| error.strip_prefix(&prefix))
            .and_then(|params| serde_json::from_str::<Value>(params).ok())
            .is_some_and(|params| disabled_project_config_warning(&params));
        if !known {
            return Ok(());
        }
        let path = crate::agent_job::journal_path(&self.spec)
            .ok_or_else(|| recovery("启动失败缺少持久记录，禁止新建线程", old))?;
        let archive = path.with_file_name(format!(
            "{}.g{}.startup-abort.json",
            path.file_stem().unwrap().to_string_lossy(),
            old.generation
        ));
        let body = std::fs::read(&path)
            .map_err(|e| LlmError::new(format!("CODEX_JOB_JOURNAL_IO: {e}")))?;
        use std::io::Write;
        match std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&archive)
        {
            Ok(mut file) => file
                .write_all(&body)
                .and_then(|_| file.sync_all())
                .map_err(|e| LlmError::new(format!("CODEX_JOB_JOURNAL_IO: {e}")))?,
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                if std::fs::read(&archive).ok().as_ref() != Some(&body) {
                    return Err(recovery("启动失败归档冲突，已保留原记录", old));
                }
            }
            Err(e) => return Err(LlmError::new(format!("CODEX_JOB_JOURNAL_IO: {e}"))),
        }
        let generation = old
            .generation
            .checked_add(1)
            .ok_or_else(|| recovery("任务代次溢出，禁止新建线程", old))?;
        let record = self.record.as_mut().unwrap();
        record.generation = generation;
        record.thread_id = None;
        record.turn_id = None;
        record.terminal_confirmed = false;
        record.result.status = AgentJobStatus::Starting;
        record.result.text.clear();
        record.result.error = None;
        record
            .result
            .evidence
            .push(archive.to_string_lossy().into_owned());
        self.thread_id = None;
        self.turn_id = None;
        self.terminal_confirmed = false;
        self.recovered = false;
        self.persist_status(AgentJobStatus::Starting, None, None)
    }

    fn persist_status(
        &mut self,
        status: AgentJobStatus,
        text: Option<String>,
        error: Option<String>,
    ) -> Result<(), LlmError> {
        let Some(record) = &mut self.record else {
            return Ok(());
        };
        record.thread_id = self.thread_id.clone();
        // Retain the completed turn identity as part of the durable result.
        if self.turn_id.is_some() {
            record.turn_id = self.turn_id.clone();
        }
        record.result.status = status;
        record.terminal_confirmed = self.terminal_confirmed;
        if let Some(text) = text {
            record.result.text = text;
        }
        record.result.error = error;
        if let Some(path) = crate::agent_job::journal_path(&self.spec) {
            let text =
                serde_json::to_string_pretty(record).map_err(|e| LlmError::new(e.to_string()))?;
            crate::sessions::write_atomic(&path, &text)
                .map_err(|e| LlmError::new(format!("CODEX_JOB_JOURNAL_IO: {e}")))?;
        }
        Ok(())
    }

    fn record_failure(&mut self, error: &LlmError) {
        // Recovery errors preserve the old identity and completed result.
        if self.recovered {
            return;
        }
        let message = error.to_string();
        let status = if message.contains(CANCELLED) {
            AgentJobStatus::Cancelled
        } else {
            AgentJobStatus::Failed
        };
        if let Err(write_error) = self.persist_status(status, None, Some(message)) {
            eprintln!("[codex-managed] {write_error}");
        }
    }

    async fn recover_existing(
        &mut self,
        mut params: Value,
    ) -> Result<Option<StepOutcome>, LlmError> {
        let old = self.record.as_ref().expect("journal loaded").clone();
        let thread = old
            .thread_id
            .as_deref()
            .ok_or_else(|| recovery("上次线程启动结果未知，禁止重复开工", &old))?;
        let turn = old.turn_id.as_deref();
        params
            .as_object_mut()
            .expect("thread params object")
            .remove("dynamicTools");
        params["threadId"] = json!(thread);
        let resumed = self
            .request("thread/resume", params)
            .await
            .map_err(|e| recovery(&format!("无法恢复原线程，未创建替代线程: {e}"), &old))?;
        if response_id(&resumed, "thread").as_deref() != Some(thread) {
            return Err(recovery("恢复返回了错误线程", &old));
        }
        self.thread_id = Some(thread.to_string());
        let Some(turn) = turn else {
            if !self.spec.retry_failed
                || !matches!(
                    old.result.status,
                    AgentJobStatus::Failed
                        | AgentJobStatus::Cancelled
                        | AgentJobStatus::RecoveryRequired
                )
            {
                return Err(recovery("上次轮次启动结果未知，禁止重复开工", &old));
            }
            // A saved thread without a saved turn can represent either a
            // pre-inference failure or an unacknowledged turn/start. Only an
            // explicit empty upstream history proves that retry is safe.
            let read = self
                .request(
                    "thread/read",
                    json!({ "threadId": thread, "includeTurns": true }),
                )
                .await
                .map_err(|e| recovery(&format!("无法确认原线程尚未开始轮次: {e}"), &old))?;
            if read.pointer("/thread/id").and_then(Value::as_str) != Some(thread)
                || !read
                    .pointer("/thread/turns")
                    .and_then(Value::as_array)
                    .is_some_and(Vec::is_empty)
            {
                return Err(recovery("原线程轮次历史非空或无法确认，禁止重复开工", &old));
            }
            self.recovered = false;
            return Ok(None);
        };
        if old.result.status == AgentJobStatus::Completed
            || (self.spec.retry_failed
                && old.terminal_confirmed
                && matches!(
                    old.result.status,
                    AgentJobStatus::Failed
                        | AgentJobStatus::Cancelled
                        | AgentJobStatus::RecoveryRequired
                ))
        {
            self.recovered = false;
            return Ok(None);
        }
        self.turn_id = Some(turn.to_string());
        let read = self
            .request(
                "thread/read",
                json!({ "threadId": thread, "includeTurns": true }),
            )
            .await?;
        let saved = read
            .pointer("/thread/turns")
            .and_then(Value::as_array)
            .and_then(|turns| turns.iter().find(|value| value["id"] == turn));
        if let Some(saved) = saved {
            if saved["status"] == "completed" {
                self.terminal_confirmed = true;
                if self.spec.retry_failed
                    && matches!(
                        old.result.status,
                        AgentJobStatus::Failed
                            | AgentJobStatus::Cancelled
                            | AgentJobStatus::RecoveryRequired
                    )
                {
                    self.turn_id = None;
                    self.recovered = false;
                    return Ok(None);
                }
                let text = saved["items"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter(|item| item["type"] == "agentMessage")
                    .filter_map(|item| item["text"].as_str())
                    .collect::<Vec<_>>()
                    .join("\n\n");
                self.turn_id = None;
                self.persist_status(AgentJobStatus::Completed, Some(text.clone()), None)?;
                self.recovered = false;
                return Ok(Some(StepOutcome {
                    message: json!({ "role": "assistant", "content": text }),
                    usage: None,
                }));
            }
            if matches!(saved["status"].as_str(), Some("interrupted" | "failed")) {
                self.turn_id = None;
                self.terminal_confirmed = true;
                if self.spec.retry_failed
                    && matches!(
                        old.result.status,
                        AgentJobStatus::Failed
                            | AgentJobStatus::Cancelled
                            | AgentJobStatus::RecoveryRequired
                    )
                {
                    self.recovered = false;
                    return Ok(None);
                }
            }
        }
        // Host tools may already have changed the project before the crash.
        // Never replay them without the coordinator reconciling its receipts.
        self.abort().await;
        let error = recovery(
            "已恢复原线程并停止未确认轮次；阶段控制器须核对现有产物后显式重试",
            &old,
        );
        self.persist_status(
            AgentJobStatus::RecoveryRequired,
            None,
            Some(error.to_string()),
        )?;
        Err(error)
    }

    async fn abort(&mut self) {
        for pending in self.pending.drain(..) {
            let _ = self
                .flow
                .client
                .respond(&pending.request_id, tool_error(CANCELLED));
        }
        if let (Some(thread_id), Some(turn_id)) = (&self.thread_id, self.turn_id.take()) {
            let confirmed = interrupt_and_confirm(&self.flow, thread_id, &turn_id).await;
            self.terminal_confirmed = confirmed;
            if !confirmed {
                self.flow.cancel();
            }
        }
    }
}

impl Drop for Bridge {
    fn drop(&mut self) {
        if self.claimed {
            self.flow
                .jobs
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .remove(&self.spec.job_id);
        }
        for pending in self.pending.drain(..) {
            let _ = self
                .flow
                .client
                .respond(&pending.request_id, tool_error(CANCELLED));
        }
        if let (Some(thread_id), Some(turn_id)) = (self.thread_id.clone(), self.turn_id.take()) {
            let flow = self.flow.clone();
            if let Ok(handle) = tokio::runtime::Handle::try_current() {
                handle.spawn(async move {
                    if !interrupt_and_confirm(&flow, &thread_id, &turn_id).await {
                        flow.cancel();
                    }
                });
            } else {
                self.flow.cancel();
            }
        }
    }
}

/// An interrupt RPC acknowledgement only means the request was received. Wait
/// for this exact turn to become terminal before allowing the process to live.
async fn interrupt_and_confirm(flow: &ManagedCodexRuntime, thread: &str, turn: &str) -> bool {
    let mut events = flow.client.subscribe();
    tokio::time::timeout(INTERRUPT_BUDGET, async {
        flow.client
            .request_with_timeout(
                "turn/interrupt",
                json!({ "threadId": thread, "turnId": turn }),
                INTERRUPT_BUDGET,
            )
            .await
            .map_err(|_| ())?;
        while let Some(event) = events.recv().await {
            let params = event.params();
            if super::rpc::thread_id_of(params) != Some(thread) {
                continue;
            }
            let event_turn = params
                .get("turnId")
                .and_then(Value::as_str)
                .or_else(|| params.pointer("/turn/id").and_then(Value::as_str));
            if event_turn != Some(turn) {
                continue;
            }
            match event {
                Inbound::Notification { method, params } if method == "turn/completed" => {
                    let status = params
                        .pointer("/turn/status")
                        .or_else(|| params.get("status"))
                        .and_then(Value::as_str);
                    if matches!(status, Some("completed" | "interrupted" | "failed")) {
                        return Ok(());
                    }
                }
                Inbound::ServerRequest { id, method, .. } => {
                    if method == "item/tool/call" || method == "tool/call" {
                        let _ = flow.client.respond(&id, tool_error(CANCELLED));
                    } else {
                        let _ = flow.client.respond_error(&id, -32601, CANCELLED);
                    }
                }
                _ => {}
            }
        }
        Err(())
    })
    .await
    .is_ok_and(|result| result.is_ok())
}

fn dynamic_context(message: &Value) -> Vec<Value> {
    match &message["content"] {
        Value::String(text) => vec![json!({ "type": "inputText", "text": text })],
        Value::Array(blocks) => blocks
            .iter()
            .filter_map(|block| {
                if let Some(url) = block.pointer("/image_url/url").and_then(Value::as_str) {
                    Some(json!({ "type": "inputImage", "imageUrl": url }))
                } else {
                    block
                        .get("text")
                        .and_then(Value::as_str)
                        .map(|text| json!({ "type": "inputText", "text": text }))
                }
            })
            .collect(),
        _ => Vec::new(),
    }
}

fn capability(message: &str) -> LlmError {
    LlmError::new(format!("{CAPABILITY_ERROR}: {message}"))
}
fn recovery(message: &str, record: &AgentJobRecord) -> LlmError {
    LlmError::new(format!(
        "CODEX_JOB_RECOVERY_REQUIRED: {}: {message}",
        record.job_id
    ))
}
fn as_llm(error: CodexError) -> LlmError {
    LlmError::new(error.0)
}
fn tool_error(message: &str) -> Value {
    json!({ "success": false, "contentItems": [{ "type": "inputText", "text": message }] })
}

fn response_id(value: &Value, kind: &str) -> Option<String> {
    value
        .get(kind)
        .and_then(|v| v.get("id"))
        .or_else(|| value.get(format!("{kind}Id")))
        .or_else(|| value.get("id"))
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

fn declarations(tools: &[Value]) -> Result<(Vec<Value>, HashMap<String, String>), LlmError> {
    let mut mapping = HashMap::new();
    let mut out = Vec::new();
    for tool in tools {
        let f = tool
            .get("function")
            .ok_or_else(|| capability("宿主工具声明缺少 function"))?;
        let name = f
            .get("name")
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
            .ok_or_else(|| capability("宿主工具名称无效"))?;
        let safe: String = name
            .chars()
            .map(|ch| {
                if ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-') {
                    ch
                } else {
                    '_'
                }
            })
            .collect();
        let remote = format!("forge__{safe}");
        if mapping.insert(remote.clone(), name.to_string()).is_some() {
            return Err(capability("宿主工具名规范化后冲突"));
        }
        out.push(json!({ "name": remote, "description": f.get("description").cloned().unwrap_or(json!(name)), "inputSchema": f.get("parameters").cloned().unwrap_or(json!({ "type": "object", "properties": {} })) }));
    }
    Ok((out, mapping))
}

/// An untrusted project's local settings staying disabled is expected. Skills
/// still require the separate exhaustive catalog and per-thread isolation below.
fn disabled_project_config_warning(params: &Value) -> bool {
    const HEADER: &str = "Project-local config, hooks, and exec policies are disabled in the following folders until the project is trusted, but skills still load.\n";
    const GUIDANCE: &str = "       To load project-local config, hooks, and exec policies, add ";
    if !params["details"].is_null() {
        return false;
    }
    let Some(body) = params["summary"]
        .as_str()
        .and_then(|s| s.strip_prefix(HEADER))
    else {
        return false;
    };
    let mut lines = body.lines();
    let mut count = 0;
    while let Some(folder) = lines.next() {
        count += 1;
        let Some(folder) = folder.strip_prefix(&format!("    {count}. ")) else {
            return false;
        };
        let Some(guidance) = lines.next().and_then(|line| line.strip_prefix(GUIDANCE)) else {
            return false;
        };
        let Some((project, config)) = guidance.split_once(" as a trusted project in ") else {
            return false;
        };
        let folder = folder.replace('\\', "/");
        let project = project.replace('\\', "/");
        let project = project.trim_end_matches('/');
        if !folder.strip_suffix("/.codex").is_some_and(|parent| {
            if cfg!(windows) {
                parent.eq_ignore_ascii_case(project)
            } else {
                parent == project
            }
        }) || project.is_empty()
            || !config.replace('\\', "/").ends_with("/config.toml.")
        {
            return false;
        }
    }
    count > 0
}

pub(super) fn isolated_config(
    effective: &Value,
    catalog: &Value,
    cwd: &std::path::Path,
) -> Result<Value, LlmError> {
    let entries = catalog["data"]
        .as_array()
        .filter(|entries| entries.len() == 1)
        .ok_or_else(|| capability("skills/list 未返回唯一任务目录，无法隔离技能"))?;
    let entry = &entries[0];
    let identity = |path: &std::path::Path| {
        let path = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
        let text =
            forge_util::pathutil::strip_verbatim_prefix(&path.to_string_lossy()).replace('\\', "/");
        if cfg!(windows) {
            text.to_lowercase()
        } else {
            text
        }
    };
    if entry["cwd"]
        .as_str()
        .map(|path| identity(std::path::Path::new(path)))
        != Some(identity(cwd))
    {
        return Err(capability("skills/list 返回了其他目录，无法隔离技能"));
    }
    if !entry["errors"].as_array().is_some_and(Vec::is_empty) {
        return Err(capability("skills/list 扫描不完整，无法隔离技能"));
    }
    let skills = entry["skills"]
        .as_array()
        .ok_or_else(|| capability("skills/list 缺少技能列表"))?;
    let mut paths = BTreeSet::new();
    for skill in skills {
        let path = skill["path"]
            .as_str()
            .filter(|path| std::path::Path::new(path).is_absolute())
            .ok_or_else(|| capability("skills/list 技能缺少绝对路径，无法禁用"))?;
        paths.insert(path.to_string());
    }
    if let Some(configured) = effective
        .pointer("/skills/config")
        .and_then(Value::as_array)
    {
        for skill in configured {
            let path = skill["path"]
                .as_str()
                .ok_or_else(|| capability("已配置技能缺少路径，无法禁用"))?;
            paths.insert(path.to_string());
        }
    }
    let mut config = json!({
        "model_reasoning_summary": "auto",
        "agents": { "enabled": false },
        "features": { "multi_agent": false, "shell_tool": false, "unified_exec": false, "hooks": false, "apps": false, "image_generation": false },
        "skills": { "config": paths.into_iter().map(|path| json!({"path": path, "enabled": false})).collect::<Vec<_>>() },
        "web_search": "disabled",
        "apps": { "_default": { "enabled": false } },
        "mcp_servers": {},
        "plugins": {},
    });
    for key in ["mcp_servers", "plugins", "apps"] {
        if let Some(entries) = effective.get(key).and_then(Value::as_object) {
            for name in entries.keys() {
                config[key][name] = json!({ "enabled": false });
            }
        }
    }
    Ok(config)
}

fn thread_params(
    spec: &AgentJobSpec,
    messages: &[Value],
    tools: Vec<Value>,
    isolation: Value,
) -> Value {
    let instructions = messages
        .iter()
        .filter(|m| matches!(m["role"].as_str(), Some("system" | "developer")))
        .filter_map(|m| m["content"].as_str())
        .collect::<Vec<_>>()
        .join("\n\n");
    let grant = write_grant(spec, tools.iter().filter_map(|t| t["name"].as_str()));
    let grant_block = grant.map(|g| format!("\n\n{g}")).unwrap_or_default();
    let mut params = json!({
        "cwd": spec.cwd.to_string_lossy(), "approvalPolicy": "never", "sandbox": "read-only",
        "developerInstructions": format!("{instructions}\n\n你正在执行 Forge 受管任务。原生 read-only 沙箱限制 Codex 自身的 shell 和文件编辑，不把 Forge 宿主任务改成只读。所有读取、写入、引擎操作与委派只使用声明的 forge__ 动态工具；这些工具由 Forge 宿主执行，写入受当前任务范围、角色白名单和 Forge 会话权限服务约束。正式制作中，已声明且获宿主授权的 Forge 写工具可正常调用；计划或只读任务仍不得写入。不得仅因原生 read-only 拒绝获准的宿主写入，也不得把工具存在当作额外授权、假造未声明工具或绕过宿主拒绝。原生 shell、文件编辑、MCP、子代理与 Goal 均不可使用。阶段仅由宿主出口工具推进。{grant_block}"),
        "config": isolation, "dynamicTools": tools,
    });
    if let Some(model) = model_id(spec) {
        params["model"] = json!(model);
    }
    params
}

fn model_id(spec: &AgentJobSpec) -> Option<&str> {
    spec.model
        .as_deref()
        .map(str::trim)
        .map(|model| model.strip_prefix("codex:").unwrap_or(model))
        .filter(|model| !model.is_empty())
}

/// Codex tells the model its native sandbox ("sandbox_mode=read-only, approval_policy=never").
/// Production members that only saw that line concluded the whole task was read-only and never
/// called the host write tools they had been granted (UltraPlan production stalled with no files
/// written). When the host granted a write scope, spell the grant out with the concrete roots and
/// the granted host write tools; the host still enforces scope and permissions on every call.
fn write_grant<'a>(spec: &AgentJobSpec, remote_names: impl IntoIterator<Item = &'a str>) -> Option<String> {
    if spec.metadata.write_scope.is_empty() {
        return None;
    }
    let mut writes: Vec<&str> = remote_names
        .into_iter()
        .filter(|name| name.strip_prefix("forge__").is_some_and(crate::agent::is_write_tool))
        .collect();
    if writes.is_empty() {
        return None;
    }
    writes.sort_unstable();
    writes.dedup();
    let roots = spec
        .metadata
        .write_scope
        .iter()
        .map(|p| p.trim_start_matches(r"\\?\"))
        .collect::<Vec<_>>()
        .join("、");
    Some(format!(
        "【写入授权】本任务已获 Forge 宿主写入授权。可写范围：{roots}。已授权写工具：{}。Codex 环境信息中的 sandbox_mode=read-only / approval_policy=never 只约束原生 shell 与原生文件编辑，不代表本任务只读；需要创建或修改文件、场景、资产时，直接调用上述 forge__ 写工具，宿主仍会逐次校验范围与权限。只有当这些工具实际返回拒绝或错误时，才把它作为阻塞汇报，并附上原始错误。",
        writes.join("、")
    ))
}

fn initial_input(messages: &[Value]) -> Vec<Value> {
    let mut input = Vec::new();
    for m in messages
        .iter()
        .filter(|m| !matches!(m["role"].as_str(), Some("system" | "developer")))
    {
        match &m["content"] {
            Value::String(text) => input.push(json!({ "type": "text", "text": format!("[{}]\n{}", m["role"].as_str().unwrap_or("context"), text) })),
            Value::Array(blocks) => {
                for block in blocks {
                    if let Some(url) = block.pointer("/image_url/url").and_then(Value::as_str) {
                        input.push(json!({ "type": "image", "url": url }));
                    } else if let Some(text) = block.get("text").and_then(Value::as_str) {
                        input.push(json!({ "type": "text", "text": text }));
                    }
                }
            }
            _ => input.push(json!({ "type": "text", "text": m.to_string() })),
        }
    }
    if input.is_empty() {
        input.push(json!({ "type": "text", "text": "继续执行当前受管任务。" }));
    }
    input
}

fn continuation_input(messages: &[Value], completed: &[Value]) -> Vec<Value> {
    // The common loop can continue after a text completion when mail arrived at
    // that boundary. Its transcript already exists in this native thread; append
    // only the newly arrived messages. A fresh task carries a fresh input vector.
    let new_messages = if !completed.is_empty() && messages.starts_with(completed) {
        &messages[completed.len()..]
    } else {
        messages
    };
    initial_input(new_messages)
}

#[cfg(test)]
mod tests {
    use super::super::rpc::ScriptedTransport;
    use super::*;
    use crate::llm::{ToolFeedback, ToolLoopCfg};
    use std::sync::atomic::AtomicUsize;

    fn tool() -> Value {
        json!({ "type": "function", "function": { "name": "test_probe", "description": "test", "parameters": { "type": "object", "properties": {} } } })
    }

    fn spec(id: &str) -> AgentJobSpec {
        AgentJobSpec::new(id, std::env::temp_dir())
    }

    #[test]
    fn native_read_only_does_not_hide_host_write_authority() {
        let mut write = tool();
        write["function"]["name"] = json!("write_file");
        let (tools, _) = declarations(&[write]).unwrap();
        let isolation = json!({"features":{"shell_tool":false,"unified_exec":false}});
        let params = thread_params(&spec("production"), &[], tools, isolation.clone());
        assert_eq!(params["sandbox"], "read-only");
        assert_eq!(params["approvalPolicy"], "never");
        assert_eq!(params["config"], isolation);
        assert_eq!(params["dynamicTools"][0]["name"], "forge__write_file");
        let instructions = params["developerInstructions"].as_str().unwrap();
        assert!(instructions.contains("不把 Forge 宿主任务改成只读"));
        assert!(instructions.contains("计划或只读任务仍不得写入"));
        assert!(instructions.contains("不得把工具存在当作额外授权"));
        // No write scope on this spec: no explicit grant is added.
        assert!(!instructions.contains("【写入授权】"));
    }

    #[test]
    fn write_scoped_job_gets_explicit_host_write_grant() {
        let mut read = tool();
        read["function"]["name"] = json!("read_file");
        let mut write = tool();
        write["function"]["name"] = json!("write_file");
        let mut apply = tool();
        apply["function"]["name"] = json!("editor_apply");
        let (tools, mapping) = declarations(&[read, write, apply]).unwrap();
        let mut spec = spec("production");
        spec.metadata.phase = Some("production".into());
        spec.metadata.role = Some("logic-programmer".into());
        spec.metadata.write_scope = vec![r"\\?\E:\work\project".into()];

        let params = thread_params(&spec, &[], tools, json!({}));
        // The native sandbox stays read-only; only the host grant is spelled out.
        assert_eq!(params["sandbox"], "read-only");
        let instructions = params["developerInstructions"].as_str().unwrap();
        assert!(instructions.contains("【写入授权】"));
        assert!(instructions.contains(r"E:\work\project"));
        assert!(!instructions.contains(r"\\?\"));
        assert!(instructions.contains("forge__write_file"));
        assert!(instructions.contains("forge__editor_apply"));
        assert!(!instructions.contains("forge__read_file、"));

        let grant = write_grant(&spec, mapping.keys().map(String::as_str)).unwrap();
        assert!(grant.contains("sandbox_mode=read-only"));
        assert!(grant.contains("forge__write_file"));
    }

    #[test]
    fn write_grant_needs_both_scope_and_write_tools() {
        let mut read = tool();
        read["function"]["name"] = json!("read_file");
        let (read_only_tools, _) = declarations(&[read]).unwrap();
        let mut scoped = spec("qa");
        scoped.metadata.write_scope = vec![r"E:\work\project".into()];
        // QA members get a write scope but only read tools: no grant.
        assert!(write_grant(&scoped, read_only_tools.iter().filter_map(|t| t["name"].as_str())).is_none());
        // Write tools without a write scope (plan / read-only tasks): no grant.
        assert!(write_grant(&spec("plan"), ["forge__write_file"]).is_none());
    }

    fn flow(transport: Arc<ScriptedTransport>) -> Arc<ManagedCodexFlow> {
        ManagedCodexFlow::from_client(Arc::new(CodexClient::new(transport)))
    }

    fn base_reply(message: &Value) -> Option<Vec<Value>> {
        match message["method"].as_str() {
            Some("initialize") => Some(vec![
                json!({ "id": message["id"], "result": { "version": "test" } }),
            ]),
            Some("initialized") => Some(vec![]),
            Some("config/read") => Some(vec![json!({ "id": message["id"], "result": { "config": {
                "mcp_servers": { "unsafe_server": { "command": "must-not-start" } },
                "plugins": { "test-plugin": { "enabled": true } },
                "apps": { "test-app": { "enabled": true } }
            } } })]),
            Some("skills/list") => Some(vec![json!({ "id": message["id"], "result": { "data": [{
                "cwd": message["params"]["cwds"][0], "skills": [], "errors": []
            }] } })]),
            Some("turn/interrupt") => Some(vec![
                json!({ "id": message["id"], "result": {} }),
                json!({ "method": "turn/completed", "params": { "threadId": message["params"]["threadId"], "turn": { "id": message["params"]["turnId"], "status": "interrupted" } } }),
            ]),
            _ => None,
        }
    }

    #[test]
    fn declarations_preserve_host_names_and_reject_collisions() {
        let (out, mapping) = declarations(&[tool()]).unwrap();
        assert_eq!(out[0]["name"], "forge__test_probe");
        assert_eq!(mapping["forge__test_probe"], "test_probe");
        let mut a = tool();
        let mut b = tool();
        a["function"]["name"] = json!("one.two");
        b["function"]["name"] = json!("one_two");
        assert!(declarations(&[a, b]).is_err());
    }

    #[test]
    fn completed_transcript_is_not_replayed_for_boundary_message() {
        let completed = vec![
            json!({"role":"user","content":"first"}),
            json!({"role":"assistant","content":"done"}),
        ];
        let mut messages = completed.clone();
        messages.push(json!({"role":"user","content":"new direction"}));
        let delta = continuation_input(&messages, &completed);
        assert_eq!(delta.len(), 1);
        assert!(delta[0]["text"].as_str().unwrap().contains("new direction"));
        assert!(!json!(delta).to_string().contains("first"));
        assert_eq!(
            continuation_input(
                &[json!({"role":"user","content":"separate task"})],
                &completed
            )
            .len(),
            1
        );
    }

    #[test]
    fn reasoning_stream_has_item_lifecycle_and_completed_snapshot_fallback() {
        let flow = flow(Arc::new(ScriptedTransport::new(|_| Vec::new())));
        let mut bridge = Bridge::new(flow.runtime.clone(), spec("reasoning-lifecycle"));
        let seen = Arc::new(std::sync::Mutex::new(Vec::<StreamDelta>::new()));
        let captured = seen.clone();
        let sink: StreamSink = Arc::new(move |event| captured.lock().unwrap().push(event));
        for (method, params) in [
            (
                "item/started",
                json!({ "item": { "id": "r1", "type": "reasoning" } }),
            ),
            (
                "item/reasoning/summaryTextDelta",
                json!({ "itemId": "r1", "delta": "Checking" }),
            ),
            (
                "item/completed",
                json!({ "item": { "id": "r1", "type": "reasoning", "summary": ["Checking the scene"] } }),
            ),
            // Duplicate lifecycle notifications must not replay a snapshot.
            (
                "item/completed",
                json!({ "item": { "id": "r1", "type": "reasoning", "summary": ["Checking the scene"] } }),
            ),
            // Older servers can return the summary only at completion.
            (
                "item/completed",
                json!({ "item": { "id": "r2", "type": "reasoning", "summaryText": "Recovered" } }),
            ),
        ] {
            assert!(bridge
                .notification(method, &params, Some(&sink))
                .unwrap()
                .is_none());
        }
        let events = seen.lock().unwrap();
        assert_eq!(events.len(), 5);
        assert!(matches!(&events[0], StreamDelta::ReasoningStarted { item_id } if item_id == "r1"));
        assert!(
            matches!(&events[1], StreamDelta::ReasoningDelta { item_id, delta } if item_id == "r1" && delta == "Checking")
        );
        assert!(
            matches!(&events[2], StreamDelta::ReasoningCompleted { item_id, text } if item_id == "r1" && text == "Checking the scene")
        );
        assert!(matches!(&events[3], StreamDelta::ReasoningStarted { item_id } if item_id == "r2"));
        assert!(
            matches!(&events[4], StreamDelta::ReasoningCompleted { item_id, text } if item_id == "r2" && text == "Recovered")
        );
    }

    #[tokio::test]
    async fn persistent_member_reuses_one_thread_with_incremental_task_input() {
        let seen = Arc::new(std::sync::Mutex::new(Vec::<Value>::new()));
        let sink = seen.clone();
        let turns = Arc::new(AtomicUsize::new(0));
        let count = turns.clone();
        let transport = Arc::new(ScriptedTransport::new(move |m| {
            sink.lock().unwrap().push(m.clone());
            if let Some(reply) = base_reply(m) {
                return reply;
            }
            match m["method"].as_str() {
                Some("thread/start") => {
                    vec![json!({"id":m["id"],"result":{"thread":{"id":"member-thread"}}})]
                }
                Some("turn/start") => {
                    let turn = format!("member-turn-{}", count.fetch_add(1, Ordering::SeqCst));
                    vec![
                        json!({"id":m["id"],"result":{"turn":{"id":turn}}}),
                        json!({"method":"item/completed","params":{"threadId":"member-thread","turnId":turn,"item":{"type":"agentMessage","text":"done"}}}),
                        json!({"method":"turn/completed","params":{"threadId":"member-thread","turn":{"id":turn,"status":"completed"}}}),
                    ]
                }
                _ => vec![],
            }
        }));
        let flow = flow(transport);
        for task in ["first task", "second task"] {
            let step = flow.participant_step(spec("persistent-member"));
            assert_eq!(
                step(
                    vec![json!({"role":"user","content":task})],
                    vec![tool()],
                    None
                )
                .await
                .unwrap()
                .message["content"],
                "done"
            );
        }
        let seen = seen.lock().unwrap();
        assert_eq!(
            seen.iter()
                .filter(|m| m["method"] == "thread/start")
                .count(),
            1
        );
        let starts: Vec<_> = seen
            .iter()
            .filter(|m| m["method"] == "turn/start")
            .collect();
        assert_eq!(starts.len(), 2);
        assert!(starts
            .iter()
            .all(|start| start["params"]["summary"] == "auto"));
        let thread = seen
            .iter()
            .find(|message| message["method"] == "thread/start")
            .unwrap();
        assert_eq!(
            thread["params"]["config"]["model_reasoning_summary"],
            "auto"
        );
        assert_eq!(starts[1]["params"]["threadId"], "member-thread");
        assert!(starts[1]["params"]["input"]
            .to_string()
            .contains("second task"));
        assert!(!starts[1]["params"]["input"]
            .to_string()
            .contains("first task"));
    }

    #[test]
    fn isolation_disables_discovered_skills_with_supported_config() {
        let cwd = std::env::temp_dir();
        let skill = cwd.join("test-skill/SKILL.md");
        let configured = cwd.join("configured-skill/SKILL.md");
        let effective = json!({"skills": {"config": [{"path": configured, "enabled": true}]}});
        let catalog = json!({"data": [{"cwd": cwd, "skills": [{"path": skill, "enabled": true}], "errors": []}]});
        let isolated = isolated_config(&effective, &catalog, &cwd).unwrap();
        assert!(isolated["features"].get("skills").is_none());
        assert!(isolated["features"].get("codex_hooks").is_none());
        assert_eq!(isolated["features"]["hooks"], false);
        let entries = isolated["skills"]["config"].as_array().unwrap();
        assert_eq!(entries.len(), 2);
        assert!(entries.iter().all(|entry| entry["enabled"] == false));
        assert!(entries.iter().any(|entry| entry["path"] == json!(skill)));
        assert!(entries
            .iter()
            .any(|entry| entry["path"] == json!(configured)));
        let mut broken = catalog.clone();
        broken["data"][0]["errors"] = json!([{"message":"unreadable skill"}]);
        assert!(isolated_config(&effective, &broken, &cwd).is_err());
        broken["data"][0]["errors"] = json!([]);
        broken["data"][0]["skills"][0]["path"] = Value::Null;
        assert!(isolated_config(&effective, &broken, &cwd).is_err());
    }

    #[tokio::test]
    async fn disabled_project_config_warning_keeps_managed_isolation() {
        let seen = Arc::new(std::sync::Mutex::new(Vec::<Value>::new()));
        let captured = seen.clone();
        let cwd = std::env::temp_dir();
        let skill = cwd.join("disabled-project-skill/SKILL.md");
        let expected_skill = skill.clone();
        let summary = format!(
            "Project-local config, hooks, and exec policies are disabled in the following folders until the project is trusted, but skills still load.\n    1. {}\n       To load project-local config, hooks, and exec policies, add {} as a trusted project in {}.\n",
            cwd.join(".codex").display(), cwd.display(), cwd.join("managed-home/config.toml").display()
        );
        let warning = json!({"method":"configWarning","params":{"details":null,"summary":summary}});
        let transport = Arc::new(ScriptedTransport::new(move |m| {
            captured.lock().unwrap().push(m.clone());
            if m["method"] == "skills/list" {
                return vec![json!({"id":m["id"],"result":{"data":[{
                    "cwd":m["params"]["cwds"][0],"skills":[{"path":skill,"enabled":true}],"errors":[]
                }]}})];
            }
            if let Some(mut replies) = base_reply(m) {
                if m["method"] == "config/read" {
                    replies.insert(0, warning.clone());
                }
                return replies;
            }
            match m["method"].as_str() {
                Some("thread/start") => {
                    vec![json!({"id":m["id"],"result":{"thread":{"id":"isolated-thread"}}})]
                }
                Some("turn/start") => vec![
                    warning.clone(),
                    json!({"id":m["id"],"result":{"turn":{"id":"isolated-turn"}}}),
                    json!({"method":"item/completed","params":{"threadId":"isolated-thread","turnId":"isolated-turn","item":{"type":"agentMessage","text":"done"}}}),
                    json!({"method":"turn/completed","params":{"threadId":"isolated-thread","turn":{"id":"isolated-turn","status":"completed"}}}),
                ],
                _ => vec![],
            }
        }));
        let flow = flow(transport);
        let step = flow.step(spec("disabled-project-config"));
        let result = step(
            vec![json!({"role":"user","content":"test"})],
            vec![tool()],
            None,
        )
        .await
        .unwrap();
        assert_eq!(result.message["content"], "done");
        assert!(!flow.runtime.closed.load(Ordering::Acquire));
        let seen = seen.lock().unwrap();
        let thread = &seen.iter().find(|m| m["method"] == "thread/start").unwrap()["params"];
        assert_eq!(thread["sandbox"], "read-only");
        assert_eq!(thread["approvalPolicy"], "never");
        for feature in [
            "shell_tool",
            "unified_exec",
            "hooks",
            "multi_agent",
            "apps",
            "image_generation",
        ] {
            assert_eq!(thread["config"]["features"][feature], false);
        }
        assert_eq!(
            thread["config"]["skills"]["config"],
            json!([{"path":expected_skill,"enabled":false}])
        );
        assert_eq!(
            thread["config"]["mcp_servers"]["unsafe_server"]["enabled"],
            false
        );
        assert_eq!(thread["config"]["plugins"]["test-plugin"]["enabled"], false);
        assert_eq!(thread["config"]["apps"]["test-app"]["enabled"], false);
        let turn = &seen.iter().find(|m| m["method"] == "turn/start").unwrap()["params"];
        assert_eq!(turn["sandboxPolicy"]["type"], "readOnly");
        assert_eq!(turn["approvalPolicy"], "never");
        assert!(!seen.iter().any(|m| m["method"]
            .as_str()
            .is_some_and(|method| method.starts_with("config/") && method != "config/read")));
    }

    #[tokio::test]
    async fn ignored_isolation_config_stops_before_model_turn() {
        for notification in [
            json!({"method":"configWarning","params":{"message":"isolation setting unrecognized"}}),
            json!({"method":"configWarning","params":{"details":null,"summary":"isolation setting unrecognized"}}),
            json!({"method":"configWarning","params":{"details":"required isolation override rejected","summary":"Project-local config, hooks, and exec policies are disabled in the following folders until the project is trusted, but skills still load.\n    1. /project/.codex\n       To load project-local config, hooks, and exec policies, add /project as a trusted project in /managed/config.toml.\n"}}),
            json!({"method":"skills/changed","params":{}}),
        ] {
            let turns = Arc::new(AtomicUsize::new(0));
            let count = turns.clone();
            let transport = Arc::new(ScriptedTransport::new(move |m| {
                if let Some(r) = base_reply(m) {
                    return r;
                }
                if m["method"] == "thread/start" {
                    return vec![
                        notification.clone(),
                        json!({"id":m["id"],"result":{"thread":{"id":"must-not-run"}}}),
                    ];
                }
                if m["method"] == "turn/start" {
                    count.fetch_add(1, Ordering::SeqCst);
                }
                vec![]
            }));
            let flow = flow(transport);
            let step = flow.step(spec("ignored-config"));
            let error = step(
                vec![json!({"role":"user","content":"test"})],
                vec![tool()],
                None,
            )
            .await
            .err()
            .unwrap();
            assert!(error.to_string().starts_with(CAPABILITY_ERROR));
            assert!(flow.runtime.closed.load(Ordering::Acquire));
            assert_eq!(turns.load(Ordering::SeqCst), 0);
        }
    }

    #[tokio::test]
    async fn managed_loop_keeps_tool_failure_and_image_evidence() {
        use base64::Engine;
        let mut png = std::io::Cursor::new(Vec::new());
        image::DynamicImage::ImageRgba8(image::RgbaImage::from_pixel(
            2,
            2,
            image::Rgba([12, 34, 56, 255]),
        ))
        .write_to(&mut png, image::ImageFormat::Png)
        .unwrap();
        let image_url = format!(
            "data:image/png;base64,{}",
            base64::engine::general_purpose::STANDARD.encode(png.into_inner())
        );
        let expected_image = image_url.clone();
        let seen = Arc::new(std::sync::Mutex::new(Vec::<Value>::new()));
        let captured = seen.clone();
        let transport = Arc::new(ScriptedTransport::new(move |m| {
            captured.lock().unwrap().push(m.clone());
            if let Some(r) = base_reply(m) {
                return r;
            }
            match m["method"].as_str() {
                Some("thread/start") => {
                    assert_eq!(m["params"]["sandbox"], "read-only");
                    assert_eq!(
                        m["params"]["config"]["mcp_servers"]["unsafe_server"]["enabled"],
                        false
                    );
                    assert_eq!(
                        m["params"]["config"]["plugins"]["test-plugin"]["enabled"],
                        false
                    );
                    assert_eq!(m["params"]["config"]["apps"]["test-app"]["enabled"], false);
                    assert_eq!(m["params"]["config"]["features"]["shell_tool"], false);
                    vec![json!({ "id": m["id"], "result": { "thread": { "id": "thread-one" } } })]
                }
                Some("turn/start") => vec![
                    // This request deliberately precedes turn/start's reply.
                    json!({ "id": "probe-request", "method": "item/tool/call", "params": { "threadId": "thread-one", "turnId": "turn-one", "tool": "forge__test_probe", "arguments": {} } }),
                    json!({ "id": m["id"], "result": { "turn": { "id": "turn-one" } } }),
                ],
                None if m["id"] == "probe-request" => {
                    assert_eq!(m["result"]["success"], false);
                    assert!(m["result"]["contentItems"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .any(|item| item["text"] == "Team task returned: visual audit passed."));
                    let image = m["result"]["contentItems"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .find(|item| item["type"] == "inputImage")
                        .expect("real screenshot must reach Codex");
                    assert_eq!(image["imageUrl"], expected_image);
                    let bytes = base64::engine::general_purpose::STANDARD
                        .decode(
                            image["imageUrl"]
                                .as_str()
                                .unwrap()
                                .split_once(',')
                                .unwrap()
                                .1,
                        )
                        .unwrap();
                    assert_eq!(
                        image::load_from_memory(&bytes)
                            .unwrap()
                            .to_rgba8()
                            .get_pixel(0, 0),
                        &image::Rgba([12, 34, 56, 255])
                    );
                    vec![
                        json!({ "method": "item/completed", "params": { "threadId": "thread-one", "turnId": "turn-one", "item": { "type": "agentMessage", "text": "Probe failed, evidence preserved." } } }),
                        json!({ "method": "turn/completed", "params": { "threadId": "thread-one", "turn": { "id": "turn-one", "status": "completed" } } }),
                    ]
                }
                _ => vec![],
            }
        }));
        let flow = flow(transport);
        let step = flow.step(spec("job-one"));
        let execute = move |name: String, _: Value| -> crate::llm::BoxFut<(bool, ToolFeedback)> {
            assert_eq!(name, "test_probe");
            let image_url = image_url.clone();
            Box::pin(async move {
                (
                    false,
                    ToolFeedback {
                        text: "Actual probe failed".to_string(),
                        images: vec![image_url],
                    },
                )
            })
        };
        let inbox_reads = AtomicUsize::new(0);
        let inbox = || {
            if inbox_reads.fetch_add(1, Ordering::SeqCst) == 1 {
                Some("Team task returned: visual audit passed.".to_string())
            } else {
                None
            }
        };
        let out = crate::llm::run_managed_tool_loop(
            "system",
            "test",
            ToolLoopCfg {
                tools: vec![tool()],
                step: step.as_ref(),
                execute: &execute,
                vision: true,
                sink: None,
                forbidden: None,
                cancelled: None,
                stream: None,
                preamble: None,
                inbox: Some(&inbox),
                history: Vec::new(),
                max_iters: Some(3),
            },
        )
        .await
        .unwrap();
        assert_eq!(out.text, "Probe failed, evidence preserved.");
        assert_eq!(out.records.len(), 1);
        assert!(!out.records[0].ok);
        assert_eq!(
            seen.lock()
                .unwrap()
                .iter()
                .filter(|m| m["method"] == "thread/start")
                .count(),
            1
        );
    }

    #[tokio::test]
    async fn concurrent_jobs_have_distinct_threads_and_ignore_other_turns() {
        let threads = Arc::new(AtomicUsize::new(0));
        let count = threads.clone();
        let transport = Arc::new(ScriptedTransport::new(move |m| {
            if let Some(r) = base_reply(m) {
                return r;
            }
            match m["method"].as_str() {
                Some("thread/start") => {
                    let id = count.fetch_add(1, Ordering::SeqCst) + 1;
                    vec![
                        json!({ "id": m["id"], "result": { "thread": { "id": format!("thread-{id}") } } }),
                    ]
                }
                Some("turn/start") => {
                    let thread = m["params"]["threadId"].as_str().unwrap();
                    let turn = format!("turn-{thread}");
                    vec![
                        json!({ "id": m["id"], "result": { "turn": { "id": turn } } }),
                        json!({ "method": "item/completed", "params": { "threadId": thread, "turnId": "old-turn", "item": { "type": "agentMessage", "text": "MUST IGNORE" } } }),
                        json!({ "method": "item/completed", "params": { "threadId": thread, "turnId": turn, "item": { "type": "agentMessage", "text": thread } } }),
                        json!({ "method": "turn/completed", "params": { "threadId": thread, "turn": { "id": turn, "status": "completed" } } }),
                    ]
                }
                _ => vec![],
            }
        }));
        let flow = flow(transport);
        let a = flow.step(spec("a"));
        let b = flow.step(spec("b"));
        let input = vec![json!({ "role": "user", "content": "test" })];
        let (a, b) = tokio::join!(
            a(input.clone(), vec![tool()], None),
            b(input, vec![tool()], None)
        );
        let a = a.unwrap().message["content"].as_str().unwrap().to_string();
        let b = b.unwrap().message["content"].as_str().unwrap().to_string();
        assert_ne!(a, b);
        assert!(a.starts_with("thread-") && b.starts_with("thread-"));
        assert_eq!(threads.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn unsupported_dynamic_tools_fail_without_removing_capabilities() {
        let starts = Arc::new(AtomicUsize::new(0));
        let count = starts.clone();
        let transport = Arc::new(ScriptedTransport::new(move |m| {
            if let Some(r) = base_reply(m) {
                return r;
            }
            if m["method"] == "thread/start" {
                count.fetch_add(1, Ordering::SeqCst);
                assert!(m["params"]["dynamicTools"].is_array());
                return vec![
                    json!({ "id": m["id"], "error": { "code": -32602, "message": "unknown field dynamicTools" } }),
                ];
            }
            vec![]
        }));
        let flow = flow(transport);
        let step = flow.step(spec("missing-capability"));
        let result = step(
            vec![json!({ "role": "user", "content": "test" })],
            vec![tool()],
            None,
        )
        .await;
        assert!(result
            .err()
            .unwrap()
            .to_string()
            .starts_with(CAPABILITY_ERROR));
        assert_eq!(starts.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn cancellation_interrupts_only_the_flow_client() {
        let interrupted = Arc::new(AtomicBool::new(false));
        let recorded = interrupted.clone();
        let transport = Arc::new(ScriptedTransport::new(move |m| {
            if m["method"] == "turn/interrupt" {
                recorded.store(true, Ordering::SeqCst);
            }
            if let Some(r) = base_reply(m) {
                return r;
            }
            match m["method"].as_str() {
                Some("thread/start") => vec![
                    json!({ "id": m["id"], "result": { "thread": { "id": "cancel-thread" } } }),
                ],
                Some("turn/start") => {
                    vec![json!({ "id": m["id"], "result": { "turn": { "id": "cancel-turn" } } })]
                }
                _ => vec![],
            }
        }));
        let flow = flow(transport);
        let cancelled = Arc::new(AtomicBool::new(false));
        let check = cancelled.clone();
        let mut job = spec("cancel-job");
        job.cancelled = Arc::new(move || check.load(Ordering::SeqCst));
        let step = flow.step(job);
        let future = tokio::spawn(async move {
            step(
                vec![json!({ "role": "user", "content": "test" })],
                vec![tool()],
                None,
            )
            .await
        });
        tokio::time::sleep(Duration::from_millis(30)).await;
        cancelled.store(true, Ordering::SeqCst);
        let result = tokio::time::timeout(Duration::from_secs(2), future)
            .await
            .unwrap()
            .unwrap();
        assert!(result.err().unwrap().to_string().contains(CANCELLED));
        assert!(interrupted.load(Ordering::SeqCst));
    }

    #[tokio::test]
    async fn completed_job_journal_is_reused_without_another_turn() {
        let dir = std::env::temp_dir().join(format!(
            "forge-managed-journal-{}",
            crate::events::new_id("t")
        ));
        let mut job = spec("persisted-job");
        job.resume_completed = true;
        job.state_dir = Some(dir.clone());
        job.metadata.flow_id = Some("flow-one".into());
        job.metadata.revision = Some(4);
        let transport = Arc::new(ScriptedTransport::new(|m| {
            if let Some(r) = base_reply(m) {
                return r;
            }
            match m["method"].as_str() {
                Some("thread/start") => vec![
                    json!({ "id": m["id"], "result": { "thread": { "id": "persisted-thread" } } }),
                ],
                Some("turn/start") => vec![
                    json!({ "id": m["id"], "result": { "turn": { "id": "persisted-turn" } } }),
                    json!({ "method": "item/completed", "params": { "threadId": "persisted-thread", "turnId": "persisted-turn", "item": { "type": "agentMessage", "text": "Saved result" } } }),
                    json!({ "method": "turn/completed", "params": { "threadId": "persisted-thread", "turn": { "id": "persisted-turn", "status": "completed" } } }),
                ],
                _ => vec![],
            }
        }));
        let first_flow = flow(transport);
        let first = first_flow.step(job.clone());
        let input = vec![json!({ "role": "user", "content": "test" })];
        assert_eq!(
            first(input.clone(), vec![tool()], None)
                .await
                .unwrap()
                .message["content"],
            "Saved result"
        );
        drop(first);
        drop(first_flow);
        let path = crate::agent_job::journal_path(&job).unwrap();
        let saved: AgentJobRecord =
            serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
        assert_eq!(saved.thread_id.as_deref(), Some("persisted-thread"));
        assert_eq!(saved.turn_id.as_deref(), Some("persisted-turn"));
        assert_eq!(saved.result.status, AgentJobStatus::Completed);
        let recovered_flow = flow(Arc::new(ScriptedTransport::new(|_| {
            panic!("completed job must not start a Codex request")
        })));
        let resumed = recovered_flow.step(job.clone());
        assert_eq!(
            resumed(input, vec![tool()], None).await.unwrap().message["content"],
            "Saved result"
        );
        drop(resumed);
        job.resume_completed = false;
        let fresh_flow = flow(Arc::new(ScriptedTransport::new(|m| {
            if let Some(r) = base_reply(m) {
                return r;
            }
            match m["method"].as_str() {
                Some("thread/resume") => vec![
                    json!({ "id": m["id"], "result": { "thread": { "id": "persisted-thread" } } }),
                ],
                Some("turn/start") => vec![
                    json!({ "id": m["id"], "result": { "turn": { "id": "fresh-turn" } } }),
                    json!({ "method": "item/completed", "params": { "threadId": "persisted-thread", "turnId": "fresh-turn", "item": { "type": "agentMessage", "text": "Fresh QA result" } } }),
                    json!({ "method": "turn/completed", "params": { "threadId": "persisted-thread", "turn": { "id": "fresh-turn", "status": "completed" } } }),
                ],
                Some("thread/start") => panic!("reuse the original job thread"),
                _ => vec![],
            }
        })));
        let fresh_step = fresh_flow.step(job.clone());
        assert_eq!(
            fresh_step(
                vec![json!({ "role": "user", "content": "Run QA again after repairs" })],
                vec![tool()],
                None
            )
            .await
            .unwrap()
            .message["content"],
            "Fresh QA result"
        );
        let saved: AgentJobRecord = serde_json::from_str(
            &std::fs::read_to_string(crate::agent_job::journal_path(&job).unwrap()).unwrap(),
        )
        .unwrap();
        assert_eq!(saved.generation, 2);
        assert_eq!(saved.turn_id.as_deref(), Some("fresh-turn"));
        drop(fresh_step);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[tokio::test]
    async fn running_job_recovers_original_completed_turn_without_replaying_tools() {
        let dir = std::env::temp_dir().join(format!(
            "forge-managed-recover-{}",
            crate::events::new_id("t")
        ));
        let mut job = spec("interrupted-host-job");
        job.state_dir = Some(dir.clone());
        let (tools, _) = declarations(&[tool()]).unwrap();
        let mut record =
            AgentJobRecord::starting(&job, "codex", serde_json::to_string(&tools).unwrap());
        record.thread_id = Some("old-thread".into());
        record.turn_id = Some("old-turn".into());
        record.result.status = AgentJobStatus::Running;
        crate::sessions::write_atomic(
            &crate::agent_job::journal_path(&job).unwrap(),
            &serde_json::to_string(&record).unwrap(),
        )
        .unwrap();
        let transport = Arc::new(ScriptedTransport::new(|m| {
            if let Some(r) = base_reply(m) {
                return r;
            }
            match m["method"].as_str() {
                Some("thread/resume") => {
                    assert_eq!(m["params"]["threadId"], "old-thread");
                    assert!(m["params"].get("dynamicTools").is_none());
                    assert_eq!(m["params"]["sandbox"], "read-only");
                    vec![json!({ "id": m["id"], "result": { "thread": { "id": "old-thread" } } })]
                }
                Some("thread/read") => vec![
                    json!({ "id": m["id"], "result": { "thread": { "id": "old-thread", "turns": [{ "id": "old-turn", "status": "completed", "items": [{ "type": "agentMessage", "text": "Recovered result" }] }] } } }),
                ],
                Some("thread/start" | "turn/start") => {
                    panic!("recovery must not create another thread or turn")
                }
                _ => vec![],
            }
        }));
        let recovered_flow = flow(transport);
        let step = recovered_flow.step(job);
        let result = step(
            vec![json!({ "role": "user", "content": "recover" })],
            vec![tool()],
            None,
        )
        .await
        .unwrap();
        assert_eq!(result.message["content"], "Recovered result");
        drop(step);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[tokio::test]
    async fn missing_turn_retry_requires_explicit_empty_original_thread() {
        for (history, retry, allowed) in [
            (json!({"id":"empty-thread","turns":[]}), true, true),
            (
                json!({"id":"empty-thread","turns":[{"id":"unknown-turn"}]}),
                true,
                false,
            ),
            (json!({"id":"empty-thread"}), true, false),
            (json!({"id":"empty-thread","turns":null}), true, false),
            (json!({"id":"other-thread","turns":[]}), true, false),
            (json!({"id":"empty-thread","turns":[]}), false, false),
        ] {
            let dir = std::env::temp_dir()
                .join(format!("forge-empty-thread-{}", crate::events::new_id("t")));
            let mut job = spec("empty-thread-job");
            job.state_dir = Some(dir.clone());
            job.retry_failed = retry;
            let (tools, _) = declarations(&[tool()]).unwrap();
            let mut record =
                AgentJobRecord::starting(&job, "codex", serde_json::to_string(&tools).unwrap());
            record.thread_id = Some("empty-thread".into());
            record.result.status = AgentJobStatus::Failed;
            record.result.error = Some("unknown startup result".into());
            let path = crate::agent_job::journal_path(&job).unwrap();
            let before = serde_json::to_string(&record).unwrap();
            crate::sessions::write_atomic(&path, &before).unwrap();
            let starts = Arc::new(AtomicUsize::new(0));
            let counted = starts.clone();
            let transport = Arc::new(ScriptedTransport::new(move |m| {
                if let Some(r) = base_reply(m) {
                    return r;
                }
                match m["method"].as_str() {
                    Some("thread/resume") => {
                        assert_eq!(m["params"]["threadId"], "empty-thread");
                        vec![json!({"id":m["id"],"result":{"thread":{"id":"empty-thread"}}})]
                    }
                    Some("thread/read") => {
                        assert_eq!(m["params"]["includeTurns"], true);
                        vec![json!({"id":m["id"],"result":{"thread":history}})]
                    }
                    Some("turn/start") => {
                        assert_eq!(m["params"]["threadId"], "empty-thread");
                        counted.fetch_add(1, Ordering::SeqCst);
                        vec![
                            json!({"id":m["id"],"result":{"turn":{"id":"fresh-turn"}}}),
                            json!({"method":"item/completed","params":{"threadId":"empty-thread","turnId":"fresh-turn","item":{"type":"agentMessage","text":"reconciled"}}}),
                            json!({"method":"turn/completed","params":{"threadId":"empty-thread","turn":{"id":"fresh-turn","status":"completed"}}}),
                        ]
                    }
                    Some("thread/start") => {
                        panic!("generic reconciliation must retain the original thread")
                    }
                    _ => vec![],
                }
            }));
            let recovered = flow(transport);
            let step = recovered.step(job.clone());
            let result = step(
                vec![json!({"role":"user","content":"retry"})],
                vec![tool()],
                None,
            )
            .await;
            if allowed {
                assert_eq!(result.unwrap().message["content"], "reconciled");
                let saved: AgentJobRecord =
                    serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
                assert_eq!(saved.generation, 2);
                assert_eq!(saved.thread_id.as_deref(), Some("empty-thread"));
            } else {
                assert!(result
                    .err()
                    .unwrap()
                    .to_string()
                    .contains("CODEX_JOB_RECOVERY_REQUIRED"));
                assert_eq!(std::fs::read_to_string(&path).unwrap(), before);
            }
            assert_eq!(starts.load(Ordering::SeqCst), usize::from(allowed));
            drop(step);
            std::fs::remove_dir_all(dir).unwrap();
        }
    }

    #[tokio::test]
    async fn known_legacy_startup_abort_is_archived_before_new_thread() {
        for (known, retry, has_turn) in [
            (true, true, false),
            (true, false, false),
            (false, true, false),
            (true, true, true),
        ] {
            let dir = std::env::temp_dir().join(format!(
                "forge-startup-abort-{}",
                crate::events::new_id("t")
            ));
            let mut job = spec("legacy-startup-job");
            job.state_dir = Some(dir.clone());
            job.retry_failed = retry;
            let (tools, _) = declarations(&[tool()]).unwrap();
            let mut record =
                AgentJobRecord::starting(&job, "codex", serde_json::to_string(&tools).unwrap());
            record.thread_id = Some("old-ephemeral-thread".into());
            record.turn_id = has_turn.then(|| "old-turn".into());
            record.result.status = AgentJobStatus::Failed;
            let warning = json!({"details":null,"summary":"Project-local config, hooks, and exec policies are disabled in the following folders until the project is trusted, but skills still load.\n    1. /project/.codex\n       To load project-local config, hooks, and exec policies, add /project as a trusted project in /managed/config.toml.\n"});
            record.result.error = Some(if known {
                capability(&format!(
                    "受管配置未获接受或技能目录已变更，已停止隔离进程: configWarning: {warning}"
                ))
                .to_string()
            } else {
                "unknown startup failure".into()
            });
            let path = crate::agent_job::journal_path(&job).unwrap();
            let before = serde_json::to_string(&record).unwrap();
            crate::sessions::write_atomic(&path, &before).unwrap();
            let seen = Arc::new(std::sync::Mutex::new(Vec::<Value>::new()));
            let captured = seen.clone();
            let transport = Arc::new(ScriptedTransport::new(move |m| {
                captured.lock().unwrap().push(m.clone());
                if let Some(r) = base_reply(m) {
                    return r;
                }
                match m["method"].as_str() {
                    Some("thread/start") => {
                        vec![json!({"id":m["id"],"result":{"thread":{"id":"new-thread"}}})]
                    }
                    Some("thread/resume") if !known => vec![
                        json!({"id":m["id"],"error":{"code":-32000,"message":"thread unavailable"}}),
                    ],
                    Some("thread/resume") => vec![
                        json!({"id":m["id"],"result":{"thread":{"id":"old-ephemeral-thread"}}}),
                    ],
                    Some("thread/read") => vec![
                        json!({"id":m["id"],"result":{"thread":{"id":"old-ephemeral-thread","turns":[{"id":"old-turn","status":"completed","items":[]}]}}}),
                    ],
                    Some("turn/start") => vec![
                        json!({"id":m["id"],"result":{"turn":{"id":"fresh-turn"}}}),
                        json!({"method":"item/completed","params":{"threadId":m["params"]["threadId"],"turnId":"fresh-turn","item":{"type":"agentMessage","text":"recovered"}}}),
                        json!({"method":"turn/completed","params":{"threadId":m["params"]["threadId"],"turn":{"id":"fresh-turn","status":"completed"}}}),
                    ],
                    _ => vec![],
                }
            }));
            let recovered = flow(transport);
            let step = recovered.step(job);
            let result = step(
                vec![json!({"role":"user","content":"retry"})],
                vec![tool()],
                None,
            )
            .await;
            let allowed = known && retry;
            if allowed {
                assert_eq!(result.unwrap().message["content"], "recovered");
            } else {
                assert!(result
                    .err()
                    .unwrap()
                    .to_string()
                    .contains("CODEX_JOB_RECOVERY_REQUIRED"));
            }
            let archive = path.with_file_name(format!(
                "{}.g1.startup-abort.json",
                path.file_stem().unwrap().to_string_lossy()
            ));
            let seen = seen.lock().unwrap();
            let replacement = known && retry && !has_turn;
            assert_eq!(
                seen.iter()
                    .filter(|m| m["method"] == "thread/start")
                    .count(),
                usize::from(replacement)
            );
            assert_eq!(
                seen.iter().filter(|m| m["method"] == "turn/start").count(),
                usize::from(allowed)
            );
            if replacement {
                assert!(!seen
                    .iter()
                    .any(|m| m["method"] == "thread/resume" || m["method"] == "thread/read"));
                assert_eq!(std::fs::read_to_string(&archive).unwrap(), before);
                let saved: AgentJobRecord =
                    serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
                assert_eq!(saved.generation, 2);
                assert_eq!(saved.thread_id.as_deref(), Some("new-thread"));
                assert!(saved
                    .result
                    .evidence
                    .contains(&archive.to_string_lossy().into_owned()));
            } else {
                assert!(!archive.exists());
                if !allowed {
                    assert_eq!(std::fs::read_to_string(&path).unwrap(), before);
                }
            }
            drop(seen);
            drop(step);
            std::fs::remove_dir_all(dir).unwrap();
        }
    }

    #[tokio::test]
    async fn explicit_retry_requires_a_confirmed_terminal_generation() {
        for uncertain in [false, true] {
            let dir = std::env::temp_dir().join(format!(
                "forge-managed-retry-{}",
                crate::events::new_id("t")
            ));
            let mut job = spec("retry-job");
            job.state_dir = Some(dir.clone());
            let (tools, _) = declarations(&[tool()]).unwrap();
            let mut record =
                AgentJobRecord::starting(&job, "codex", serde_json::to_string(&tools).unwrap());
            record.thread_id = Some("retry-thread".into());
            record.turn_id = Some("old-turn".into());
            record.result.status = if uncertain {
                AgentJobStatus::Running
            } else {
                AgentJobStatus::Failed
            };
            record.terminal_confirmed = !uncertain;
            let path = crate::agent_job::journal_path(&job).unwrap();
            crate::sessions::write_atomic(&path, &serde_json::to_string(&record).unwrap()).unwrap();
            let starts = Arc::new(AtomicUsize::new(0));
            let counted = starts.clone();
            let transport = Arc::new(ScriptedTransport::new(move |m| {
                if let Some(r) = base_reply(m) {
                    return r;
                }
                match m["method"].as_str() {
                    Some("thread/resume") => vec![
                        json!({ "id": m["id"], "result": { "thread": { "id": "retry-thread" } } }),
                    ],
                    Some("thread/read") => vec![
                        json!({ "id": m["id"], "result": { "thread": { "id": "retry-thread", "turns": [{ "id": "old-turn", "status": "inProgress" }] } } }),
                    ],
                    Some("turn/start") => {
                        counted.fetch_add(1, Ordering::SeqCst);
                        vec![
                            json!({ "id": m["id"], "result": { "turn": { "id": "retry-turn" } } }),
                            json!({ "method": "item/completed", "params": { "threadId": "retry-thread", "turnId": "retry-turn", "item": { "type": "agentMessage", "text": "Fresh retry result" } } }),
                            json!({ "method": "turn/completed", "params": { "threadId": "retry-thread", "turn": { "id": "retry-turn", "status": "completed" } } }),
                        ]
                    }
                    Some("thread/start") => panic!("retry must retain the original thread"),
                    _ => vec![],
                }
            }));
            // Explicit retry still cannot replay an unconfirmed running turn.
            // A known failure needs authorization, even though it is terminal.
            job.retry_failed = uncertain;
            let first_flow = flow(transport.clone());
            let first = first_flow.step(job.clone());
            assert!(first(
                vec![json!({ "role": "user", "content": "retry" })],
                vec![tool()],
                None
            )
            .await
            .err()
            .unwrap()
            .to_string()
            .contains("CODEX_JOB_RECOVERY_REQUIRED"));
            assert_eq!(starts.load(Ordering::SeqCst), 0);
            drop(first);
            drop(first_flow);
            let saved: AgentJobRecord =
                serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
            assert!(saved.terminal_confirmed);
            job.retry_failed = true;
            let retry_flow = flow(transport);
            let retry = retry_flow.step(job);
            assert_eq!(
                retry(
                    vec![json!({ "role": "user", "content": "Authorized retry" })],
                    vec![tool()],
                    None
                )
                .await
                .unwrap()
                .message["content"],
                "Fresh retry result"
            );
            assert_eq!(starts.load(Ordering::SeqCst), 1);
            let saved: AgentJobRecord =
                serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
            assert_eq!(saved.generation, 2);
            assert!(saved.terminal_confirmed);
            drop(retry);
            std::fs::remove_dir_all(dir).unwrap();
        }
    }
}
