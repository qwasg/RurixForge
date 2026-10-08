//! Stable editor references and optimistic, atomic commands. Called while HostState is locked.
use super::*;

fn scene_location(path: &std::path::Path) -> PathBuf {
    path.canonicalize().unwrap_or_else(|_| {
        path.parent()
            .and_then(|parent| parent.canonicalize().ok())
            .and_then(|parent| path.file_name().map(|name| parent.join(name)))
            .unwrap_or_else(|| path.to_path_buf())
    })
}
pub(super) fn same_scene_location(a: &std::path::Path, b: &std::path::Path) -> bool {
    scene_location(a) == scene_location(b)
}
pub(super) fn independent_scene_copy(scene: &Scene) -> Result<Scene, String> {
    let mut copy = scene.clone();
    copy.scene_guid = None;
    for entity in &mut copy.entities {
        entity.entity_guid = None;
    }
    copy.ensure_editor_identity(assetd::new_guid)?;
    Ok(copy)
}

/// A copied file is not a new scene identity. Check disk as well as this host's cache,
/// so the first load after a restart cannot silently resolve a duplicated GUID.
pub(super) fn check_scene_identity(
    st: &HostState,
    scene: &Scene,
    path: &std::path::Path,
    content: &std::path::Path,
) -> Result<(), (i64, String)> {
    let Some(guid) = scene.scene_guid.as_deref() else {
        return Ok(());
    };
    #[derive(serde::Deserialize)]
    struct Identity {
        #[serde(default, rename = "sceneGuid")]
        scene_guid: Option<String>,
    }
    let requested = scene_location(path);
    let mut checked = std::collections::HashSet::new();
    let mut inspect = |other: PathBuf| -> Result<(), (i64, String)> {
        let canonical = scene_location(&other);
        if canonical == requested || !checked.insert(canonical) || !other.is_file() {
            return Ok(());
        }
        let identity = std::fs::File::open(&other)
            .ok()
            .and_then(|file| serde_json::from_reader::<_, Identity>(file).ok());
        if identity.is_some_and(|identity| identity.scene_guid.as_deref() == Some(guid)) {
            return domain_err(format!(
                "AMBIGUOUS_SCENE_IDENTITY: sceneGuid {guid} also belongs to {}",
                other.display()
            ));
        }
        Ok(())
    };
    for (known, _) in st.scene_documents.values() {
        if let Some(known) = known {
            inspect(PathBuf::from(known))?;
        }
    }
    let mut pending = vec![content.to_path_buf()];
    while let Some(directory) = pending.pop() {
        let entries = match std::fs::read_dir(directory) {
            Ok(entries) => entries,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(error) => return domain_err(format!("SCENE_IDENTITY_SCAN_FAILED: {error}")),
        };
        for entry in entries {
            let entry =
                entry.map_err(|error| (-32000, format!("SCENE_IDENTITY_SCAN_FAILED: {error}")))?;
            let metadata = std::fs::symlink_metadata(entry.path())
                .map_err(|error| (-32000, error.to_string()))?;
            if metadata.file_type().is_symlink() {
                continue;
            }
            #[cfg(windows)]
            {
                use std::os::windows::fs::MetadataExt;
                if metadata.file_attributes() & 0x400 != 0 {
                    continue;
                }
            }
            if metadata.is_dir() {
                pending.push(entry.path());
            } else if entry
                .path()
                .extension()
                .and_then(|v| v.to_str())
                .is_some_and(|ext| ext.eq_ignore_ascii_case("rxscene"))
            {
                inspect(entry.path())?;
            }
        }
    }
    Ok(())
}

fn request_hash(params: &Value) -> String {
    forge_util::hashutil::sha256_hex(params.to_string().as_bytes())
}
pub(super) fn replay_receipt(
    st: &HostState,
    params: &Value,
) -> Result<Option<Value>, (i64, String)> {
    if params.get("action").is_some() {
        return Ok(None);
    }
    if let Some(id) = params.get("changeSetId").and_then(Value::as_str) {
        if let Some((hash, result)) = st.editor_receipts.get(id) {
            if hash != &request_hash(params) {
                return domain_err(
                    "IDEMPOTENCY_MISMATCH: changeSetId was already used for a different request",
                );
            }
            return Ok(Some(result.clone()));
        }
    }
    Ok(None)
}
pub(super) fn store_receipt(st: &mut HostState, params: &Value, result: &Value) {
    if params.get("action").is_some() {
        return;
    }
    if let Some(id) = params.get("changeSetId").and_then(Value::as_str) {
        st.editor_receipts
            .insert(id.to_string(), (request_hash(params), result.clone()));
    }
}

pub(super) fn mode(st: &HostState) -> &'static str {
    if st.play == PlayState::Edit {
        "edit"
    } else {
        "runtime"
    }
}
pub(super) fn revision(st: &HostState) -> u64 {
    if st.play == PlayState::Edit {
        st.content_revision
    } else {
        st.runtime_revision
    }
}
pub(super) fn stamp(st: &HostState, value: &mut Value) {
    if let Some(object) = value.as_object_mut() {
        object.insert("sceneGuid".into(), json!(st.active().scene_guid));
        object.insert("hostEpoch".into(), json!(st.host_epoch));
        object.insert("contentRevision".into(), json!(revision(st)));
        object.insert("targetMode".into(), json!(mode(st)));
        object.insert("scenePath".into(), json!(st.scene_path));
        object.insert(
            "identityPersisted".into(),
            json!(st
                .active()
                .scene_guid
                .as_ref()
                .is_some_and(|g| st.persisted_guids.contains(g))),
        );
    }
}
pub(super) fn changes_content(method: &str) -> bool {
    matches!(
        method,
        "editor.apply"
            | "entity.create"
            | "entity.destroy"
            | "entity.rename"
            | "entity.batchApply"
            | "component.add"
            | "component.remove"
            | "component.set"
            | "transform.set"
            | "transform.batchSet"
            | "scene.new"
            | "scene.load"
            | "scene.rollback"
            | "edit.undo"
            | "edit.redo"
            | "prefab.instantiate"
            | "prefab.revert"
            | "asset.reload"
            | "animation.control"
    )
}
pub(super) fn check_history(
    st: &HostState,
    method: &str,
    params: &Value,
) -> Result<(), (i64, String)> {
    let head = if method == "edit.undo" {
        st.undo.last()
    } else {
        st.redo.last()
    };
    if let Some(id) = params.get("commandId").and_then(Value::as_str) {
        if head.is_none_or(|c| c.id != id) {
            return domain_err("COMMAND_CONFLICT: a later edit is at the history head");
        }
    }
    if let Some(id) = params.get("changeSetId").and_then(Value::as_str) {
        if head.and_then(|c| c.change_set_id.as_deref()) != Some(id) {
            return domain_err("COMMAND_CONFLICT: change set does not own the history head");
        }
    }
    Ok(())
}
pub(super) fn check_expected(
    st: &HostState,
    params: &Value,
    required: bool,
) -> Result<(), (i64, String)> {
    let Some(expected) = params.get("expected") else {
        return if required {
            domain_err("EXPECTED_REQUIRED: resolve the current target before editing")
        } else {
            Ok(())
        };
    };
    let scene = expected
        .get("sceneGuid")
        .and_then(Value::as_str)
        .ok_or((-32602, "INVALID_EXPECTED: sceneGuid required".into()))?;
    let epoch = expected
        .get("hostEpoch")
        .and_then(Value::as_str)
        .ok_or((-32602, "INVALID_EXPECTED: hostEpoch required".into()))?;
    let target = expected
        .get("targetMode")
        .and_then(Value::as_str)
        .ok_or((-32602, "INVALID_EXPECTED: targetMode required".into()))?;
    let rev = expected
        .get("contentRevision")
        .and_then(Value::as_u64)
        .ok_or((-32602, "INVALID_EXPECTED: contentRevision required".into()))?;
    if epoch != st.host_epoch {
        return domain_err("STALE_HOST: engine-host restarted; resolve the reference again");
    }
    if Some(scene) != st.active().scene_guid.as_deref() {
        return domain_err("TARGET_SCENE_INACTIVE: requested scene is not active");
    }
    if !matches!(target, "edit" | "runtime") {
        return param_err("INVALID_EXPECTED: targetMode must be edit or runtime");
    }
    if target != mode(st) {
        return domain_err("TARGET_MODE_MISMATCH: wait for the requested scene mode");
    }
    if rev != revision(st) {
        return domain_err("CONTENT_CONFLICT: target changed since it was read");
    }
    Ok(())
}
fn reference(params: &Value) -> &Value {
    params.get("ref").unwrap_or(params)
}
fn resolve_id(scene: &Scene, reference: &Value) -> Result<u64, (i64, String)> {
    if reference
        .get("sceneGuid")
        .and_then(Value::as_str)
        .is_some_and(|g| Some(g) != scene.scene_guid.as_deref())
    {
        return domain_err("REFERENCE_MISMATCH: entity reference belongs to a different scene");
    }
    let guid = reference.get("entityGuid").and_then(Value::as_str);
    let id = reference
        .get("id")
        .or_else(|| reference.get("entityId"))
        .and_then(Value::as_u64);
    if let Some(guid) = guid {
        let entity = scene.entity_by_guid(guid).map_err(|e| (-32000, e))?;
        if id.is_some_and(|id| id != entity.id) {
            return domain_err("REFERENCE_MISMATCH: id does not identify entityGuid");
        }
        return Ok(entity.id);
    }
    let id = id.ok_or((
        -32602,
        "REFERENCE_REQUIRED: entityGuid or id required".into(),
    ))?;
    if scene.entity(id).is_none() {
        return domain_err("REFERENCE_NOT_FOUND: entity was removed");
    }
    Ok(id)
}
pub(super) fn resolve(st: &HostState, params: &Value) -> HResult {
    if let Some(id) = params.get("changeSetId").and_then(Value::as_str) {
        let receipt = st.editor_receipts.get(id);
        let command_id = receipt.and_then(|(_, r)| r["commandId"].as_str());
        let (undo, redo) = st
            .edit_history
            .as_ref()
            .map(|(undo, redo)| (undo, redo))
            .unwrap_or((&st.undo, &st.redo));
        let applied = command_id.is_some_and(|id| undo.iter().any(|c| c.id == id));
        let undone = command_id.is_some_and(|id| redo.iter().any(|c| c.id == id));
        return Ok(
            json!({"receipt":receipt.map(|(_,r)|r),"requestHash":receipt.map(|(h,_)|h),"receiptState":if applied{"applied"}else if undone{"undone"}else{"unknown"}}),
        );
    }
    let r = reference(params);
    if r.get("entityGuid").and_then(Value::as_str).is_none()
        && (r.get("id").is_some() || r.get("entityId").is_some())
        && r.get("hostEpoch")
            .and_then(Value::as_str)
            .is_some_and(|epoch| epoch != st.host_epoch)
    {
        return domain_err("STALE_HOST: numeric entity references cannot cross host lifetimes");
    }
    if r.get("sceneGuid")
        .and_then(Value::as_str)
        .is_some_and(|g| Some(g) != st.active().scene_guid.as_deref())
    {
        return domain_err("TARGET_SCENE_INACTIVE: requested scene is not active");
    }
    if r.get("targetMode")
        .and_then(Value::as_str)
        .is_some_and(|m| m != mode(st))
    {
        return domain_err("TARGET_MODE_MISMATCH: requested mode is not active");
    }
    if r.get("entityGuid").is_none() && r.get("entityId").is_none() && r.get("id").is_none() {
        return Ok(
            json!({"kind":"scene","name":st.active().name,"entityCount":st.active().entities.len(),
            "undo":st.undo.last().map(|c|json!({"commandId":c.id,"changeSetId":c.change_set_id})),
            "redo":st.redo.last().map(|c|json!({"commandId":c.id,"changeSetId":c.change_set_id}))}),
        );
    }
    let entity = st.active().entity(resolve_id(st.active(), r)?).unwrap();
    let mut value = entity_json_with_category(entity);
    if let Some(component) = r.get("component").and_then(Value::as_str) {
        value = serde_json::to_value(
            entity
                .component(component)
                .ok_or((-32000, "REFERENCE_NOT_FOUND: component was removed".into()))?,
        )
        .map_err(|e| (-32000, e.to_string()))?;
    }
    if let Some(pointer) = r.get("pointer").and_then(Value::as_str) {
        value = value
            .pointer(pointer)
            .cloned()
            .ok_or((-32000, "REFERENCE_NOT_FOUND: JSON pointer is absent".into()))?;
    }
    Ok(
        json!({"kind":"entity","id":entity.id,"entityGuid":entity.entity_guid,"name":entity.name,"value":value,
        "entity":entity_json_with_category(entity),"entityIdentityPersisted":entity.entity_guid.as_ref().is_some_and(|g|st.persisted_guids.contains(g))}),
    )
}

pub(super) fn apply(st: &mut HostState, params: &Value) -> HResult {
    check_expected(st, params, true)?;
    if st.play != PlayState::Edit {
        return domain_err("TARGET_MODE_MISMATCH: persistent editor commands require edit mode");
    }
    if let Some(action) = params.get("action").and_then(Value::as_str) {
        let expected_id = params
            .get("commandId")
            .and_then(Value::as_str)
            .ok_or((-32602, "commandId required for undo/redo".into()))?;
        let top = match action {
            "undo" => st.undo.last(),
            "redo" => st.redo.last(),
            _ => return param_err("unknown editor action"),
        };
        if top.is_none_or(|c| c.id != expected_id) {
            return domain_err("COMMAND_CONFLICT: command is no longer at the history head");
        }
        return if action == "undo" {
            edit_undo(st)
        } else {
            edit_redo(st)
        };
    }
    let ops = params
        .get("ops")
        .and_then(Value::as_array)
        .ok_or((-32602, "ops array required".into()))?;
    if ops.is_empty() || ops.len() > 2048 {
        return param_err("ops must contain 1..2048 operations");
    }
    // Validate every operation against a private working copy. Nothing becomes visible until commit.
    let mut working = st.scene.clone();
    let mut inverses = Vec::with_capacity(ops.len());
    let mut created = Vec::new();
    let mut client_ids = HashMap::new();
    for (index, input) in ops.iter().enumerate() {
        let mut input = input.clone();
        let name = input["op"]
            .as_str()
            .ok_or((-32602, "op required".into()))?
            .to_string();
        if let Some(client_id) = input.get("targetClientId").and_then(Value::as_str) {
            input["id"] = json!(client_ids
                .get(client_id)
                .ok_or((-32602, "unknown targetClientId".into()))?);
        }
        if name != "create" {
            input["id"] = json!(resolve_id(&working, reference(&input))?);
        }
        let op = if name == "duplicate" {
            let mut entity = working.entity(req_id(&input)?).unwrap().clone();
            entity.id = working.next_id;
            entity.entity_guid = Some(assetd::new_guid());
            entity.name = input
                .get("name")
                .and_then(Value::as_str)
                .map(str::to_string)
                .unwrap_or_else(|| format!("{} Copy", entity.name));
            // A duplicated template entity is an independent entity, not another member of the same instance.
            entity.components.retain(|c| c.ctype != "PrefabInstance");
            Op::RestoreEntity {
                index: working.entities.len(),
                entity,
            }
        } else {
            let mut op = parse_batch_op(&input, &working)?;
            if let Op::CreateEntity { id, .. } = &mut op {
                *id = working.next_id;
            }
            op
        };
        let created_id = match &op {
            Op::CreateEntity { id, .. } => Some(*id),
            Op::RestoreEntity { entity, .. } if name == "duplicate" => Some(entity.id),
            _ => None,
        };
        inverses.push(
            op.apply(&mut working)
                .map_err(|e| (-32000, format!("EDIT_APPLY_FAILED: operation {index}: {e}")))?,
        );
        if let Some(id) = created_id {
            if let Some(key) = input.get("clientId").and_then(Value::as_str) {
                if client_ids.insert(key.to_string(), id).is_some() {
                    return param_err("duplicate clientId");
                }
            }
            created.push(json!({"opIndex":index,"clientId":input.get("clientId"),"id":id,"entityGuid":working.entity(id).and_then(|e|e.entity_guid.as_ref())}));
        }
    }
    // Bindings are validated against the final staged scene under the same lock. A
    // target that this very batch deleted must not survive as a dangling board link.
    if let Some(bindings) = params.get("bindings") {
        let bindings = bindings
            .as_array()
            .ok_or((-32602, "INVALID_BINDINGS: array required".into()))?;
        for binding in bindings {
            let id = if let Some(client) = binding.get("clientId").and_then(Value::as_str) {
                *client_ids
                    .get(client)
                    .ok_or((-32602, "BINDING_TARGET_NOT_FOUND: unknown clientId".into()))?
            } else if let Some(index) = binding.get("opIndex").and_then(Value::as_u64) {
                created
                    .iter()
                    .find(|entry| entry["opIndex"].as_u64() == Some(index))
                    .and_then(|entry| entry["id"].as_u64())
                    .ok_or((
                        -32602,
                        "BINDING_TARGET_NOT_FOUND: unknown creation opIndex".into(),
                    ))?
            } else {
                let target = binding
                    .get("target")
                    .ok_or((-32602, "BINDING_TARGET_NOT_FOUND: target required".into()))?;
                if target["entityGuid"].as_str().is_none() {
                    return param_err("BINDING_TARGET_NOT_FOUND: entityGuid required");
                }
                resolve_id(&working, target)?
            };
            if working.entity(id).is_none() {
                return domain_err(
                    "BINDING_TARGET_NOT_FOUND: entity is absent from the committed scene",
                );
            }
        }
    }
    working
        .ensure_editor_identity(assetd::new_guid)
        .map_err(|e| (-32000, e))?;
    for entity in &working.entities {
        crate::modelrt::entity_world(&working, entity).map_err(|e| (-32000, e))?;
    }
    inverses.reverse();
    let command = Command {
        id: assetd::new_guid(),
        change_set_id: params
            .get("changeSetId")
            .and_then(Value::as_str)
            .map(str::to_string),
        op: Op::Batch(inverses),
    };
    let result = json!({"applied":ops.len(),"created":created,"commandId":command.id,"changeSetId":command.change_set_id});
    st.scene = working;
    st.undo.push(command);
    st.redo.clear();
    push_event(st, "editor.applied", result.clone());
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn request(st: &Mutex<HostState>, method: &str, params: Value) -> Value {
        dispatch(
            st,
            &json!({"jsonrpc":"2.0","id":1,"method":method,"params":params}),
        )
    }
    fn expected(st: &Mutex<HostState>) -> Value {
        let mut value = json!({});
        stamp(&lock(st), &mut value);
        value
    }
    #[test]
    fn identity_survives_undo_and_editor_history_survives_pie() {
        let state = Mutex::new(HostState::new());
        let created = request(
            &state,
            "editor.apply",
            json!({"expected":expected(&state),"changeSetId":"test","ops":[{"op":"create","name":"hero","clientId":"hero"}]}),
        );
        let guid = created["result"]["created"][0]["entityGuid"].clone();
        let command = created["result"]["commandId"].clone();
        assert!(guid.as_str().is_some());
        assert!(request(&state, "play.enter", json!({}))["error"].is_null());
        assert!(request(&state, "play.exit", json!({}))["error"].is_null());
        assert!(request(
            &state,
            "edit.undo",
            json!({"expected":expected(&state),"commandId":command,"changeSetId":"test"})
        )["error"]
            .is_null());
        assert!(lock(&state).scene.entities.is_empty());
        assert!(request(&state, "edit.redo", json!({}))["error"].is_null());
        assert_eq!(json!(lock(&state).scene.entities[0].entity_guid), guid);
    }
    #[test]
    fn atomic_batch_resolves_created_ids_and_rejects_stale_or_failed_writes() {
        let state = Mutex::new(HostState::new());
        let base = expected(&state);
        let result = request(
            &state,
            "editor.apply",
            json!({"expected":base,"ops":[{"op":"create","name":"hero","clientId":"hero"},{"op":"rename","targetClientId":"hero","name":"renamed"}]}),
        );
        assert_eq!(lock(&state).scene.entities[0].name, "renamed");
        assert_eq!(result["result"]["created"].as_array().unwrap().len(), 1);
        assert_eq!(
            request(
                &state,
                "editor.apply",
                json!({"expected":base,"ops":[{"op":"create","name":"late"}]})
            )["error"]["data"]["code"],
            "CONTENT_CONFLICT"
        );
        let before = lock(&state).scene.clone();
        let failure = request(
            &state,
            "editor.apply",
            json!({"expected":expected(&state),"ops":[{"op":"create","name":"temporary"},{"op":"rename","entityGuid":"deleted-guid","name":"bad"}]}),
        );
        assert_eq!(failure["error"]["data"]["code"], "REFERENCE_NOT_FOUND");
        assert_eq!(lock(&state).scene, before);
    }
    #[test]
    fn camera_does_not_invalidate_content_and_wrong_history_head_is_rejected() {
        let state = Mutex::new(HostState::new());
        let base = expected(&state);
        request(&state, "viewport.setCamera", json!({"yaw":12.0}));
        assert_eq!(base["contentRevision"], expected(&state)["contentRevision"]);
        let first = request(
            &state,
            "editor.apply",
            json!({"expected":base,"ops":[{"op":"create","name":"a"}]}),
        );
        request(&state, "entity.create", json!({"name":"human"}));
        let denied = request(
            &state,
            "edit.undo",
            json!({"commandId":first["result"]["commandId"]}),
        );
        assert_eq!(denied["error"]["data"]["code"], "COMMAND_CONFLICT");
        assert_eq!(lock(&state).scene.entities.len(), 2);
    }
    #[test]
    fn same_numeric_id_in_replaced_scene_never_resolves_old_guid() {
        let state = Mutex::new(HostState::new());
        let entity =
            request(&state, "entity.create", json!({"name":"old"}))["result"]["entity"].clone();
        request(&state, "scene.new", json!({"name":"new"}));
        request(&state, "entity.create", json!({"name":"new"}));
        assert_eq!(entity["id"], json!(lock(&state).scene.entities[0].id));
        assert_eq!(
            request(
                &state,
                "editor.resolve",
                json!({"entityGuid":entity["entityGuid"]})
            )["error"]["data"]["code"],
            "REFERENCE_NOT_FOUND"
        );
    }
    #[test]
    fn change_set_receipt_replays_once_and_rejects_payload_mismatch() {
        let state = Mutex::new(HostState::new());
        let params = json!({"expected":expected(&state),"changeSetId":"dedup","ops":[{"op":"create","name":"once"}]});
        let first = request(&state, "editor.apply", params.clone());
        assert!(first["error"].is_null());
        let replay = request(&state, "editor.apply", params.clone());
        assert_eq!(first, replay);
        assert_eq!(lock(&state).scene.entities.len(), 1);
        let receipt = request(&state, "editor.resolve", json!({"changeSetId":"dedup"}));
        assert_eq!(receipt["result"]["receiptState"], "applied");
        assert_eq!(receipt["result"]["requestHash"], request_hash(&params));
        let mut other = params;
        other["ops"][0]["name"] = json!("different");
        assert_eq!(
            request(&state, "editor.apply", other)["error"]["data"]["code"],
            "IDEMPOTENCY_MISMATCH"
        );
        request(&state, "edit.undo", json!({}));
        assert_eq!(
            request(&state, "editor.resolve", json!({"changeSetId":"dedup"}))["result"]
                ["receiptState"],
            "undone"
        );
    }
    #[test]
    fn unchanged_scene_return_reuses_baseline_but_changed_content_does_not() {
        let state = Mutex::new(HostState::new());
        request(&state, "entity.create", json!({"name":"original"}));
        let baseline = expected(&state);
        request(&state, "scene.new", json!({"name":"other"}));
        let other = expected(&state);
        assert_ne!(baseline["sceneGuid"], other["sceneGuid"]);
        request(&state, "edit.undo", json!({}));
        assert_eq!(expected(&state)["sceneGuid"], baseline["sceneGuid"]);
        assert_eq!(
            expected(&state)["contentRevision"],
            baseline["contentRevision"]
        );
        request(&state, "entity.create", json!({"name":"new content"}));
        assert!(
            expected(&state)["contentRevision"].as_u64().unwrap()
                > other["contentRevision"].as_u64().unwrap()
        );
    }

    #[test]
    fn numeric_references_reject_previous_host_lifetime() {
        let state = Mutex::new(HostState::new());
        let created = request(&state, "entity.create", json!({"name":"entity"}));
        let reference = json!({"sceneGuid":expected(&state)["sceneGuid"],"hostEpoch":"previous host","id":created["result"]["id"]});
        assert_eq!(
            request(&state, "editor.resolve", reference)["error"]["data"]["code"],
            "STALE_HOST"
        );
        let scene_reference =
            json!({"sceneGuid":expected(&state)["sceneGuid"],"hostEpoch":"previous host"});
        assert!(
            request(&state, "editor.resolve", scene_reference)["error"].is_null(),
            "scene GUID references survive host restart"
        );
    }

    #[test]
    fn runtime_refresh_invalidates_edit_baseline_only_when_edit_content_changes() {
        let mut state = HostState::new();
        state.remember_scene_revision();
        let base = state.content_revision;
        let guid = state.scene.scene_guid.clone();
        state.play = PlayState::Running;
        state.commit_content_revision("component.set", guid.as_deref());
        assert_eq!(state.content_revision, base);
        state.scene.name = "refreshed prefab content".into();
        state.commit_content_revision("asset.reload", guid.as_deref());
        assert!(state.content_revision > base);
    }

    #[test]
    fn disk_scene_guid_copies_are_rejected_even_before_the_first_load() {
        let state = HostState::new();
        let root =
            std::env::temp_dir().join(format!("forge-scene-identity-{}", assetd::new_guid()));
        std::fs::create_dir_all(&root).unwrap();
        let first = root.join("first.rxscene");
        let duplicate = root.join("duplicate.rxscene");
        state.scene.save(&first).unwrap();
        state.scene.save(&duplicate).unwrap();
        let failure = check_scene_identity(&state, &state.scene, &duplicate, &root).unwrap_err();
        assert!(failure.1.starts_with("AMBIGUOUS_SCENE_IDENTITY:"));
        independent_scene_copy(&state.scene)
            .unwrap()
            .save(&first)
            .unwrap();
        assert!(check_scene_identity(&state, &state.scene, &duplicate, &root).is_ok());
        std::fs::remove_file(first).unwrap();
        std::fs::remove_file(duplicate).unwrap();
        std::fs::remove_dir(root).unwrap();
    }

    #[test]
    fn saving_an_independent_copy_preserves_live_source_identity_and_numeric_links() {
        let state = Mutex::new(HostState::new());
        request(&state, "entity.create", json!({"name":"source"}));
        let original = lock(&state).scene.clone();
        let copy = independent_scene_copy(&original).unwrap();
        assert_ne!(copy.scene_guid, original.scene_guid);
        assert_ne!(
            copy.entities[0].entity_guid,
            original.entities[0].entity_guid
        );
        assert_eq!(copy.entities[0].id, original.entities[0].id);
        assert_eq!(lock(&state).scene, original);
    }
    #[test]
    fn bindings_require_targets_that_survive_the_entire_atomic_batch() {
        let state = Mutex::new(HostState::new());
        let created = request(&state, "entity.create", json!({"name":"source"}));
        let entity = &created["result"]["entity"];
        let before = lock(&state).scene.clone();
        let deleted = request(
            &state,
            "editor.apply",
            json!({"expected":expected(&state),"ops":[{"op":"destroy","entityGuid":entity["entityGuid"]}],
            "bindings":[{"target":{"entityGuid":entity["entityGuid"],"id":entity["id"]}}]}),
        );
        assert_eq!(deleted["error"]["data"]["code"], "REFERENCE_NOT_FOUND");
        assert_eq!(lock(&state).scene, before);
        let created_then_deleted = request(
            &state,
            "editor.apply",
            json!({"expected":expected(&state),"ops":[{"op":"create","name":"temporary","clientId":"temporary"},{"op":"destroy","targetClientId":"temporary"}],
            "bindings":[{"clientId":"temporary"}]}),
        );
        assert_eq!(
            created_then_deleted["error"]["data"]["code"],
            "BINDING_TARGET_NOT_FOUND"
        );
        assert_eq!(lock(&state).scene, before);
        let mismatch = request(
            &state,
            "editor.apply",
            json!({"expected":expected(&state),"ops":[{"op":"rename","entityGuid":entity["entityGuid"],"name":"rename"}],
            "bindings":[{"target":{"entityGuid":entity["entityGuid"],"id":999}}]}),
        );
        assert_eq!(mismatch["error"]["data"]["code"], "REFERENCE_MISMATCH");
        assert_eq!(lock(&state).scene, before);
    }
}
