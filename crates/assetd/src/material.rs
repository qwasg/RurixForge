//! 材质资产(08 §4.2):.rxmat JSON 创建/校验 + 纹理 GUID 引用建边(material→texture)。
//!
//! .rxmat 格式(JSON,键有序确定性写出):
//! ```json
//! { "version": 1, "shader": "pbr-default", "params": { ... }, "textures": { "albedo": "<guid>" } }
//! ```
//! rurix-render material closure 绑定上游无公开面 → 本波如实记"参数记录 + 引用边",
//! closure id 绑定留 RD(见契约 deferred)。

use serde_json::{json, Map, Value};

use crate::meta::{MetaDoc, Provenance};
use crate::project::ForgeProject;
use crate::{meta_path_for, normalize_rel, new_guid, AssetError, Result};

/// 校验 .rxmat 文档:version==1;shader 非空串;params 对象;textures 对象且值为 string(GUID)。
pub fn validate_rxmat(v: &Value) -> Result<()> {
    let obj = v
        .as_object()
        .ok_or_else(|| AssetError::new("MATERIAL_INVALID", ".rxmat 须为 JSON 对象"))?;
    match obj.get("version").and_then(Value::as_u64) {
        Some(1) => {}
        _ => return Err(AssetError::new("MATERIAL_INVALID", ".rxmat version 须为 1")),
    }
    match obj.get("shader").and_then(Value::as_str) {
        Some(s) if !s.is_empty() => {}
        _ => return Err(AssetError::new("MATERIAL_INVALID", ".rxmat shader 须为非空串")),
    }
    if let Some(p) = obj.get("params") {
        if !p.is_object() {
            return Err(AssetError::new("MATERIAL_INVALID", ".rxmat params 须为对象"));
        }
    }
    if let Some(t) = obj.get("textures") {
        let t = t
            .as_object()
            .ok_or_else(|| AssetError::new("MATERIAL_INVALID", ".rxmat textures 须为对象"))?;
        for (slot, guid) in t {
            if !guid.is_string() {
                return Err(AssetError::new(
                    "MATERIAL_INVALID",
                    format!("textures.{slot} 须为 GUID 字符串"),
                ));
            }
        }
    }
    Ok(())
}

/// 收集项目内全部已知 GUID(扫 Content/ .meta)。
fn known_guids(project: &ForgeProject) -> Result<std::collections::HashSet<String>> {
    let mut set = std::collections::HashSet::new();
    for rel in project.scan_content()? {
        let mp = meta_path_for(&project.content_root(), &rel);
        if mp.is_file() {
            if let Ok(m) = MetaDoc::load(&mp) {
                set.insert(m.guid);
            }
        }
    }
    Ok(set)
}

/// material_create 返回。
#[derive(Debug, Clone)]
pub struct MaterialCreated {
    pub asset_path: String,
    pub guid: String,
    /// 纹理引用(textures 槽位 → GUID),引用图重建时自动成边(material→texture)。
    pub texture_refs: Vec<(String, String)>,
}

/// 创建材质资产:写 .rxmat(确定性 JSON)+ .meta(GUID 新建);纹理 GUID 须已存在。
pub fn create_material(
    project: &ForgeProject,
    dest_folder: &str,
    name: &str,
    shader: Option<&str>,
    params: Option<&Map<String, Value>>,
    textures: Option<&Map<String, Value>>,
) -> Result<MaterialCreated> {
    if name.is_empty() || name.contains('/') || name.contains('\\') || name.contains('.') {
        return Err(AssetError::new(
            "INVALID_OPS",
            format!("材质名非法(禁含 / \\ .): {name}"),
        ));
    }
    let folder = if dest_folder.is_empty() {
        "Materials".to_string()
    } else {
        normalize_rel(dest_folder)?
    };
    let rel = format!("{folder}/{name}.rxmat");

    // textures 值提取 + GUID 存在性校验(08 §4.2 参数校验)。
    let mut texture_refs: Vec<(String, String)> = Vec::new();
    if let Some(t) = textures {
        let known = known_guids(project)?;
        for (slot, g) in t {
            let guid = g.as_str().ok_or_else(|| {
                AssetError::new("MATERIAL_INVALID", format!("textures.{slot} 须为 GUID 字符串"))
            })?;
            if !known.contains(guid) {
                return Err(AssetError::new(
                    "UNKNOWN_GUID",
                    format!("textures.{slot} 引用的 GUID 不存在: {guid}"),
                ));
            }
            texture_refs.push((slot.clone(), guid.to_string()));
        }
    }

    let doc = json!({
        "version": 1,
        "shader": shader.unwrap_or("pbr-default"),
        "params": params.cloned().unwrap_or_default(),
        "textures": textures.cloned().unwrap_or_default(),
    });
    validate_rxmat(&doc)?;

    // 确定性写出:serde_json Map 默认 BTreeMap(键字典序)+ 两空格缩进 + 末尾换行。
    let text = serde_json::to_string_pretty(&doc)
        .map_err(|e| AssetError::new("META_SERIALIZE", format!(".rxmat 序列化失败: {e}")))?;
    let abs = project.content_root().join(&rel);
    if let Some(parent) = abs.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&abs, format!("{text}\n"))?;

    // .meta(新 GUID;reimport 语义 = 复用已有 .meta 的 GUID)。
    let meta_path = meta_path_for(&project.content_root(), &rel);
    let mut meta = if meta_path.is_file() {
        MetaDoc::load(&meta_path)?
    } else {
        MetaDoc::new(&rel, new_guid())?
    };
    if meta.provenance.is_none() {
        meta.provenance = Some(Provenance {
            origin: "user-import".into(),
            detail: None,
        });
    }
    meta.build_state = Some("current".into());
    meta.save(&meta_path)?;

    Ok(MaterialCreated {
        asset_path: rel,
        guid: meta.guid,
        texture_refs,
    })
}
