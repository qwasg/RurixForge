//! 导入流程(08 §4.1):复制进 Content/ → 生成 .meta → 构建 → 记引用边。

use std::path::{Path, PathBuf};

use serde_json::{json, Value};

use crate::meta::{MetaDoc, Provenance};
use crate::project::ForgeProject;
use crate::{meta_path_for, normalize_rel, new_guid, AssetError, AssetType, Result};

/// 单个资产导入结果。
#[derive(Debug, Clone)]
pub struct ImportOne {
    pub asset_path: String,
    pub guid: String,
    pub atype: AssetType,
    /// 缓存命中 = true(二次导入零重建)。
    pub cache_hit: bool,
    /// 构建产物相对路径(网格 = .rxmesh;贴图 = None)。
    pub artifact: Option<String>,
    pub vertex_count: Option<u32>,
    pub triangle_count: Option<u32>,
    /// 贴图解码尺寸(wave.4;非贴图 = None)。
    pub width: Option<u32>,
    pub height: Option<u32>,
}

/// 导入失败条目。
#[derive(Debug, Clone)]
pub struct ImportFailed {
    pub source: String,
    pub error: String,
}

/// asset_import 返回。
#[derive(Debug, Clone)]
pub struct ImportOutcome {
    pub imported: Vec<ImportOne>,
    pub failed: Vec<ImportFailed>,
}

/// 导入一批源文件到 `dest_folder`(相对 Content/;源可以是绝对路径或相对路径)。
pub fn import_assets(
    project: &ForgeProject,
    sources: &[String],
    dest_folder: &str,
    import_settings: Option<&serde_json::Map<String, Value>>,
) -> Result<ImportOutcome> {
    project.ensure_dirs()?;
    let content_root = project.content_root();
    let dest_rel = normalize_rel(dest_folder)?;
    let dest_abs = project
        .resolve_content_path(&dest_rel)
        .unwrap_or_else(|_| content_root.join(&dest_rel));
    std::fs::create_dir_all(&dest_abs)?;

    let mut imported = Vec::new();
    let mut failed = Vec::new();

    for src_str in sources {
        let src = Path::new(src_str);
        let file_name = src
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("unnamed");
        let dest_abs_file = dest_abs.join(file_name);
        let dest_rel_file = format!("{}/{}", dest_rel, file_name);

        match import_one(project, src, &dest_abs_file, &dest_rel_file, import_settings) {
            Ok(one) => imported.push(one),
            Err(e) => failed.push(ImportFailed {
                source: src_str.clone(),
                error: e.to_string(),
            }),
        }
    }

    Ok(ImportOutcome { imported, failed })
}

/// 导入单个文件:复制 → .meta → 构建(网格)。
fn import_one(
    project: &ForgeProject,
    source: &Path,
    dest_abs: &Path,
    dest_rel: &str,
    import_settings: Option<&serde_json::Map<String, Value>>,
) -> Result<ImportOne> {
    // 1. 复制源文件进 Content/(源已在 Content 内则跳过)。
    let source_abs = if source.is_absolute() {
        source.to_path_buf()
    } else {
        project.root.join(source)
    };
    let same_file = match (source_abs.canonicalize(), dest_abs.canonicalize()) {
        (Ok(a), Ok(b)) => a == b,
        // 任一不可解析 = 不同文件(双失败时判同会跳拷贝,wave.4 冒烟实测踩中)。
        _ => false,
    };
    if !same_file {
        std::fs::copy(&source_abs, dest_abs).map_err(|e| {
            AssetError::new("IO", format!("复制失败 {} → {}: {e}", source_abs.display(), dest_abs.display()))
        })?;
    }

    // 2. .meta 已存在 → 复用 GUID(reimport 语义);否则新建。
    let meta_path = meta_path_for(&project.content_root(), dest_rel);
    let mut meta = if meta_path.is_file() {
        MetaDoc::load(&meta_path)?
    } else {
        MetaDoc::new(dest_rel, new_guid())?
    };

    // 3. 合并 importSettings(调用方覆盖默认)。
    if let Some(over) = import_settings {
        for (k, v) in over {
            meta.import_settings.insert(k.clone(), v.clone());
        }
    }
    // provenance:人工导入标记。
    if meta.provenance.is_none() {
        meta.provenance = Some(Provenance {
            origin: "user-import".into(),
            detail: None,
        });
    }
    meta.save(&meta_path)?;

    // 4. 构建/解码(网格 → .rxmesh;贴图 → 解码尺寸;材质/场景等登记)。
    let ext = Path::new(dest_rel)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("");
    let (atype, _) = AssetType::from_extension(ext)
        .ok_or_else(|| AssetError::new("UNKNOWN_TYPE", format!("不支持的扩展名: {ext}")))?;

    let mut cache_hit = false;
    let mut artifact = None;
    let mut vertex_count = None;
    let mut triangle_count = None;
    let mut width = None;
    let mut height = None;

    if atype == AssetType::Mesh {
        // 预判缓存命中:构建前查 cache 文件存在。
        let source_bytes = std::fs::read(dest_abs)?;
        let key = meta.cache_key(&source_bytes);
        let cache_file = project.cache_root().join("rxmesh").join(format!("{key}.rxmesh"));
        cache_hit = cache_file.is_file();

        let art = crate::build::build_mesh(dest_abs, &meta, &project.cache_root())?;
        artifact = Some(art.rel_path);
        vertex_count = Some(art.vertex_count);
        triangle_count = Some(art.triangle_count);
        meta.build_state = Some("current".into());
        meta.save(&meta_path)?;
    } else if atype == AssetType::Texture {
        // wave.4:png/jpg 导入解码尺寸(08 §4.2;解码失败 → 导入失败,不静默登记)。
        let (w, h) = crate::texture::decode_size(dest_abs)?;
        width = Some(w);
        height = Some(h);
        meta.build_state = Some("current".into());
        meta.save(&meta_path)?;
    } else {
        meta.build_state = Some("current".into());
        meta.save(&meta_path)?;
    }

    Ok(ImportOne {
        asset_path: dest_rel.to_string(),
        guid: meta.guid.clone(),
        atype,
        cache_hit,
        artifact,
        vertex_count,
        triangle_count,
        width,
        height,
    })
}
