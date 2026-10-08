//! Editor-only candidate acceptance + scene assembly. Ordinary generation still returns candidates.
//! Exact `asset:<candidate-id>` values in ops become GUIDs; bindings.assets entries
//! `{candidateId: "..."}` become verified `{guid,path,version}` receipts.
use super::*;
use assetd::{meta::MetaDoc, project::ForgeProject};
use std::collections::BTreeMap;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Candidate {
    id: String,
    kind: String,
    file_ref: String,
    source_hash: String,
    dest_folder: String,
    name: String,
    path: String,
    asset: Option<Value>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Receipt {
    version: u32,
    change_set_id: String,
    request_hash: String,
    status: String,
    candidates: Vec<Candidate>,
    assembly: Option<Value>,
    error: Option<String>,
}
fn hash(value: &Value) -> String {
    forge_util::hashutil::sha256_hex(value.to_string().as_bytes())
}
fn journal(root: &FsPath, id: &str) -> Result<PathBuf, String> {
    safe_id(id)?;
    safe_path(root, &format!(".forge/editor/generation/{id}.json"))
}
fn gate(root: &FsPath, id: &str) -> Result<std::fs::File, String> {
    let path = journal(root, id)?.with_extension("lock");
    std::fs::create_dir_all(path.parent().unwrap()).map_err(|e| e.to_string())?;
    let file = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(path)
        .map_err(|e| e.to_string())?;
    fs2::FileExt::lock_exclusive(&file).map_err(|e| e.to_string())?;
    Ok(file)
}
fn load(path: &FsPath) -> Result<Receipt, String> {
    let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
    if bytes.len() > 8 * 1024 * 1024 {
        return Err("EDITOR_GENERATION_RECEIPT_TOO_LARGE".into());
    }
    serde_json::from_slice(&bytes).map_err(|e| format!("EDITOR_GENERATION_RECEIPT_INVALID: {e}"))
}
fn save(path: &FsPath, receipt: &Receipt) -> Result<(), String> {
    atomic_json(path, &json!(receipt))
}
fn asset_path(p: &ForgeProject, relative: &str) -> Result<PathBuf, String> {
    let content = p.content_root();
    let within_project = content
        .strip_prefix(&p.root)
        .map_err(|_| "EDITOR_PATH_OUTSIDE_ROOT")?
        .join(relative);
    let safe = safe_path(&p.root, &within_project.to_string_lossy())?;
    p.resolve_content_path(relative)
        .map_err(|e| e.to_string())?;
    Ok(safe)
}
fn file_hash(path: &FsPath) -> Result<String, String> {
    forge_util::hashutil::sha256_file(path).map_err(|e| e.to_string())
}
fn assets(receipt: &Receipt) -> BTreeMap<String, Value> {
    receipt
        .candidates
        .iter()
        .filter_map(|c| c.asset.clone().map(|asset| (c.id.clone(), asset)))
        .collect()
}
fn expanded(args: &Value, receipt: &Receipt) -> Result<Value, String> {
    fn replace(value: &mut Value, assets: &BTreeMap<String, Value>) -> Result<(), String> {
        match value {
            Value::String(s) if s.starts_with("asset:") => {
                let id = &s[6..];
                let asset = assets
                    .get(id)
                    .ok_or_else(|| format!("EDITOR_CANDIDATE_UNKNOWN: {id}"))?;
                *value = asset["guid"].clone();
            }
            Value::Array(items) => {
                for item in items {
                    replace(item, assets)?;
                }
            }
            Value::Object(fields) => {
                for item in fields.values_mut() {
                    replace(item, assets)?;
                }
            }
            _ => {}
        }
        Ok(())
    }
    let map = assets(receipt);
    let mut request = args.clone();
    request
        .as_object_mut()
        .ok_or("EDITOR_REQUEST_INVALID")?
        .remove("candidates");
    if request["ops"].is_null() {
        request["ops"] = json!([]);
    }
    replace(&mut request["ops"], &map)?;
    if let Some(bindings) = request.get_mut("bindings").and_then(Value::as_array_mut) {
        for binding in bindings {
            if let Some(entries) = binding.get_mut("assets").and_then(Value::as_array_mut) {
                for entry in entries {
                    if let Some(id) = entry.get("candidateId").and_then(Value::as_str) {
                        *entry = map
                            .get(id)
                            .ok_or_else(|| format!("EDITOR_CANDIDATE_UNKNOWN: {id}"))?
                            .clone();
                    }
                }
            }
        }
    }
    Ok(request)
}
fn prepare(p: &ForgeProject, args: &Value) -> Result<Receipt, String> {
    let id = args["changeSetId"]
        .as_str()
        .ok_or("EDITOR_CHANGESET_ID_REQUIRED")?;
    safe_id(id)?;
    if (!args["ops"].is_null() && !args["ops"].is_array())
        || (!args["bindings"].is_null() && !args["bindings"].is_array())
    {
        return Err("EDITOR_ASSEMBLY_OPS_AND_BINDINGS_MUST_BE_ARRAYS".into());
    }
    let assembling = args["ops"].as_array().is_some_and(|a| !a.is_empty())
        || args["bindings"].as_array().is_some_and(|a| !a.is_empty());
    if assembling && !args["expected"].is_object() {
        return Err("EDITOR_ASSEMBLY_EXPECTED_AND_OPS_REQUIRED".into());
    }
    let candidates = args["candidates"]
        .as_array()
        .filter(|v| !v.is_empty() && v.len() <= 32)
        .ok_or("EDITOR_CANDIDATES_REQUIRED: 1..32 candidates")?;
    let mut plans = Vec::new();
    let mut ids = std::collections::BTreeSet::new();
    for item in candidates {
        let candidate_id = item["id"].as_str().ok_or("EDITOR_CANDIDATE_ID_REQUIRED")?;
        safe_id(candidate_id)?;
        if !ids.insert(candidate_id) {
            return Err("EDITOR_DUPLICATE_CANDIDATE_ID".into());
        }
        let kind = item["kind"]
            .as_str()
            .filter(|s| matches!(*s, "image" | "mesh"))
            .ok_or("EDITOR_CANDIDATE_KIND: image or mesh required")?;
        let file_ref = item["fileRef"]
            .as_str()
            .ok_or("EDITOR_CANDIDATE_FILE_REQUIRED")?
            .replace('\\', "/");
        if !file_ref.starts_with(".forge/tmp/gen/") {
            return Err("EDITOR_CANDIDATE_OUTSIDE_GENERATION".into());
        }
        let source = safe_path(&p.root, &file_ref)?;
        let extension = source
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();
        if !(kind == "image" && matches!(extension.as_str(), "png" | "jpg" | "jpeg")
            || kind == "mesh" && matches!(extension.as_str(), "glb" | "gltf"))
        {
            return Err("EDITOR_CANDIDATE_TYPE_MISMATCH".into());
        }
        let stem = item["name"].as_str().unwrap_or(candidate_id);
        if stem.is_empty()
            || stem.len() > 128
            || stem.contains(['/', '\\', ':', '.'])
            || stem.trim() != stem
        {
            return Err("EDITOR_CANDIDATE_NAME_INVALID".into());
        }
        let suffix = hash(&json!([id, candidate_id]));
        let name = format!("{stem}-{}", &suffix[..12]);
        let dest = item["destFolder"].as_str().unwrap_or(if kind == "image" {
            "Textures/Generated"
        } else {
            "Meshes/Generated"
        });
        let dest_folder = assetd::normalize_rel(dest).map_err(|e| e.to_string())?;
        if dest_folder.is_empty() {
            return Err("EDITOR_CANDIDATE_DESTINATION_REQUIRED".into());
        }
        let path = format!("{dest_folder}/{name}.{extension}");
        if asset_path(p, &path)?.exists() {
            return Err(format!("EDITOR_GENERATION_DESTINATION_EXISTS: {path}"));
        }
        // accept_asset stages this exact file name before import; enforce its path as well.
        safe_path(&p.root, &format!(".forge/tmp/gen/{name}.{extension}"))?;
        plans.push(Candidate {
            id: candidate_id.into(),
            kind: kind.into(),
            file_ref,
            source_hash: file_hash(&source)?,
            dest_folder,
            name,
            path,
            asset: None,
        });
    }
    let mut request = args.clone();
    request.as_object_mut().unwrap().remove("source");
    let receipt = Receipt {
        version: 1,
        change_set_id: id.into(),
        request_hash: hash(&request),
        status: "importing".into(),
        candidates: plans,
        assembly: None,
        error: None,
    };
    // Validate every placeholder before the first side effect.
    let mut synthetic = receipt.clone();
    for plan in &mut synthetic.candidates {
        plan.asset =
            Some(json!({"guid":"placeholder","path":plan.path,"version":plan.source_hash}));
    }
    expanded(args, &synthetic)?;
    Ok(receipt)
}

fn verify_asset(p: &ForgeProject, candidate: &Candidate) -> Result<Value, String> {
    let path = asset_path(p, &candidate.path)?;
    if file_hash(&path)? != candidate.source_hash {
        return Err(format!("EDITOR_IMPORTED_ASSET_CHANGED: {}", candidate.path));
    }
    let meta_path = assetd::meta_path_for(&p.content_root(), &candidate.path);
    safe_path(
        &p.root,
        &meta_path
            .strip_prefix(&p.root)
            .map_err(|_| "EDITOR_PATH_OUTSIDE_ROOT")?
            .to_string_lossy(),
    )?;
    let meta = MetaDoc::load(&meta_path).map_err(|e| e.to_string())?;
    let asset = json!({"guid":meta.guid,"path":candidate.path,"version":candidate.source_hash});
    if candidate.asset.as_ref().is_some_and(|old| *old != asset) {
        return Err("EDITOR_IMPORTED_ASSET_IDENTITY_CHANGED".into());
    }
    Ok(asset)
}

/// Independent import journal: a later scene conflict never loses or re-imports completed assets.
fn accept_candidates(root: &FsPath, args: &Value) -> Result<Receipt, String> {
    let id = args["changeSetId"]
        .as_str()
        .ok_or("EDITOR_CHANGESET_ID_REQUIRED")?;
    let _gate = gate(root, id)?;
    let path = journal(root, id)?;
    let p = if root.join("forge.toml").exists() {
        ForgeProject::load(root).map_err(|e| e.to_string())?
    } else {
        ForgeProject::with_defaults(root.to_owned())
    };
    let content = p.content_root();
    let content = safe_path(
        root,
        &content
            .strip_prefix(root)
            .map_err(|_| "EDITOR_PATH_OUTSIDE_ROOT")?
            .to_string_lossy(),
    )?;
    std::fs::create_dir_all(content).map_err(|e| e.to_string())?;
    let mut request = args.clone();
    request
        .as_object_mut()
        .ok_or("EDITOR_REQUEST_INVALID")?
        .remove("source");
    let mut receipt = if path.exists() {
        let receipt = load(&path)?;
        if receipt.request_hash != hash(&request) {
            return Err(
                "IDEMPOTENCY_MISMATCH: generation changeSetId was used for another request".into(),
            );
        }
        receipt
    } else {
        let receipt = prepare(&p, args)?;
        save(&path, &receipt)?;
        receipt
    };
    for index in 0..receipt.candidates.len() {
        let c = receipt.candidates[index].clone();
        if c.asset.is_some() {
            verify_asset(&p, &c)?;
            continue;
        }
        let source = safe_path(root, &c.file_ref)?;
        if file_hash(&source)? != c.source_hash {
            return Err("EDITOR_CANDIDATE_CHANGED: candidate changed before acceptance".into());
        }
        let destination = asset_path(&p, &c.path)?;
        let ext = destination
            .extension()
            .and_then(|s| s.to_str())
            .ok_or("EDITOR_CANDIDATE_TYPE_MISMATCH")?;
        safe_path(root, &format!(".forge/tmp/gen/{}.{ext}", c.name))?;
        safe_path(root, &format!("{}.json", c.file_ref))?;
        let meta_path = assetd::meta_path_for(&p.content_root(), &c.path);
        safe_path(
            root,
            &meta_path
                .strip_prefix(root)
                .map_err(|_| "EDITOR_PATH_OUTSIDE_ROOT")?
                .to_string_lossy(),
        )?;
        // A previous import may have completed before its receipt was persisted. Recover
        // only matching bytes; never overwrite someone else's changed destination.
        if destination.exists() && file_hash(&destination)? != c.source_hash {
            return Err("EDITOR_GENERATION_DESTINATION_CONFLICT".into());
        }
        let mut detail = gend::tmpstore::load_sidecar(&p, &c.file_ref)
            .unwrap_or_else(|| json!({"backendId":"user-provided","sourceRefs":[c.file_ref]}));
        if !detail.is_object() {
            detail = json!({"generation":detail});
        }
        detail["editorAssembly"] =
            json!({"changeSetId":id,"candidateId":c.id,"sourceHash":c.source_hash});
        gend::accept::accept_asset(
            &p,
            &c.file_ref,
            &c.dest_folder,
            &c.name,
            if c.kind == "image" {
                "gen-image"
            } else {
                "gen-model"
            },
            detail,
            None,
        )
        .map_err(|e| e.to_string())?;
        receipt.candidates[index].asset = Some(verify_asset(&p, &c)?);
        save(&path, &receipt)?;
    }
    if matches!(receipt.status.as_str(), "importing" | "import_failed") {
        receipt.status = "assets_imported".into();
        receipt.error = None;
        save(&path, &receipt)?;
    }
    Ok(receipt)
}

pub(super) async fn accept_and_assemble(p: &ScopeProject, args: &Value) -> Result<Value, String> {
    let root = p.project_root.clone();
    let input = args.clone();
    let accepted = tokio::task::spawn_blocking(move || accept_candidates(&root, &input))
        .await
        .map_err(|e| e.to_string())?;
    let mut receipt = match accepted {
        Ok(receipt) => receipt,
        Err(error) => {
            let root = p.project_root.clone();
            let input = args.clone();
            let failure = error.clone();
            let partial =
                tokio::task::spawn_blocking(move || -> Result<Option<Receipt>, String> {
                    let Some(id) = input["changeSetId"].as_str() else {
                        return Ok(None);
                    };
                    let path = journal(&root, id)?;
                    if !path.exists() {
                        return Ok(None);
                    }
                    let _gate = gate(&root, id)?;
                    let mut receipt = load(&path)?;
                    let mut request = input.clone();
                    request
                        .as_object_mut()
                        .ok_or("EDITOR_REQUEST_INVALID")?
                        .remove("source");
                    if hash(&request) != receipt.request_hash {
                        return Ok(None);
                    }
                    receipt.status = "import_failed".into();
                    receipt.error = Some(failure);
                    save(&path, &receipt)?;
                    Ok(Some(receipt))
                })
                .await
                .map_err(|e| e.to_string())??;
            if let Some(receipt) = partial {
                emit(
                    &p.project_root,
                    "editor.changed",
                    json!({"domains":["assets"],"changeSetId":receipt.change_set_id,"status":"import_failed"}),
                );
                return Ok(
                    json!({"ok":false,"changeSetId":receipt.change_set_id,"status":"import_failed","assets":assets(&receipt),"error":receipt.error}),
                );
            }
            return Err(error);
        }
    };
    let request = expanded(args, &receipt)?;
    if request["ops"].as_array().is_some_and(Vec::is_empty)
        && request["bindings"].as_array().is_none_or(Vec::is_empty)
    {
        emit(
            &p.project_root,
            "editor.changed",
            json!({"domains":["assets"],"changeSetId":receipt.change_set_id}),
        );
        return Ok(
            json!({"ok":true,"changeSetId":receipt.change_set_id,"status":"assets_imported","assets":assets(&receipt)}),
        );
    }
    // This keeps scene/PIE identity gates, atomic host ops, blueprint binding checks,
    // pending recovery, and undo/redo on exactly the same path as editor_apply.
    match changes::apply(p, &request).await {
        Ok(assembly) => {
            receipt.status = assembly["status"]
                .as_str()
                .unwrap_or("assets_imported")
                .to_string();
            receipt.assembly = Some(assembly);
            receipt.error = None;
        }
        Err(error) => {
            receipt.status = "assets_imported_scene_conflict".into();
            receipt.error = Some(error);
        }
    }
    let root = p.project_root.clone();
    let persisted = receipt.clone();
    tokio::task::spawn_blocking(move || {
        let _gate = gate(&root, &persisted.change_set_id)?;
        save(&journal(&root, &persisted.change_set_id)?, &persisted)
    })
    .await
    .map_err(|e| e.to_string())??;
    emit(
        &p.project_root,
        "editor.changed",
        json!({"domains":["assets","scene","blueprint"],"changeSetId":receipt.change_set_id,"status":receipt.status}),
    );
    Ok(
        json!({"ok":receipt.error.is_none(),"changeSetId":receipt.change_set_id,"status":receipt.status,"assets":assets(&receipt),"assembly":receipt.assembly,"error":receipt.error}),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn assets_only_needs_no_scene_and_partial_import_returns_completed_receipts() {
        let root = std::env::temp_dir().join(crate::events::new_id("editor-generation-assets"));
        std::fs::create_dir_all(root.join(".forge/tmp/gen")).unwrap();
        image::RgbaImage::from_pixel(2, 2, image::Rgba([0, 255, 0, 255]))
            .save(root.join(".forge/tmp/gen/green.png"))
            .unwrap();
        std::fs::write(root.join(".forge/tmp/gen/broken.png"), b"not an image").unwrap();
        let project = ScopeProject::for_test(&root.to_string_lossy(), &root.to_string_lossy());
        let first = json!({"id":"paint","kind":"image","fileRef":".forge/tmp/gen/green.png","name":"Paint"});
        let args = json!({"changeSetId":"assets-only","candidates":[first],"ops":[],"bindings":[]});
        let imported = accept_and_assemble(&project, &args).await.unwrap();
        assert_eq!(imported["status"], "assets_imported");
        assert_eq!(imported["ok"], true);
        assert_eq!(
            accept_and_assemble(&project, &args).await.unwrap(),
            imported
        );
        let partial = json!({"changeSetId":"partial","candidates":[first,{"id":"broken","kind":"image","fileRef":".forge/tmp/gen/broken.png","name":"Broken"}],"ops":[],"bindings":[]});
        let result = accept_and_assemble(&project, &partial).await.unwrap();
        assert_eq!(result["ok"], false);
        assert_eq!(result["status"], "import_failed");
        assert!(result["assets"]["paint"]["guid"].is_string());
        assert!(result["assets"]["broken"].is_null());
        let retry = accept_and_assemble(&project, &partial).await.unwrap();
        assert_eq!(retry["assets"], result["assets"]);
        assert!(root.starts_with(std::env::temp_dir()));
        assert!(!std::fs::symlink_metadata(&root)
            .unwrap()
            .file_type()
            .is_symlink());
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn real_candidate_import_is_idempotent_and_binding_placeholders_use_receipts() {
        let root = std::env::temp_dir().join(crate::events::new_id("editor-generation-test"));
        std::fs::create_dir_all(root.join(".forge/tmp/gen")).unwrap();
        image::RgbaImage::from_pixel(2, 2, image::Rgba([255, 0, 0, 255]))
            .save(root.join(".forge/tmp/gen/red.png"))
            .unwrap();
        let args = json!({"changeSetId":"assemble-test","expected":{"sceneGuid":"test","hostEpoch":"test","contentRevision":1,"targetMode":"edit"},"candidates":[{"id":"paint","kind":"image","fileRef":".forge/tmp/gen/red.png","name":"Paint"}],"ops":[{"op":"create","clientId":"sprite","components":[{"type":"Sprite","props":{"texture":"asset:paint"}}]}],"bindings":[{"boardId":"main","nodeId":"sprite","expectedRevision":1,"clientId":"sprite","assets":[{"candidateId":"paint"}]}]});
        let first = accept_candidates(&root, &args).unwrap();
        let asset = first.candidates[0].asset.clone().unwrap();
        let content = root.join("Content").join(asset["path"].as_str().unwrap());
        let modified = std::fs::metadata(&content).unwrap().modified().unwrap();
        let second = accept_candidates(&root, &args).unwrap();
        assert_eq!(assets(&first), assets(&second));
        assert_eq!(
            std::fs::metadata(&content).unwrap().modified().unwrap(),
            modified
        );
        let request = expanded(&args, &first).unwrap();
        assert_eq!(
            request["ops"][0]["components"][0]["props"]["texture"],
            asset["guid"]
        );
        assert_eq!(request["bindings"][0]["assets"][0], asset);
        assert!(request.get("candidates").is_none());
        let mut different = args.clone();
        different["ops"][0]["name"] = json!("different");
        assert!(accept_candidates(&root, &different)
            .unwrap_err()
            .contains("IDEMPOTENCY_MISMATCH"));
        std::fs::write(&content, b"changed by human").unwrap();
        assert!(accept_candidates(&root, &args)
            .unwrap_err()
            .contains("EDITOR_IMPORTED_ASSET_CHANGED"));
        assert!(root.starts_with(std::env::temp_dir()));
        assert!(!std::fs::symlink_metadata(&root)
            .unwrap()
            .file_type()
            .is_symlink());
        std::fs::remove_dir_all(root).unwrap();
    }
}
