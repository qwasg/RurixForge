//! Basic prefab instances with stable child identities and three-way property updates.
use forge_scene::{Component, Entity, Scene, Transform};
use serde_json::{json, Value};
#[cfg(test)]
static TEST_PREFABS: std::sync::OnceLock<std::sync::Mutex<HashMap<String, Value>>> =
    std::sync::OnceLock::new();
use std::collections::{BTreeMap, HashMap};

fn load(reference: &str) -> Result<(Value, String), String> {
    #[cfg(test)]
    if let Some(v) = TEST_PREFABS
        .get_or_init(|| std::sync::Mutex::new(HashMap::new()))
        .lock()
        .unwrap()
        .get(reference)
        .cloned()
    {
        return Ok((v, reference.into()));
    }
    let root = crate::rpc::project_root();
    let p = assetd::project::ForgeProject::load(&root).map_err(|e| e.to_string())?;
    for rel in p.scan_content().map_err(|e| e.to_string())? {
        if !rel.ends_with(".rxprefab") {
            continue;
        }
        let meta =
            assetd::meta::MetaDoc::load(&assetd::meta_path_for(&p.content_root(), &rel)).ok();
        if reference == rel
            || reference == format!("Content/{rel}")
            || meta.as_ref().is_some_and(|m| m.guid == reference)
        {
            let text =
                std::fs::read_to_string(p.content_root().join(&rel)).map_err(|e| e.to_string())?;
            let v: Value = serde_json::from_str(&text).map_err(|e| e.to_string())?;
            if v.get("version").and_then(Value::as_u64) != Some(1) {
                return Err("PREFAB_INVALID: unsupported version".into());
            }
            return Ok((v, meta.map(|m| m.guid).unwrap_or(rel)));
        }
    }
    Err(format!("PREFAB_NOT_FOUND: {reference}"))
}
fn entity_doc(e: &Entity) -> Value {
    let mut v = serde_json::to_value(e).unwrap();
    v.as_object_mut().unwrap().remove("id");
    let cs = e
        .components
        .iter()
        .filter(|c| c.ctype != "PrefabInstance")
        .map(|c| {
            (
                c.ctype.clone(),
                json!({"enabled":c.enabled,"props":c.props}),
            )
        })
        .collect::<serde_json::Map<_, _>>();
    v["components"] = Value::Object(cs);
    v
}
fn apply_doc(e: &mut Entity, v: &Value) -> Result<(), String> {
    e.name = v["name"].as_str().ok_or("prefab name missing")?.into();
    e.transform = serde_json::from_value(v["transform"].clone()).map_err(|e| e.to_string())?;
    let old = e.component("PrefabInstance").cloned();
    e.components.clear();
    for (k, v) in v["components"].as_object().ok_or("components not object")? {
        let props = forge_scene::normalize_props(k, &v["props"])?;
        forge_scene::validate_props(k, &props)?;
        e.components.push(Component {
            ctype: k.clone(),
            enabled: v["enabled"].as_bool().unwrap_or(true),
            props,
        });
    }
    if let Some(c) = old {
        e.components.push(c);
    }
    Ok(())
}
/// Preserve a locally changed leaf; update every leaf still equal to its old template.
fn merge(
    old: Option<&Value>,
    current: Option<&Value>,
    new: Option<&Value>,
    path: &str,
    overrides: &mut Vec<String>,
) -> Option<Value> {
    if current == old {
        return new.cloned();
    }
    if let (Some(Value::Object(o)), Some(Value::Object(c)), Some(Value::Object(n))) =
        (old, current, new)
    {
        let keys = o
            .keys()
            .chain(c.keys())
            .chain(n.keys())
            .collect::<std::collections::BTreeSet<_>>();
        let mut out = serde_json::Map::new();
        for k in keys {
            let kp = format!("{path}/{}", k.replace('~', "~0").replace('/', "~1"));
            if let Some(v) = merge(o.get(k), c.get(k), n.get(k), &kp, overrides) {
                out.insert(k.clone(), v);
            }
        }
        Some(Value::Object(out))
    } else {
        overrides.push(path.into());
        current.cloned()
    }
}
fn source_entities(v: &Value) -> Result<Vec<Entity>, String> {
    let mut entities: Vec<Entity> = serde_json::from_value(v["entities"].clone())
        .map_err(|e| format!("PREFAB_INVALID: {e}"))?;
    if entities.is_empty() {
        return Err("PREFAB_INVALID: no entities".into());
    }
    let mut seen = std::collections::HashSet::new();
    for e in &mut entities {
        if !seen.insert(e.id) {
            return Err("PREFAB_INVALID: duplicate id".into());
        }
        for c in &mut e.components {
            c.props = forge_scene::normalize_props(&c.ctype, &c.props)?;
            forge_scene::validate_component(c)?;
        }
    }
    Ok(entities)
}
fn placement(args: &Value) -> Result<Transform, String> {
    let mut t = Transform::default();
    for k in ["translation", "rotation", "scale"] {
        if let Some(v) = args.get(k) {
            let a = v.as_array().ok_or("invalid placement")?;
            if a.len() != if k == "rotation" { 4 } else { 3 }
                || !a.iter().all(|v| v.as_f64().is_some_and(f64::is_finite))
            {
                return Err("invalid placement".into());
            }
            match k {
                "translation" => t.translation = serde_json::from_value(v.clone()).unwrap(),
                "scale" => t.scale = serde_json::from_value(v.clone()).unwrap(),
                _ => t.rotation = serde_json::from_value(v.clone()).unwrap(),
            }
        }
    }
    Ok(t)
}
fn combine(a: Transform, b: Transform) -> Transform {
    let q = a.rotation;
    let r = b.rotation;
    let rotation = [
        q[3] * r[0] + q[0] * r[3] + q[1] * r[2] - q[2] * r[1],
        q[3] * r[1] - q[0] * r[2] + q[1] * r[3] + q[2] * r[0],
        q[3] * r[2] + q[0] * r[1] - q[1] * r[0] + q[2] * r[3],
        q[3] * r[3] - q[0] * r[0] - q[1] * r[1] - q[2] * r[2],
    ];
    Transform {
        translation: crate::modelrt::point(crate::viewport::trs_model(&a), b.translation),
        rotation,
        scale: std::array::from_fn(|i| a.scale[i] * b.scale[i]),
    }
}
pub fn instantiate(scene: &Scene, args: &Value) -> Result<(Scene, Value), String> {
    let reference = args["prefabRef"].as_str().ok_or("prefabRef required")?;
    let (v, guid) = load(reference)?;
    let source = source_entities(&v)?;
    let place = placement(args)?;
    let mut result = scene.clone();
    let mut ids = HashMap::new();
    for e in &source {
        ids.insert(e.id, result.alloc_id());
    }
    let root_id = ids[&source[0].id];
    let revision = v["revision"].as_u64().unwrap_or(1);
    let mut created = Vec::new();
    for mut e in source {
        let local = e.id;
        e.id = ids[&local];
        if let Some(parent) = e.component_mut("Parent") {
            let p = parent.props["entity"].as_u64().ok_or("invalid parent id")?;
            parent.props["entity"] = json!(ids.get(&p).ok_or("missing prefab parent")?);
        } else {
            e.transform = combine(place, e.transform);
        }
        let baseline = entity_doc(&e);
        e.components.push(Component::new("PrefabInstance",json!({"prefabRef":guid,"model":v["model"],"revision":revision,"localId":local,"rootId":root_id,"placement":place,"baseline":baseline,"overrides":[]})));
        created.push(e.id);
        result.entities.push(e);
    }
    for e in &result.entities {
        crate::modelrt::entity_world(&result, e)?;
    }
    Ok((
        result,
        json!({"rootId":root_id,"entityIds":created,"revision":revision}),
    ))
}
pub fn refresh(
    scene: &Scene,
    only_root: Option<u64>,
    revert: bool,
) -> Result<(Scene, Value), String> {
    let roots: BTreeMap<u64, Value> = scene
        .entities
        .iter()
        .filter_map(|e| {
            let p = e.component("PrefabInstance")?;
            let r = p.props["rootId"].as_u64()?;
            (r == e.id && only_root.is_none_or(|x| x == r)).then_some((r, p.props.clone()))
        })
        .collect();
    let mut out = scene.clone();
    let mut conflicts = Vec::new();
    let mut updated = Vec::new();
    for (root_id, instance) in roots {
        let Some(reference) = instance["prefabRef"].as_str() else {
            continue;
        };
        let (v, _) = match load(reference) {
            Ok(v) => v,
            Err(e) => {
                conflicts.push(json!({"rootId":root_id,"reason":e}));
                continue;
            }
        };
        let revision = v["revision"].as_u64().unwrap_or(1);
        if !revert && revision == instance["revision"].as_u64().unwrap_or(0) {
            continue;
        }
        let mut source = source_entities(&v)?;
        if !revert {
            let mut omitted = instance["deletedLocalIds"]
                .as_array()
                .map(|a| {
                    a.iter()
                        .filter_map(Value::as_u64)
                        .collect::<std::collections::HashSet<_>>()
                })
                .unwrap_or_default();
            loop {
                let before = omitted.len();
                for e in &source {
                    if e.component("Parent")
                        .and_then(|p| p.props["entity"].as_u64())
                        .is_some_and(|id| omitted.contains(&id))
                    {
                        omitted.insert(e.id);
                    }
                }
                if omitted.len() == before {
                    break;
                }
            }
            source.retain(|e| !omitted.contains(&e.id));
        }
        let place: Transform =
            serde_json::from_value(instance["placement"].clone()).unwrap_or_default();
        let existing: HashMap<u64, u64> = out
            .entities
            .iter()
            .filter_map(|e| {
                let p = e.component("PrefabInstance")?;
                (p.props["rootId"].as_u64() == Some(root_id))
                    .then(|| Some((p.props["localId"].as_u64()?, e.id)))
                    .flatten()
            })
            .collect();
        let mut ids = existing.clone();
        for e in &source {
            if !ids.contains_key(&e.id) {
                ids.insert(e.id, out.alloc_id());
            }
        }
        let source_ids: std::collections::HashSet<_> = source.iter().map(|e| e.id).collect();
        for (local, id) in &existing {
            if !source_ids.contains(local) {
                let e = out.entity(*id).unwrap();
                let p = e.component("PrefabInstance").unwrap();
                let edited_descendant = out.entities.iter().any(|child| {
                    let Some(meta) = child.component("PrefabInstance") else {
                        return false;
                    };
                    if entity_doc(child) == meta.props["baseline"] {
                        return false;
                    }
                    let mut current = child;
                    let mut seen = std::collections::HashSet::new();
                    while let Some(parent) = current
                        .component("Parent")
                        .and_then(|p| p.props["entity"].as_u64())
                    {
                        if parent == *id {
                            return true;
                        }
                        if !seen.insert(parent) {
                            break;
                        }
                        let Some(p) = out.entity(parent) else { break };
                        current = p;
                    }
                    false
                });
                if revert || (entity_doc(e) == p.props["baseline"] && !edited_descendant) {
                    out.entities.retain(|e| e.id != *id);
                } else {
                    let old_revision = p.props["revision"].as_u64().unwrap_or(1);
                    let animation =
                        crate::modelrt::ancestor_component(&out, e, "Animator").cloned();
                    let retained = out.entity_mut(*id).unwrap();
                    if let Some(renderer) = retained.component_mut("ModelRenderer") {
                        renderer.props["revision"] = json!(old_revision);
                    }
                    if retained.component("Animator").is_none() {
                        if let Some(animation) = animation {
                            retained.components.push(animation);
                        }
                    }
                    retained.component_mut("PrefabInstance").unwrap().props["detached"] =
                        json!(true);
                    conflicts.push(json!({"id":id,"rootId":root_id,"reason":"removed template node has local changes"}));
                }
            }
        }
        for mut desired in source {
            let local = desired.id;
            desired.id = ids[&local];
            if let Some(parent) = desired.component_mut("Parent") {
                let p = parent.props["entity"].as_u64().ok_or("invalid parent")?;
                parent.props["entity"] = json!(ids.get(&p).ok_or("missing parent")?);
            } else {
                desired.transform = combine(place, desired.transform);
            }
            let next = entity_doc(&desired);
            let mut overrides = Vec::new();
            if let Some(current) = out.entity_mut(desired.id) {
                let old = current.component("PrefabInstance").unwrap().props["baseline"].clone();
                let merged = if revert {
                    next.clone()
                } else {
                    merge(
                        Some(&old),
                        Some(&entity_doc(current)),
                        Some(&next),
                        "",
                        &mut overrides,
                    )
                    .unwrap()
                };
                apply_doc(current, &merged)?;
                let p = current.component_mut("PrefabInstance").unwrap();
                p.props["baseline"] = next;
                p.props["revision"] = json!(revision);
                p.props["overrides"] = json!(overrides);
            } else {
                desired.components.push(Component::new("PrefabInstance",json!({"prefabRef":reference,"model":v["model"],"revision":revision,"localId":local,"rootId":root_id,"placement":place,"baseline":next,"overrides":[]})));
                out.entities.push(desired);
            }
        }
        updated.push(root_id);
        if revert {
            if let Some(root) = out
                .entity_mut(root_id)
                .and_then(|e| e.component_mut("PrefabInstance"))
            {
                root.props["deletedLocalIds"] = json!([]);
            }
        }
    }
    for e in &out.entities {
        crate::modelrt::entity_world(&out, e)?;
    }
    Ok((
        out,
        json!({"updatedInstances":updated,"conflicts":conflicts}),
    ))
}
pub fn root_id(scene: &Scene, id: u64) -> Result<u64, String> {
    scene
        .entity(id)
        .and_then(|e| e.component("PrefabInstance"))
        .and_then(|p| p.props["rootId"].as_u64())
        .ok_or_else(|| "entity is not a prefab instance".into())
}
#[cfg(test)]
mod tests {
    use super::*;
    fn template(revision: u64, child: bool) -> Value {
        let mut entities =
            vec![json!({"id":1,"name":"map","transform":Transform::default(),"components":[]})];
        if child {
            entities.push(json!({"id":100,"name":format!("box-v{revision}"),"transform":Transform{translation:[revision as f32,0.,0.],..Default::default()},"components":[{"type":"Parent","enabled":true,"props":{"entity":1}},{"type":"ModelRenderer","enabled":true,"props":{"model":"model-a","nodeId":"stable-box"}}]}));
        }
        json!({"version":1,"revision":revision,"model":"model-a","name":"map","entities":entities})
    }
    fn put(v: Value) {
        TEST_PREFABS
            .get_or_init(|| std::sync::Mutex::new(HashMap::new()))
            .lock()
            .unwrap()
            .insert("template-test".into(), v);
    }
    #[test]
    fn instance_nodes_updates_overrides_deletion_and_roundtrip() {
        put(template(1, true));
        let (a, first) = instantiate(
            &Scene::new("test"),
            &json!({"prefabRef":"template-test","translation":[10,0,0]}),
        )
        .unwrap();
        let (mut both, second) = instantiate(
            &a,
            &json!({"prefabRef":"template-test","translation":[20,0,0]}),
        )
        .unwrap();
        let child1 = first["entityIds"][1].as_u64().unwrap();
        let child2 = second["entityIds"][1].as_u64().unwrap();
        assert_eq!(
            crate::modelrt::point(
                crate::modelrt::entity_world(&both, both.entity(child1).unwrap()).unwrap(),
                [0.; 3]
            ),
            [11., 0., 0.]
        );
        both.entity_mut(child1).unwrap().transform.translation = [5., 0., 0.];
        both.entity_mut(child1)
            .unwrap()
            .component_mut("ModelRenderer")
            .unwrap()
            .props["materialOverrides"] = json!({"0":{"roughness":0.2}});
        put(template(2, true));
        let (updated, report) = refresh(&both, None, false).unwrap();
        assert_eq!(report["updatedInstances"].as_array().unwrap().len(), 2);
        assert_eq!(
            updated.entity(child1).unwrap().transform.translation,
            [5., 0., 0.]
        );
        assert_eq!(
            updated.entity(child2).unwrap().transform.translation,
            [2., 0., 0.]
        );
        assert_eq!(updated.entity(child1).unwrap().name, "box-v2");
        assert_eq!(
            updated
                .entity(child1)
                .unwrap()
                .component("ModelRenderer")
                .unwrap()
                .props["materialOverrides"]["0"]["roughness"],
            json!(0.2)
        );
        let text = updated.to_json().unwrap();
        let restored: Scene = serde_json::from_str(&text).unwrap();
        assert_eq!(restored, updated);
        put(template(3, false));
        let (deleted, report) = refresh(&restored, None, false).unwrap();
        assert!(deleted.entity(child1).is_some());
        assert!(deleted.entity(child2).is_none());
        assert_eq!(report["conflicts"].as_array().unwrap().len(), 1);
        assert_eq!(
            deleted
                .entity(child1)
                .unwrap()
                .component("ModelRenderer")
                .unwrap()
                .props["revision"],
            json!(2)
        );
        let (reverted, _) =
            refresh(&deleted, Some(first["rootId"].as_u64().unwrap()), true).unwrap();
        assert!(reverted.entity(child1).is_none());
        assert_eq!(
            reverted
                .entity(first["rootId"].as_u64().unwrap())
                .unwrap()
                .transform
                .translation,
            [10., 0., 0.]
        );
    }
    #[test]
    fn update_preserves_changed_leaf_and_takes_new_leaf() {
        let old = json!({"name":"hero","p":{"speed":1,"material":"old"}});
        let current = json!({"name":"my hero","p":{"speed":1,"material":"old"}});
        let new = json!({"name":"new hero","p":{"speed":2,"material":"new"}});
        let mut o = Vec::new();
        assert_eq!(
            merge(Some(&old), Some(&current), Some(&new), "", &mut o),
            Some(json!({"name":"my hero","p":{"speed":2,"material":"new"}}))
        );
        assert_eq!(o, vec!["/name"]);
    }
}
