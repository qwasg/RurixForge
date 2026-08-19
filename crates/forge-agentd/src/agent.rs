//! F7 wave.2 turn 执行事件化(D-F7-A;参考 I:\agent-debug-frontend-backend-copy-20260530
//! gateway-go/backend-rs agent-core engine/react.rs turn 事件流语义,语义级自研不 fork 代码)。
//!
//! - POST /api/forge/sessions/{id}/ask:execute {userInput, mode?}:五模式 turn 引擎。
//!   事件序列(全持久):composer.user.message → agent.started → [agent.tool.invoked /
//!   agent.tool.completed|failed / agent.usage] → agent.message → agent.completed|failed|cancelled。
//!   run 注册表(内存 RunRegistry + CancelToken)+ session.activeRunId 持久化 + 终态清理。
//!   首条 composer.user.message 自动命名会话标题(前 48 字,titleManuallySet 保持 false)。
//! - 五模式:ask=禁工具纯对话(tools 空)/build=全量工具循环(复用 llm.rs run_tool_loop)/
//!   debug=build+调试导向提示/plan=只读工具集+写工具 TOOL_FORBIDDEN 门/
//!   multitask=F3 swarm 确定性模板链服务端化(client executeMultitask 同正则,进程内调
//!   swarm 协调器非 HTTP)。
//! - runs REST:GET /api/forge/runs/{id} + POST /api/forge/runs/{id}/cancel(内存 RunControl,
//!   进程重启即空——与 swarm 同纪律,如实)。
//! - todos REST:GET /api/forge/sessions/{id}/todos + POST /api/forge/todos +
//!   PATCH /api/forge/todos/{id};TodoStore 持久化 data/agent-sessions/todos.json
//!   (读-改-写,同 sessions.json 纪律);todo.created/todo.updated 持久事件。
//! - R-5 继承:密钥永不进事件/错误消息(本模块不触密钥本体,仅经 llm.rs 工厂闭包)。

use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::path::{Path as FsPath, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use crate::events::{new_id, now_rfc3339, EventDraft};
use crate::llm::{self, ExecFn, LoopEvent, StepFn, ToolLoopCfg};
use crate::sessions::DebugSession;
use crate::AppState;

/// composer 五模式(契约 G-F7-2;未知 mode → 400 INVALID_INPUT)。
pub const MODES: [&str; 5] = ["ask", "build", "debug", "plan", "multitask"];

/// 写工具显式名集合(plan 模式:从给 provider 的 tools 中剔除 + 调用侧 TOOL_FORBIDDEN 门)。
/// 判定口径:凡改变场景/资产/代码/播放态/编辑器视图态者皆写;已按 mcp::KNOWN_TOOLS 全量
/// 核对(单测 write_tools_subset_of_known_tools 守门防漏名)。
pub const WRITE_TOOLS: &[&str] = &[
    // engine-scene:场景/实体/组件/变换写
    "mcp__engine-scene__scene_new",
    "mcp__engine-scene__entity_create",
    "mcp__engine-scene__entity_destroy",
    "mcp__engine-scene__entity_rename",
    "mcp__engine-scene__entity_batch_apply",
    "mcp__engine-scene__component_add",
    "mcp__engine-scene__component_remove",
    "mcp__engine-scene__component_set",
    "mcp__engine-scene__transform_set",
    "mcp__engine-scene__transform_batch_set",
    // engine-scene:场景文件/checkpoint/撤销重做
    "mcp__engine-scene__scene_save",
    "mcp__engine-scene__scene_load",
    "mcp__engine-scene__scene_checkpoint",
    "mcp__engine-scene__scene_rollback",
    "mcp__engine-scene__edit_undo",
    "mcp__engine-scene__edit_redo",
    // engine-scene:播放态迁移与输入注入
    "mcp__engine-scene__play_enter",
    "mcp__engine-scene__play_pause",
    "mcp__engine-scene__play_resume",
    "mcp__engine-scene__play_step",
    "mcp__engine-scene__play_exit",
    "mcp__engine-scene__logic_inject_input",
    // engine-scene:编辑器视图态写(相机/共享)
    "mcp__engine-scene__viewport_set_camera",
    "mcp__engine-scene__viewport_share_open",
    "mcp__engine-scene__viewport_share_close",
    // asset-pipeline:资产写
    "mcp__asset-pipeline__asset_import",
    "mcp__asset-pipeline__asset_delete",
    "mcp__asset-pipeline__asset_move",
    "mcp__asset-pipeline__asset_fix_redirectors",
    "mcp__asset-pipeline__asset_reimport",
    "mcp__asset-pipeline__asset_set_meta",
    "mcp__asset-pipeline__material_create",
    "mcp__asset-pipeline__texture_process",
    // code-forge:构建/运行/格式化(产物或源文件写)
    "mcp__code-forge__rx_build",
    "mcp__code-forge__rx_run",
    "mcp__code-forge__rx_fmt",
    "mcp__code-forge__graph_create",
    "mcp__code-forge__code_structured_edit",
    // gen-image / gen-model:生成与接受落资产
    "mcp__gen-image__gen_image",
    "mcp__gen-image__gen_texture_set",
    "mcp__gen-image__gen_accept",
    "mcp__gen-image__gen_variations",
    "mcp__gen-model__gen_mesh",
    "mcp__gen-model__gen_mesh_refine",
    "mcp__gen-model__gen_accept",
];

/// 写工具判定(plan 模式门)。
pub fn is_write_tool(name: &str) -> bool {
    WRITE_TOOLS.contains(&name)
}

// 留痕:debug = build + 调试导向提示(F7 wave.2 契约,文案自定注释留痕)。
const DEBUG_PROMPT_SUFFIX: &str = "\n当前为 debug 模式:优先使用 viewport/scene 只读工具\
(viewport_frame/scene_summary/scene_graph_dump/entity_list/entity_get 等)取证定位问题,\
再决定是否需要写操作;每步观察与结论如实汇报,不得猜测式修改。";
// 留痕:plan = 只读工具集 + 计划提示(写工具已双侧门控:tools 剔除 + 调用 TOOL_FORBIDDEN)。
const PLAN_PROMPT_SUFFIX: &str = "\n当前为 plan 模式:只读取证与方案规划,写工具已被禁用\
(调用将被拒绝 TOOL_FORBIDDEN);请输出分步实施计划,不要试图修改场景/资产/代码。";

// ---------- runs:内存 RunRegistry + CancelToken ----------

/// run 本体(wire camelCase;trigger 现仅 composer_chat)。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RunRecord {
    pub id: String,
    pub session_id: String,
    pub trigger: String,
    /// running | completed | failed | cancelled。
    pub status: String,
    pub created_at: String,
    pub updated_at: String,
}

/// 取消令牌(原子旗;工具循环每迭代开头检查一次)。
#[derive(Clone, Default)]
pub struct CancelToken(Arc<AtomicBool>);

impl CancelToken {
    pub fn cancel(&self) {
        self.0.store(true, Ordering::SeqCst);
    }
    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::SeqCst)
    }
}

struct RunEntry {
    record: RunRecord,
    cancel: CancelToken,
}

/// 内存 run 注册表(进程重启即空——snapshot run 字段注册表丢失则 null,如实)。
#[derive(Default)]
pub struct RunRegistry {
    inner: Mutex<HashMap<String, RunEntry>>,
}

impl RunRegistry {
    /// 创建 running run,返回 (记录, 取消令牌)。
    pub fn begin(&self, session_id: &str, trigger: &str) -> (RunRecord, CancelToken) {
        let ts = now_rfc3339();
        let record = RunRecord {
            id: new_id("run"),
            session_id: session_id.to_string(),
            trigger: trigger.to_string(),
            status: "running".to_string(),
            created_at: ts.clone(),
            updated_at: ts,
        };
        let cancel = CancelToken::default();
        self.inner.lock().unwrap().insert(
            record.id.clone(),
            RunEntry {
                record: record.clone(),
                cancel: cancel.clone(),
            },
        );
        (record, cancel)
    }

    pub fn get(&self, id: &str) -> Option<RunRecord> {
        self.inner.lock().unwrap().get(id).map(|e| e.record.clone())
    }

    /// 终态迁移(completed|failed|cancelled);返回迁移后记录。
    pub fn finish(&self, id: &str, status: &str) -> Option<RunRecord> {
        let mut inner = self.inner.lock().unwrap();
        let entry = inner.get_mut(id)?;
        entry.record.status = status.to_string();
        entry.record.updated_at = now_rfc3339();
        Some(entry.record.clone())
    }

    /// 置取消旗;run 不存在 → false。
    pub fn cancel(&self, id: &str) -> bool {
        let inner = self.inner.lock().unwrap();
        match inner.get(id) {
            Some(e) => {
                e.cancel.cancel();
                true
            }
            None => false,
        }
    }

    /// 取消该会话处于 running 的 run(单测迭代间注入取消用);无 running run → false。
    pub fn cancel_active_for_session(&self, session_id: &str) -> bool {
        let inner = self.inner.lock().unwrap();
        for e in inner.values() {
            if e.record.session_id == session_id && e.record.status == "running" {
                e.cancel.cancel();
                return true;
            }
        }
        false
    }
}

// ---------- todos:TodoStore 持久化 data/agent-sessions/todos.json ----------

pub const TODO_KINDS: [&str; 2] = ["edit", "explore"];
pub const TODO_STATUSES: [&str; 4] = ["queued", "running", "completed", "failed"];

/// 待办项(wire camelCase;kind/source/status 带默认,兼容旧文件)。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TodoItem {
    pub id: String,
    pub session_id: String,
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default = "default_todo_kind")]
    pub kind: String,
    #[serde(default = "default_todo_source")]
    pub source: String,
    #[serde(default = "default_todo_status")]
    pub status: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
    #[serde(default)]
    pub created_at: String,
    #[serde(default)]
    pub updated_at: String,
}

fn default_todo_kind() -> String {
    "edit".to_string()
}
fn default_todo_source() -> String {
    "user".to_string()
}
fn default_todo_status() -> String {
    "queued".to_string()
}

#[derive(Debug)]
pub enum TodoError {
    NotFound,
    /// 非法 status/kind/空 title(400 TODO_INVALID)。
    Invalid(String),
}

/// 待办存贮:内存 Vec(创建序)+ todos.json 整文件读-改-写(Mutex 串行,同 sessions 纪律)。
pub struct TodoStore {
    path: PathBuf,
    inner: Mutex<Vec<TodoItem>>,
}

impl TodoStore {
    pub fn load(path: PathBuf) -> Self {
        let items = read_todos_file(&path);
        TodoStore {
            path,
            inner: Mutex::new(items),
        }
    }

    fn persist_locked(&self, inner: &[TodoItem]) {
        let doc = json!({ "todos": inner });
        let text = serde_json::to_string_pretty(&doc).expect("todos 序列化失败");
        if let Err(e) = write_atomic(&self.path, &text) {
            eprintln!("todos.json 写盘失败({}): {e}", self.path.display());
        }
    }

    pub fn list_by_session(&self, session_id: &str) -> Vec<TodoItem> {
        self.inner
            .lock()
            .unwrap()
            .iter()
            .filter(|t| t.session_id == session_id)
            .cloned()
            .collect()
    }

    pub fn create(
        &self,
        session_id: &str,
        title: &str,
        description: Option<String>,
        kind: Option<String>,
    ) -> Result<TodoItem, TodoError> {
        let title = title.trim();
        if title.is_empty() {
            return Err(TodoError::Invalid("title 不可空".to_string()));
        }
        let kind = kind.unwrap_or_else(default_todo_kind);
        if !TODO_KINDS.contains(&kind.as_str()) {
            return Err(TodoError::Invalid(format!("非法 kind: {kind}")));
        }
        let ts = now_rfc3339();
        let todo = TodoItem {
            id: new_id("todo"),
            session_id: session_id.to_string(),
            title: title.to_string(),
            description,
            kind,
            source: default_todo_source(),
            status: default_todo_status(),
            summary: None,
            created_at: ts.clone(),
            updated_at: ts,
        };
        let mut inner = self.inner.lock().unwrap();
        inner.push(todo.clone());
        self.persist_locked(&inner);
        Ok(todo)
    }

    pub fn patch(&self, id: &str, req: &PatchTodoRequest) -> Result<TodoItem, TodoError> {
        let mut inner = self.inner.lock().unwrap();
        let Some(todo) = inner.iter_mut().find(|t| t.id == id) else {
            return Err(TodoError::NotFound);
        };
        if let Some(status) = &req.status {
            if !TODO_STATUSES.contains(&status.as_str()) {
                return Err(TodoError::Invalid(format!("非法 status: {status}")));
            }
            todo.status = status.clone();
        }
        if let Some(title) = &req.title {
            if title.trim().is_empty() {
                return Err(TodoError::Invalid("title 不可空".to_string()));
            }
            todo.title = title.trim().to_string();
        }
        if let Some(description) = &req.description {
            todo.description = Some(description.clone());
        }
        if let Some(summary) = &req.summary {
            todo.summary = Some(summary.clone());
        }
        todo.updated_at = now_rfc3339();
        let out = todo.clone();
        self.persist_locked(&inner);
        Ok(out)
    }
}

fn read_todos_file(path: &FsPath) -> Vec<TodoItem> {
    let Ok(text) = std::fs::read_to_string(path) else {
        return Vec::new();
    };
    let v: Value = match serde_json::from_str(&text) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("todos.json 解析失败({}): {e},按空处理", path.display());
            return Vec::new();
        }
    };
    v.get("todos")
        .and_then(Value::as_array)
        .cloned()
        .or_else(|| v.as_array().cloned())
        .unwrap_or_default()
        .into_iter()
        .filter_map(|item| {
            serde_json::from_value::<TodoItem>(item)
                .map_err(|e| eprintln!("todos.json 条目解析失败: {e}"))
                .ok()
        })
        .collect()
}

/// 原子写:tmp 全量写 + rename(与 sessions.rs write_atomic 同纪律)。
fn write_atomic(path: &FsPath, text: &str) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, text)?;
    std::fs::rename(&tmp, path)
}

// ---------- turn 引擎 ----------

/// turn 输入(step/execute 可注入:生产 = mock|deepseek + mcp;单测 = scripted fake 全内存)。
pub struct TurnInput<'a> {
    pub user_input: &'a str,
    pub mode: &'a str,
    /// "mock" | "deepseek"(agent.message payload.provider)。
    pub provider_label: &'a str,
    /// "mock" | "deepseek-chat"(agent.started payload.model / agent.usage payload.model)。
    pub model_label: &'a str,
    /// 给 provider 的 openai tools(plan 过滤在引擎内;ask/multitask 由调用方传空)。
    pub tools: Vec<Value>,
    pub step: &'a StepFn,
    pub execute: &'a ExecFn,
}

/// turn 结果(HTTP 200 如实返回面;run 终态 + 文本/错误)。
pub struct TurnOutput {
    pub run_id: String,
    /// completed | failed | cancelled。
    pub status: String,
    pub text: String,
    pub error: Option<String>,
}

/// turn 全流程:建 run → 事件序列 → 模式分派 → 终态 + activeRunId 清理(均持久事件)。
pub async fn execute_turn(state: &AppState, session: &DebugSession, input: TurnInput<'_>) -> TurnOutput {
    let sid = session.id.as_str();
    let (run, token) = state.runs.begin(sid, "composer_chat");
    let run_id = run.id.clone();
    // session.activeRunId = runId 持久化。
    let mut s = state.sessions.get(sid).unwrap_or_else(|| session.clone());
    s.active_run_id = Some(run_id.clone());
    s.touch();
    state.sessions.save(&s);

    // 首条消息判定须在发事件前查日志(「无先前 composer.user.message」口径)。
    let first_message = !state
        .events
        .persisted(sid)
        .iter()
        .any(|e| e.event_type == "composer.user.message");
    state.events.emit(
        EventDraft::new(sid, "composer.user.message", "composer").payload(json!({
            "text": input.user_input,
            "composerMode": input.mode,
            "runId": run_id,
        })),
    );
    state.events.emit(
        EventDraft::new(sid, "agent.started", "agent").payload(json!({
            "runId": run_id,
            "model": input.model_label,
        })),
    );

    // 首条消息自动命名:前 48 字(char 边界);titleManuallySet 保持 false,手动命名保护。
    if first_message && !session.title_manually_set {
        if let Some(mut s2) = state.sessions.get(sid) {
            s2.title = input.user_input.chars().take(48).collect();
            s2.touch();
            state.sessions.save(&s2);
        }
    }

    // 事件 sink:LoopEvent → 持久事件(agent.tool.* / agent.usage)。
    let events = state.events.clone();
    let sid_owned = sid.to_string();
    let rid = run_id.clone();
    let provider_label = input.provider_label.to_string();
    let model_label = input.model_label.to_string();
    let sink = move |ev: LoopEvent| match ev {
        LoopEvent::ToolInvoked {
            name,
            args,
            tool_call_id,
        } => {
            events.emit(
                EventDraft::new(&sid_owned, "agent.tool.invoked", "agent").payload(json!({
                    "name": name, "args": args, "toolCallId": tool_call_id, "runId": rid,
                })),
            );
        }
        LoopEvent::ToolCompleted {
            name,
            tool_call_id,
            duration_ms,
        } => {
            events.emit(
                EventDraft::new(&sid_owned, "agent.tool.completed", "agent").payload(json!({
                    "name": name, "ok": true, "toolCallId": tool_call_id, "runId": rid,
                    "durationMs": duration_ms,
                })),
            );
        }
        LoopEvent::ToolFailed {
            name,
            error,
            tool_call_id,
            duration_ms,
        } => {
            events.emit(
                EventDraft::new(&sid_owned, "agent.tool.failed", "agent").payload(json!({
                    "name": name, "error": error, "toolCallId": tool_call_id, "runId": rid,
                    "durationMs": duration_ms,
                })),
            );
        }
        LoopEvent::Usage(u) => {
            events.emit(
                EventDraft::new(&sid_owned, "agent.usage", "agent").payload(json!({
                    "runId": rid, "provider": provider_label, "model": model_label,
                    "promptTokens": u.prompt_tokens, "completionTokens": u.completion_tokens,
                    "totalTokens": u.total_tokens,
                })),
            );
        }
    };

    // 模式分派 → (status, text, error)。
    let outcome: (String, String, Option<String>) = if input.mode == "multitask" {
        match run_multitask(state, input.user_input, input.execute, &sink).await {
            Ok(text) => ("completed".to_string(), text, None),
            Err(e) => ("failed".to_string(), String::new(), Some(e)),
        }
    } else {
        let (system_prompt, tools) = match input.mode {
            "ask" => (llm::SYSTEM_PROMPT.to_string(), Vec::new()),
            "debug" => (
                format!("{}{}", llm::SYSTEM_PROMPT, DEBUG_PROMPT_SUFFIX),
                input.tools,
            ),
            "plan" => (
                format!("{}{}", llm::SYSTEM_PROMPT, PLAN_PROMPT_SUFFIX),
                // 只读工具集:写工具从 provider tools 剔除(第一侧门)。
                input
                    .tools
                    .into_iter()
                    .filter(|t| {
                        t.pointer("/function/name")
                            .and_then(Value::as_str)
                            .map(|n| !is_write_tool(n))
                            .unwrap_or(true)
                    })
                    .collect(),
            ),
            // build(默认):全量工具循环,原提示词。
            _ => (llm::SYSTEM_PROMPT.to_string(), input.tools),
        };
        let forbid = |n: &str| is_write_tool(n);
        let cancel_pred = {
            let token = token.clone();
            move || token.is_cancelled()
        };
        match llm::run_tool_loop(
            &system_prompt,
            input.user_input,
            ToolLoopCfg {
                tools,
                step: input.step,
                execute: input.execute,
                sink: Some(&sink),
                forbidden: if input.mode == "plan" { Some(&forbid) } else { None },
                cancelled: Some(&cancel_pred),
            },
        )
        .await
        {
            Ok(out) if out.cancelled => ("cancelled".to_string(), String::new(), None),
            Ok(out) => ("completed".to_string(), out.text, None),
            Err(e) => ("failed".to_string(), String::new(), Some(e.to_string())),
        }
    };

    // 终态事件 + run 迁移 + activeRunId 清理(三态均 HTTP 200 如实返回)。
    match outcome.0.as_str() {
        "completed" => {
            state.events.emit(
                EventDraft::new(sid, "agent.message", "agent").payload(json!({
                    "text": outcome.1, "runId": run_id, "provider": input.provider_label,
                })),
            );
            state.events.emit(
                EventDraft::new(sid, "agent.completed", "agent").payload(json!({
                    "runId": run_id, "text": outcome.1,
                })),
            );
        }
        "cancelled" => {
            state.events.emit(
                EventDraft::new(sid, "agent.cancelled", "agent")
                    .payload(json!({ "runId": run_id })),
            );
        }
        _ => {
            state.events.emit(
                EventDraft::new(sid, "agent.failed", "agent").payload(json!({
                    "runId": run_id,
                    "error": outcome.2.clone().unwrap_or_default(),
                })),
            );
        }
    }
    state.runs.finish(&run_id, &outcome.0);
    if let Some(mut s3) = state.sessions.get(sid) {
        s3.active_run_id = None;
        s3.touch();
        state.sessions.save(&s3);
    }
    TurnOutput {
        run_id,
        status: outcome.0,
        text: outcome.1,
        error: outcome.2,
    }
}

/// multitask 模板链(F3 D-F3-E 服务端化;client executeMultitask 同正则 /碰撞|collider/i)。
/// 命中 → entity_list 取 ids → swarm 协调器进程内分片执行 → swarm.execute 工具事件 + 聚合文本;
/// 未命中 → Err("模板未命中…")(agent.failed 如实,run.status=failed)。
async fn run_multitask(
    state: &AppState,
    user_input: &str,
    execute: &ExecFn,
    sink: &(dyn Fn(LoopEvent) + Send + Sync),
) -> Result<String, String> {
    let hit = user_input.contains("碰撞") || user_input.to_ascii_lowercase().contains("collider");
    if !hit {
        let head: String = user_input.chars().take(40).collect();
        return Err(format!(
            "multitask 模板未命中:「{head}」。当前支持模板:批量碰撞体(含「碰撞体/collider」);更多模板随 F4/F6 工具面落地"
        ));
    }
    // 经 executor 取场景实体(与 F3 client 链同数据源;可注入 → 单测全内存)。
    let (ok, feedback) = execute("mcp__engine-scene__entity_list".to_string(), json!({})).await;
    if !ok {
        return Err(format!("entity_list 调用失败: {feedback}"));
    }
    let list: Value = serde_json::from_str(&feedback)
        .map_err(|e| format!("entity_list 结果解析失败: {e}"))?;
    let ids: Vec<String> = list
        .get("entities")
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter_map(|e| e.get("id").and_then(Value::as_u64).map(|id| id.to_string()))
                .collect()
        })
        .unwrap_or_default();
    if ids.is_empty() {
        return Err("multitask 失败:当前场景无实体,无法生成碰撞体".to_string());
    }
    // swarm.execute 工具事件(分片聚合成一次工具语义;toolCallId 合成)。
    let tool_call_id = new_id("call");
    let swarm_args = json!({
        "shardType": "scene-partition",
        "items": ids,
        "shardCount": 4,
        "operation": { "kind": "add_component", "type": "RigidBody", "props": { "kind": "static", "mass": 0 } },
    });
    sink(LoopEvent::ToolInvoked {
        name: "swarm.execute".to_string(),
        args: swarm_args,
        tool_call_id: tool_call_id.clone(),
    });
    let started = std::time::Instant::now();
    match swarm_collision_run(state, execute, ids).await {
        Ok(summary) => {
            sink(LoopEvent::ToolCompleted {
                name: "swarm.execute".to_string(),
                tool_call_id,
                duration_ms: started.elapsed().as_millis() as u64,
            });
            Ok(summary)
        }
        Err(e) => {
            sink(LoopEvent::ToolFailed {
                name: "swarm.execute".to_string(),
                error: e.clone(),
                tool_call_id,
                duration_ms: started.elapsed().as_millis() as u64,
            });
            Err(e)
        }
    }
}

/// swarm 分片执行(进程内协调器,非 HTTP;每 item 经 executor component_add RigidBody static)。
/// 逐 item 失败如实计数不遮蔽(F3 分片报告纪律);全片失败 → 整体 Err(agent.tool.failed)。
async fn swarm_collision_run(
    state: &AppState,
    execute: &ExecFn,
    ids: Vec<String>,
) -> Result<String, String> {
    let total_items = ids.len();
    let shard_ids = state
        .swarm
        .create_shards("scene-partition", ids, 4)
        .map_err(|e| match e {
            crate::swarm::ShardError::Overlap(m) | crate::swarm::ShardError::Invalid(m) => {
                format!("swarm 分片创建失败: {m}")
            }
        })?;
    let shard_count = shard_ids.len();
    let mut total_ok = 0usize;
    let mut total_err = 0usize;
    let mut first_errors: Vec<String> = Vec::new();
    for id in &shard_ids {
        let shard = state
            .swarm
            .get_shard(id)
            .ok_or_else(|| format!("分片丢失: {id}"))?;
        state.swarm.mark_running(id);
        let mut ok_items: Vec<String> = Vec::new();
        let mut errors: Vec<Value> = Vec::new();
        for item in &shard.items {
            let eid: u64 = match item.parse() {
                Ok(v) => v,
                Err(_) => {
                    errors.push(json!({ "item": item, "error": "实体 id 非数值" }));
                    continue;
                }
            };
            let (ok, feedback) = execute(
                "mcp__engine-scene__component_add".to_string(),
                json!({ "id": eid, "type": "RigidBody", "props": { "kind": "static", "mass": 0 } }),
            )
            .await;
            if ok {
                ok_items.push(item.clone());
            } else {
                errors.push(json!({ "item": item, "error": feedback }));
            }
        }
        total_ok += ok_items.len();
        total_err += errors.len();
        for e in errors.iter().take(3) {
            first_errors.push(e.to_string());
        }
        let shard_ok = errors.is_empty();
        state.swarm.complete_shard(
            id,
            json!({
                "shardId": id,
                "ok": ok_items,
                "okCount": ok_items.len(),
                "errors": errors,
            }),
            shard_ok,
        );
    }
    let mut summary =
        format!("swarm 分片聚合:{shard_count} 片,共 {total_items} 项,成功 {total_ok},失败 {total_err}");
    if !first_errors.is_empty() {
        summary.push_str(&format!(";首批失败: {}", first_errors.join(" | ")));
    }
    // 全数失败 = 整体失败(不遮蔽);部分失败如实计数仍 completed(分片纪律)。
    if total_ok == 0 && total_err > 0 {
        return Err(summary);
    }
    Ok(summary)
}

// ---------- REST handlers ----------

fn not_found(code: &str, message: String) -> Response {
    (
        StatusCode::NOT_FOUND,
        Json(json!({ "error": { "code": code, "message": message } })),
    )
        .into_response()
}

fn bad_request(code: &str, message: &str) -> Response {
    (
        StatusCode::BAD_REQUEST,
        Json(json!({ "error": { "code": code, "message": message } })),
    )
        .into_response()
}

/// F7 wave.4:provider 选择——会话显式选 "mock" 模型 → 强制 Mock(有 key 也如实走 mock);
/// 选 "openai-compat" → 走 resolve_openai_compat 配置面(已配齐返回 OpenAiCompat,缺一返回
/// OpenAiCompatNotConfigured 显式错误态,不静默回落 deepseek/mock);
/// 其余(未选/选 deepseek-chat 等)走 resolve_provider 现状逻辑。抽出以便确定单测。
fn provider_for_session(session: &DebugSession) -> llm::Provider {
    match session.selected_model_id.as_deref() {
        Some("mock") => llm::Provider::Mock,
        Some("openai-compat") => match llm::resolve_openai_compat() {
            Some((base_url, model, key)) => llm::Provider::OpenAiCompat {
                base_url,
                model,
                key,
            },
            None => llm::Provider::OpenAiCompatNotConfigured,
        },
        _ => llm::resolve_provider(),
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AskExecuteRequest {
    #[serde(default)]
    user_input: String,
    #[serde(default)]
    mode: Option<String>,
}

/// POST /api/forge/sessions/{id}/ask:execute {userInput, mode?默认 build}。
/// 404 SESSION_NOT_FOUND / 400 INVALID_INPUT(空 userInput 或未知 mode);
/// 三态终态均 HTTP 200 {message:{text}, run:{id,status}, mode[, error]}。
pub async fn ask_execute(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    Json(req): Json<AskExecuteRequest>,
) -> Response {
    let Some(session) = state.sessions.get(&id) else {
        return not_found("SESSION_NOT_FOUND", format!("会话不存在: {id}"));
    };
    let user_input = req.user_input.trim().to_string();
    if user_input.is_empty() {
        return bad_request("INVALID_INPUT", "userInput 不可空");
    }
    let mode = req
        .mode
        .map(|m| m.trim().to_string())
        .filter(|m| !m.is_empty())
        .unwrap_or_else(|| "build".to_string());
    if !MODES.contains(&mode.as_str()) {
        return bad_request(
            "INVALID_INPUT",
            &format!("未知 mode: {mode}(支持 {MODES:?})"),
        );
    }

    // F7 wave.4:会话显式选 "mock" 模型 → 强制 Mock provider(有 key 也如实走 mock);
    // F8 wave.2:选 "openai-compat" → 配置面解析(未配齐 = 显式 NOT_CONFIGURED 步进);
    // 其余(未选/选 deepseek-chat)走 resolve_provider 现状逻辑。
    let provider = provider_for_session(&session);
    let (provider_label, model_label) = match &provider {
        llm::Provider::Mock => ("mock", "mock"),
        llm::Provider::Deepseek(_) => ("deepseek", "deepseek-chat"),
        // openai-compat:model 标签 = 配置的模型名(usage/started 事件如实)。
        llm::Provider::OpenAiCompat { model, .. } => ("openai-compat", model.as_str()),
        llm::Provider::OpenAiCompatNotConfigured => ("openai-compat", "openai-compat"),
    };
    let mut step: Box<StepFn> = match &provider {
        llm::Provider::Mock => llm::mock_step(),
        llm::Provider::Deepseek(k) => llm::deepseek_step(k),
        llm::Provider::OpenAiCompat {
            base_url,
            model,
            key,
        } => llm::openai_compat_step(base_url, model, key),
        // 选中未配齐:显式 NOT_CONFIGURED 错误(首轮即败,run failed 如实;不静默回落)。
        llm::Provider::OpenAiCompatNotConfigured => llm::openai_compat_not_configured_step(),
    };
    // tools:ask/multitask 或 mock provider → 空(mock 不触网不触 MCP,恒绿 seam);
    // deepseek/openai-compat build/debug/plan → MCP 工具面实测拉取(失败 = step 即错,走 agent.failed 链,
    // 与 llm/chat 502 形态差异留痕:agent 语义 HTTP 200 + run failed);
    // 未配齐 openai-compat 不拉工具面(步进首轮即显式错)。
    let mut tools: Vec<Value> = Vec::new();
    if matches!(mode.as_str(), "build" | "debug" | "plan") {
        if matches!(
            provider,
            llm::Provider::Deepseek(_) | llm::Provider::OpenAiCompat { .. }
        ) {
            match crate::mcp::list_all_tools().await {
                Ok(t) => tools = llm::to_openai_tools(&t),
                Err(e) => {
                    let msg = format!("MCP 工具面拉取失败: {e}");
                    step = Box::new(move |_, _| {
                        let m = msg.clone();
                        Box::pin(async move { Err(llm::LlmError::new(m)) })
                    });
                }
            }
        }
    }
    let execute = llm::mcp_executor();
    let out = execute_turn(
        &state,
        &session,
        TurnInput {
            user_input: &user_input,
            mode: &mode,
            provider_label,
            model_label,
            tools,
            step: step.as_ref(),
            execute: execute.as_ref(),
        },
    )
    .await;
    let mut body = json!({
        "message": { "text": out.text },
        "run": { "id": out.run_id, "status": out.status },
        "mode": mode,
    });
    if let Some(e) = out.error {
        body["error"] = json!(e);
    }
    Json(body).into_response()
}

/// GET /api/forge/runs/{id} → {run}(404 RUN_NOT_FOUND)。
pub async fn get_run(State(state): State<Arc<AppState>>, Path(id): Path<String>) -> Response {
    match state.runs.get(&id) {
        Some(run) => Json(json!({ "run": run })).into_response(),
        None => not_found("RUN_NOT_FOUND", format!("run 不存在: {id}")),
    }
}

/// POST /api/forge/runs/{id}/cancel:running → 置取消旗 {ok:true,runId};
/// 非 running 如实 {ok:false,runId,status};不存在 404 RUN_NOT_FOUND。
pub async fn cancel_run(State(state): State<Arc<AppState>>, Path(id): Path<String>) -> Response {
    let Some(run) = state.runs.get(&id) else {
        return not_found("RUN_NOT_FOUND", format!("run 不存在: {id}"));
    };
    if run.status != "running" {
        return Json(json!({ "ok": false, "runId": id, "status": run.status })).into_response();
    }
    state.runs.cancel(&id);
    Json(json!({ "ok": true, "runId": id })).into_response()
}

/// GET /api/forge/sessions/{id}/todos → {todos}(404 SESSION_NOT_FOUND)。
pub async fn list_todos(State(state): State<Arc<AppState>>, Path(id): Path<String>) -> Response {
    if state.sessions.get(&id).is_none() {
        return not_found("SESSION_NOT_FOUND", format!("会话不存在: {id}"));
    }
    Json(json!({ "todos": state.todos.list_by_session(&id) })).into_response()
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateTodoRequest {
    session_id: String,
    #[serde(default)]
    title: String,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    kind: Option<String>,
}

/// POST /api/forge/todos {sessionId, title, description?, kind?} → {todo} + todo.created。
pub async fn create_todo(
    State(state): State<Arc<AppState>>,
    Json(req): Json<CreateTodoRequest>,
) -> Response {
    if state.sessions.get(&req.session_id).is_none() {
        return not_found("SESSION_NOT_FOUND", format!("会话不存在: {}", req.session_id));
    }
    match state.todos.create(
        &req.session_id,
        &req.title,
        req.description,
        req.kind,
    ) {
        Ok(todo) => {
            state.events.emit(
                EventDraft::new(&req.session_id, "todo.created", "todo").payload(json!({
                    "id": todo.id, "title": todo.title, "kind": todo.kind, "status": todo.status,
                })),
            );
            Json(json!({ "todo": todo })).into_response()
        }
        Err(TodoError::Invalid(m)) => bad_request("TODO_INVALID", &m),
        Err(TodoError::NotFound) => unreachable!("create 不产生 NotFound"),
    }
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PatchTodoRequest {
    #[serde(default)]
    status: Option<String>,
    #[serde(default)]
    title: Option<String>,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    summary: Option<String>,
}

/// PATCH /api/forge/todos/{id} {status?,title?,description?,summary?} → {todo} + todo.updated。
/// 404 TODO_NOT_FOUND;非法 status/空 title → 400 TODO_INVALID。
pub async fn patch_todo(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    Json(req): Json<PatchTodoRequest>,
) -> Response {
    match state.todos.patch(&id, &req) {
        Ok(todo) => {
            state.events.emit(
                EventDraft::new(&todo.session_id, "todo.updated", "todo").payload(json!({
                    "id": todo.id, "title": todo.title, "status": todo.status,
                    "summary": todo.summary,
                })),
            );
            Json(json!({ "todo": todo })).into_response()
        }
        Err(TodoError::NotFound) => not_found("TODO_NOT_FOUND", format!("todo 不存在: {id}")),
        Err(TodoError::Invalid(m)) => bad_request("TODO_INVALID", &m),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::events::EventBus;
    use crate::llm::{StepOutcome, Usage};
    use crate::sessions::{ChatFolderStore, SessionStore};
    use std::path::PathBuf;
    use std::time::Instant;

    /// 隔离数据根的 AppState(事件/会话/todos 落盘隔离;run/swarm 内存)。
    fn test_state(tag: &str) -> (Arc<AppState>, PathBuf) {
        let dir = std::env::temp_dir().join(format!(
            "agentd-agent-{tag}-{}-{}",
            std::process::id(),
            new_id("t")
        ));
        let state = Arc::new(AppState {
            started: Instant::now(),
            proposals: crate::proposals::ProposalStore::default(),
            swarm: crate::swarm::SwarmCoordinator::default(),
            events: Arc::new(EventBus::new(dir.join("agent-events"), 256)),
            sessions: Arc::new(SessionStore::load(
                dir.join("agent-sessions").join("sessions.json"),
            )),
            folders: Arc::new(ChatFolderStore::load(
                dir.join("agent-sessions").join("chat-folders.json"),
            )),
            runs: Arc::new(RunRegistry::default()),
            todos: Arc::new(TodoStore::load(dir.join("agent-sessions").join("todos.json"))),
        });
        (state, dir)
    }

    fn event_types(state: &AppState, sid: &str) -> Vec<String> {
        state
            .events
            .persisted(sid)
            .iter()
            .map(|e| e.event_type.clone())
            .collect()
    }

    fn tool_call_msg(name: &str, args: &str) -> Value {
        json!({
            "role": "assistant",
            "tool_calls": [{
                "id": "call_1",
                "type": "function",
                "function": { "name": name, "arguments": args },
            }],
        })
    }
    fn final_msg(text: &str) -> Value {
        json!({ "role": "assistant", "content": text })
    }

    /// scripted step:依次弹出;可选记录每轮 tools 名集合。
    fn scripted_step(
        msgs: Vec<Value>,
        tools_seen: Option<Arc<Mutex<Vec<Vec<String>>>>>,
    ) -> Box<StepFn> {
        let queue = Arc::new(Mutex::new(std::collections::VecDeque::from(msgs)));
        Box::new(move |_m, tools| {
            if let Some(seen) = &tools_seen {
                seen.lock().unwrap().push(
                    tools
                        .iter()
                        .filter_map(|t| {
                            t.pointer("/function/name")
                                .and_then(Value::as_str)
                                .map(str::to_owned)
                        })
                        .collect(),
                );
            }
            let next = queue.lock().unwrap().pop_front().expect("script 耗尽");
            Box::pin(async move {
                Ok(StepOutcome {
                    message: next,
                    usage: None,
                })
            })
        })
    }

    fn ok_executor() -> Box<ExecFn> {
        Box::new(|name, _a| Box::pin(async move { (true, format!("{name} ok")) }))
    }

    fn turn_input<'a>(
        mode: &'a str,
        text: &'a str,
        tools: Vec<Value>,
        step: &'a StepFn,
        execute: &'a ExecFn,
    ) -> TurnInput<'a> {
        TurnInput {
            user_input: text,
            mode,
            provider_label: "mock",
            model_label: "mock",
            tools,
            step,
            execute,
        }
    }

    #[test]
    fn provider_for_session_selected_mock_forces_mock() {
        // F7 wave.4:selectedModelId=="mock" 强制 Mock provider(判定不经 resolve_provider,
        // 与环境 key 有无无关,确定性断言);未选模型则回落现状 resolve_provider(环境相关不断言)。
        let (state, dir) = test_state("provsel");
        let s = state
            .sessions
            .create("t", "coding", Some("mock".to_string()), true);
        assert!(
            matches!(provider_for_session(&s), llm::Provider::Mock),
            "显式 mock 须强制 Mock provider"
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    /// F8 wave.2:openai-compat 选模解析(两腿);env 操作走 llm::TEST_ENV_LOCK 同源纪律。
    #[test]
    fn provider_for_session_openai_compat_two_legs() {
        let _g = crate::llm::TEST_ENV_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        std::env::remove_var("FORGE_LLM_API_KEY");
        std::env::remove_var("FORGE_GEN_API_KEY");
        let dir = std::env::temp_dir().join(format!(
            "agentd-agent-oaiprov-{}-{}",
            std::process::id(),
            new_id("t")
        ));
        std::fs::create_dir_all(&dir).unwrap();
        std::env::set_var("FORGE_GEN_DATA_DIR", &dir);
        let (state, state_dir) = test_state("oaiprov");
        let s = state
            .sessions
            .create("t", "coding", Some("openai-compat".to_string()), true);
        // 未配置腿:显式 NotConfigured(不静默回落 deepseek/mock)。
        assert!(
            matches!(provider_for_session(&s), llm::Provider::OpenAiCompatNotConfigured),
            "未配齐须显式 NotConfigured"
        );
        // 配齐腿:config JSON + keystore → OpenAiCompat 三联。
        std::fs::write(
            dir.join("llm-openai-compat.json"),
            r#"{"base_url":"http://127.0.0.1:1","model":"qwen2.5-7b"}"#,
        )
        .unwrap();
        gend::keystore::set_key("openai-compat", "sk-test-oai-agent-leg").unwrap();
        match provider_for_session(&s) {
            llm::Provider::OpenAiCompat {
                base_url,
                model,
                key,
            } => {
                assert_eq!(base_url, "http://127.0.0.1:1");
                assert_eq!(model, "qwen2.5-7b");
                assert_eq!(key, "sk-test-oai-agent-leg");
            }
            other => panic!("已配齐应 OpenAiCompat: {other:?}"),
        }
        std::env::remove_var("FORGE_GEN_DATA_DIR");
        std::fs::remove_dir_all(&dir).ok();
        std::fs::remove_dir_all(&state_dir).ok();
    }

    /// F8 wave.2:选 openai-compat 未配置 → ask:execute 首轮显式败(agent.failed,错误码面)。
    #[tokio::test]
    async fn ask_execute_openai_compat_not_configured_explicit_failure() {
        let _g = crate::llm::TEST_ENV_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        std::env::remove_var("FORGE_LLM_API_KEY");
        std::env::remove_var("FORGE_GEN_API_KEY");
        let dir = std::env::temp_dir().join(format!(
            "agentd-agent-oainc-{}-{}",
            std::process::id(),
            new_id("t")
        ));
        std::env::set_var("FORGE_GEN_DATA_DIR", &dir);
        let (state, state_dir) = test_state("oainc");
        let session = state
            .sessions
            .create("t", "coding", Some("openai-compat".to_string()), true);
        // ask 模式(不拉 MCP 工具面);handler 级端到端。
        let resp = ask_execute(
            State(state.clone()),
            Path(session.id.clone()),
            Json(AskExecuteRequest {
                user_input: "你好".to_string(),
                mode: Some("ask".to_string()),
            }),
        )
        .await;
        let (parts, body) = resp.into_parts();
        assert_eq!(parts.status, StatusCode::OK, "agent 语义三态均 200");
        let bytes = axum::body::to_bytes(body, 1 << 20).await.unwrap();
        let v: Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(v["run"]["status"], "failed", "{v}");
        let err = v["error"].as_str().unwrap();
        assert!(
            err.starts_with("OPENAI_COMPAT_NOT_CONFIGURED"),
            "显式 NOT_CONFIGURED 同族: {err}"
        );
        assert!(!err.contains("sk-"), "错误面含 sk- 串(R-5): {err}");
        // 持久事件:agent.failed 带同码;无 agent.completed。
        let types = event_types(&state, &session.id);
        assert_eq!(types.last().unwrap(), "agent.failed", "{types:?}");
        let failed = state
            .events
            .persisted(&session.id)
            .into_iter()
            .find(|e| e.event_type == "agent.failed")
            .unwrap();
        assert!(failed.payload["error"]
            .as_str()
            .unwrap()
            .starts_with("OPENAI_COMPAT_NOT_CONFIGURED"));
        assert_eq!(state.runs.get(v["run"]["id"].as_str().unwrap()).unwrap().status, "failed");
        std::env::remove_var("FORGE_GEN_DATA_DIR");
        std::fs::remove_dir_all(&dir).ok();
        std::fs::remove_dir_all(&state_dir).ok();
    }

    #[tokio::test]
    async fn build_full_event_sequence_and_run_terminal() {
        let (state, dir) = test_state("build");
        let session = state.sessions.create("t", "coding", None, true);
        let step = scripted_step(
            vec![
                tool_call_msg("mcp__engine-scene__entity_list", "{}"),
                final_msg("完成:已列出实体"),
            ],
            None,
        );
        let execute = ok_executor();
        let out = execute_turn(
            &state,
            &session,
            turn_input("build", "列出实体", vec![], step.as_ref(), execute.as_ref()),
        )
        .await;
        assert_eq!(out.status, "completed");
        assert_eq!(out.text, "完成:已列出实体");
        assert_eq!(
            event_types(&state, &session.id),
            vec![
                "composer.user.message",
                "agent.started",
                "agent.tool.invoked",
                "agent.tool.completed",
                "agent.message",
                "agent.completed",
            ]
        );
        let evs = state.events.persisted(&session.id);
        // payload 面:toolCallId/runId/durationMs/ok。
        let invoked = evs.iter().find(|e| e.event_type == "agent.tool.invoked").unwrap();
        assert_eq!(invoked.payload["name"], "mcp__engine-scene__entity_list");
        assert_eq!(invoked.payload["runId"], out.run_id.as_str());
        assert!(invoked.payload["toolCallId"].as_str().unwrap().starts_with("call_"));
        let completed = evs
            .iter()
            .find(|e| e.event_type == "agent.tool.completed")
            .unwrap();
        assert_eq!(completed.payload["ok"], true);
        assert!(completed.payload["durationMs"].as_u64().is_some(), "durationMs≥0");
        let msg = evs.iter().find(|e| e.event_type == "agent.message").unwrap();
        assert_eq!(msg.payload["provider"], "mock");
        assert_eq!(msg.payload["text"], "完成:已列出实体");
        // run 终态 + activeRunId 清理。
        let run = state.runs.get(&out.run_id).unwrap();
        assert_eq!(run.status, "completed");
        assert_eq!(run.trigger, "composer_chat");
        assert!(run.id.starts_with("run_"));
        assert!(state.sessions.get(&session.id).unwrap().active_run_id.is_none());
        std::fs::remove_dir_all(&dir).ok();
    }

    #[tokio::test]
    async fn ask_mode_empty_tools_and_zero_tool_events() {
        let (state, dir) = test_state("ask");
        let session = state.sessions.create("t", "coding", None, true);
        let seen = Arc::new(Mutex::new(Vec::new()));
        let step = scripted_step(vec![final_msg("纯对话答")], Some(seen.clone()));
        // executor 若被调用即panic 语义:记录调用。
        let calls = Arc::new(Mutex::new(Vec::<String>::new()));
        let calls2 = calls.clone();
        let execute: Box<ExecFn> = Box::new(move |n, _a| {
            calls2.lock().unwrap().push(n);
            Box::pin(async move { (true, "x".into()) })
        });
        // 调用方即便误传 tools,ask 模式也应给 provider 空集。
        let tools = vec![json!({"type":"function","function":{"name":"mcp__engine-scene__entity_list"}})];
        let out = execute_turn(
            &state,
            &session,
            turn_input("ask", "你好吗", tools, step.as_ref(), execute.as_ref()),
        )
        .await;
        assert_eq!(out.status, "completed");
        assert_eq!(seen.lock().unwrap()[0].len(), 0, "ask provider tools=空");
        assert!(calls.lock().unwrap().is_empty());
        let types = event_types(&state, &session.id);
        assert!(!types.iter().any(|t| t == "agent.tool.invoked"), "零 tool.invoked: {types:?}");
        assert_eq!(
            types,
            vec!["composer.user.message", "agent.started", "agent.message", "agent.completed"]
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    #[tokio::test]
    async fn plan_mode_strips_write_tools_and_forbidden_gate() {
        let (state, dir) = test_state("plan");
        let session = state.sessions.create("t", "coding", None, true);
        // 按 KNOWN_TOOLS 全量构造 provider tools(对照集合)。
        let all_tools: Vec<Value> = crate::mcp::KNOWN_TOOLS
            .iter()
            .map(|n| json!({ "name": n, "description": "d" }))
            .collect();
        let openai = llm::to_openai_tools(&all_tools);
        let seen = Arc::new(Mutex::new(Vec::new()));
        // scripted fake 强发写工具调用(验证第二侧门)。
        let step = scripted_step(
            vec![
                tool_call_msg("mcp__engine-scene__entity_create", r#"{"name":"x"}"#),
                final_msg("计划如下"),
            ],
            Some(seen.clone()),
        );
        let executed = Arc::new(Mutex::new(Vec::<String>::new()));
        let executed2 = executed.clone();
        let execute: Box<ExecFn> = Box::new(move |n, _a| {
            executed2.lock().unwrap().push(n);
            Box::pin(async move { (true, "ok".into()) })
        });
        let out = execute_turn(
            &state,
            &session,
            turn_input("plan", "做个计划", openai, step.as_ref(), execute.as_ref()),
        )
        .await;
        assert_eq!(out.status, "completed");
        // provider 收到的 tools 无写工具(对照 WRITE_TOOLS)。
        let got = &seen.lock().unwrap()[0];
        assert!(!got.is_empty(), "只读工具集非空");
        for n in got {
            assert!(!is_write_tool(n), "plan tools 含写工具 {n}");
        }
        assert!(got.contains(&"mcp__engine-scene__entity_list".to_string()), "只读工具保留");
        // 强发写工具 → TOOL_FORBIDDEN 且 executor 未被调用。
        assert!(executed.lock().unwrap().is_empty(), "写工具不得执行");
        let evs = state.events.persisted(&session.id);
        let failed = evs
            .iter()
            .find(|e| e.event_type == "agent.tool.failed")
            .expect("agent.tool.failed 须在");
        assert!(
            failed.payload["error"].as_str().unwrap().starts_with("TOOL_FORBIDDEN"),
            "{}",
            failed.payload["error"]
        );
        assert_eq!(failed.payload["name"], "mcp__engine-scene__entity_create");
        assert!(evs.iter().all(|e| e.event_type != "agent.tool.completed"), "无 completed");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[tokio::test]
    async fn tool_failure_event_honest_and_turn_completes() {
        let (state, dir) = test_state("toolfail");
        let session = state.sessions.create("t", "coding", None, true);
        let step = scripted_step(
            vec![
                tool_call_msg("mcp__engine-scene__entity_get", "{}"),
                final_msg("部分失败收尾"),
            ],
            None,
        );
        let execute: Box<ExecFn> =
            Box::new(|_n, _a| Box::pin(async move { (false, "executor 假失败".into()) }));
        let out = execute_turn(
            &state,
            &session,
            turn_input("build", "查实体", vec![], step.as_ref(), execute.as_ref()),
        )
        .await;
        assert_eq!(out.status, "completed", "工具失败不打断 turn");
        assert_eq!(out.text, "部分失败收尾");
        let evs = state.events.persisted(&session.id);
        let failed = evs
            .iter()
            .find(|e| e.event_type == "agent.tool.failed")
            .expect("agent.tool.failed 须在");
        assert!(failed.payload["error"].as_str().unwrap().contains("executor 假失败"));
        assert_eq!(event_types(&state, &session.id).last().unwrap(), "agent.completed");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[tokio::test]
    async fn cancel_mid_loop_terminal_state() {
        let (state, dir) = test_state("cancel");
        let session = state.sessions.create("t", "coding", None, true);
        // 多迭代 script:恒产工具调用;executor 首调后取消该会话 running run → 次迭代开头收束。
        let step = scripted_step(
            vec![
                tool_call_msg("mcp__engine-scene__entity_list", "{}"),
                tool_call_msg("mcp__engine-scene__entity_list", "{}"),
                final_msg("不应到达"),
            ],
            None,
        );
        let registry = state.runs.clone();
        let sid = session.id.clone();
        let execute: Box<ExecFn> = Box::new(move |_n, _a| {
            registry.cancel_active_for_session(&sid);
            Box::pin(async move { (true, "ok".into()) })
        });
        let out = execute_turn(
            &state,
            &session,
            turn_input("build", "循环", vec![], step.as_ref(), execute.as_ref()),
        )
        .await;
        assert_eq!(out.status, "cancelled");
        assert_eq!(out.text, "");
        let types = event_types(&state, &session.id);
        assert_eq!(types.last().unwrap(), "agent.cancelled", "{types:?}");
        assert!(!types.iter().any(|t| t == "agent.completed"));
        let run = state.runs.get(&out.run_id).unwrap();
        assert_eq!(run.status, "cancelled");
        assert!(state.sessions.get(&session.id).unwrap().active_run_id.is_none());
        std::fs::remove_dir_all(&dir).ok();
    }

    #[tokio::test]
    async fn usage_event_emitted_with_usage_absent_without() {
        let (state, dir) = test_state("usage");
        let session = state.sessions.create("t", "coding", None, true);
        let step: Box<StepFn> = Box::new(|_m, _t| {
            Box::pin(async move {
                Ok(StepOutcome {
                    message: final_msg("ok"),
                    usage: Some(Usage {
                        prompt_tokens: 10,
                        completion_tokens: 5,
                        total_tokens: 15,
                    }),
                })
            })
        });
        let execute = ok_executor();
        let out = execute_turn(
            &state,
            &session,
            turn_input("build", "x", vec![], step.as_ref(), execute.as_ref()),
        )
        .await;
        assert_eq!(out.status, "completed");
        let evs = state.events.persisted(&session.id);
        let usage = evs
            .iter()
            .find(|e| e.event_type == "agent.usage")
            .expect("带 usage → agent.usage 事件");
        assert_eq!(usage.payload["promptTokens"], 10);
        assert_eq!(usage.payload["completionTokens"], 5);
        assert_eq!(usage.payload["totalTokens"], 15);
        assert_eq!(usage.payload["provider"], "mock");
        assert_eq!(usage.payload["runId"], out.run_id.as_str());
        std::fs::remove_dir_all(&dir).ok();
    }

    #[tokio::test]
    async fn multitask_hit_swarm_events_and_miss_failed() {
        let (state, dir) = test_state("mt");
        let session = state.sessions.create("t", "coding", None, true);
        // fake executor:entity_list 回 3 实体;component_add 假成功。
        let execute: Box<ExecFn> = Box::new(|name, _a| {
            Box::pin(async move {
                if name == "mcp__engine-scene__entity_list" {
                    (true, r#"{"entities":[{"id":1},{"id":2},{"id":3}]}"#.to_string())
                } else {
                    (true, r#"{"ok":true}"#.to_string())
                }
            })
        });
        let step: Box<StepFn> = Box::new(|_m, _t| {
            Box::pin(async move { Ok(StepOutcome { message: final_msg("不应调用 LLM"), usage: None }) })
        });
        let out = execute_turn(
            &state,
            &session,
            turn_input("multitask", "给所有方块加碰撞体", vec![], step.as_ref(), execute.as_ref()),
        )
        .await;
        assert_eq!(out.status, "completed");
        assert!(out.text.contains("成功 3"), "聚合文本: {}", out.text);
        assert!(out.text.contains("失败 0"));
        let evs = state.events.persisted(&session.id);
        let invoked = evs
            .iter()
            .find(|e| e.event_type == "agent.tool.invoked")
            .expect("swarm.execute invoked");
        assert_eq!(invoked.payload["name"], "swarm.execute");
        assert_eq!(invoked.payload["args"]["shardType"], "scene-partition");
        assert_eq!(invoked.payload["args"]["shardCount"], 4);
        let completed = evs
            .iter()
            .find(|e| e.event_type == "agent.tool.completed")
            .expect("swarm.execute completed");
        assert_eq!(completed.payload["name"], "swarm.execute");
        let msg = evs.iter().find(|e| e.event_type == "agent.message").unwrap();
        assert!(msg.payload["text"].as_str().unwrap().contains("swarm 分片聚合"));
        // swarm 协调器分片落账(3 实体 4→3 片,全 done)。
        let st = state.swarm.state_json();
        let shards = st["shards"].as_array().unwrap();
        assert_eq!(shards.len(), 3);
        assert!(shards.iter().all(|s| s["status"] == "done"));

        // 未命中腿:agent.failed「模板未命中」,run failed,无 tool.invoked。
        let session2 = state.sessions.create("t2", "coding", None, true);
        let out2 = execute_turn(
            &state,
            &session2,
            turn_input("multitask", "随便聊聊", vec![], step.as_ref(), execute.as_ref()),
        )
        .await;
        assert_eq!(out2.status, "failed");
        assert!(out2.error.as_deref().unwrap().contains("模板未命中"));
        let types2 = event_types(&state, &session2.id);
        assert_eq!(types2.last().unwrap(), "agent.failed");
        assert!(!types2.iter().any(|t| t == "agent.tool.invoked"));
        assert_eq!(state.runs.get(&out2.run_id).unwrap().status, "failed");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[tokio::test]
    async fn auto_title_first_message_48_chars_and_protection() {
        let (state, dir) = test_state("title");
        let session = state.sessions.create("新会话", "coding", None, true);
        let execute = ok_executor();
        // 60 字输入 → 截 48。
        let long_input = "一二三四五六七八九十".repeat(6);
        let step = scripted_step(vec![final_msg("a"), final_msg("b"), final_msg("c")], None);
        execute_turn(
            &state,
            &session,
            turn_input("build", &long_input, vec![], step.as_ref(), execute.as_ref()),
        )
        .await;
        let t1 = state.sessions.get(&session.id).unwrap();
        let expect: String = long_input.chars().take(48).collect();
        assert_eq!(t1.title, expect);
        assert_eq!(t1.title.chars().count(), 48);
        assert!(!t1.title_manually_set, "自动命名不置手动旗");
        // 第二条消息不改名。
        execute_turn(
            &state,
            &t1,
            turn_input("build", "第二条完全不同", vec![], step.as_ref(), execute.as_ref()),
        )
        .await;
        assert_eq!(state.sessions.get(&session.id).unwrap().title, expect);
        // titleManuallySet=true 保护:新会话手动命名后首条消息不改名。
        let s2 = state.sessions.create("手动题", "coding", None, true);
        let s2 = state
            .sessions
            .patch(
                &s2.id,
                &crate::sessions::PatchSessionRequest {
                    title: Some("手动题-改".to_string()),
                    ..Default::default()
                },
            )
            .unwrap();
        assert!(s2.title_manually_set);
        execute_turn(
            &state,
            &s2,
            turn_input("build", "首条消息内容", vec![], step.as_ref(), execute.as_ref()),
        )
        .await;
        assert_eq!(state.sessions.get(&s2.id).unwrap().title, "手动题-改");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[tokio::test]
    async fn snapshot_todos_and_run_filled_honest_null() {
        let (state, dir) = test_state("snap");
        let session = state.sessions.create("t", "coding", None, true);
        state
            .todos
            .create(&session.id, "任务甲", None, None)
            .unwrap();
        // running run 挂 activeRunId → snapshot.run 填真。
        let (run, _tok) = state.runs.begin(&session.id, "composer_chat");
        let mut s = state.sessions.get(&session.id).unwrap();
        s.active_run_id = Some(run.id.clone());
        s.touch();
        state.sessions.save(&s);
        let snap = |state: &Arc<AppState>, sid: &str| {
            let st = state.clone();
            let sid = sid.to_string();
            async move {
                crate::snapshot::design_snapshot(
                    State(st),
                    axum::extract::Query(crate::snapshot::SnapshotQuery {
                        session_id: Some(sid),
                    }),
                )
                .await
                .0
            }
        };
        let v = snap(&state, &session.id).await;
        let todos = v["todos"].as_array().unwrap();
        assert_eq!(todos.len(), 1);
        assert_eq!(todos[0]["title"], "任务甲");
        assert_eq!(todos[0]["kind"], "edit");
        assert_eq!(todos[0]["source"], "user");
        assert_eq!(todos[0]["status"], "queued");
        assert_eq!(v["run"]["id"], run.id.as_str());
        assert_eq!(v["run"]["status"], "running");
        assert_eq!(v["run"]["trigger"], "composer_chat");
        // activeRunId 指向注册表丢失 run → null 如实。
        let mut s2 = state.sessions.get(&session.id).unwrap();
        s2.active_run_id = Some("run_missing".to_string());
        state.sessions.save(&s2);
        let v2 = snap(&state, &session.id).await;
        assert!(v2["run"].is_null(), "注册表丢失 → null: {v2}");
        // 清 activeRunId → null;无会话 → todos []。
        let mut s3 = state.sessions.get(&session.id).unwrap();
        s3.active_run_id = None;
        state.sessions.save(&s3);
        let v3 = snap(&state, &session.id).await;
        assert!(v3["run"].is_null());
        let v4 = snap(&state, "sess_none").await;
        assert_eq!(v4["todos"], json!([]));
        assert!(v4["run"].is_null());
        // TodoStore 持久化:重启 load 同路径恢复。
        let store2 = TodoStore::load(dir.join("agent-sessions").join("todos.json"));
        let list = store2.list_by_session(&session.id);
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].title, "任务甲");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn write_tools_subset_of_known_tools() {
        for w in WRITE_TOOLS {
            assert!(
                crate::mcp::KNOWN_TOOLS.contains(w),
                "WRITE_TOOLS 含 KNOWN_TOOLS 未登记名: {w}"
            );
        }
        // 抽查:只读侧不得误收(entity_list/scene_summary/viewport_frame 等)。
        for r in [
            "mcp__engine-scene__entity_list",
            "mcp__engine-scene__scene_summary",
            "mcp__engine-scene__viewport_frame",
            "mcp__asset-pipeline__asset_list",
            "mcp__code-forge__rx_check",
            "mcp__gen-image__gen_backends_list",
        ] {
            assert!(!is_write_tool(r), "{r} 应为只读");
        }
        // 契约点名写工具逐项在册。
        for w in [
            "mcp__engine-scene__entity_create",
            "mcp__engine-scene__entity_destroy",
            "mcp__engine-scene__entity_rename",
            "mcp__engine-scene__transform_set",
            "mcp__engine-scene__component_add",
            "mcp__engine-scene__component_remove",
            "mcp__engine-scene__component_set",
            "mcp__engine-scene__play_enter",
            "mcp__engine-scene__play_pause",
            "mcp__engine-scene__play_resume",
            "mcp__engine-scene__play_step",
            "mcp__engine-scene__play_exit",
            "mcp__engine-scene__edit_undo",
            "mcp__engine-scene__edit_redo",
            "mcp__engine-scene__scene_save",
            "mcp__engine-scene__scene_load",
            "mcp__asset-pipeline__asset_import",
            "mcp__asset-pipeline__asset_reimport",
            "mcp__asset-pipeline__asset_move",
            "mcp__asset-pipeline__asset_delete",
            "mcp__code-forge__graph_create",
            "mcp__gen-image__gen_image",
            "mcp__gen-image__gen_accept",
        ] {
            assert!(is_write_tool(w), "{w} 应判写");
        }
    }
}
