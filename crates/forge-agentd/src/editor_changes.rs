//! Recoverable scene + blueprint commands. Assets remain registered when a binding is undone.
//! Each document patch owns one binding leaf; unrelated human edits are never restored from snapshots.
use super::*;
use std::collections::{BTreeMap, HashSet};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct BindingPatch {
    board_id: String,
    node_id: String,
    before_exists: bool,
    before: Value,
    after: Value,
    spec: Value,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Record {
    version: u32,
    id: String,
    change_set_id: String,
    status: String,
    request_hash: String,
    request: Value,
    patches: Vec<BindingPatch>,
    result: Value,
    #[serde(default)]
    source: Value,
    #[serde(default)]
    action_expected: Value,
    #[serde(default)]
    action_result: Value,
    #[serde(default)]
    last_error: Option<String>,
}

fn fingerprint(value: &Value) -> String {
    forge_util::hashutil::sha256_hex(value.to_string().as_bytes())
}
fn journal_path(root: &FsPath, id: &str) -> Result<PathBuf, String> {
    safe_id(id)?;
    safe_path(root, &format!(".forge/editor/changes/{id}.json"))
}
fn load(root: &FsPath, id: &str) -> Result<Record, String> {
    let bytes = std::fs::read(journal_path(root, id)?).map_err(|e| e.to_string())?;
    if bytes.len() > 8 * 1024 * 1024 {
        return Err("EDITOR_DOCUMENT_TOO_LARGE".into());
    }
    serde_json::from_slice(&bytes).map_err(|e| format!("EDITOR_JOURNAL_NEEDS_RECONCILE: {e}"))
}
fn save(root: &FsPath, record: &Record) -> Result<(), String> {
    atomic_json(&journal_path(root, &record.id)?, &json!(record))
}
fn failed(root: &FsPath, record: &mut Record, status: &str, error: String) -> String {
    record.status = status.into();
    record.last_error = Some(error.clone());
    if let Err(save_error) = save(root, record) {
        return format!("{error}; journal persistence failed: {save_error}");
    }
    emit(root, "editor.change_set", json!(record));
    error
}
fn expected_from(value: &Value) -> Value {
    json!({"sceneGuid":value["sceneGuid"],"hostEpoch":value["hostEpoch"],
        "contentRevision":value["contentRevision"],"targetMode":value["targetMode"]})
}
fn validate_expected(value: &Value) -> Result<(), String> {
    if value["sceneGuid"].as_str().is_none()
        || value["hostEpoch"].as_str().is_none()
        || value["contentRevision"].as_u64().is_none()
        || value["targetMode"] != "edit"
    {
        return Err("EDITOR_EXPECTED_REQUIRED: an edit sceneGuid, hostEpoch and contentRevision are required".into());
    }
    Ok(())
}
fn readiness(expected: &Value, current: &Value) -> Result<Option<&'static str>, String> {
    validate_expected(expected)?;
    if expected["hostEpoch"] != current["hostEpoch"] {
        return Err(
            "STALE_HOST: saved command belongs to another host lifetime; reconcile explicitly"
                .into(),
        );
    }
    if expected["sceneGuid"] != current["sceneGuid"] {
        return Ok(Some("pending_target_scene"));
    }
    if current["targetMode"] != "edit" {
        return Ok(Some("pending_edit_mode"));
    }
    if expected["contentRevision"] != current["contentRevision"] {
        return Err("CONTENT_CONFLICT: target scene changed since the command was prepared".into());
    }
    Ok(None)
}
fn node_exists(document: &Value, node: &str) -> bool {
    document["nodes"]
        .as_array()
        .is_some_and(|nodes| nodes.iter().any(|n| n["id"] == node))
        || document["nodes"]
            .as_object()
            .is_some_and(|nodes| nodes.contains_key(node))
}
fn validate_assets(root: &FsPath, assets: &Value) -> Result<(), String> {
    if assets.is_null() {
        return Ok(());
    }
    let assets = assets
        .as_array()
        .ok_or("EDITOR_ASSET_REFERENCE_INVALID: assets must be an array")?;
    if assets.is_empty() {
        return Ok(());
    }
    let project = assetd::project::ForgeProject::load(root)
        .map_err(|e| format!("EDITOR_ASSET_REFERENCE_INVALID: {e}"))?;
    let content = project.content_root();
    let content_relative = content
        .strip_prefix(root)
        .map_err(|_| "EDITOR_ASSET_REFERENCE_INVALID: content root is outside project")?
        .to_string_lossy()
        .replace('\\', "/");
    let prefix = format!("{content_relative}/");
    for asset in assets {
        let guid = asset["guid"]
            .as_str()
            .filter(|v| !v.is_empty())
            .ok_or("EDITOR_ASSET_REFERENCE_INVALID: guid required")?;
        let supplied = asset["path"]
            .as_str()
            .filter(|v| !v.is_empty())
            .ok_or("EDITOR_ASSET_REFERENCE_INVALID: path required")?
            .replace('\\', "/");
        let relative = assetd::normalize_rel(supplied.strip_prefix(&prefix).unwrap_or(&supplied))
            .map_err(|e| format!("EDITOR_ASSET_REFERENCE_INVALID: {e}"))?;
        let version = asset["version"]
            .as_str()
            .filter(|v| v.len() == 64 && v.bytes().all(|b| b.is_ascii_hexdigit()))
            .ok_or("EDITOR_ASSET_REFERENCE_INVALID: version must be a SHA-256 file hash")?;
        let path = safe_path(root, &format!("{content_relative}/{relative}"))?;
        project
            .resolve_content_path(&relative)
            .map_err(|e| format!("EDITOR_ASSET_REFERENCE_INVALID: {e}"))?;
        let meta_path = safe_path(root, &format!("{content_relative}/{relative}.meta"))?;
        let meta = assetd::meta::MetaDoc::load(&meta_path)
            .map_err(|e| format!("EDITOR_ASSET_REFERENCE_NOT_FOUND: {e}"))?;
        if meta.guid != guid {
            return Err(
                "EDITOR_ASSET_REFERENCE_MISMATCH: GUID does not identify the supplied asset path"
                    .into(),
            );
        }
        let actual = forge_util::hashutil::sha256_file(&path)
            .map_err(|e| format!("EDITOR_ASSET_REFERENCE_NOT_FOUND: {e}"))?;
        if actual != version.to_ascii_lowercase() {
            return Err(
                "EDITOR_ASSET_VERSION_CONFLICT: asset bytes changed since the version was selected"
                    .into(),
            );
        }
    }
    Ok(())
}
fn prepare_patches(root: &FsPath, request: &Value) -> Result<Vec<BindingPatch>, String> {
    let Some(bindings) = request.get("bindings") else {
        return Ok(Vec::new());
    };
    let bindings = bindings
        .as_array()
        .ok_or("EDITOR_BINDINGS_INVALID: array required")?;
    let mut keys = HashSet::new();
    let mut patches = Vec::new();
    for spec in bindings {
        validate_assets(root, &spec["assets"])?;
        let board = spec["boardId"].as_str().unwrap_or("main");
        let node = spec["nodeId"]
            .as_str()
            .filter(|n| !n.is_empty())
            .ok_or("EDITOR_BINDING_NODE_REQUIRED")?;
        if !keys.insert((board.to_owned(), node.to_owned())) {
            return Err(
                "EDITOR_BINDING_DUPLICATE: only one binding per board node is allowed".into(),
            );
        }
        let envelope = read_document(root, "blueprint", board)?;
        let doc = &envelope["document"];
        if !node_exists(doc, node) {
            return Err(format!("EDITOR_BINDING_NODE_NOT_FOUND: {board}/{node}"));
        }
        if let Some(expected) = spec.get("expectedRevision") {
            if expected != &envelope["revision"] {
                return Err("EDITOR_REVISION_CONFLICT: blueprint changed".into());
            }
        }
        if !doc["bindings"].is_null() && !doc["bindings"].is_object() {
            return Err("EDITOR_BINDINGS_INVALID: document bindings must be an object".into());
        }
        let before = doc["bindings"].get(node);
        if before.is_some_and(|value| !value.is_null() && !value.is_object()) {
            return Err("EDITOR_BINDINGS_INVALID: a binding must be an object or null".into());
        }
        // Verify the target selector before any scene write, including missing create selectors.
        if let Some(client) = spec["clientId"].as_str() {
            let count = request["ops"].as_array().map_or(0, |ops| {
                ops.iter()
                    .filter(|op| {
                        op["clientId"] == client
                            && matches!(op["op"].as_str(), Some("create" | "duplicate"))
                    })
                    .count()
            });
            if count != 1 {
                return Err(
                    "EDITOR_BINDING_TARGET_INVALID: clientId must select one creation".into(),
                );
            }
        } else if let Some(index) = spec["opIndex"].as_u64() {
            if !matches!(
                request["ops"]
                    .get(index as usize)
                    .and_then(|op| op["op"].as_str()),
                Some("create" | "duplicate")
            ) {
                return Err("EDITOR_BINDING_TARGET_INVALID: opIndex must select a creation".into());
            }
        } else if spec["target"]["entityGuid"].as_str().is_none() {
            return Err(
                "EDITOR_BINDING_TARGET_REQUIRED: clientId, opIndex or target.entityGuid required"
                    .into(),
            );
        }
        patches.push(BindingPatch {
            board_id: board.into(),
            node_id: node.into(),
            before_exists: before.is_some(),
            before: before.cloned().unwrap_or(Value::Null),
            after: Value::Null,
            spec: spec.clone(),
        });
    }
    Ok(patches)
}
fn fill_targets(record: &mut Record) -> Result<(), String> {
    for patch in &mut record.patches {
        let spec = &patch.spec;
        let target = if spec["clientId"].is_string() || spec["opIndex"].is_u64() {
            let found: Vec<_> = record.result["created"]
                .as_array()
                .into_iter()
                .flatten()
                .filter(|created| {
                    if spec["clientId"].is_string() {
                        created["clientId"] == spec["clientId"]
                    } else {
                        created["opIndex"] == spec["opIndex"]
                    }
                })
                .collect();
            if found.len() != 1 {
                return Err(
                    "EDITOR_BINDING_TARGET_INVALID: host creation mapping is missing or ambiguous"
                        .into(),
                );
            }
            found[0].clone()
        } else {
            spec["target"].clone()
        };
        if target["entityGuid"].as_str().is_none() {
            return Err("EDITOR_BINDING_TARGET_INVALID: entityGuid missing".into());
        }
        let entity_id = target
            .get("id")
            .or_else(|| target.get("entityId"))
            .cloned()
            .unwrap_or(Value::Null);
        let mut after = patch.before.as_object().cloned().unwrap_or_default();
        let fields = json!({"sceneGuid":record.result["sceneGuid"],"entityGuid":target["entityGuid"],
            "entityId":entity_id,"assets":spec["assets"],"changeSetId":record.id});
        after.extend(fields.as_object().unwrap().clone());
        patch.after = Value::Object(after);
    }
    Ok(())
}
fn patch_document(
    document: &mut Value,
    patch: &BindingPatch,
    forward: bool,
    allow_done: bool,
) -> Result<bool, String> {
    if !node_exists(document, &patch.node_id) {
        return Err(format!(
            "EDITOR_BINDING_NODE_NOT_FOUND: {}/{}",
            patch.board_id, patch.node_id
        ));
    }
    let before = patch.before_exists.then_some(&patch.before);
    let after = Some(&patch.after);
    let (source, target) = if forward {
        (before, after)
    } else {
        (after, before)
    };
    let current = document["bindings"].get(&patch.node_id);
    if current.is_some_and(|v| !v.is_null() && !v.is_object()) {
        return Err("EDITOR_BINDINGS_INVALID".into());
    }
    let mut next = current
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    // Own only keys that this command changed, including each key's absence.
    let keys: std::collections::BTreeSet<_> = source
        .into_iter()
        .chain(target)
        .filter_map(Value::as_object)
        .flat_map(|m| m.keys())
        .collect();
    for key in keys {
        let from = source.and_then(|v| v.get(key));
        let to = target.and_then(|v| v.get(key));
        if from == to {
            continue;
        }
        let value = current.and_then(|v| v.get(key));
        if allow_done && value == to {
            continue;
        }
        if value != from {
            return Err(format!(
                "EDITOR_BINDING_CONFLICT: {}/{}/{} changed",
                patch.board_id, patch.node_id, key
            ));
        }
        if let Some(value) = to {
            next.insert(key.clone(), value.clone());
        } else {
            next.remove(key);
        }
    }
    let next = if next.is_empty() {
        target.map(|value| {
            if value.is_null() {
                Value::Null
            } else {
                json!({})
            }
        })
    } else {
        Some(Value::Object(next))
    };
    if next.as_ref() == current {
        return Ok(false);
    }
    if document["bindings"].is_null() {
        document["bindings"] = json!({});
    }
    let bindings = document["bindings"]
        .as_object_mut()
        .ok_or("EDITOR_BINDINGS_INVALID")?;
    if let Some(value) = next {
        bindings.insert(patch.node_id.clone(), value);
    } else {
        bindings.remove(&patch.node_id);
    }
    Ok(true)
}
// Validate every affected field before starting a multi-document commit. The writer lock is held.
fn plan_documents(
    root: &FsPath,
    patches: &[BindingPatch],
    forward: bool,
    allow_done: bool,
) -> Result<Vec<(String, Value, u64)>, String> {
    let mut documents = BTreeMap::<String, (Value, u64, bool)>::new();
    for patch in patches {
        if !documents.contains_key(&patch.board_id) {
            let envelope = read_document(root, "blueprint", &patch.board_id)?;
            documents.insert(
                patch.board_id.clone(),
                (
                    envelope["document"].clone(),
                    envelope["revision"].as_u64().unwrap_or(0),
                    false,
                ),
            );
        }
        let (doc, _, changed) = documents.get_mut(&patch.board_id).unwrap();
        *changed |= patch_document(doc, patch, forward, allow_done)?;
    }
    Ok(documents
        .into_iter()
        .filter_map(|(id, (doc, rev, changed))| changed.then_some((id, doc, rev)))
        .collect())
}
fn commit_documents(root: &FsPath, plan: Vec<(String, Value, u64)>) -> Result<(), String> {
    for (id, doc, revision) in plan {
        write_document(root, "blueprint", &id, doc, Some(revision))?;
    }
    Ok(())
}
fn terminal(status: &str) -> bool {
    matches!(
        status,
        "applied" | "undone" | "conflict" | "needs_reconcile"
    )
}
fn returned_host_failure_status(error: &str) -> Option<&'static str> {
    let code = error.split(':').next().unwrap_or("");
    match code {
        "TARGET_SCENE_INACTIVE" => Some("pending_target_scene"),
        "TARGET_MODE_MISMATCH" => Some("pending_edit_mode"),
        "STALE_HOST" => Some("needs_reconcile"),
        "CONTENT_CONFLICT"
        | "COMMAND_CONFLICT"
        | "IDEMPOTENCY_MISMATCH"
        | "EDIT_APPLY_FAILED"
        | "HOST_ERROR" => Some("conflict"),
        code if code.starts_with("REFERENCE_")
            || code.starts_with("BINDING_")
            || code.starts_with("INVALID_") =>
        {
            Some("conflict")
        }
        _ => None,
    }
}

async fn resume_apply(p: &ScopeProject, record: &mut Record) -> Result<Value, String> {
    let root = &p.project_root;
    if terminal(&record.status) {
        if matches!(record.status.as_str(), "conflict" | "needs_reconcile") {
            return Err(record
                .last_error
                .clone()
                .unwrap_or_else(|| "EDITOR_RECOVERY_REQUIRED".into()));
        }
        return Ok(json!(record));
    }
    for patch in &record.patches {
        if let Err(error) = validate_assets(root, &patch.spec["assets"]) {
            return Err(failed(root, record, "conflict", error));
        }
    }
    if record.status == "scene_committed" {
        // A host restart or a subsequent undo must not attach a board to unknown scene contents.
        let receipt = engine(p, "editor_resolve", json!({"changeSetId":record.id})).await?;
        if receipt["hostEpoch"] != record.request["expected"]["hostEpoch"]
            || receipt["receiptState"] != "applied"
        {
            return Err(failed(
                root,
                record,
                "needs_reconcile",
                "EDITOR_RECOVERY_UNCERTAIN: committed scene command is no longer present".into(),
            ));
        }
    } else {
        let current = engine(p, "editor_resolve", json!({})).await?;
        // A prepared request may have reached the host before the daemon lost its response.
        let receipt = if record.status == "prepared" {
            Some(engine(p, "editor_resolve", json!({"changeSetId":record.id})).await?)
        } else {
            None
        };
        if current["hostEpoch"] != record.request["expected"]["hostEpoch"] {
            return Err(failed(
                root,
                record,
                "needs_reconcile",
                "STALE_HOST: command outcome requires explicit reconciliation after host restart"
                    .into(),
            ));
        }
        if let Some(receipt) = receipt.filter(|r| !r["receipt"].is_null()) {
            if receipt["requestHash"] != record.request_hash || receipt["receiptState"] != "applied"
            {
                return Err(failed(
                    root,
                    record,
                    "needs_reconcile",
                    "EDITOR_RECOVERY_UNCERTAIN: host receipt differs or was undone".into(),
                ));
            }
            record.result = receipt["receipt"].clone();
        } else {
            match readiness(&record.request["expected"], &current) {
                Ok(Some(status)) => {
                    record.status = status.into();
                    save(root, record)?;
                    return Ok(json!(record));
                }
                Err(error) => return Err(failed(root, record, "conflict", error)),
                Ok(None) => {}
            }
            // Revalidate only owned preimages before committing the scene.
            for patch in &record.patches {
                let envelope = read_document(root, "blueprint", &patch.board_id)?;
                let current_binding = envelope["document"]["bindings"].get(&patch.node_id);
                if !node_exists(&envelope["document"], &patch.node_id)
                    || current_binding.is_some_and(|v| !v.is_null() && !v.is_object())
                    || [
                        "sceneGuid",
                        "entityGuid",
                        "entityId",
                        "assets",
                        "changeSetId",
                    ]
                    .iter()
                    .any(|key| {
                        current_binding.and_then(|value| value.get(*key)) != patch.before.get(*key)
                    })
                {
                    return Err(failed(
                        root,
                        record,
                        "conflict",
                        "EDITOR_BINDING_CONFLICT: pending blueprint target changed".into(),
                    ));
                }
                if patch.spec["clientId"].is_null() && patch.spec["opIndex"].is_null() {
                    let target = engine(
                        p,
                        "editor_resolve",
                        json!({
                        "sceneGuid":record.request["expected"]["sceneGuid"],
                        "entityGuid":patch.spec["target"]["entityGuid"],"targetMode":"edit"}),
                    )
                    .await?;
                    if let Some(id) = patch.spec["target"]
                        .get("id")
                        .or_else(|| patch.spec["target"].get("entityId"))
                    {
                        if id != &target["id"] {
                            return Err(failed(
                                root,
                                record,
                                "conflict",
                                "EDITOR_BINDING_TARGET_INVALID: numeric id and GUID disagree"
                                    .into(),
                            ));
                        }
                    }
                }
            }
            record.status = "prepared".into();
            save(root, record)?;
            match engine(p, "editor_apply", record.request.clone()).await {
                Ok(result) => record.result = result,
                Err(error) => {
                    if let Some(status) = returned_host_failure_status(&error) {
                        if status.starts_with("pending_") {
                            record.status = status.into();
                            record.last_error = Some(error);
                            save(root, record)?;
                            return Ok(json!(record));
                        }
                        return Err(failed(root, record, status, error));
                    }
                    // Keep prepared: transport errors are ambiguous. A host receipt decides on recovery.
                    record.last_error = Some(error.clone());
                    save(root, record)?;
                    return Err(error);
                }
            }
        }
        if let Err(error) = fill_targets(record) {
            return Err(failed(root, record, "needs_reconcile", error));
        }
        record.status = "scene_committed".into();
        save(root, record)?;
    }
    let plan = match plan_documents(root, &record.patches, true, true) {
        Ok(plan) => plan,
        Err(error) => return Err(failed(root, record, "conflict", error)),
    };
    commit_documents(root, plan)?;
    record.status = "applied".into();
    record.last_error = None;
    save(root, record)?;
    emit(
        root,
        "editor.changed",
        json!({"domains":["scene","assets","blueprint"],"changeSetId":record.id,"result":record.result}),
    );
    Ok(json!(record))
}

pub(super) async fn apply(p: &ScopeProject, args: &Value) -> Result<Value, String> {
    if let Some(action) = args.get("action") {
        return match action.as_str() {
            Some("undo") => undo(p, args, false).await,
            Some("redo") => undo(p, args, true).await,
            _ => Err("EDITOR_ACTION_INVALID: expected undo or redo".into()),
        };
    }
    register(p);
    let b = bus(&p.project_root);
    let _guard = b.writes.lock().await;
    let id = args["changeSetId"]
        .as_str()
        .map(str::to_owned)
        .unwrap_or_else(|| crate::events::new_id("change"));
    safe_id(&id)?;
    let mut request = args.clone();
    if !request.is_object() {
        return Err("EDITOR_REQUEST_INVALID".into());
    }
    request["changeSetId"] = json!(id);
    let source = request
        .as_object_mut()
        .unwrap()
        .remove("source")
        .unwrap_or_else(|| json!({"kind":"editor"}));
    let hash = fingerprint(&request);
    let mut record = if journal_path(&p.project_root, &id)?.exists() {
        let previous = load(&p.project_root, &id)?;
        if previous.request_hash != hash {
            return Err("IDEMPOTENCY_MISMATCH: changeSetId was used for another request".into());
        }
        previous
    } else {
        validate_expected(&request["expected"])?;
        let patches = prepare_patches(&p.project_root, &request)?;
        let record = Record {
            source,
            version: 2,
            id: id.clone(),
            change_set_id: id,
            status: "pending_target_scene".into(),
            request_hash: hash,
            request,
            patches,
            result: Value::Null,
            action_expected: Value::Null,
            action_result: Value::Null,
            last_error: None,
        };
        save(&p.project_root, &record)?;
        record
    };
    resume_apply(p, &mut record).await
}

async fn resume_history(
    p: &ScopeProject,
    record: &mut Record,
    redo: bool,
) -> Result<Value, String> {
    let root = &p.project_root;
    if redo {
        for patch in &record.patches {
            if let Err(error) = validate_assets(root, &patch.spec["assets"]) {
                return Err(failed(root, record, "conflict", error));
            }
        }
    }
    let prepared = if redo {
        "redo_prepared"
    } else {
        "undo_prepared"
    };
    let committed = if redo { "scene_redone" } else { "scene_undone" };
    let target_state = if redo { "applied" } else { "undone" };
    let receipt = engine(p, "editor_resolve", json!({"changeSetId":record.id})).await?;
    if receipt["hostEpoch"] != record.request["expected"]["hostEpoch"] {
        return Err(failed(
            root,
            record,
            "needs_reconcile",
            "STALE_HOST: history command belongs to another host lifetime".into(),
        ));
    }
    if record.status == prepared {
        if receipt["receiptState"] != target_state {
            let source_state = if redo { "undone" } else { "applied" };
            if receipt["receiptState"] != source_state {
                return Err(failed(
                    root,
                    record,
                    "needs_reconcile",
                    "EDITOR_HISTORY_UNKNOWN: command is no longer in host history".into(),
                ));
            }
            // A retry still requires its original exact scene version and command head.
            let result = engine(p, if redo { "edit_redo" } else { "edit_undo" }, json!({
                "commandId":record.result["commandId"],"changeSetId":record.id,"expected":record.action_expected})).await?;
            record.action_result = result;
        }
        record.status = committed.into();
        save(root, record)?;
    } else if receipt["receiptState"] != target_state {
        return Err(failed(
            root,
            record,
            "needs_reconcile",
            "EDITOR_HISTORY_CHANGED: command changed while document recovery was pending".into(),
        ));
    }
    let plan = match plan_documents(root, &record.patches, redo, true) {
        Ok(plan) => plan,
        Err(error) => return Err(failed(root, record, "conflict", error)),
    };
    commit_documents(root, plan)?;
    record.status = target_state.into();
    record.last_error = None;
    save(root, record)?;
    emit(
        root,
        "editor.changed",
        json!({"domains":["scene","blueprint"],"changeSetId":record.id,"history":target_state}),
    );
    Ok(json!(record))
}

pub(super) async fn undo(p: &ScopeProject, args: &Value, redo: bool) -> Result<Value, String> {
    register(p);
    let b = bus(&p.project_root);
    let _guard = b.writes.lock().await;
    let current = engine(p, "editor_resolve", json!({})).await?;
    let head = &current[if redo { "redo" } else { "undo" }];
    let id = args["changeSetId"]
        .as_str()
        .or_else(|| head["changeSetId"].as_str());
    if args.get("changeSetId").is_none()
        && args
            .get("commandId")
            .is_some_and(|id| id != &head["commandId"])
    {
        return Err("HISTORY_CONFLICT: requested command is no longer at the history head".into());
    }
    let expected = args
        .get("expected")
        .filter(|v| !v.is_null())
        .cloned()
        .unwrap_or_else(|| expected_from(&current));
    let Some(id) = id else {
        let mut request = args.clone();
        request["expected"] = expected;
        if let Some(command) = head.get("commandId") {
            request["commandId"] = command.clone();
        }
        return engine(p, if redo { "edit_redo" } else { "edit_undo" }, request).await;
    };
    let mut record = load(&p.project_root, id)?;
    let prepared = if redo {
        "redo_prepared"
    } else {
        "undo_prepared"
    };
    let committed = if redo { "scene_redone" } else { "scene_undone" };
    let source = if redo { "undone" } else { "applied" };
    let target = if redo { "applied" } else { "undone" };
    if record.status == target {
        return Ok(json!(record));
    }
    if record.status != prepared && record.status != committed {
        if record.status != source {
            return Err(
                "EDITOR_CHANGE_SET_INCOMPLETE: recover or reconcile the pending command first"
                    .into(),
            );
        }
        if head["commandId"] != record.result["commandId"] || head["changeSetId"] != id {
            return Err("HISTORY_CONFLICT: another command is above this change set".into());
        }
        plan_documents(&p.project_root, &record.patches, redo, false)?;
        record.action_expected = expected;
        record.action_result = Value::Null;
        record.status = prepared.into();
        save(&p.project_root, &record)?;
    }
    resume_history(p, &mut record, redo).await
}

#[derive(Default)]
struct RecoveryRegistry {
    projects: HashMap<PathBuf, ScopeProject>,
    running: HashSet<PathBuf>,
}
static RECOVERY: OnceLock<Mutex<RecoveryRegistry>> = OnceLock::new();
fn key(root: &FsPath) -> PathBuf {
    root.canonicalize().unwrap_or_else(|_| root.to_path_buf())
}
fn register(p: &ScopeProject) {
    RECOVERY
        .get_or_init(Default::default)
        .lock()
        .unwrap()
        .projects
        .insert(key(&p.project_root), p.clone());
}
pub(super) fn project_for_root(root: &FsPath) -> ScopeProject {
    let canonical = key(root);
    if let Some(project) = RECOVERY
        .get_or_init(Default::default)
        .lock()
        .unwrap()
        .projects
        .get(&canonical)
        .cloned()
    {
        return project;
    }
    ScopeProject {
        workspace_id: None,
        name: "Editor project".into(),
        workspace_root: canonical.clone(),
        project_root: canonical,
        game_mode: crate::scope::game_mode_of(root),
    }
}
pub(super) fn schedule(p: &ScopeProject) {
    register(p);
    wake(&p.project_root);
}
pub(super) fn wake(root: &FsPath) {
    let Ok(runtime) = tokio::runtime::Handle::try_current() else {
        return;
    };
    let path = key(root);
    let project = {
        let mut registry = RECOVERY.get_or_init(Default::default).lock().unwrap();
        let Some(project) = registry.projects.get(&path).cloned() else {
            return;
        };
        if !registry.running.insert(path.clone()) {
            return;
        }
        project
    };
    runtime.spawn(async move {
        let _running = RecoveryGuard(path);
        if let Err(error) = recover(&project).await {
            emit(
                &project.project_root,
                "editor.recovery_failed",
                json!({"error":error}),
            );
        }
    });
}
struct RecoveryGuard(PathBuf);
impl Drop for RecoveryGuard {
    fn drop(&mut self) {
        if let Some(registry) = RECOVERY.get() {
            registry.lock().unwrap().running.remove(&self.0);
        }
    }
}
async fn recover(p: &ScopeProject) -> Result<(), String> {
    let folder = safe_path(&p.project_root, ".forge/editor/changes")?;
    if !folder.exists() {
        return Ok(());
    }
    let b = bus(&p.project_root);
    let _guard = b.writes.lock().await;
    // Bound one pass; subsequent editor reads/events trigger another pass.
    let entries = std::fs::read_dir(folder).map_err(|e| e.to_string())?;
    let mut pending = 0;
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("json") {
            continue;
        }
        let Some(id) = path.file_stem().and_then(|n| n.to_str()) else {
            continue;
        };
        let mut record = match load(&p.project_root, id) {
            Ok(record) => record,
            Err(error) => {
                emit(
                    &p.project_root,
                    "editor.recovery_failed",
                    json!({"changeSetId":id,"error":error}),
                );
                continue;
            }
        };
        if terminal(&record.status) {
            continue;
        }
        pending += 1;
        let result = match record.status.as_str() {
            "undo_prepared" | "scene_undone" => resume_history(p, &mut record, false).await,
            "redo_prepared" | "scene_redone" => resume_history(p, &mut record, true).await,
            _ => resume_apply(p, &mut record).await,
        };
        if let Err(error) = result {
            emit(
                &p.project_root,
                "editor.recovery_failed",
                json!({"changeSetId":id,"error":error}),
            );
        }
        if pending >= 16 {
            break;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn board(bindings: Value) -> Value {
        documents::normalize("blueprint",json!({"version":3,"seq":3,"name":"human title","nodes":[{"id":"hero","pos":[7,8]},{"id":"other"}],"edges":[],"bindings":bindings})).unwrap()
    }
    fn patch() -> BindingPatch {
        BindingPatch {
            board_id: "main".into(),
            node_id: "hero".into(),
            before_exists: false,
            before: Value::Null,
            after: json!({"entityGuid":"entity"}),
            spec: Value::Null,
        }
    }
    #[test]
    fn inverse_preserves_unrelated_human_fields_and_is_restart_idempotent() {
        let patch = patch();
        let mut doc = board(json!({"other":{"entityGuid":"other"}}));
        assert!(patch_document(&mut doc, &patch, true, false).unwrap());
        assert!(!patch_document(&mut doc, &patch, true, true).unwrap());
        doc["nodes"][0]["pos"] = json!([9, 10]);
        assert!(patch_document(&mut doc, &patch, false, false).unwrap());
        assert_eq!(doc["nodes"][0]["pos"], json!([9, 10]));
        assert_eq!(doc["bindings"]["other"]["entityGuid"], "other");
        assert!(doc["bindings"].get("hero").is_none());
        assert!(!patch_document(&mut doc, &patch, false, true).unwrap());
    }
    #[test]
    fn conflicting_owned_field_is_never_overwritten() {
        let patch = patch();
        let mut doc = board(json!({"hero":{"entityGuid":"human"}}));
        let original = doc.clone();
        assert!(patch_document(&mut doc, &patch, false, true).is_err());
        assert_eq!(doc, original);
    }
    #[test]
    fn inverse_preserves_human_edits_inside_the_same_binding() {
        let mut patch = patch();
        patch.before_exists = true;
        patch.before = json!({"note":"old"});
        patch.after = json!({"note":"old","entityGuid":"entity"});
        let mut doc = board(json!({"hero":{"note":"old"}}));
        patch_document(&mut doc, &patch, true, false).unwrap();
        doc["bindings"]["hero"]["note"] = json!("human changed");
        patch_document(&mut doc, &patch, false, false).unwrap();
        assert_eq!(doc["bindings"]["hero"], json!({"note":"human changed"}));
        patch_document(&mut doc, &patch, true, false).unwrap();
        doc["bindings"]["hero"]
            .as_object_mut()
            .unwrap()
            .remove("note");
        patch_document(&mut doc, &patch, false, false).unwrap();
        assert_eq!(doc["bindings"]["hero"], json!({}));
    }
    #[test]
    fn pending_scene_returns_without_rebasing_and_host_restart_is_rejected() {
        let expected =
            json!({"sceneGuid":"a","hostEpoch":"host","contentRevision":4,"targetMode":"edit"});
        let mut current = expected.clone();
        current["sceneGuid"] = json!("b");
        current["contentRevision"] = json!(9);
        assert_eq!(
            readiness(&expected, &current).unwrap(),
            Some("pending_target_scene")
        );
        assert_eq!(readiness(&expected, &expected).unwrap(), None);
        current = expected.clone();
        current["contentRevision"] = json!(5);
        assert!(readiness(&expected, &current).is_err());
        current = expected.clone();
        current["hostEpoch"] = json!("new");
        assert!(readiness(&expected, &current).is_err());
    }
    #[test]
    fn request_fingerprint_detects_mutated_idempotent_payload() {
        let a = json!({"changeSetId":"same","ops":[{"op":"create","name":"one"}]});
        let mut b = a.clone();
        b["ops"][0]["name"] = json!("two");
        assert_ne!(fingerprint(&a), fingerprint(&b));
        assert_eq!(fingerprint(&a), fingerprint(&a.clone()));
    }
    #[test]
    fn nullable_preimage_remains_distinct_from_absent_binding() {
        let mut patch = patch();
        patch.before_exists = true;
        patch.before = Value::Null;
        let mut doc = board(json!({"hero":null}));
        patch_document(&mut doc, &patch, true, false).unwrap();
        patch_document(&mut doc, &patch, false, false).unwrap();
        assert!(doc["bindings"].get("hero").is_some());
        assert!(doc["bindings"]["hero"].is_null());
    }
    #[test]
    fn asset_binding_requires_real_guid_path_and_current_file_version() {
        let root = std::env::temp_dir().join(crate::events::new_id("editor-binding-asset"));
        let project = assetd::project::ForgeProject::with_defaults(root.clone());
        std::fs::create_dir_all(project.content_root().join("Textures")).unwrap();
        project.save_manifest().unwrap();
        let path = project.content_root().join("Textures/test.png");
        std::fs::write(&path, b"source bytes").unwrap();
        let (meta_path, meta) =
            assetd::meta::ensure_meta(&project.content_root(), "Textures/test.png").unwrap();
        let asset = json!({"guid":meta.guid,"path":"Textures/test.png","version":forge_util::hashutil::sha256_file(&path).unwrap()});
        assert!(validate_assets(&root, &json!([asset.clone()])).is_ok());
        let mut wrong = asset.clone();
        wrong["guid"] = json!("forged");
        assert!(validate_assets(&root, &json!([wrong]))
            .unwrap_err()
            .contains("MISMATCH"));
        let mut escaped = asset.clone();
        escaped["path"] = json!("../outside.png");
        assert!(validate_assets(&root, &json!([escaped])).is_err());
        std::fs::write(&path, b"changed").unwrap();
        assert!(validate_assets(&root, &json!([asset]))
            .unwrap_err()
            .contains("VERSION_CONFLICT"));
        std::fs::remove_file(path).unwrap();
        std::fs::remove_file(meta_path).unwrap();
        std::fs::remove_file(root.join("forge.toml")).unwrap();
        std::fs::remove_dir(project.content_root().join("Textures")).unwrap();
        std::fs::remove_dir(project.content_root()).unwrap();
        std::fs::remove_dir(root).unwrap();
    }
}
