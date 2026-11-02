//! Execute approved checks against the selected project backend. Evidence comes only
//! from authorized MCP calls; callers cannot submit a verdict or another tool's report.
use super::{
    read_json, record_check, TurnKind, UltraRuntime, CHECKS_FILE, PHASE_RUNNING, STAGE_PRODUCTION,
};
use crate::{llm, playtest, AppState};
use base64::Engine as _;
use futures_util::future::BoxFuture;
use serde_json::{json, Value};
use std::path::{Component, Path, PathBuf};
use std::sync::{Arc, Mutex};

type ToolCall<'a> = dyn Fn(String, Value) -> BoxFuture<'a, playtest::ToolResult> + Send + Sync + 'a;
// All checks share the editor's active scene/play state. Serialize whole matrices,
// then separately guard the production.json read-modify-write transaction.
pub(crate) static EXECUTION_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
static RECORD_LOCK: Mutex<()> = Mutex::new(());
const FRAME: &str = "mcp__engine-scene__viewport_frame";
const EVENTS: &str = "mcp__engine-scene__host_events_drain";
const EXIT: &str = "mcp__engine-scene__play_exit";

/// Shared by the verification tool and approved per-task matrix handoffs.
pub fn matrix_schema() -> Value {
    json!({"type":"object","additionalProperties":false,"properties":{
        "scene":{"type":"string","minLength":1,"description":"批准检查中的场景路径"},
        "camera":{"type":"object"},"enter_play":{"type":"boolean","default":true},"enterPlay":{"type":"boolean"},
        "settle_frames":{"type":"integer","minimum":0,"maximum":3600},"settleFrames":{"type":"integer","minimum":0,"maximum":3600},
        "inputs":{"type":"array","maxItems":256,"items":{"type":"object","properties":{"action":{"type":"string","minLength":1},"value":{"type":"number"},"settle":{"type":"integer","minimum":0,"maximum":3600}},"required":["action","value"]}},
        "cases":{"type":"array","minItems":1,"maxItems":256,"items":{"type":"object","properties":{
            "name":{"type":"string","minLength":1},"assert":{"type":"object","properties":{
                "kind":{"type":"string","enum":["entity_count","component_field","transform_near","screenshot_ssim","screenshot_nonblank"]},
                "entity":{"description":"实体id或唯一名称","anyOf":[{"type":"integer","minimum":0},{"type":"string"}]},
                "type":{"type":"string","description":"component_field组件类型"},"field":{"type":"string","description":"component_field字段路径"},
                "op":{"type":"string","enum":["eq","neq","gt","ge","lt","le"]},"expected":{},
                "translation":{"type":"array","items":{"type":"number"},"minItems":3,"maxItems":3},"tolerance":{"type":"number","minimum":0},
                "width":{"type":"integer","minimum":8,"maximum":1920},"height":{"type":"integer","minimum":8,"maximum":1080},
                "golden":{"type":"string"},"threshold":{"type":"number","minimum":0.5,"maximum":1}
            },"required":["kind"]}
        },"required":["name","assert"]}}
    },"required":["scene","cases"]})
}

pub fn check_context(checks: &Value) -> Value {
    json!({"allowedCheckIds":checks["automated"].as_array().into_iter().flatten()
        .filter_map(|check| check["id"].as_str()).collect::<Vec<_>>(),
        "checks":checks["automated"].as_array().into_iter().flatten()
            .map(|check| json!({"id":check["id"],"kind":check["kind"],"scene":check["scene"]})).collect::<Vec<_>>(),
        "matrixSchema":matrix_schema()})
}

/// Reject malformed assertions before calling the engine so request errors do
/// not masquerade as gameplay failures and trigger implementation repairs.
pub(super) fn validate_matrix_shape(matrix: &playtest::Matrix) -> Result<(), String> {
    if matrix.scene.trim().is_empty()
        || matrix.cases.is_empty()
        || matrix.cases.len() > 256
        || matrix.inputs.len() > 256
    {
        return Err("矩阵需要 scene 与1–256个cases，inputs最多256条".into());
    }
    let mut frames = u64::from(matrix.settle_frames);
    for input in &matrix.inputs {
        if input["action"].as_str().is_none_or(|s| s.trim().is_empty())
            || !input["value"].is_number()
        {
            return Err("矩阵inputs须包含非空action和数值value".into());
        }
        let settle = match input.get("settle") {
            None => 1,
            Some(value) => value.as_u64().ok_or("输入settle须为非负整数")?,
        };
        frames = frames.checked_add(settle).ok_or("矩阵帧数溢出")?;
    }
    if frames > 3600 {
        return Err("矩阵最多推进3600帧".into());
    }
    for case in &matrix.cases {
        if case.name.trim().is_empty() {
            return Err("断言name不可为空".into());
        }
        let a = &case.assert_;
        let required: &[&str] = match a["kind"].as_str() {
            Some("entity_count") => &["expected"],
            Some("component_field") => &["entity", "type", "field", "expected"],
            Some("transform_near") => &["entity", "translation"],
            Some("screenshot_ssim") => &["golden"],
            Some("screenshot_nonblank") => &[],
            _ => return Err(format!("断言 {} 的kind无效", case.name)),
        };
        for field in required {
            if a.get(*field).is_none() {
                return Err(format!("断言 {} 缺少{field}", case.name));
            }
        }
        if let Some(op) = a.get("op") {
            if !["eq", "neq", "gt", "ge", "lt", "le"]
                .iter()
                .any(|allowed| op == allowed)
            {
                return Err(format!("断言 {} 的op须为eq/neq/gt/ge/lt/le", case.name));
            }
        }
        if let Some(entity) = a.get("entity") {
            if entity.as_u64().is_none() && entity.as_str().is_none_or(|s| s.trim().is_empty()) {
                return Err(format!("断言 {} 的entity须为id或非空名称", case.name));
            }
        }
        for field in ["type", "field", "golden"] {
            if a.get(field)
                .is_some_and(|v| v.as_str().is_none_or(|s| s.trim().is_empty()))
            {
                return Err(format!("断言 {} 的{field}须为非空字符串", case.name));
            }
        }
        if let Some(translation) = a.get("translation") {
            if translation
                .as_array()
                .is_none_or(|v| v.len() != 3 || v.iter().any(|n| !n.is_number()))
            {
                return Err(format!("断言 {} 的translation须为三元数值数组", case.name));
            }
        }
        for (field, minimum, maximum) in [("width", 8, 1920), ("height", 8, 1080)] {
            if a.get(field)
                .is_some_and(|v| v.as_u64().is_none_or(|n| !(minimum..=maximum).contains(&n)))
            {
                return Err(format!("断言 {} 的{field}越界", case.name));
            }
        }
        if a.get("tolerance")
            .is_some_and(|v| v.as_f64().is_none_or(|n| n < 0.0))
            || a.get("threshold")
                .is_some_and(|v| v.as_f64().is_none_or(|n| !(0.5..=1.0).contains(&n)))
        {
            return Err(format!("断言 {} 的tolerance或threshold无效", case.name));
        }
    }
    Ok(())
}

fn current(state: &AppState, sid: &str, rid: &str, rt: &UltraRuntime) -> Result<(), String> {
    if !matches!(rt.kind, TurnKind::Production(_)) {
        return Err("TOOL_FORBIDDEN: ultraplan_verify 仅用于制作阶段".into());
    }
    let session = state.sessions.get(sid).ok_or("会话不存在")?;
    let up = session.ultraplan.ok_or("UltraPlan 流程不存在")?;
    if up.id != rt.flow_id
        || up.stage != STAGE_PRODUCTION
        || up.phase != PHASE_RUNNING
        || up.production_run_id.as_deref() != Some(rid)
        || session.active_run_id.as_deref() != Some(rid)
    {
        return Err("ULTRAPLAN_STAGE_MISMATCH: 制作流程已变化或停止".into());
    }
    Ok(())
}

fn selected_check<'a>(
    checks: &'a Value,
    args: &Value,
) -> Result<(&'a Value, playtest::Matrix), String> {
    let id = args
        .get("id")
        .and_then(Value::as_str)
        .filter(|s| !s.trim().is_empty())
        .ok_or("检查 id 不可为空")?;
    if args.as_object().is_none()
        || args
            .as_object()
            .unwrap()
            .keys()
            .any(|k| k != "id" && k != "matrix")
    {
        return Err("仅接受 id 和 matrix；验收结果必须由服务端生成".into());
    }
    let matches: Vec<_> = checks["automated"]
        .as_array()
        .ok_or("自动检查清单缺失")?
        .iter()
        .filter(|c| c["id"].as_str() == Some(id))
        .collect();
    if matches.len() != 1 {
        return Err(format!(
            "检查 {id} 不存在或重复；合法检查id: {}",
            check_context(checks)["allowedCheckIds"]
        ));
    }
    let check = matches[0];
    let matrix: playtest::Matrix =
        serde_json::from_value(args["matrix"].clone()).map_err(|e| format!("断言矩阵无效: {e}"))?;
    validate_matrix_shape(&matrix)?;
    if matrix.scene != check["scene"].as_str().unwrap_or_default() {
        return Err(format!(
            "matrix.scene须逐字使用批准场景: {}",
            check["scene"]
        ));
    }
    let visual = matrix.cases.iter().any(|c| {
        matches!(
            c.assert_["kind"].as_str(),
            Some("screenshot_ssim" | "screenshot_nonblank")
        )
    });
    let behavioral = matrix.cases.iter().any(|c| {
        matches!(
            c.assert_["kind"].as_str(),
            Some("component_field" | "transform_near")
        )
    });
    match check["kind"].as_str() {
        Some("visual") if visual => {}
        Some("visual") => {
            return Err(
                "画面检查需要screenshot_nonblank真实视口帧；已有基准时可用screenshot_ssim".into(),
            )
        }
        Some("gameplay") if matrix.enter_play && !matrix.inputs.is_empty() && behavioral => {
            for input in &matrix.inputs {
                if input["action"]
                    .as_str()
                    .map_or(true, |s| s.trim().is_empty())
                    || !input["value"].is_number()
                {
                    return Err("玩法输入须包含非空 action 和数值 value".into());
                }
            }
        }
        Some("gameplay") => {
            return Err("玩法检查必须进 play、注入输入，并断言组件状态或位置变化".into())
        }
        _ => return Err("检查 kind 只能是 visual 或 gameplay".into()),
    }
    if check["scene"]
        .as_str()
        .map_or(true, |s| s.trim().is_empty())
    {
        return Err("批准检查缺少 scene 路径".into());
    }
    Ok((check, matrix))
}

fn scoped_file(
    project: &Path,
    workspace: &Path,
    raw: &str,
    project_only: bool,
) -> Result<PathBuf, String> {
    let raw = Path::new(raw);
    if raw.components().any(|c| matches!(c, Component::ParentDir)) {
        return Err("检查路径不可包含 ..".into());
    }
    let candidates = if raw.is_absolute() {
        vec![raw.to_path_buf()]
    } else {
        vec![project.join(raw), workspace.join(raw)]
    };
    let project = project.canonicalize().map_err(|e| e.to_string())?;
    let workspace = workspace.canonicalize().map_err(|e| e.to_string())?;
    for candidate in candidates {
        if let Ok(path) = candidate.canonicalize() {
            if path.is_file()
                && (path.starts_with(&project) || (!project_only && path.starts_with(&workspace)))
            {
                return Ok(path);
            }
        }
    }
    Err("UP_VERIFY_PREREQUISITE_MISSING: 检查文件缺失或不属于目标项目/工作区".into())
}

fn bind_paths(
    check: &Value,
    matrix: &mut playtest::Matrix,
    project: &Path,
    workspace: &Path,
) -> Result<(), String> {
    let expected = scoped_file(
        project,
        workspace,
        check["scene"].as_str().unwrap_or_default(),
        true,
    )?;
    let actual = scoped_file(project, workspace, &matrix.scene, true)?;
    if actual != expected {
        return Err("矩阵 scene 与批准检查的 scene 不一致".into());
    }
    super::evidence::require_published_file(project, &actual)?;
    matrix.scene = actual.to_string_lossy().into_owned();
    for case in &mut matrix.cases {
        if case.assert_["kind"] == "screenshot_ssim" {
            let golden = case.assert_["golden"]
                .as_str()
                .ok_or("画面断言缺少 golden")?;
            let golden = scoped_file(project, workspace, golden, false)?;
            let (w, h) =
                image::image_dimensions(&golden).map_err(|e| format!("golden 无效: {e}"))?;
            if w < 8 || h < 8 || w > 1920 || h > 1080 {
                return Err("golden 尺寸须介于 8x8 与 1920x1080".into());
            }
            let requested_w = case.assert_["width"].as_u64().unwrap_or(960);
            let requested_h = case.assert_["height"].as_u64().unwrap_or(540);
            if requested_w != u64::from(w) || requested_h != u64::from(h) {
                return Err("截图尺寸与 golden 不一致".into());
            }
            let threshold = case.assert_["threshold"].as_f64().unwrap_or(0.98);
            if !(0.5..=1.0).contains(&threshold) {
                return Err("SSIM threshold 须为 0.5..1.0".into());
            }
            case.assert_["golden"] = json!(golden.to_string_lossy());
        }
    }
    Ok(())
}

#[derive(Default)]
struct Evidence {
    frames: Vec<Value>,
    images: Vec<String>,
    events: Vec<Value>,
}

fn save_frame(value: &Value, directory: &Path, evidence: &Mutex<Evidence>) -> Result<(), String> {
    let w = value["width"]
        .as_u64()
        .filter(|v| *v > 0 && *v <= 1920)
        .ok_or("视口帧宽度无效")? as u32;
    let h = value["height"]
        .as_u64()
        .filter(|v| *v > 0 && *v <= 1080)
        .ok_or("视口帧高度无效")? as u32;
    let b64 = value["pixelsB64"]
        .as_str()
        .ok_or("视口未返回真实 RGBA 像素")?;
    if b64.len() > 12 * 1024 * 1024 {
        return Err("视口帧超过限制".into());
    }
    let pixels = base64::engine::general_purpose::STANDARD
        .decode(b64)
        .map_err(|e| format!("视口像素无效: {e}"))?;
    if pixels.len() != w as usize * h as usize * 4 {
        return Err("视口像素长度与尺寸不匹配".into());
    }
    if pixels.chunks_exact(4).all(|p| p[..3] == pixels[..3]) {
        return Err("视口只有纯色背景，不能作为画面通过证据".into());
    }
    let png = gend::mock::encode_png_rgba8(&pixels, w, h).map_err(|e| e.to_string())?;
    let mut evidence = evidence.lock().unwrap_or_else(|e| e.into_inner());
    // Keep the first four real frames; still validate subsequent frame payloads.
    if evidence.frames.len() >= 4 {
        return Ok(());
    }
    std::fs::create_dir_all(directory).map_err(|e| e.to_string())?;
    let file = directory.join(format!("frame-{}.png", evidence.frames.len() + 1));
    std::fs::write(&file, &png).map_err(|e| e.to_string())?;
    evidence.frames.push(json!({"path": file.to_string_lossy(), "width":w,"height":h,
        "sha256":forge_util::hashutil::sha256_hex(&png), "draws":value["draws"],"nonZeroPixels":value["nonZeroPixels"]}));
    evidence.images.push(format!(
        "data:image/png;base64,{}",
        base64::engine::general_purpose::STANDARD.encode(png)
    ));
    Ok(())
}

fn collect_events(value: Value, evidence: &Mutex<Evidence>) -> Result<(), String> {
    let events = value
        .as_array()
        .or_else(|| value["events"].as_array())
        .ok_or("events.drain 响应无效")?;
    let errors: Vec<_> = events
        .iter()
        .filter(|event| {
            matches!(
                event["type"].as_str().or_else(|| event["kind"].as_str()),
                Some("logic.unsupported" | "logic.call_error" | "anim.warn" | "host.crashed")
            )
        })
        .cloned()
        .collect();
    evidence
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .events
        .extend(events.iter().cloned());
    if errors.is_empty() {
        Ok(())
    } else {
        Err(format!("玩法运行错误: {}", json!(errors)))
    }
}

/// Dependency-injected engine seam: production supplies permission-checked calls;
/// tests supply responses but exercise the same matrix, evidence and failure logic.
async fn run_checked<'a>(
    matrix: &playtest::Matrix,
    expected_backend: &str,
    directory: &Path,
    call: Arc<ToolCall<'a>>,
) -> (Value, Vec<String>) {
    let evidence = Arc::new(Mutex::new(Evidence::default()));
    let mut backend = Value::Null;
    let mut capabilities = Value::Null;
    let mut matrix_report = Value::Null;
    let outcome = async {
        backend = call("mcp__engine-scene__render_backend_info".into(), json!({})).await?;
        if backend["renderBackend"].as_str() != Some(expected_backend) {
            return Err("运行后端与批准目标不一致，请重启正确的项目宿主".into());
        }
        capabilities = call("mcp__engine-scene__render_capabilities".into(), json!({})).await?;
        if capabilities["renderBackend"].as_str() != Some(expected_backend) {
            return Err("能力报告与批准后端不一致".into());
        }
        // Drain earlier work's events before measuring this matrix.
        call(EVENTS.into(), json!({})).await?;
        let frames = evidence.clone();
        let dir = directory.to_path_buf();
        let execute = call.clone();
        let mut capture = move |name: String, args: Value| {
            let execute = execute.clone();
            let frames = frames.clone();
            let dir = dir.clone();
            async move {
                if name == EXIT {
                    let mut errors = Vec::new();
                    match execute(EVENTS.into(), json!({})).await {
                        Ok(events) => {
                            if let Err(error) = collect_events(events, &frames) {
                                errors.push(error);
                            }
                        }
                        Err(error) => errors.push(error),
                    }
                    match execute(FRAME.into(), json!({"width":960,"height":540})).await {
                        Ok(frame) => {
                            if let Err(error) = save_frame(&frame, &dir, &frames) {
                                errors.push(error);
                            }
                        }
                        Err(error) => errors.push(error),
                    }
                    // Cleanup is always attempted, even when collecting evidence failed.
                    let result = execute(name, args).await;
                    if let Err(error) = &result {
                        errors.push(error.clone());
                    }
                    if !errors.is_empty() {
                        return Err(errors.join("; "));
                    }
                    return result;
                }
                let result = execute(name.clone(), args).await?;
                if name == FRAME {
                    save_frame(&result, &dir, &frames)?;
                }
                Ok(result)
            }
        };
        matrix_report = playtest::run_matrix(matrix, &mut capture).await?.to_json();
        if !matrix.enter_play {
            collect_events(call(EVENTS.into(), json!({})).await?, &evidence)?;
        }
        if matrix_report["ok"] != true {
            return Err("自动检查存在失败断言或引擎控制错误".into());
        }
        if evidence
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .frames
            .is_empty()
        {
            return Err("未获得真实截图证据".into());
        }
        Ok::<_, String>(())
    }
    .await;
    let evidence = evidence.lock().unwrap_or_else(|e| e.into_inner());
    let report = json!({"ok":outcome.is_ok(),"error":outcome.err(),"backendInfo":backend,
        "capabilities":capabilities,"matrix":matrix_report,"screenshots":evidence.frames,"events":evidence.events,
        "recordedAt":crate::events::now_rfc3339()});
    (report, evidence.images.clone())
}

fn failure_kind(report: &Value) -> Value {
    if report["ok"] == true {
        return Value::Null;
    }
    let cases = report["matrix"]["cases"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    let failed: Vec<_> = cases.iter().filter(|case| case["pass"] == false).collect();
    let errors = format!("{} {}", report["error"], json!(failed));
    if [
        "PERMISSION_DENIED",
        "CANCELLED",
        "TOOL_FORBIDDEN",
        "RX_CLI_NOT_FOUND",
    ]
    .iter()
    .any(|code| errors.contains(code))
    {
        return json!("blocked");
    }
    if errors.contains("玩法运行错误") {
        return json!("runtime");
    }
    if failed
        .iter()
        .any(|case| !matches!(case["kind"].as_str(), Some("lifecycle" | "inject")))
    {
        return json!("assertion");
    }
    json!("execution")
}

pub async fn execute(
    state: &AppState,
    sid: &str,
    rid: &str,
    rt: &UltraRuntime,
    project_root: &Path,
    args: &Value,
) -> (bool, llm::ToolFeedback) {
    let result = execute_inner(state, sid, rid, rt, project_root, args).await;
    match result {
        Ok(result) => result,
        Err(error) => {
            let checks = read_json(&rt.dir_abs.join(CHECKS_FILE)).unwrap_or(Value::Null);
            let failure_kind = if error.starts_with("UP_VERIFY_PREREQUISITE_MISSING:") {
                "blocked"
            } else {
                "invalid_request"
            };
            let diagnostic = json!({"ok":false,"failureKind":failure_kind,"error":error,
                "id":args["id"],"runId":rid,"at":crate::events::now_rfc3339(),"context":check_context(&checks)});
            // Invalid matrices and missing prerequisites are attempt diagnostics,
            // not fabricated gameplay failures and never replace a valid report.
            if current(state, sid, rid, rt).is_ok() {
                let _record_guard = RECORD_LOCK.lock().unwrap_or_else(|e| e.into_inner());
                if let Some(mut production) = read_json(&rt.dir_abs.join(super::PRODUCTION_FILE)) {
                    if !production["verificationErrors"].is_array() {
                        production["verificationErrors"] = json!([]);
                    }
                    let mut saved = diagnostic.clone();
                    saved["context"]
                        .as_object_mut()
                        .unwrap()
                        .remove("matrixSchema");
                    production["verificationErrors"]
                        .as_array_mut()
                        .unwrap()
                        .push(saved);
                    if let Err(save_error) = super::write_json_atomic(
                        &rt.dir_abs.join(super::PRODUCTION_FILE),
                        &production,
                    ) {
                        return (
                            false,
                            format!(
                                "ULTRAPLAN_VERIFY_FAILED: {diagnostic}; 诊断记录失败: {save_error}"
                            )
                            .into(),
                        );
                    }
                }
            }
            (
                false,
                format!("ULTRAPLAN_VERIFY_FAILED: {diagnostic}").into(),
            )
        }
    }
}

async fn execute_inner(
    state: &AppState,
    sid: &str,
    rid: &str,
    rt: &UltraRuntime,
    project_root: &Path,
    args: &Value,
) -> Result<(bool, llm::ToolFeedback), String> {
    current(state, sid, rid, rt)?;
    let _engine_guard = EXECUTION_LOCK.lock().await;
    current(state, sid, rid, rt)?;
    let checks = read_json(&rt.dir_abs.join(CHECKS_FILE)).ok_or("检查清单缺失")?;
    let (check, mut matrix) = selected_check(&checks, args)?;
    let id = check["id"].as_str().unwrap();
    let workspace = rt.dir_abs.ancestors().nth(3).ok_or("流程目录无效")?;
    bind_paths(check, &mut matrix, project_root, workspace)?;
    let canonical_project = project_root.canonicalize().map_err(|e| e.to_string())?;
    let canonical_workspace = workspace.canonicalize().map_err(|e| e.to_string())?;
    let project_relative = canonical_project
        .strip_prefix(&canonical_workspace)
        .map_err(|_| "正式验证项目不属于流程工作区")?
        .to_string_lossy()
        .replace('\\', "/");
    let project_relative = if project_relative.is_empty() {
        ".".to_string()
    } else {
        project_relative
    };
    let fingerprint = super::evidence::project_fingerprint(&canonical_project)?;
    let target = read_json(&rt.dir_abs.join("target.json")).ok_or("批准目标缺失")?;
    let backend = target["renderBackend"]
        .as_str()
        .filter(|b| matches!(*b, "rurix" | "godot"))
        .ok_or("批准后端无效")?;
    let directory = rt
        .dir_abs
        .join("verification")
        .join(crate::events::new_id("check"));
    let project = project_root.to_path_buf();
    let call: Arc<ToolCall<'_>> = Arc::new(move |name, args| {
        let project = project.clone();
        Box::pin(async move {
            if name != EXIT && state.runs.is_cancelled(rid) {
                return Err("CANCELLED: 验证轮次已停止".into());
            }
            let write = crate::agent::is_write_tool(&name);
            let allowed = state
                .permissions
                .authorize_with(
                    &state.events,
                    sid,
                    rid,
                    &name,
                    write,
                    json!({"projectRoot":project.to_string_lossy(),"source":"ultraplan_verify"}),
                )
                .await?;
            if !allowed {
                return Err(format!("PERMISSION_DENIED: {name}"));
            }
            let result = crate::mcp::call_tool_in(&project, &name, Some(args))
                .await
                .map_err(|e| e.to_string())?;
            playtest::unwrap_envelope(&result)
        })
    });
    let (mut report, images) = run_checked(&matrix, backend, &directory, call).await;
    report["failureKind"] = failure_kind(&report);
    let after = super::evidence::project_fingerprint(&canonical_project);
    if after.as_ref().map_or(true, |value| value != &fingerprint) {
        report["ok"] = json!(false);
        report["error"] = json!("项目在验证期间发生变化，必须重新运行全部自动检查");
        report["failureKind"] = json!("stale");
    }
    report["id"] = json!(id);
    report["kind"] = check["kind"].clone();
    report["runId"] = json!(rid);
    report["flowId"] = json!(rt.flow_id);
    report["approvedCheck"] = check.clone();
    report["requestedMatrix"] = args["matrix"].clone();
    report["projectRoot"] = json!(project_relative);
    report["projectFingerprint"] = json!(fingerprint);
    if let Some(shots) = report["screenshots"].as_array_mut() {
        for shot in shots {
            if let Some(path) = shot["path"].as_str().map(PathBuf::from) {
                shot["path"] = json!(path
                    .strip_prefix(workspace)
                    .unwrap_or(&path)
                    .to_string_lossy()
                    .replace('\\', "/"));
            }
        }
    }
    current(state, sid, rid, rt)?;
    report["reportPath"] = json!(directory
        .join("report.json")
        .strip_prefix(workspace)
        .map_err(|e| e.to_string())?
        .to_string_lossy()
        .replace('\\', "/"));
    super::write_json_atomic(&directory.join("report.json"), &report).map_err(|e| e.to_string())?;
    {
        let _record_guard = RECORD_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        record_check(&rt.dir_abs, id, report.clone())?;
    }
    Ok((
        report["ok"] == true,
        llm::ToolFeedback {
            text: report.to_string(),
            images,
        },
    ))
}

/// Supply current, protected visual evidence directly to the reviewer model.
/// A pure textual APPROVE is not an alternative to actually receiving images.
pub fn reviewer_feedback(rt: &UltraRuntime) -> Result<llm::ToolFeedback, String> {
    let production = super::validation_summary(rt)?;
    let workspace = rt.dir_abs.ancestors().nth(3).ok_or("流程目录无效")?;
    let mut images = Vec::new();
    let mut notes = Vec::new();
    let mut total_bytes = 0usize;
    for (id, report) in production["checks"]
        .as_object()
        .ok_or("验证记录缺少checks")?
    {
        if report["kind"] != "visual" {
            continue;
        }
        let shot = report["screenshots"]
            .as_array()
            .and_then(|shots| shots.first())
            .ok_or("画面检查缺少截图")?;
        let raw = shot["path"].as_str().ok_or("截图路径缺失")?;
        let file = scoped_file(workspace, workspace, raw, true)?;
        let bytes = std::fs::read(file).map_err(|e| e.to_string())?;
        if shot["sha256"] != forge_util::hashutil::sha256_hex(&bytes) {
            return Err("审阅截图在读取前被修改".into());
        }
        total_bytes += bytes.len();
        if images.len() >= 16 || total_bytes > 32 * 1024 * 1024 {
            return Err("视觉检查截图超出一次审阅容量，请拆分批准的检查清单".into());
        }
        image::load_from_memory(&bytes).map_err(|e| format!("审阅截图无效: {e}"))?;
        images.push(format!(
            "data:image/png;base64,{}",
            base64::engine::general_purpose::STANDARD.encode(bytes)
        ));
        notes.push(format!(
            "图{} / 检查{id}: {}。真实测量: {}。",
            images.len(),
            report["approvedCheck"],
            report["matrix"]
        ));
    }
    if images.is_empty() {
        return Err("终审没有可供目视的真实截图".into());
    }
    Ok(llm::ToolFeedback { text: format!("以下是本次正式项目自动验证产生且已校验版本的真实截图。screenshot_nonblank只证明视口非空，必须逐图核对布局、美术、可见操作反馈与批准需求，才能给出视觉通过结论。\n{}",notes.join("\n")), images })
}

#[cfg(test)]
mod tests {
    use super::*;
    fn checks() -> Value {
        json!({"automated":[{"id":"v","kind":"visual","scene":"game.rxscene"},{"id":"g","kind":"gameplay","scene":"game.rxscene"}]})
    }
    fn matrix() -> Value {
        json!({"scene":"game.rxscene","inputs":[{"action":"right","value":1}],
        "cases":[{"name":"position","assert":{"kind":"transform_near","entity":1,"translation":[1,0,0]}}]})
    }
    #[test]
    fn checks_cannot_be_substituted_with_report_or_wrong_kind() {
        assert!(selected_check(&checks(), &json!({"id":"g","matrix":matrix()})).is_ok());
        assert!(selected_check(&checks(), &json!({"id":"v","matrix":matrix()})).is_err());
        assert!(selected_check(&checks(), &json!({"id":"missing","matrix":matrix()})).is_err());
        assert!(selected_check(&checks(), &json!({"id":"g","matrix":matrix(),"ok":true})).is_err());
        let mut no_input = matrix();
        no_input["inputs"] = json!([]);
        assert!(selected_check(&checks(), &json!({"id":"g","matrix":no_input})).is_err());
        let first_visual = json!({"scene":"game.rxscene","enterPlay":false,"cases":[{"name":"first frame","assert":{"kind":"screenshot_nonblank","width":960,"height":540}}]});
        assert!(selected_check(&checks(), &json!({"id":"v","matrix":first_visual})).is_ok());
    }
    #[test]
    fn verification_context_names_approved_ids_and_documents_the_real_matrix_contract() {
        let context = check_context(&checks());
        assert_eq!(context["allowedCheckIds"], json!(["v", "g"]));
        assert_eq!(
            context["matrixSchema"]["required"],
            json!(["scene", "cases"])
        );
        assert_eq!(
            context["matrixSchema"]["properties"]["cases"]["items"]["properties"]["assert"]
                ["properties"]["op"]["enum"],
            json!(["eq", "neq", "gt", "ge", "lt", "le"])
        );
        let error =
            selected_check(&checks(), &json!({"id":"flow_123","matrix":matrix()})).unwrap_err();
        assert!(error.contains("合法检查id"));
        assert!(error.contains("\"v\"") && error.contains("\"g\""));
    }
    #[test]
    fn malformed_matrix_is_a_request_error_before_any_engine_work() {
        let mut input = matrix();
        input["cases"][0]["assert"]["translation"] = json!([1, 0]);
        assert!(selected_check(&checks(), &json!({"id":"g","matrix":input})).is_err());
        input = matrix();
        input["cases"][0]["assert"] = json!({"kind":"component_field","entity":1,"type":"Transform","field":"x","op":"gte","expected":1});
        assert!(selected_check(&checks(), &json!({"id":"g","matrix":input}))
            .unwrap_err()
            .contains("op"));
        input["cases"][0]["assert"]["op"] = json!("ge");
        assert!(selected_check(&checks(), &json!({"id":"g","matrix":input})).is_ok());
        input["settle_frames"] = json!(3600);
        assert!(selected_check(&checks(), &json!({"id":"g","matrix":input}))
            .unwrap_err()
            .contains("3600"));
    }
    #[test]
    fn failure_kind_separates_capability_and_control_errors_from_gameplay_failures() {
        assert_eq!(
            failure_kind(&json!({"ok":false,"error":"PERMISSION_DENIED"})),
            "blocked"
        );
        assert_eq!(
            failure_kind(&json!({"ok":false,"error":"玩法运行错误: logic.call_error"})),
            "runtime"
        );
        assert_eq!(
            failure_kind(
                &json!({"ok":false,"matrix":{"cases":[{"pass":false,"kind":"lifecycle","detail":"play_step failed"}]}})
            ),
            "execution"
        );
        assert_eq!(
            failure_kind(
                &json!({"ok":false,"matrix":{"cases":[{"pass":false,"kind":"transform_near","detail":"wrong position"}]}})
            ),
            "assertion"
        );
    }
    #[tokio::test]
    async fn real_matrix_and_frames_decide_result_and_cleanup_survives_errors() {
        let directory = std::env::temp_dir().join(crate::events::new_id("ultraplan-verify-test"));
        for failed in [
            None,
            Some("play_step"),
            Some("runtime"),
            Some("permission"),
            Some("backend"),
        ] {
            let calls = Arc::new(Mutex::new(Vec::new()));
            let recorded = calls.clone();
            let call: Arc<ToolCall<'_>> = Arc::new(move |name, _args| {
                recorded.lock().unwrap().push(name.clone());
                Box::pin(async move {
                    if failed == Some("permission") {
                        return Err("PERMISSION_DENIED".into());
                    }
                    if failed == Some("play_step") && name.ends_with("play_step") {
                        return Err("step failed".into());
                    }
                    if name.ends_with("render_backend_info")
                        || name.ends_with("render_capabilities")
                    {
                        return Ok(
                            json!({"renderBackend":if failed == Some("backend") {"godot"}else{"rurix"}}),
                        );
                    }
                    if name == EVENTS {
                        return Ok(if failed == Some("runtime") {
                            json!([{"type":"logic.unsupported"}])
                        } else {
                            json!([])
                        });
                    }
                    if name == FRAME {
                        let mut pixels = vec![0_u8; 16 * 16 * 4];
                        pixels[4] = 255;
                        return Ok(
                            json!({"width":16,"height":16,"pixelsB64":base64::engine::general_purpose::STANDARD.encode(pixels)}),
                        );
                    }
                    if name.ends_with("transform_get") {
                        return Ok(json!({"translation":[1,0,0]}));
                    }
                    Ok(json!({}))
                })
            });
            let matrix: playtest::Matrix = serde_json::from_value(matrix()).unwrap();
            let (report, images) = run_checked(&matrix, "rurix", &directory, call).await;
            assert_eq!(report["ok"], failed.is_none(), "{failed:?}: {report}");
            if failed.is_none() {
                assert!(!images.is_empty());
                assert_eq!(report["matrix"]["passed"], 1);
            }
            if matches!(failed, None | Some("play_step" | "runtime")) {
                assert_eq!(calls.lock().unwrap().last().unwrap(), EXIT);
            }
        }
        if directory.exists() {
            assert_eq!(
                directory.canonicalize().unwrap().parent(),
                Some(std::env::temp_dir().canonicalize().unwrap().as_path())
            );
            std::fs::remove_dir_all(directory).unwrap();
        }
    }
}
