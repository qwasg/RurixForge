//! 一整轮 Codex 执行的编排。
//!
//! 与本地引擎的 [`execute_turn`](crate::agent::execute_turn) 是同一套生命周期:
//! `runs.begin` → 认领 `activeRunId` → `composer.user.message` → `agent.started` →
//! 中间事件 → `agent.message` → `agent.completed|failed|cancelled` → 释放。
//! 差别只在中间那段:本地引擎自己跑工具循环,这里把活交给 `codex app-server`,
//! 再把它推回来的通知翻成同样的事件([map](super::map))。
//!
//! 结果是:会话列表、时间线、审批卡、Plan 页签、待办面板、用量统计对两种引擎完全一致,
//! 用户切换引擎时界面不会换一副样子。

use serde_json::{json, Value};
use std::sync::Arc;

use super::map::{Action, Mapper};
use super::rpc::{CodexError, Inbound};
use crate::agent::{CancelToken, TurnOutput};
use crate::events::EventDraft;
use crate::scope::ScopeContext;
use crate::sessions::DebugSession;
use crate::AppState;

/// Codex 引擎支持的 composer 模式。
///
/// team / multitask 不在列:那两个模式的实现是本仓的编排器(结构化计划 DAG 分层派发、
/// 异步回执唤醒),跑在 Forge 进程里,不是「给模型一组工具」那么简单。硬映到 Codex 上
/// 只会得到一个假装在派单的单体 agent,所以显式拒绝而不是悄悄降级成 build。
pub const CODEX_MODES: [&str; 4] = ["ask", "build", "debug", "plan"];

pub fn mode_supported(mode: &str) -> bool {
    CODEX_MODES.contains(&mode)
}

/// 一轮的输入。
pub struct CodexTurnInput<'a> {
    pub user_input: &'a str,
    pub mode: &'a str,
    /// composer 勾选的技能名(转成 Codex 的 skill input 条目)。
    pub skills: &'a [String],
    /// Plan 页签 Build 下发的计划全文(有则整篇作为本轮任务书)。
    pub plan_body: Option<String>,
    pub scope: ScopeContext,
}

/// 给 Codex 的项目约定。
///
/// Codex 不知道自己正跑在一个游戏引擎编辑器里:它看到一堆叫 `engine-scene` 的 MCP 工具,
/// 但不知道该优先用它们而不是自己去读写场景文件,也不知道 `viewport_frame` 就是它的
/// 「看屏幕」。这段说明就是补这层认知的,缺了它 Codex 会退化成一个只会 shell + 编辑器的
/// 通用 coding agent。
fn developer_instructions(scope: &ScopeContext, computer_use: bool) -> String {
    let mut s = String::from(
        "你正作为 RurixForge(AI 优先的游戏引擎与编辑器)的内置 agent 工作。\n\
         思考/推理过程用英文;面向用户可见的正文、结论、总结统一用中文。\n\n\
         【优先用引擎工具,而不是直接改文件】\n\
         - 场景与实体:`engine-scene` 服务(scene_summary / scene_index / entity_create / \
         component_set / transform_set / scene_save…)。场景是活的运行时状态,\
         直接编辑场景文件会与 engine-host 的内存态打架。\n\
         - 资产:`asset-pipeline`(asset_import / asset_list / asset_set_description / \
         sprite_create…);生成素材用 `gen-image` / `gen-model`。\n\
         - 代码与逻辑图:`code-forge`(rx_check / rx_build / rx_test / graph_* / code_*)。\n\
         - 检索优先:找素材/实体/代码/文档先用 `context` 服务的 context_search 定位,\
         命中不足再全量遍历;返回 INDEX_NOT_BUILT 时先 context_index_build。\n\
         - 跨项目资源与项目清单:宿主工具 `forge.project_list` / `forge.resource_search` / \
         `forge.resource_get`。\n\n\
         【游戏视口就是你的 Computer Use】\n\
         要看游戏画面、点游戏里的东西,用 engine-scene 的这几个工具,别去截桌面:\n\
         - `viewport_frame` 取当前帧(等于「看一眼屏幕」);\n\
         - `viewport_pick` 按归一化视口坐标问「这个位置是哪个实体」;\n\
         - `logic_inject_pointer` / `logic_inject_input` 注入点击与按键(等于「动手操作」);\n\
         - `play_enter` / `play_pause` / `play_step` / `play_exit` 控制播放态。\n\
         验证玩法的标准流程:play_enter → 注入输入 → viewport_frame 看结果 → play_exit。\n",
    );
    if computer_use {
        s.push_str(
            "\n【桌面级 Computer Use】\n\
             `computer-use` 服务能看/点/输入**整个桌面**(list_apps / screenshot / click / \
             type_text / press_key / scroll)。它只用于引擎之外的事(比如核对外部工具的界面);\
             凡是游戏内的操作一律走上面的视口工具——桌面截图里的游戏画面是二手的,\
             坐标换算还会出错。\n",
        );
    }
    s.push_str(&format!(
        "\n【工作目录与项目】\n\
         当前项目根:{}\n工作区根:{}\n\
         写入一律落当前项目;其他项目只读。\n",
        scope.current.project_root.display(),
        scope.current.workspace_root.display()
    ));
    s
}

/// Forge 权限模式 → Codex 的审批策略与沙箱。
///
/// 对齐口径:本地引擎的 `bypass` 是「不问直接干」,`auto` 是「写操作要批」,
/// `plan` 是「只读」。Codex 侧分别对应 never/on-request 与 workspaceWrite/readOnly。
pub fn approval_for(permission_mode: &str) -> (&'static str, &'static str) {
    match permission_mode {
        "plan" => ("never", "readOnly"),
        "auto" => ("on-request", "workspaceWrite"),
        _ => ("never", "workspaceWrite"),
    }
}

/// 执行一整轮。
pub async fn execute_codex_turn(
    state: &Arc<AppState>,
    session: &DebugSession,
    input: CodexTurnInput<'_>,
) -> TurnOutput {
    let sid = session.id.as_str();
    let (run, token) = state.runs.begin(sid, "composer_chat");
    let run_id = run.id.clone();
    if let Err(busy) = state.sessions.claim_active_run(sid, &run_id) {
        state.runs.finish(&run_id, "failed");
        return TurnOutput {
            run_id,
            status: "failed".to_string(),
            text: String::new(),
            error: Some(format!(
                "SESSION_BUSY: 会话已有运行中的 run({busy}),请等待完成或中止"
            )),
        };
    }

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
    if first_message && !session.title_manually_set {
        if let Some(mut s2) = state.sessions.get(sid) {
            s2.title = input.user_input.chars().take(48).collect();
            s2.touch();
            state.sessions.save(&s2);
        }
    }

    let model = model_for(session);
    let mut started = json!({
        "runId": run_id,
        "engine": "codex",
        "model": model.clone().unwrap_or_else(|| "codex 默认模型".to_string()),
    });
    if let Some(obj) = started.as_object_mut() {
        if let Some(scope_obj) = crate::scope::summary_json(&input.scope).as_object() {
            for (k, v) in scope_obj {
                obj.insert(k.clone(), v.clone());
            }
        }
    }
    state
        .events
        .emit(EventDraft::new(sid, "agent.started", "agent").payload(started));

    let outcome = drive(state, session, &input, &run_id, &token, model.as_deref()).await;
    finish(state, session, &run_id, outcome)
}

/// 一轮的三态结果。
struct Outcome {
    status: String,
    text: String,
    error: Option<String>,
}

fn finish(
    state: &Arc<AppState>,
    session: &DebugSession,
    run_id: &str,
    outcome: Outcome,
) -> TurnOutput {
    let sid = session.id.as_str();
    match outcome.status.as_str() {
        "completed" => {
            state.events.emit(
                EventDraft::new(sid, "agent.message", "agent").payload(json!({
                    "text": outcome.text,
                    "runId": run_id,
                    "provider": "codex",
                    "degraded": false,
                })),
            );
            state.events.emit(
                EventDraft::new(sid, "agent.completed", "agent")
                    .payload(json!({ "runId": run_id, "text": outcome.text })),
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
                    "error": outcome.error.clone().unwrap_or_default(),
                })),
            );
        }
    }
    state.runs.finish(run_id, &outcome.status);
    state.sessions.release_active_run(sid, run_id);
    TurnOutput {
        run_id: run_id.to_string(),
        status: outcome.status,
        text: outcome.text,
        error: outcome.error,
    }
}

/// 会话选中的模型 id → Codex 模型名。`codex:` 前缀是模型目录里给 Codex 条目加的
/// 命名空间(见 [modelspec](crate::modelspec)),发给 Codex 前要剥掉。
fn model_for(session: &DebugSession) -> Option<String> {
    let cfg = super::config::load();
    let picked = session
        .selected_model_id
        .as_deref()
        .and_then(|id| id.strip_prefix("codex:"))
        .map(str::to_string);
    picked.or_else(|| Some(cfg.default_model).filter(|m| !m.is_empty()))
}

async fn drive(
    state: &Arc<AppState>,
    session: &DebugSession,
    input: &CodexTurnInput<'_>,
    run_id: &str,
    token: &CancelToken,
    model: Option<&str>,
) -> Outcome {
    let sid = session.id.as_str();
    let client = match state.codex.ready_client().await {
        Ok(c) => c,
        Err(e) => return failed(e.0),
    };
    // 订阅必须早于 thread/start:线程的头几条通知可能比 start 的响应先到。
    let mut inbox = client.subscribe();
    let mut mapper = Mapper::new(run_id);

    let thread_id = match ensure_thread(state, session, input).await {
        Ok(id) => id,
        Err(e) => return failed(e.0),
    };
    mapper.bind_thread(&thread_id);

    // 目标:本会话若已设目标,推给 Codex 让它自己的 goal 循环接管推进。
    if let Some(goal) = state.goals.get(sid).filter(|g| g.is_active()) {
        if let Err(e) = super::goal_set(state, &thread_id, &goal).await {
            eprintln!("[codex] thread/goal/set 失败(目标仍在 Forge 侧记录): {e}");
        }
    }

    let turn_params = turn_start_params(&thread_id, input, model, session);
    if let Err(e) = client.request("turn/start", turn_params).await {
        return failed(e.0);
    }

    let mut end: Option<Outcome> = None;
    let mut interrupt_sent = false;
    while end.is_none() {
        // 取消是本地状态,不会经通知流到来,所以每次等消息都带一个短超时去轮询它。
        let next = tokio::time::timeout(std::time::Duration::from_millis(250), inbox.recv()).await;
        match next {
            Err(_) => {
                if token.is_cancelled() && !interrupt_sent {
                    interrupt_sent = true;
                    let _ = client.notify(
                        "turn/interrupt",
                        json!({ "threadId": thread_id, "reason": "user" }),
                    );
                }
                continue;
            }
            // 连接断了:本轮无从收尾,如实失败(下一轮 ensure_started 会重连)。
            Ok(None) => {
                end = Some(Outcome {
                    status: if token.is_cancelled() {
                        "cancelled".to_string()
                    } else {
                        "failed".to_string()
                    },
                    text: mapper.final_text(),
                    error: if token.is_cancelled() {
                        None
                    } else {
                        Some("codex app-server 连接中断,本轮未能完成".to_string())
                    },
                });
            }
            Ok(Some(msg)) => {
                if !mapper.owns(msg.params()) {
                    continue;
                }
                match msg {
                    Inbound::Notification { method, params } => {
                        for action in mapper.handle(&method, &params) {
                            if let Some(o) = apply(state, session, run_id, &mapper, action) {
                                end = Some(o);
                            }
                        }
                    }
                    Inbound::ServerRequest { id, method, params } => {
                        serve_request(state, session, run_id, &client, id, method, params);
                    }
                }
            }
        }
    }

    let mut out = end.expect("循环只在拿到终态时退出");
    if out.text.is_empty() {
        out.text = mapper.final_text();
    }
    // 用户按了停止但 Codex 已经自己收尾成 completed:以用户意图为准显示成已中止,
    // 免得界面上出现「点了停止却显示正常完成」。
    if token.is_cancelled() && out.status == "completed" {
        out.status = "cancelled".to_string();
    }
    out
}

fn failed(error: String) -> Outcome {
    Outcome {
        status: "failed".to_string(),
        text: String::new(),
        error: Some(error),
    }
}

/// 执行一条映射产物。返回 `Some` = 本轮到此结束。
fn apply(
    state: &Arc<AppState>,
    session: &DebugSession,
    run_id: &str,
    mapper: &Mapper,
    action: Action,
) -> Option<Outcome> {
    let sid = session.id.as_str();
    match action {
        Action::Emit {
            event_type,
            domain,
            payload,
        } => {
            state.events.emit(
                EventDraft::new(sid, event_type, domain)
                    .payload(payload)
                    .correlation(Some(run_id.to_string())),
            );
            None
        }
        Action::Stream {
            event_type,
            payload,
        } => {
            crate::engine::emit_stream_delta(&state.events, sid, run_id, &event_type, payload);
            None
        }
        Action::WritePlan {
            name,
            overview,
            body,
        } => {
            write_plan(state, session, run_id, &name, &overview, &body);
            None
        }
        Action::SyncTodos(steps) => {
            sync_todos(state, sid, run_id, &steps);
            None
        }
        Action::TurnEnd { status, error } => Some(Outcome {
            status,
            text: mapper.final_text(),
            error,
        }),
    }
}

/// Codex 的计划 → 本仓的计划文件(走 plan_doc 那条既有腿,于是 Plan 页签、
/// 会话 activePlanPath 与「Build」按钮对两种引擎完全一致)。
fn write_plan(
    state: &Arc<AppState>,
    session: &DebugSession,
    run_id: &str,
    name: &str,
    overview: &str,
    body: &str,
) {
    let ws_root = crate::scope::workspace_root_for(state, session.workspace_id.as_deref());
    // 待办取自 TodoStore 里 codex 来源的条目(它们由 turn/plan/updated 建立)。
    // 计划文件的 front matter 要求 todos 非空,而 Codex 的计划正文与步骤表是两条通知,
    // 步骤还没到就先用一条占位待办兜住,后续 turn/plan/updated 会把真步骤补上。
    let todos: Vec<Value> = state
        .todos
        .list_by_session(&session.id)
        .into_iter()
        .filter(|t| t.source == "codex")
        .enumerate()
        .map(|(i, t)| json!({ "id": format!("codex-{}", i + 1), "content": t.title }))
        .collect();
    let todos = if todos.is_empty() {
        vec![json!({ "id": "codex-1", "content": "按计划正文实施" })]
    } else {
        todos
    };
    let (ok, msg) = crate::plan_doc::handle_create_plan(
        &ws_root,
        &state.events,
        &state.sessions,
        &session.id,
        run_id,
        &json!({
            "name": name,
            "overview": overview,
            "plan": body,
            "todos": todos,
        }),
    );
    if !ok {
        eprintln!("[codex] 计划落盘失败: {msg}");
    }
}

/// `turn/plan/updated` 的步骤表 → TodoStore。
///
/// 按标题对账而不是全量重建:重建会让每次步骤状态变化都把待办 id 换一批,
/// 前端的待办条目会整列闪掉,已有的展开/滚动状态全丢。
fn sync_todos(state: &Arc<AppState>, session_id: &str, run_id: &str, steps: &[super::map::PlanStep]) {
    let existing = state.todos.list_by_session(session_id);
    for step in steps {
        let status = todo_status_for(&step.status);
        match existing
            .iter()
            .find(|t| t.source == "codex" && t.title == step.step)
        {
            Some(prev) => {
                if prev.status == status {
                    continue;
                }
                if let Ok(todo) = state.todos.patch(
                    &prev.id,
                    &crate::agent::PatchTodoRequest {
                        status: Some(status.to_string()),
                        ..Default::default()
                    },
                ) {
                    state.events.emit(
                        EventDraft::new(session_id, "todo.updated", "todo").payload(json!({
                            "id": todo.id, "title": todo.title, "status": todo.status,
                            "runId": run_id,
                        })),
                    );
                }
            }
            None => {
                let created = state.todos.create(
                    session_id,
                    crate::agent::NewTodo {
                        title: step.step.clone(),
                        source: Some("codex".to_string()),
                        ..Default::default()
                    },
                );
                if let Ok(todo) = created {
                    state.events.emit(
                        EventDraft::new(session_id, "todo.created", "todo").payload(json!({
                            "id": todo.id, "title": todo.title, "kind": todo.kind,
                            "status": todo.status, "source": todo.source, "runId": run_id,
                        })),
                    );
                    if status != todo.status {
                        if let Ok(t2) = state.todos.patch(
                            &todo.id,
                            &crate::agent::PatchTodoRequest {
                                status: Some(status.to_string()),
                                ..Default::default()
                            },
                        ) {
                            state.events.emit(
                                EventDraft::new(session_id, "todo.updated", "todo").payload(
                                    json!({
                                        "id": t2.id, "title": t2.title, "status": t2.status,
                                        "runId": run_id,
                                    }),
                                ),
                            );
                        }
                    }
                }
            }
        }
    }
}

/// Codex 的步骤状态 → 本仓 todo 状态词汇。
pub fn todo_status_for(codex_status: &str) -> &'static str {
    match codex_status {
        "in_progress" | "inProgress" | "running" => "running",
        "completed" | "done" => "completed",
        "failed" | "blocked" => "failed",
        _ => "queued",
    }
}

/// 服务端请求分发。审批要等人回话(可能几分钟),dynamicTool 要跑 MCP,
/// 两者都不能堵住读循环 —— 否则同一轮里后续的通知全部积压,界面看着像卡死了。
fn serve_request(
    state: &Arc<AppState>,
    session: &DebugSession,
    run_id: &str,
    client: &Arc<super::rpc::CodexClient>,
    id: Value,
    method: String,
    params: Value,
) {
    let state = Arc::clone(state);
    let client = Arc::clone(client);
    let sid = session.id.clone();
    let run_id = run_id.to_string();
    let scope_ws = session.workspace_id.clone();
    tokio::spawn(async move {
        if let Some(approval) = super::approvals::classify(&method, &params) {
            let decision = state
                .permissions
                .request_decision(&state.events, &sid, &run_id, approval.kind, approval.payload)
                .await;
            let result = match decision {
                Ok(d) => super::approvals::to_codex_result(approval.reply, &d),
                // 审批通道断了(turn 被中止收尾)→ 回拒绝让 Codex 干净收场。
                Err(_) => super::approvals::abandon_result(approval.reply),
            };
            let _ = client.respond(&id, result);
            return;
        }
        if method == "item/tool/call" || method == "tool/call" {
            let name = params
                .get("name")
                .or_else(|| params.get("tool"))
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string();
            let args = params
                .get("arguments")
                .or_else(|| params.get("args"))
                .cloned()
                .unwrap_or_else(|| json!({}));
            let scope = crate::scope::resolve(&state, scope_ws.as_deref(), &[], true);
            let result = super::tools::call(&state, &scope, &name, &args).await;
            let _ = client.respond(&id, result);
            return;
        }
        // 不认的服务端请求必须回错误而不是沉默:沉默会让那一轮永久挂住。
        let _ = client.respond_error(&id, -32601, &format!("Forge 未实现该请求: {method}"));
    });
}

/// 首轮 `thread/start`,后续轮 `thread/resume`;线程 id 写回会话。
async fn ensure_thread(
    state: &Arc<AppState>,
    session: &DebugSession,
    input: &CodexTurnInput<'_>,
) -> Result<String, CodexError> {
    let client = state.codex.ready_client().await?;
    if let Some(existing) = session.codex_thread_id.as_deref().filter(|t| !t.is_empty()) {
        match client
            .request("thread/resume", json!({ "threadId": existing }))
            .await
        {
            Ok(_) => return Ok(existing.to_string()),
            // 续不上(app-server 重启过、线程已过期)→ 新开一条,而不是让整轮失败。
            Err(e) => eprintln!("[codex] thread/resume 失败({e}),改新开线程"),
        }
    }
    let cfg = super::config::load();
    let permission_mode = state.permissions.mode(&session.id);
    let (approval_policy, sandbox) = approval_for(&permission_mode);
    let params = thread_start_params(input, &cfg, approval_policy, sandbox);
    let out = match client.request("thread/start", params.clone()).await {
        Ok(v) => v,
        Err(e) => {
            // 兜底:某些 codex 版本会拒收 per-thread 的 `config`/`dynamicTools`。
            // 那种情况下「Codex 能跑但没有引擎工具」远好过「整轮直接失败」——
            // 缺了什么如实进事件,用户看得见自己这一轮为什么没有引擎 MCP。
            eprintln!("[codex] thread/start 带 config 被拒({e}),退回精简参数重试");
            let mut lean = params;
            if let Some(obj) = lean.as_object_mut() {
                obj.remove("config");
                obj.remove("dynamicTools");
            }
            state.events.emit(
                EventDraft::new(&session.id, "agent.warning", "agent").payload(json!({
                    "code": "CODEX_THREAD_CONFIG_REJECTED",
                    "message": format!(
                        "当前 codex 版本不接受 per-thread MCP 注入({e});\
                         本轮 Codex 只有自带工具,引擎 MCP 未生效。升级 codex 后重试"
                    ),
                })),
            );
            client.request("thread/start", lean).await?
        }
    };
    let id = out
        .get("thread")
        .and_then(|t| t.get("id"))
        .or_else(|| out.get("threadId"))
        .or_else(|| out.get("id"))
        .and_then(Value::as_str)
        .ok_or_else(|| CodexError("thread/start 响应缺线程 id".to_string()))?
        .to_string();
    if let Some(mut s) = state.sessions.get(&session.id) {
        s.codex_thread_id = Some(id.clone());
        s.touch();
        state.sessions.save(&s);
    }
    Ok(id)
}

/// `thread/start` 参数。
pub fn thread_start_params(
    input: &CodexTurnInput<'_>,
    cfg: &super::config::CodexConfig,
    approval_policy: &str,
    sandbox: &str,
) -> Value {
    json!({
        "cwd": input.scope.current.project_root.to_string_lossy(),
        "approvalPolicy": approval_policy,
        "sandbox": sandbox,
        "config": {
            "mcp_servers": super::mcp_config::mcp_servers_json(
                &input.scope.current.project_root,
                cfg.auto_register_mcp,
            ),
            "developer_instructions": developer_instructions(
                &input.scope,
                crate::mcp::computer_use_enabled(),
            ),
        },
        "dynamicTools": super::tools::declarations(),
    })
}

/// `turn/start` 参数。
pub fn turn_start_params(
    thread_id: &str,
    input: &CodexTurnInput<'_>,
    model: Option<&str>,
    session: &DebugSession,
) -> Value {
    let mut items: Vec<Value> = Vec::new();
    // 技能走结构化条目而不是拼进正文:Codex 自己会读 SKILL.md 全文,
    // 拼进正文只会得到一句「请使用技能 X」而技能内容根本没进上下文。
    for name in input.skills {
        items.push(json!({
            "type": "skill",
            "name": name,
            "path": format!("skills/{name}/SKILL.md"),
        }));
    }
    let mut text = input.user_input.to_string();
    if let Some(plan) = &input.plan_body {
        text.push_str("\n\n【按下列计划实施】\n");
        text.push_str(plan);
    }
    items.push(json!({ "type": "text", "text": text }));

    let mut params = json!({
        "threadId": thread_id,
        "input": items,
    });
    if let Some(m) = model {
        params["model"] = json!(m);
    }
    if let Some(e) = effort_for(session) {
        params["effort"] = json!(e);
    }
    // plan 模式:Codex 自己有协作模式开关,比「用提示词求它别动手」可靠得多。
    if input.mode == "plan" {
        params["collaborationMode"] = json!({ "mode": "plan" });
    }
    if input.mode == "debug" {
        params["input"] = json!(with_debug_hint(items));
    }
    params
}

fn with_debug_hint(mut items: Vec<Value>) -> Vec<Value> {
    items.push(json!({
        "type": "text",
        "text": "当前为 debug 模式:先用只读工具取证定位(viewport_frame / scene_summary / \
                 scene_graph_dump / entity_list / 读日志),确认因果后再动手改;\
                 每步观察与结论如实汇报,不得猜测式修改。",
    }));
    items
}

fn effort_for(session: &DebugSession) -> Option<String> {
    if !session.thinking_enabled {
        return None;
    }
    session.reasoning_effort.clone()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn session() -> DebugSession {
        serde_json::from_value(json!({
            "id": "sess_1", "title": "t", "status": "idle", "agentKind": "coding",
            "webSearchEnabled": true, "createdAt": "", "updatedAt": "",
            "pinned": false, "titleManuallySet": false, "agentEngine": "codex"
        }))
        .unwrap()
    }

    /// 模式门:team/multitask 显式不支持(硬映过去只会得到一个假装在派单的单体 agent)。
    #[test]
    fn only_four_modes_supported() {
        for m in ["ask", "build", "debug", "plan"] {
            assert!(mode_supported(m), "{m} 应支持");
        }
        assert!(!mode_supported("team"));
        assert!(!mode_supported("multitask"));
    }

    /// 权限映射:plan 只读、auto 要批、bypass 放行。
    #[test]
    fn permission_mode_maps_to_codex_policy() {
        assert_eq!(approval_for("plan"), ("never", "readOnly"));
        assert_eq!(approval_for("auto"), ("on-request", "workspaceWrite"));
        assert_eq!(approval_for("bypass"), ("never", "workspaceWrite"));
    }

    /// turn/start:技能进结构化条目、计划全文接在正文后、plan 模式带协作开关。
    #[test]
    fn turn_start_shape() {
        let scope = crate::scope::ScopeContext::single(crate::scope::ScopeProject::for_test(
            "D:/proj",
            "D:/ws",
        ));
        let skills = vec!["game-2d-kit".to_string()];
        let input = CodexTurnInput {
            user_input: "加个二段跳",
            mode: "plan",
            skills: &skills,
            plan_body: Some("## 步骤\n1. 读现有跳跃逻辑".to_string()),
            scope,
        };
        let p = turn_start_params("th_1", &input, Some("gpt-5.6-terra"), &session());
        assert_eq!(p["threadId"], "th_1");
        assert_eq!(p["model"], "gpt-5.6-terra");
        assert_eq!(p["collaborationMode"]["mode"], "plan");
        let items = p["input"].as_array().unwrap();
        assert_eq!(items[0]["type"], "skill");
        assert_eq!(items[0]["name"], "game-2d-kit");
        assert_eq!(items[0]["path"], "skills/game-2d-kit/SKILL.md");
        let text = items.last().unwrap()["text"].as_str().unwrap();
        assert!(text.contains("加个二段跳"), "{text}");
        assert!(text.contains("读现有跳跃逻辑"), "{text}");
    }

    /// 思考没开就不发 effort(发了会被上游按显式设置处理,覆盖模型默认档)。
    #[test]
    fn effort_only_when_thinking_enabled() {
        let mut s = session();
        s.reasoning_effort = Some("high".to_string());
        assert_eq!(effort_for(&s), None, "思考未开不该发 effort");
        s.thinking_enabled = true;
        assert_eq!(effort_for(&s).as_deref(), Some("high"));
    }

    /// 模型 id 的 `codex:` 命名空间在发给 Codex 前要剥掉。
    #[test]
    fn model_namespace_is_stripped() {
        let mut s = session();
        s.selected_model_id = Some("codex:gpt-5.6-terra".to_string());
        assert_eq!(model_for(&s).as_deref(), Some("gpt-5.6-terra"));
    }

    /// 步骤状态词汇对齐 TodoStore(写错会让 patch 直接被 TODO_INVALID 拒掉)。
    #[test]
    fn plan_step_status_maps_to_todo_vocabulary() {
        assert_eq!(todo_status_for("pending"), "queued");
        assert_eq!(todo_status_for("in_progress"), "running");
        assert_eq!(todo_status_for("completed"), "completed");
        for s in ["queued", "running", "completed", "failed"] {
            assert!(
                crate::agent::TODO_STATUSES.contains(&s),
                "{s} 不在 TodoStore 允许的状态里"
            );
        }
    }

    /// developer_instructions 必须把「游戏视口就是 Computer Use」讲清楚,
    /// 否则 Codex 会去截桌面而不用 viewport_frame。
    #[test]
    fn developer_instructions_teach_viewport_computer_use() {
        let scope = crate::scope::ScopeContext::single(crate::scope::ScopeProject::for_test(
            "D:/proj",
            "D:/ws",
        ));
        let s = developer_instructions(&scope, false);
        assert!(s.contains("viewport_frame"), "{s}");
        assert!(s.contains("logic_inject_pointer"), "{s}");
        assert!(s.contains("engine-scene"), "{s}");
        assert!(s.contains("D:/proj") || s.contains("D:\\proj"), "{s}");
        // 关掉桌面 Computer Use 时不该提它(提了模型会去调不存在的工具)。
        assert!(!s.contains("list_apps"), "{s}");
        let with_cu = developer_instructions(&scope, true);
        assert!(with_cu.contains("list_apps"), "{with_cu}");
    }
}
