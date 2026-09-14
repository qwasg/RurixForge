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
         - 跨项目资源与项目清单:宿主工具 `forge__project_list` / `forge__resource_search` / \
         `forge__resource_get`。\n\n\
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
/// `plan` 是「只读」。Codex 线上枚举是 kebab-case，不能使用 Rust 字段常见的 camelCase。
pub fn approval_for(permission_mode: &str) -> (&'static str, &'static str) {
    match permission_mode {
        "plan" => ("never", "read-only"),
        "auto" => ("on-request", "workspace-write"),
        _ => ("never", "workspace-write"),
    }
}

fn approval_for_turn(permission_mode: &str, composer_mode: &str) -> (&'static str, &'static str) {
    if composer_mode == "plan" {
        // Plan is a semantic read-only mode even when the session-wide permission
        // switch is bypass. Enforce it at the server boundary, not just by prompt.
        approval_for("plan")
    } else {
        approval_for(permission_mode)
    }
}

fn sandbox_policy(sandbox: &str) -> Value {
    match sandbox {
        "read-only" => json!({ "type": "readOnly" }),
        "danger-full-access" => json!({ "type": "dangerFullAccess" }),
        _ => json!({ "type": "workspaceWrite" }),
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

    for name in input.skills {
        match crate::skills::find_skill(name) {
            Some(skill) if skill.enabled => {}
            Some(_) => {
                state.events.emit(
                    EventDraft::new(sid, "agent.warning", "agent").payload(json!({
                        "code": "CODEX_SKILL_DISABLED",
                        "message": format!("技能 {name} 已禁用，本轮未注入")
                    })),
                );
            }
            None => {
                state.events.emit(
                    EventDraft::new(sid, "agent.warning", "agent").payload(json!({
                        "code": "CODEX_SKILL_NOT_FOUND",
                        "message": format!("技能 {name} 不存在或不可读，本轮未注入")
                    })),
                );
            }
        };
    }

    let mut model = model_for(session);
    if model.is_none() && input.mode == "plan" {
        // collaborationMode.settings.model is required by the v2 schema. Resolve
        // Codex's first visible model when the Forge session intentionally leaves
        // model selection on "automatic".
        model = state.codex.models(false).await.ok().and_then(|models| {
            models.into_iter().find_map(|entry| {
                entry
                    .get("id")
                    .or_else(|| entry.get("model"))
                    .and_then(Value::as_str)
                    .filter(|id| !id.is_empty())
                    .map(str::to_string)
            })
        });
    }
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
    state.codex.set_goal_bridge(sid, false);
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
    // 先释放审批 sender；serve_request 会回 Codex 正式 decline 并发 resolved，旧卡
    // 不会在 run 结束后仍可点击，pending 也不会永久泄漏。
    state.permissions.abandon_run(run_id);
    match outcome.status.as_str() {
        "completed" => {
            if !outcome.text.is_empty() {
                state.events.emit(
                    EventDraft::new(sid, "agent.message", "agent").payload(json!({
                        "text": outcome.text,
                        "runId": run_id,
                        "provider": "codex",
                        "degraded": false,
                    })),
                );
            }
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
    // Cold thread/resume can immediately continue a persisted active Goal before
    // returning its response. Subscribe before both resume and start so the first
    // auto-turn item/approval can never be lost.
    let inbox = client.subscribe();
    let mut mapper = Mapper::new(run_id);

    let thread_id = match ensure_thread(state, session, input, model, false).await {
        Ok(id) => id,
        Err(e) => return failed(e.0),
    };
    mapper.bind_thread(&thread_id);

    // 先完成用户明确发起的这一轮；若有 active Goal，在这轮 completed 后再 set。
    // app-server 的 status=active 会自己立刻起 turn，提前 set 再 turn/start 会造成双轮并发。
    let activate_goal_after_turn = state.goals.get(sid).filter(|g| g.is_active());

    let permission_mode = state.permissions.mode(sid);
    let turn_params = turn_start_params(&thread_id, input, model, session, &permission_mode);
    let turn = match client.request("turn/start", turn_params).await {
        Ok(v) => v,
        Err(e) => return failed(e.0),
    };
    let turn_id = match turn_id_from_start(&turn) {
        Some(id) => id,
        None => return failed("turn/start 响应缺 turn.id，无法安全中止本轮".to_string()),
    };

    listen_lifecycle(
        state,
        session,
        run_id,
        token,
        client,
        inbox,
        mapper,
        input.scope.clone(),
        thread_id,
        Some(turn_id),
        activate_goal_after_turn,
        false,
    )
    .await
}

/// 从 Goal REST 启动原生自动循环。订阅、run 锁都在 `thread/goal/set(active)` 之前
/// 建好，因为该 RPC 会立刻启动第一轮；反过来会丢最早的 item/审批通知。
pub async fn start_goal_lifecycle(
    state: &Arc<AppState>,
    session: &DebugSession,
    goal: &crate::goals::Goal,
) -> Result<(), CodexError> {
    start_goal_lifecycle_inner(state, session, goal, false).await
}

/// Process-start recovery differs from an explicit PUT/resume: the remote Goal is
/// authoritative, so a terminal remote snapshot must never be overwritten with the
/// stale locally persisted `active` value.
pub async fn recover_goal_lifecycle(
    state: &Arc<AppState>,
    session: &DebugSession,
    goal: &crate::goals::Goal,
) -> Result<(), CodexError> {
    start_goal_lifecycle_inner(state, session, goal, true).await
}

async fn start_goal_lifecycle_inner(
    state: &Arc<AppState>,
    session: &DebugSession,
    _goal: &crate::goals::Goal,
    recovery: bool,
) -> Result<(), CodexError> {
    if recovery
        && session
            .codex_thread_id
            .as_deref()
            .filter(|id| !id.is_empty())
            .is_none()
    {
        if let Ok(paused) = state.goals.set_status(
            &session.id,
            crate::goals::STATUS_PAUSED,
            Some("agentd 重启时没有可恢复的 Codex thread，目标已暂停"),
        ) {
            crate::goals::emit_updated(state, &paused, &session.agent_engine);
        }
        return Ok(());
    }
    if state.codex.goal_bridge_active(&session.id) {
        if recovery {
            return Ok(());
        }
        let thread_id = state
            .sessions
            .get(&session.id)
            .and_then(|current| current.codex_thread_id)
            .ok_or_else(|| CodexError("Goal 监听桥存在但会话缺 Codex thread id".to_string()))?;
        if let Some(latest) = state.goals.get(&session.id).filter(|goal| goal.is_active()) {
            super::goal_set(state, &thread_id, &latest).await?;
        }
        return Ok(());
    }
    if state
        .sessions
        .get(&session.id)
        .and_then(|s| s.active_run_id)
        .is_some()
    {
        // 当前用户 turn 已有唯一订阅者；它在 completed 时会读取最新本地 Goal 并激活。
        return Ok(());
    }

    let client = state.codex.ready_client().await?;
    // A brand-new Codex session has no thread yet. Create/resume it here instead of
    // silently waiting for a later composer turn: thread/goal/set(active) is itself
    // the operation that starts the first native goal turn.
    let inbox = client.subscribe();
    let (run, token) = state.runs.begin(&session.id, "codex_goal");
    if state
        .sessions
        .claim_active_run(&session.id, &run.id)
        .is_err()
    {
        state.runs.finish(&run.id, "failed");
        return Ok(());
    }
    let no_skills = Vec::new();
    let bootstrap = CodexTurnInput {
        user_input: "",
        mode: "build",
        skills: &no_skills,
        plan_body: None,
        scope: crate::scope::resolve(state, session.workspace_id.as_deref(), &[], true),
    };
    let goal_model = model_for(session);
    let thread_id =
        match ensure_thread(state, session, &bootstrap, goal_model.as_deref(), true).await {
            Ok(thread_id) => thread_id,
            Err(error) => {
                state.runs.finish(&run.id, "failed");
                state.sessions.release_active_run(&session.id, &run.id);
                state.events.emit(
                    EventDraft::new(&session.id, "agent.warning", "agent").payload(json!({
                        "code": "CODEX_GOAL_START_FAILED",
                        "message": error.0.clone(),
                    })),
                );
                return Err(error);
            }
        };
    if recovery {
        let remote = client
            .request_with_timeout(
                "thread/goal/get",
                json!({ "threadId": thread_id }),
                std::time::Duration::from_secs(5),
            )
            .await;
        match remote {
            Ok(value) => {
                let remote = value.get("goal").cloned().unwrap_or(value);
                if remote.is_null() {
                    if let Ok(paused) = state.goals.set_status(
                        &session.id,
                        crate::goals::STATUS_PAUSED,
                        Some("Codex thread 中没有可恢复的 Goal，已暂停本地目标"),
                    ) {
                        crate::goals::emit_updated(state, &paused, &session.agent_engine);
                    }
                    state.runs.finish(&run.id, "completed");
                    state.sessions.release_active_run(&session.id, &run.id);
                    return Ok(());
                }
                let remote = super::normalize_goal_value(remote);
                let active = remote.get("status").and_then(Value::as_str)
                    == Some(crate::goals::STATUS_ACTIVE);
                if let Some(remote_goal) = state.goals.sync_codex(&session.id, &remote) {
                    crate::goals::emit_updated(state, &remote_goal, &session.agent_engine);
                }
                if !active {
                    state.runs.finish(&run.id, "completed");
                    state.sessions.release_active_run(&session.id, &run.id);
                    return Ok(());
                }
            }
            Err(error) => {
                if let Ok(paused) = state.goals.set_status(
                    &session.id,
                    crate::goals::STATUS_PAUSED,
                    Some("Codex Goal 恢复对账失败，已安全暂停"),
                ) {
                    crate::goals::emit_updated(state, &paused, &session.agent_engine);
                }
                if client
                    .request_with_timeout(
                        "thread/goal/set",
                        json!({ "threadId": thread_id, "status": "paused" }),
                        std::time::Duration::from_secs(3),
                    )
                    .await
                    .is_err()
                {
                    client.suspend();
                }
                state.runs.finish(&run.id, "failed");
                state.sessions.release_active_run(&session.id, &run.id);
                return Err(error);
            }
        }
    }
    state.codex.set_goal_bridge(&session.id, true);
    let latest_goal = state.goals.get(&session.id).filter(|goal| goal.is_active());
    if !recovery && latest_goal.is_none() {
        state.codex.set_goal_bridge(&session.id, false);
        state.runs.finish(&run.id, "completed");
        state.sessions.release_active_run(&session.id, &run.id);
        return Ok(());
    }
    state.events.emit(
        EventDraft::new(&session.id, "agent.started", "agent").payload(json!({
            "runId": run.id,
            "engine": "codex",
            "model": model_for(session).unwrap_or_else(|| "codex 默认模型".to_string()),
            "threadId": thread_id,
            "source": "goal",
        })),
    );
    let mut mapper = Mapper::new(&run.id);
    mapper.bind_thread(&thread_id);
    let goal_scope = bootstrap.scope.clone();
    if !recovery {
        let latest_goal = latest_goal.expect("explicit Goal lifecycle checked active goal");
        if let Err(error) = super::goal_set(state, &thread_id, &latest_goal).await {
            if let Ok(paused) = state.goals.set_status(
                &session.id,
                crate::goals::STATUS_PAUSED,
                Some("Codex Goal 启动失败，已安全暂停"),
            ) {
                crate::goals::emit_updated(state, &paused, &session.agent_engine);
            }
            if client
                .request_with_timeout(
                    "thread/goal/set",
                    json!({ "threadId": thread_id, "status": "paused" }),
                    std::time::Duration::from_secs(3),
                )
                .await
                .is_err()
            {
                client.suspend();
            }
            state.codex.set_goal_bridge(&session.id, false);
            finish(state, session, &run.id, failed(error.0.clone()));
            return Err(error);
        }
    }

    let state = Arc::clone(state);
    let session = session.clone();
    tokio::spawn(async move {
        let outcome = listen_lifecycle(
            &state, &session, &run.id, &token, client, inbox, mapper, goal_scope, thread_id, None,
            None, true,
        )
        .await;
        state.codex.set_goal_bridge(&session.id, false);
        finish(&state, &session, &run.id, outcome);
    });
    Ok(())
}

#[allow(clippy::too_many_arguments)]
async fn listen_lifecycle(
    state: &Arc<AppState>,
    session: &DebugSession,
    run_id: &str,
    token: &CancelToken,
    client: Arc<super::rpc::CodexClient>,
    mut inbox: tokio::sync::mpsc::UnboundedReceiver<Inbound>,
    mut mapper: Mapper,
    scope: crate::scope::ScopeContext,
    thread_id: String,
    mut current_turn_id: Option<String>,
    mut activate_goal_after_turn: Option<crate::goals::Goal>,
    mut goal_mode: bool,
) -> Outcome {
    let sid = session.id.as_str();
    let mut last_text = String::new();
    let mut last_text_emitted = false;
    // Cold thread/resume emits a persisted Goal snapshot before its response. When
    // an explicit PUT immediately follows, that stale snapshot (notably
    // goal/cleared) is queued ahead of the new activation. Do not let it mutate the
    // local Goal or terminate the bridge until this lifecycle has observed either
    // the matching active snapshot or its first turn/started.
    let mut goal_activation_observed = false;
    loop {
        // 高频 delta 可能让 recv 永不超时，所以必须在每次收消息前都检查 Stop。
        if token.is_cancelled() {
            if let Some(turn_id) = current_turn_id.as_deref() {
                let _ = client
                    .request_with_timeout(
                        "turn/interrupt",
                        json!({ "threadId": thread_id, "turnId": turn_id }),
                        std::time::Duration::from_secs(3),
                    )
                    .await;
            }
            if goal_mode || activate_goal_after_turn.is_some() {
                // Local state is the immediate safety gate. If the upstream pause
                // cannot be confirmed quickly, tear down the transport so an active
                // native Goal cannot keep running invisibly after Forge releases it.
                if let Ok(goal) = state.goals.set_status(
                    sid,
                    crate::goals::STATUS_PAUSED,
                    Some("用户中止了 Goal 运行"),
                ) {
                    crate::goals::emit_updated(state, &goal, &session.agent_engine);
                }
                if client
                    .request_with_timeout(
                        "thread/goal/set",
                        json!({ "threadId": thread_id, "status": "paused" }),
                        std::time::Duration::from_secs(3),
                    )
                    .await
                    .is_err()
                {
                    state
                        .events
                        .emit(EventDraft::new(sid, "agent.warning", "agent").payload(
                        json!({
                            "code": "CODEX_GOAL_PAUSE_UNCONFIRMED",
                            "message": "未能确认 Codex Goal 已暂停，已断开 app-server 作为安全闸"
                        }),
                    ));
                    client.suspend();
                }
            }
            return Outcome {
                status: "cancelled".to_string(),
                text: last_text,
                error: None,
            };
        }

        // terminal goal 通知可能在 turn/completed 之后才到。此时没有当前轮，收到
        // terminal/cleared 后即可释放唯一监听桥。
        if goal_mode
            && goal_activation_observed
            && current_turn_id.is_none()
            && !state.goals.get(sid).is_some_and(|goal| goal.is_active())
        {
            return Outcome {
                status: "completed".to_string(),
                text: if last_text_emitted {
                    String::new()
                } else {
                    last_text
                },
                error: None,
            };
        }

        // 取消是本地状态,不会经通知流到来,所以每次等消息都带一个短超时去轮询它。
        let next = tokio::time::timeout(std::time::Duration::from_millis(250), inbox.recv()).await;
        match next {
            Err(_) => continue,
            // 连接断了:本轮无从收尾,如实失败(下一轮 ensure_started 会重连)。
            Ok(None) => {
                if goal_mode || activate_goal_after_turn.is_some() {
                    if let Ok(goal) = state.goals.set_status(
                        sid,
                        crate::goals::STATUS_PAUSED,
                        Some("Codex app-server 连接中断，目标已暂停；重连后可恢复"),
                    ) {
                        crate::goals::emit_updated(state, &goal, &session.agent_engine);
                    }
                    state
                        .events
                        .emit(
                            EventDraft::new(sid, "agent.warning", "agent").payload(json!({
                                "code": "CODEX_GOAL_CONNECTION_LOST",
                                "message": "Codex app-server 连接中断，Goal 已安全暂停"
                            })),
                        );
                }
                return Outcome {
                    status: "failed".to_string(),
                    text: mapper.final_text(),
                    error: Some("codex app-server 连接中断,本轮未能完成".to_string()),
                };
            }
            Ok(Some(msg)) => {
                if !mapper.owns(msg.params()) {
                    continue;
                }
                match msg {
                    Inbound::Notification { method, params } => {
                        if method == "turn/started" {
                            current_turn_id = turn_id_from_start(&params);
                            goal_activation_observed = true;
                        }
                        if matches!(
                            method.as_str(),
                            "thread/goal/updated" | "thread/goal/cleared"
                        ) && !goal_activation_observed
                        {
                            let local = state.goals.get(sid);
                            if goal_notification_matches_local(&method, &params, local.as_ref()) {
                                goal_activation_observed = true;
                            } else {
                                continue;
                            }
                        }
                        for action in mapper.handle(&method, &params) {
                            if let Some(o) = apply(state, session, run_id, &mapper, action) {
                                if o.status != "completed" {
                                    return o;
                                }
                                current_turn_id = None;
                                last_text = o.text.clone();

                                let goal_to_activate =
                                    activate_goal_after_turn.take().or_else(|| {
                                        (!goal_mode).then(|| state.goals.get(sid)).flatten()
                                    });
                                if let Some(_goal) = goal_to_activate {
                                    // 用户在首轮期间可能暂停/清掉了目标；以最新镜像为准。
                                    let Some(goal) = state.goals.get(sid).filter(|g| g.is_active())
                                    else {
                                        return o;
                                    };
                                    emit_goal_turn_message(state, sid, run_id, &last_text);
                                    last_text_emitted = !last_text.is_empty();
                                    mapper.reset_turn();
                                    goal_mode = true;
                                    goal_activation_observed = false;
                                    state.codex.set_goal_bridge(sid, true);
                                    if let Err(e) = super::goal_set(state, &thread_id, &goal).await
                                    {
                                        return failed(e.0);
                                    }
                                    continue;
                                }

                                if !goal_mode {
                                    return o;
                                }
                                if state.goals.get(sid).is_some_and(|goal| goal.is_active()) {
                                    emit_goal_turn_message(state, sid, run_id, &last_text);
                                    last_text_emitted = !last_text.is_empty();
                                    mapper.reset_turn();
                                } else {
                                    return o;
                                }
                            }
                        }
                    }
                    Inbound::ServerRequest { id, method, params } => {
                        serve_request(state, session, run_id, &scope, &client, id, method, params);
                    }
                }
            }
        }
    }
}

fn goal_notification_matches_local(
    method: &str,
    params: &Value,
    local: Option<&crate::goals::Goal>,
) -> bool {
    if method == "thread/goal/cleared" {
        return local.is_none();
    }
    if method != "thread/goal/updated" {
        return false;
    }
    let Some(local) = local else {
        return false;
    };
    let remote = super::normalize_goal_value(
        params
            .get("goal")
            .cloned()
            .unwrap_or_else(|| params.clone()),
    );
    if remote.get("objective").and_then(Value::as_str) != Some(local.objective.as_str())
        || remote.get("status").and_then(Value::as_str) != Some(local.status.as_str())
    {
        return false;
    }
    match remote.get("tokenBudget") {
        Some(value) => value.as_u64() == Some(local.token_budget),
        None => true,
    }
}

fn emit_goal_turn_message(state: &AppState, sid: &str, run_id: &str, text: &str) {
    if text.is_empty() {
        return;
    }
    state.events.emit(
        EventDraft::new(sid, "agent.message", "agent").payload(json!({
            "text": text,
            "runId": run_id,
            "provider": "codex",
            "source": "goal",
            "degraded": false,
        })),
    );
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
        Action::SyncGoal(goal) => {
            state.goals.sync_codex(sid, &goal);
            None
        }
        Action::ClearGoal => {
            state.goals.clear(sid);
            None
        }
        Action::ResolveServerRequest(request_id) => {
            state.permissions.abandon_upstream(&request_id);
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
fn sync_todos(
    state: &Arc<AppState>,
    session_id: &str,
    run_id: &str,
    steps: &[super::map::PlanStep],
) {
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
                    state
                        .events
                        .emit(
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
                    state
                        .events
                        .emit(
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
    scope: &crate::scope::ScopeContext,
    client: &Arc<super::rpc::CodexClient>,
    id: Value,
    method: String,
    params: Value,
) {
    let state = Arc::clone(state);
    let client = Arc::clone(client);
    let sid = session.id.clone();
    let run_id = run_id.to_string();
    let scope = scope.clone();
    tokio::spawn(async move {
        if let Some(approval) = super::approvals::classify(&method, &params) {
            let decision = state
                .permissions
                .request_decision(
                    &state.events,
                    &sid,
                    &run_id,
                    approval.kind,
                    approval.payload,
                    &id,
                )
                .await;
            let result = match decision {
                Ok(d) => super::approvals::to_codex_result(&approval.reply, &d),
                // 审批通道断了(turn 被中止收尾)→ 回拒绝让 Codex 干净收场。
                Err(_) => super::approvals::abandon_result(&approval.reply),
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
            let result = super::tools::call(&state, &scope, &name, &args).await;
            let _ = client.respond(&id, result);
            return;
        }
        // 不认的服务端请求必须回错误而不是沉默:沉默会让那一轮永久挂住。
        let _ = client.respond_error(&id, -32601, &format!("Forge 未实现该请求: {method}"));
    });
}

/// After cold resume the thread is materialized, so Goal get/set is now reliable.
/// `turn/start` steers an already-running auto Goal turn; therefore an active local
/// Goal stays active. A local paused/terminal state wins over stale remote active
/// state and is pushed upstream immediately to prevent another automatic turn.
async fn reconcile_goal_after_resume(
    state: &Arc<AppState>,
    client: &Arc<super::rpc::CodexClient>,
    session: &DebugSession,
    thread_id: &str,
) -> Result<(), CodexError> {
    let value = client
        .request_with_timeout(
            "thread/goal/get",
            json!({ "threadId": thread_id }),
            std::time::Duration::from_secs(5),
        )
        .await?;
    let remote = value.get("goal").cloned().unwrap_or(value);
    if remote.is_null() {
        // The remote thread is authoritative after resume. Keeping a stale local
        // active Goal here would cause the explicit user turn's settlement to call
        // goal/set and resurrect something cleared from another client.
        if state
            .goals
            .get(&session.id)
            .is_some_and(|goal| goal.is_active())
        {
            if let Ok(paused) = state.goals.set_status(
                &session.id,
                crate::goals::STATUS_PAUSED,
                Some("Codex thread 中的 Goal 已被清除，本地旧目标已暂停"),
            ) {
                crate::goals::emit_updated(state, &paused, &session.agent_engine);
            }
        }
        return Ok(());
    }

    let remote = super::normalize_goal_value(remote);
    let remote_active =
        remote.get("status").and_then(Value::as_str) == Some(crate::goals::STATUS_ACTIVE);
    let local_before = state.goals.get(&session.id);
    // A local pause/terminal state is an explicit safety decision and wins over a
    // stale upstream active snapshot. Remote terminal states, conversely, must win
    // over a stale local active snapshot so completed goals are never resurrected.
    let keep_local_stop = remote_active
        && local_before
            .as_ref()
            .is_some_and(|local| !local.is_active());
    if !keep_local_stop {
        if let Some(goal) = state.goals.sync_codex(&session.id, &remote) {
            crate::goals::emit_updated(state, &goal, &session.agent_engine);
        }
    }
    if remote_active && keep_local_stop {
        let local = local_before.expect("keep_local_stop requires local goal");
        client
            .request_with_timeout(
                "thread/goal/set",
                json!({
                    "threadId": thread_id,
                    "status": super::codex_goal_status(&local.status),
                }),
                std::time::Duration::from_secs(5),
            )
            .await?;
    }
    Ok(())
}

/// 首轮 `thread/start`,后续轮 `thread/resume`;线程 id 写回会话。
async fn ensure_thread(
    state: &Arc<AppState>,
    session: &DebugSession,
    input: &CodexTurnInput<'_>,
    model: Option<&str>,
    goal_lifecycle: bool,
) -> Result<String, CodexError> {
    let client = state.codex.ready_client().await?;
    let cfg = super::config::load();
    let permission_mode = state.permissions.mode(&session.id);
    let (approval_policy, sandbox) = approval_for_turn(&permission_mode, input.mode);
    if let Some(existing) = session.codex_thread_id.as_deref().filter(|t| !t.is_empty()) {
        match client
            .request(
                "thread/resume",
                thread_resume_params(existing, input, &cfg, approval_policy, sandbox, model),
            )
            .await
        {
            Ok(_) => {
                if !goal_lifecycle {
                    reconcile_goal_after_resume(state, &client, session, existing).await?;
                }
                return Ok(existing.to_string());
            }
            // Only an explicitly missing persisted rollout may create a new thread.
            // Auth, quota, transient and MCP errors must preserve history and surface.
            Err(e) if thread_resume_missing(&e) => {
                eprintln!("[codex] thread/resume 找不到原 rollout({e}),改新开线程")
            }
            Err(e) => return Err(e),
        }
    }
    let params = thread_start_params(input, &cfg, approval_policy, sandbox);
    let out = match client.request("thread/start", params.clone()).await {
        Ok(v) => v,
        Err(e) if thread_start_compat_error(&e) => {
            // dynamicTools 比 per-thread MCP 更晚进入协议。先只撤动态工具，保住七工；
            // 只有第二次仍是参数兼容错误时才撤 config，不能一次失败就把 MCP 一起丢掉。
            eprintln!("[codex] thread/start dynamicTools 被拒({e}),保留 MCP 重试");
            let mut without_dynamic = params;
            without_dynamic
                .as_object_mut()
                .map(|obj| obj.remove("dynamicTools"));
            state.events.emit(
                EventDraft::new(&session.id, "agent.warning", "agent").payload(json!({
                    "code": "CODEX_DYNAMIC_TOOLS_REJECTED",
                    "message": "当前 codex 版本不接受 dynamicTools；Forge 资源工具未注入，本项目 MCP 仍保留",
                })),
            );
            match client
                .request("thread/start", without_dynamic.clone())
                .await
            {
                Ok(v) => v,
                Err(e2) if thread_start_compat_error(&e2) => {
                    eprintln!("[codex] thread/start per-thread config 被拒({e2}),撤掉 MCP 重试");
                    let mut lean = without_dynamic;
                    lean.as_object_mut().map(|obj| obj.remove("config"));
                    state.events.emit(
                        EventDraft::new(&session.id, "agent.warning", "agent").payload(json!({
                            "code": "CODEX_THREAD_CONFIG_REJECTED",
                            "message": "当前 codex 版本不接受 per-thread MCP 配置；本轮只有 Codex 内建工具",
                        })),
                    );
                    client.request("thread/start", lean).await?
                }
                Err(e2) => return Err(e2),
            }
        }
        Err(e) => return Err(e),
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

fn thread_resume_missing(error: &CodexError) -> bool {
    let message = error.0.to_ascii_lowercase();
    message.contains("thread not found")
        || message.contains("unknown thread")
        || message.contains("no rollout found")
        || (message.contains("rollout") && message.contains("not found"))
}

fn thread_start_compat_error(error: &CodexError) -> bool {
    let s = error.0.to_ascii_lowercase();
    s.contains("invalid request")
        || s.contains("unknown field")
        || s.contains("unrecognized field")
        || s.contains("experimentalapi capability")
}

fn turn_id_from_start(value: &Value) -> Option<String> {
    value
        .get("turn")
        .and_then(|turn| turn.get("id"))
        .or_else(|| value.get("turnId"))
        .or_else(|| value.get("id"))
        .and_then(Value::as_str)
        .filter(|id| !id.is_empty())
        .map(str::to_string)
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
        // developerInstructions 是 thread/start 的顶层字段；放到 config 里不会生效。
        "developerInstructions": developer_instructions(
            &input.scope,
            crate::mcp::computer_use_enabled(),
        ),
        "config": {
            "mcp_servers": super::mcp_config::mcp_servers_json(
                &input.scope.current.project_root,
                cfg.auto_register_mcp,
            ),
        },
        "dynamicTools": super::tools::declarations(),
    })
}

/// Resume is also the configuration refresh boundary. Session permission mode,
/// MCP/Computer Use settings and developer instructions can all change after a
/// thread was first created; resuming with only `threadId` would retain stale
/// (potentially broader) privileges.
fn thread_resume_params(
    thread_id: &str,
    input: &CodexTurnInput<'_>,
    cfg: &super::config::CodexConfig,
    approval_policy: &str,
    sandbox: &str,
    model: Option<&str>,
) -> Value {
    let mut params = thread_start_params(input, cfg, approval_policy, sandbox);
    params
        .as_object_mut()
        .map(|object| object.remove("dynamicTools"));
    params["threadId"] = json!(thread_id);
    if let Some(model) = model {
        params["model"] = json!(model);
    }
    params
}

/// `turn/start` 参数。
pub fn turn_start_params(
    thread_id: &str,
    input: &CodexTurnInput<'_>,
    model: Option<&str>,
    session: &DebugSession,
    permission_mode: &str,
) -> Value {
    let mut items: Vec<Value> = Vec::new();
    // 技能走结构化条目而不是拼进正文:Codex 自己会读 SKILL.md 全文,
    // 拼进正文只会得到一句「请使用技能 X」而技能内容根本没进上下文。
    for name in input.skills {
        let Some(skill) = crate::skills::find_skill(name).filter(|skill| skill.enabled) else {
            continue;
        };
        let path = std::fs::canonicalize(&skill.file).unwrap_or(skill.file);
        if !path.is_absolute() || !path.is_file() {
            continue;
        }
        items.push(json!({
            "type": "skill",
            "name": skill.front.name,
            "path": path.to_string_lossy(),
        }));
    }
    let mut text = input.user_input.to_string();
    if input.mode == "plan" {
        text.push_str(
            "\n\n【只读计划模式】只调查、分析并给出实施计划；不要修改文件或执行有副作用的命令。",
        );
    }
    if let Some(plan) = &input.plan_body {
        text.push_str("\n\n【按下列计划实施】\n");
        text.push_str(plan);
    }
    items.push(json!({ "type": "text", "text": text }));

    let mut params = json!({
        "threadId": thread_id,
        "input": items,
    });
    let (approval_policy, sandbox) = approval_for_turn(permission_mode, input.mode);
    params["approvalPolicy"] = json!(approval_policy);
    params["sandboxPolicy"] = sandbox_policy(sandbox);
    if let Some(m) = model {
        params["model"] = json!(m);
    }
    if let Some(e) = effort_for(session) {
        params["effort"] = json!(e);
    }
    // v2 collaborationMode.settings.model 必填。没有显式/配置模型时宁可省略协作对象，
    // 让普通 plan 指令兜底，也不能发一个会被整个 turn/start 拒绝的半截结构。
    if input.mode == "plan" {
        if let Some(m) = model {
            params["collaborationMode"] = json!({
                "mode": "plan",
                "settings": {
                    "model": m,
                    "reasoning_effort": effort_for(session),
                    "developer_instructions": Value::Null,
                }
            });
        }
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

    fn state_with_transport(
        tag: &str,
        transport: Arc<dyn super::super::rpc::CodexTransport>,
    ) -> (Arc<AppState>, std::path::PathBuf) {
        let dir = std::env::temp_dir().join(format!(
            "agentd-codex-turn-{tag}-{}-{}",
            std::process::id(),
            crate::events::new_id("t")
        ));
        let state = Arc::new(AppState {
            started: std::time::Instant::now(),
            proposals: crate::proposals::ProposalStore::default(),
            swarm: crate::swarm::SwarmCoordinator::default(),
            events: Arc::new(crate::events::EventBus::new(dir.join("agent-events"), 256)),
            sessions: Arc::new(crate::sessions::SessionStore::load(
                dir.join("agent-sessions").join("sessions.json"),
            )),
            folders: Arc::new(crate::sessions::ChatFolderStore::load(
                dir.join("agent-sessions").join("chat-folders.json"),
            )),
            workspaces: Arc::new(crate::workspaces::WorkspaceStore::load(
                dir.join("agent-sessions").join("workspaces.json"),
            )),
            runs: Arc::new(crate::agent::RunRegistry::default()),
            todos: Arc::new(crate::agent::TodoStore::load(
                dir.join("agent-sessions").join("todos.json"),
            )),
            receipts: Arc::new(crate::receipts::ReceiptStore::load(
                dir.join("agent-sessions").join("receipts.json"),
            )),
            wakes: Arc::new(crate::agent::WakeRegistry::default()),
            permissions: Arc::new(crate::permission::PermissionService::load(
                dir.join("agent-sessions").join("permissions.json"),
            )),
            codex: Arc::new(super::super::service::CodexService::with_transport(
                transport,
            )),
            goals: Arc::new(crate::goals::GoalStore::load(
                dir.join("agent-sessions").join("goals.json"),
            )),
        });
        (state, dir)
    }

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
        assert_eq!(approval_for("plan"), ("never", "read-only"));
        assert_eq!(approval_for("auto"), ("on-request", "workspace-write"));
        assert_eq!(approval_for("bypass"), ("never", "workspace-write"));
    }

    /// turn/start:技能进结构化条目、计划全文接在正文后、plan 模式带协作开关。
    #[test]
    fn turn_start_shape() {
        let scope = crate::scope::ScopeContext::single(crate::scope::ScopeProject::for_test(
            "D:/proj", "D:/ws",
        ));
        let skills = vec!["game-2d-kit".to_string()];
        let input = CodexTurnInput {
            user_input: "加个二段跳",
            mode: "plan",
            skills: &skills,
            plan_body: Some("## 步骤\n1. 读现有跳跃逻辑".to_string()),
            scope,
        };
        let p = turn_start_params("th_1", &input, Some("gpt-5.6-terra"), &session(), "bypass");
        assert_eq!(p["threadId"], "th_1");
        assert_eq!(p["model"], "gpt-5.6-terra");
        assert_eq!(p["collaborationMode"]["mode"], "plan");
        assert_eq!(p["collaborationMode"]["settings"]["model"], "gpt-5.6-terra");
        assert_eq!(p["approvalPolicy"], "never");
        assert_eq!(p["sandboxPolicy"]["type"], "readOnly");
        let items = p["input"].as_array().unwrap();
        assert_eq!(items[0]["type"], "skill");
        assert_eq!(items[0]["name"], "game-2d-kit");
        let skill_path = items[0]["path"].as_str().unwrap();
        assert!(
            std::path::Path::new(skill_path).is_absolute(),
            "{skill_path}"
        );
        assert!(skill_path
            .replace('\\', "/")
            .ends_with("skills/game-2d-kit/SKILL.md"));
        let text = items.last().unwrap()["text"].as_str().unwrap();
        assert!(text.contains("加个二段跳"), "{text}");
        assert!(text.contains("读现有跳跃逻辑"), "{text}");
        assert!(text.contains("只读计划模式"), "{text}");
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
            "D:/proj", "D:/ws",
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

    #[test]
    fn turn_start_response_yields_interrupt_id() {
        assert_eq!(
            turn_id_from_start(&json!({ "turn": { "id": "turn_457" } })).as_deref(),
            Some("turn_457")
        );
        assert!(turn_id_from_start(&json!({ "turn": {} })).is_none());
    }

    #[test]
    fn thread_start_uses_top_level_instructions_and_kebab_sandbox() {
        let scope = crate::scope::ScopeContext::single(crate::scope::ScopeProject::for_test(
            "D:/proj", "D:/ws",
        ));
        let skills = Vec::new();
        let input = CodexTurnInput {
            user_input: "测试",
            mode: "plan",
            skills: &skills,
            plan_body: None,
            scope,
        };
        let params = thread_start_params(
            &input,
            &super::super::config::CodexConfig::default(),
            "never",
            "read-only",
        );
        assert_eq!(params["sandbox"], "read-only");
        assert!(params["developerInstructions"]
            .as_str()
            .is_some_and(|v| !v.is_empty()));
        assert!(params["config"].get("developer_instructions").is_none());
    }

    #[test]
    fn plan_without_resolved_model_stays_read_only_and_explicit() {
        let scope = crate::scope::ScopeContext::single(crate::scope::ScopeProject::for_test(
            "D:/proj", "D:/ws",
        ));
        let skills = Vec::new();
        let input = CodexTurnInput {
            user_input: "设计一个存档系统",
            mode: "plan",
            skills: &skills,
            plan_body: None,
            scope,
        };
        let params = turn_start_params("th_1", &input, None, &session(), "bypass");
        assert!(params.get("collaborationMode").is_none());
        assert_eq!(params["approvalPolicy"], "never");
        assert_eq!(params["sandboxPolicy"]["type"], "readOnly");
        assert!(params["input"][0]["text"]
            .as_str()
            .unwrap()
            .contains("只读计划模式"));
    }

    #[test]
    fn resume_refreshes_restricted_policy_and_thread_config() {
        let scope = crate::scope::ScopeContext::single(crate::scope::ScopeProject::for_test(
            "D:/proj", "D:/ws",
        ));
        let skills = Vec::new();
        let input = CodexTurnInput {
            user_input: "只做计划",
            mode: "plan",
            skills: &skills,
            plan_body: None,
            scope,
        };
        let params = thread_resume_params(
            "th_existing",
            &input,
            &super::super::config::CodexConfig::default(),
            "never",
            "read-only",
            Some("gpt-5.6-terra"),
        );
        assert_eq!(params["threadId"], "th_existing");
        assert_eq!(params["approvalPolicy"], "never");
        assert_eq!(params["sandbox"], "read-only");
        assert_eq!(params["model"], "gpt-5.6-terra");
        assert!(params.get("developerInstructions").is_some());
        assert!(params.get("config").is_some());
        assert!(params.get("dynamicTools").is_none());
    }

    #[tokio::test]
    async fn stale_resume_goal_clear_cannot_kill_new_goal_bridge() {
        let transport =
            super::super::rpc::scripted_with_handshake(|method, params, id| match method {
                "thread/resume" => vec![
                    json!({
                        "method": "thread/goal/cleared",
                        "params": { "threadId": "th_1" }
                    }),
                    json!({ "id": id, "result": { "thread": { "id": "th_1" } } }),
                ],
                "thread/goal/set" if params.get("objective").is_some() => vec![
                    json!({ "id": id, "result": {} }),
                    json!({
                        "method": "thread/goal/updated",
                        "params": { "threadId": "th_1", "goal": {
                            "objective": "完成存档系统", "status": "active",
                            "tokenBudget": 10000
                        }}
                    }),
                    json!({
                        "method": "turn/started",
                        "params": { "threadId": "th_1", "turn": { "id": "turn_goal_1" } }
                    }),
                ],
                "thread/goal/set" if params.get("status") == Some(&json!("paused")) => vec![
                    json!({ "id": id, "result": {} }),
                    json!({
                        "method": "thread/goal/updated",
                        "params": { "threadId": "th_1", "goal": {
                            "objective": "完成存档系统", "status": "paused",
                            "tokenBudget": 10000
                        }}
                    }),
                    json!({
                        "method": "turn/completed",
                        "params": { "threadId": "th_1", "turn": {
                            "id": "turn_goal_1", "status": "completed"
                        }}
                    }),
                ],
                _ => vec![],
            });
        let (state, dir) = state_with_transport("stale-goal-clear", Arc::new(transport));
        let mut session = state.sessions.create("goal", "coding", None, true, None);
        session.agent_engine = super::super::config::ENGINE_CODEX.to_string();
        session.codex_thread_id = Some("th_1".to_string());
        state.sessions.save(&session);
        let goal = state
            .goals
            .set(&session.id, "完成存档系统", Some(10_000))
            .unwrap();

        start_goal_lifecycle(&state, &session, &goal).await.unwrap();
        // Let the spawned listener consume the queued stale clear, the fresh active
        // snapshot and turn/started before checking that the bridge survived.
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        for _ in 0..50 {
            if state.codex.goal_bridge_active(&session.id)
                && state
                    .goals
                    .get(&session.id)
                    .is_some_and(|goal| goal.is_active())
            {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
        assert!(state.codex.goal_bridge_active(&session.id));
        assert!(state
            .goals
            .get(&session.id)
            .is_some_and(|goal| goal.is_active()));

        let response = crate::goals::set_goal_status(
            axum::extract::State(Arc::clone(&state)),
            axum::extract::Path((session.id.clone(), "pause".to_string())),
        )
        .await;
        assert_eq!(response.status(), axum::http::StatusCode::OK);
        assert_eq!(
            state.goals.get(&session.id).unwrap().status,
            crate::goals::STATUS_PAUSED
        );
        for _ in 0..50 {
            if !state.codex.goal_bridge_active(&session.id) {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
        assert!(!state.codex.goal_bridge_active(&session.id));
        std::fs::remove_dir_all(dir).ok();
    }

    #[tokio::test]
    async fn immediate_pause_matches_barrier_without_turn_started() {
        let transport =
            super::super::rpc::scripted_with_handshake(|method, params, id| match method {
                "thread/resume" => vec![
                    json!({
                        "method": "thread/goal/cleared",
                        "params": { "threadId": "th_1" }
                    }),
                    json!({ "id": id, "result": { "thread": { "id": "th_1" } } }),
                ],
                "thread/goal/set" if params.get("objective").is_some() => vec![
                    json!({ "id": id, "result": {} }),
                    json!({
                        "method": "thread/goal/updated",
                        "params": { "threadId": "th_1", "goal": {
                            "objective": "完成存档系统", "status": "active",
                            "tokenBudget": 10000
                        }}
                    }),
                ],
                "thread/goal/set" if params.get("status") == Some(&json!("paused")) => vec![
                    json!({ "id": id, "result": {} }),
                    json!({
                        "method": "thread/goal/updated",
                        "params": { "threadId": "th_1", "goal": {
                            "objective": "完成存档系统", "status": "paused",
                            "tokenBudget": 10000
                        }}
                    }),
                ],
                _ => vec![],
            });
        let (state, dir) = state_with_transport("immediate-pause", Arc::new(transport));
        let mut session = state.sessions.create("goal", "coding", None, true, None);
        session.agent_engine = super::super::config::ENGINE_CODEX.to_string();
        session.codex_thread_id = Some("th_1".to_string());
        state.sessions.save(&session);
        let goal = state
            .goals
            .set(&session.id, "完成存档系统", Some(10_000))
            .unwrap();

        start_goal_lifecycle(&state, &session, &goal).await.unwrap();
        // Do not yield before pausing: the listener still has stale cleared + fresh
        // active queued, so local expected state becomes paused before either is read.
        let response = crate::goals::set_goal_status(
            axum::extract::State(Arc::clone(&state)),
            axum::extract::Path((session.id.clone(), "pause".to_string())),
        )
        .await;
        assert_eq!(response.status(), axum::http::StatusCode::OK);
        for _ in 0..50 {
            if !state.codex.goal_bridge_active(&session.id) {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
        assert_eq!(
            state.goals.get(&session.id).unwrap().status,
            crate::goals::STATUS_PAUSED
        );
        assert!(
            !state.codex.goal_bridge_active(&session.id),
            "matching paused notification must open the barrier and release the bridge"
        );
        std::fs::remove_dir_all(dir).ok();
    }
}
