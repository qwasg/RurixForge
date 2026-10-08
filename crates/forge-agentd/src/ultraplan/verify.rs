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
static EXECUTION_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
static RECORD_LOCK: Mutex<()> = Mutex::new(());
const FRAME: &str = "mcp__engine-scene__viewport_frame";
const EVENTS: &str = "mcp__engine-scene__host_events_drain";
const EXIT: &str = "mcp__engine-scene__play_exit";

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
        return Err(format!("检查 {id} 不存在或重复"));
    }
    let check = matches[0];
    let matrix: playtest::Matrix =
        serde_json::from_value(args["matrix"].clone()).map_err(|e| format!("断言矩阵无效: {e}"))?;
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
    Err("检查文件缺失或不属于目标项目/工作区".into())
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
        Err(error) => (false, format!("ULTRAPLAN_VERIFY_FAILED: {error}").into()),
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
    let after = super::evidence::project_fingerprint(&canonical_project);
    if after.as_ref().map_or(true, |value| value != &fingerprint) {
        report["ok"] = json!(false);
        report["error"] = json!("项目在验证期间发生变化，必须重新运行全部自动检查");
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
