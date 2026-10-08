//! Durable frozen-frame evidence. Live picks still resolve through the original host snapshot.
use super::*;
use base64::Engine as _;

pub(super) fn persist(p: &ScopeProject, mut frame: Value) -> Result<Value, String> {
    let w = frame["width"].as_u64().ok_or("EDITOR_FRAME_INVALID")?;
    let h = frame["height"].as_u64().ok_or("EDITOR_FRAME_INVALID")?;
    if w == 0 || h == 0 || w.checked_mul(h).is_none_or(|n| n > 4_194_304) {
        return Err("EDITOR_FRAME_TOO_LARGE".into());
    }
    let pixels = base64::engine::general_purpose::STANDARD
        .decode(frame["pixelsB64"].as_str().ok_or("EDITOR_FRAME_INVALID")?)
        .map_err(|e| e.to_string())?;
    if pixels.len() != (w * h * 4) as usize {
        return Err("EDITOR_FRAME_INVALID: RGBA dimensions differ".into());
    }
    let png =
        gend::mock::encode_png_rgba8(&pixels, w as u32, h as u32).map_err(|e| e.to_string())?;
    let id = frame["observationId"]
        .as_str()
        .map(str::to_owned)
        .unwrap_or_else(|| crate::events::new_id("preview"));
    safe_id(&id)?;
    let path = safe_path(
        &p.project_root,
        &format!(".forge/editor/observations/{id}.png"),
    )?;
    std::fs::create_dir_all(path.parent().unwrap()).map_err(|e| e.to_string())?;
    std::fs::write(&path, png).map_err(|e| e.to_string())?;
    frame
        .as_object_mut()
        .ok_or("EDITOR_FRAME_INVALID")?
        .remove("pixelsB64");
    frame["observationId"] = json!(id);
    frame["imageUrl"] = json!(format!(
        "/api/forge/editor/images/{id}?workspaceId={}",
        p.id()
    ));
    frame["reference"] = json!({"workspaceId":p.id(),"kind":"viewport","resourceId":id,"sceneGuid":frame["sceneGuid"],"revision":frame["contentRevision"],"hostEpoch":frame["hostEpoch"],"targetMode":frame["targetMode"]});
    atomic_json(&path.with_extension("json"), &frame)?;
    Ok(frame)
}

pub(super) async fn capture(p: &ScopeProject, args: &Value) -> Result<Value, String> {
    let id = args["observationId"].as_str().or_else(|| {
        args.pointer("/reference/resourceId")
            .and_then(Value::as_str)
    });
    if let Some(id) = id {
        safe_id(id)?;
        let path = safe_path(
            &p.project_root,
            &format!(".forge/editor/observations/{id}.json"),
        )?;
        let mut saved: Value = serde_json::from_slice(
            &std::fs::read(path).map_err(|e| format!("EDITOR_OBSERVATION_NOT_FOUND: {e}"))?,
        )
        .map_err(|e| e.to_string())?;
        let mut request = json!({"observationId":id});
        if let Some(region) = args
            .get("region")
            .or_else(|| args.pointer("/reference/selection/region"))
        {
            request["region"] = region.clone();
        }
        if let Some(point) = args.get("point") {
            request["point"] = point.clone();
        }
        match engine(p, "observation_resolve", request).await {
            Ok(current) => {
                for (key, value) in current.as_object().into_iter().flatten() {
                    saved[key] = value.clone();
                }
            }
            Err(e) => {
                saved["status"] = json!("historical");
                saved["staleForEditing"] = json!(true);
                saved["resolutionError"] = json!(e);
                saved["candidates"] = json!([]);
            }
        }
        return Ok(saved);
    }
    let mut request = args.clone();
    if let Some(reference) = args.get("reference").filter(|r| r["sceneGuid"].is_string()) {
        request["expected"] = json!({"sceneGuid":reference["sceneGuid"],"hostEpoch":reference["hostEpoch"],"contentRevision":reference["revision"],"targetMode":reference["targetMode"]});
    }
    request["width"] = json!(args["width"].as_u64().unwrap_or(960).clamp(64, 1920));
    request["height"] = json!(args["height"].as_u64().unwrap_or(540).clamp(64, 1080));
    persist(p, engine(p, "observation_capture", request).await?)
}

pub(super) fn images(scope: &ScopeContext, value: &Value) -> Vec<String> {
    let Some(id) = value["observationId"]
        .as_str()
        .filter(|id| safe_id(id).is_ok())
    else {
        return Vec::new();
    };
    let workspace = value
        .pointer("/reference/workspaceId")
        .and_then(Value::as_str)
        .unwrap_or(scope.current.id());
    let Some(p) = scope.find(workspace) else {
        return Vec::new();
    };
    let Ok(path) = safe_path(
        &p.project_root,
        &format!(".forge/editor/observations/{id}.png"),
    ) else {
        return Vec::new();
    };
    match std::fs::read(path) {
        Ok(bytes) if bytes.len() <= 4 * 1024 * 1024 => vec![format!(
            "data:image/png;base64,{}",
            base64::engine::general_purpose::STANDARD.encode(bytes)
        )],
        _ => Vec::new(),
    }
}

pub(super) async fn image_route(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    Query(q): Query<WorkspaceQuery>,
) -> Response {
    let result = (|| {
        safe_id(&id)?;
        let p = checked_project(&state, q.workspace_id.as_deref())?;
        let path = safe_path(
            &p.project_root,
            &format!(".forge/editor/observations/{id}.png"),
        )?;
        std::fs::read(path).map_err(|e| format!("EDITOR_OBSERVATION_NOT_FOUND: {e}"))
    })();
    match result {
        Ok(bytes) => (
            [
                (axum::http::header::CONTENT_TYPE, "image/png"),
                (
                    axum::http::header::CACHE_CONTROL,
                    "private, max-age=31536000, immutable",
                ),
            ],
            bytes,
        )
            .into_response(),
        Err(e) => response(Err(e)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn frame_dimensions_are_validated_before_writing() {
        let p = ScopeProject::for_test("unused", "unused");
        assert!(persist(&p, json!({"width":u64::MAX,"height":2,"pixelsB64":""})).is_err());
        assert!(persist(&p, json!({"width":1,"height":1,"pixelsB64":"AA=="})).is_err());
    }
}
