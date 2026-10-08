//! Codex native image generation. Authentication stays in app-server; Forge
//! receives imageGeneration items and stores PNGs in the existing asset pipeline.
//! Design tools use a private, image-only thread after the host permission gate.

use axum::extract::{Query, State};
use axum::http::{header, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use base64::Engine as _;
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::HashSet;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use super::rpc::{CodexClient, CodexError, Inbound};
use crate::AppState;

pub const BACKEND_ID: &str = "codex-native";
const IMAGE_MAX_BYTES: usize = 32 * 1024 * 1024;
const GENERATION_TIMEOUT: Duration = Duration::from_secs(600);

#[derive(Clone)]
pub struct Request {
    pub prompt: String,
    pub negative_prompt: Option<String>,
    pub images: Vec<Vec<u8>>,
    pub mask: Option<Vec<u8>>,
    pub aspect: gend::backends::Aspect,
    pub n: u32,
    pub quality: Option<String>,
    pub background: Option<String>,
}

/// A private process must be retired even if the caller drops its future.
struct PrivateClient(Arc<CodexClient>);
impl Drop for PrivateClient {
    fn drop(&mut self) {
        self.0.suspend();
    }
}

pub async fn generate(
    state: &Arc<AppState>,
    sid: &str,
    run_id: &str,
    cwd: &Path,
    req: Request,
) -> Result<Vec<gend::backends::GenCandidate>, CodexError> {
    if req.prompt.trim().is_empty() || !(1..=4).contains(&req.n) {
        return Err(CodexError(
            "GEN_BAD_PARAMS: prompt 不可空，n 须为 1..=4".into(),
        ));
    }
    if state.runs.is_cancelled(run_id) {
        return Err(CodexError("CODEX_IMAGEGEN_CANCELLED: 生图已取消".into()));
    }
    let client = PrivateClient(state.codex.isolated_client()?);
    let cancelled = async {
        loop {
            if state.runs.is_cancelled(run_id) {
                break;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    };
    tokio::select! {
        biased;
        _ = cancelled => Err(CodexError("CODEX_IMAGEGEN_CANCELLED: 生图已取消".into())),
        result = tokio::time::timeout(GENERATION_TIMEOUT, generate_inner(&client.0, state, sid, cwd, &req)) => {
            result.unwrap_or_else(|_| Err(CodexError("CODEX_IMAGEGEN_TIMEOUT: 原生生图超过 10 分钟".into())))
        }
    }
}

async fn generate_inner(
    client: &CodexClient,
    state: &AppState,
    sid: &str,
    cwd: &Path,
    req: &Request,
) -> Result<Vec<gend::backends::GenCandidate>, CodexError> {
    let mut inbox = client.subscribe();
    client.ensure_started().await?;
    // The provider, rather than the text model picker, bounds hosted tools.
    let capabilities = client
        .request("modelProvider/capabilities/read", json!({}))
        .await?;
    if capabilities["imageGeneration"] != true {
        return Err(CodexError(
            "CODEX_IMAGEGEN_UNAVAILABLE: 当前 Codex 上游不支持原生生图，请检查 Codex 登录与渠道"
                .into(),
        ));
    }
    let effective = client
        .request("config/read", json!({"includeLayers": false, "cwd": cwd}))
        .await?;
    let config = effective
        .get("config")
        .filter(|v| v.is_object())
        .ok_or_else(|| CodexError("CODEX_IMAGEGEN_CONFIG: 无法读取隔离配置".into()))?;
    let catalog = client
        .request("skills/list", json!({"cwds": [cwd], "forceReload": true}))
        .await?;
    let mut isolation = super::managed::isolated_config(config, &catalog, cwd)
        .map_err(|e| CodexError(e.to_string()))?;
    isolation["features"]["image_generation"] = json!(true);
    let mut params = json!({
        "cwd": cwd, "ephemeral": true, "approvalPolicy": "never", "sandbox": "read-only",
        "config": isolation,
        "developerInstructions": "你是 Forge 的原生生图执行器。只使用 Codex 内置 imagegen 工具生成或编辑指定图片；不得调用 shell、文件编辑、MCP、子代理、Goal 或其它工具。参考图与蒙版由本轮输入提供；编辑必须使用参考图。生成指定数量的图片后结束本轮。工具失败就如实报告，不得用代码绘图替代。",
    });
    let cfg = super::config::load();
    let model = state
        .sessions
        .get(sid)
        .and_then(|s| s.selected_model_id)
        .and_then(|id| id.strip_prefix("codex:").map(str::to_string))
        .or_else(|| Some(cfg.default_model).filter(|m| !m.trim().is_empty()));
    if let Some(model) = model {
        params["model"] = json!(model);
    }
    let started = client.request("thread/start", params).await?;
    let thread_id = started
        .pointer("/thread/id")
        .and_then(Value::as_str)
        .ok_or_else(|| CodexError("CODEX_IMAGEGEN_PROTOCOL: 缺少 thread.id".into()))?;
    // Isolation warnings can precede thread/start's response.
    let mut queued = Vec::new();
    while let Ok(event) = inbox.try_recv() {
        check_config(&event)?;
        queued.push(event);
    }
    let turn = client
        .request(
            "turn/start",
            json!({
                "threadId": thread_id, "input": input(req), "cwd": cwd,
                "approvalPolicy": "never", "sandboxPolicy": {"type": "readOnly"},
            }),
        )
        .await?;
    let turn_id = turn
        .pointer("/turn/id")
        .and_then(Value::as_str)
        .ok_or_else(|| CodexError("CODEX_IMAGEGEN_PROTOCOL: 缺少 turn.id".into()))?;
    let mut pending = queued.into_iter();
    let mut seen = HashSet::new();
    let mut candidates = Vec::new();
    loop {
        let event = match pending.next() {
            Some(event) => event,
            None => inbox.recv().await.ok_or_else(|| {
                CodexError("CODEX_IMAGEGEN_CONNECTION_LOST: 生图连接已断开".into())
            })?,
        };
        check_config(&event)?;
        let p = event.params();
        if super::rpc::thread_id_of(p) != Some(thread_id)
            || p.get("turnId")
                .and_then(Value::as_str)
                .is_some_and(|id| id != turn_id)
        {
            continue;
        }
        match event {
            Inbound::ServerRequest { id, method, .. } => {
                client.respond_error(&id, -32601, "原生生图线程只允许内置 imagegen")?;
                return Err(CodexError(format!(
                    "CODEX_IMAGEGEN_TOOL_FORBIDDEN: {method}"
                )));
            }
            Inbound::Notification { method, params } => match method.as_str() {
                "item/started" | "item/completed" => {
                    let item = &params["item"];
                    let kind = item["type"].as_str().unwrap_or_default();
                    if matches!(
                        kind,
                        "commandExecution"
                            | "fileChange"
                            | "mcpToolCall"
                            | "dynamicToolCall"
                            | "collabAgentToolCall"
                            | "collabToolCall"
                    ) {
                        return Err(CodexError(format!("CODEX_IMAGEGEN_TOOL_FORBIDDEN: {kind}")));
                    }
                    if method == "item/completed"
                        && kind == "imageGeneration"
                        && seen.insert(item["id"].to_string())
                    {
                        let png_bytes = png_from_item(item)?;
                        // Native imagegen does not expose a reproducible RNG seed.
                        candidates.push(gend::backends::GenCandidate {
                            seed: gend::fnv1a64(&png_bytes),
                            png_bytes,
                        });
                    }
                }
                "turn/completed" => {
                    if params.pointer("/turn/status").and_then(Value::as_str) != Some("completed") {
                        return Err(CodexError(format!(
                            "CODEX_IMAGEGEN_FAILED: {}",
                            params["turn"]["error"]
                        )));
                    }
                    if candidates.len() != req.n as usize {
                        return Err(CodexError(format!(
                            "CODEX_IMAGEGEN_NO_IMAGE: 要求 {} 张，实际返回 {} 张",
                            req.n,
                            candidates.len()
                        )));
                    }
                    return Ok(candidates);
                }
                "error" if params.get("willRetry") != Some(&Value::Bool(true)) => {
                    return Err(CodexError(format!(
                        "CODEX_IMAGEGEN_FAILED: {}",
                        params["message"]
                    )));
                }
                _ => {}
            },
        }
    }
}

fn check_config(event: &Inbound) -> Result<(), CodexError> {
    if matches!(event.method(), "configWarning" | "skills/changed") {
        return Err(CodexError(format!(
            "CODEX_IMAGEGEN_CONFIG: {}",
            event.params()
        )));
    }
    Ok(())
}

fn input(req: &Request) -> Vec<Value> {
    let (w, h) = req.aspect.dims(1024);
    let mut text = format!(
        "{}\n\nUse native imagegen to {} exactly {} image(s), size {w}x{h}.",
        req.prompt,
        if req.images.is_empty() {
            "generate"
        } else {
            "edit the supplied reference into"
        },
        req.n
    );
    if let Some(negative) = &req.negative_prompt {
        text.push_str(&format!("\nAvoid: {negative}"));
    }
    if let Some(quality) = &req.quality {
        text.push_str(&format!("\nRequested quality: {quality}."));
    }
    if let Some(background) = &req.background {
        text.push_str(&format!("\nBackground: {background}; use the native transparent_background option when transparent is requested."));
    }
    if req.mask.is_some() {
        text.push_str("\nThe last image is an edit mask: transparent pixels may be repainted; keep everything outside the transparent area unchanged.");
    }
    let mut out = vec![json!({"type": "text", "text": text})];
    for bytes in req.images.iter().chain(req.mask.iter()) {
        out.push(json!({"type": "image", "url": format!("data:image/png;base64,{}", base64::engine::general_purpose::STANDARD.encode(bytes))}));
    }
    out
}

pub fn failure_message(item: &Value) -> String {
    let failure = item.get("failure").filter(|v| !v.is_null());
    let detail = failure
        .map(Value::to_string)
        .or_else(|| {
            item.get("error")
                .filter(|v| !v.is_null())
                .map(Value::to_string)
        })
        .or_else(|| {
            item.get("result")
                .and_then(Value::as_str)
                .filter(|s| !s.is_empty())
                .map(str::to_string)
        })
        .unwrap_or_else(|| "Codex 没有返回可用图片".into());
    format!("CODEX_IMAGEGEN_FAILED: {detail}")
}

/// Decode the protocol's result, never interpret base64 as a human tool output.
/// savedPath is optional; do not read arbitrary local files through an item.
pub fn png_from_item(item: &Value) -> Result<Vec<u8>, CodexError> {
    if item["status"] != "completed" || item.get("failure").is_some_and(|v| !v.is_null()) {
        return Err(CodexError(failure_message(item)));
    }
    let raw = item["result"].as_str().unwrap_or_default();
    let encoded = raw.strip_prefix("data:image/png;base64,").unwrap_or(raw);
    if encoded.is_empty() || encoded.len() > IMAGE_MAX_BYTES * 4 / 3 + 4 {
        return Err(CodexError(
            "CODEX_IMAGEGEN_BAD_IMAGE: 图片为空或超过 32 MiB".into(),
        ));
    }
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(encoded)
        .map_err(|e| CodexError(format!("CODEX_IMAGEGEN_BAD_IMAGE: base64 解码失败: {e}")))?;
    let mut reader = image::ImageReader::new(std::io::Cursor::new(bytes))
        .with_guessed_format()
        .map_err(|e| CodexError(format!("CODEX_IMAGEGEN_BAD_IMAGE: {e}")))?;
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(8192);
    limits.max_image_height = Some(8192);
    limits.max_alloc = Some(256 * 1024 * 1024);
    reader.limits(limits);
    let image = reader
        .decode()
        .map_err(|e| CodexError(format!("CODEX_IMAGEGEN_BAD_IMAGE: {e}")))?;
    let mut png = std::io::Cursor::new(Vec::new());
    image
        .write_to(&mut png, image::ImageFormat::Png)
        .map_err(|e| CodexError(format!("CODEX_IMAGEGEN_BAD_IMAGE: {e}")))?;
    Ok(png.into_inner())
}

/// Persist ordinary Codex chat outputs with compact replayable event metadata.
pub fn persist(
    state: &AppState,
    session: &crate::sessions::DebugSession,
    item: &Value,
) -> Result<Value, CodexError> {
    let png = png_from_item(item)?;
    let project = crate::media_project(state, session.workspace_id.as_deref())
        .map_err(|_| CodexError("CODEX_IMAGEGEN_WORKSPACE: 生图工作区不可用".into()))?;
    let sidecar = json!({"backendId": BACKEND_ID, "toolCallId": item["id"],
        "revisedPrompt": item["revisedPrompt"], "transparentBackground": item["transparentBackground"],
        "generatedAt": forge_util::timeutil::unix_millis(), "seedAvailable": false});
    let file_ref = gend::tmpstore::save_candidate(&project, &png, gend::fnv1a64(&png), 0, &sidecar)
        .map_err(|e| CodexError(format!("CODEX_IMAGEGEN_SAVE_FAILED: {e}")))?;
    let mut url = format!("/api/forge/gen/image/file?fileRef={file_ref}");
    if let Some(id) = &session.workspace_id {
        let encoded: String = id
            .bytes()
            .map(|b| {
                if b.is_ascii_alphanumeric() || b"-_.~".contains(&b) {
                    (b as char).to_string()
                } else {
                    format!("%{b:02X}")
                }
            })
            .collect();
        url.push_str(&format!("&workspaceId={encoded}"));
    }
    Ok(
        json!({"toolCallId": item["id"], "imageFileRef": file_ref, "url": url,
        "backendId": BACKEND_ID, "revisedPrompt": item["revisedPrompt"]}),
    )
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileQuery {
    pub workspace_id: Option<String>,
    pub file_ref: String,
}

/// Only serve generated PNGs in the selected project's confined tmp directory.
pub async fn image_file(
    State(state): State<Arc<AppState>>,
    Query(req): Query<FileQuery>,
) -> Response {
    let project = match crate::media_project(&state, req.workspace_id.as_deref()) {
        Ok(project) => project,
        Err(response) => return response,
    };
    let joined = tokio::task::spawn_blocking(move || {
        let missing = || (StatusCode::NOT_FOUND, Json(json!({"error":{"code":"GEN_FILE_NOT_FOUND","message":"当前工作区的图片产物不存在或路径无效"}}))).into_response();
        let Ok(resolved) = gend::tmpstore::resolve_ref(&project, &req.file_ref) else { return missing(); };
        let (Ok(root), Ok(gen_root), Ok(path)) = (project.root.canonicalize(), project.root.join(".forge/tmp/gen").canonicalize(), resolved.canonicalize()) else { return missing(); };
        if !gen_root.starts_with(&root) || !path.starts_with(&gen_root) || path.extension().and_then(|s| s.to_str()) != Some("png") {
            return missing();
        }
        if std::fs::metadata(&path).map_or(true, |m| !m.is_file() || m.len() > IMAGE_MAX_BYTES as u64) {
            return missing();
        }
        match std::fs::read(path) {
            Ok(bytes) => ([(header::CONTENT_TYPE, "image/png"), (header::CACHE_CONTROL, "no-cache"), (header::X_CONTENT_TYPE_OPTIONS, "nosniff")], bytes).into_response(),
            Err(_) => missing(),
        }
    }).await;
    joined.unwrap_or_else(|_| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error":{"code":"FORGE_IO","message":"读取图片产物失败"}})),
        )
            .into_response()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    pub(super) fn item() -> Value {
        let png = gend::mock::encode_png_rgba8(&[12, 34, 56, 255], 1, 1).unwrap();
        json!({"type":"imageGeneration","id":"image-1","status":"completed",
            "result":base64::engine::general_purpose::STANDARD.encode(png), "revisedPrompt":"a test pixel"})
    }

    #[test]
    fn native_result_decodes_real_png_and_never_reads_saved_path() {
        let png = png_from_item(&item()).unwrap();
        assert_eq!(
            image::load_from_memory(&png)
                .unwrap()
                .to_rgba8()
                .get_pixel(0, 0)
                .0,
            [12, 34, 56, 255]
        );
        for value in [
            json!({"status":"completed","result":"", "savedPath":"C:/private/auth.json"}),
            json!({"status":"completed","result":"not-base64"}),
            json!({"status":"completed","result":base64::engine::general_purpose::STANDARD.encode(b"not an image")}),
        ] {
            assert!(png_from_item(&value)
                .unwrap_err()
                .0
                .contains("CODEX_IMAGEGEN_BAD_IMAGE"));
        }
        let mut failed = item();
        failed["failure"] = json!({"type":"usageLimitExceeded","limitId":"imagegen"});
        assert!(png_from_item(&failed)
            .unwrap_err()
            .0
            .contains("usageLimitExceeded"));
    }

    #[test]
    fn native_edit_input_preserves_references_mask_and_output_options() {
        let req = Request {
            prompt: "edit only the button".into(),
            negative_prompt: Some("no text".into()),
            images: vec![vec![1, 2, 3]],
            mask: Some(vec![4, 5, 6]),
            aspect: gend::backends::Aspect::Portrait,
            n: 2,
            quality: Some("high".into()),
            background: Some("transparent".into()),
        };
        let out = input(&req);
        assert_eq!(out.len(), 3);
        let text = out[0]["text"].as_str().unwrap();
        for expected in [
            "edit only the button",
            "exactly 2",
            "1024x1536",
            "no text",
            "high",
            "transparent",
            "edit mask",
        ] {
            assert!(text.contains(expected), "missing {expected}: {text}");
        }
        assert_eq!(out[1]["url"], "data:image/png;base64,AQID");
        assert_eq!(out[2]["url"], "data:image/png;base64,BAUG");
    }
}
