//! Canonical renderable board documents. Missing presentation fields receive explicit,
//! versioned defaults; malformed supplied fields and dangling references are rejected.
//! Unknown extension fields are retained, including collaboration bindings and history.
use serde_json::{json, Map, Value};
use std::collections::{HashMap, HashSet};

type Result<T> = std::result::Result<T, String>;
fn invalid(path: &str, message: &str) -> String {
    format!("EDITOR_DOCUMENT_INVALID: {path}: {message}")
}
fn object<'a>(value: &'a mut Value, path: &str) -> Result<&'a mut Map<String, Value>> {
    value
        .as_object_mut()
        .ok_or_else(|| invalid(path, "object required"))
}
fn default(value: &mut Value, key: &str, fallback: Value) {
    value
        .as_object_mut()
        .unwrap()
        .entry(key)
        .or_insert(fallback);
}
fn string<'a>(value: &'a Value, key: &str, path: &str) -> Result<&'a str> {
    value[key]
        .as_str()
        .ok_or_else(|| invalid(path, &format!("{key} must be a string")))
}
fn id(value: &Value, path: &str) -> Result<String> {
    let id = string(value, "id", path)?;
    if id.is_empty() || id.len() > 256 {
        return Err(invalid(path, "id must contain 1..256 bytes"));
    }
    Ok(id.into())
}
fn string_default(value: &mut Value, key: &str, fallback: &str, path: &str) -> Result<()> {
    default(value, key, json!(fallback));
    string(value, key, path).map(|_| ())
}
fn bool_default(value: &mut Value, key: &str, fallback: bool, path: &str) -> Result<()> {
    default(value, key, json!(fallback));
    if !value[key].is_boolean() {
        return Err(invalid(path, &format!("{key} must be boolean")));
    }
    Ok(())
}
fn array<'a>(value: &'a mut Value, key: &str, path: &str) -> Result<&'a mut Vec<Value>> {
    default(value, key, json!([]));
    value[key]
        .as_array_mut()
        .ok_or_else(|| invalid(path, &format!("{key} must be an array")))
}
fn string_array(value: &mut Value, key: &str, path: &str) -> Result<()> {
    if array(value, key, path)?.iter().any(|v| !v.is_string()) {
        return Err(invalid(path, &format!("{key} must contain strings")));
    }
    Ok(())
}
fn position(value: &mut Value, path: &str) -> Result<()> {
    default(value, "pos", json!([80, 80]));
    if !value["pos"]
        .as_array()
        .is_some_and(|a| a.len() == 2 && a.iter().all(|v| v.as_f64().is_some_and(f64::is_finite)))
    {
        return Err(invalid(path, "pos must contain two finite numbers"));
    }
    Ok(())
}
fn unique(ids: &mut HashSet<String>, value: &Value, path: &str) -> Result<String> {
    let key = id(value, path)?;
    if !ids.insert(key.clone()) {
        return Err(invalid(path, "duplicate id"));
    }
    Ok(key)
}
fn builtin_kinds() -> Value {
    json!([
        {"id":"role","label":"角色","builtin":true,"tone":"info","category":"role","defaultFeatures":[]},
        {"id":"map","label":"地图","builtin":true,"tone":"sage","category":"map","defaultFeatures":[]}
    ])
}

pub(super) fn normalize(kind: &str, mut doc: Value) -> Result<Value> {
    if !matches!(kind, "blueprint" | "studio") {
        return Ok(doc);
    }
    object(&mut doc, "document")?;
    let version = match doc.get("version") {
        None => None,
        Some(value) => Some(
            value
                .as_u64()
                .ok_or_else(|| invalid("document", "version must be an integer"))?,
        ),
    };
    let supported = if kind == "blueprint" {
        version.is_none() || matches!(version, Some(2 | 3))
    } else {
        version.is_none() || matches!(version, Some(1 | 2))
    };
    if !supported {
        return Err("EDITOR_DOCUMENT_VERSION_UNSUPPORTED: migrate this document with a compatible editor first".into());
    }
    doc["version"] = json!(if kind == "blueprint" { 3 } else { 2 });
    default(&mut doc, "seq", json!(1));
    if !doc["seq"]
        .as_u64()
        .is_some_and(|seq| seq >= 1 && seq < 9_007_199_254_740_991)
    {
        return Err(invalid("document", "seq must be a positive safe integer"));
    }
    let mut node_ids = HashSet::new();
    if kind == "blueprint" {
        blueprint(&mut doc, &mut node_ids)?;
    } else {
        studio(&mut doc, &mut node_ids)?;
    }
    let edges = array(&mut doc, "edges", "document")?;
    if edges.len() > 50_000 {
        return Err(invalid("document", "too many edges"));
    }
    let mut edge_ids = HashSet::new();
    for (index, edge) in edges.iter_mut().enumerate() {
        let path = format!("edges[{index}]");
        object(edge, &path)?;
        unique(&mut edge_ids, edge, &path)?;
        string_default(edge, "label", "", &path)?;
        if kind == "studio" {
            for end in ["from", "to"] {
                let node = string(edge, end, &path)?;
                if !node_ids.contains(node) {
                    return Err(invalid(&path, "edge points to a missing node"));
                }
            }
        }
    }
    if kind == "blueprint" {
        validate_board_edges(&mut doc, &node_ids)?;
    }
    // Both boards use one sequence for node/edge/feature/version IDs. Fill or repair
    // the counter so adding the next item cannot reuse an existing stable identity.
    let mut seq = doc["seq"].as_u64().unwrap();
    let mut observe = |item: &Value| {
        if let Some(id) = item["id"].as_str() {
            if id.len() > 1 && id.as_bytes()[0].is_ascii_alphabetic() {
                if let Ok(number) = id[1..].parse::<u64>() {
                    seq = seq.max(number.saturating_add(1));
                }
            }
        }
    };
    for field in ["nodes", "edges", "kinds"] {
        for item in doc[field].as_array().into_iter().flatten() {
            observe(item);
            for child in ["features", "assets", "versions"] {
                for item in item[child].as_array().into_iter().flatten() {
                    observe(item);
                }
            }
        }
    }
    if seq >= 9_007_199_254_740_991 {
        return Err(invalid(
            "document",
            "generated identity sequence exceeds safe integer range",
        ));
    }
    doc["seq"] = json!(seq);
    Ok(doc)
}

fn blueprint(doc: &mut Value, node_ids: &mut HashSet<String>) -> Result<()> {
    if doc.get("kinds").is_none() {
        let mut kinds = builtin_kinds();
        if let Some(custom) = doc.get("customKinds") {
            let custom = custom
                .as_array()
                .ok_or_else(|| invalid("document", "customKinds must be an array"))?;
            kinds.as_array_mut().unwrap().extend(custom.iter().cloned());
        }
        doc["kinds"] = kinds;
    }
    let kinds = doc["kinds"]
        .as_array_mut()
        .ok_or_else(|| invalid("document", "kinds must be an array"))?;
    let mut kind_ids = HashSet::new();
    for (index, kind) in kinds.iter_mut().enumerate() {
        let path = format!("kinds[{index}]");
        object(kind, &path)?;
        let name = unique(&mut kind_ids, kind, &path)?;
        string_default(kind, "label", &name, &path)?;
        bool_default(kind, "builtin", false, &path)?;
        string_default(kind, "tone", "info", &path)?;
        string_default(kind, "category", "interaction", &path)?;
        if !matches!(
            kind["tone"].as_str(),
            Some("info" | "sage" | "warn" | "acc" | "danger")
        ) {
            return Err(invalid(&path, "unknown tone"));
        }
        if !matches!(
            kind["category"].as_str(),
            Some("role" | "map" | "interaction")
        ) {
            return Err(invalid(&path, "unknown category"));
        }
        string_array(kind, "defaultFeatures", &path)?;
    }
    let nodes = array(doc, "nodes", "document")?;
    if nodes.len() > 10_000 {
        return Err(invalid("document", "too many nodes"));
    }
    for (index, node) in nodes.iter_mut().enumerate() {
        let path = format!("nodes[{index}]");
        object(node, &path)?;
        let name = unique(node_ids, node, &path)?;
        string_default(node, "kindId", "role", &path)?;
        if !kind_ids.contains(string(node, "kindId", &path)?) {
            return Err(invalid(&path, "kindId points to a missing kind"));
        }
        string_default(node, "name", &name, &path)?;
        string_default(node, "desc", "", &path)?;
        position(node, &path)?;
        bool_default(node, "collapsed", false, &path)?;
        let mut feature_ids = HashSet::new();
        for (index, feature) in array(node, "features", &path)?.iter_mut().enumerate() {
            let path = format!("{path}.features[{index}]");
            object(feature, &path)?;
            unique(&mut feature_ids, feature, &path)?;
            string(feature, "type", &path)?;
            bool_default(feature, "custom", false, &path)?;
            string_default(feature, "note", "", &path)?;
        }
        let mut asset_ids = HashSet::new();
        for (index, asset) in array(node, "assets", &path)?.iter_mut().enumerate() {
            let path = format!("{path}.assets[{index}]");
            object(asset, &path)?;
            unique(&mut asset_ids, asset, &path)?;
            for key in ["guid", "path", "type"] {
                string(asset, key, &path)?;
            }
            string_default(asset, "note", "", &path)?;
            position(asset, &path)?;
        }
    }
    default(doc, "bindings", json!({}));
    let bindings = doc["bindings"]
        .as_object()
        .ok_or_else(|| invalid("document", "bindings must be an object"))?;
    for (node, binding) in bindings {
        if !node_ids.contains(node) {
            return Err(invalid("bindings", "binding points to a missing node"));
        }
        if !binding.is_null() && !binding.is_object() {
            return Err(invalid("bindings", "binding must be an object or null"));
        }
        for key in ["sceneGuid", "entityGuid", "changeSetId"] {
            if binding.get(key).is_some_and(|v| !v.is_string()) {
                return Err(invalid("bindings", &format!("{key} must be a string")));
            }
        }
        if binding
            .get("entityId")
            .is_some_and(|v| !v.is_null() && !v.is_u64())
        {
            return Err(invalid("bindings", "entityId must be an integer or null"));
        }
    }
    Ok(())
}

fn validate_board_edges(doc: &mut Value, node_ids: &HashSet<String>) -> Result<()> {
    let features: HashMap<String, HashSet<String>> = doc["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|node| {
            (
                node["id"].as_str().unwrap().to_owned(),
                node["features"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|f| f["id"].as_str().unwrap().to_owned())
                    .collect(),
            )
        })
        .collect();
    for (index, edge) in doc["edges"].as_array_mut().unwrap().iter_mut().enumerate() {
        let path = format!("edges[{index}]");
        for end in ["from", "to"] {
            if let Some(node) = edge[end].as_str() {
                edge[end] = json!({"node":node});
            }
            object(&mut edge[end], &path)?;
            let node = string(&edge[end], "node", &path)?;
            if !node_ids.contains(node) {
                return Err(invalid(&path, "edge points to a missing node"));
            }
            if let Some(feature) = edge[end].get("feature") {
                let feature = feature
                    .as_str()
                    .ok_or_else(|| invalid(&path, "feature must be a string"))?;
                if !features[node].contains(feature) {
                    return Err(invalid(&path, "edge points to a missing feature"));
                }
            }
        }
    }
    Ok(())
}

fn studio(doc: &mut Value, node_ids: &mut HashSet<String>) -> Result<()> {
    string_array(doc, "readonlyWorkspaceIds", "document")?;
    bool_default(doc, "includeLibrary", true, "document")?;
    let nodes = array(doc, "nodes", "document")?;
    if nodes.len() > 10_000 {
        return Err(invalid("document", "too many nodes"));
    }
    for (index, node) in nodes.iter_mut().enumerate() {
        let path = format!("nodes[{index}]");
        object(node, &path)?;
        let name = unique(node_ids, node, &path)?;
        string_default(node, "preset", "outline", &path)?;
        if !matches!(
            node["preset"].as_str(),
            Some(
                "outline"
                    | "mapdraft"
                    | "concept"
                    | "texture"
                    | "ui"
                    | "mesh"
                    | "video"
                    | "audio"
                    | "charanim"
            )
        ) {
            return Err(invalid(&path, "unknown studio preset"));
        }
        string_default(node, "name", &name, &path)?;
        string_default(node, "prompt", "", &path)?;
        position(node, &path)?;
        default(node, "params", json!({}));
        object(&mut node["params"], &path)?;
        let mut versions = HashSet::new();
        for (index, version) in array(node, "versions", &path)?.iter_mut().enumerate() {
            let path = format!("{path}.versions[{index}]");
            object(version, &path)?;
            unique(&mut versions, version, &path)?;
            if !version["createdAt"]
                .as_f64()
                .is_some_and(|v| v.is_finite() && v >= 0.)
            {
                return Err(invalid(
                    &path,
                    "createdAt must be a nonnegative finite timestamp",
                ));
            }
            string(version, "backendId", &path)?;
            string(version, "prompt", &path)?;
            for key in ["text", "fileRef", "mime", "assetPath", "guid", "spritePath"] {
                if version.get(key).is_some_and(|v| !v.is_string()) {
                    return Err(invalid(&path, &format!("{key} must be a string")));
                }
            }
        }
        default(node, "currentVersionId", Value::Null);
        if !node["currentVersionId"].is_null()
            && !node["currentVersionId"]
                .as_str()
                .is_some_and(|id| versions.contains(id))
        {
            return Err(invalid(
                &path,
                "currentVersionId points to a missing version",
            ));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn defaults_are_versioned_and_preserve_unknown_fields() {
        let doc = normalize(
            "blueprint",
            json!({"nodes":[{"id":"hero","future":{"value":42}}],"futureTop":true}),
        )
        .unwrap();
        assert_eq!(doc["version"], 3);
        assert_eq!(doc["nodes"][0]["kindId"], "role");
        assert_eq!(doc["nodes"][0]["future"]["value"], 42);
        assert!(
            doc["edges"].is_array()
                && doc["nodes"][0]["assets"].is_array()
                && doc["futureTop"] == true
        );
        assert_eq!(normalize("blueprint", doc.clone()).unwrap(), doc);
        assert_eq!(
            normalize("studio", json!({"nodes":[{"id":"s17"}]})).unwrap()["seq"],
            18
        );
    }
    #[test]
    fn malformed_supplied_fields_and_dangling_references_are_rejected() {
        for doc in [
            json!({"nodes":[{"id":"n","pos":"bad"}]}),
            json!({"nodes":[{"id":"n"},{"id":"n"}]}),
            json!({"nodes":[{"id":"n","kindId":"missing"}]}),
            json!({"nodes":[],"edges":[{"id":"e","from":{"node":"n"},"to":{"node":"n"}}]}),
            json!({"nodes":[],"bindings":{"missing":{}}}),
        ] {
            assert!(normalize("blueprint", doc)
                .unwrap_err()
                .starts_with("EDITOR_DOCUMENT_INVALID:"));
        }
    }
    #[test]
    fn studio_normalization_never_discards_versions_or_unknown_metadata() {
        let doc=normalize("studio",json!({"version":1,"nodes":[{"id":"n","versions":[{"id":"v","createdAt":1,"backendId":"test","prompt":"p","text":"kept","custom":true}],"currentVersionId":"v"}]})).unwrap();
        assert_eq!(doc["version"], 2);
        assert_eq!(doc["nodes"][0]["versions"][0]["text"], "kept");
        assert_eq!(doc["nodes"][0]["versions"][0]["custom"], true);
        let mut invalid_doc = doc;
        invalid_doc["nodes"][0]["currentVersionId"] = json!("missing");
        assert!(normalize("studio", invalid_doc).is_err());
    }
    #[test]
    fn feature_anchors_are_validated_and_legacy_string_anchors_upgrade_losslessly() {
        let doc=normalize("blueprint",json!({"version":2,"nodes":[{"id":"n","features":[{"id":"f","type":"Script"}]}],"edges":[{"id":"e","from":"n","to":{"node":"n","feature":"f"}}]})).unwrap();
        assert_eq!(doc["edges"][0]["from"], json!({"node":"n"}));
        let mut invalid_doc = doc;
        invalid_doc["edges"][0]["to"]["feature"] = json!("missing");
        assert!(normalize("blueprint", invalid_doc).is_err());
    }
}
