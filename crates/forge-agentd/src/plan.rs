//! F-GAME-4 wave.3:Plan DAG 调度器 + team 模式代码级编排。
//!
//! 分层:
//! - 纯函数调度核心(ready_ids/next_wave/failed_dependents/stall_reason):输入 todo 集,
//!   输出就绪集/失败传播集/停摆诊断——不碰 IO,单测确定性。
//! - `run_ready_waves` 波次执行器:同阶段就绪任务并行派发(并发上限 4,段内 join_all),
//!   完成置 completed/failed + todo.updated 事件;依赖失败向下游如实传播;
//!   循环依赖/悬空依赖如实报停摆,不死循环。
//! - `run_team_flow` 编排循环:leader 轮产出的结构化计划(带 role 的 queued 任务)→
//!   波次执行 → verify=qa 任务完成后自动派 qa-tester 复测 → 问题回注 leader 修复轮
//!   (上限 max_fix_rounds,超限如实 failed)→ 全部完成后派 reviewer 终审
//!   (VERDICT: APPROVE 收束 / REJECT 回修复轮)。
//!
//! 注入面:leader 修复轮与子代理派发都是泛型闭包(生产 = run_tool_loop /
//! run_nested_task;单测 = 脚本闭包全内存,不触网不触 MCP,mock 恒绿纪律)。
//! 事件面只复用既有信封(todo.updated / subagent.* 由派发方发;不发明新 kind)。

use serde_json::json;

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
}

// ---------- 纯函数调度核心 ----------

/// 可被编排器派发 = 带非空 role。无 role 的待办是 leader/用户自留项,调度器不碰。
pub(crate) fn is_schedulable(t: &TodoItem) -> bool {
    t.role.as_deref().map(|r| !r.trim().is_empty()).unwrap_or(false)
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
    for t in all.iter().filter(|t| is_schedulable(t) && t.status == "queued") {
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
    /// Some = 调度停摆(循环依赖/悬空依赖),如实诊断文本。
    pub stalled: Option<String>,
    pub cancelled: bool,
}

fn clip(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    let head: String = s.chars().take(max).collect();
    format!("{head}…")
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
    ctx.todos.list_by_session(ctx.session_id)
}

/// 波次推进:按 deps 分层,同阶段就绪任务并行派发(≤4 并发),直到无波可派。
/// 返回本次调用新造成的状态迁移(completed/failed)与停摆/取消旗。
pub(crate) async fn run_ready_waves<D, DFut>(ctx: &TeamFlowCtx<'_>, dispatch: &D) -> WaveReport
where
    D: Fn(DispatchReq) -> DFut,
    DFut: std::future::Future<Output = (bool, String)>,
{
    let mut report = WaveReport::default();
    loop {
        if (ctx.cancelled)() {
            report.cancelled = true;
            return report;
        }
        let all = schedulable_tasks(ctx);
        // 失败传播优先:先把被失败依赖阻塞的任务如实标掉,再算就绪波。
        let prop = failed_dependents(&all);
        if !prop.is_empty() {
            for (id, why) in prop {
                mark(ctx, &id, "failed", Some(why.clone()));
                report.failed.push((id, why));
            }
            continue;
        }
        let wave = next_wave(&all);
        if wave.is_empty() {
            if all.iter().any(|t| is_schedulable(t) && t.status == "queued") {
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
                mark(ctx, id, "running", None);
                let req = DispatchReq {
                    subagent_type: t.role.clone().unwrap_or_default(),
                    prompt: t
                        .prompt
                        .clone()
                        .filter(|p| !p.trim().is_empty())
                        .unwrap_or_else(|| t.title.clone()),
                    description: t.title.clone(),
                    tool_call_id: format!("team-{}", t.id),
                };
                let id2 = id.clone();
                futs.push(async move { (id2, dispatch(req).await) });
            }
            for (id, (ok, text)) in futures_util::future::join_all(futs).await {
                let status = if ok { "completed" } else { "failed" };
                mark(ctx, &id, status, Some(clip(&text, 500)));
                if ok {
                    report.completed.push(id);
                } else {
                    report.failed.push((id, clip(&text, 500)));
                }
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
        (_, Some(ri)) => Verdict::Reject(clip(&text[ri..], 300)),
        (None, None) => Verdict::Reject(format!(
            "终审未按格式输出 VERDICT(按 REJECT 处理);原文截断:{}",
            clip(text, 200)
        )),
    }
}

/// qa 复测结论:Ok(()) = PASS;Err(原因) = 有问题(派发失败/显式 FAIL/缺标记均如实算问题)。
pub(crate) fn parse_qa_result(ok: bool, text: &str) -> Result<(), String> {
    if !ok {
        return Err(format!("qa 复测派发失败:{}", clip(text, 200)));
    }
    let p = text.rfind("QA_RESULT: PASS");
    let f = text.rfind("QA_RESULT: FAIL");
    match (p, f) {
        (Some(pi), Some(fi)) if pi > fi => Ok(()),
        (Some(_), None) => Ok(()),
        (_, Some(fi)) => Err(clip(&text[fi..], 300)),
        (None, None) => Err(format!(
            "复测报告未按格式输出 QA_RESULT(按未通过处理);原文截断:{}",
            clip(text, 200)
        )),
    }
}

// ---------- 提示词拼装 ----------

fn qa_retest_prompt(t: &TodoItem) -> String {
    format!(
        "{}\n\n——以上是原任务委派词,该任务已由工种 {} 执行完毕,其汇报如下:\n{}\n\n\
         请复测该任务是否真正达成验收标准:运行取证(play/viewport/scene 查询类工具),\
         逐条给出 PASS/FAIL 与证据;只测不改。最终消息必须以 `QA_RESULT: PASS` 或 \
         `QA_RESULT: FAIL`(附问题清单)结尾。",
        t.prompt.as_deref().unwrap_or(&t.title),
        t.role.as_deref().unwrap_or("?"),
        t.summary.as_deref().unwrap_or("(无汇报)"),
    )
}

fn reviewer_prompt(goal: &str, tasks: &[TodoItem]) -> String {
    let mut lines = String::new();
    for t in tasks.iter().filter(|t| is_schedulable(t)) {
        lines.push_str(&format!(
            "- [{}] {}:{}\n",
            t.status,
            t.title,
            clip(t.summary.as_deref().unwrap_or("(无汇报)"), 160)
        ));
    }
    format!(
        "用户目标:{goal}\n\n计划任务执行情况:\n{lines}\n\
         请以对抗式终审姿态复核成品:按 Functionality(功能可用)→ Visual(画面正确)→ \
         Playability(可玩性)顺序递进检查,发现足以否决的问题立即停止并输出。\
         最终消息必须以 `VERDICT: APPROVE` 或 `VERDICT: REJECT`(附理由)结尾。"
    )
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
/// - leader 轮后没有任何带 role 的 queued 任务 → 维持现状路径直接收束(mock 恒绿);
/// - 波次执行 → verify=qa 完成后自动复测 → 问题回注 leader 修复轮(共享上限);
/// - 全部完成 → reviewer 终审:APPROVE 收束 completed;REJECT 回修复轮;
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
    DFut: std::future::Future<Output = (bool, String)>,
{
    let has_queued = |tasks: &[TodoItem]| {
        tasks
            .iter()
            .any(|t| is_schedulable(t) && t.status == "queued")
    };
    if !has_queued(&schedulable_tasks(ctx)) {
        // 情形 5:leader 没落结构化计划(如 mock provider 不产工具调用)→ 现状路径。
        return ("completed".to_string(), leader_text, None);
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
            if (ctx.cancelled)() {
                return ("cancelled".to_string(), String::new(), None);
            }
            let (ok, text) = dispatch(DispatchReq {
                subagent_type: "qa-tester".to_string(),
                prompt: qa_retest_prompt(t),
                description: format!("qa 复测:{}", t.title),
                tool_call_id: format!("team-qa-{}", t.id),
            })
            .await;
            qa_done.insert(t.id.clone());
            if let Err(why) = parse_qa_result(ok, &text) {
                problems.push(format!("任务「{}」qa 复测未通过:{why}", t.title));
            }
        }
        // 无问题 → reviewer 终审(计划非空即终审,verify=reviewer 任务也归口于此)。
        if problems.is_empty() {
            if (ctx.cancelled)() {
                return ("cancelled".to_string(), String::new(), None);
            }
            review_attempts += 1;
            let (ok, text) = dispatch(DispatchReq {
                subagent_type: "reviewer".to_string(),
                prompt: reviewer_prompt(ctx.user_goal, &tasks),
                description: format!("reviewer 终审(第 {review_attempts} 次)"),
                tool_call_id: format!("team-review-{review_attempts}"),
            })
            .await;
            if !ok {
                return (
                    "failed".to_string(),
                    last_text,
                    Some(format!("reviewer 终审派发失败:{}", clip(&text, 300))),
                );
            }
            match parse_verdict(&text) {
                Verdict::Approve => {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::NewTodo;
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

    fn seed(todos: &TodoStore, sid: &str, title: &str, role: Option<&str>, deps: &[&str], stage: Option<&str>, verify: Option<&str>) -> TodoItem {
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

    // ---------- 纯函数调度核心 ----------

    /// deps 拓扑推进:A→(B,C)→D 逐层就绪;title 引用与 id 引用等价。
    #[test]
    fn scheduler_topo_progression() {
        let (todos, _ev, dir) = test_env("topo");
        let sid = "s1";
        let a = seed(&todos, sid, "素材", Some("material-smith"), &[], Some("素材"), None);
        let b = seed(&todos, sid, "搭场景", Some("scene-builder"), &["素材"], Some("场景"), None);
        let c = seed(&todos, sid, "写逻辑", Some("logic-programmer"), &[&a.id], Some("场景"), None);
        let d = seed(&todos, sid, "验收", Some("qa-tester"), &["搭场景", &c.id], Some("测试"), None);
        let all = todos.list_by_session(sid);
        assert_eq!(ready_ids(&all), vec![a.id.clone()], "仅无依赖的 A 就绪");
        assert_eq!(next_wave(&all), vec![a.id.clone()]);
        // A 完成 → B、C 同阶段并行就绪;D 仍等待。
        todos.patch(&a.id, &PatchTodoRequest { status: Some("completed".into()), ..Default::default() }).unwrap();
        let all = todos.list_by_session(sid);
        assert_eq!(ready_ids(&all), vec![b.id.clone(), c.id.clone()]);
        assert_eq!(next_wave(&all), vec![b.id.clone(), c.id.clone()], "同阶段就绪任务同波");
        // B 完成、C 未完 → D 未就绪(deps 全 completed 才行)。
        todos.patch(&b.id, &PatchTodoRequest { status: Some("completed".into()), ..Default::default() }).unwrap();
        let all = todos.list_by_session(sid);
        assert!(ready_ids(&all).is_empty() || ready_ids(&all) == vec![c.id.clone()]);
        todos.patch(&c.id, &PatchTodoRequest { status: Some("completed".into()), ..Default::default() }).unwrap();
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
        let a = seed(&todos, sid, "任务甲", Some("scene-builder"), &[], Some("一期"), None);
        let _b = seed(&todos, sid, "任务乙", Some("scene-builder"), &[], Some("二期"), None);
        let all = todos.list_by_session(sid);
        assert_eq!(ready_ids(&all).len(), 2, "无 role 项不进就绪集");
        assert_eq!(next_wave(&all), vec![a.id.clone()], "首波只含首个就绪任务的阶段");
        std::fs::remove_dir_all(&dir).ok();
    }

    /// 循环依赖:如实报停摆,不死循环;悬空依赖同样点名。
    #[tokio::test]
    async fn scheduler_cycle_and_dangling_dep_reported_honestly() {
        let (todos, events, dir) = test_env("cycle");
        let sid = "s1";
        let _a = seed(&todos, sid, "甲", Some("scene-builder"), &["乙"], None, None);
        let _b = seed(&todos, sid, "乙", Some("scene-builder"), &["甲"], None, None);
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
        let _c = seed(&todos, sid2, "丙", Some("scene-builder"), &["不存在的任务"], None, None);
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
        let a = seed(&todos, sid, "会失败的活", Some("scene-builder"), &[], None, None);
        let b = seed(&todos, sid, "下游一", Some("scene-builder"), &[&a.id], None, None);
        let c = seed(&todos, sid, "下游二", Some("scene-builder"), &["下游一"], None, None);
        let ok_task = seed(&todos, sid, "无关分支", Some("scene-builder"), &[], None, None);
        let cancelled = never_cancel();
        let ctx = TeamFlowCtx {
            todos: &todos,
            events: &events,
            session_id: sid,
            user_goal: "目标",
            cancelled: &cancelled,
            max_fix_rounds: 3,
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
        assert_eq!(report.completed, vec![ok_task.id.clone()], "无关分支照常完成");
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
        let b_summary = all.iter().find(|t| t.id == b.id).unwrap().summary.clone().unwrap();
        assert!(b_summary.contains("依赖"), "传播原因如实入 summary: {b_summary}");
        std::fs::remove_dir_all(&dir).ok();
    }

    /// 波内并行:同波 4 任务的派发时间窗互相重叠(join_all 并发),超出上限的分段。
    #[tokio::test]
    async fn wave_dispatch_runs_parallel_with_cap() {
        let (todos, events, dir) = test_env("wavepar");
        let sid = "s1";
        for i in 0..5 {
            seed(&todos, sid, &format!("并行任务{i}"), Some("scene-builder"), &[], None, None);
        }
        let cancelled = never_cancel();
        let ctx = TeamFlowCtx {
            todos: &todos,
            events: &events,
            session_id: sid,
            user_goal: "目标",
            cancelled: &cancelled,
            max_fix_rounds: 3,
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
        assert!(spans[0].0 < spans[1].1 && spans[1].0 < spans[0].1, "同段任务应并发");
        std::fs::remove_dir_all(&dir).ok();
    }

    // ---------- 裁决解析 ----------

    #[test]
    fn verdict_and_qa_parsing_honest() {
        assert_eq!(parse_verdict("检查完毕\nVERDICT: APPROVE"), Verdict::Approve);
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
        assert!(parse_qa_result(true, "QA_RESULT: FAIL 挡板不动").unwrap_err().contains("挡板"));
        assert!(parse_qa_result(true, "我觉得行").unwrap_err().contains("未按格式"));
        assert!(parse_qa_result(false, "网络炸了").unwrap_err().contains("派发失败"));
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
        ) -> impl Fn(DispatchReq) -> std::pin::Pin<Box<dyn std::future::Future<Output = (bool, String)> + Send>>
        {
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
        seed(&todos, sid, "搭场景", Some("scene-builder"), &[], None, None);
        seed(&todos, sid, "写逻辑", Some("logic-programmer"), &["搭场景"], None, Some("reviewer"));
        let h = FlowHarness::new(
            vec!["检查发现挡板缺失\nVERDICT: REJECT(挡板缺失)", "复核通过\nVERDICT: APPROVE"],
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
        };
        let leader_calls = h.leader_calls.clone();
        let todos2 = todos.clone();
        let leader_round = move |fix: String| {
            leader_calls.lock().unwrap().push(fix);
            let todos = todos2.clone();
            async move {
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
        seed(&todos, sid, "搭场景", Some("scene-builder"), &[], None, None);
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
        };
        let leader_calls = h.leader_calls.clone();
        let todos2 = todos.clone();
        let n = Arc::new(Mutex::new(0usize));
        let leader_round = move |fix: String| {
            leader_calls.lock().unwrap().push(fix);
            let todos = todos2.clone();
            let n = n.clone();
            async move {
                let mut n = n.lock().unwrap();
                *n += 1;
                todos
                    .create(
                        sid,
                        NewTodo {
                            title: format!("修复 {n}"),
                            role: Some("scene-builder".to_string()),
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
        let t = seed(&todos, sid, "写逻辑", Some("logic-programmer"), &[], None, Some("qa"));
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
        };
        let leader_calls = h.leader_calls.clone();
        let todos2 = todos.clone();
        let leader_round = move |fix: String| {
            leader_calls.lock().unwrap().push(fix);
            let todos = todos2.clone();
            async move {
                todos
                    .create(
                        sid,
                        NewTodo {
                            title: "修输入响应".to_string(),
                            role: Some("logic-programmer".to_string()),
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

    /// 情形 5(恒绿保证):leader 轮后没有任何带 role 的 queued 任务 → 直接以 leader
    /// 文本收束,零派发零修复轮。
    #[tokio::test]
    async fn team_flow_without_plan_keeps_status_quo() {
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
        };
        let leader_round = |_fix: String| async move { Ok(("不应被调".to_string(), false)) };
        let (status, text, error) =
            run_team_flow(&ctx, "mock 直答".to_string(), leader_round, h.dispatch_fn()).await;
        assert_eq!(status, "completed");
        assert_eq!(text, "mock 直答", "无计划时维持现状路径原文收束");
        assert!(error.is_none());
        assert!(h.dispatches.lock().unwrap().is_empty(), "零派发");
        std::fs::remove_dir_all(&dir).ok();
    }

    /// 修复轮未产出新任务 → 如实 failed(不空转)。
    #[tokio::test]
    async fn team_flow_fix_round_without_new_tasks_fails_honestly() {
        let (todos, events, dir) = test_env("flownofix");
        let sid = "s1";
        seed(&todos, sid, "搭场景", Some("scene-builder"), &[], None, None);
        let h = FlowHarness::new(vec!["VERDICT: REJECT(缺相机)"], vec![]);
        let cancelled = never_cancel();
        let ctx = TeamFlowCtx {
            todos: &todos,
            events: &events,
            session_id: sid,
            user_goal: "目标",
            cancelled: &cancelled,
            max_fix_rounds: 3,
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
        seed(&todos, sid, "任务一", Some("scene-builder"), &[], None, None);
        seed(&todos, sid, "任务二", Some("scene-builder"), &["任务一"], None, None);
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
        };
        let leader_round = |_fix: String| async move { Ok(("不应被调".to_string(), false)) };
        let (status, _text, _error) =
            run_team_flow(&ctx, "计划已排".to_string(), leader_round, dispatch).await;
        assert_eq!(status, "cancelled");
        assert_eq!(*dispatched.lock().unwrap(), 1, "取消后不得派发第二个任务");
        std::fs::remove_dir_all(&dir).ok();
    }
}
