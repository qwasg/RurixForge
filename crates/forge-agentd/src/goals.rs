//! 目标(Goal)面:「给一个目标,让 agent 自己一轮接一轮推进到完成」。
//!
//! 两种引擎统一:
//! - Codex 引擎:`thread/goal/set|get|clear` 透传,Codex 自己负责续跑,
//!   `thread/goal/updated|cleared` 通知经 [map](crate::codex::map) 翻成 `goal.*` 事件;
//! - 本地引擎:Codex 那套在本仓不存在,所以这里自己实现——目标持久到
//!   `data/agent-sessions/goals.json`,`execute_turn` 收尾时若目标仍 active、模型没标完成、
//!   预算没耗尽,就自动起下一轮。
//!
//! 两条腿产出**同形的** `goal.updated` / `goal.cleared` 事件,前端一套 GoalBar/GoalTab 通吃。
//!
//! 预算的意义:自动续跑没有天然终点,模型判断「还没做完」就会一直跑下去。token 预算是
//! 用户能看懂也能预估花费的闸门,耗尽即暂停(不是失败——用户加预算就能接着跑)。

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use crate::events::{now_rfc3339, EventDraft};
use crate::AppState;

/// 目标状态。
pub const STATUS_ACTIVE: &str = "active";
pub const STATUS_PAUSED: &str = "paused";
pub const STATUS_COMPLETED: &str = "completed";
pub const STATUS_BLOCKED: &str = "blocked";

const STATUSES: [&str; 4] = [
    STATUS_ACTIVE,
    STATUS_PAUSED,
    STATUS_COMPLETED,
    STATUS_BLOCKED,
];

/// 缺省 token 预算。没有缺省的话「设个目标」等于「无上限自动跑」,
/// 用户在一觉醒来之前不会知道花了多少。
pub const DEFAULT_TOKEN_BUDGET: u64 = 2_000_000;
/// 单个目标的自动续跑轮数上限(兜底:模型可能每轮只烧几百 token 却永远不说完成)。
pub const MAX_AUTO_TURNS: u64 = 200;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Goal {
    pub session_id: String,
    /// 目标正文(用户的一句话)。
    pub objective: String,
    /// active | paused | completed | blocked
    pub status: String,
    /// token 预算(0 = 不限;显式设 0 是用户的选择,不是缺省)。
    #[serde(default)]
    pub token_budget: u64,
    #[serde(default)]
    pub tokens_used: u64,
    #[serde(default)]
    pub time_used_seconds: u64,
    /// 已自动续跑的轮数。
    #[serde(default)]
    pub turns: u64,
    /// 模型/系统给出的最近一条说明(完成理由、卡住原因、暂停原因)。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
    #[serde(default)]
    pub created_at: String,
    #[serde(default)]
    pub updated_at: String,
}

impl Goal {
    fn new(session_id: &str, objective: &str, token_budget: u64) -> Self {
        let ts = now_rfc3339();
        Goal {
            session_id: session_id.to_string(),
            objective: objective.to_string(),
            status: STATUS_ACTIVE.to_string(),
            token_budget,
            tokens_used: 0,
            time_used_seconds: 0,
            turns: 0,
            note: None,
            created_at: ts.clone(),
            updated_at: ts,
        }
    }

    pub fn is_active(&self) -> bool {
        self.status == STATUS_ACTIVE
    }

    /// 预算是否已耗尽(预算 0 = 不限)。
    pub fn budget_exhausted(&self) -> bool {
        (self.token_budget > 0 && self.tokens_used >= self.token_budget)
            || self.turns >= MAX_AUTO_TURNS
    }
}

/// 会话 → 目标的持久存贮(读-改-写整文件,同 sessions.json / todos.json 纪律)。
#[derive(Default)]
pub struct GoalStore {
    path: PathBuf,
    inner: Mutex<HashMap<String, Goal>>,
}

impl GoalStore {
    pub fn load(path: PathBuf) -> Self {
        let inner = read_file(&path);
        GoalStore {
            path,
            inner: Mutex::new(inner),
        }
    }

    pub fn get(&self, session_id: &str) -> Option<Goal> {
        self.inner
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(session_id)
            .cloned()
    }

    /// 设目标(已有则替换;objective 空 → Err)。
    pub fn set(
        &self,
        session_id: &str,
        objective: &str,
        token_budget: Option<u64>,
    ) -> Result<Goal, String> {
        let objective = objective.trim();
        if objective.is_empty() {
            return Err("objective 不可空".to_string());
        }
        if objective.chars().count() > 4_000 {
            return Err("objective 最多 4000 个字符".to_string());
        }
        let mut inner = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        let budget = token_budget.unwrap_or(DEFAULT_TOKEN_BUDGET);
        let goal = match inner.get(session_id) {
            // 同一个目标改预算 = 接着跑,不该把已跑的进度归零。
            Some(prev) if prev.objective == objective => {
                let mut g = prev.clone();
                g.token_budget = budget;
                g.status = STATUS_ACTIVE.to_string();
                g.updated_at = now_rfc3339();
                g
            }
            _ => Goal::new(session_id, objective, budget),
        };
        inner.insert(session_id.to_string(), goal.clone());
        self.persist_locked(&inner);
        Ok(goal)
    }

    /// 改状态(+ 说明)。未知 status → Err。
    pub fn set_status(
        &self,
        session_id: &str,
        status: &str,
        note: Option<&str>,
    ) -> Result<Goal, String> {
        if !STATUSES.contains(&status) {
            return Err(format!("未知 status: {status}(支持 {STATUSES:?})"));
        }
        let mut inner = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        let Some(g) = inner.get_mut(session_id) else {
            return Err("本会话没有目标".to_string());
        };
        g.status = status.to_string();
        if let Some(n) = note.map(str::trim).filter(|n| !n.is_empty()) {
            g.note = Some(n.to_string());
        }
        g.updated_at = now_rfc3339();
        let out = g.clone();
        self.persist_locked(&inner);
        Ok(out)
    }

    /// 记一轮的开销(轮数 +1)。返回记账后的目标。
    pub fn record_turn(&self, session_id: &str, tokens: u64, seconds: u64) -> Option<Goal> {
        let mut inner = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        let g = inner.get_mut(session_id)?;
        g.tokens_used += tokens;
        g.time_used_seconds += seconds;
        g.turns += 1;
        g.updated_at = now_rfc3339();
        let out = g.clone();
        self.persist_locked(&inner);
        Some(out)
    }

    pub fn clear(&self, session_id: &str) -> bool {
        let mut inner = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        let existed = inner.remove(session_id).is_some();
        if existed {
            self.persist_locked(&inner);
        }
        existed
    }

    /// Codex 原生 goal 通知 → 本地镜像。下一轮启动会从这个镜像决定是否再次
    /// `thread/goal/set`，所以终态必须先落进来，否则旧 active 会把已完成目标复活。
    pub fn sync_codex(&self, session_id: &str, remote: &Value) -> Option<Goal> {
        let objective = remote
            .get("objective")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|s| !s.is_empty());
        let mut inner = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        let mut goal = match inner.get(session_id).cloned() {
            Some(goal) => goal,
            None => Goal::new(
                session_id,
                objective?,
                remote_u64(remote, &["tokenBudget", "token_budget"]).unwrap_or(0),
            ),
        };
        if let Some(objective) = objective {
            goal.objective = objective.to_string();
        }
        if let Some(status) = remote.get("status").and_then(Value::as_str) {
            if STATUSES.contains(&status) {
                goal.status = status.to_string();
            }
        }
        if remote.get("tokenBudget") == Some(&Value::Null) {
            goal.token_budget = 0;
        } else if let Some(value) = remote_u64(remote, &["tokenBudget", "token_budget"]) {
            goal.token_budget = value;
        }
        if let Some(value) = remote_u64(remote, &["tokensUsed", "tokens_used"]).or_else(|| {
            remote
                .pointer("/tokenUsage/total/totalTokens")
                .and_then(Value::as_u64)
        }) {
            goal.tokens_used = value;
        }
        if let Some(value) = remote_u64(remote, &["timeUsedSeconds", "time_used_seconds"]) {
            goal.time_used_seconds = value;
        }
        if let Some(value) = remote_u64(remote, &["turns", "turnCount"]) {
            goal.turns = value;
        }
        if let Some(note) = remote.get("note").and_then(Value::as_str) {
            goal.note = Some(note.to_string());
        }
        goal.updated_at = now_rfc3339();
        inner.insert(session_id.to_string(), goal.clone());
        self.persist_locked(&inner);
        Some(goal)
    }

    fn persist_locked(&self, inner: &HashMap<String, Goal>) {
        if self.path.as_os_str().is_empty() {
            return;
        }
        let mut v: Vec<&Goal> = inner.values().collect();
        v.sort_by(|a, b| a.session_id.cmp(&b.session_id));
        let Ok(text) = serde_json::to_string_pretty(&json!({ "goals": v })) else {
            return;
        };
        if let Some(parent) = self.path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let tmp = self.path.with_extension("json.tmp");
        if std::fs::write(&tmp, text).is_ok() {
            if let Err(e) = replace_persisted_file(&tmp, &self.path) {
                eprintln!("[goal] 持久化失败: {e}");
            }
        }
    }
}

fn remote_u64(value: &Value, keys: &[&str]) -> Option<u64> {
    keys.iter()
        .find_map(|key| value.get(*key).and_then(Value::as_u64))
}

/// Windows 不允许 `rename(tmp, existing)`；用同目录备份做可恢复替换。
fn replace_persisted_file(tmp: &Path, destination: &Path) -> std::io::Result<()> {
    if !destination.exists() {
        return std::fs::rename(tmp, destination);
    }
    let backup = destination.with_extension("json.bak");
    if backup.exists() {
        std::fs::remove_file(&backup)?;
    }
    std::fs::rename(destination, &backup)?;
    if let Err(e) = std::fs::rename(tmp, destination) {
        let _ = std::fs::rename(&backup, destination);
        return Err(e);
    }
    let _ = std::fs::remove_file(backup);
    Ok(())
}

fn read_file(path: &Path) -> HashMap<String, Goal> {
    let Ok(text) = std::fs::read_to_string(path) else {
        return HashMap::new();
    };
    let Ok(v) = serde_json::from_str::<Value>(&text) else {
        return HashMap::new();
    };
    let Some(arr) = v.get("goals").and_then(Value::as_array) else {
        return HashMap::new();
    };
    arr.iter()
        .filter_map(|g| serde_json::from_value::<Goal>(g.clone()).ok())
        .map(|g| (g.session_id.clone(), g))
        .collect()
}

/// 目标的 wire 形态(两种引擎共用;前端 GoalBar 直接读)。
pub fn goal_json(goal: &Goal, engine: &str) -> Value {
    let mut v = serde_json::to_value(goal).unwrap_or_else(|_| json!({}));
    v["engine"] = json!(engine);
    v["budgetExhausted"] = json!(goal.budget_exhausted());
    v
}

/// 发 `goal.updated`(两条腿同形)。
pub fn emit_updated(state: &AppState, goal: &Goal, engine: &str) {
    state.events.emit(
        EventDraft::new(&goal.session_id, "goal.updated", "goal")
            .payload(json!({ "goal": goal_json(goal, engine), "engine": engine })),
    );
}

pub fn emit_cleared(state: &AppState, session_id: &str, engine: &str) {
    state.events.emit(
        EventDraft::new(session_id, "goal.cleared", "goal").payload(json!({ "engine": engine })),
    );
}

/// 本地引擎的续跑决策。抽成纯函数以便直接断言(续跑逻辑写错的代价是无限刷 token)。
#[derive(Debug, Clone, PartialEq)]
pub enum Continuation {
    /// 起下一轮,正文如下。
    Continue(String),
    /// 停下,并把目标改成这个状态(附原因)。
    Stop { status: &'static str, note: String },
    /// 什么都不做(没有目标 / 目标已不是 active)。
    Idle,
}

/// 上一轮的收尾情况 → 是否续跑。
///
/// `turn_status` 为 `failed`/`cancelled` 时一律停:失败自动重试会把同一个错误刷成
/// 一串一模一样的 turn,取消更是用户明确说了「停」。
pub fn decide(goal: Option<&Goal>, turn_status: &str, last_text: &str) -> Continuation {
    let Some(g) = goal else {
        return Continuation::Idle;
    };
    if !g.is_active() {
        return Continuation::Idle;
    }
    if turn_status == "cancelled" {
        return Continuation::Stop {
            status: STATUS_PAUSED,
            note: "用户中止了本轮,目标已暂停".to_string(),
        };
    }
    if turn_status != "completed" {
        return Continuation::Stop {
            status: STATUS_PAUSED,
            note: "上一轮失败,目标已暂停;修好问题后可恢复".to_string(),
        };
    }
    if g.budget_exhausted() {
        return Continuation::Stop {
            status: STATUS_PAUSED,
            note: format!(
                "预算耗尽(已用 {} tokens / {} 轮),目标已暂停;加预算后可恢复",
                g.tokens_used, g.turns
            ),
        };
    }
    Continuation::Continue(continue_prompt(g, last_text))
}

/// 续跑轮的正文。
fn continue_prompt(goal: &Goal, last_text: &str) -> String {
    let mut s = format!(
        "【目标续跑】继续推进目标:{}\n\n\
         纪律:你在自动续跑模式下,用户现在可能不在,不要提问等待——直接做该做的事。\n\
         目标达成时调用 goal_update{{status:\"completed\", note:\"…\"}} 收尾;\
         确实卡住(缺信息、缺权限、外部依赖不可用)时调用 goal_update{{status:\"blocked\", note:\"…\"}} 并说明卡在哪。\n\
         别重复已经做完的工作:先核对当前实际状态(读文件 / scene_summary / 待办清单),再决定下一步。",
        goal.objective
    );
    let last = last_text.trim();
    if !last.is_empty() {
        let brief: String = last.chars().take(1200).collect();
        s.push_str("\n\n上一轮的收尾汇报:\n");
        s.push_str(&brief);
    }
    s
}

/// 本地引擎的目标状态工具名。
pub const GOAL_UPDATE_TOOL: &str = "goal_update";

/// 本地引擎的 `goal_update` 原生工具 spec(模型用它标完成/卡住)。
pub fn goal_update_spec() -> Value {
    json!({
        "type": "function",
        "function": {
            "name": GOAL_UPDATE_TOOL,
            "description": "更新当前目标的状态。目标达成 → completed;确实卡住(缺信息/缺权限/外部依赖不可用)→ blocked;\
    仍需继续自动推进 → continue;需要用户介入时可用 paused。note 写清理由(用户直接看这行)。没有正在推进的目标时调用会返回错误。",
            "parameters": {
                "type": "object",
                "properties": {
                    "status": {
                        "type": "string",
                        "enum": ["completed", "blocked", "continue", "paused"],
                        "description": "目标新状态"
                    },
                    "note": { "type": "string", "description": "一句话理由" }
                },
                "required": ["status"]
            }
        }
    })
}

/// 执行 `goal_update`(本地引擎的原生工具腿)。
pub fn dispatch_goal_update(state: &AppState, session_id: &str, args: &Value) -> (bool, String) {
    let status = args
        .get("status")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .trim();
    if status.is_empty() {
        return (
            false,
            "status 必填(completed|blocked|continue|paused)".to_string(),
        );
    }
    // 模型不该经这个工具把目标重新激活——那是用户的决定(GoalBar 的「恢复」)。
    if status == STATUS_ACTIVE {
        return (
            false,
            "不能经 goal_update 把目标改回 active(恢复目标由用户操作)".to_string(),
        );
    }
    let status = match goal_update_status(status) {
        Some(status) => status,
        None => return (false, format!("未知 status: {status}")),
    };
    let note = args.get("note").and_then(Value::as_str);
    match state.goals.set_status(session_id, status, note) {
        Ok(g) => {
            emit_updated(state, &g, crate::codex::config::ENGINE_LOCAL);
            (
                true,
                format!(
                    "目标状态已更新为 {}{}",
                    g.status,
                    g.note
                        .as_deref()
                        .map(|n| format!(":{n}"))
                        .unwrap_or_default()
                ),
            )
        }
        Err(e) => (false, e),
    }
}

fn goal_update_status(status: &str) -> Option<&str> {
    match status {
        "continue" => Some(STATUS_ACTIVE),
        STATUS_COMPLETED => Some(STATUS_COMPLETED),
        STATUS_BLOCKED => Some(STATUS_BLOCKED),
        STATUS_PAUSED => Some(STATUS_PAUSED),
        _ => None,
    }
}

// ---------------- REST ----------------

fn not_found(code: &str, message: String) -> axum::response::Response {
    use axum::response::IntoResponse;
    (
        axum::http::StatusCode::NOT_FOUND,
        axum::Json(json!({ "error": { "code": code, "message": message } })),
    )
        .into_response()
}

fn bad_request(code: &str, message: &str) -> axum::response::Response {
    use axum::response::IntoResponse;
    (
        axum::http::StatusCode::BAD_REQUEST,
        axum::Json(json!({ "error": { "code": code, "message": message } })),
    )
        .into_response()
}

/// GET /api/forge/sessions/{id}/goal → {goal|null}。
pub async fn get_goal(
    axum::extract::State(state): axum::extract::State<Arc<AppState>>,
    axum::extract::Path(id): axum::extract::Path<String>,
) -> axum::response::Response {
    use axum::response::IntoResponse;
    let Some(session) = state.sessions.get(&id) else {
        return not_found("SESSION_NOT_FOUND", format!("会话不存在: {id}"));
    };
    let engine = session.agent_engine.clone();
    // Codex 引擎:线程侧才是事实源(Codex 自己在推进),本地缓存只是镜像。
    if session.is_codex() {
        if let Some(thread) = session.codex_thread_id.as_deref() {
            if let Ok(v) = crate::codex::goal_read(&state, thread).await {
                return axum::Json(json!({ "goal": v, "engine": engine })).into_response();
            }
        }
    }
    let goal = state.goals.get(&id).map(|g| goal_json(&g, &engine));
    axum::Json(json!({ "goal": goal, "engine": engine })).into_response()
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PutGoalRequest {
    #[serde(default)]
    pub objective: String,
    #[serde(default)]
    pub token_budget: Option<u64>,
}

/// PUT /api/forge/sessions/{id}/goal {objective, tokenBudget?} → {goal}。
pub async fn put_goal(
    axum::extract::State(state): axum::extract::State<Arc<AppState>>,
    axum::extract::Path(id): axum::extract::Path<String>,
    axum::Json(req): axum::Json<PutGoalRequest>,
) -> axum::response::Response {
    use axum::response::IntoResponse;
    let Some(session) = state.sessions.get(&id) else {
        return not_found("SESSION_NOT_FOUND", format!("会话不存在: {id}"));
    };
    let goal = match state.goals.set(&id, &req.objective, req.token_budget) {
        Ok(g) => g,
        Err(e) => return bad_request("GOAL_INVALID", &e),
    };
    // Codex 引擎:先建唯一的 run/订阅桥，再 set active。该 RPC 会立即自动起 turn。
    if session.is_codex() {
        if let Err(e) = crate::codex::turn::start_goal_lifecycle(&state, &session, &goal).await {
            // 透传失败不回滚本地目标:本地这份仍是用户意图的记录,
            // 下一轮 thread/start 会重新推送。如实告知即可。
            eprintln!("[goal] Codex Goal 启动失败: {e}");
        }
    } else if let Err(e) = crate::agent::start_local_goal_lifecycle(&state, &session, &goal) {
        eprintln!("[goal] 本地 Goal 启动失败: {e}");
    }
    emit_updated(&state, &goal, &session.agent_engine);
    axum::Json(json!({ "goal": goal_json(&goal, &session.agent_engine) })).into_response()
}

/// DELETE /api/forge/sessions/{id}/goal → {ok}。
pub async fn delete_goal(
    axum::extract::State(state): axum::extract::State<Arc<AppState>>,
    axum::extract::Path(id): axum::extract::Path<String>,
) -> axum::response::Response {
    use axum::response::IntoResponse;
    let Some(session) = state.sessions.get(&id) else {
        return not_found("SESSION_NOT_FOUND", format!("会话不存在: {id}"));
    };
    if session.is_codex() {
        if let Some(thread) = session.codex_thread_id.as_deref() {
            if let Err(e) = crate::codex::goal_clear(&state, thread).await {
                return (
                    axum::http::StatusCode::BAD_GATEWAY,
                    axum::Json(json!({ "error": {
                        "code": "CODEX_UPSTREAM",
                        "message": format!("Codex Goal 清除失败: {e}")
                    }})),
                )
                    .into_response();
            }
        }
    }
    let existed = state.goals.clear(&id);
    if existed {
        emit_cleared(&state, &id, &session.agent_engine);
    }
    axum::Json(json!({ "ok": existed })).into_response()
}

/// POST /api/forge/sessions/{id}/goal/pause|resume → {goal}。
///
/// 两种引擎的 resume 都会立即续跑。Codex 必须先建立原生通知桥；本地引擎则后台
/// 重建一条 Goal turn（若当前已有 turn，交给其收尾接续）。
pub async fn set_goal_status(
    axum::extract::State(state): axum::extract::State<Arc<AppState>>,
    axum::extract::Path((id, action)): axum::extract::Path<(String, String)>,
) -> axum::response::Response {
    use axum::response::IntoResponse;
    let Some(session) = state.sessions.get(&id) else {
        return not_found("SESSION_NOT_FOUND", format!("会话不存在: {id}"));
    };
    let status = match action.as_str() {
        "pause" => STATUS_PAUSED,
        "resume" => STATUS_ACTIVE,
        other => return bad_request("GOAL_INVALID", &format!("未知动作: {other}")),
    };
    let previous = state.goals.get(&id);
    match state.goals.set_status(&id, status, None) {
        Ok(g) => {
            if session.is_codex() {
                let synced = if status == STATUS_ACTIVE {
                    crate::codex::turn::start_goal_lifecycle(&state, &session, &g).await
                } else if let Some(thread) = session.codex_thread_id.as_deref() {
                    crate::codex::goal_status_set(&state, thread, status)
                        .await
                        .map(|_| ())
                } else {
                    Ok(())
                };
                if let Err(e) = synced {
                    if let Some(previous) = previous {
                        let _ =
                            state
                                .goals
                                .set_status(&id, &previous.status, previous.note.as_deref());
                    }
                    return (
                        axum::http::StatusCode::BAD_GATEWAY,
                        axum::Json(json!({ "error": {
                            "code": "CODEX_UPSTREAM",
                            "message": format!("Codex Goal 状态同步失败: {e}")
                        }})),
                    )
                        .into_response();
                }
            } else if status == STATUS_ACTIVE {
                if let Err(e) = crate::agent::start_local_goal_lifecycle(&state, &session, &g) {
                    eprintln!("[goal] 本地 Goal 恢复失败: {e}");
                }
            }
            emit_updated(&state, &g, &session.agent_engine);
            axum::Json(json!({ "goal": goal_json(&g, &session.agent_engine) })).into_response()
        }
        Err(e) => bad_request("GOAL_INVALID", &e),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn store(tag: &str) -> (GoalStore, PathBuf) {
        let dir = std::env::temp_dir().join(format!(
            "agentd-goals-{tag}-{}-{}",
            std::process::id(),
            gend::timeutil::unix_millis()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        (GoalStore::load(dir.join("goals.json")), dir)
    }

    /// 设/改状态/记账/清除往返,且能从磁盘复读。
    #[test]
    fn goal_roundtrip_persists() {
        let (s, dir) = store("rt");
        assert!(s.get("s1").is_none());
        let g = s.set("s1", "  做个平台跳跃 demo  ", None).unwrap();
        assert_eq!(g.objective, "做个平台跳跃 demo");
        assert_eq!(g.status, STATUS_ACTIVE);
        assert_eq!(g.token_budget, DEFAULT_TOKEN_BUDGET);

        s.record_turn("s1", 1500, 42).unwrap();
        let g = s.record_turn("s1", 500, 8).unwrap();
        assert_eq!(g.tokens_used, 2000);
        assert_eq!(g.time_used_seconds, 50);
        assert_eq!(g.turns, 2);

        let g = s
            .set_status("s1", STATUS_BLOCKED, Some("缺美术素材"))
            .unwrap();
        assert_eq!(g.status, STATUS_BLOCKED);
        assert_eq!(g.note.as_deref(), Some("缺美术素材"));
        assert!(s.set_status("s1", "whatever", None).is_err());
        assert!(s.set("s1", "   ", None).is_err());

        let reread = GoalStore::load(dir.join("goals.json"));
        let g = reread.get("s1").expect("落盘后应可复读");
        assert_eq!(g.tokens_used, 2000);
        assert_eq!(g.status, STATUS_BLOCKED);

        assert!(reread.clear("s1"));
        assert!(!reread.clear("s1"));
        std::fs::remove_dir_all(&dir).ok();
    }

    /// 同一目标改预算 = 接着跑,进度不归零(否则「加点预算」会把已用量清空,
    /// 预算闸门等于形同虚设)。
    #[test]
    fn raising_budget_keeps_progress() {
        let (s, dir) = store("budget");
        s.set("s1", "目标甲", Some(1000)).unwrap();
        s.record_turn("s1", 900, 10).unwrap();
        let g = s.set("s1", "目标甲", Some(5000)).unwrap();
        assert_eq!(g.tokens_used, 900, "改预算不该清空已用量");
        assert_eq!(g.turns, 1);
        assert_eq!(g.token_budget, 5000);
        // 换了目标才归零。
        let g = s.set("s1", "目标乙", Some(5000)).unwrap();
        assert_eq!(g.tokens_used, 0);
        std::fs::remove_dir_all(&dir).ok();
    }

    /// 续跑决策:完成且有预算 → 续;预算耗尽 / 上轮失败 / 被取消 → 停。
    #[test]
    fn continuation_rules() {
        let (s, dir) = store("decide");
        let g = s.set("s1", "做个平台跳跃 demo", Some(10_000)).unwrap();
        match decide(Some(&g), "completed", "已加好角色实体。") {
            Continuation::Continue(text) => {
                assert!(text.contains("做个平台跳跃 demo"), "{text}");
                // 续跑轮里用户可能不在,必须明确禁止提问等待。
                assert!(text.contains("不要提问等待"), "{text}");
                assert!(text.contains("已加好角色实体。"), "{text}");
            }
            other => panic!("应续跑,实得 {other:?}"),
        }

        // 上轮失败:自动重试会把同一个错误刷成一串一样的 turn。
        assert!(matches!(
            decide(Some(&g), "failed", ""),
            Continuation::Stop {
                status: STATUS_PAUSED,
                ..
            }
        ));
        // 用户按了停止 = 明确说了别跑了。
        assert!(matches!(
            decide(Some(&g), "cancelled", ""),
            Continuation::Stop {
                status: STATUS_PAUSED,
                ..
            }
        ));

        s.record_turn("s1", 10_000, 1).unwrap();
        let spent = s.get("s1").unwrap();
        match decide(Some(&spent), "completed", "") {
            Continuation::Stop { status, note } => {
                assert_eq!(status, STATUS_PAUSED);
                assert!(note.contains("预算耗尽"), "{note}");
            }
            other => panic!("预算耗尽应停,实得 {other:?}"),
        }

        // 非 active 与无目标都不该有动作。
        let paused = s.set_status("s1", STATUS_PAUSED, None).unwrap();
        assert_eq!(decide(Some(&paused), "completed", ""), Continuation::Idle);
        assert_eq!(decide(None, "completed", ""), Continuation::Idle);
        std::fs::remove_dir_all(&dir).ok();
    }

    /// 轮数上限兜底:每轮只烧几个 token 也不能无限续。
    #[test]
    fn turn_cap_stops_cheap_loops() {
        let (s, dir) = store("cap");
        s.set("s1", "无尽目标", Some(0)).unwrap();
        for _ in 0..MAX_AUTO_TURNS {
            s.record_turn("s1", 1, 0).unwrap();
        }
        let g = s.get("s1").unwrap();
        assert!(g.budget_exhausted(), "轮数达上限应视为预算耗尽");
        assert!(matches!(
            decide(Some(&g), "completed", ""),
            Continuation::Stop { .. }
        ));
        std::fs::remove_dir_all(&dir).ok();
    }

    /// wire 形态带 engine 与 budgetExhausted(前端 GoalBar 直接读,不必自己算)。
    #[test]
    fn wire_shape_carries_engine_and_budget_flag() {
        let (s, dir) = store("wire");
        let g = s.set("s1", "目标", Some(100)).unwrap();
        let v = goal_json(&g, "codex");
        assert_eq!(v["engine"], "codex");
        assert_eq!(v["objective"], "目标");
        assert_eq!(v["budgetExhausted"], false);
        assert_eq!(v["status"], "active");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn codex_goal_terminal_state_updates_persistent_mirror() {
        let (s, dir) = store("codex-sync");
        s.set("s1", "完成存档系统", Some(10_000)).unwrap();
        let synced = s
            .sync_codex(
                "s1",
                &json!({
                    "objective": "完成存档系统", "status": "completed",
                    "tokenBudget": 10_000, "tokensUsed": 4_200,
                    "timeUsedSeconds": 31, "turns": 3, "note": "已完成"
                }),
            )
            .unwrap();
        assert_eq!(synced.status, STATUS_COMPLETED);
        assert_eq!(synced.tokens_used, 4_200);
        let reread = GoalStore::load(dir.join("goals.json"));
        let persisted = reread.get("s1").unwrap();
        assert_eq!(persisted.status, STATUS_COMPLETED);
        assert_eq!(persisted.turns, 3);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn goal_update_continue_keeps_goal_active() {
        assert_eq!(goal_update_status("continue"), Some(STATUS_ACTIVE));
        assert_eq!(goal_update_status("completed"), Some(STATUS_COMPLETED));
        assert_eq!(goal_update_status("whatever"), None);
        let spec = goal_update_spec();
        let choices = spec["function"]["parameters"]["properties"]["status"]["enum"]
            .as_array()
            .unwrap();
        assert!(choices.contains(&json!("continue")));
    }

    #[test]
    fn goal_objective_matches_codex_length_limit() {
        let (s, dir) = store("objective-limit");
        assert!(s.set("s1", &"x".repeat(4_000), None).is_ok());
        assert!(s.set("s2", &"x".repeat(4_001), None).is_err());
        std::fs::remove_dir_all(&dir).ok();
    }
}
