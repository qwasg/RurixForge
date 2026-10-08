//! Versioned Shader Graph files and material instances. Draft saving and runtime publication are distinct.
use crate::{meta::MetaDoc, project::ForgeProject, AssetError, Result};
pub use forge_shader::GraphDoc;
use serde_json::{json, Value};
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};

pub use forge_shader::{catalog, CompiledGraph, Diagnostic};

fn write_lock(project: &ForgeProject) -> Result<std::fs::File> {
    let dir = project.root.join(".forge/locks");
    std::fs::create_dir_all(&dir)?;
    let file = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(dir.join("shader-writes.lock"))?;
    fs2::FileExt::lock_exclusive(&file)?;
    Ok(file)
}

pub fn resolve(project: &ForgeProject, reference: &str) -> Result<(String, PathBuf)> {
    let reference = reference.strip_prefix("Content/").unwrap_or(reference);
    if reference.contains('/')
        || reference.ends_with(".rxshadergraph")
        || reference.ends_with(".rxmat")
    {
        let rel = crate::normalize_rel(reference)?;
        let abs = project.resolve_content_path(&rel)?;
        return Ok((rel, abs));
    }
    for rel in project.scan_content()? {
        let path = crate::meta_path_for(&project.content_root(), &rel);
        if let Ok(m) = MetaDoc::load(&path) {
            if m.guid == reference {
                let abs = project.resolve_content_path(&rel)?;
                return Ok((rel, abs));
            }
        }
    }
    Err(AssetError::new(
        "SHADER_REFERENCE_NOT_FOUND",
        format!("Unknown asset reference {reference}"),
    ))
}
pub fn load(project: &ForgeProject, reference: &str) -> Result<Value> {
    let (rel, abs) = resolve(project, reference)?;
    if !rel.ends_with(".rxshadergraph") {
        return Err(AssetError::new(
            "SHADER_REFERENCE_TYPE",
            "Reference is not a Shader Graph",
        ));
    }
    let bytes = std::fs::read(abs)?;
    if bytes.len() > 1024 * 1024 {
        return Err(AssetError::new(
            "SHADER_BUDGET",
            "Shader Graph exceeds 1 MiB",
        ));
    }
    let graph: GraphDoc = serde_json::from_slice(&bytes)
        .map_err(|e| AssetError::new("SHADER_PARSE", e.to_string()))?;
    let meta = MetaDoc::load(&crate::meta_path_for(&project.content_root(), &rel)).ok();
    Ok(
        json!({"path":rel,"guid":meta.map(|m|m.guid),"sourceHash":forge_shader::content_hash(&bytes),"graph":graph}),
    )
}
pub fn compile(project: &ForgeProject, reference: &str) -> Result<Value> {
    let loaded = load(project, reference)?;
    compile_document(&loaded["graph"]).map(|mut result| {
        result["sourceHash"] = loaded["sourceHash"].clone();
        result["path"] = loaded["path"].clone();
        result
    })
}
pub fn compile_document(value: &Value) -> Result<Value> {
    let graph: GraphDoc = serde_json::from_value(value.clone())
        .map_err(|e| AssetError::new("SHADER_PARSE", e.to_string()))?;
    Ok(match forge_shader::compile(&graph) {
        Ok(c) => {
            json!({"ok":true,"compiled":c,"diagnostics":[],"validation":{"rurix":"spirv-compiled","godot":"source-generated"}})
        }
        Err(d) => json!({"ok":false,"diagnostics":d}),
    })
}
/// Existing documents require the exact bytes hash, including concurrent external editor changes.
/// Process and project-file gates serialize cooperating editor/agent writers around the hash check.
pub fn save(
    project: &ForgeProject,
    path: &str,
    graph: &GraphDoc,
    expected_hash: Option<&str>,
) -> Result<Value> {
    static WRITES: OnceLock<Mutex<()>> = OnceLock::new();
    let _gate = WRITES.get_or_init(|| Mutex::new(())).lock().unwrap();
    let _file_gate = write_lock(project)?;
    let rel = crate::normalize_rel(path.strip_prefix("Content/").unwrap_or(path))?;
    if !rel.ends_with(".rxshadergraph") {
        return Err(AssetError::new("SHADER_PATH", "Expected .rxshadergraph"));
    }
    let abs = project.resolve_content_path(&rel)?;
    if abs.is_file() {
        let hash = forge_shader::content_hash(&std::fs::read(&abs)?);
        if expected_hash != Some(hash.as_str()) {
            return Err(AssetError::new(
                "SHADER_CONFLICT",
                "Shader changed since it was read; reload before editing",
            ));
        }
    } else if expected_hash.is_some() {
        return Err(AssetError::new(
            "SHADER_CONFLICT",
            "Shader no longer exists",
        ));
    }
    let bytes = serde_json::to_vec_pretty(graph)
        .map_err(|e| AssetError::new("SHADER_PARSE", e.to_string()))?;
    if bytes.len() > 1024 * 1024
        || graph.version != 1
        || graph.nodes.len() > 256
        || graph.parameters.len() > 64
    {
        return Err(AssetError::new(
            "SHADER_BUDGET",
            "Invalid graph version or document budget exceeded",
        ));
    }
    if let Some(p) = abs.parent() {
        std::fs::create_dir_all(p)?;
    }
    // File::sync_all ensures the source is durable before metadata becomes visible.
    use std::io::Write;
    let tmp = abs.with_extension(format!("rxshadergraph.{}.tmp", crate::new_guid()));
    let result = (|| -> std::io::Result<()> {
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&tmp)?;
        file.write_all(&bytes)?;
        file.sync_all()?;
        std::fs::rename(&tmp, &abs)
    })();
    if let Err(error) = result {
        let _ = std::fs::remove_file(&tmp);
        return Err(error.into());
    }
    let (_, mut meta) = crate::meta::ensure_meta(&project.content_root(), &rel)?;
    let diagnostics = forge_shader::compile(graph).err().unwrap_or_default();
    meta.build_state = Some(
        if diagnostics.is_empty() {
            "current"
        } else {
            "failed"
        }
        .into(),
    );
    meta.save(&crate::meta_path_for(&project.content_root(), &rel))?;
    crate::refs::RefGraph::rebuild(project)?;
    Ok(
        json!({"path":rel,"guid":meta.guid,"sourceHash":forge_shader::content_hash(&bytes),"graph":graph,"diagnostics":diagnostics,"published":false}),
    )
}
pub fn create_material(
    project: &ForgeProject,
    path: &str,
    shader_ref: &str,
    params: &Value,
    textures: &Value,
) -> Result<Value> {
    let _file_gate = write_lock(project)?;
    let graph = load(project, shader_ref)?;
    let guid = graph["guid"].as_str().ok_or_else(|| {
        AssetError::new("SHADER_META_MISSING", "Shader Graph requires an asset GUID")
    })?;
    let rel = crate::normalize_rel(path.strip_prefix("Content/").unwrap_or(path))?;
    if !rel.ends_with(".rxmat") {
        return Err(AssetError::new("MATERIAL_INVALID", "Expected .rxmat"));
    }
    let abs = project.resolve_content_path(&rel)?;
    if abs.exists() {
        return Err(AssetError::new(
            "MATERIAL_EXISTS",
            "Material already exists",
        ));
    }
    let doc = json!({"version":2,"shaderGraph":guid,"params":params,"textures":textures});
    crate::material::validate_rxmat(&doc)?;
    let typed: GraphDoc = serde_json::from_value(graph["graph"].clone())
        .map_err(|e| AssetError::new("SHADER_PARSE", e.to_string()))?;
    forge_shader::compile(&typed)
        .map_err(|d| AssetError::new("SHADER_INVALID", serde_json::to_string(&d).unwrap()))?;
    for (values, texture_group) in [(params, false), (textures, true)] {
        let values = values
            .as_object()
            .ok_or_else(|| AssetError::new("SHADER_PARAMETER", "Expected parameter object"))?;
        for (id, value) in values {
            let p = typed
                .parameters
                .iter()
                .find(|p| &p.id == id)
                .ok_or_else(|| {
                    AssetError::new("SHADER_PARAMETER", format!("Unknown parameter {id}"))
                })?;
            if (p.ty == forge_shader::ValueType::Texture2d) != texture_group
                || !forge_shader::validate_parameter(p.ty, value)
            {
                return Err(AssetError::new(
                    "SHADER_PARAMETER",
                    format!("Wrong type for parameter {id}"),
                ));
            }
            if texture_group && value.as_str() != Some("") {
                let (rel, _) = resolve(project, value.as_str().unwrap())?;
                let meta = MetaDoc::load(&crate::meta_path_for(&project.content_root(), &rel))?;
                if meta.atype != "texture" || Some(meta.guid.as_str()) != value.as_str() {
                    return Err(AssetError::new(
                        "SHADER_TEXTURE",
                        "Textures require a texture asset GUID",
                    ));
                }
            }
        }
    }
    if let Some(p) = abs.parent() {
        std::fs::create_dir_all(p)?;
    }
    use std::io::Write;
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(abs)?;
    file.write_all(&serde_json::to_vec_pretty(&doc).unwrap())?;
    file.sync_all()?;
    let (_, meta) = crate::meta::ensure_meta(&project.content_root(), &rel)?;
    crate::refs::RefGraph::rebuild(project)?;
    Ok(json!({"path":rel,"guid":meta.guid,"material":doc}))
}
