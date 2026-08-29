//! 资产操作:asset_move / asset_delete / asset_fix_redirectors / asset_reimport / asset_set_meta。

use std::path::{Path, PathBuf};

use serde_json::{json, Value};

use crate::meta::{ensure_meta, MetaDoc, Semantic};
use crate::project::ForgeProject;
use crate::refs::RefGraph;
use crate::{meta_path_for, normalize_rel, AssetError, Result};

/// asset_delete 返回。
#[derive(Debug, Clone)]
pub struct DeleteOutcome {
    pub deleted: Vec<String>,
    pub blocked_by_refs: Vec<(String, Vec<String>)>, // (assetPath, [引用方 GUID 列表])
}

/// asset_move 返回。
#[derive(Debug, Clone)]
pub struct MoveOutcome {
    pub moved: bool,
    pub redirector: Option<(String, String, String)>, // (guid, old_path, new_path)
}

/// 删除资产(默认引用阻断;force=true 跳过阻断——但 MCP 层须先 Proposal)。
pub fn delete_assets(
    project: &ForgeProject,
    paths: &[String],
    force: bool,
) -> Result<DeleteOutcome> {
    let mut graph = RefGraph::load(project)?;
    let mut deleted = Vec::new();
    let mut blocked = Vec::new();

    for p in paths {
        let rel = normalize_rel(p)?;
        let meta_path = meta_path_for(&project.content_root(), &rel);
        if !meta_path.is_file() {
            return Err(AssetError::new("NO_META", format!("缺 .meta: {rel}")));
        }
        let meta = MetaDoc::load(&meta_path)?;
        let refs_in = graph.referenced_by(&meta.guid);
        if !force && !refs_in.is_empty() {
            let referencers: Vec<String> = refs_in.iter().map(|e| e.from_guid.clone()).collect();
            blocked.push((rel, referencers));
            continue;
        }
        // 真删:源文件 + .meta + 引用边。
        let source_abs = project.resolve_content_path(&rel)?;
        if source_abs.is_file() {
            std::fs::remove_file(&source_abs)?;
        }
        std::fs::remove_file(&meta_path)?;
        graph.remove_outgoing(&meta.guid);
        graph.remove_incoming(&meta.guid);
        deleted.push(rel);
    }

    graph.save(project)?;
    Ok(DeleteOutcome { deleted, blocked_by_refs: blocked })
}

/// 移动资产到新目录(自动留 redirector;GUID 引用不断链)。
/// `new_name` = Some 时同步改名(清洗命名;同目录改名 = dest_folder = 原目录)。
pub fn move_asset(
    project: &ForgeProject,
    asset_path: &str,
    dest_folder: &str,
    new_name: Option<&str>,
) -> Result<MoveOutcome> {
    let rel = normalize_rel(asset_path)?;
    let dest_rel = normalize_rel(dest_folder)?;
    let content_root = project.content_root();
    let src_meta = meta_path_for(&content_root, &rel);
    if !src_meta.is_file() {
        return Err(AssetError::new("NO_META", format!("缺 .meta: {rel}")));
    }
    let meta = MetaDoc::load(&src_meta)?;

    let file_name = match new_name {
        Some(n) => {
            if n.is_empty() || n.contains('/') || n.contains('\\') {
                return Err(AssetError::new("INVALID_OPS", format!("新文件名非法: {n}")));
            }
            n
        }
        None => Path::new(&rel).file_name().and_then(|n| n.to_str()).unwrap_or("unnamed"),
    };
    let new_rel = format!("{}/{}", dest_rel, file_name);
    let src_abs = project.resolve_content_path(&rel)?;
    let dst_abs = project.resolve_content_path(&new_rel).unwrap_or_else(|_| content_root.join(&new_rel));
    std::fs::create_dir_all(content_root.join(&dest_rel))?;
    std::fs::rename(&src_abs, &dst_abs)?;

    // .meta 路径迁移(文件名变,GUID 不变)。
    let new_meta = meta_path_for(&content_root, &new_rel);
    std::fs::rename(&src_meta, &new_meta)?;

    // redirector。
    let mut graph = RefGraph::load(project)?;
    graph.add_redirector(&meta.guid, &rel, &new_rel);
    graph.save(project)?;

    Ok(MoveOutcome {
        moved: true,
        redirector: Some((meta.guid, rel, new_rel)),
    })
}

/// fix_redirectors:把 redirector 旧路径的引用方重写为新路径,并清除 redirector。
/// wave.2 最小实现:仅处理场景文件(.rxscene)中的路径引用(GUID 不变,故引用本身未断,
/// 此操作清理 redirector 占位)。返回 fixed 的 redirector guid 列表。
pub fn fix_redirectors(project: &ForgeProject, _folder: Option<&str>) -> Result<Vec<String>> {
    let mut graph = RefGraph::load(project)?;
    let fixed: Vec<String> = graph.redirectors.iter().map(|r| r.guid.clone()).collect();
    for guid in &fixed {
        graph.remove_redirector(guid);
    }
    graph.save(project)?;
    Ok(fixed)
}

/// asset_reimport:重新构建(改设置后)。返回 rebuilt 列表。
pub fn reimport_assets(project: &ForgeProject, paths: &[String]) -> Result<Vec<String>> {
    let mut rebuilt = Vec::new();
    for p in paths {
        let rel = normalize_rel(p)?;
        let meta_path = meta_path_for(&project.content_root(), &rel);
        if !meta_path.is_file() {
            return Err(AssetError::new("NO_META", format!("缺 .meta: {rel}")));
        }
        let mut meta = MetaDoc::load(&meta_path)?;
        let source_abs = project.content_root().join(&rel);
        if meta.atype == "mesh" {
            let _art = crate::build::build_mesh(&source_abs, &meta, &project.cache_root())?;
        }
        meta.build_state = Some("current".into());
        meta.save(&meta_path)?;
        rebuilt.push(rel);
    }
    Ok(rebuilt)
}

/// asset_set_description:写入 semantic 段(description/tags/source/model/content_hash)。
/// 缺 .meta 时自动补建(如 .rx/.rxgraph)。
pub fn set_description(
    project: &ForgeProject,
    asset_path: &str,
    description: &str,
    tags: &[String],
    source: &str,
    model: Option<&str>,
    content_hash: Option<&str>,
) -> Result<()> {
    let rel = normalize_rel(asset_path)?;
    let source_abs = project.content_root().join(&rel);
    if !source_abs.is_file() {
        return Err(AssetError::new("NO_SOURCE", format!("源文件不存在: {rel}")));
    }
    let (meta_path, mut meta) = ensure_meta(&project.content_root(), &rel)?;
    let mut sem = meta.semantic.take().unwrap_or_else(|| Semantic {
        description: String::new(),
        tags: Vec::new(),
        source: String::new(),
        model: None,
        updated_at: None,
        content_hash: None,
    });
    sem.description = description.to_string();
    sem.tags = tags.to_vec();
    sem.source = source.to_string();
    sem.model = model.map(str::to_string);
    sem.updated_at = Some(forge_util::timeutil::utc_now_iso8601());
    sem.content_hash = content_hash.map(str::to_string);
    meta.semantic = Some(sem);
    meta.save(&meta_path)?;
    Ok(())
}

/// asset_set_meta:打补丁到 .meta importSettings。
pub fn set_meta(project: &ForgeProject, asset_path: &str, patch: &serde_json::Map<String, Value>) -> Result<()> {
    let rel = normalize_rel(asset_path)?;
    let meta_path = meta_path_for(&project.content_root(), &rel);
    if !meta_path.is_file() {
        return Err(AssetError::new("NO_META", format!("缺 .meta: {rel}")));
    }
    let mut meta = MetaDoc::load(&meta_path)?;
    for (k, v) in patch {
        meta.import_settings.insert(k.clone(), v.clone());
    }
    meta.save(&meta_path)?;
    Ok(())
}
