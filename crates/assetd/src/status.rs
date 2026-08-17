//! asset_build_status:按当前缓存键与 .meta buildState 对比,报告 current|stale|building|failed。

use crate::meta::MetaDoc;
use crate::project::ForgeProject;
use crate::{meta_path_for, normalize_rel, AssetError, BuildState, Result};

/// 单个资产构建状态。
#[derive(Debug, Clone)]
pub struct BuildStatus {
    pub path: String,
    pub state: BuildState,
    pub hash: String,
}

/// 查一批资产(空 = 全项目)的构建状态。
pub fn build_status(project: &ForgeProject, paths: &[String]) -> Result<Vec<BuildStatus>> {
    let targets: Vec<String> = if paths.is_empty() {
        project.scan_content()?
    } else {
        paths.iter().map(|p| normalize_rel(p)).collect::<Result<Vec<_>>>()?
    };

    let mut out = Vec::new();
    for rel in targets {
        let meta_path = meta_path_for(&project.content_root(), &rel);
        if !meta_path.is_file() {
            return Err(AssetError::new("NO_META", format!("缺 .meta: {rel}")));
        }
        let meta = MetaDoc::load(&meta_path)?;
        let source_abs = project.content_root().join(&rel);
        let source_bytes = std::fs::read(&source_abs)
            .map_err(|e| AssetError::new("IO", format!("读源文件失败 {rel}: {e}")))?;
        let current_key = meta.cache_key(&source_bytes);

        // .meta 记的 buildState 是上次构建后状态;再用当前键比对缓存是否真存在。
        let cache_file = project.cache_root().join("rxmesh").join(format!("{current_key}.rxmesh"));
        let state = if let Some(bs) = &meta.build_state {
            let parsed = BuildState::parse(bs).unwrap_or(BuildState::Stale);
            match parsed {
                BuildState::Current => {
                    // current 需产物真在缓存里(网格);贴图无产物,直接 current。
                    if rel.ends_with(".gltf") || rel.ends_with(".glb") {
                        if cache_file.is_file() { BuildState::Current } else { BuildState::Stale }
                    } else {
                        BuildState::Current
                    }
                }
                other => other,
            }
        } else {
            BuildState::Stale // 从未构建
        };

        out.push(BuildStatus { path: rel, state, hash: current_key });
    }
    Ok(out)
}
