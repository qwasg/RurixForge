//! Validation used by generic Assets operations, not only by Blender publication.
use super::*;
use crate::meta::MetaDoc;
use serde_json::Value;
use std::collections::{HashMap, HashSet};

pub(crate) fn asset_hash_matches(meta: &MetaDoc, bytes: &[u8]) -> bool {
    meta.provenance
        .as_ref()
        .and_then(|p| p.detail.as_ref())
        .and_then(|d| d.get("assetHash"))
        .and_then(Value::as_str)
        .is_none_or(|hash| hash == forge_util::hashutil::sha256_hex(bytes))
}

/// Check a complete template subtree independently of engine runtime state.
pub fn validate_template_document(doc: &Value) -> Result<()> {
    if doc["version"].as_u64() != Some(1) {
        return Err(model_error("prefab version must be 1"));
    }
    let entities = doc["entities"]
        .as_array()
        .filter(|v| !v.is_empty())
        .ok_or_else(|| model_error("prefab entities must be nonempty"))?;
    let mut ids = HashSet::new();
    let mut parents = HashMap::new();
    for entity in entities {
        let id = entity["id"]
            .as_u64()
            .filter(|i| *i > 0 && *i < (1u64 << 53))
            .ok_or_else(|| model_error("prefab entity id must be a positive safe integer"))?;
        if !ids.insert(id) {
            return Err(model_error("duplicate prefab entity id"));
        }
        if entity["name"].as_str().is_none() {
            return Err(model_error("prefab entity name missing"));
        }
        for (name, n) in [("translation", 3), ("rotation", 4), ("scale", 3)] {
            if !entity["transform"][name].as_array().is_some_and(|a| {
                a.len() == n && a.iter().all(|v| v.as_f64().is_some_and(f64::is_finite))
            }) {
                return Err(model_error(format!("invalid prefab transform.{name}")));
            }
        }
        let components = entity["components"]
            .as_array()
            .ok_or_else(|| model_error("prefab components must be an array"))?;
        let mut types = HashSet::new();
        for component in components {
            let ty = component["type"]
                .as_str()
                .ok_or_else(|| model_error("component type missing"))?;
            if !types.insert(ty)
                || !component["props"].is_object()
                || !component["enabled"].is_boolean()
            {
                return Err(model_error("invalid/duplicate prefab component"));
            }
            if ty == "Parent" {
                parents.insert(
                    id,
                    component["props"]["entity"]
                        .as_u64()
                        .ok_or_else(|| model_error("invalid prefab parent id"))?,
                );
            }
        }
    }
    for (&child, &parent) in &parents {
        if !ids.contains(&parent) {
            return Err(model_error("prefab parent does not exist"));
        }
        let mut chain = HashSet::new();
        let mut next = Some(child);
        while let Some(id) = next {
            if !chain.insert(id) {
                return Err(model_error("prefab parent cycle"));
            }
            next = parents.get(&id).copied();
        }
    }
    Ok(())
}

/// Generic asset_reimport validates what is present. Blender-generated files are immutable
/// derivatives; editing them requires publishing the bound source, not relabeling them current.
pub fn validate_asset_document(project: &ForgeProject, rel: &str) -> Result<()> {
    let path = project.resolve_content_path(rel)?;
    let bytes = std::fs::read(&path)?;
    let meta = MetaDoc::load(&crate::meta_path_for(&project.content_root(), rel))?;
    if !asset_hash_matches(&meta, &bytes) {
        return Err(crate::AssetError::new("MODEL_SOURCE_MODIFIED","generated asset differs from its published revision; republish the bound Blender source"));
    }
    match meta.atype.as_str() {
        "model" => {
            let bundle: ModelBundle =
                serde_json::from_slice(&bytes).map_err(|e| model_error(e.to_string()))?;
            validate_model_bundle(&bundle)?;
            if bundle.guid != meta.guid {
                return Err(model_error("model GUID differs from sidecar"));
            }
            let mut known = HashMap::new();
            for p in project.scan_content()? {
                if let Ok(m) = MetaDoc::load(&crate::meta_path_for(&project.content_root(), &p)) {
                    known.insert(m.guid, (p, m.atype));
                }
            }
            for (guid, kind) in bundle
                .textures
                .iter()
                .map(|t| (&t.guid, "texture"))
                .chain(bundle.materials.iter().map(|m| (&m.guid, "material")))
            {
                if !known.get(guid).is_some_and(|(_, t)| t == kind) {
                    return Err(crate::AssetError::new(
                        "MODEL_DEPENDENCY_MISSING",
                        format!("missing {kind} asset {guid}"),
                    ));
                }
                validate_asset_document(project, &known[guid].0)?;
            }
        }
        "prefab" => {
            let doc: Value =
                serde_json::from_slice(&bytes).map_err(|e| model_error(e.to_string()))?;
            validate_template_document(&doc)?;
            let mut loaded = HashMap::new();
            for e in doc["entities"].as_array().unwrap() {
                for c in e["components"].as_array().unwrap() {
                    if matches!(
                        c["type"].as_str(),
                        Some("ModelRenderer" | "ModelNode" | "Collider")
                    ) {
                        if let Some(reference) =
                            c["props"]["model"].as_str().filter(|s| !s.is_empty())
                        {
                            if !loaded.contains_key(reference) {
                                loaded
                                    .insert(reference.to_string(), load_model(project, reference)?);
                            }
                            if let Some(node) =
                                c["props"]["nodeId"].as_str().filter(|s| !s.is_empty())
                            {
                                if !loaded[reference].nodes.iter().any(|n| n.id == node) {
                                    return Err(model_error(format!(
                                        "prefab node reference missing: {node}"
                                    )));
                                }
                            }
                        }
                    }
                }
            }
        }
        "texture" => {
            crate::texture::decode_size(&path)?;
        }
        "material" => {
            let doc: Value =
                serde_json::from_slice(&bytes).map_err(|e| model_error(e.to_string()))?;
            crate::material::validate_rxmat(&doc)?;
        }
        _ => {}
    }
    Ok(())
}
