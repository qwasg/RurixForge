//! The durable half of UltraPlan. Only the coordinator advances gates; models
//! supply validated artifacts, never a self-reported workflow status.
use super::*;
use crate::agent::{NewTodo, PatchTodoRequest};
use forge_util::hashutil::sha256_hex;
use std::collections::HashMap;

#[cfg(test)]
#[path = "flow_tests.rs"]
mod flow_tests;

#[cfg(test)]
#[path = "native_smoke.rs"]
mod native_smoke;

pub const VERIFY_TOOL: &str = "ultraplan_verify";
const TARGET: &str = "target.json";
const MANIFEST: &str = "requirements.json";
const FEEDBACK: &str = "feedback.md";
const DELIVERY: &str = "delivery.json";
const DELEGATIONS: &str = "production-delegations.json";

fn failure(message: impl std::fmt::Display) -> String {
    format!("{ERR_TURN_INVALID}: {message}")
}
fn save(dir: &Path, file: &str, value: &Value) -> Result<(), String> {
    write_json_atomic(&dir.join(file), value).map_err(failure)
}
fn text_field<'a>(v: &'a Value, key: &str) -> Result<&'a str, String> {
    v.get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| format!("{key} 不可为空"))
}
fn get_flow(state: &AppState, sid: &str, rt: &UltraRuntime) -> Result<UltraPlanState, String> {
    state
        .sessions
        .get(sid)
        .and_then(|s| s.ultraplan)
        .filter(|u| u.id == rt.flow_id && u.phase == PHASE_RUNNING)
        .ok_or_else(|| failure("流程已失效或停止"))
}
fn advance(
    state: &AppState,
    sid: &str,
    rid: &str,
    rt: &UltraRuntime,
    edit: impl FnOnce(&mut UltraPlanState),
) -> Result<UltraPlanState, String> {
    let result = state
        .sessions
        .update_ultraplan(sid, |slot| {
            let up = slot
                .as_mut()
                .filter(|u| u.id == rt.flow_id && u.phase == PHASE_RUNNING)?;
            edit(up);
            Some(up.clone())
        })
        .and_then(|(_, up)| up)
        .ok_or_else(|| failure("流程已失效"))?;
    emit(state, sid, "ultraplan.stage", rt.stage_event(&result, rid));
    emit_session_updated(state, sid);
    Ok(result)
}

/// Answers are validated against the exact questionnaire revision, not a client
/// supplied schema. Delegation is explicit and does not silently pick defaults.
pub fn validate_answers(questionnaire: &Value, answers: &Value) -> Result<Value, String> {
    let q = validate_questionnaire(questionnaire)?;
    let map = answers.as_object().ok_or("answers 必须为对象")?;
    let questions: HashMap<_, _> = q
        .sections
        .iter()
        .flat_map(|s| &s.questions)
        .map(|q| (q.id.as_str(), q))
        .collect();
    for id in map.keys() {
        if !questions.contains_key(id.as_str()) {
            return Err(format!("未知问题 {id}"));
        }
    }
    for (id, q) in questions {
        let Some(answer) = map.get(id) else {
            if q.required {
                return Err(format!("问题 {id} 尚未回答"));
            }
            continue;
        };
        let a = answer
            .as_object()
            .ok_or_else(|| format!("{id} 的答案须为对象"))?;
        if a.keys()
            .any(|k| !["delegate", "choice", "text", "scale", "other"].contains(&k.as_str()))
        {
            return Err(format!("{id} 存在未知答案字段"));
        }
        if a.get("delegate") == Some(&Value::Bool(true)) {
            if !q.allow_delegate || a.len() != 1 {
                return Err(format!("{id} 不允许委托或委托与答案混用"));
            }
            continue;
        }
        if a.contains_key("delegate") {
            return Err(format!("{id} delegate 必须是 true 且不能混用"));
        }
        let other = a
            .get("other")
            .map(|v| v.as_str().map(str::trim).filter(|s| !s.is_empty()))
            .flatten();
        if a.contains_key("other") && (other.is_none() || !q.allow_other) {
            return Err(format!("{id} 不允许此其他答案"));
        }
        let keys_valid = match q.kind.as_str() {
            KIND_SINGLE | KIND_MULTI => a.keys().all(|k| k == "choice" || k == "other"),
            KIND_TEXT => a.len() == 1 && a.contains_key("text"),
            KIND_SCALE => a.len() == 1 && a.contains_key("scale"),
            _ => false,
        };
        if !keys_valid {
            return Err(format!("{id} 答案与题型不匹配"));
        }
        match q.kind.as_str() {
            KIND_SINGLE | KIND_MULTI => {
                let empty = Vec::new();
                let choices = match a.get("choice") {
                    Some(v) => v
                        .as_array()
                        .ok_or_else(|| format!("{id} choice 须为数组"))?,
                    None => &empty,
                };
                let mut unique = HashSet::new();
                for v in choices {
                    let s = v.as_str().ok_or_else(|| format!("{id} 选项须为 id"))?;
                    if !unique.insert(s)
                        || !q
                            .options
                            .as_ref()
                            .is_some_and(|options| options.iter().any(|o| o.id == s))
                    {
                        return Err(format!("{id} 选项无效或重复"));
                    }
                }
                let n = choices.len() + usize::from(other.is_some());
                let (min, max) = if q.kind == KIND_SINGLE {
                    (1, 1)
                } else {
                    (q.min.unwrap_or(1) as usize, q.max.unwrap_or(6) as usize)
                };
                if n < min || n > max {
                    return Err(format!("{id} 需要选择 {min}–{max} 项"));
                }
            }
            KIND_TEXT => {
                if a.get("text")
                    .and_then(Value::as_str)
                    .map_or(true, |s| s.trim().is_empty() || s.chars().count() > 32_000)
                {
                    return Err(format!("{id} 文本为空或过长"));
                }
            }
            KIND_SCALE => {
                let v = a
                    .get("scale")
                    .and_then(Value::as_i64)
                    .ok_or_else(|| format!("{id} 量表须为整数"))?;
                if v < q.min.unwrap_or(1) || v > q.max.unwrap_or(5) {
                    return Err(format!("{id} 量表越界"));
                }
            }
            _ => return Err(format!("{id} 题型无效")),
        }
    }
    Ok(answers.clone())
}

fn target_for(scope: &crate::scope::ScopeProject) -> Result<Value, String> {
    use assetd::project::{ForgeProject, RenderConfig};
    let existing = project_in_workspace(scope);
    // Scope resolution can fall back to the repository's demo for an empty
    // workspace. It is never the new game's source of configuration.
    let project = if existing {
        Some(ForgeProject::load(&scope.project_root).map_err(failure)?)
    } else {
        None
    };
    let config = project.as_ref().map(|p| p.render).unwrap_or_default();
    let (backend, method, driver) = config.as_strs();
    let env = |name| std::env::var(name).ok().filter(|v| !v.trim().is_empty());
    let eb = env("FORGE_RENDER_BACKEND");
    let em = env("FORGE_RENDER_METHOD");
    let ed = env("FORGE_RENDER_DRIVER");
    let (render, _) = RenderConfig::from_parts(
        eb.as_deref().or(Some(backend)),
        em.as_deref().or(method),
        ed.as_deref().or(driver),
    )
    .map_err(failure)?;
    let (backend, method, driver) = render.as_strs();
    Ok(
        json!({"renderBackend":backend,"renderMethod":method,"renderDriver":driver,
        "gameMode":project.as_ref().map(|p|p.mode.as_str()).unwrap_or("3d"),"existingProject":existing,"projectRoot":"."}),
    )
}

/// Bind production to the user's explicit inquiry-stage choice. Engine-wide
/// overrides must agree with it, and a new backend gets its own driver defaults.
fn target_for_answers(
    scope: &crate::scope::ScopeProject,
    questionnaire: &Value,
    answers: &Value,
) -> Result<Value, String> {
    let (mode, backend) = confirmed_implementation_stack(questionnaire, answers)?;
    let mut target = target_for(scope)?;
    if target["existingProject"] == true {
        if target["gameMode"] != mode || target["renderBackend"] != backend {
            return Err("已有项目技术栈已变化，请重新生成问卷并确认；不得静默迁移项目".into());
        }
    } else {
        if mode == "2d" && backend != "godot" {
            return Err("新建 2D 项目的技术栈应选择 Godot，请重新确认问卷".into());
        }
        let env = |name| std::env::var(name).ok().filter(|v| !v.trim().is_empty());
        if env("FORGE_RENDER_BACKEND").is_some_and(|v| v != backend) {
            return Err(
                "问卷选择与 FORGE_RENDER_BACKEND 冲突，请调整运行配置后重新确认技术栈".into(),
            );
        }
        let method = env("FORGE_RENDER_METHOD");
        let driver = env("FORGE_RENDER_DRIVER");
        let (render, _) = assetd::project::RenderConfig::from_parts(
            Some(backend),
            method.as_deref(),
            driver.as_deref(),
        )
        .map_err(failure)?;
        let (backend, method, driver) = render.as_strs();
        target["gameMode"] = json!(mode);
        target["renderBackend"] = json!(backend);
        target["renderMethod"] = json!(method);
        target["renderDriver"] = json!(driver);
    }
    target["stackConfirmed"] = json!(true);
    Ok(target)
}

fn validate_target_overrides(
    target: &Value,
    backend: Option<&str>,
    method: Option<&str>,
    driver: Option<&str>,
) -> Result<(), String> {
    let expected_backend = text_field(target, "renderBackend")?;
    if backend.is_some_and(|value| value != expected_backend) {
        return Err("ULTRAPLAN_PLAN_CHANGED: FORGE_RENDER_BACKEND 与已确认技术栈冲突，请恢复运行配置或重新确认计划".into());
    }
    let (actual, _) = assetd::project::RenderConfig::from_parts(
        Some(expected_backend),
        method.or_else(|| target["renderMethod"].as_str()),
        driver.or_else(|| target["renderDriver"].as_str()),
    )
    .map_err(failure)?;
    let (_, method, driver) = actual.as_strs();
    if target["renderMethod"] != json!(method) || target["renderDriver"] != json!(driver) {
        return Err(
            "ULTRAPLAN_PLAN_CHANGED: 渲染方法或驱动与批准目标冲突，请恢复运行配置或重新确认计划"
                .into(),
        );
    }
    Ok(())
}

/// Called after a production run is reserved, before its ScopeContext is rebuilt.
/// The normal permission service authorizes scaffold writes, including auto mode.
pub async fn initialize_production_project(
    state: &AppState,
    sid: &str,
    rid: &str,
    scope: &crate::scope::ScopeProject,
    ut: &UltraTurn,
) -> Result<(), String> {
    if !matches!(ut.kind, TurnKind::Production(_)) {
        return Ok(());
    }
    if state.permissions.mode(sid) == "plan" {
        return Err("ULTRAPLAN_NEEDS_BYPASS: 当前会话为只读计划权限，不能制作项目；请先选择允许写入的权限模式".into());
    }
    let up = ut.flow.as_ref().ok_or_else(|| failure("缺少流程"))?;
    let dir = up
        .dir_abs(&scope.workspace_root)
        .ok_or_else(|| failure("流程目录非法"))?;
    verify_plan(up, &scope.workspace_root, &dir)?;
    let target = read_json(&dir.join(TARGET)).ok_or_else(|| failure("缺少制作目标"))?;
    let env = |name| std::env::var(name).ok().filter(|v| !v.trim().is_empty());
    let backend = env("FORGE_RENDER_BACKEND");
    let method = env("FORGE_RENDER_METHOD");
    let driver = env("FORGE_RENDER_DRIVER");
    validate_target_overrides(
        &target,
        backend.as_deref(),
        method.as_deref(),
        driver.as_deref(),
    )?;
    if project_in_workspace(scope) && scope.project_root.join("forge.toml").is_file() {
        let actual = target_for(scope)?;
        if actual["renderBackend"] != target["renderBackend"]
            || actual["gameMode"] != target["gameMode"]
            || actual["renderMethod"] != target["renderMethod"]
            || actual["renderDriver"] != target["renderDriver"]
        {
            return Err("ULTRAPLAN_PLAN_CHANGED: 项目后端或维度已变化，请重新确认计划".into());
        }
        return Ok(());
    }
    let root = &scope.workspace_root;
    let defaults = assetd::project::ForgeProject::with_defaults(root.clone());
    if root.join(&defaults.entry_scene).exists() {
        return Err(failure(
            "工作区已有同名入口场景，不能用初始化覆盖；请先将现有项目配置完整",
        ));
    }
    let allowed=state.permissions.authorize_with(&state.events,sid,rid,"project_init",true,json!({"projectRoot":root,"renderBackend":target["renderBackend"],"gameMode":target["gameMode"]})).await?;
    if !allowed {
        return Err("ULTRAPLAN_PROJECT_INIT_DENIED: 项目初始化未获批准".into());
    }
    if state.runs.is_cancelled(rid) {
        return Err("ULTRAPLAN_CANCELLED: 已取消项目初始化".into());
    }
    let mode = text_field(&target, "gameMode")?;
    let backend = text_field(&target, "renderBackend")?;
    let (render, _) = assetd::project::RenderConfig::from_parts(
        Some(backend),
        target["renderMethod"].as_str(),
        target["renderDriver"].as_str(),
    )
    .map_err(failure)?;
    crate::project::init_project_with_backend(root, &scope.name, mode, Some(backend))
        .map_err(|(code, msg)| format!("{code}: {msg}"))?;
    let mut project = assetd::project::ForgeProject::load(root).map_err(failure)?;
    project.render = render;
    project.save_manifest().map_err(failure)?;
    Ok(())
}

pub(super) fn begin_followup(
    state: &AppState,
    sid: &str,
    rid: &str,
    scope: &crate::scope::ScopeProject,
    user: &str,
    ut: &UltraTurn,
) -> Result<UltraRuntime, String> {
    let snap = ut.flow.as_ref().ok_or_else(|| failure("缺少流程"))?;
    let dir = snap
        .dir_abs(&scope.workspace_root)
        .ok_or_else(|| failure("流程目录非法"))?;
    match ut.kind {
        TurnKind::SpecAndDemo { revise: false } => {
            let q =
                read_json(&dir.join(QUESTIONNAIRE_FILE)).ok_or_else(|| failure("问卷文件缺失"))?;
            let saved = read_json(&dir.join(ANSWERS_FILE));
            let raw = ut
                .answers
                .as_ref()
                .or_else(|| {
                    saved
                        .as_ref()
                        .filter(|v| v["rev"] == snap.questionnaire_rev)
                        .and_then(|v| v.get("answers"))
                })
                .ok_or("ULTRAPLAN_ANSWERS_INVALID: 缺少答案")?;
            let answers =
                validate_answers(&q, raw).map_err(|e| format!("ULTRAPLAN_ANSWERS_INVALID: {e}"))?;
            let target = target_for_answers(scope, &q, &answers)
                .map_err(|e| format!("ULTRAPLAN_ANSWERS_INVALID: {e}"))?;
            save(
                &dir,
                ANSWERS_FILE,
                &json!({"rev": snap.questionnaire_rev, "answers": answers}),
            )?;
            let markdown = answers_markdown(&q, &answers);
            write_atomic(&dir.join(ANSWERS_MD_FILE), &markdown).map_err(failure)?;
            save(&dir, TARGET, &target)?;
            emit(
                state,
                sid,
                "ultraplan.answers.submitted",
                json!({"runId":rid,"id":snap.id,"rev":snap.questionnaire_rev,"answers":answers}),
            );
        }
        TurnKind::SpecAndDemo { revise: true } | TurnKind::Planning { revise: true } => {
            append_brief(&dir.join(FEEDBACK), user).map_err(failure)?;
        }
        TurnKind::Planning { revise: false } => {
            let expected = read_json(&dir.join("demo-digest.json"))
                .ok_or("ULTRAPLAN_DEMO_CHANGED: Demo 缺少版本指纹，请重新制作")?;
            if expected["iteration"] != snap.demo_iteration
                || expected["sha256"] != demo_digest(&dir.join(DEMO_DIR))?
            {
                return Err(
                    "ULTRAPLAN_DEMO_CHANGED: Demo 已在流程外修改，请重新制作并试玩确认".into(),
                );
            }
            if !dir.join(DEMO_DIR).join("index.html").is_file() {
                return Err("ULTRAPLAN_DEMO_MISSING: Demo 入口不存在".into());
            }
            save(
                &dir,
                "demo-approved.json",
                &json!({"iteration":snap.demo_iteration,"at":now_rfc3339()}),
            )?;
        }
        TurnKind::Production(phase) => {
            if state.permissions.mode(sid) == "plan" {
                return Err("ULTRAPLAN_NEEDS_BYPASS: 当前会话没有制作项目的写入权限".into());
            }
            verify_plan(snap, &scope.workspace_root, &dir)?;
            if phase == ProductionPhase::Fix {
                let a = read_json(&dir.join(ACCEPTANCE_FILE))
                    .ok_or("ULTRAPLAN_FIX_NO_TASKS: 没有验收记录")?;
                if a["rounds"]
                    .as_array()
                    .and_then(|r| r.last())
                    .and_then(|r| r["failed"].as_array())
                    .map_or(true, Vec::is_empty)
                {
                    return Err("ULTRAPLAN_FIX_NO_TASKS: 没有未通过的检查项".into());
                }
            }
            materialize(state, sid, snap, &dir, phase)?;
        }
        _ => {}
    }
    let updated = state
        .sessions
        .update_ultraplan(sid, |slot| {
            let up = slot.as_mut().filter(|u| u.id == snap.id)?;
            up.phase = PHASE_RUNNING.into();
            up.running = Some(ut.kind.running().into());
            up.last_error = None;
            if matches!(ut.kind, TurnKind::Production(_)) {
                up.stage = STAGE_PRODUCTION.into();
                up.production_run_id = Some(rid.into());
            }
            Some(up.clone())
        })
        .and_then(|(_, up)| up)
        .ok_or_else(|| failure("流程在开始时变化"))?;
    emit(
        state,
        sid,
        "ultraplan.stage",
        stage_payload_with(&updated, Some(rid), &ut.deep),
    );
    if let TurnKind::Production(phase) = ut.kind {
        emit(
            state,
            sid,
            "ultraplan.production.started",
            json!({"id":snap.id,"runId":rid,"phase":phase.as_str(),"rev":snap.plan_rev}),
        );
    }
    let mut context = requirement_context(&dir, snap)?;
    if matches!(ut.kind, TurnKind::Production(_)) {
        context.push_str(&production_delegation_context(state, sid, snap, &dir)?);
    }
    let (prompt, specs) = match ut.kind {
        TurnKind::SpecAndDemo { .. } => (SPEC_PROMPT, vec![spec_tool()]),
        TurnKind::Planning { .. } => (PLAN_PROMPT, vec![plan_doc_tool(), plan_tasks_tool()]),
        _ => (PRODUCTION_PROMPT, vec![]),
    };
    Ok(UltraRuntime {
        kind: ut.kind,
        flow_id: snap.id.clone(),
        dir_rel: snap.dir.clone(),
        dir_abs: dir,
        facts: ut.facts.clone(),
        deep: ut.deep.clone(),
        deep_fallback: ut.deep_fallback.clone(),
        tracker: TurnTracker::default(),
        prompt_suffix: prompt.into(),
        exit_tool_specs: specs,
        preamble: context,
        sections: vec![],
    })
}

fn answers_markdown(q: &Value, answers: &Value) -> String {
    let mut result = String::from("# 已确认问答（原始答案同时保存在 answers.json）\n\n");
    for section in q["sections"].as_array().into_iter().flatten() {
        result.push_str(&format!(
            "## {}\n\n",
            section["title"].as_str().unwrap_or_default()
        ));
        for question in section["questions"].as_array().into_iter().flatten() {
            let id = question["id"].as_str().unwrap_or_default();
            result.push_str(&format!(
                "### {}\n{}\n\n",
                question["question"].as_str().unwrap_or(id),
                answers.get(id).unwrap_or(&Value::Null)
            ));
        }
    }
    result
}

fn requirement_files(dir: &Path) -> Vec<PathBuf> {
    let mut files: Vec<_> = [
        BRIEF_FILE,
        UNDERSTANDING_FILE,
        QUESTIONNAIRE_FILE,
        ANSWERS_FILE,
        ANSWERS_MD_FILE,
        SPEC_FILE,
        TARGET,
        FEEDBACK,
        TEAM_PLAN_FILE,
        CHECKS_FILE,
    ]
    .iter()
    .chain([DELIVERY].iter())
    .map(|f| dir.join(f))
    .filter(|p| p.is_file())
    .collect();
    files.extend(explore_reports(dir).into_iter().map(|(_, p)| p));
    files
}
fn requirement_context(dir: &Path, up: &UltraPlanState) -> Result<String, String> {
    let rel = &up.dir;
    let mut manifest = Vec::new();
    let mut context = format!("完整需求包位于 {rel}/{MANIFEST}。开工前逐文件全文读取；长文件分段读取，不得只根据摘要执行。\n");
    for path in requirement_files(dir) {
        let bytes = std::fs::read(&path).map_err(failure)?;
        let name = path
            .strip_prefix(dir)
            .map_err(failure)?
            .to_string_lossy()
            .replace('\\', "/");
        manifest.push(
            json!({"path":format!("{rel}/{name}"),"bytes":bytes.len(),"sha256":sha256_hex(&bytes)}),
        );
        context.push_str(&format!("- {rel}/{name}\n"));
    }
    let digest = sha256_hex(&serde_json::to_vec(&manifest).map_err(failure)?);
    save(
        dir,
        MANIFEST,
        &json!({"schema":1,"flowId":up.id,"version":digest,
        "questionnaireRev":up.questionnaire_rev,"demoIteration":up.demo_iteration,
        "planRev":up.plan_rev,"files":manifest}),
    )?;
    Ok(context)
}

/// Expose real task IDs and failure context before the leader's first tool call.
/// Keep the complete snapshot readable even when individual prompts are long.
fn production_delegation_context(
    state: &AppState,
    sid: &str,
    up: &UltraPlanState,
    dir: &Path,
) -> Result<String, String> {
    let report = read_json(&dir.join(PRODUCTION_FILE)).ok_or_else(|| failure("制作记录缺失"))?;
    let ids = report.get("repairTodoIds").unwrap_or(&report["activeTodoIds"])
        .as_array()
        .ok_or_else(|| failure("当前制作任务缺失"))?;
    let source = format!("ultraplan:{}:{}", up.id, up.plan_rev);
    let todos = state.todos.list_by_session(sid);
    let mut records = Vec::new();
    let mut context = format!("\n本轮开始时的制作任务真实 id 和内容预览如下。开工前全文读取 {}/{DELEGATIONS} 快照中的 description/prompt 及关联需求；空 prompt 必须由主 agent 编写详细委派后用 todo_update {{id,prompt}} 补齐，不得猜测任务 id。后续修改以 todo_update 返回结果为准。\n", up.dir);
    for id in ids {
        let todo = todos
            .iter()
            .find(|todo| todo.source == source && Some(todo.id.as_str()) == id.as_str())
            .ok_or_else(|| failure("当前任务不属于本流程"))?;
        records.push(json!({"id":todo.id,"title":todo.title,"status":todo.status,"prompt":todo.prompt,"description":todo.description}));
        let preview =
            |text: Option<&str>| text.map(|text| text.chars().take(800).collect::<String>());
        context.push_str(&format!("{}\n", json!({"id":todo.id,"title":todo.title,"status":todo.status,
            "prompt":preview(todo.prompt.as_deref()),"description":preview(todo.description.as_deref())})));
    }
    save(
        dir,
        DELEGATIONS,
        &json!({"flowId":up.id,"planRev":up.plan_rev,"tasks":records}),
    )?;
    let checks = read_json(&dir.join(CHECKS_FILE)).ok_or_else(|| failure("检查清单缺失"))?;
    let mut check_context = super::verify::check_context(&checks);
    check_context
        .as_object_mut()
        .unwrap()
        .remove("matrixSchema");
    context.push_str(&format!("\n最终集中QA的合法检查上下文：{check_context}\n调用ultraplan_verify时id必须来自allowedCheckIds，不是流程id或todo id；matrix.scene使用对应批准scene，矩阵字段遵循工具schema。\n"));
    Ok(context)
}

pub fn production_context(rt: &UltraRuntime) -> String {
    let mut out = rt.preamble.clone();
    for file in [
        SPEC_FILE,
        TARGET,
        CHECKS_FILE,
        DELIVERY,
        FEEDBACK,
        ACCEPTANCE_FILE,
    ] {
        if let Ok(text) = std::fs::read_to_string(rt.dir_abs.join(file)) {
            out.push_str(&format!("\n## {file}\n{text}\n"));
        }
    }
    out
}

const SPEC_PROMPT: &str = "\nUltraPlan · 需求定稿。先全文读取需求包所有文件，包括原始需求、问卷选项、答案、target.json、探索报告和当前反馈。由你理解需求后，使用 ultraplan_spec 提交完整 spec、gameMode(2d/3d)、renderBackend(rurix/godot) 和本轮亲自编写的 demoPrompt。技术栈必须与询问阶段已确认的 implementation_stack 和 target.json 一致，禁止到定稿或制作时再代选后端。新项目 2D 默认 Godot，3D 由用户确认 Godot 或 rurix；已有项目沿用已确认的目标配置。spec 必须包含每条需求、委托决策及理由、可玩核心循环、操作/胜负/重开、美术/UI/音频方向、MVP取舍、资产引用和Demo与正式游戏差异。需规划好技术与实施细节：玩法状态与数据结构、场景/资源组织、模块职责及接口、输入与存档方案、目标后端能力和限制、关键技术风险与验证办法。demoPrompt 是你给 web-demo-builder 的明确委派，首次和修订轮都必须根据已读需求和答案重新编写；写成自足、针对本项目的详细提示词，包含具体目标、玩法/操作/胜负/重开、视觉与资产方向、读取需求包及仅写隔离目录的范围、实现要求、预期产物和可执行验收步骤，不可只说开始或使用通用模板。你只读，不自行实现 Demo；调用 ultraplan_spec 即提交你编写的这份 Demo 委派，协调器随后将原文交给恰一个 web-demo-builder 执行。";
const PLAN_PROMPT: &str = "\nUltraPlan · 制作计划。先全文读取需求包、target.json、Demo批准记录和反馈；沿用询问阶段已确认的维度与后端，核实 render_backend_info/render_capabilities。使用 ultraplan_plan_doc 提交 name/overview/plan，正文必须包含范围、技术选型及理由、目标后端、资产复用、模块接口、制作步骤、Team职责、自动画面/玩法验证和最终人工行为检查。必须规划好技术与实施细节，不能只有功能标题：逐项写清场景/资源/脚本路径与命名、数据结构与玩法状态转换、模块接口和输入/存档方案、后端组件及能力限制、任务依赖与执行顺序、风险处理；每步注明实现方法、交付物、验收步骤与预期结果，并让任务 prompt 带齐这些细节。再用 ultraplan_plan_tasks 提交完整tasks、checks和delivery。delivery.entry是工作区相对的正式游戏入口场景，delivery.controls是完整操作说明。任务字段 id/title/role/prompt/stage/deps/verify=qa/checkIds/requiredTools/outputs/handoffChecks；deps使用任务id。requiredTools列工种实际需要的宿主工具原名；outputs列明确工作区相对文件；handoffChecks逐产物声明file/json/scene/graph/rx或matrix检查，matrix绑定输出场景并遵守playtest矩阵schema。上游仅通过轻量、可执行交接门槛才放行下游；checkIds对应最终集中QA，禁止以本阶段尚未实现的完整成品作为局部交接。仅派 material-smith、asset-wrangler、scene-builder、logic-programmer。共享场景/play任务串行。checks.automated 每项 id/kind(visual|gameplay)/scene/steps/expected，checks.manual 每项 id/title/steps/expected/required。人工检查只在最终集中验收。至少一个画面和一个玩法自动检查，所有检查关联产出任务。不要实现游戏或改项目。";
const PRODUCTION_PROMPT: &str = "\nUltraPlan · 连续制作。使用系统物化的既有任务id与批准契约，不用plan_write追加替代任务或改变范围。先核对任务快照、当前阻塞、上游结构化交接及本模块必要需求；已读信息沿用，不要求所有子agent重复全文领读。按requiredTools核验当前工种能力，缺宿主能力先提交blocked及明确恢复条件，暂停依赖链，不把其余未执行任务都转成修复。修复只针对可确认的代码/资产缺陷：用todo_update {id,prompt}更新原任务，保留已通过内容与批准outputs/handoffChecks；外部条件未变化时不重派。子agent结束须提交结构化结果，包含实际产物、执行过的交接检查、失败原因和恢复条件；宿主验证交接后才记completed。全部产出交接完成后集中执行checkIds对应自动QA与reviewer，仅实际断言失败触发定向修复；missing/stale证据要求补验证，不新建整条生产链。全部自动检查与reviewer通过后才集中人工验收。";

fn tool(name: &str, description: &str, properties: Value, required: &[&str]) -> Value {
    json!({"type":"function","function":{"name":name,"description":description,"parameters":{"type":"object","properties":properties,"required":required}}})
}
fn spec_tool() -> Value {
    tool(
        SPEC_TOOL,
        "保存完整需求规格，并提交主 agent 阅读需求后亲自编写的详细 demoPrompt，交给单个 Demo 子代理执行",
        json!({"spec":{"type":"string"},"gameMode":{"type":"string","enum":["2d","3d"]},"renderBackend":{"type":"string","enum":["rurix","godot"]},"demoPrompt":{"type":"string","minLength":1,"description":"主 agent 针对本轮项目需求编写的自足 Demo 委派：目标、玩法和视觉、读写范围、实现要求、产物与验收"}}),
        &["spec", "gameMode", "renderBackend", "demoPrompt"],
    )
}
fn plan_doc_tool() -> Value {
    tool(
        PLAN_DOC_TOOL,
        "保存供用户确认的详细制作计划，与结构任务同版提交",
        json!({"name":{"type":"string"},"overview":{"type":"string"},"plan":{"type":"string"}}),
        &["name", "overview", "plan"],
    )
}
fn plan_tasks_tool() -> Value {
    tool(PLAN_TASKS_TOOL,"保存批准任务契约及最终集中自动/人工检查。requiredTools 使用宿主工具名；每个 outputs 文件须有可执行 handoffChecks，交接通过后才放行依赖。",json!({
        "tasks":{"type":"array","minItems":1,"maxItems":64,"items":{"type":"object","properties":{
            "id":{"type":"string"},"title":{"type":"string"},"role":{"type":"string","enum":["material-smith","asset-wrangler","scene-builder","logic-programmer"]},
            "prompt":{"type":"string"},"stage":{"type":"string"},"deps":{"type":"array","items":{"type":"string"}},"verify":{"type":"string","enum":["qa"]},"checkIds":{"type":"array","items":{"type":"string"}},
            "requiredTools":{"type":"array","minItems":1,"uniqueItems":true,"items":{"type":"string"},"description":"当前工种必须具备的宿主工具，使用 mcp__.../read_file 等原名，不加 forge__ 前缀"},
            "outputs":{"type":"array","minItems":1,"uniqueItems":true,"items":{"type":"string"},"description":"工作区相对文件路径，不可为目录、通配符或外部路径"},
            "handoffChecks":{"type":"array","minItems":1,"items":{"type":"object","additionalProperties":false,"properties":{
                "kind":{"type":"string","enum":["file","json","scene","graph","rx","matrix"]},"path":{"type":"string","description":"必须列在 outputs 中"},"matrix":super::verify::matrix_schema()
            },"required":["kind","path"]}}
        },"required":["id","title","role","prompt","stage","deps","verify","checkIds","requiredTools","outputs","handoffChecks"]}},
        "checks":{"type":"object"},"delivery":{"type":"object","properties":{"entry":{"type":"string"},"controls":{"type":"string"}},"required":["entry","controls"]}
    }), &["tasks","checks","delivery"])
}

pub(super) fn handle_spec(
    state: &AppState,
    sid: &str,
    rid: &str,
    rt: &UltraRuntime,
    args: &Value,
) -> (bool, String) {
    let result = (|| -> Result<(), String> {
        // Keep each spec and its leader-authored assignment paired even when
        // the model submits multiple tool calls concurrently.
        let mut delegation = rt.tracker.demo_prompt.lock().unwrap();
        let up = get_flow(state, sid, rt)?;
        let spec = text_field(args, "spec")?;
        let demo_prompt = text_field(args, "demoPrompt")?;
        let mode = text_field(args, "gameMode")?;
        let backend = text_field(args, "renderBackend")?;
        if !["2d", "3d"].contains(&mode) || !["rurix", "godot"].contains(&backend) {
            return Err("gameMode 或 renderBackend 无效".into());
        }
        let target = read_json(&rt.dir_abs.join(TARGET))
            .ok_or("缺少已确认的技术栈，请重新生成问卷并确认")?;
        let questionnaire =
            read_json(&rt.dir_abs.join(QUESTIONNAIRE_FILE)).ok_or("问卷文件缺失")?;
        let answers = read_json(&rt.dir_abs.join(ANSWERS_FILE)).ok_or("问卷答案缺失")?;
        let (confirmed_mode, confirmed_backend) =
            confirmed_implementation_stack(&questionnaire, &answers["answers"])?;
        if target["stackConfirmed"] != true
            || target["gameMode"] != confirmed_mode
            || target["renderBackend"] != confirmed_backend
        {
            return Err("目标技术栈与已确认问卷不一致，请重新确认问卷".into());
        }
        if confirmed_mode != mode || confirmed_backend != backend {
            return Err("gameMode 和 renderBackend 必须沿用询问阶段已确认的技术栈；不得在需求定稿时更换后端".into());
        }
        *delegation = None;
        write_atomic(&rt.dir_abs.join(SPEC_FILE), spec).map_err(failure)?;
        save(&rt.dir_abs, TARGET, &target)?;
        requirement_context(&rt.dir_abs, &up)?;
        // A fresh runtime must receive a fresh leader delegation even if an old
        // spec survived a failed run or was loaded from an earlier session.
        *delegation = Some(demo_prompt.to_owned());
        emit(
            state,
            sid,
            "ultraplan.spec.ready",
            json!({"id":rt.flow_id,"runId":rid,"specPath":format!("{}/{SPEC_FILE}",rt.dir_rel),"renderBackend":backend}),
        );
        Ok(())
    })();
    match result {
        Ok(()) => (
            true,
            "需求与本轮 Demo 委派已保存；本轮结束后按你的 demoPrompt 执行。".into(),
        ),
        Err(e) => (false, e),
    }
}

/// All planning artifacts validate together before any new revision becomes
/// visible. Their composite hash catches edits to either markdown or the DAG.
pub(super) fn handle_plan(
    state: &AppState,
    sid: &str,
    rid: &str,
    rt: &UltraRuntime,
    name: &str,
    args: &Value,
) -> (bool, String) {
    let result = (|| -> Result<String, String> {
        get_flow(state, sid, rt)?;
        // Serialize both exit tools. Repeated/parallel calls in this turn must
        // not publish another revision after the user-facing gate is visible.
        let mut document = rt.tracker.plan_document.lock().unwrap();
        if document.as_ref().is_some_and(|d| d["_published"] == true) {
            return Ok("本轮制作计划已经发布，等待用户确认。".into());
        }
        if name == PLAN_DOC_TOOL {
            for field in ["name", "overview", "plan"] {
                text_field(args, field)?;
            }
            *document = Some(args.clone());
        } else {
            validate_tasks(args)?;
            *rt.tracker.plan_tasks.lock().unwrap() = Some(args.clone());
        }
        let doc = document.clone();
        let tasks = rt.tracker.plan_tasks.lock().unwrap().clone();
        let (Some(doc), Some(tasks)) = (doc, tasks) else {
            return Ok("已暂存；请继续提交另一份计划产物。".into());
        };
        let up = get_flow(state, sid, rt)?;
        let ws = flow_ws_root(state, &up);
        let path = up.reserved_plan_path();
        let list = tasks["tasks"].as_array().unwrap();
        let todos = list
            .iter()
            .map(|t| crate::plan_doc::PlanTodo {
                id: t["id"].as_str().unwrap().into(),
                content: t["title"].as_str().unwrap().into(),
                status: "pending".into(),
            })
            .collect();
        let written = crate::plan_doc::write_plan(
            &ws,
            Some(&path),
            doc["name"].as_str().unwrap(),
            doc["overview"].as_str().unwrap(),
            doc["plan"].as_str().unwrap(),
            todos,
        )?;
        save(&rt.dir_abs, TEAM_PLAN_FILE, &tasks["tasks"])?;
        save(&rt.dir_abs, CHECKS_FILE, &tasks["checks"])?;
        save(&rt.dir_abs, DELIVERY, &tasks["delivery"])?;
        let hash = plan_hash(&ws, &path, &rt.dir_abs)?;
        let up = advance(state, sid, rid, rt, |u| {
            u.plan_rev += 1;
            u.plan_path = Some(path.clone());
            u.plan_hash = Some(hash.clone());
            u.stage = STAGE_PLAN_REVIEW.into();
        })?;
        state
            .sessions
            .set_ultraplan_active_plan(sid, &up.id, rid, &path);
        document.as_mut().unwrap()["_published"] = json!(true);
        emit(
            state,
            sid,
            "ultraplan.plan.ready",
            json!({"id":up.id,"runId":rid,"rev":up.plan_rev,"planPath":path,"name":written.front.name,"overview":written.front.overview,
            "taskCount":list.len(),"roles":list.iter().filter_map(|t|t["role"].as_str()).collect::<HashSet<_>>(),
            "gameMode":read_json(&rt.dir_abs.join(TARGET)).map(|v|v["gameMode"].clone()),"renderBackend":read_json(&rt.dir_abs.join(TARGET)).map(|v|v["renderBackend"].clone()),
            "automated":tasks["checks"]["automated"].as_array().unwrap().len(),"manual":tasks["checks"]["manual"].as_array().unwrap().len()}),
        );
        Ok("制作计划已发布，等待用户确认。".into())
    })();
    match result {
        Ok(s) => (true, s),
        Err(e) => (false, e),
    }
}

/// A missing contract is an explicit plan upgrade, never an implicit rewrite of
/// an already approved plan or its hash. Runtime validates role capabilities.
pub(super) fn validate_task_contract(task: &Value) -> Result<(), String> {
    for key in ["requiredTools", "outputs", "handoffChecks"] {
        if !task.get(key).is_some_and(Value::is_array) {
            return Err(format!("UP_CONTRACT_UPGRADE_REQUIRED: 任务 {} 缺少 {key}；请重新提交并确认契约，保留原批准版本", task["id"]));
        }
    }
    let strings = |key: &str| -> Result<Vec<&str>, String> {
        let entries = task[key].as_array().unwrap();
        if entries.is_empty() || entries.len() > 128 {
            return Err(format!("{key} 需要1–128项"));
        }
        let mut seen = HashSet::new();
        entries
            .iter()
            .map(|value| {
                let text = value
                    .as_str()
                    .filter(|s| !s.trim().is_empty() && s.trim() == *s)
                    .ok_or_else(|| format!("{key} 须为非空字符串数组"))?;
                if !seen.insert(text) {
                    return Err(format!("{key} 不可重复"));
                }
                Ok(text)
            })
            .collect()
    };
    for name in strings("requiredTools")? {
        if name.starts_with("forge__")
            || !name
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || "_-".contains(c))
        {
            return Err("requiredTools 须使用宿主工具原名，不加 forge__ 前缀".into());
        }
    }
    let outputs = strings("outputs")?;
    for path in &outputs {
        safe_relative(path)?;
        if path.contains(['*', '?', '\0'])
            || path
                .split(['/', '\\'])
                .any(|part| part.ends_with([' ', '.']))
            || path.split(['/', '\\']).next().is_some_and(|part| {
                part.eq_ignore_ascii_case(".forge") || part.eq_ignore_ascii_case(".git")
            })
        {
            return Err("outputs 必须是明确文件路径".into());
        }
    }
    let checks = task["handoffChecks"].as_array().unwrap();
    if checks.is_empty() || checks.len() > 256 {
        return Err("handoffChecks 需要1–256项".into());
    }
    let mut covered = HashSet::new();
    for check in checks {
        let kind = text_field(check, "kind")?;
        let path = text_field(check, "path")?;
        if !outputs.contains(&path) {
            return Err(format!("交接路径 {path} 未列入 outputs"));
        }
        if !["file", "json", "scene", "graph", "rx", "matrix"].contains(&kind) {
            return Err(format!("未知交接检查 {kind}"));
        }
        if check
            .as_object()
            .unwrap()
            .keys()
            .any(|k| !["kind", "path", "matrix"].contains(&k.as_str()))
        {
            return Err("handoffChecks 仅接受 kind/path/matrix".into());
        }
        if kind == "matrix" {
            let matrix: crate::playtest::Matrix = serde_json::from_value(check["matrix"].clone())
                .map_err(|e| format!("交接 matrix 无效: {e}"))?;
            super::verify::validate_matrix_shape(&matrix)?;
            if matrix.scene != path || matrix.cases.is_empty() || !path.ends_with(".rxscene") {
                return Err("交接 matrix 必须绑定 outputs 中的 .rxscene，且包含非空 cases".into());
            }
        } else if check.get("matrix").is_some() {
            return Err("只有 matrix 交接检查可包含 matrix".into());
        }
        covered.insert(path);
    }
    if outputs.iter().any(|path| !covered.contains(path)) {
        return Err("每个 outputs 文件都必须包含 handoffChecks".into());
    }
    for path in outputs {
        let required = match Path::new(path)
            .extension()
            .and_then(|value| value.to_str())
            .map(str::to_ascii_lowercase)
            .as_deref()
        {
            Some("rx") => Some("rx"),
            Some("rxgraph") => Some("graph"),
            Some("rxscene") => Some("scene"),
            _ => None,
        };
        if let Some(kind) = required {
            if !checks
                .iter()
                .any(|check| check["path"] == path && check["kind"] == kind)
            {
                return Err(format!(
                    "产物 {path} 必须包含 {kind} 交接检查；file/json/matrix不能替代结构或编译检查"
                ));
            }
        }
    }
    Ok(())
}

fn validate_tasks(v: &Value) -> Result<(), String> {
    let entry = text_field(&v["delivery"], "entry")?;
    text_field(&v["delivery"], "controls")?;
    safe_relative(entry)?;
    let tasks = v["tasks"]
        .as_array()
        .filter(|a| !a.is_empty() && a.len() <= 64)
        .ok_or("tasks 需要1–64项")?;
    let automated = v["checks"]["automated"]
        .as_array()
        .filter(|a| !a.is_empty())
        .ok_or("需要自动检查")?;
    let manual = v["checks"]["manual"]
        .as_array()
        .filter(|a| !a.is_empty())
        .ok_or("需要人工检查")?;
    let mut checks = HashSet::new();
    for c in automated.iter().chain(manual) {
        for k in ["id", "steps", "expected"] {
            text_field(c, k)?;
        }
        if !checks.insert(c["id"].as_str().unwrap()) {
            return Err("检查id重复".into());
        }
    }
    for c in manual {
        text_field(c, "title")?;
        if c.get("required").is_some_and(|v| !v.is_boolean()) {
            return Err("required须为布尔值".into());
        }
    }
    for c in automated {
        safe_relative(text_field(c, "scene")?)?;
    }
    for kind in ["visual", "gameplay"] {
        if !automated.iter().any(|c| c["kind"] == kind) {
            return Err(format!("缺少{kind}自动检查"));
        }
    }
    if automated
        .iter()
        .any(|c| !matches!(c["kind"].as_str(), Some("visual" | "gameplay")))
    {
        return Err("自动检查kind非法".into());
    }
    let mut ids = HashSet::new();
    let mut titles = HashSet::new();
    let mut covered = HashSet::new();
    for t in tasks {
        validate_task_contract(t)?;
        for key in ["id", "title", "role", "prompt", "stage"] {
            text_field(t, key)?;
        }
        let id = t["id"].as_str().unwrap();
        if !id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
            || !ids.insert(id)
            || !titles.insert(t["title"].as_str().unwrap())
        {
            return Err("任务id非法或id/title重复".into());
        }
        if ![
            "material-smith",
            "asset-wrangler",
            "scene-builder",
            "logic-programmer",
        ]
        .contains(&t["role"].as_str().unwrap())
        {
            return Err("任务工种非法".into());
        }
        if t["verify"] != "qa" {
            return Err("产出任务必须verify=qa".into());
        }
        for id in t["checkIds"].as_array().ok_or("任务必须包含checkIds")? {
            let id = id.as_str().ok_or("checkIds须为字符串")?;
            if !checks.contains(id) {
                return Err(format!("未知检查{id}"));
            }
            covered.insert(id);
        }
        t["deps"].as_array().ok_or("deps须为数组")?;
    }
    for c in automated.iter().chain(manual) {
        if !covered.contains(c["id"].as_str().unwrap()) {
            return Err(format!("检查{}未分配任务", c["id"]));
        }
    }
    let mut resolved = HashSet::new();
    loop {
        let before = resolved.len();
        for t in tasks {
            let id = t["id"].as_str().unwrap();
            let deps = t["deps"].as_array().unwrap();
            if deps
                .iter()
                .any(|d| d.as_str().map_or(true, |s| !ids.contains(s) || s == id))
            {
                return Err("依赖不存在或依赖自身".into());
            }
            if deps.iter().all(|d| resolved.contains(d.as_str().unwrap())) {
                resolved.insert(id);
            }
        }
        if resolved.len() == tasks.len() {
            break;
        }
        if before == resolved.len() {
            return Err("任务依赖有环".into());
        }
    }
    Ok(())
}
fn plan_hash(ws: &Path, path: &str, dir: &Path) -> Result<String, String> {
    let mut bytes = Vec::new();
    for file in [
        crate::plan_doc::confined_path(ws, path)?,
        dir.join(TEAM_PLAN_FILE),
        dir.join(CHECKS_FILE),
        dir.join(TARGET),
        dir.join(DELIVERY),
    ] {
        let content = std::fs::read(file).map_err(failure)?;
        bytes.extend_from_slice(&(content.len() as u64).to_le_bytes());
        bytes.extend(content);
    }
    Ok(sha256_hex(&bytes))
}
fn verify_plan(up: &UltraPlanState, ws: &Path, dir: &Path) -> Result<(), String> {
    let path = up
        .plan_path
        .as_deref()
        .ok_or("ULTRAPLAN_PLAN_CHANGED: 缺少计划")?;
    if up.plan_hash.as_deref() != Some(plan_hash(ws, path, dir)?.as_str()) {
        return Err("ULTRAPLAN_PLAN_CHANGED: 计划、任务或验证清单已修改，请重新生成并确认".into());
    }
    Ok(())
}

fn safe_relative(value: &str) -> Result<(), String> {
    if value.is_empty()
        || value.contains(':')
        || value.starts_with(['/', '\\'])
        || value
            .split(['/', '\\'])
            .any(|p| p.is_empty() || p == "." || p == "..")
    {
        return Err(failure("路径必须为工作区内的相对路径，不允许目录穿越"));
    }
    Ok(())
}

fn no_link(path: &Path) -> Result<(), String> {
    let meta = std::fs::symlink_metadata(path).map_err(failure)?;
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        if meta.file_attributes() & 0x400 != 0 {
            return Err(failure("产物路径不能包含重解析点"));
        }
    }
    if meta.file_type().is_symlink() {
        return Err(failure("产物路径不能包含符号链接"));
    }
    Ok(())
}

fn evidence_file(ws: &Path, relative: &str) -> Result<PathBuf, String> {
    safe_relative(relative)?;
    let root = ws.canonicalize().map_err(failure)?;
    let mut path = root.clone();
    for component in relative.split(['/', '\\']) {
        path.push(component);
        no_link(&path)?;
    }
    let path = path.canonicalize().map_err(failure)?;
    if !path.starts_with(&root) || !path.is_file() {
        return Err(failure("证据必须为工作区内的普通文件"));
    }
    Ok(path)
}

fn materialize(
    state: &AppState,
    sid: &str,
    up: &UltraPlanState,
    dir: &Path,
    phase: ProductionPhase,
) -> Result<(), String> {
    let tasks = read_json(&dir.join(TEAM_PLAN_FILE)).ok_or_else(|| failure("任务文件缺失"))?;
    let tasks = tasks
        .as_array()
        .filter(|a| !a.is_empty())
        .ok_or_else(|| failure("禁止空任务制作"))?;
    for task in tasks {
        validate_task_contract(task)?;
    }
    let source = format!("ultraplan:{}:{}", up.id, up.plan_rev);
    let existing: Vec<_> = state
        .todos
        .list_by_session(sid)
        .into_iter()
        .filter(|t| t.source == source)
        .collect();
    let prefix = format!("{}:{}:", up.id, up.plan_rev);
    let old = read_json(&dir.join(PRODUCTION_FILE));
    let same_plan = old.as_ref().is_some_and(|p| {
        p["flowId"] == up.id && p["planRev"] == up.plan_rev && p["planHash"] == json!(up.plan_hash)
    });
    let acceptance = read_json(&dir.join(ACCEPTANCE_FILE));
    let latest = acceptance
        .as_ref()
        .and_then(|a| a["rounds"].as_array())
        .and_then(|r| r.last());
    let repair_round = if phase == ProductionPhase::Fix {
        latest.and_then(|r| r["round"].as_u64())
    } else if phase == ProductionPhase::Resume && same_plan {
        old.as_ref().and_then(|p| p["repairRound"].as_u64())
    } else {
        None
    };
    let repair_record = repair_round.and_then(|round| {
        acceptance
            .as_ref()
            .and_then(|a| a["rounds"].as_array())
            .and_then(|rs| rs.iter().find(|r| r["round"] == round))
    });
    let failed: HashSet<&str> = if repair_round.is_some() {
        repair_record
            .and_then(|r| r["failed"].as_array())
            .ok_or("ULTRAPLAN_FIX_NO_TASKS: 修复轮的失败记录不存在")?
            .iter()
            .filter_map(Value::as_str)
            .collect()
    } else {
        HashSet::new()
    };
    if repair_round.is_some() && failed.is_empty() {
        return Err("ULTRAPLAN_FIX_NO_TASKS: 没有失败检查".into());
    }
    // Reuse the selected original tasks once per repair round. Historical fix
    // chains remain reference only; unaffected completed tasks and modern
    // leader delegations are preserved.
    let migrate_repair_delegations = repair_round.is_some()
        && !old.as_ref().is_some_and(|report| {
            same_plan
                && report["repairRound"].as_u64() == repair_round
                && report["repairDelegationVersion"] == 2
        });
    let selected: HashSet<&str> = tasks
        .iter()
        .filter(|t| {
            repair_round.is_none()
                || t["checkIds"].as_array().is_some_and(|cs| {
                    cs.iter()
                        .any(|c| c.as_str().is_some_and(|s| failed.contains(s)))
                })
        })
        .filter_map(|t| t["id"].as_str())
        .collect();
    if selected.is_empty() {
        return Err("ULTRAPLAN_FIX_NO_TASKS: 失败检查没有对应制作任务".into());
    }
    let repair_ids: Vec<String> = if repair_round.is_some() {
        let mut ids = Vec::new();
        for id in &selected {
            let key = format!("{prefix}{id}");
            let task = existing.iter().find(|task| task.plan_todo_id.as_deref() == Some(key.as_str()))
                .ok_or_else(|| format!("UP_REPAIR_ORIGINAL_MISSING: 缺少原批准任务{id}，保留产物先恢复交接；禁止新建第二条修复链"))?;
            ids.push(task.id.clone());
        }
        ids.sort();
        ids
    } else { Vec::new() };
    if migrate_repair_delegations {
        crate::plan::authorize_production_retry(dir, &repair_ids, super::MAX_FIX_ROUNDS)?;
    }
    // Topological order makes the additional serial-engine edge safe even when
    // the model emitted tasks in reverse order. Use todo IDs, never shared titles.
    let ordered = ordered_tasks(tasks)?;
    let mut by_task: HashMap<String, String> = HashMap::new();
    let mut previous_engine: Option<String> = None;
    let mut active_ids = Vec::new();
    for task in ordered {
        let id = task["id"].as_str().ok_or_else(|| failure("任务id缺失"))?;
        let key = format!("{prefix}{id}");
        let prepare_repair = repair_round.is_some() && selected.contains(id) && migrate_repair_delegations;
        let engine = matches!(
            task["role"].as_str(),
            Some("scene-builder" | "logic-programmer" | "material-smith")
        );
        let approved_prompt = format!("{}\n\n需求来源: {}/{MANIFEST}；批准计划: {}。按本任务契约及上游交接定向读取，接续已读上下文，不重复遍历无关文件。最终集中检查id: {}。当前目标后端: {}。本任务必需工具: {}；产物: {}；宿主交接检查: {}。交付后由宿主检查输出并放行依赖；阻塞须提交结构化原因与恢复条件，不以会话正常结束表示任务完成。全局QA/reviewer与人工验收在制作交接完成后集中执行。",
            task["prompt"].as_str().unwrap(), up.dir, up.plan_path.as_deref().unwrap_or_default(), task["checkIds"], read_json(&dir.join(TARGET)).unwrap_or(Value::Null), task["requiredTools"], task["outputs"], task["handoffChecks"]);
        let repair_description = repair_round.map(|round| {
            let feedback: Vec<_> = repair_record
                .and_then(|record| record["results"].as_array())
                .into_iter()
                .flatten()
                .filter(|result| {
                    result["id"].as_str().is_some_and(|check| failed.contains(check))
                        && task["checkIds"].as_array().is_some_and(|checks| checks.contains(&result["id"]))
                })
                .collect();
            format!("待主 agent 阅读后编写修复委派；本段只供参考，不可作为子 agent 提示词直接派发。\n\n原批准任务：\n{approved_prompt}\n\n第 {round} 轮本任务的失败反馈：{}\n读取 {}/{ACCEPTANCE_FILE} 全部反馈，只修复本任务关联的失败检查，保留已通过功能，不重做批准的全部范围。主 agent 必须用 todo_update {{id,prompt}} 保存具体目标、读写范围、修复方法、产物和验收步骤后才允许执行。",
                json!(feedback), up.dir)
        });
        if let Some(old) = existing
            .iter()
            .find(|t| t.plan_todo_id.as_deref() == Some(&key))
        {
            if old.status != "completed" || prepare_repair {
                let description = if prepare_repair {
                    let mut context = repair_description.clone().unwrap_or_default();
                    if let Some(previous) =
                        old.description.as_deref().filter(|s| !s.trim().is_empty())
                    {
                        context.push_str(&format!("\n\n旧任务说明（保留参考）：\n{previous}"));
                    }
                    if let Some(previous) = old.prompt.as_deref().filter(|s| !s.trim().is_empty()) {
                        context.push_str(&format!(
                            "\n\n旧修复提示词（须由主 agent 重新审阅和编写）：\n{previous}"
                        ));
                    }
                    for legacy in existing.iter().filter(|legacy| legacy.plan_todo_id.as_deref().is_some_and(|key| key.starts_with(&format!("{prefix}fix:")) && key.ends_with(&format!(":{id}")))) {
                        context.push_str(&format!("\n\n历史修复记录（只保留证据，不再派发）：\n{}", serde_json::to_string(legacy).map_err(failure)?));
                    }
                    Some(context)
                } else {
                    None
                };
                let mut patched = state
                    .todos
                    .patch(
                        &old.id,
                        &PatchTodoRequest {
                            // Resume does not authorize replay. The scheduler
                            // reconciles interrupted work from host receipts.
                            status: prepare_repair.then(|| "queued".into()),
                            description,
                            ..Default::default()
                        },
                    )
                    .map_err(|e| failure(format!("{e:?}")))?;
                if prepare_repair {
                    patched = state
                        .todos
                        .clear_delegation(&old.id)
                        .map_err(|e| failure(format!("{e:?}")))?;
                }
                emit(
                    state,
                    sid,
                    "todo.updated",
                    serde_json::to_value(patched).map_err(failure)?,
                );
            }
            by_task.insert(id.into(), old.id.clone());
            active_ids.push(old.id.clone());
            if engine {
                previous_engine = Some(old.id.clone());
            }
            continue;
        }
        let mut deps: Vec<String> = task["deps"]
            .as_array()
            .ok_or_else(|| failure("deps缺失"))?
            .iter()
            .filter_map(|d| by_task.get(d.as_str().unwrap_or_default()).cloned())
            .collect();
        if engine {
            if let Some(previous) = previous_engine.as_ref() {
                if !deps.contains(previous) {
                    deps.push(previous.clone());
                }
            }
        }
        if repair_round.is_some() { return Err(format!("UP_REPAIR_ORIGINAL_MISSING: 缺少原批准任务{id}")); }
        let title = task["title"].as_str().unwrap().to_string();
        let todo = state
            .todos
            .create(
                sid,
                NewTodo {
                    title,
                    description: repair_description,
                    role: task["role"].as_str().map(str::to_string),
                    prompt: repair_round.is_none().then_some(approved_prompt),
                    stage: task["stage"].as_str().map(str::to_string),
                    deps,
                    verify: Some("qa".into()),
                    plan_todo_id: Some(key),
                    source: Some(source.clone()),
                    ..Default::default()
                },
            )
            .map_err(|e| failure(format!("{e:?}")))?;
        by_task.insert(id.into(), todo.id.clone());
        active_ids.push(todo.id.clone());
        if engine {
            previous_engine = Some(todo.id.clone());
        }
        emit(
            state,
            sid,
            "todo.created",
            serde_json::to_value(todo).map_err(failure)?,
        );
    }
    let same_round =
        same_plan && old.as_ref().and_then(|p| p["repairRound"].as_u64()) == repair_round;
    let mut report = if same_round {
        old.clone().unwrap()
    } else {
        let mut history = old
            .as_ref()
            .and_then(|p| p["history"].as_array())
            .cloned()
            .unwrap_or_default();
        if let Some(mut previous) = old {
            if let Some(map) = previous.as_object_mut() {
                map.remove("history");
            }
            history.push(previous);
        }
        json!({"flowId":up.id,"planRev":up.plan_rev,"planHash":up.plan_hash,"planPath":up.plan_path,"repairRound":repair_round,"checks":{},"history":history})
    };
    report["phase"] = json!(phase.as_str());
    report["status"] = json!("running");
    report["activeTodoIds"] = json!(active_ids);
    if repair_round.is_some() {
        report["repairDelegationVersion"] = json!(2);
        report["repairTodoIds"] = json!(repair_ids);
    }
    save(dir, PRODUCTION_FILE, &report)?;
    crate::plan::advance_production_plan(dir)?;
    Ok(())
}

fn ordered_tasks(tasks: &[Value]) -> Result<Vec<&Value>, String> {
    let mut result = Vec::new();
    let mut done = HashSet::new();
    while result.len() < tasks.len() {
        let before = result.len();
        for t in tasks {
            let id = text_field(t, "id")?;
            if !done.contains(id)
                && t["deps"]
                    .as_array()
                    .ok_or("deps缺失")?
                    .iter()
                    .all(|d| d.as_str().is_some_and(|s| done.contains(s)))
            {
                done.insert(id);
                result.push(t);
            }
        }
        if result.len() == before {
            return Err(failure("任务依赖不存在或有环"));
        }
    }
    Ok(result)
}

/// Demo revisions are independent directories; a failed attempt cannot destroy
/// the last playable build. The builder sees full text copies in _requirements.
pub fn prepare_demo(rt: &UltraRuntime) -> Result<(PathBuf, String), String> {
    let demo_prompt = rt
        .tracker
        .demo_prompt
        .lock()
        .unwrap()
        .clone()
        .filter(|prompt| !prompt.trim().is_empty())
        .ok_or("ULTRAPLAN_DEMO_DELEGATION_MISSING: 主 agent 尚未提交本轮 demoPrompt，请阅读需求后编写详细委派")?;
    if !rt.dir_abs.join(SPEC_FILE).is_file() {
        return Err("ULTRAPLAN_SPEC_MISSING: 未提交完整需求".into());
    }
    let work = rt.dir_abs.join(format!("demo-attempt-{}", new_id("d")));
    std::fs::create_dir_all(&work).map_err(failure)?;
    let mut manifest = Vec::new();
    for (source_index, file) in requirement_files(&rt.dir_abs).into_iter().enumerate() {
        let rel = file.strip_prefix(&rt.dir_abs).map_err(failure)?;
        let bytes = std::fs::read(&file).map_err(failure)?;
        let dest = work.join("_requirements").join(rel);
        if let Some(parent) = dest.parent() {
            std::fs::create_dir_all(parent).map_err(failure)?;
        }
        std::fs::write(&dest, &bytes).map_err(failure)?;
        let source = format!("_requirements/{}", rel.to_string_lossy().replace('\\', "/"));
        let text = String::from_utf8(bytes).map_err(failure)?;
        for (index, chunk) in requirement_chunks(&text).into_iter().enumerate() {
            let chunk_rel = format!("_requirements/chunks/{source_index:04}-{index:04}.txt");
            let chunk_path = work.join(&chunk_rel);
            std::fs::create_dir_all(chunk_path.parent().unwrap()).map_err(failure)?;
            std::fs::write(&chunk_path, chunk.as_bytes()).map_err(failure)?;
            manifest.push(json!({"path":chunk_rel,"source":source,"index":index,"sha256":sha256_hex(chunk.as_bytes())}));
        }
    }
    let package = read_json(&rt.dir_abs.join(MANIFEST)).ok_or("需求包版本缺失")?;
    save(
        &work,
        "requirements.json",
        &json!({"schema":1,"flowId":rt.flow_id,
        "version":package["version"],"questionnaireRev":package["questionnaireRev"],
        "demoIteration":package["demoIteration"],"planRev":package["planRev"],"files":manifest}),
    )?;
    // The manifest itself may exceed the tool feedback limit. Include every
    // short chunk path in the delegation so its tail cannot become invisible.
    let read_list = manifest
        .iter()
        .filter_map(|entry| entry["path"].as_str())
        .collect::<Vec<_>>()
        .join("\n");
    let prompt = format!("{demo_prompt}\n\n协调器强制执行约束：\n开工前全文读取 requirements.json 及其中每个需求文件，长文件逐段读完。工具结果超过 4000 字符会被截断，优先逐个 read_file 读取以下短分段；完整读取对应原文也会计入，分页读取会累计，输出截断不代表读完。若写入或探测返回 ULTRAPLAN_REQUIREMENTS_INCOMPLETE，先补读错误列出的分段再重试操作。\n需求分段完整清单：\n{read_list}\n当前目录是本轮隔离 Demo 目录，只能写当前目录；不得修改 _requirements 或 requirements.json，不得访问外部网络。入口必须为 index.html。输出 probe.json，包含真实键鼠 inputs 和基于 window.__demo.getState() 的 assertions；暴露 reset/getState，供浏览器实际验证操作和状态变化。完成后说明操作方法、目标和 Demo 范围，并报告验证结果。\n");
    Ok((work, prompt))
}
fn requirement_chunks(text: &str) -> Vec<String> {
    let mut chunks = Vec::new();
    let mut chunk = String::new();
    let mut count = 0;
    for c in text.chars() {
        chunk.push(c);
        count += 1;
        if count == 3000 {
            chunks.push(std::mem::take(&mut chunk));
            count = 0;
        }
    }
    if !chunk.is_empty() || chunks.is_empty() {
        chunks.push(chunk);
    }
    chunks
}
pub async fn complete_demo(
    state: &AppState,
    sid: &str,
    rid: &str,
    rt: &UltraRuntime,
    work: &Path,
    ok: bool,
    summary: &str,
) -> Result<(), String> {
    if !ok {
        return Err(format!("ULTRAPLAN_DEMO_BUILD_FAILED: {summary}"));
    }
    if !work.join("index.html").is_file() {
        return Err("ULTRAPLAN_DEMO_MISSING: 子代理未生成index.html".into());
    }
    let up = get_flow(state, sid, rt)?;
    let registry = crate::demo_host::global();
    validate_demo_dir(&rt.dir_abs, work)?;
    let probe_token = format!("{}-attempt", up.token);
    let probe = crate::web_probe::probe(
        &flow_ws_root(state, &up),
        work,
        &probe_token,
        &rt.dir_abs
            .join(PROBE_DIR)
            .join(format!("{}", up.demo_iteration + 1)),
        &json!({}),
    )
    .await;
    registry.unregister(&probe_token);
    if probe["ok"] != true {
        return Err(format!(
            "ULTRAPLAN_DEMO_BUILD_FAILED: 浏览器探测未通过: {probe}"
        ));
    }
    registry.ensure_started().await.map_err(failure)?;
    write_atomic(&rt.dir_abs.join("demo-notes.md"), summary).map_err(failure)?;
    let (_, committed) = state
        .sessions
        .update_ultraplan(sid, |slot| -> Result<UltraPlanState, String> {
            let current = slot
                .as_mut()
                .filter(|u| {
                    u.id == up.id
                        && u.phase == PHASE_RUNNING
                        && u.demo_iteration == up.demo_iteration
                })
                .ok_or_else(|| failure("Demo构建轮次已过期"))?;
            replace_demo(&rt.dir_abs, work, current.demo_iteration)?;
            current.demo_iteration += 1;
            current.demo_verified = true;
            current.demo_note = Some(summary.into());
            current.stage = STAGE_DEMO_REVIEW.into();
            current.plan_hash = None;
            Ok(current.clone())
        })
        .ok_or_else(|| failure("流程已消失"))?;
    let up = committed?;
    save(
        &rt.dir_abs,
        "demo-digest.json",
        &json!({"iteration":up.demo_iteration,"sha256":demo_digest(&rt.dir_abs.join(DEMO_DIR))?}),
    )?;
    registry.register(&up.token, &rt.dir_abs.join(DEMO_DIR));
    emit(state, sid, "ultraplan.stage", rt.stage_event(&up, rid));
    emit_session_updated(state, sid);
    emit(
        state,
        sid,
        "ultraplan.demo.ready",
        json!({"id":up.id,"runId":rid,"iteration":up.demo_iteration,"verified":true,"note":summary,"entry":"index.html","probe":probe}),
    );
    Ok(())
}

fn validate_demo_dir(dir: &Path, candidate: &Path) -> Result<(), String> {
    let root = dir.canonicalize().map_err(failure)?;
    no_link(candidate)?;
    let candidate = candidate.canonicalize().map_err(failure)?;
    if candidate.parent() != Some(root.as_path()) || !candidate.is_dir() {
        return Err(failure("Demo必须为当前流程内的独立目录"));
    }
    fn walk(path: &Path, depth: usize) -> Result<(), String> {
        if depth > 64 {
            return Err(failure("Demo目录层次过深"));
        }
        no_link(path)?;
        for entry in std::fs::read_dir(path).map_err(failure)? {
            let path = entry.map_err(failure)?.path();
            no_link(&path)?;
            if path.is_dir() {
                walk(&path, depth + 1)?;
            } else if !path.is_file() {
                return Err(failure("Demo包含非常规文件"));
            }
        }
        Ok(())
    }
    walk(&candidate, 0)?;
    if !candidate.join("index.html").is_file() {
        return Err("ULTRAPLAN_DEMO_MISSING: Demo入口不存在".into());
    }
    Ok(())
}

fn demo_digest(root: &Path) -> Result<String, String> {
    fn collect(root: &Path, dir: &Path, out: &mut Vec<PathBuf>) -> Result<(), String> {
        no_link(dir)?;
        for entry in std::fs::read_dir(dir).map_err(failure)? {
            let path = entry.map_err(failure)?.path();
            no_link(&path)?;
            if path.file_name().is_some_and(|name| name == "_probe") {
                continue;
            }
            if path.is_dir() {
                collect(root, &path, out)?;
            } else if path.is_file() {
                out.push(path.strip_prefix(root).map_err(failure)?.to_path_buf());
            }
        }
        Ok(())
    }
    let mut files = Vec::new();
    collect(root, root, &mut files)?;
    files.sort();
    let mut data = Vec::new();
    for path in files {
        let bytes = std::fs::read(root.join(&path)).map_err(failure)?;
        data.extend_from_slice(path.to_string_lossy().as_bytes());
        data.push(0);
        data.extend_from_slice(sha256_hex(&bytes).as_bytes());
        data.push(0);
    }
    Ok(sha256_hex(&data))
}

fn replace_demo(dir: &Path, candidate: &Path, iteration: u32) -> Result<(), String> {
    validate_demo_dir(dir, candidate)?;
    let current = dir.join(DEMO_DIR);
    let snapshot = dir.join(format!("demo-revision-{iteration}"));
    let had_current = current.exists();
    if had_current {
        validate_demo_dir(dir, &current)?;
        if snapshot.exists() {
            return Err(failure("同版本Demo快照已存在，拒绝覆盖"));
        }
        std::fs::rename(&current, &snapshot).map_err(failure)?;
    }
    if let Err(error) = std::fs::rename(candidate, &current) {
        if had_current {
            std::fs::rename(&snapshot, &current).map_err(|restore| {
                failure(format!(
                    "Demo安装失败{error}；恢复原Demo失败{restore}，原版本保留于{}",
                    snapshot.display()
                ))
            })?;
        }
        return Err(failure(error));
    }
    Ok(())
}

fn copy_demo(source: &Path, dest: &Path) -> Result<(), String> {
    no_link(source)?;
    std::fs::create_dir(dest).map_err(failure)?;
    for entry in std::fs::read_dir(source).map_err(failure)? {
        let entry = entry.map_err(failure)?;
        let path = entry.path();
        no_link(&path)?;
        let target = dest.join(entry.file_name());
        if path.is_dir() {
            copy_demo(&path, &target)?;
        } else if path.is_file() {
            std::fs::copy(&path, &target).map_err(failure)?;
        } else {
            return Err(failure("Demo包含非常规文件"));
        }
    }
    Ok(())
}
pub(super) fn demo_face(up: Option<&UltraPlanState>, dir: Option<&Path>) -> Value {
    match (up, dir, crate::demo_host::global().port()) {
        (Some(up), Some(dir), Some(port))
            if up.demo_iteration > 0 && dir.join(DEMO_DIR).join("index.html").is_file() =>
        {
            json!({"port":port,"token":up.token,"entry":"index.html"})
        }
        _ => Value::Null,
    }
}

pub fn verification_tool_spec() -> Value {
    tool(VERIFY_TOOL,"执行批准的单项自动检查。id必须是当前checks.automated的检查id（不是flowId/todoId）；matrix必须包含scene和cases。返回逐case真实结果，禁止自报pass。",json!({"id":{"type":"string","description":"当前批准checks.automated中的唯一id"},"matrix":super::verify::matrix_schema()}), &["id","matrix"])
}

pub fn record_check(dir: &Path, id: &str, result: Value) -> Result<(), String> {
    let mut production =
        read_json(&dir.join(PRODUCTION_FILE)).ok_or_else(|| failure("没有制作记录"))?;
    let previous = production["checks"][id].clone();
    if previous.is_object() && previous != result {
        if !production["checkHistory"].is_object() {
            production["checkHistory"] = json!({});
        }
        if !production["checkHistory"][id].is_array() {
            production["checkHistory"][id] = json!([]);
        }
        production["checkHistory"][id]
            .as_array_mut()
            .unwrap()
            .push(previous);
    }
    production["checks"][id] = result;
    save(dir, PRODUCTION_FILE, &production)
}
pub fn record_review(dir: &Path, mut result: Value) -> Result<(), String> {
    let mut production =
        read_json(&dir.join(PRODUCTION_FILE)).ok_or_else(|| failure("没有制作记录"))?;
    if !result["ok"].is_boolean()
        || result["flowId"] != production["flowId"]
        || result["planRev"] != production["planRev"]
        || result["planHash"] != production["planHash"]
    {
        return Err(failure("审阅结果未批准当前版本"));
    }
    let ws = dir
        .ancestors()
        .nth(3)
        .ok_or_else(|| failure("流程目录非法"))?;
    let target = read_json(&dir.join(TARGET)).ok_or_else(|| failure("缺少制作目标"))?;
    let relative = target["projectRoot"].as_str().unwrap_or(".");
    if relative != "." {
        safe_relative(relative)?;
    }
    result["projectFingerprint"] = json!(super::evidence::project_fingerprint(&ws.join(relative))?);
    save(dir, "reviewer.json", &result)?;
    production["reviewer"] = result;
    save(dir, PRODUCTION_FILE, &production)
}
pub fn validation_summary(rt: &UltraRuntime) -> Result<Value, String> {
    let ws = rt
        .dir_abs
        .ancestors()
        .nth(3)
        .ok_or_else(|| failure("流程目录非法"))?;
    validated_evidence(&rt.flow_id, ws, &rt.dir_abs)
}

fn evidence_issues(
    checks: &Value,
    production: &Value,
    target: &Value,
    flow_id: &str,
    ws: &Path,
    project_relative: &str,
    fingerprint: &str,
) -> Vec<Value> {
    checks["automated"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|check| {
            let id = check["id"].as_str().unwrap_or_default();
            let report = &production["checks"][id];
            let result = (|| -> Result<(), (&str, String)> {
                if report.is_null() {
                    if let Some(attempt) =
                        production["verificationErrors"]
                            .as_array()
                            .and_then(|attempts| {
                                attempts.iter().rev().find(|attempt| attempt["id"] == id)
                            })
                    {
                        let status = if attempt["failureKind"] == "blocked" {
                            "blocked"
                        } else {
                            "invalid_request"
                        };
                        return Err((
                            status,
                            attempt["error"].as_str().unwrap_or("验证请求未执行").into(),
                        ));
                    }
                    return Err(("missing", "尚未执行自动检查".into()));
                }
                if report["projectRoot"] != project_relative
                    || report["projectFingerprint"] != fingerprint
                {
                    return Err((
                        "stale",
                        "报告对应的项目内容已变化，须对当前产物重新验证".into(),
                    ));
                }
                if report["id"] != id
                    || report["flowId"] != flow_id
                    || report["kind"] != check["kind"]
                    || report["approvedCheck"] != *check
                {
                    return Err(("stale", "报告与当前批准检查不匹配".into()));
                }
                if report["ok"] != true {
                    let category = match report["failureKind"].as_str() {
                        Some("stale") => "stale",
                        Some("invalid_request") => "invalid_request",
                        Some("execution" | "blocked") => "blocked",
                        _ => "failed",
                    };
                    let reason = report["error"].as_str().unwrap_or("断言未通过");
                    return Err((category, reason.into()));
                }
                if report["backendInfo"]["renderBackend"] != target["renderBackend"] {
                    return Err(("stale", "验证后端与批准目标不一致".into()));
                }
                let path = text_field(report, "reportPath").map_err(|e| ("missing", e))?;
                let report_path = evidence_file(ws, path).map_err(|e| ("missing", e))?;
                if read_json(&report_path).as_ref() != Some(report) {
                    return Err(("stale", "落盘报告与验证记录不匹配".into()));
                }
                let shots = report["screenshots"]
                    .as_array()
                    .filter(|s| !s.is_empty())
                    .ok_or(("missing", "没有截图证据".to_string()))?;
                for shot in shots {
                    let path = text_field(shot, "path").map_err(|e| ("missing", e))?;
                    let file = evidence_file(ws, path).map_err(|e| ("missing", e))?;
                    let bytes = std::fs::read(file).map_err(|e| ("missing", e.to_string()))?;
                    if shot["sha256"] != sha256_hex(&bytes) {
                        return Err(("stale", "截图证据已修改".into()));
                    }
                }
                Ok(())
            })();
            result.err().map(|(status, reason)| {
                json!({"id":id,"status":status,"reason":reason,
            "cases":report["matrix"]["cases"],"reportPath":report["reportPath"]})
            })
        })
        .collect()
}

/// Diagnostics for targeted recovery: missing/stale evidence asks for another
/// verification, while only actual failed checks justify implementation repair.
pub fn verification_issues(rt: &UltraRuntime) -> Result<Vec<Value>, String> {
    let ws = rt.dir_abs.ancestors().nth(3).ok_or("流程目录非法")?;
    let checks = read_json(&rt.dir_abs.join(CHECKS_FILE)).ok_or("缺少检查清单")?;
    let production = read_json(&rt.dir_abs.join(PRODUCTION_FILE)).ok_or("缺少制作记录")?;
    let target = read_json(&rt.dir_abs.join(TARGET)).ok_or("缺少制作目标")?;
    let relative = target["projectRoot"].as_str().unwrap_or(".");
    if relative != "." {
        safe_relative(relative)?;
    }
    let workspace = ws.canonicalize().map_err(failure)?;
    let project = workspace.join(relative).canonicalize().map_err(failure)?;
    if !project.starts_with(&workspace) {
        return Err(failure("制作项目不属于当前工作区"));
    }
    let fingerprint = super::evidence::project_fingerprint(&project)?;
    Ok(evidence_issues(
        &checks,
        &production,
        &target,
        &rt.flow_id,
        ws,
        relative,
        &fingerprint,
    ))
}

fn validated_evidence(flow_id: &str, ws: &Path, dir: &Path) -> Result<Value, String> {
    let checks = read_json(&dir.join(CHECKS_FILE)).ok_or_else(|| failure("缺少检查清单"))?;
    let production =
        read_json(&dir.join(PRODUCTION_FILE)).ok_or_else(|| failure("缺少制作记录"))?;
    let target = read_json(&dir.join(TARGET)).ok_or_else(|| failure("缺少制作目标"))?;
    let project_relative = target["projectRoot"].as_str().unwrap_or(".");
    if project_relative != "." {
        safe_relative(project_relative)?;
    }
    let workspace = ws.canonicalize().map_err(failure)?;
    let project = workspace
        .join(project_relative)
        .canonicalize()
        .map_err(failure)?;
    if !project.starts_with(&workspace) {
        return Err(failure("制作项目不属于当前工作区"));
    }
    let fingerprint = super::evidence::project_fingerprint(&project)?;
    let path = text_field(&production, "planPath")?;
    if production["flowId"] != flow_id || production["planHash"] != plan_hash(ws, path, dir)? {
        return Err("ULTRAPLAN_PLAN_CHANGED: 验证记录对应的计划已变化".into());
    }
    checks["automated"]
        .as_array()
        .filter(|items| !items.is_empty())
        .ok_or_else(|| failure("自动检查缺失"))?;
    let failed = evidence_issues(
        &checks,
        &production,
        &target,
        flow_id,
        ws,
        project_relative,
        &fingerprint,
    );
    if !failed.is_empty() {
        return Err(format!(
            "ULTRAPLAN_PRODUCTION_INCOMPLETE: {}",
            json!({"checks":failed,"recovery":"missing/stale补验证；blocked先解除能力阻塞；failed仅定向修复关联既有任务"})
        ));
    }
    let delivery = read_json(&dir.join(DELIVERY)).ok_or_else(|| failure("缺少游戏交付入口"))?;
    evidence_file(ws, text_field(&delivery, "entry")?)?;
    text_field(&delivery, "controls")?;
    Ok(production)
}
fn validated_production(up: &UltraPlanState, ws: &Path, dir: &Path) -> Result<Value, String> {
    verify_plan(up, ws, dir)?;
    let production = validated_evidence(&up.id, ws, dir)?;
    if production["planRev"] != up.plan_rev || production["planHash"] != json!(up.plan_hash) {
        return Err("ULTRAPLAN_PLAN_CHANGED: 制作版本已变化".into());
    }
    let review = &production["reviewer"];
    let reviewed_current = production["checks"].as_object().is_some_and(|checks| {
        !checks.is_empty()
            && checks
                .values()
                .all(|report| report["projectFingerprint"] == review["projectFingerprint"])
    });
    if review["ok"] != true
        || !reviewed_current
        || review["flowId"] != up.id
        || review["planRev"] != up.plan_rev
        || review["planHash"] != json!(up.plan_hash)
        || read_json(&dir.join("reviewer.json")).as_ref() != Some(review)
    {
        return Err("ULTRAPLAN_PRODUCTION_INCOMPLETE: 缺少当前版本的审阅批准证据".into());
    }
    Ok(production)
}
pub fn complete_production(
    state: &AppState,
    sid: &str,
    rid: &str,
    rt: &UltraRuntime,
    summary: &str,
) -> Result<(), String> {
    let up = get_flow(state, sid, rt)?;
    if up.stage != STAGE_PRODUCTION
        || up.production_run_id.as_deref() != Some(rid)
        || state
            .sessions
            .get(sid)
            .and_then(|s| s.active_run_id)
            .as_deref()
            != Some(rid)
    {
        return Err("ULTRAPLAN_STAGE_MISMATCH: 制作轮次已结束或变化".into());
    }
    let mut report = validated_production(&up, &flow_ws_root(state, &up), &rt.dir_abs)?;
    let active: HashSet<_> = report["activeTodoIds"]
        .as_array()
        .ok_or_else(|| failure("缺少本轮任务清单"))?
        .iter()
        .filter_map(Value::as_str)
        .collect();
    let todos: Vec<_> = state
        .todos
        .list_by_session(sid)
        .into_iter()
        .filter(|t| {
            t.source == format!("ultraplan:{}:{}", up.id, up.plan_rev)
                && active.contains(t.id.as_str())
        })
        .collect();
    if todos.is_empty()
        || todos.len() != active.len()
        || todos.iter().any(|t| t.status != "completed")
    {
        return Err("ULTRAPLAN_PRODUCTION_INCOMPLETE: 制作任务没有全部完成".into());
    }
    report["status"] = json!("awaiting_acceptance");
    report["summary"] = json!(summary);
    save(&rt.dir_abs, PRODUCTION_FILE, &report)?;
    let up = advance(state, sid, rid, rt, |u| {
        u.stage = STAGE_ACCEPTANCE.into();
        u.acceptance_round += 1;
    })?;
    let checks = read_json(&rt.dir_abs.join(CHECKS_FILE)).unwrap();
    emit(
        state,
        sid,
        "ultraplan.acceptance.ready",
        json!({"id":up.id,"runId":rid,"round":up.acceptance_round,"manual":checks["manual"]}),
    );
    Ok(())
}

pub(super) fn rest_action(
    state: &AppState,
    sid: &str,
    action: &str,
    body: Option<&Value>,
) -> Response {
    let Some(up) = state.sessions.get(sid).and_then(|s| s.ultraplan) else {
        return err_response(
            StatusCode::CONFLICT,
            "ULTRAPLAN_STAGE_MISMATCH",
            "没有流程",
            None,
        );
    };
    let ws = flow_ws_root(state, &up);
    let Some(dir) = up.dir_abs(&ws) else {
        return err_response(StatusCode::CONFLICT, ERR_TURN_INVALID, "流程目录非法", None);
    };
    let result = (|| -> Result<Value, String> {
        let supplied = body.ok_or("缺少流程版本")?;
        let expected = if action == "rollback_demo" {
            up.demo_iteration
        } else {
            up.plan_rev
        };
        if supplied["id"] != up.id || supplied["rev"].as_u64() != Some(expected as u64) {
            return Err("ULTRAPLAN_STAGE_MISMATCH: 流程或版本已变化，请刷新后重试".into());
        }
        if action == "rollback_demo" {
            if up.stage != STAGE_DEMO_REVIEW {
                return Err("只能在试玩阶段回退Demo".into());
            }
            let (_, committed) = state
                .sessions
                .update_ultraplan_idle(sid, |slot| -> Result<UltraPlanState, String> {
                    let current = slot
                        .as_mut()
                        .filter(|u| {
                            u.id == up.id
                                && u.stage == STAGE_DEMO_REVIEW
                                && u.phase != PHASE_RUNNING
                                && u.demo_iteration == up.demo_iteration
                        })
                        .ok_or("Demo版本已变化")?;
                    let previous = (1..up.demo_iteration)
                        .rev()
                        .map(|n| dir.join(format!("demo-revision-{n}")))
                        .find(|p| p.join("index.html").is_file())
                        .ok_or("ULTRAPLAN_NO_DEMO_SNAPSHOT: 没有Demo快照")?;
                    validate_demo_dir(&dir, &previous)?;
                    let candidate = dir.join(format!("demo-rollback-{}", new_id("d")));
                    copy_demo(&previous, &candidate)?;
                    replace_demo(&dir, &candidate, current.demo_iteration)?;
                    current.demo_iteration += 1;
                    current.demo_verified = false;
                    current.plan_hash = None;
                    current.demo_note = Some("已恢复上一份Demo，请重新试玩".into());
                    Ok(current.clone())
                })
                .map_err(|e| format!("{e:?}"))?;
            let updated = committed?;
            save(
                &dir,
                "demo-digest.json",
                &json!({"iteration":updated.demo_iteration,"sha256":demo_digest(&dir.join(DEMO_DIR))?}),
            )?;
            emit(
                state,
                sid,
                "ultraplan.demo.ready",
                json!({"id":updated.id,"iteration":updated.demo_iteration,"verified":false,"note":updated.demo_note,"entry":"index.html"}),
            );
            emit(state, sid, "ultraplan.stage", stage_payload(&updated, None));
            emit_session_updated(state, sid);
            return Ok(json!({"ultraplan":updated}));
        }
        if up.stage != STAGE_ACCEPTANCE {
            return Err("当前不在人工验收阶段".into());
        }
        let body = body.ok_or("缺少验收结果")?;
        if body["round"].as_u64() != Some(up.acceptance_round as u64) {
            return Err("验收轮次已过期".into());
        }
        let checks = read_json(&dir.join(CHECKS_FILE)).ok_or("检查清单缺失")?;
        let manual = checks["manual"].as_array().ok_or("人工检查缺失")?;
        let results = body["results"].as_array().ok_or("results须为数组")?;
        if manual.len() != results.len() {
            return Err("必须逐项提交全部人工检查".into());
        }
        let mut ids = HashSet::new();
        let mut failed = Vec::new();
        for r in results {
            let id = text_field(r, "id")?;
            let status = text_field(r, "status")?;
            let c = manual.iter().find(|c| c["id"] == id).ok_or("未知检查id")?;
            if !ids.insert(id) {
                return Err("检查重复".into());
            }
            match status {
                "pass" => {}
                "fail" => {
                    text_field(r, "note")?;
                    failed.push(id.to_string());
                }
                "skip" => {
                    if c["required"].as_bool().unwrap_or(true) {
                        return Err("必要检查不能跳过".into());
                    }
                    text_field(r, "note")?;
                }
                _ => return Err("检查状态非法".into()),
            }
        }
        // Commit file and state under the same idle session lock: a competing
        // start/restart cannot race between validation and acceptance recording.
        let (_, committed)=state.sessions.update_ultraplan_idle(sid,|slot|->Result<UltraPlanState,String>{
            let current=slot.as_mut().filter(|u|u.id==up.id&&u.stage==STAGE_ACCEPTANCE&&u.acceptance_round==up.acceptance_round).ok_or("验收已过期")?;
            validated_production(current,&ws,&dir)?;
            let mut all=read_json(&dir.join(ACCEPTANCE_FILE)).unwrap_or(json!({"rounds":[]}));
            let rounds=all["rounds"].as_array_mut().ok_or("验收文件损坏")?;
            if rounds.iter().any(|r|r["round"]==up.acceptance_round){return Err("本轮验收已经提交".into());}
            rounds.push(json!({"round":up.acceptance_round,"results":results,"failed":failed,"at":now_rfc3339()}));save(&dir,ACCEPTANCE_FILE,&all)?;
            if failed.is_empty(){current.stage=STAGE_DONE.into();}current.phase=PHASE_WAITING.into();Ok(current.clone())
        }).map_err(|e|format!("{e:?}"))?;
        let updated = committed?;
        emit(
            state,
            sid,
            "ultraplan.acceptance.recorded",
            json!({"id":up.id,"round":up.acceptance_round,"results":results,"failed":failed}),
        );
        emit(state, sid, "ultraplan.stage", stage_payload(&updated, None));
        emit_session_updated(state, sid);
        if failed.is_empty() {
            emit(
                state,
                sid,
                "ultraplan.done",
                json!({"id":up.id,"planPath":up.plan_path,"dir":up.dir,"delivery":read_json(&dir.join(DELIVERY))}),
            );
        }
        Ok(
            json!({"ultraplan":updated,"next":if failed.is_empty(){"done"}else{"fix"},"failed":failed}),
        )
    })();
    match result {
        Ok(v) => Json(v).into_response(),
        Err(e) => err_response(
            StatusCode::CONFLICT,
            if action == "acceptance" {
                "ULTRAPLAN_ACCEPTANCE_INVALID"
            } else {
                "ULTRAPLAN_NO_DEMO_SNAPSHOT"
            },
            e,
            None,
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plan() -> Value {
        json!({"tasks":[
            {"id":"logic","title":"Gameplay","role":"logic-programmer","prompt":"Implement loop","stage":"build","deps":["scene"],"verify":"qa","checkIds":["g","m"],"requiredTools":["mcp__code-forge__rx_check"],"outputs":["Content/Scripts/game.rx"],"handoffChecks":[{"kind":"rx","path":"Content/Scripts/game.rx"}]},
            {"id":"scene","title":"Scene","role":"scene-builder","prompt":"Build scene","stage":"build","deps":[],"verify":"qa","checkIds":["v"],"requiredTools":["mcp__engine-scene__scene_save"],"outputs":["Content/Scenes/game.rxscene"],"handoffChecks":[{"kind":"scene","path":"Content/Scenes/game.rxscene"}]}],
            "checks":{"automated":[{"id":"v","kind":"visual","scene":"Content/Scenes/main.rxscene","steps":"Capture","expected":"Visible"},{"id":"g","kind":"gameplay","scene":"Content/Scenes/main.rxscene","steps":"Move","expected":"Moved"}],"manual":[{"id":"m","title":"Feel","steps":"Play","expected":"Responsive","required":true}]},
            "delivery":{"entry":"Content/Scenes/main.rxscene","controls":"Arrow keys; R to restart"}})
    }

    fn fixture(tag: &str) -> (Arc<AppState>, PathBuf, PathBuf, String, UltraPlanState) {
        let (state, base) = crate::test_app_state(tag);
        let ws = base.join("workspace");
        std::fs::create_dir_all(&ws).unwrap();
        crate::project::init_project(&ws, "game", "3d").unwrap();
        let workspace = state
            .workspaces
            .create("game", ws.to_str().unwrap())
            .unwrap();
        let session = state.sessions.create(
            "UltraPlan",
            "coding",
            None,
            false,
            Some(workspace.id.clone()),
        );
        let mut up = UltraPlanState::new_flow("game", Some(&workspace.id), &ws);
        up.plan_rev = 1;
        up.stage = STAGE_PLAN_REVIEW.into();
        let dir = up.dir_abs(&ws).unwrap();
        std::fs::create_dir_all(&dir).unwrap();
        let plan = plan();
        save(&dir, TEAM_PLAN_FILE, &plan["tasks"]).unwrap();
        save(&dir, CHECKS_FILE, &plan["checks"]).unwrap();
        save(&dir, DELIVERY, &plan["delivery"]).unwrap();
        save(
            &dir,
            TARGET,
            &json!({"renderBackend":"rurix","gameMode":"3d"}),
        )
        .unwrap();
        let path = up.reserved_plan_path();
        let absolute = crate::plan_doc::abs_path(&ws, &path);
        std::fs::create_dir_all(absolute.parent().unwrap()).unwrap();
        std::fs::write(absolute, "Approved plan").unwrap();
        up.plan_path = Some(path);
        up.plan_hash = Some(plan_hash(&ws, up.plan_path.as_deref().unwrap(), &dir).unwrap());
        state
            .sessions
            .update_ultraplan(&session.id, |slot| *slot = Some(up.clone()))
            .unwrap();
        (state, base, ws, session.id, up)
    }

    #[test]
    fn production_rechecks_runtime_overrides_before_initializing() {
        let target =
            json!({"renderBackend":"godot", "renderMethod":"forward_plus", "renderDriver":"d3d12"});
        assert!(validate_target_overrides(&target, None, None, None).is_ok());
        assert!(validate_target_overrides(&target, Some("godot"), None, None).is_ok());
        assert!(
            validate_target_overrides(&target, Some("rurix"), None, None)
                .unwrap_err()
                .contains("FORGE_RENDER_BACKEND")
        );
        assert!(validate_target_overrides(&target, None, Some("mobile"), None).is_err());
        assert!(validate_target_overrides(&target, None, None, Some("vulkan")).is_err());
    }

    #[test]
    fn confirmed_target_rejects_changes_to_existing_project() {
        let (_state, base, ws, _sid, _up) = fixture("up-confirmed-existing-target");
        let scope = crate::scope::ScopeProject {
            workspace_id: None,
            name: "game".into(),
            workspace_root: ws.clone(),
            project_root: ws.clone(),
            game_mode: assetd::project::GameMode::ThreeD,
        };
        let custom = validate_questionnaire(&json!({
            "title":"Game", "understanding":"A playable game", "sections":[
                {"id":"core", "title":"Core", "questions":[
                    {"id":"goal", "kind":"text", "question":"Goal?"}
                ]}
            ]
        }))
        .unwrap();
        let questionnaire = serde_json::to_value(
            ensure_implementation_stack(custom, &project_facts(&ws, &ws)).unwrap(),
        )
        .unwrap();
        let answers = json!({IMPLEMENTATION_STACK_QUESTION:{"choice":["3d_rurix"]}});
        let target = target_for_answers(&scope, &questionnaire, &answers).unwrap();
        assert_eq!(target["existingProject"], true);
        assert_eq!(target["renderBackend"], "rurix");
        assert_eq!(target["stackConfirmed"], true);
        let mut project = assetd::project::ForgeProject::load(&ws).unwrap();
        project.render =
            assetd::project::RenderConfig::for_mode(project.mode, Some("godot")).unwrap();
        project.save_manifest().unwrap();
        assert!(target_for_answers(&scope, &questionnaire, &answers)
            .unwrap_err()
            .contains("已变化"));
        let resolved = base.canonicalize().unwrap();
        assert_eq!(
            resolved.parent(),
            Some(std::env::temp_dir().canonicalize().unwrap().as_path())
        );
        std::fs::remove_dir_all(resolved).unwrap();
    }

    #[test]
    fn plan_requires_scene_delivery_and_manual_task_ownership() {
        let p = plan();
        validate_tasks(&p).unwrap();
        let mut missing = p.clone();
        missing["checks"]["automated"][0]
            .as_object_mut()
            .unwrap()
            .remove("scene");
        assert!(validate_tasks(&missing).is_err());
        let mut unsafe_entry = p.clone();
        unsafe_entry["delivery"]["entry"] = json!("../other/game.rxscene");
        assert!(validate_tasks(&unsafe_entry).is_err());
        let mut unowned = p;
        unowned["tasks"][0]["checkIds"] = json!(["g"]);
        assert!(validate_tasks(&unowned).unwrap_err().contains("未分配"));
    }

    #[test]
    fn task_contract_requires_covered_outputs_and_executable_matrix_shape() {
        let mut task = plan()["tasks"][1].clone();
        validate_task_contract(&task).unwrap();
        task["outputs"]
            .as_array_mut()
            .unwrap()
            .push(json!("Content/missing.json"));
        assert!(validate_task_contract(&task)
            .unwrap_err()
            .contains("每个 outputs"));
        task["outputs"] = json!(["Content/Scenes/game.rxscene"]);
        task["handoffChecks"] = json!([{"kind":"matrix","path":"Content/Scenes/game.rxscene",
            "matrix":{"scene":"Content/Scenes/game.rxscene","enter_play":false,
                "cases":[{"name":"entity exists","assert":{"kind":"entity_count","op":"ge","expected":1}}]}},
            {"kind":"scene","path":"Content/Scenes/game.rxscene"}]);
        validate_task_contract(&task).unwrap();
        task["handoffChecks"][0]["matrix"]["cases"][0]["assert"]["op"] = json!("gte");
        assert!(validate_task_contract(&task).unwrap_err().contains("op"));
        task["requiredTools"] = json!(["forge__mcp__engine-scene__scene_save"]);
        assert!(validate_task_contract(&task).unwrap_err().contains("原名"));
    }

    #[test]
    fn known_output_types_require_semantic_handoff_checks() {
        for (extension, kind) in [("rx", "rx"), ("rxgraph", "graph"), ("rxscene", "scene")] {
            let path = format!("Content/game.{extension}");
            let mut task = plan()["tasks"][1].clone();
            task["outputs"] = json!([path]);
            task["handoffChecks"] =
                json!([{"kind":"file","path":path},{"kind":"json","path":path}]);
            assert!(validate_task_contract(&task)
                .unwrap_err()
                .contains("不能替代"));
            task["handoffChecks"]
                .as_array_mut()
                .unwrap()
                .push(json!({"kind":kind,"path":path}));
            validate_task_contract(&task).unwrap();
        }
    }

    #[test]
    fn legacy_contract_requires_explicit_upgrade_without_mutating_approved_files() {
        let (state, base, ws, sid, mut up) = fixture("up-legacy-contract");
        let dir = up.dir_abs(&ws).unwrap();
        let mut tasks = read_json(&dir.join(TEAM_PLAN_FILE)).unwrap();
        tasks[0].as_object_mut().unwrap().remove("outputs");
        save(&dir, TEAM_PLAN_FILE, &tasks).unwrap();
        up.plan_hash = Some(plan_hash(&ws, up.plan_path.as_deref().unwrap(), &dir).unwrap());
        let approved_hash = up.plan_hash.clone();
        let bytes = std::fs::read(dir.join(TEAM_PLAN_FILE)).unwrap();
        assert!(materialize(&state, &sid, &up, &dir, ProductionPhase::Start)
            .unwrap_err()
            .starts_with("UP_CONTRACT_UPGRADE_REQUIRED"));
        assert!(state.todos.list_by_session(&sid).is_empty());
        assert!(!dir.join(PRODUCTION_FILE).exists());
        assert_eq!(std::fs::read(dir.join(TEAM_PLAN_FILE)).unwrap(), bytes);
        assert_eq!(up.plan_hash, approved_hash);
        assert_eq!(
            Some(plan_hash(&ws, up.plan_path.as_deref().unwrap(), &dir).unwrap()),
            approved_hash
        );
        std::fs::remove_dir_all(base).unwrap();
    }

    #[test]
    fn resume_preserves_failure_and_blocked_status_until_targeted_retry() {
        let (state, base, ws, sid, up) = fixture("up-resume-preserve-failure");
        let dir = up.dir_abs(&ws).unwrap();
        materialize(&state, &sid, &up, &dir, ProductionPhase::Start).unwrap();
        for status in ["failed", "blocked", "running", "verifying"] {
            for todo in state.todos.list_by_session(&sid) {
                state
                    .todos
                    .patch(
                        &todo.id,
                        &PatchTodoRequest {
                            status: Some(status.into()),
                            ..Default::default()
                        },
                    )
                    .unwrap();
            }
            materialize(&state, &sid, &up, &dir, ProductionPhase::Resume).unwrap();
            assert!(state
                .todos
                .list_by_session(&sid)
                .iter()
                .all(|t| t.status == status));
        }
        std::fs::remove_dir_all(base).unwrap();
    }

    #[test]
    fn evidence_recovery_preserves_failed_cases_and_distinguishes_missing_and_stale() {
        let check = json!({"id":"g","kind":"gameplay","scene":"Content/Scenes/game.rxscene"});
        let checks = json!({"automated":[check.clone()]});
        let report = json!({"id":"g","flowId":"flow","kind":"gameplay","approvedCheck":check,
            "projectRoot":".","projectFingerprint":"current","ok":false,"failureKind":"assertion",
            "error":"player did not move","matrix":{"cases":[{"name":"move right","pass":false,"detail":"x remains 0"}]}});
        let target = json!({"renderBackend":"rurix"});
        let issues = |production: &Value| {
            evidence_issues(
                &checks,
                production,
                &target,
                "flow",
                Path::new("."),
                ".",
                "current",
            )
        };
        assert_eq!(issues(&json!({"checks":{}}))[0]["status"], "missing");
        let mut production = json!({"checks":{"g":report}});
        let failure = issues(&production);
        assert_eq!(failure[0]["status"], "failed");
        assert_eq!(failure[0]["reason"], "player did not move");
        assert_eq!(failure[0]["cases"][0]["detail"], "x remains 0");
        production["checks"]["g"]["projectFingerprint"] = json!("old");
        assert_eq!(issues(&production)[0]["status"], "stale");
        production["checks"]["g"]["projectFingerprint"] = json!("current");
        production["checks"]["g"]["failureKind"] = json!("execution");
        assert_eq!(issues(&production)[0]["status"], "blocked");
    }

    #[test]
    fn replacing_check_keeps_previous_failure_reason_in_history() {
        let (_state, base, ws, _sid, up) = fixture("up-check-history");
        let dir = up.dir_abs(&ws).unwrap();
        save(&dir, PRODUCTION_FILE, &json!({"checks":{}})).unwrap();
        record_check(&dir, "g", json!({"ok":false,"error":"missing movement"})).unwrap();
        record_check(&dir, "g", json!({"ok":true})).unwrap();
        let production = read_json(&dir.join(PRODUCTION_FILE)).unwrap();
        assert_eq!(production["checks"]["g"]["ok"], true);
        assert_eq!(
            production["checkHistory"]["g"][0]["error"],
            "missing movement"
        );
        std::fs::remove_dir_all(base).unwrap();
    }

    #[test]
    fn reverse_order_materializes_safe_ids_and_resume_preserves_evidence() {
        let (state, base, ws, sid, up) = fixture("up-materialize-resume");
        let dir = up.dir_abs(&ws).unwrap();
        // A same-key todo from another flow must never be adopted or changed.
        let other = state
            .todos
            .create(
                &sid,
                NewTodo {
                    title: "Scene".into(),
                    source: Some("other".into()),
                    plan_todo_id: Some(format!("{}:{}:scene", up.id, up.plan_rev)),
                    ..Default::default()
                },
            )
            .unwrap();
        materialize(&state, &sid, &up, &dir, ProductionPhase::Start).unwrap();
        let todos = state.todos.list_by_session(&sid);
        let scene = todos
            .iter()
            .find(|t| t.title == "Scene" && t.id != other.id)
            .unwrap();
        let logic = todos.iter().find(|t| t.title == "Gameplay").unwrap();
        assert!(scene.deps.is_empty());
        assert_eq!(logic.deps, vec![scene.id.clone()]);
        state
            .todos
            .patch(
                &scene.id,
                &PatchTodoRequest {
                    status: Some("completed".into()),
                    ..Default::default()
                },
            )
            .unwrap();
        record_check(&dir, "v", json!({"ok":true,"evidence":"preserved"})).unwrap();
        materialize(&state, &sid, &up, &dir, ProductionPhase::Resume).unwrap();
        assert_eq!(state.todos.list_by_session(&sid).len(), 3);
        assert_eq!(
            read_json(&dir.join(PRODUCTION_FILE)).unwrap()["checks"]["v"]["evidence"],
            "preserved"
        );
        assert_eq!(
            state
                .todos
                .list_by_session(&sid)
                .iter()
                .find(|t| t.id == scene.id)
                .unwrap()
                .status,
            "completed"
        );
        std::fs::remove_dir_all(base).unwrap();
    }

    #[test]
    fn repair_only_materializes_failed_checks_and_is_idempotent() {
        let (state, base, ws, sid, up) = fixture("up-materialize-fix");
        let dir = up.dir_abs(&ws).unwrap();
        materialize(&state, &sid, &up, &dir, ProductionPhase::Start).unwrap();
        for todo in state.todos.list_by_session(&sid) {
            state
                .todos
                .patch(
                    &todo.id,
                    &PatchTodoRequest {
                        status: Some("completed".into()),
                        ..Default::default()
                    },
                )
                .unwrap();
        }
        record_check(&dir, "v", json!({"ok":true})).unwrap();
        save(&dir,ACCEPTANCE_FILE,&json!({"rounds":[{"round":1,"failed":["m"],"results":[{"id":"m","status":"fail","note":"Movement too slow"}]}]})).unwrap();
        materialize(&state, &sid, &up, &dir, ProductionPhase::Fix).unwrap();
        let todos = state.todos.list_by_session(&sid);
        assert_eq!(todos.len(), 2);
        let fix = todos.iter().find(|t| t.title == "Gameplay").unwrap();
        let scene = todos.iter().find(|t| t.title == "Scene").unwrap();
        assert_eq!(fix.role.as_deref(), Some("logic-programmer"));
        assert_eq!(fix.deps, vec![scene.id.clone()]);
        assert_eq!(scene.status, "completed", "已通过依赖保持完成");
        assert!(fix.prompt.is_none());
        assert!(fix
            .description
            .as_deref()
            .unwrap()
            .contains("Movement too slow"));
        assert!(fix
            .description
            .as_deref()
            .unwrap()
            .contains("Implement loop"));
        let report = read_json(&dir.join(PRODUCTION_FILE)).unwrap();
        assert_eq!(report["repairDelegationVersion"], 2);
        assert_eq!(report["repairTodoIds"], json!([fix.id]));
        assert_eq!(report["activeTodoIds"].as_array().unwrap().len(), 2);
        assert_eq!(report["checks"], json!({}));
        assert_eq!(report["history"][0]["checks"]["v"]["ok"], true);
        record_check(&dir, "v", json!({"ok":true,"generation":2})).unwrap();
        materialize(&state, &sid, &up, &dir, ProductionPhase::Resume).unwrap();
        assert_eq!(state.todos.list_by_session(&sid).len(), 2);
        assert_eq!(
            read_json(&dir.join(PRODUCTION_FILE)).unwrap()["checks"]["v"]["generation"],
            2
        );
        assert!(state
            .todos
            .list_by_session(&sid)
            .iter()
            .find(|todo| todo.id == fix.id)
            .unwrap()
            .prompt
            .is_none());
        let prompt = "Read the approved loop and failed movement-speed feedback. Adjust only the existing movement script to apply input each frame; preserve scene, win, and restart behavior. Deliver the script change and evidence from the gameplay check, then describe how to repeat the manual feel check.";
        let (ok, message) = crate::native_tools::dispatch_native_scoped(
            &ws,
            &state.events,
            &state.todos,
            &sid,
            "leader-repair-fixture",
            "todo_update",
            &json!({"id":fix.id,"prompt":prompt}),
            &fix.source,
        );
        assert!(ok, "{message}");
        materialize(&state, &sid, &up, &dir, ProductionPhase::Resume).unwrap();
        assert_eq!(
            state
                .todos
                .list_by_session(&sid)
                .iter()
                .find(|todo| todo.id == fix.id)
                .unwrap()
                .prompt
                .as_deref(),
            Some(prompt)
        );
        std::fs::remove_dir_all(base).unwrap();
    }

    #[test]
    fn legacy_repair_delegations_are_retained_as_context_before_reassignment() {
        let (state, base, ws, sid, up) = fixture("up-materialize-legacy-fix");
        let dir = up.dir_abs(&ws).unwrap();
        materialize(&state, &sid, &up, &dir, ProductionPhase::Start).unwrap();
        let originals = state.todos.list_by_session(&sid);
        let original = originals.iter().find(|todo| todo.title == "Gameplay").unwrap();
        for todo in &originals {
            state.todos.patch(&todo.id, &PatchTodoRequest { status: Some("completed".into()), ..Default::default() }).unwrap();
        }
        let legacy = state.todos.create(&sid, NewTodo {
            title: "修复第1轮 · Gameplay".into(), role: Some("logic-programmer".into()),
            source: Some(original.source.clone()), plan_todo_id: Some(format!("{}:{}:fix:1:logic", up.id, up.plan_rev)),
            prompt: Some("Historical coordinator-generated repair instruction".into()),
            description: Some("Retained historical context".into()), ..Default::default()
        }).unwrap();
        save(&dir, ACCEPTANCE_FILE, &json!({"rounds":[{"round":1,"failed":["m"],"results":[{"id":"m","status":"fail","note":"Movement too slow"}]}]})).unwrap();
        let mut report = read_json(&dir.join(PRODUCTION_FILE)).unwrap();
        report["repairRound"] = json!(1);
        report["repairDelegationVersion"] = json!(1);
        report["activeTodoIds"] = json!([legacy.id]);
        save(&dir, PRODUCTION_FILE, &report).unwrap();
        materialize(&state, &sid, &up, &dir, ProductionPhase::Resume).unwrap();
        let tasks = state.todos.list_by_session(&sid);
        assert_eq!(tasks.len(), 3, "保留2个原任务和1个历史修复，不创建新链");
        let migrated = tasks.iter().find(|todo| todo.id == original.id).unwrap();
        assert_eq!(migrated.status, "queued");
        assert!(migrated.prompt.is_none());
        let context = migrated.description.as_deref().unwrap();
        assert!(context.contains("Movement too slow"));
        assert!(context.contains("Retained historical context"));
        assert!(context.contains("Historical coordinator-generated repair instruction"));
        let current = read_json(&dir.join(PRODUCTION_FILE)).unwrap();
        assert_eq!(current["repairDelegationVersion"], 2);
        assert_eq!(current["repairTodoIds"], json!([original.id]));
        assert!(!current["activeTodoIds"].as_array().unwrap().contains(&json!(legacy.id)));
        let retained = tasks.iter().find(|todo| todo.id == legacy.id).unwrap();
        assert_eq!(retained.prompt, legacy.prompt);
        assert_eq!(retained.description, legacy.description);
        assert_eq!(retained.updated_at, legacy.updated_at);
        materialize(&state, &sid, &up, &dir, ProductionPhase::Resume).unwrap();
        assert_eq!(state.todos.list_by_session(&sid).len(), 3);
        std::fs::remove_dir_all(base).unwrap();
    }

    #[test]
    fn snapshots_preserve_versions_and_never_overwrite_collisions() {
        let (_, base, _, _, _) = fixture("up-demo-snapshots");
        let dir = base.join("snapshots");
        std::fs::create_dir_all(&dir).unwrap();
        let old = dir.join(DEMO_DIR);
        let attempt = dir.join("demo-attempt");
        std::fs::create_dir(&old).unwrap();
        std::fs::create_dir(&attempt).unwrap();
        std::fs::write(old.join("index.html"), "old").unwrap();
        std::fs::write(attempt.join("index.html"), "new").unwrap();
        replace_demo(&dir, &attempt, 1).unwrap();
        assert_eq!(
            std::fs::read_to_string(old.join("index.html")).unwrap(),
            "new"
        );
        assert_eq!(
            std::fs::read_to_string(dir.join("demo-revision-1/index.html")).unwrap(),
            "old"
        );
        let rollback = dir.join("demo-rollback");
        copy_demo(&dir.join("demo-revision-1"), &rollback).unwrap();
        assert!(replace_demo(&dir, &rollback, 1).is_err());
        assert_eq!(
            std::fs::read_to_string(old.join("index.html")).unwrap(),
            "new"
        );
        replace_demo(&dir, &rollback, 2).unwrap();
        assert!(dir.join("demo-revision-1/index.html").is_file());
        assert_eq!(
            std::fs::read_to_string(old.join("index.html")).unwrap(),
            "old"
        );
        std::fs::remove_dir_all(base).unwrap();
    }

    #[test]
    fn full_requirement_chunks_are_utf8_safe_and_reconstruct_exactly() {
        let source = "游戏🎮 e\u{301}\n".repeat(1800);
        let chunks = requirement_chunks(&source);
        assert!(chunks.len() > 1);
        assert!(chunks.iter().all(|s| s.chars().count() <= 3000));
        assert_eq!(chunks.concat(), source);
    }

    #[test]
    fn approval_hash_covers_delivery_and_rejects_edits() {
        let (_, base, ws, _, up) = fixture("up-delivery-hash");
        let dir = up.dir_abs(&ws).unwrap();
        verify_plan(&up, &ws, &dir).unwrap();
        save(
            &dir,
            DELIVERY,
            &json!({"entry":"different.rxscene","controls":"changed"}),
        )
        .unwrap();
        assert!(verify_plan(&up, &ws, &dir)
            .unwrap_err()
            .contains("ULTRAPLAN_PLAN_CHANGED"));
        std::fs::remove_dir_all(base).unwrap();
    }

    #[test]
    fn completion_revalidates_report_screenshots_and_reviewer() {
        let (state, base, ws, sid, up) = fixture("up-evidence-integrity");
        let dir = up.dir_abs(&ws).unwrap();
        materialize(&state, &sid, &up, &dir, ProductionPhase::Start).unwrap();
        let entry = ws.join("Content/Scenes/main.rxscene");
        std::fs::create_dir_all(entry.parent().unwrap()).unwrap();
        std::fs::write(entry, "scene").unwrap();
        let shot = dir.join("verified.png");
        std::fs::write(&shot, b"captured-frame").unwrap();
        let shot_rel = shot
            .strip_prefix(&ws)
            .unwrap()
            .to_string_lossy()
            .replace('\\', "/");
        for check in plan()["checks"]["automated"].as_array().unwrap() {
            let id = check["id"].as_str().unwrap();
            let report_path = dir.join(format!("{id}-report.json"));
            let report = json!({"id":id,"kind":check["kind"],"ok":true,"flowId":up.id,"approvedCheck":check,"projectRoot":".","projectFingerprint":super::super::evidence::project_fingerprint(&ws).unwrap(),"backendInfo":{"renderBackend":"rurix"},"reportPath":report_path.strip_prefix(&ws).unwrap().to_string_lossy().replace('\\',"/"),"screenshots":[{"path":shot_rel,"sha256":sha256_hex(b"captured-frame")}]});
            write_json_atomic(&report_path, &report).unwrap();
            record_check(&dir, id, report).unwrap();
        }
        validated_evidence(&up.id, &ws, &dir).unwrap();
        assert!(validated_production(&up, &ws, &dir)
            .unwrap_err()
            .contains("审阅"));
        record_review(
            &dir,
            json!({"ok":false,"flowId":up.id,"planRev":up.plan_rev,"planHash":up.plan_hash}),
        )
        .unwrap();
        assert!(validated_production(&up, &ws, &dir).is_err());
        record_review(
            &dir,
            json!({"ok":true,"flowId":up.id,"planRev":up.plan_rev,"planHash":up.plan_hash}),
        )
        .unwrap();
        validated_production(&up, &ws, &dir).unwrap();
        std::fs::write(&shot, b"changed-frame").unwrap();
        assert!(validated_production(&up, &ws, &dir)
            .unwrap_err()
            .contains("ULTRAPLAN_PRODUCTION_INCOMPLETE"));
        std::fs::remove_dir_all(base).unwrap();
    }
}
