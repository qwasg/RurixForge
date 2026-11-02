//! F-GAME-4 wave.3:Plan DAG 调度器 + team 模式代码级编排。
//!
//! 分层:
//! - 纯函数调度核心(ready_ids/next_wave/failed_dependents/stall_reason):输入 todo 集,
//!   输出就绪集/失败传播集/停摆诊断——不碰 IO,单测确定性。
//! - `run_ready_waves` 波次执行器:同阶段就绪任务并行派发(并发上限 4,段内 join_all),
//!   完成置 completed/failed + todo.updated 事件;依赖失败向下游如实传播;
//!   循环依赖/悬空依赖如实报停摆,不死循环。
//! - `run_team_flow` 编排循环:leader 轮产出的结构化计划(带 role 的 queued 任务)→
//!   波次执行 → leader 编写 qa-tester 复测委派词后派发 → 问题回注 leader 修复轮
//!   (上限 max_fix_rounds,超限如实 failed)→ leader 编写 reviewer 终审委派词后派发
//!   (VERDICT: APPROVE 收束 / REJECT 回修复轮)。
//!
//! 注入面:leader 委派编写/修复轮与子代理派发都是泛型闭包(生产 = run_tool_loop /
//! run_nested_task;单测 = 脚本闭包全内存,不触网不触 MCP,mock 恒绿纪律)。
//! 事件面只复用既有信封(todo.updated / subagent.* 由派发方发;不发明新 kind)。

use serde_json::json;
use std::path::Path;

mod execution;

pub(crate) fn authorize_blocked_retry(dir: &Path, ids: &[String]) -> Result<(), String> {
    execution::authorize_blocked_retry(dir, ids)
}

pub(crate) fn authorize_production_retry(
    dir: &Path,
    ids: &[String],
    maximum: usize,
) -> Result<(), String> {
    execution::reserve_retry(dir, ids, maximum)
}

pub(crate) fn active_attempt(dir: &Path, id: &str) -> Option<u32> {
    execution::active_attempt(dir, id).ok().flatten()
}

pub(crate) fn advance_production_plan(dir: &Path) -> Result<(), String> {
    execution::advance_plan(dir)
}

pub(crate) fn legacy_revalidation_attempt(
    dir: &Path,
    id: &str,
    prompt: &str,
) -> Result<Option<u32>, String> {
    execution::legacy_revalidation_attempt(dir, id, prompt)
}

pub(crate) fn finish_revalidation(
    dir: &Path,
    id: &str,
    attempt: u32,
    status: &str,
    report: &str,
) -> Result<(), String> {
    execution::finish_attempt(dir, id, attempt, status, report)
}

use crate::agent::{PatchTodoRequest, TodoItem, TodoStore};
use crate::events::{EventBus, EventDraft};

/// 波内并行派发上限(与 llm.rs TASK_PARALLEL_MAX 同值;子代理以 IO 等待为主)。
const WAVE_PARALLEL_MAX: usize = 4;

/// 子代理派发请求(生产侧映射 run_nested_task 的 args;tool_call_id 为合成
/// parentToolCallId,如 "team-<todoId>",前端 SubagentBlock 按它归组)。
pub(crate) struct DispatchReq {
    pub subagent_type: String,
    pub prompt: String,
    pub description: String,
    pub tool_call_id: String,
    pub task_id: Option<String>,
    pub attempt: Option<u32>,
}

/// Provider completion is not a production completion. The host adapter only
/// returns Completed after accepting the worker's structured handoff.
pub(crate) enum DispatchOutcome {
    Completed(String),
    Blocked(String),
    Failed(String),
    Cancelled(String),
}

impl From<(bool, String)> for DispatchOutcome {
    fn from((ok, text): (bool, String)) -> Self {
        if ok {
            Self::Completed(text)
        } else {
            Self::Failed(text)
        }
    }
}

impl DispatchOutcome {
    fn into_result(self) -> (bool, String) {
        match self {
            Self::Completed(text) => (true, text),
            Self::Blocked(text) | Self::Failed(text) | Self::Cancelled(text) => (false, text),
        }
    }
}

/// team 编排上下文(全部借用;闭包注入见 run_team_flow)。
pub(crate) struct TeamFlowCtx<'a> {
    pub todos: &'a TodoStore,
    pub events: &'a EventBus,
    pub session_id: &'a str,
    /// 用户原始诉求(reviewer 终审的验收基准)。
    pub user_goal: &'a str,
    pub cancelled: &'a (dyn Fn() -> bool + Send + Sync),
    /// 修复轮上限(qa 复测失败与 reviewer REJECT 共用计数;超限如实 failed)。
    pub max_fix_rounds: usize,
    /// UltraPlan owns a subset of the session todos; unrelated work must not run or gate it.
    pub source_filter: Option<&'a str>,
    /// Shared editor/PIE state may only be used by one production worker at a time.
    pub serialize_engine: bool,
    /// Server-side evidence gate. QA_RESULT/VERDICT alone cannot satisfy UltraPlan.
    pub validate: Option<&'a (dyn Fn() -> Result<(), String> + Send + Sync)>,
    /// Durable UltraPlan scheduling journal, separate from verification reports.
    pub production_dir: Option<&'a Path>,
    /// Only confirmed server-side product failures authorize implementation repair.
    pub verification_issues:
        Option<&'a (dyn Fn() -> Result<Vec<serde_json::Value>, String> + Send + Sync)>,
}

// ---------- 纯函数调度核心 ----------

/// 可被编排器派发 = 带非空 role。无 role 的待办是 leader/用户自留项,调度器不碰。
pub(crate) fn is_schedulable(t: &TodoItem) -> bool {
    t.role
        .as_deref()
        .map(|r| !r.trim().is_empty())
        .unwrap_or(false)
}

/// 解析一条 dep 引用:优先按 id 精确匹配,其次按 title 匹配(排除自身;同名取创建序
/// 首条)。返回 None = 引用悬空。
fn dep_resolved<'a>(all: &'a [TodoItem], me: &str, dep: &str) -> Option<&'a TodoItem> {
    all.iter()
        .find(|t| t.id == dep)
        .or_else(|| all.iter().find(|t| t.title == dep && t.id != me))
}

/// 就绪集:自身 queued 且带 role,deps 全部解析成功且全 completed(保持输入序 = 创建序)。
pub(crate) fn ready_ids(all: &[TodoItem]) -> Vec<String> {
    all.iter()
        .filter(|t| is_schedulable(t) && t.status == "queued")
        .filter(|t| {
            t.deps.iter().all(|d| {
                dep_resolved(all, &t.id, d)
                    .map(|x| x.status == "completed")
                    .unwrap_or(false)
            })
        })
        .map(|t| t.id.clone())
        .collect()
}

/// 下一波:就绪集中与「首个就绪任务」同阶段者(同阶段并行派发;其余留给后续波)。
pub(crate) fn next_wave(all: &[TodoItem]) -> Vec<String> {
    let ready = ready_ids(all);
    let Some(first) = ready.first() else {
        return Vec::new();
    };
    let stage_of = |id: &str| {
        all.iter()
            .find(|t| t.id == id)
            .and_then(|t| t.stage.clone())
    };
    let first_stage = stage_of(first);
    ready
        .into_iter()
        .filter(|id| stage_of(id) == first_stage)
        .collect()
}

/// 失败传播:queued 且任一 dep 已 failed →(id, 如实原因)。调用方将其标 failed
/// 后再算下一波,失败会逐层传导到全部下游。
pub(crate) fn failed_dependents(all: &[TodoItem]) -> Vec<(String, String)> {
    all.iter()
        .filter(|t| is_schedulable(t) && t.status == "queued")
        .filter_map(|t| {
            let bad = t.deps.iter().find(|d| {
                dep_resolved(all, &t.id, d)
                    .map(|x| x.status == "failed")
                    .unwrap_or(false)
            })?;
            Some((
                t.id.clone(),
                format!("依赖「{bad}」失败,本任务被阻塞(未执行)"),
            ))
        })
        .collect()
}

/// 停摆诊断:仍有 queued 可调度任务但无就绪波、也无失败可传播时,逐任务给出如实原因
/// (依赖悬空 / 依赖不可调度 / 循环依赖)。
pub(crate) fn stall_reason(all: &[TodoItem]) -> String {
    let mut lines = Vec::new();
    for t in all
        .iter()
        .filter(|t| is_schedulable(t) && t.status == "queued")
    {
        for d in &t.deps {
            match dep_resolved(all, &t.id, d) {
                None => lines.push(format!("任务「{}」依赖「{d}」不存在(引用悬空)", t.title)),
                Some(x) if x.id == t.id => {
                    lines.push(format!("任务「{}」依赖自身(循环依赖)", t.title))
                }
                Some(x) if !is_schedulable(x) && x.status != "completed" => lines.push(format!(
                    "任务「{}」依赖「{}」无 role,调度器无法推进它",
                    t.title, x.title
                )),
                _ => {}
            }
        }
    }
    if lines.is_empty() {
        lines.push("剩余 queued 任务的依赖互相等待(循环依赖),无法推进".to_string());
    }
    lines.join(";")
}

// ---------- 波次执行器 ----------

#[derive(Debug, Default)]
pub(crate) struct WaveReport {
    pub completed: Vec<String>,
    /// (todoId, 失败原因)——含执行失败与依赖失败传播。
    pub failed: Vec<(String, String)>,
    /// Unavailable prerequisites/capabilities are not product defects.
    pub blocked: Vec<(String, String)>,
    /// Some = 调度停摆(循环依赖/悬空依赖),如实诊断文本。
    pub stalled: Option<String>,
    pub cancelled: bool,
}

/// todo 状态迁移 + todo.updated 事件(与 REST PATCH / todo_update 同 payload 形态)。
fn mark(ctx: &TeamFlowCtx<'_>, id: &str, status: &str, summary: Option<String>) {
    let req = PatchTodoRequest {
        status: Some(status.to_string()),
        summary,
        ..Default::default()
    };
    if let Ok(todo) = ctx.todos.patch(id, &req) {
        ctx.events.emit(
            EventDraft::new(&todo.session_id, "todo.updated", "todo").payload(json!({
                "id": todo.id, "title": todo.title, "status": todo.status,
                "summary": todo.summary,
            })),
        );
    }
}

/// 会话内可调度任务快照。
fn schedulable_tasks(ctx: &TeamFlowCtx<'_>) -> Vec<TodoItem> {
    let active = ctx.production_dir.map(|dir| {
        std::fs::read(dir.join("production.json"))
            .ok()
            .and_then(|bytes| serde_json::from_slice::<serde_json::Value>(&bytes).ok())
            .and_then(|report| report["activeTodoIds"].as_array().cloned())
            .unwrap_or_default()
            .into_iter()
            .filter_map(|id| id.as_str().map(str::to_owned))
            .collect::<std::collections::HashSet<_>>()
    });
    ctx.todos
        .list_by_session(ctx.session_id)
        .into_iter()
        .filter(|t| {
            ctx.source_filter.map_or_else(
                || !t.source.starts_with("ultraplan:"),
                |source| t.source == source,
            )
        })
        .filter(|t| active.as_ref().is_none_or(|ids| ids.contains(&t.id)))
        .collect()
}

fn uses_shared_engine(role: &str) -> bool {
    matches!(
        role,
        "scene-builder"
            | "logic-programmer"
            | "gameplay-scripter"
            | "material-smith"
            | "qa-tester"
            | "reviewer"
    )
}

/// Independent asset/file workers can share a wave. Engine workers use a dedicated
/// one-item wave, including material workers whose tools can adjust the live scene.
fn execution_wave(all: &[TodoItem], serialize_engine: bool) -> Vec<String> {
    let wave = next_wave(all);
    if !serialize_engine {
        return wave;
    }
    let engine = |id: &str| {
        all.iter()
            .find(|t| t.id == id)
            .and_then(|t| t.role.as_deref())
            .is_some_and(uses_shared_engine)
    };
    if wave.first().is_some_and(|id| engine(id)) {
        wave.into_iter().take(1).collect()
    } else {
        wave.into_iter().filter(|id| !engine(id)).collect()
    }
}

/// 波次推进:按 deps 分层,同阶段就绪任务并行派发(≤4 并发),直到无波可派。
/// 返回本次调用新造成的状态迁移(completed/failed)与停摆/取消旗。
pub(crate) async fn run_ready_waves<D, DFut>(ctx: &TeamFlowCtx<'_>, dispatch: &D) -> WaveReport
where
    D: Fn(DispatchReq) -> DFut,
    DFut: std::future::Future,
    DFut::Output: Into<DispatchOutcome>,
{
    let mut report = WaveReport::default();
    loop {
        if (ctx.cancelled)() {
            report.cancelled = true;
            return report;
        }
        let all = schedulable_tasks(ctx);
        if ctx.source_filter.is_some() {
            if let Some(dir) = ctx.production_dir {
                let mut reconciled = false;
                for task in all.iter().filter(|task| {
                    is_schedulable(task) && matches!(task.status.as_str(), "running" | "verifying")
                }) {
                    match execution::recover_result(dir, task, &all) {
                        Ok(Some((status, text))) => {
                            mark(ctx, &task.id, &status, Some(text.clone()));
                            match status.as_str() {
                                "completed" => report.completed.push(task.id.clone()),
                                "blocked" => report.blocked.push((task.id.clone(), text)),
                                _ => report.failed.push((task.id.clone(), text)),
                            }
                            reconciled = true;
                        }
                        Ok(None) => {
                            report.stalled = Some(format!(
                                "ULTRAPLAN_RECOVERY_REQUIRED: 任务{}仍有未确认终态的执行，未派发",
                                task.id
                            ));
                            return report;
                        }
                        Err(error) => {
                            report.stalled = Some(error);
                            return report;
                        }
                    }
                }
                if reconciled {
                    continue;
                }
            }
            // A downstream task has not failed execution when its prerequisite
            // failed. Release it only after that prerequisite's verified handoff.
            let mut changed = false;
            for task in all
                .iter()
                .filter(|t| is_schedulable(t) && t.status == "blocked")
            {
                if task
                    .summary
                    .as_deref()
                    .is_some_and(|s| s.starts_with("DEPENDENCY_BLOCKED:"))
                    && task.deps.iter().all(|d| {
                        dep_resolved(&all, &task.id, d).is_some_and(|t| t.status == "completed")
                    })
                {
                    mark(ctx, &task.id, "queued", None);
                    changed = true;
                }
            }
            if changed {
                continue;
            }
            let mut propagated = false;
            for task in all
                .iter()
                .filter(|t| is_schedulable(t) && t.status == "queued")
            {
                if let Some(dep) = task.deps.iter().find(|d| {
                    dep_resolved(&all, &task.id, d)
                        .is_some_and(|t| matches!(t.status.as_str(), "failed" | "blocked"))
                }) {
                    let why = format!("DEPENDENCY_BLOCKED: 依赖 {dep} 未通过交接，本任务未执行");
                    mark(ctx, &task.id, "blocked", Some(why.clone()));
                    report.blocked.push((task.id.clone(), why));
                    propagated = true;
                }
            }
            if propagated {
                continue;
            }
        }
        // 失败传播优先:先把被失败依赖阻塞的任务如实标掉,再算就绪波。
        let prop = if ctx.source_filter.is_some() {
            Vec::new()
        } else {
            failed_dependents(&all)
        };
        if !prop.is_empty() {
            for (id, why) in prop {
                mark(ctx, &id, "failed", Some(why.clone()));
                report.failed.push((id, why));
            }
            continue;
        }
        let wave = execution_wave(&all, ctx.serialize_engine);
        if wave.is_empty() {
            if all
                .iter()
                .any(|t| is_schedulable(t) && t.status == "queued")
            {
                report.stalled = Some(stall_reason(&all));
            }
            return report;
        }
        for chunk in wave.chunks(WAVE_PARALLEL_MAX) {
            if (ctx.cancelled)() {
                report.cancelled = true;
                return report;
            }
            let mut futs = Vec::new();
            for id in chunk {
                let Some(t) = all.iter().find(|x| &x.id == id) else {
                    continue;
                };
                let Some(prompt) = t
                    .prompt
                    .as_ref()
                    .filter(|p| !p.trim().is_empty() && p.trim() != t.title.trim())
                else {
                    let why = format!(
                        "任务「{}」缺少主 agent 编写的委派提示词（不能为空或仅为任务标题），未派发",
                        t.title
                    );
                    mark(ctx, id, "failed", Some(why.clone()));
                    report.failed.push((id.clone(), why));
                    continue;
                };
                let mut upstream = Vec::new();
                if ctx.source_filter.is_some() {
                    for dep in &t.deps {
                        let Some(dependency) = dep_resolved(&all, &t.id, dep) else {
                            continue;
                        };
                        let handoff = match ctx.production_dir {
                            Some(dir) => {
                                match execution::completed_handoff(dir, dependency, &all) {
                                    Ok(handoff) => handoff,
                                    Err(error) => {
                                        report.stalled = Some(error);
                                        return report;
                                    }
                                }
                            }
                            None => {
                                json!({"taskId":dependency.id,"title":dependency.title,"summary":dependency.summary})
                            }
                        };
                        upstream.push(handoff);
                    }
                }
                let attempt = if let Some(dir) = ctx.production_dir {
                    match execution::recover_result(dir, t, &all) {
                        Ok(Some((status, text))) => {
                            mark(ctx, id, &status, Some(text.clone()));
                            match status.as_str() {
                                "completed" => report.completed.push(id.clone()),
                                "blocked" => report.blocked.push((id.clone(), text)),
                                _ => report.failed.push((id.clone(), text)),
                            }
                            continue;
                        }
                        Ok(None) => {}
                        Err(error) => {
                            report.stalled = Some(error);
                            return report;
                        }
                    }
                    match execution::begin_attempt(dir, id, prompt) {
                        Ok(number) => Some(number),
                        Err(error) => {
                            report.stalled = Some(error);
                            return report;
                        }
                    }
                } else {
                    None
                };
                mark(ctx, id, "running", None);
                let req = DispatchReq {
                    subagent_type: t.role.clone().unwrap_or_default(),
                    prompt: format!(
                        "{}{}{}",
                        prompt,
                        t.summary
                            .as_ref()
                            .filter(|s| !s.starts_with("DEPENDENCY_BLOCKED:"))
                            .map(|s| format!("\n[上次执行报告]\n{s}"))
                            .unwrap_or_default(),
                        if upstream.is_empty() {
                            String::new()
                        } else {
                            format!("\n[上游已验证交接资料]\n{}", json!(upstream))
                        }
                    ),
                    description: t.title.clone(),
                    tool_call_id: format!("team-{}", t.id),
                    task_id: ctx.source_filter.map(|_| t.id.clone()),
                    attempt,
                };
                let id2 = id.clone();
                futs.push(async move { (id2, attempt, dispatch(req).await.into()) });
            }
            for (id, attempt, outcome) in futures_util::future::join_all(futs).await {
                let (status, text) = match outcome {
                    DispatchOutcome::Completed(text) => ("completed", text),
                    DispatchOutcome::Blocked(text) => (
                        if ctx.source_filter.is_some() {
                            "blocked"
                        } else {
                            "failed"
                        },
                        text,
                    ),
                    DispatchOutcome::Failed(text) => ("failed", text),
                    DispatchOutcome::Cancelled(text) => {
                        report.cancelled = true;
                        ("failed", text)
                    }
                };
                if let (Some(dir), Some(number)) = (ctx.production_dir, attempt) {
                    if let Err(error) = execution::finish_attempt(dir, &id, number, status, &text) {
                        report.stalled = Some(error);
                        return report;
                    }
                }
                mark(ctx, &id, status, Some(text.clone()));
                match status {
                    "completed" => report.completed.push(id),
                    "blocked" => report.blocked.push((id, text)),
                    _ => report.failed.push((id, text)),
                }
            }
            if report.cancelled {
                return report;
            }
        }
    }
}

// ---------- 裁决解析(qa / reviewer) ----------

/// reviewer 终审裁决。缺 VERDICT 标记按 REJECT 处理(如实注明),不静默放行。
#[derive(Debug, PartialEq)]
pub(crate) enum Verdict {
    Approve,
    Reject(String),
}

pub(crate) fn parse_verdict(text: &str) -> Verdict {
    let a = text.rfind("VERDICT: APPROVE");
    let r = text.rfind("VERDICT: REJECT");
    match (a, r) {
        (Some(ai), Some(ri)) if ai > ri => Verdict::Approve,
        (Some(_), None) => Verdict::Approve,
        (_, Some(_)) => Verdict::Reject(text.to_string()),
        (None, None) => Verdict::Reject(format!(
            "终审未按格式输出 VERDICT(按 REJECT 处理);完整报告:{text}"
        )),
    }
}

/// qa 复测结论:Ok(()) = PASS;Err(原因) = 有问题(派发失败/显式 FAIL/缺标记均如实算问题)。
pub(crate) fn parse_qa_result(ok: bool, text: &str) -> Result<(), String> {
    if !ok {
        return Err(format!("qa 复测派发失败:{text}"));
    }
    let p = text.rfind("QA_RESULT: PASS");
    let f = text.rfind("QA_RESULT: FAIL");
    match (p, f) {
        (Some(pi), Some(fi)) if pi > fi => Ok(()),
        (Some(_), None) => Ok(()),
        (_, Some(_)) => Err(text.to_string()),
        (None, None) => Err(format!(
            "复测报告未按格式输出 QA_RESULT(按未通过处理);完整报告:{text}"
        )),
    }
}

// ---------- 主 agent 委派编写与修复请求 ----------

const DELEGATION_DRAFT_PREFIX: &str = "[team 委派编写]";

/// These templates instruct the leader; they are never dispatched to a worker.
fn qa_delegation_request(goal: &str, t: &TodoItem) -> String {
    format!(
        "{DELEGATION_DRAFT_PREFIX} 请你作为主 agent，先阅读以下用户需求、任务委派词与执行结果，\
         再亲自为 qa-tester 编写本次复测的详细提示词。输出将原样交给该子 agent。\
         本轮仅输出完整委派提示词，不调用 task 或 plan_write，不直接执行复测或给出验收结论。\n\n\
         用户目标:{goal}\n任务:{}\n原任务委派词:\n{}\n执行工种:{}\n执行汇报:\n{}\n\n\
         提示词必须明确需求背景、复测范围、只测不改的边界、具体检查步骤和验收标准、\
         必须获取的真实证据(play/viewport/scene 查询类工具)及报告格式。\
         要求 qa-tester 逐项给出 PASS/FAIL 与证据，并以 `QA_RESULT: PASS` 或 \
         `QA_RESULT: FAIL`(附问题清单)结束报告。不要仅转发模板或任务标题。",
        t.title,
        t.prompt.as_deref().unwrap_or(&t.title),
        t.role.as_deref().unwrap_or("?"),
        t.summary.as_deref().unwrap_or("(无汇报)"),
    )
}

fn reviewer_delegation_request(goal: &str, tasks: &[TodoItem]) -> String {
    let mut lines = String::new();
    for t in tasks.iter().filter(|t| is_schedulable(t)) {
        lines.push_str(&format!(
            "- [{}] {} (工种:{})\n原任务委派词:{}\n执行汇报:{}\n",
            t.status,
            t.title,
            t.role.as_deref().unwrap_or("?"),
            t.prompt.as_deref().unwrap_or("(无委派记录)"),
            t.summary.as_deref().unwrap_or("(无汇报)")
        ));
    }
    format!(
        "{DELEGATION_DRAFT_PREFIX} 请你作为主 agent，先阅读以下用户需求、各任务委派词与执行结果，\
         再亲自为 reviewer 编写本次终审的详细提示词。输出将原样交给该子 agent。\
         本轮仅输出完整委派提示词，不调用 task 或 plan_write，不直接执行终审或给出验收结论。\n\n\
         用户目标:{goal}\n\n计划任务执行情况:\n{lines}\n\
         提示词必须明确需求背景、审查范围与边界、具体检查步骤、验收标准、证据和报告要求。\
         要求 reviewer 对照用户需求，以对抗式终审姿态按 Functionality(功能可用)→ Visual(画面正确)→ \
         Playability(可玩性)递进检查，发现足以否决的问题立即停止并输出，\
         最终以 `VERDICT: APPROVE` 或 `VERDICT: REJECT`(附理由)结束报告。不要仅转发模板或任务标题。"
    )
}

/// A missing leader response must never fall back to an automatic worker template.
/// None means the authoring turn was cancelled; no dispatch is allowed afterwards.
async fn author_delegation<L, LFut>(
    ctx: &TeamFlowCtx<'_>,
    leader_round: &L,
    request: String,
) -> Result<Option<String>, String>
where
    L: Fn(String) -> LFut,
    LFut: std::future::Future<Output = Result<(String, bool), String>>,
{
    if (ctx.cancelled)() {
        return Ok(None);
    }
    let response = leader_round(request).await;
    if (ctx.cancelled)() {
        return Ok(None);
    }
    match response {
        Ok((_, true)) => Ok(None),
        Ok((prompt, false)) if !prompt.trim().is_empty() => Ok(Some(prompt)),
        Ok(_) => Err("主 agent 未输出委派提示词，未启动子 agent".to_string()),
        Err(error) => Err(format!(
            "主 agent 编写委派提示词失败，未启动子 agent:{error}"
        )),
    }
}

fn fix_prompt(problems: &[String]) -> String {
    format!(
        "[team 编排回注] 以下问题在任务执行 / qa 复测 / reviewer 终审中被发现:\n{}\n\
         请针对每个问题用 plan_write 追加修复任务(每项带 role/prompt,必要时 deps/verify=qa);\
         确认无法修复或无需修复的,如实说明理由。",
        problems
            .iter()
            .map(|p| format!("- {p}"))
            .collect::<Vec<_>>()
            .join("\n")
    )
}

// ---------- team 编排循环 ----------

/// team 模式编排主流程(execute_turn 的 team 分支在 leader 首轮后调用)。
///
/// 返回 (status, text, error) 三元组,与 execute_turn 的 outcome 同形态:
/// - 没有任何带 role 的任务 → 如实失败;恢复全 completed 任务时仍复测、终审并验证证据;
/// - 波次执行 → leader 编写 verify=qa 复测委派词 → 复测 → 问题回注 leader 修复轮;
/// - 全部完成 → leader 编写 reviewer 委派词 → 终审:APPROVE 收束;REJECT 回修复轮;
/// - 修复轮超限 / 修复轮未产出新任务 / 调度停摆 → 如实 failed(不伪装通过)。
pub(crate) async fn run_team_flow<L, LFut, D, DFut>(
    ctx: &TeamFlowCtx<'_>,
    leader_text: String,
    leader_round: L,
    dispatch: D,
) -> (String, String, Option<String>)
where
    L: Fn(String) -> LFut,
    LFut: std::future::Future<Output = Result<(String, bool), String>>,
    D: Fn(DispatchReq) -> DFut,
    DFut: std::future::Future,
    DFut::Output: Into<DispatchOutcome>,
{
    if ctx.source_filter.is_some() {
        return run_production_flow(ctx, leader_text, leader_round, dispatch).await;
    }
    let has_queued = |tasks: &[TodoItem]| {
        tasks
            .iter()
            .any(|t| is_schedulable(t) && t.status == "queued")
    };
    if !schedulable_tasks(ctx).iter().any(is_schedulable) {
        return (
            "failed".to_string(),
            leader_text,
            Some("team 没有可执行任务，不能判定游戏完成".into()),
        );
    }
    let mut last_text = leader_text;
    let mut fix_rounds = 0usize;
    let mut review_attempts = 0usize;
    let mut qa_done: std::collections::HashSet<String> = Default::default();
    loop {
        if (ctx.cancelled)() {
            return ("cancelled".to_string(), String::new(), None);
        }
        let wave = run_ready_waves(ctx, &dispatch).await;
        for id in &wave.completed {
            qa_done.remove(id);
        }
        if wave.cancelled {
            return ("cancelled".to_string(), String::new(), None);
        }
        if let Some(stall) = wave.stalled {
            return (
                "failed".to_string(),
                last_text,
                Some(format!("team 编排无法推进:{stall}")),
            );
        }
        // 问题面 = 本轮执行失败 + qa 复测未通过(全部如实收集,不遮蔽)。
        let mut problems: Vec<String> = wave
            .failed
            .iter()
            .map(|(id, e)| format!("任务 {id} 执行失败:{e}"))
            .collect();
        let tasks = schedulable_tasks(ctx);
        let qa_targets: Vec<&TodoItem> = tasks
            .iter()
            .filter(|t| {
                is_schedulable(t)
                    && t.status == "completed"
                    && t.verify.as_deref() == Some("qa")
                    && !qa_done.contains(&t.id)
            })
            .collect();
        for t in qa_targets {
            let prompt = match author_delegation(
                ctx,
                &leader_round,
                qa_delegation_request(ctx.user_goal, t),
            )
            .await
            {
                Ok(Some(prompt)) => prompt,
                Ok(None) => return ("cancelled".to_string(), String::new(), None),
                Err(error) => return ("failed".to_string(), last_text, Some(error)),
            };
            let (ok, text) = dispatch(DispatchReq {
                subagent_type: "qa-tester".to_string(),
                prompt,
                description: format!("qa 复测:{}", t.title),
                tool_call_id: format!("team-qa-{}", t.id),
                task_id: None,
                attempt: None,
            })
            .await
            .into()
            .into_result();
            qa_done.insert(t.id.clone());
            if let Err(why) = parse_qa_result(ok, &text) {
                problems.push(format!("任务「{}」qa 复测未通过:{why}", t.title));
            }
        }
        if problems.is_empty() {
            if let Some(validate) = ctx.validate {
                if let Err(error) = validate() {
                    problems.push(format!("服务端自动验证未通过:{error}"));
                }
            }
        }
        // 无问题 → reviewer 终审(计划非空即终审,verify=reviewer 任务也归口于此)。
        if problems.is_empty() {
            let prompt = match author_delegation(
                ctx,
                &leader_round,
                reviewer_delegation_request(ctx.user_goal, &tasks),
            )
            .await
            {
                Ok(Some(prompt)) => prompt,
                Ok(None) => return ("cancelled".to_string(), String::new(), None),
                Err(error) => return ("failed".to_string(), last_text, Some(error)),
            };
            review_attempts += 1;
            let (ok, text) = dispatch(DispatchReq {
                subagent_type: "reviewer".to_string(),
                prompt,
                description: format!("reviewer 终审(第 {review_attempts} 次)"),
                tool_call_id: format!("team-review-{review_attempts}"),
                task_id: None,
                attempt: None,
            })
            .await
            .into()
            .into_result();
            if !ok {
                return (
                    "failed".to_string(),
                    last_text,
                    Some(format!("reviewer 终审派发失败:{text}")),
                );
            }
            match parse_verdict(&text) {
                Verdict::Approve => {
                    if let Some(validate) = ctx.validate {
                        if let Err(error) = validate() {
                            problems.push(format!("终审后的服务端证据复核未通过:{error}"));
                        }
                    }
                    if problems.is_empty() {
                        let done = tasks
                            .iter()
                            .filter(|t| is_schedulable(t) && t.status == "completed")
                            .count();
                        let final_text = format!(
                            "{last_text}\n\n[team 编排] 计划任务 {done} 项全部完成;\
                         修复轮 {fix_rounds} 次;reviewer 终审 APPROVE。"
                        );
                        return ("completed".to_string(), final_text, None);
                    }
                }
                Verdict::Reject(reason) => {
                    problems.push(format!("reviewer 终审 REJECT:{reason}"));
                }
            }
        }
        // 修复轮(qa 失败与 reviewer REJECT 共用上限;超限如实 failed)。
        fix_rounds += 1;
        if fix_rounds > ctx.max_fix_rounds {
            return (
                "failed".to_string(),
                last_text,
                Some(format!(
                    "修复轮已达上限 {},问题仍未解决:{}",
                    ctx.max_fix_rounds,
                    problems.join(" | ")
                )),
            );
        }
        match leader_round(fix_prompt(&problems)).await {
            Err(e) => {
                return (
                    "failed".to_string(),
                    last_text,
                    Some(format!("leader 修复轮失败:{e}")),
                )
            }
            Ok((_, true)) => return ("cancelled".to_string(), String::new(), None),
            Ok((text, _)) => {
                if !text.trim().is_empty() {
                    last_text = text;
                }
            }
        }
        if !has_queued(&schedulable_tasks(ctx)) {
            return (
                "failed".to_string(),
                last_text,
                Some(format!(
                    "leader 修复轮未产出新任务,问题未解决:{}",
                    problems.join(" | ")
                )),
            );
        }
    }
}

/// UltraPlan uses verified worker handoffs, one integrated QA per candidate, and
/// explicit retries of existing tasks. Ordinary Team retains its original policy.
fn recollect_evidence(
    ctx: &TeamFlowCtx<'_>,
    candidate: &str,
    qa_cache: &mut std::collections::HashMap<String, String>,
    recollections: &mut std::collections::HashSet<String>,
) -> Result<bool, String> {
    let allowed = match ctx.production_dir {
        Some(dir) => execution::reserve_evidence_recollection(dir, candidate)?,
        None => recollections.insert(candidate.into()),
    };
    if allowed {
        if let Some(dir) = ctx.production_dir {
            execution::invalidate_qa(dir, candidate)?;
        }
        qa_cache.remove(candidate);
    }
    Ok(allowed)
}

async fn run_production_flow<L, LFut, D, DFut>(
    ctx: &TeamFlowCtx<'_>,
    leader_text: String,
    leader_round: L,
    dispatch: D,
) -> (String, String, Option<String>)
where
    L: Fn(String) -> LFut,
    LFut: std::future::Future<Output = Result<(String, bool), String>>,
    D: Fn(DispatchReq) -> DFut,
    DFut: std::future::Future,
    DFut::Output: Into<DispatchOutcome>,
{
    let mut last_text = leader_text;
    let mut rounds = match ctx.production_dir.map(execution::fix_rounds).transpose() {
        Ok(value) => value.unwrap_or(0),
        Err(error) => return ("failed".into(), last_text, Some(error)),
    };
    let mut qa_cache = std::collections::HashMap::<String, String>::new();
    let mut evidence_recollections = std::collections::HashSet::new();
    if let Some(dir) = ctx.production_dir {
        let recovery_tasks = schedulable_tasks(ctx);
        for task in recovery_tasks
            .iter()
            .filter(|t| is_schedulable(t) && t.status == "completed")
        {
            if let Err(error) = execution::validate_completed(dir, task, &recovery_tasks) {
                return ("failed".into(), last_text, Some(error));
            }
        }
    }
    let mut transient_candidate = 0u64;
    let mut reviews = 0usize;
    loop {
        if (ctx.cancelled)() {
            return ("cancelled".into(), String::new(), None);
        }
        let wave = run_ready_waves(ctx, &dispatch).await;
        if wave.cancelled {
            return ("cancelled".into(), String::new(), None);
        }
        if let Some(error) = wave.stalled {
            return ("failed".into(), last_text, Some(error));
        }
        if !wave.completed.is_empty() {
            transient_candidate += 1;
        }
        let tasks = schedulable_tasks(ctx);
        if !tasks.iter().any(is_schedulable) {
            return (
                "failed".into(),
                last_text,
                Some("team 没有可执行任务，不能判定游戏完成".into()),
            );
        }
        let blockers: Vec<_> = tasks
            .iter()
            .filter(|t| {
                is_schedulable(t)
                    && t.status == "blocked"
                    && !t
                        .summary
                        .as_deref()
                        .is_some_and(|s| s.starts_with("DEPENDENCY_BLOCKED:"))
            })
            .map(|t| {
                format!(
                    "任务 {}「{}」阻塞：{}",
                    t.id,
                    t.title,
                    t.summary.as_deref().unwrap_or("缺少阻塞详情")
                )
            })
            .collect();
        if !blockers.is_empty() {
            return (
                "failed".into(),
                last_text,
                Some(format!(
                    "ULTRAPLAN_PRODUCTION_BLOCKED: {}。未派发修复任务；先解决能力或环境阻塞。",
                    blockers.join("\n")
                )),
            );
        }
        let mut problems: Vec<_> = tasks
            .iter()
            .filter(|t| is_schedulable(t) && t.status == "failed")
            .map(|t| {
                format!(
                    "任务 {}「{}」执行失败：{}",
                    t.id,
                    t.title,
                    t.summary.as_deref().unwrap_or("缺少失败详情")
                )
            })
            .collect();
        let all_completed = tasks
            .iter()
            .filter(|t| is_schedulable(t))
            .all(|t| t.status == "completed");
        if problems.is_empty() && !all_completed {
            return (
                "failed".into(),
                last_text,
                Some("ULTRAPLAN_PRODUCTION_BLOCKED: 仍有任务未通过依赖交接，不能启动终验".into()),
            );
        }
        if problems.is_empty() {
            let candidate = match ctx.production_dir.map(execution::candidate).transpose() {
                Ok(value) => value.unwrap_or_else(|| format!("transient-{transient_candidate}")),
                Err(error) => return ("failed".into(), last_text, Some(error)),
            };
            let prior = match ctx.production_dir {
                Some(dir) => match execution::qa_result(dir, &candidate) {
                    Ok(report) => report,
                    Err(error) => return ("failed".into(), last_text, Some(error)),
                },
                None => qa_cache.get(&candidate).cloned(),
            };
            let unchanged_failed_candidate = !wave.completed.is_empty()
                && prior
                    .as_ref()
                    .is_some_and(|report| parse_qa_result(true, report).is_err());
            let qa = if let Some(report) = prior {
                report
            } else {
                let details = tasks
                    .iter()
                    .filter(|t| is_schedulable(t))
                    .map(|t| {
                        format!(
                            "任务id:{}\n任务:{}\n委派:{}\n完整交接报告:{}",
                            t.id,
                            t.title,
                            t.prompt.as_deref().unwrap_or(""),
                            t.summary.as_deref().unwrap_or("")
                        )
                    })
                    .collect::<Vec<_>>()
                    .join("\n\n");
                let request = format!("{DELEGATION_DRAFT_PREFIX} 请为本候选版本亲自编写一份整合 qa-tester 委派词。用户目标：{}。\n候选版本:{candidate}\n{details}\n逐项覆盖批准 checks.json 的全部自动检查及核心操作、胜负和重开，用 ultraplan_verify 获取当前版本的真实证据；只测不改。不要按历史任务拆成多个 QA。完整报告保留每项预期、实际及证据路径，结尾 QA_RESULT: PASS 或 QA_RESULT: FAIL。仅输出委派词，不调用 task/plan_write。", ctx.user_goal);
                let prompt = match author_delegation(ctx, &leader_round, request).await {
                    Ok(Some(prompt)) => prompt,
                    Ok(None) => return ("cancelled".into(), String::new(), None),
                    Err(error) => return ("failed".into(), last_text, Some(error)),
                };
                let outcome: DispatchOutcome = dispatch(DispatchReq {
                    subagent_type: "qa-tester".into(),
                    prompt,
                    description: "候选版本整合 QA".into(),
                    tool_call_id: format!("team-qa-candidate-{candidate}"),
                    task_id: None,
                    attempt: None,
                })
                .await
                .into();
                let report = match outcome {
                    DispatchOutcome::Completed(report) => report,
                    DispatchOutcome::Cancelled(_) => {
                        return ("cancelled".into(), String::new(), None)
                    }
                    DispatchOutcome::Blocked(error) => {
                        return (
                            "failed".into(),
                            last_text,
                            Some(format!("ULTRAPLAN_PRODUCTION_BLOCKED: {error}")),
                        )
                    }
                    DispatchOutcome::Failed(error) => {
                        return (
                            "failed".into(),
                            last_text,
                            Some(format!("QA 执行失败（未创建产品修复）：{error}")),
                        )
                    }
                };
                if let Some(dir) = ctx.production_dir {
                    if let Err(error) = execution::record_qa(dir, &candidate, &report) {
                        return ("failed".into(), last_text, Some(error));
                    }
                } else {
                    qa_cache.insert(candidate.clone(), report.clone());
                }
                report
            };
            if let Some(classify) = ctx.verification_issues {
                let issues = match classify() {
                    Ok(issues) => issues,
                    Err(error) => return ("failed".into(), last_text, Some(error)),
                };
                if issues.iter().any(|issue| {
                    !matches!(
                        issue["status"].as_str(),
                        Some("missing" | "stale" | "failed")
                    )
                }) {
                    return ("failed".into(), last_text, Some(format!("ULTRAPLAN_VERIFICATION_BLOCKED: {}；保留产物，未创建产品修复。\n完整 QA：{qa}", json!(issues))));
                }
                if issues
                    .iter()
                    .any(|issue| matches!(issue["status"].as_str(), Some("missing" | "stale")))
                {
                    match recollect_evidence(ctx, &candidate, &mut qa_cache, &mut evidence_recollections) {
                        Ok(true) => continue,
                        Ok(false) => return ("failed".into(), last_text, Some(format!("ULTRAPLAN_EVIDENCE_INCOMPLETE: {}；本候选版本已补取证一次，未创建产品修复。\n完整 QA：{qa}", json!(issues)))),
                        Err(error) => return ("failed".into(), last_text, Some(error)),
                    }
                }
                problems.extend(
                    issues
                        .iter()
                        .filter(|issue| issue["status"] == "failed")
                        .map(|issue| format!("服务端已确认产品失败：{issue}\n完整 QA：{qa}")),
                );
                if problems.is_empty() {
                    if let Err(error) = parse_qa_result(true, &qa) {
                        return ("failed".into(), last_text, Some(format!("UP_QA_UNCONFIRMED: QA 未通过，但没有服务端确认的产品缺陷，不派制作修复。\n{error}")));
                    }
                }
            } else if let Err(error) = parse_qa_result(true, &qa) {
                problems.push(error);
            }
            if unchanged_failed_candidate && !problems.is_empty() {
                return ("failed".into(), last_text, Some("ULTRAPLAN_REPAIR_NO_PROGRESS: 修复后正式游戏文件版本未变化，既有 QA 失败仍有效；停止重复派遣".into()));
            }
            if problems.is_empty() {
                if let Some(validate) = ctx.validate {
                    if let Err(error) = validate() {
                        match recollect_evidence(
                            ctx,
                            &candidate,
                            &mut qa_cache,
                            &mut evidence_recollections,
                        ) {
                            Ok(true) => continue,
                            Ok(false) => {}
                            Err(persist_error) => {
                                return ("failed".into(), last_text, Some(persist_error))
                            }
                        }
                        // Missing/stale evidence is a verification failure, not an
                        // instruction to rebuild the approved game scope.
                        return (
                            "failed".into(),
                            last_text,
                            Some(format!(
                                "服务端自动验证未通过（保留产物，需补足验证证据）：{error}"
                            )),
                        );
                    }
                }
            }
        }
        if problems.is_empty() {
            let prompt = match author_delegation(
                ctx,
                &leader_round,
                reviewer_delegation_request(ctx.user_goal, &tasks),
            )
            .await
            {
                Ok(Some(prompt)) => prompt,
                Ok(None) => return ("cancelled".into(), String::new(), None),
                Err(error) => return ("failed".into(), last_text, Some(error)),
            };
            reviews += 1;
            let outcome: DispatchOutcome = dispatch(DispatchReq {
                subagent_type: "reviewer".into(),
                prompt,
                description: format!("reviewer 终审(第 {reviews} 次)"),
                tool_call_id: format!("team-review-{reviews}"),
                task_id: None,
                attempt: None,
            })
            .await
            .into();
            let text = match outcome {
                DispatchOutcome::Completed(text) => text,
                DispatchOutcome::Cancelled(_) => return ("cancelled".into(), String::new(), None),
                other => {
                    return (
                        "failed".into(),
                        last_text,
                        Some(format!("reviewer 终审执行未完成:{}", other.into_result().1)),
                    )
                }
            };
            match parse_verdict(&text) {
                Verdict::Approve => {
                    if let Some(validate) = ctx.validate {
                        if let Err(error) = validate() {
                            return ("failed".into(), last_text, Some(error));
                        }
                    }
                    return ("completed".into(), format!("{last_text}\n\n[team 编排] 计划任务 {} 项全部完成；修复轮 {rounds} 次；整合 QA 与 reviewer APPROVE。", tasks.iter().filter(|t| is_schedulable(t)).count()), None);
                }
                Verdict::Reject(error) => {
                    if let Some(classify) = ctx.verification_issues {
                        let issues = match classify() {
                            Ok(issues) => issues,
                            Err(error) => return ("failed".into(), last_text, Some(error)),
                        };
                        if issues.is_empty()
                            || issues.iter().any(|issue| issue["status"] != "failed")
                        {
                            return ("failed".into(), last_text, Some(format!("UP_REVIEW_UNCONFIRMED: 审阅未通过，但未取得可修复的服务端产品失败；保留产物。{error}\n{}", json!(issues))));
                        }
                        problems.extend(
                            issues
                                .iter()
                                .map(|issue| format!("服务端已确认产品失败：{issue}\n{error}")),
                        );
                    } else {
                        problems.push(error);
                    }
                }
            }
        }
        if rounds >= ctx.max_fix_rounds {
            return (
                "failed".into(),
                last_text,
                Some(format!(
                    "修复轮已达上限 {}，问题仍未解决:{}",
                    ctx.max_fix_rounds,
                    problems.join("\n")
                )),
            );
        }
        let task_context = tasks
            .iter()
            .filter(|t| is_schedulable(t))
            .map(|t| format!("{} [{}] {}", t.id, t.status, t.title))
            .collect::<Vec<_>>()
            .join("\n");
        let request = format!("[team 定向修复] 以下完整证据需要处理：\n{}\n已有批准任务：\n{task_context}\n请定位根因，只用 todo_update {{id,prompt,status:\"queued\"}} 为确需修复的已有任务编写具体修复委派并授权一次重试。prompt 必须根据本次证据实际更新，写清保留范围、修改点与交接验收。不得 plan_write 追加修复任务，不得重建全部游戏。DEPENDENCY_BLOCKED 任务从未执行，不是额外产品缺陷，不重排它们；上游交接通过后系统释放。无法修复的环境/能力问题如实报告。", problems.join("\n\n"));
        match leader_round(request).await {
            Ok((_, true)) => return ("cancelled".into(), String::new(), None),
            Ok((text, false)) => {
                if !text.trim().is_empty() {
                    last_text = text;
                }
            }
            Err(error) => return ("failed".into(), last_text, Some(error)),
        }
        if (ctx.cancelled)() {
            return ("cancelled".into(), String::new(), None);
        }
        let after = schedulable_tasks(ctx);
        if after
            .iter()
            .any(|t| is_schedulable(t) && !tasks.iter().any(|old| old.id == t.id))
        {
            return (
                "failed".into(),
                last_text,
                Some(
                    "ULTRAPLAN_REPAIR_SCOPE: 修复轮新增了任务，未派发；只能定向重试已有批准任务"
                        .into(),
                ),
            );
        }
        let mut retry = Vec::new();
        for task in after
            .iter()
            .filter(|t| is_schedulable(t) && t.status == "queued")
        {
            let old = tasks.iter().find(|old| old.id == task.id).unwrap();
            if !matches!(old.status.as_str(), "failed" | "completed")
                || task.prompt == old.prompt
                || task.prompt.as_deref().is_none_or(|p| p.trim().is_empty())
            {
                return ("failed".into(), last_text, Some(format!("ULTRAPLAN_RETRY_NOT_AUTHORIZED: {} 未提供针对当前失败的新委派，不执行重复重试", task.id)));
            }
            retry.push(task.id.clone());
        }
        if retry.is_empty() {
            return (
                "failed".into(),
                last_text,
                Some(format!(
                    "主 agent 未授权任何已有任务的定向修复：{}",
                    problems.join("\n")
                )),
            );
        }
        if let Some(dir) = ctx.production_dir {
            if let Err(error) = execution::reserve_retry(dir, &retry, ctx.max_fix_rounds) {
                return ("failed".into(), last_text, Some(error));
            }
        }
        rounds += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::NewTodo;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex};

    fn test_env(tag: &str) -> (Arc<TodoStore>, Arc<EventBus>, std::path::PathBuf) {
        let dir = std::env::temp_dir().join(format!(
            "agentd-plan-{tag}-{}-{}",
            std::process::id(),
            crate::events::new_id("t")
        ));
        let todos = Arc::new(TodoStore::load(dir.join("todos.json")));
        let events = Arc::new(EventBus::new(dir.join("events"), 64));
        (todos, events, dir)
    }

    fn seed(
        todos: &TodoStore,
        sid: &str,
        title: &str,
        role: Option<&str>,
        deps: &[&str],
        stage: Option<&str>,
        verify: Option<&str>,
    ) -> TodoItem {
        todos
            .create(
                sid,
                NewTodo {
                    title: title.to_string(),
                    role: role.map(str::to_string),
                    deps: deps.iter().map(|d| d.to_string()).collect(),
                    stage: stage.map(str::to_string),
                    verify: verify.map(str::to_string),
                    prompt: Some(format!("做「{title}」")),
                    ..Default::default()
                },
            )
            .unwrap()
    }

    fn never_cancel() -> impl Fn() -> bool + Send + Sync {
        || false
    }

    fn production_task(todos: &TodoStore, title: &str, deps: Vec<String>) -> TodoItem {
        todos
            .create(
                "s",
                NewTodo {
                    title: title.into(),
                    role: Some("logic-programmer".into()),
                    source: Some("ultraplan:regression:1".into()),
                    prompt: Some(format!("实现并验证{title}的具体交接")),
                    deps,
                    verify: Some("qa".into()),
                    ..Default::default()
                },
            )
            .unwrap()
    }

    #[tokio::test]
    async fn production_blocked_worker_never_completes_or_dispatches_dependents_or_repairs() {
        let (todos, events, dir) = test_env("blockedhandoff");
        let first = production_task(&todos, "基础脚本", vec![]);
        let second = production_task(&todos, "下游场景", vec![first.id.clone()]);
        let cancelled = never_cancel();
        let ctx = TeamFlowCtx {
            todos: &todos,
            events: &events,
            session_id: "s",
            user_goal: "game",
            cancelled: &cancelled,
            max_fix_rounds: 5,
            source_filter: Some("ultraplan:regression:1"),
            serialize_engine: true,
            validate: None,
            production_dir: None,
            verification_issues: None,
        };
        let count = AtomicUsize::new(0);
        let dispatch = |_req: DispatchReq| {
            count.fetch_add(1, Ordering::SeqCst);
            async {
                DispatchOutcome::Blocked("缺少脚本写入能力，未生成任何产物".into())
            }
        };
        let leader =
            |_request: String| async { panic!("能力阻塞不得开启修复或QA委派") };
        let (status, _, error) = run_team_flow(&ctx, String::new(), leader, dispatch).await;
        assert_eq!(status, "failed");
        assert!(error.unwrap().contains("PRODUCTION_BLOCKED"));
        assert_eq!(count.load(Ordering::SeqCst), 1);
        let tasks = todos.list_by_session("s");
        assert_eq!(
            tasks.iter().find(|t| t.id == first.id).unwrap().status,
            "blocked"
        );
        let dependent = tasks.iter().find(|t| t.id == second.id).unwrap();
        assert_eq!(dependent.status, "blocked");
        assert!(dependent
            .summary
            .as_deref()
            .unwrap()
            .starts_with("DEPENDENCY_BLOCKED:"));
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[tokio::test]
    async fn production_twelve_tasks_use_one_integrated_qa_and_preserve_reports() {
        let (todos, events, dir) = test_env("integratedqa");
        for index in 0..12 {
            production_task(&todos, &format!("模块{index}"), vec![]);
        }
        let cancelled = never_cancel();
        let ctx = TeamFlowCtx {
            todos: &todos,
            events: &events,
            session_id: "s",
            user_goal: "game",
            cancelled: &cancelled,
            max_fix_rounds: 5,
            source_filter: Some("ultraplan:regression:1"),
            serialize_engine: true,
            validate: None,
            production_dir: None,
            verification_issues: None,
        };
        let qa = AtomicUsize::new(0);
        let workers = AtomicUsize::new(0);
        let report = format!(
            "{}关键接口及真实产物路径在报告尾部",
            "详细验证结果".repeat(200)
        );
        let dispatch = |req: DispatchReq| {
            let text = match req.subagent_type.as_str() {
                "qa-tester" => {
                    qa.fetch_add(1, Ordering::SeqCst);
                    "QA_RESULT: PASS".into()
                }
                "reviewer" => "VERDICT: APPROVE".into(),
                _ => {
                    workers.fetch_add(1, Ordering::SeqCst);
                    report.clone()
                }
            };
            async { DispatchOutcome::Completed(text) }
        };
        let leader = |request: String| async move {
            assert!(request.contains("关键接口及真实产物路径在报告尾部"));
            Ok((request, false))
        };
        let (status, _, error) = run_team_flow(&ctx, String::new(), leader, dispatch).await;
        assert_eq!(status, "completed", "{error:?}");
        assert_eq!(workers.load(Ordering::SeqCst), 12);
        assert_eq!(qa.load(Ordering::SeqCst), 1);
        assert!(todos
            .list_by_session("s")
            .iter()
            .all(|t| t.summary.as_deref() == Some(report.as_str())));
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[tokio::test]
    async fn production_failed_handoff_retries_only_original_then_releases_dependency() {
        let (todos, events, dir) = test_env("targetedretry");
        let first = production_task(&todos, "基础接口", vec![]);
        let second = production_task(&todos, "消费接口", vec![first.id.clone()]);
        let cancelled = never_cancel();
        let ctx = TeamFlowCtx {
            todos: &todos,
            events: &events,
            session_id: "s",
            user_goal: "game",
            cancelled: &cancelled,
            max_fix_rounds: 5,
            source_filter: Some("ultraplan:regression:1"),
            serialize_engine: true,
            validate: None,
            production_dir: None,
            verification_issues: None,
        };
        let first_attempts = AtomicUsize::new(0);
        let second_attempts = AtomicUsize::new(0);
        let dispatch = |req: DispatchReq| {
            let outcome = if req.task_id.as_deref() == Some(first.id.as_str()) {
                if first_attempts.fetch_add(1, Ordering::SeqCst) == 0 {
                    DispatchOutcome::Failed("接口产物缺失".into())
                } else {
                    DispatchOutcome::Completed("接口已落盘并通过交接".into())
                }
            } else if req.task_id.as_deref() == Some(second.id.as_str()) {
                assert_eq!(first_attempts.load(Ordering::SeqCst), 2);
                second_attempts.fetch_add(1, Ordering::SeqCst);
                DispatchOutcome::Completed("消费接口通过".into())
            } else {
                DispatchOutcome::Completed(
                    if req.subagent_type == "qa-tester" {
                        "QA_RESULT: PASS"
                    } else {
                        "VERDICT: APPROVE"
                    }
                    .into(),
                )
            };
            async { outcome }
        };
        let leader = |request: String| {
            if !request.starts_with(DELEGATION_DRAFT_PREFIX) {
                assert!(request.contains(&first.id));
                todos
                    .patch(
                        &first.id,
                        &PatchTodoRequest {
                            status: Some("queued".into()),
                            prompt: Some("只补齐基础接口产物并重新验证，不重建消费者".into()),
                            ..Default::default()
                        },
                    )
                    .unwrap();
            }
            async { Ok(("已亲自编写本次委派".into(), false)) }
        };
        let (status, _, error) = run_team_flow(&ctx, String::new(), leader, dispatch).await;
        assert_eq!(status, "completed", "{error:?}");
        assert_eq!(first_attempts.load(Ordering::SeqCst), 2);
        assert_eq!(second_attempts.load(Ordering::SeqCst), 1);
        assert_eq!(todos.list_by_session("s").len(), 2);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[tokio::test]
    async fn production_missing_server_evidence_recollects_once_without_rebuilding() {
        let (todos, events, dir) = test_env("evidencerecollect");
        production_task(&todos, "完成的游戏", vec![]);
        let cancelled = never_cancel();
        let validations = AtomicUsize::new(0);
        let validate = || {
            if validations.fetch_add(1, Ordering::SeqCst) == 0 {
                Err("自动检查gameplay-1缺少当前证据".into())
            } else {
                Ok(())
            }
        };
        let ctx = TeamFlowCtx {
            todos: &todos,
            events: &events,
            session_id: "s",
            user_goal: "game",
            cancelled: &cancelled,
            max_fix_rounds: 5,
            source_filter: Some("ultraplan:regression:1"),
            serialize_engine: true,
            validate: Some(&validate),
            production_dir: None,
            verification_issues: None,
        };
        let h = FlowHarness::new(
            vec!["VERDICT: APPROVE"],
            vec!["QA_RESULT: PASS", "QA_RESULT: PASS"],
        );
        let leader = |request: String| async move {
            assert!(
                request.starts_with(DELEGATION_DRAFT_PREFIX),
                "缺验证证据不得派产品修复"
            );
            Ok((request, false))
        };
        let (status, _, error) = run_team_flow(&ctx, String::new(), leader, h.dispatch_fn()).await;
        assert_eq!(status, "completed", "{error:?}");
        let calls = h.dispatches.lock().unwrap();
        assert_eq!(
            calls
                .iter()
                .filter(|(role, _)| role == "logic-programmer")
                .count(),
            1
        );
        assert_eq!(
            calls.iter().filter(|(role, _)| role == "qa-tester").count(),
            2
        );
        assert_eq!(todos.list_by_session("s").len(), 1);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[tokio::test]
    async fn production_qa_fail_requires_confirmed_product_defect_before_repair() {
        for kind in [
            "missing",
            "stale",
            "blocked",
            "invalid_request",
            "unconfirmed",
        ] {
            let (todos, events, dir) = test_env(&format!("qa-classification-{kind}"));
            production_task(&todos, "已有游戏", vec![]);
            let cancelled = never_cancel();
            let issues = || {
                Ok(if kind == "unconfirmed" {
                    vec![]
                } else {
                    vec![json!({"id":"gameplay-1","status":kind,"reason":"真实验证诊断"})]
                })
            };
            let ctx = TeamFlowCtx {
                todos: &todos,
                events: &events,
                session_id: "s",
                user_goal: "game",
                cancelled: &cancelled,
                max_fix_rounds: 5,
                source_filter: Some("ultraplan:regression:1"),
                serialize_engine: true,
                validate: None,
                production_dir: None,
                verification_issues: Some(&issues),
            };
            let h = FlowHarness::new(
                vec![],
                vec![
                    "QA_RESULT: FAIL 没有通过验证",
                    "QA_RESULT: FAIL 没有通过验证",
                ],
            );
            let leader = |request: String| async move {
                assert!(
                    request.starts_with(DELEGATION_DRAFT_PREFIX),
                    "{kind} 不得启动制作修复"
                );
                Ok((request, false))
            };
            let (status, _, error) =
                run_team_flow(&ctx, String::new(), leader, h.dispatch_fn()).await;
            assert_eq!(status, "failed", "{kind}");
            assert!(error.is_some());
            let calls = h.dispatches.lock().unwrap();
            assert_eq!(
                calls
                    .iter()
                    .filter(|(role, _)| role == "logic-programmer")
                    .count(),
                1,
                "{kind}"
            );
            assert_eq!(
                calls.iter().filter(|(role, _)| role == "qa-tester").count(),
                if matches!(kind, "missing" | "stale") {
                    2
                } else {
                    1
                },
                "{kind}"
            );
            assert_eq!(todos.list_by_session("s").len(), 1);
            std::fs::remove_dir_all(dir).unwrap();
        }
    }

    #[tokio::test]
    async fn production_interrupted_attempt_reconciles_host_result_without_replay() {
        use crate::ultraplan::production::{self, ProductionTaskStatus};
        for has_result in [false, true] {
            let (todos, events, dir) = test_env(&format!("interrupted-{has_result}"));
            let upstream = production_task(&todos, "写游戏", vec![]);
            let downstream = production_task(&todos, "接续游戏", vec![upstream.id.clone()]);
            std::fs::write(
                dir.join("production.json"),
                serde_json::to_vec(&json!({
                    "flowId":"flow","planRev":1,"planHash":"approved",
                    "activeTodoIds":[upstream.id,downstream.id]
                }))
                .unwrap(),
            )
            .unwrap();
            execution::begin_attempt(&dir, &upstream.id, "write game").unwrap();
            todos
                .patch(
                    &upstream.id,
                    &PatchTodoRequest {
                        status: Some("verifying".into()),
                        ..Default::default()
                    },
                )
                .unwrap();
            if has_result {
                let mut result = production::ProductionTaskResult::failure(
                    "RX_CLI_NOT_FOUND",
                    "compiler missing",
                );
                result.status = ProductionTaskStatus::Blocked;
                production::save_task_result(&dir, &upstream.id, 1, &result).unwrap();
            }
            let cancelled = never_cancel();
            let ctx = TeamFlowCtx {
                todos: &todos,
                events: &events,
                session_id: "s",
                user_goal: "game",
                cancelled: &cancelled,
                max_fix_rounds: 5,
                source_filter: Some("ultraplan:regression:1"),
                serialize_engine: true,
                validate: None,
                production_dir: Some(&dir),
                verification_issues: None,
            };
            let calls = AtomicUsize::new(0);
            let dispatch = |_: DispatchReq| {
                calls.fetch_add(1, Ordering::SeqCst);
                async { DispatchOutcome::Completed("unexpected".into()) }
            };
            let report = run_ready_waves(&ctx, &dispatch).await;
            assert_eq!(calls.load(Ordering::SeqCst), 0);
            let tasks = todos.list_by_session("s");
            let first = tasks.iter().find(|task| task.id == upstream.id).unwrap();
            if has_result {
                assert_eq!(first.status, "blocked");
                assert_eq!(
                    tasks
                        .iter()
                        .find(|task| task.id == downstream.id)
                        .unwrap()
                        .status,
                    "blocked"
                );
                assert!(report.stalled.is_none());
            } else {
                assert_eq!(first.status, "verifying");
                assert!(report.stalled.unwrap().contains("RECOVERY_REQUIRED"));
            }
            std::fs::remove_dir_all(dir).unwrap();
        }
    }

    // ---------- 纯函数调度核心 ----------

    /// deps 拓扑推进:A→(B,C)→D 逐层就绪;title 引用与 id 引用等价。
    #[test]
    fn scheduler_topo_progression() {
        let (todos, _ev, dir) = test_env("topo");
        let sid = "s1";
        let a = seed(
            &todos,
            sid,
            "素材",
            Some("material-smith"),
            &[],
            Some("素材"),
            None,
        );
        let b = seed(
            &todos,
            sid,
            "搭场景",
            Some("scene-builder"),
            &["素材"],
            Some("场景"),
            None,
        );
        let c = seed(
            &todos,
            sid,
            "写逻辑",
            Some("logic-programmer"),
            &[&a.id],
            Some("场景"),
            None,
        );
        let d = seed(
            &todos,
            sid,
            "验收",
            Some("qa-tester"),
            &["搭场景", &c.id],
            Some("测试"),
            None,
        );
        let all = todos.list_by_session(sid);
        assert_eq!(ready_ids(&all), vec![a.id.clone()], "仅无依赖的 A 就绪");
        assert_eq!(next_wave(&all), vec![a.id.clone()]);
        // A 完成 → B、C 同阶段并行就绪;D 仍等待。
        todos
            .patch(
                &a.id,
                &PatchTodoRequest {
                    status: Some("completed".into()),
                    ..Default::default()
                },
            )
            .unwrap();
        let all = todos.list_by_session(sid);
        assert_eq!(ready_ids(&all), vec![b.id.clone(), c.id.clone()]);
        assert_eq!(
            next_wave(&all),
            vec![b.id.clone(), c.id.clone()],
            "同阶段就绪任务同波"
        );
        // B 完成、C 未完 → D 未就绪(deps 全 completed 才行)。
        todos
            .patch(
                &b.id,
                &PatchTodoRequest {
                    status: Some("completed".into()),
                    ..Default::default()
                },
            )
            .unwrap();
        let all = todos.list_by_session(sid);
        assert!(ready_ids(&all).is_empty() || ready_ids(&all) == vec![c.id.clone()]);
        todos
            .patch(
                &c.id,
                &PatchTodoRequest {
                    status: Some("completed".into()),
                    ..Default::default()
                },
            )
            .unwrap();
        let all = todos.list_by_session(sid);
        assert_eq!(next_wave(&all), vec![d.id.clone()], "B、C 全完成后 D 就绪");
        std::fs::remove_dir_all(&dir).ok();
    }

    /// 无 role 的待办不可调度;不同阶段的就绪任务分波。
    #[test]
    fn scheduler_skips_roleless_and_splits_stages() {
        let (todos, _ev, dir) = test_env("stage");
        let sid = "s1";
        let _leader_note = seed(&todos, sid, "自留项", None, &[], None, None);
        let a = seed(
            &todos,
            sid,
            "任务甲",
            Some("scene-builder"),
            &[],
            Some("一期"),
            None,
        );
        let _b = seed(
            &todos,
            sid,
            "任务乙",
            Some("scene-builder"),
            &[],
            Some("二期"),
            None,
        );
        let all = todos.list_by_session(sid);
        assert_eq!(ready_ids(&all).len(), 2, "无 role 项不进就绪集");
        assert_eq!(
            next_wave(&all),
            vec![a.id.clone()],
            "首波只含首个就绪任务的阶段"
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    /// 循环依赖:如实报停摆,不死循环;悬空依赖同样点名。
    #[tokio::test]
    async fn scheduler_cycle_and_dangling_dep_reported_honestly() {
        let (todos, events, dir) = test_env("cycle");
        let sid = "s1";
        let _a = seed(
            &todos,
            sid,
            "甲",
            Some("scene-builder"),
            &["乙"],
            None,
            None,
        );
        let _b = seed(
            &todos,
            sid,
            "乙",
            Some("scene-builder"),
            &["甲"],
            None,
            None,
        );
        let all = todos.list_by_session(sid);
        assert!(ready_ids(&all).is_empty());
        assert!(next_wave(&all).is_empty());
        let cancelled = never_cancel();
        let ctx = TeamFlowCtx {
            todos: &todos,
            events: &events,
            session_id: sid,
            user_goal: "目标",
            cancelled: &cancelled,
            max_fix_rounds: 3,
            source_filter: None,
            serialize_engine: false,
            validate: None,
            production_dir: None,
            verification_issues: None,
        };
        let dispatched = Arc::new(Mutex::new(0usize));
        let d2 = dispatched.clone();
        let dispatch = move |_req: DispatchReq| {
            let d = d2.clone();
            async move {
                *d.lock().unwrap() += 1;
                (true, "done".to_string())
            }
        };
        // run_ready_waves 必须返回(不死循环)且如实报停摆;循环双方零派发。
        let report = run_ready_waves(&ctx, &dispatch).await;
        let stall = report.stalled.expect("循环依赖应报停摆");
        assert!(stall.contains("循环依赖"), "诊断应点名循环依赖: {stall}");
        assert_eq!(*dispatched.lock().unwrap(), 0);
        // 悬空依赖:引用不存在的任务名。
        let sid2 = "s2";
        let _c = seed(
            &todos,
            sid2,
            "丙",
            Some("scene-builder"),
            &["不存在的任务"],
            None,
            None,
        );
        let all2 = todos.list_by_session(sid2);
        let stall2 = stall_reason(&all2);
        assert!(stall2.contains("不存在"), "诊断应点名悬空依赖: {stall2}");
        std::fs::remove_dir_all(&dir).ok();
    }

    /// 失败传播:依赖失败 → 下游如实标 failed(带原因),再下游继续传导;
    /// 无关分支不受影响照常派发。
    #[tokio::test]
    async fn scheduler_failure_propagates_downstream() {
        let (todos, events, dir) = test_env("failprop");
        let sid = "s1";
        let a = seed(
            &todos,
            sid,
            "会失败的活",
            Some("scene-builder"),
            &[],
            None,
            None,
        );
        let b = seed(
            &todos,
            sid,
            "下游一",
            Some("scene-builder"),
            &[&a.id],
            None,
            None,
        );
        let c = seed(
            &todos,
            sid,
            "下游二",
            Some("scene-builder"),
            &["下游一"],
            None,
            None,
        );
        let ok_task = seed(
            &todos,
            sid,
            "无关分支",
            Some("scene-builder"),
            &[],
            None,
            None,
        );
        let cancelled = never_cancel();
        let ctx = TeamFlowCtx {
            todos: &todos,
            events: &events,
            session_id: sid,
            user_goal: "目标",
            cancelled: &cancelled,
            max_fix_rounds: 3,
            source_filter: None,
            serialize_engine: false,
            validate: None,
            production_dir: None,
            verification_issues: None,
        };
        let a_id = a.id.clone();
        let dispatch = move |req: DispatchReq| {
            let fail = req.tool_call_id == format!("team-{a_id}");
            async move {
                if fail {
                    (false, "子代理执行炸了".to_string())
                } else {
                    (true, "done".to_string())
                }
            }
        };
        let report = run_ready_waves(&ctx, &dispatch).await;
        assert!(report.stalled.is_none());
        assert_eq!(
            report.completed,
            vec![ok_task.id.clone()],
            "无关分支照常完成"
        );
        let failed_ids: Vec<&str> = report.failed.iter().map(|(id, _)| id.as_str()).collect();
        assert!(failed_ids.contains(&a.id.as_str()), "A 执行失败入账");
        assert!(failed_ids.contains(&b.id.as_str()), "B 因依赖失败被传播");
        assert!(failed_ids.contains(&c.id.as_str()), "C 因 B 失败继续传播");
        let all = todos.list_by_session(sid);
        let status_of = |id: &str| all.iter().find(|t| t.id == id).unwrap().status.clone();
        assert_eq!(status_of(&a.id), "failed");
        assert_eq!(status_of(&b.id), "failed");
        assert_eq!(status_of(&c.id), "failed");
        assert_eq!(status_of(&ok_task.id), "completed");
        let b_summary = all
            .iter()
            .find(|t| t.id == b.id)
            .unwrap()
            .summary
            .clone()
            .unwrap();
        assert!(
            b_summary.contains("依赖"),
            "传播原因如实入 summary: {b_summary}"
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    /// 波内并行:同波 4 任务的派发时间窗互相重叠(join_all 并发),超出上限的分段。
    #[tokio::test]
    async fn wave_dispatch_runs_parallel_with_cap() {
        let (todos, events, dir) = test_env("wavepar");
        let sid = "s1";
        for i in 0..5 {
            seed(
                &todos,
                sid,
                &format!("并行任务{i}"),
                Some("scene-builder"),
                &[],
                None,
                None,
            );
        }
        let cancelled = never_cancel();
        let ctx = TeamFlowCtx {
            todos: &todos,
            events: &events,
            session_id: sid,
            user_goal: "目标",
            cancelled: &cancelled,
            max_fix_rounds: 3,
            source_filter: None,
            serialize_engine: false,
            validate: None,
            production_dir: None,
            verification_issues: None,
        };
        type Span = (std::time::Instant, std::time::Instant);
        let spans: Arc<Mutex<Vec<Span>>> = Default::default();
        let s2 = spans.clone();
        let dispatch = move |_req: DispatchReq| {
            let spans = s2.clone();
            async move {
                let t0 = std::time::Instant::now();
                tokio::time::sleep(std::time::Duration::from_millis(100)).await;
                spans.lock().unwrap().push((t0, std::time::Instant::now()));
                (true, "done".to_string())
            }
        };
        let t0 = std::time::Instant::now();
        let report = run_ready_waves(&ctx, &dispatch).await;
        let elapsed = t0.elapsed();
        assert_eq!(report.completed.len(), 5);
        // 5 任务 cap4 → 两段:约 200ms;串行则 500ms。
        assert!(
            elapsed < std::time::Duration::from_millis(450),
            "波内并行应显著快于串行 500ms,实测 {elapsed:?}"
        );
        let spans = spans.lock().unwrap();
        // 首段 4 条的时间窗应互相重叠(取前两条抽查)。
        assert!(
            spans[0].0 < spans[1].1 && spans[1].0 < spans[0].1,
            "同段任务应并发"
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    // ---------- 裁决解析 ----------

    #[test]
    fn verdict_and_qa_parsing_honest() {
        assert_eq!(
            parse_verdict("检查完毕\nVERDICT: APPROVE"),
            Verdict::Approve
        );
        match parse_verdict("有问题\nVERDICT: REJECT(缺相机)") {
            Verdict::Reject(r) => assert!(r.contains("缺相机")),
            v => panic!("应 REJECT: {v:?}"),
        }
        // 先 REJECT 后 APPROVE(修正语)→ 最后出现者胜。
        assert_eq!(
            parse_verdict("初判 VERDICT: REJECT……复核后撤回,VERDICT: APPROVE"),
            Verdict::Approve
        );
        // 缺标记 → 按 REJECT 处理且如实注明。
        match parse_verdict("大概没问题吧") {
            Verdict::Reject(r) => assert!(r.contains("未按格式")),
            v => panic!("缺标记应 REJECT: {v:?}"),
        }
        assert!(parse_qa_result(true, "全部通过 QA_RESULT: PASS").is_ok());
        assert!(parse_qa_result(true, "QA_RESULT: FAIL 挡板不动")
            .unwrap_err()
            .contains("挡板"));
        assert!(parse_qa_result(true, "我觉得行")
            .unwrap_err()
            .contains("未按格式"));
        assert!(parse_qa_result(false, "网络炸了")
            .unwrap_err()
            .contains("派发失败"));
    }

    // ---------- team 编排循环 ----------

    struct FlowHarness {
        /// (subagent_type, tool_call_id) 派发流水。
        dispatches: Arc<Mutex<Vec<(String, String)>>>,
        /// reviewer 脚本应答(按次弹出;耗尽 → APPROVE)。
        reviewer_script: Arc<Mutex<std::collections::VecDeque<String>>>,
        /// qa 脚本应答(按次弹出;耗尽 → PASS)。
        qa_script: Arc<Mutex<std::collections::VecDeque<String>>>,
        /// leader 修复轮调用流水(收到的回注提示)。
        leader_calls: Arc<Mutex<Vec<String>>>,
    }

    impl FlowHarness {
        fn new(reviewer: Vec<&str>, qa: Vec<&str>) -> Self {
            FlowHarness {
                dispatches: Default::default(),
                reviewer_script: Arc::new(Mutex::new(
                    reviewer.into_iter().map(str::to_string).collect(),
                )),
                qa_script: Arc::new(Mutex::new(qa.into_iter().map(str::to_string).collect())),
                leader_calls: Default::default(),
            }
        }
        fn dispatch_fn(
            &self,
        ) -> impl Fn(
            DispatchReq,
        ) -> std::pin::Pin<
            Box<dyn std::future::Future<Output = (bool, String)> + Send>,
        > {
            let dispatches = self.dispatches.clone();
            let reviewer = self.reviewer_script.clone();
            let qa = self.qa_script.clone();
            move |req: DispatchReq| {
                dispatches
                    .lock()
                    .unwrap()
                    .push((req.subagent_type.clone(), req.tool_call_id.clone()));
                let reply = match req.subagent_type.as_str() {
                    "reviewer" => reviewer
                        .lock()
                        .unwrap()
                        .pop_front()
                        .unwrap_or_else(|| "VERDICT: APPROVE".to_string()),
                    "qa-tester" => qa
                        .lock()
                        .unwrap()
                        .pop_front()
                        .unwrap_or_else(|| "QA_RESULT: PASS".to_string()),
                    _ => "done".to_string(),
                };
                Box::pin(async move { (true, reply) })
            }
        }
    }

    /// 核心用例:leader 计划两任务(一个 verify=reviewer)→ 调度执行(脚本子代理)→
    /// reviewer 先 REJECT 一轮(触发修复轮,leader 追加修复任务)→ 复审 APPROVE →
    /// 最终 completed;修复轮计数如实进收尾文案。
    #[tokio::test]
    async fn team_flow_reject_then_approve_triggers_one_fix_round() {
        let (todos, events, dir) = test_env("flow");
        let sid = "s1";
        seed(
            &todos,
            sid,
            "搭场景",
            Some("scene-builder"),
            &[],
            None,
            None,
        );
        seed(
            &todos,
            sid,
            "写逻辑",
            Some("logic-programmer"),
            &["搭场景"],
            None,
            Some("reviewer"),
        );
        let h = FlowHarness::new(
            vec![
                "检查发现挡板缺失\nVERDICT: REJECT(挡板缺失)",
                "复核通过\nVERDICT: APPROVE",
            ],
            vec![],
        );
        let cancelled = never_cancel();
        let ctx = TeamFlowCtx {
            todos: &todos,
            events: &events,
            session_id: sid,
            user_goal: "做个打砖块",
            cancelled: &cancelled,
            max_fix_rounds: 3,
            source_filter: None,
            serialize_engine: false,
            validate: None,
            production_dir: None,
            verification_issues: None,
        };
        let leader_calls = h.leader_calls.clone();
        let todos2 = todos.clone();
        let leader_round = move |fix: String| {
            let leader_calls = leader_calls.clone();
            let todos = todos2.clone();
            async move {
                if fix.starts_with(DELEGATION_DRAFT_PREFIX) {
                    return Ok(("主 agent 编写的本次终审委派词".into(), false));
                }
                leader_calls.lock().unwrap().push(fix);
                // leader 修复轮:追加一个修复任务(等价 plan_write 落库)。
                todos
                    .create(
                        sid,
                        NewTodo {
                            title: "补挡板".to_string(),
                            role: Some("scene-builder".to_string()),
                            prompt: Some("补上挡板".to_string()),
                            ..Default::default()
                        },
                    )
                    .unwrap();
                Ok(("修复任务已追加".to_string(), false))
            }
        };
        let (status, text, error) =
            run_team_flow(&ctx, "计划已排".to_string(), leader_round, h.dispatch_fn()).await;
        assert_eq!(status, "completed", "err={error:?}");
        assert!(text.contains("reviewer 终审 APPROVE"), "{text}");
        assert!(text.contains("修复轮 1 次"), "{text}");
        assert!(error.is_none());
        // 修复轮触发恰一次,回注提示含 REJECT 理由。
        let calls = h.leader_calls.lock().unwrap();
        assert_eq!(calls.len(), 1);
        assert!(calls[0].contains("挡板缺失"), "{}", calls[0]);
        // 派发流水:两任务 + reviewer(REJECT)+ 修复任务 + reviewer(APPROVE)。
        let d = h.dispatches.lock().unwrap();
        let reviewer_count = d.iter().filter(|(t, _)| t == "reviewer").count();
        assert_eq!(reviewer_count, 2);
        assert!(d.iter().any(|(_, id)| id == "team-review-1"));
        assert!(d.iter().any(|(_, id)| id == "team-review-2"));
        // 全部任务(含修复任务)completed。
        let all = todos.list_by_session(sid);
        assert_eq!(all.len(), 3);
        assert!(all.iter().all(|t| t.status == "completed"), "{all:?}");
        std::fs::remove_dir_all(&dir).ok();
    }

    /// 超限用例:reviewer 恒 REJECT、leader 每轮都追加新任务 → 修复轮打满 3 次后
    /// 如实 failed(错误点名上限),不无限循环、不伪装通过。
    #[tokio::test]
    async fn team_flow_fix_rounds_capped_and_honest_failed() {
        let (todos, events, dir) = test_env("flowcap");
        let sid = "s1";
        seed(
            &todos,
            sid,
            "搭场景",
            Some("scene-builder"),
            &[],
            None,
            None,
        );
        let h = FlowHarness::new(
            vec![
                "VERDICT: REJECT(问题1)",
                "VERDICT: REJECT(问题2)",
                "VERDICT: REJECT(问题3)",
                "VERDICT: REJECT(问题4)",
            ],
            vec![],
        );
        let cancelled = never_cancel();
        let ctx = TeamFlowCtx {
            todos: &todos,
            events: &events,
            session_id: sid,
            user_goal: "目标",
            cancelled: &cancelled,
            max_fix_rounds: 3,
            source_filter: None,
            serialize_engine: false,
            validate: None,
            production_dir: None,
            verification_issues: None,
        };
        let leader_calls = h.leader_calls.clone();
        let todos2 = todos.clone();
        let n = Arc::new(Mutex::new(0usize));
        let leader_round = move |fix: String| {
            let leader_calls = leader_calls.clone();
            let todos = todos2.clone();
            let n = n.clone();
            async move {
                if fix.starts_with(DELEGATION_DRAFT_PREFIX) {
                    return Ok(("主 agent 编写的本次终审委派词".into(), false));
                }
                leader_calls.lock().unwrap().push(fix);
                let mut n = n.lock().unwrap();
                *n += 1;
                todos
                    .create(
                        sid,
                        NewTodo {
                            title: format!("修复 {n}"),
                            role: Some("scene-builder".to_string()),
                            prompt: Some(format!("按第 {n} 次终审的问题修复场景并取证")),
                            ..Default::default()
                        },
                    )
                    .unwrap();
                Ok((format!("第 {n} 轮修复已排"), false))
            }
        };
        let (status, _text, error) =
            run_team_flow(&ctx, "计划已排".to_string(), leader_round, h.dispatch_fn()).await;
        assert_eq!(status, "failed");
        let err = error.expect("超限须有错误说明");
        assert!(err.contains("上限 3"), "{err}");
        assert!(err.contains("REJECT"), "错误应带问题原文: {err}");
        // leader 修复轮恰 3 次;reviewer 第 4 次 REJECT 后按超限收束。
        assert_eq!(h.leader_calls.lock().unwrap().len(), 3);
        let d = h.dispatches.lock().unwrap();
        assert_eq!(d.iter().filter(|(t, _)| t == "reviewer").count(), 4);
        std::fs::remove_dir_all(&dir).ok();
    }

    /// qa 闭环:verify=qa 任务完成后自动派 qa-tester;FAIL → 修复轮;修复任务完成、
    /// 复测不重复旧任务 → reviewer APPROVE 收束。
    #[tokio::test]
    async fn team_flow_qa_gate_fail_then_fixed() {
        let (todos, events, dir) = test_env("flowqa");
        let sid = "s1";
        let t = seed(
            &todos,
            sid,
            "写逻辑",
            Some("logic-programmer"),
            &[],
            None,
            Some("qa"),
        );
        let h = FlowHarness::new(
            vec!["VERDICT: APPROVE"],
            vec!["QA_RESULT: FAIL 挡板不响应输入"],
        );
        let cancelled = never_cancel();
        let ctx = TeamFlowCtx {
            todos: &todos,
            events: &events,
            session_id: sid,
            user_goal: "目标",
            cancelled: &cancelled,
            max_fix_rounds: 3,
            source_filter: None,
            serialize_engine: false,
            validate: None,
            production_dir: None,
            verification_issues: None,
        };
        let leader_calls = h.leader_calls.clone();
        let todos2 = todos.clone();
        let leader_round = move |fix: String| {
            let leader_calls = leader_calls.clone();
            let todos = todos2.clone();
            async move {
                if fix.starts_with(DELEGATION_DRAFT_PREFIX) {
                    return Ok(("主 agent 编写的本次验收委派词".into(), false));
                }
                leader_calls.lock().unwrap().push(fix);
                todos
                    .create(
                        sid,
                        NewTodo {
                            title: "修输入响应".to_string(),
                            role: Some("logic-programmer".to_string()),
                            prompt: Some("修复挡板输入响应并实际运行取证".into()),
                            ..Default::default()
                        },
                    )
                    .unwrap();
                Ok(("修复已排".to_string(), false))
            }
        };
        let (status, text, error) =
            run_team_flow(&ctx, "计划已排".to_string(), leader_round, h.dispatch_fn()).await;
        assert_eq!(status, "completed", "err={error:?}");
        assert!(text.contains("APPROVE"));
        let calls = h.leader_calls.lock().unwrap();
        assert_eq!(calls.len(), 1, "qa FAIL 应触发一轮修复");
        assert!(calls[0].contains("挡板不响应输入"), "{}", calls[0]);
        let d = h.dispatches.lock().unwrap();
        // qa 只测一次(qa_done 去重,修复轮后不重复复测旧任务)。
        assert_eq!(d.iter().filter(|(t, _)| t == "qa-tester").count(), 1);
        assert!(d.iter().any(|(_, id)| *id == format!("team-qa-{}", t.id)));
        std::fs::remove_dir_all(&dir).ok();
    }

    /// A missing structured plan is not a successfully built game.
    #[tokio::test]
    async fn ultraplan_repair_retests_previously_failed_qa_before_review() {
        let (todos, events, dir) = test_env("ultraqaretest");
        let sid = "s1";
        let source = "ultraplan:flow:1";
        let original = todos
            .create(
                sid,
                NewTodo {
                    title: "原批准玩法".into(),
                    role: Some("logic-programmer".into()),
                    verify: Some("qa".into()),
                    source: Some(source.into()),
                    prompt: Some("实现批准的挡板玩法并实际运行取证".into()),
                    ..Default::default()
                },
            )
            .unwrap();
        let h = FlowHarness::new(
            vec!["VERDICT: APPROVE"],
            vec!["QA_RESULT: FAIL 挡板不响应输入", "QA_RESULT: PASS"],
        );
        let cancelled = never_cancel();
        let ctx = TeamFlowCtx {
            todos: &todos,
            events: &events,
            session_id: sid,
            user_goal: "目标",
            cancelled: &cancelled,
            max_fix_rounds: 3,
            source_filter: Some(source),
            serialize_engine: true,
            validate: None,
            production_dir: None,
            verification_issues: None,
        };
        let todos2 = todos.clone();
        let original_id = original.id.clone();
        let leader_round = move |fix: String| {
            let todos = todos2.clone();
            let original_id = original_id.clone();
            async move {
                if fix.starts_with(DELEGATION_DRAFT_PREFIX) {
                    return Ok(("主 agent 编写的本次验收委派词".into(), false));
                }
                todos
                    .patch(
                        &original_id,
                        &PatchTodoRequest {
                            status: Some("queued".into()),
                            ..Default::default()
                        },
                    )
                    .unwrap();
                todos
                    .patch(
                        &original_id,
                        &PatchTodoRequest {
                            prompt: Some("只修复挡板输入响应并验证交接，保留原场景".into()),
                            ..Default::default()
                        },
                    )
                    .unwrap();
                Ok(("修复已排".into(), false))
            }
        };
        let (status, _, error) =
            run_team_flow(&ctx, "计划已排".into(), leader_round, h.dispatch_fn()).await;
        assert_eq!(status, "completed", "{error:?}");
        let d = h.dispatches.lock().unwrap();
        assert_eq!(d.iter().filter(|(role, _)| role == "qa-tester").count(), 2);
        assert_eq!(todos.list_by_session(sid).len(), 1, "定向修复不追加任务");
        assert_eq!(d.last().map(|(role, _)| role.as_str()), Some("reviewer"));
        std::fs::remove_dir_all(&dir).ok();
    }

    /// A missing structured plan is not a successfully built game.
    #[tokio::test]
    async fn team_flow_without_plan_fails_honestly() {
        let (todos, events, dir) = test_env("flownoop");
        let sid = "s1";
        // 只有无 role 的普通待办(leader 自留),不触发编排。
        seed(&todos, sid, "自留项", None, &[], None, None);
        let h = FlowHarness::new(vec![], vec![]);
        let cancelled = never_cancel();
        let ctx = TeamFlowCtx {
            todos: &todos,
            events: &events,
            session_id: sid,
            user_goal: "目标",
            cancelled: &cancelled,
            max_fix_rounds: 3,
            source_filter: None,
            serialize_engine: false,
            validate: None,
            production_dir: None,
            verification_issues: None,
        };
        let leader_round = |_fix: String| async move { Ok(("不应被调".to_string(), false)) };
        let (status, text, error) =
            run_team_flow(&ctx, "mock 直答".to_string(), leader_round, h.dispatch_fn()).await;
        assert_eq!(status, "failed");
        assert_eq!(text, "mock 直答");
        assert!(error.unwrap().contains("没有可执行任务"));
        assert!(h.dispatches.lock().unwrap().is_empty(), "零派发");
        std::fs::remove_dir_all(&dir).ok();
    }

    /// 修复轮未产出新任务 → 如实 failed(不空转)。
    #[tokio::test]
    async fn team_flow_fix_round_without_new_tasks_fails_honestly() {
        let (todos, events, dir) = test_env("flownofix");
        let sid = "s1";
        seed(
            &todos,
            sid,
            "搭场景",
            Some("scene-builder"),
            &[],
            None,
            None,
        );
        let h = FlowHarness::new(vec!["VERDICT: REJECT(缺相机)"], vec![]);
        let cancelled = never_cancel();
        let ctx = TeamFlowCtx {
            todos: &todos,
            events: &events,
            session_id: sid,
            user_goal: "目标",
            cancelled: &cancelled,
            max_fix_rounds: 3,
            source_filter: None,
            serialize_engine: false,
            validate: None,
            production_dir: None,
            verification_issues: None,
        };
        // leader 修复轮嘴上答应,不落任何新任务。
        let leader_round = |_fix: String| async move { Ok(("知道了".to_string(), false)) };
        let (status, _text, error) =
            run_team_flow(&ctx, "计划已排".to_string(), leader_round, h.dispatch_fn()).await;
        assert_eq!(status, "failed");
        let err = error.unwrap();
        assert!(err.contains("未产出新任务"), "{err}");
        assert!(err.contains("缺相机"), "问题原文如实透传: {err}");
        std::fs::remove_dir_all(&dir).ok();
    }

    /// 取消令牌:波间取消 → 编排整体收束 cancelled(不再派发后续)。
    #[tokio::test]
    async fn team_flow_cancel_token_effective() {
        let (todos, events, dir) = test_env("flowcancel");
        let sid = "s1";
        seed(
            &todos,
            sid,
            "任务一",
            Some("scene-builder"),
            &[],
            None,
            None,
        );
        seed(
            &todos,
            sid,
            "任务二",
            Some("scene-builder"),
            &["任务一"],
            None,
            None,
        );
        let flag = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let flag2 = flag.clone();
        let dispatched = Arc::new(Mutex::new(0usize));
        let d2 = dispatched.clone();
        let dispatch = move |_req: DispatchReq| {
            let flag = flag2.clone();
            let d = d2.clone();
            async move {
                *d.lock().unwrap() += 1;
                // 首个任务执行时置取消旗 → 下一波不再派发。
                flag.store(true, std::sync::atomic::Ordering::SeqCst);
                (true, "done".to_string())
            }
        };
        let flag3 = flag.clone();
        let cancelled = move || flag3.load(std::sync::atomic::Ordering::SeqCst);
        let ctx = TeamFlowCtx {
            todos: &todos,
            events: &events,
            session_id: sid,
            user_goal: "目标",
            cancelled: &cancelled,
            max_fix_rounds: 3,
            source_filter: None,
            serialize_engine: false,
            validate: None,
            production_dir: None,
            verification_issues: None,
        };
        let leader_round = |_fix: String| async move { Ok(("不应被调".to_string(), false)) };
        let (status, _text, _error) =
            run_team_flow(&ctx, "计划已排".to_string(), leader_round, dispatch).await;
        assert_eq!(status, "cancelled");
        assert_eq!(*dispatched.lock().unwrap(), 1, "取消后不得派发第二个任务");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn engine_roles_have_dedicated_waves_while_assets_stay_parallel() {
        let (todos, _, dir) = test_env("enginewaves");
        let asset1 = seed(
            &todos,
            "s",
            "asset1",
            Some("asset-wrangler"),
            &[],
            None,
            None,
        );
        let scene = seed(&todos, "s", "scene", Some("scene-builder"), &[], None, None);
        let asset2 = seed(
            &todos,
            "s",
            "asset2",
            Some("asset-wrangler"),
            &[],
            None,
            None,
        );
        let logic = seed(
            &todos,
            "s",
            "logic",
            Some("gameplay-scripter"),
            &[],
            None,
            None,
        );
        let all = vec![asset1.clone(), scene.clone(), asset2.clone(), logic.clone()];
        assert_eq!(
            execution_wave(&all, true),
            vec![asset1.id.clone(), asset2.id.clone()]
        );
        assert_eq!(
            execution_wave(&[scene.clone(), logic], true),
            vec![scene.id]
        );
        assert_eq!(execution_wave(&all, false).len(), 4);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[tokio::test]
    async fn resume_completed_flow_ignores_unrelated_tasks_but_rechecks_real_evidence() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        let (todos, events, dir) = test_env("ultraresume");
        let source = "ultraplan:current";
        let mine = todos
            .create(
                "s",
                NewTodo {
                    title: "finished".into(),
                    role: Some("scene-builder".into()),
                    source: Some(source.into()),
                    ..Default::default()
                },
            )
            .unwrap();
        todos
            .patch(
                &mine.id,
                &PatchTodoRequest {
                    status: Some("completed".into()),
                    ..Default::default()
                },
            )
            .unwrap();
        let unrelated = seed(
            &todos,
            "s",
            "unrelated",
            Some("logic-programmer"),
            &[],
            None,
            None,
        );
        let h = FlowHarness::new(vec!["VERDICT: APPROVE"], vec![]);
        let checks = AtomicUsize::new(0);
        let validate = || {
            checks.fetch_add(1, Ordering::SeqCst);
            Ok(())
        };
        let cancelled = never_cancel();
        let ctx = TeamFlowCtx {
            todos: &todos,
            events: &events,
            session_id: "s",
            user_goal: "goal",
            cancelled: &cancelled,
            max_fix_rounds: 5,
            source_filter: Some(source),
            serialize_engine: true,
            validate: Some(&validate),
            production_dir: None,
            verification_issues: None,
        };
        let (status, _, error) = run_team_flow(
            &ctx,
            "resume".into(),
            |_| async { Ok(("主 agent 编写的恢复后终审委派词".into(), false)) },
            h.dispatch_fn(),
        )
        .await;
        assert_eq!(status, "completed", "{error:?}");
        assert_eq!(
            checks.load(Ordering::SeqCst),
            2,
            "before and after review use server evidence"
        );
        assert_eq!(
            h.dispatches
                .lock()
                .unwrap()
                .iter()
                .map(|(role, _)| role.as_str())
                .collect::<Vec<_>>(),
            ["qa-tester", "reviewer"]
        );
        assert_eq!(
            todos
                .list_by_session("s")
                .iter()
                .find(|t| t.id == unrelated.id)
                .unwrap()
                .status,
            "queued"
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    #[tokio::test]
    async fn qa_pass_cannot_bypass_missing_server_evidence() {
        let (todos, events, dir) = test_env("ultraevidence");
        seed(
            &todos,
            "s",
            "game",
            Some("scene-builder"),
            &[],
            None,
            Some("qa"),
        );
        let h = FlowHarness::new(vec!["VERDICT: APPROVE"], vec!["QA_RESULT: PASS"]);
        let cancelled = never_cancel();
        let validate = || Err("缺少 gameplay-1 的真实检查".to_string());
        let ctx = TeamFlowCtx {
            todos: &todos,
            events: &events,
            session_id: "s",
            user_goal: "goal",
            cancelled: &cancelled,
            max_fix_rounds: 0,
            source_filter: None,
            serialize_engine: true,
            validate: Some(&validate),
            production_dir: None,
            verification_issues: None,
        };
        let (status, _, error) = run_team_flow(
            &ctx,
            "build".into(),
            |_| async { Ok(("主 agent 编写的证据复测委派词".into(), false)) },
            h.dispatch_fn(),
        )
        .await;
        assert_eq!(status, "failed");
        assert!(error.unwrap().contains("gameplay-1"));
        assert!(!h
            .dispatches
            .lock()
            .unwrap()
            .iter()
            .any(|(role, _)| role == "reviewer"));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[tokio::test]
    async fn team_flow_leader_authors_qa_and_reviewer_before_each_dispatch() {
        let (todos, events, dir) = test_env("authoreddelegations");
        seed(
            &todos,
            "s",
            "实现挡板输入",
            Some("logic-programmer"),
            &[],
            None,
            Some("qa"),
        );
        let cancelled = never_cancel();
        let ctx = TeamFlowCtx {
            todos: &todos,
            events: &events,
            session_id: "s",
            user_goal: "制作键盘控制的打砖块游戏",
            cancelled: &cancelled,
            max_fix_rounds: 0,
            source_filter: None,
            serialize_engine: false,
            validate: None,
            production_dir: None,
            verification_issues: None,
        };
        const QA_PROMPT: &str =
            "  主 agent：使用键盘左右移动挡板，检查边界并记录证据。\nQA_RESULT: PASS 或 FAIL\n";
        const REVIEW_PROMPT: &str =
            " 主 agent：按打砖块需求复核挡板输入与成品。\nVERDICT: APPROVE 或 REJECT  ";
        let sequence = Arc::new(Mutex::new(Vec::new()));
        let leader_sequence = sequence.clone();
        let leader_round = move |request: String| {
            assert!(request.starts_with(DELEGATION_DRAFT_PREFIX));
            assert!(request.contains("制作键盘控制的打砖块游戏"));
            assert!(request.contains("做「实现挡板输入」"));
            assert!(request.contains("已实现键盘输入，证据: input-check"));
            let (event, prompt) = if request.contains("为 qa-tester") {
                assert!(request.contains("QA_RESULT"));
                ("leader:qa", QA_PROMPT)
            } else {
                assert!(request.contains("为 reviewer"));
                assert!(request.contains("VERDICT"));
                ("leader:reviewer", REVIEW_PROMPT)
            };
            leader_sequence.lock().unwrap().push(event);
            async move { Ok((prompt.to_string(), false)) }
        };
        let dispatch_sequence = sequence.clone();
        let dispatch = move |req: DispatchReq| {
            let (event, report) = match req.subagent_type.as_str() {
                "logic-programmer" => {
                    assert_eq!(req.prompt, "做「实现挡板输入」");
                    ("worker", "已实现键盘输入，证据: input-check")
                }
                "qa-tester" => {
                    assert_eq!(req.prompt, QA_PROMPT, "主 agent 输出必须原样派发");
                    ("qa", "QA_RESULT: PASS")
                }
                "reviewer" => {
                    assert_eq!(req.prompt, REVIEW_PROMPT, "主 agent 输出必须原样派发");
                    ("reviewer", "VERDICT: APPROVE")
                }
                other => panic!("unexpected role: {other}"),
            };
            dispatch_sequence.lock().unwrap().push(event);
            async move { (true, report.to_string()) }
        };
        let (status, _, error) =
            run_team_flow(&ctx, "计划已排".into(), leader_round, dispatch).await;
        assert_eq!(status, "completed", "{error:?}");
        assert_eq!(
            *sequence.lock().unwrap(),
            ["worker", "leader:qa", "qa", "leader:reviewer", "reviewer"]
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    #[tokio::test]
    async fn missing_failed_or_cancelled_authoring_never_dispatches_a_template() {
        use std::sync::atomic::{AtomicBool, Ordering};

        for verify in [Some("qa"), None] {
            for outcome in ["empty", "error", "cancelled", "token"] {
                let (todos, events, dir) = test_env("draftfailure");
                let task = seed(
                    &todos,
                    "s",
                    "已完成的批准任务",
                    Some("scene-builder"),
                    &[],
                    None,
                    verify,
                );
                todos
                    .patch(
                        &task.id,
                        &PatchTodoRequest {
                            status: Some("completed".into()),
                            summary: Some("实际执行结果".into()),
                            ..Default::default()
                        },
                    )
                    .unwrap();
                let flag = AtomicBool::new(false);
                let cancelled = || flag.load(Ordering::SeqCst);
                let ctx = TeamFlowCtx {
                    todos: &todos,
                    events: &events,
                    session_id: "s",
                    user_goal: "真实用户需求",
                    cancelled: &cancelled,
                    max_fix_rounds: 3,
                    source_filter: None,
                    serialize_engine: false,
                    validate: None,
                    production_dir: None,
                    verification_issues: None,
                };
                let leader_round = |request: String| {
                    assert!(request.starts_with(DELEGATION_DRAFT_PREFIX));
                    if outcome == "token" {
                        flag.store(true, Ordering::SeqCst);
                    }
                    async move {
                        match outcome {
                            "empty" => Ok((" \n\t".to_string(), false)),
                            "error" => Err("主 agent 模型不可用".to_string()),
                            "cancelled" => Ok(("不应派发的部分输出".to_string(), true)),
                            "token" => Ok(("取消时返回的完整输出".to_string(), false)),
                            _ => unreachable!(),
                        }
                    }
                };
                let h = FlowHarness::new(vec![], vec![]);
                let (status, _, error) =
                    run_team_flow(&ctx, "原计划".into(), leader_round, h.dispatch_fn()).await;
                if matches!(outcome, "cancelled" | "token") {
                    assert_eq!(status, "cancelled");
                    assert!(error.is_none());
                } else {
                    assert_eq!(status, "failed");
                    assert!(error.unwrap().contains("未启动子 agent"));
                }
                assert!(
                    h.dispatches.lock().unwrap().is_empty(),
                    "verify={verify:?}, outcome={outcome}: 不得回退到固定模板派发"
                );
                std::fs::remove_dir_all(&dir).ok();
            }
        }
    }

    #[tokio::test]
    async fn queued_task_without_leader_prompt_fails_without_title_fallback() {
        let (todos, events, dir) = test_env("noprompt");
        for prompt in [
            None,
            Some(" \n\t".to_string()),
            Some(" \t标题不是委派提示词\n".to_string()),
        ] {
            todos
                .create(
                    "s",
                    NewTodo {
                        title: "标题不是委派提示词".into(),
                        role: Some("scene-builder".into()),
                        prompt,
                        ..Default::default()
                    },
                )
                .unwrap();
        }
        let cancelled = never_cancel();
        let ctx = TeamFlowCtx {
            todos: &todos,
            events: &events,
            session_id: "s",
            user_goal: "用户需求",
            cancelled: &cancelled,
            max_fix_rounds: 0,
            source_filter: None,
            serialize_engine: false,
            validate: None,
            production_dir: None,
            verification_issues: None,
        };
        let h = FlowHarness::new(vec![], vec![]);
        let report = run_ready_waves(&ctx, &h.dispatch_fn()).await;
        assert_eq!(report.failed.len(), 3);
        assert!(report.completed.is_empty());
        assert!(h.dispatches.lock().unwrap().is_empty());
        assert!(report
            .failed
            .iter()
            .all(|(_, error)| error.contains("缺少主 agent 编写的委派提示词")));
        assert!(todos
            .list_by_session("s")
            .iter()
            .all(|task| task.status == "failed"));
        std::fs::remove_dir_all(&dir).ok();
    }
}
