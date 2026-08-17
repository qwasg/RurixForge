//! 网格构建:rurix-asset::gltf → ImportedMesh → TriMesh → rurix-geom-build DAG → RXGB `.rxmesh`。

use std::path::{Path, PathBuf};

use crate::meta::MetaDoc;
use crate::{AssetError, Result};

/// 构建产物信息。
#[derive(Debug, Clone)]
pub struct BuildArtifact {
    /// .rxmesh 相对 .forge/cache/ 的路径。
    pub rel_path: String,
    pub vertex_count: u32,
    pub triangle_count: u32,
    pub meshlet_count: u32,
    pub lod_level_count: u32,
}

/// 构建网格资产:gltf 导入 → 合并所有 ImportedMesh 为单一 TriMesh → build_dag → write_dag。
///
/// 缓存:按 cache_key 命名 `<key>.rxmesh`,命中即跳过(08 §4.3 确定性)。
pub fn build_mesh(
    source_path: &Path,
    meta: &MetaDoc,
    cache_root: &Path,
) -> Result<BuildArtifact> {
    let source_bytes = std::fs::read(source_path)?;
    let key = meta.cache_key(&source_bytes);
    let cache_file = cache_root.join("rxmesh").join(format!("{key}.rxmesh"));
    if cache_file.is_file() {
        // 缓存命中:反序列化 .rxmesh 统计(从 RXGB 字节读回,确保真实)。
        let bytes = std::fs::read(&cache_file)?;
        let dag = rurix_geom_build::read_dag(&bytes)
            .map_err(|e| AssetError::new("BUILD_ERR", format!("缓存 .rxmesh 反序列化失败: {e}")))?;
        return Ok(BuildArtifact {
            rel_path: format!("rxmesh/{key}.rxmesh"),
            vertex_count: dag.vertices.len() as u32,
            triangle_count: (dag.triangle_indices.len() / 3) as u32,
            meshlet_count: dag.records.len() as u32,
            lod_level_count: dag.levels.len() as u32,
        });
    }

    // 缓存未命中:真实构建。
    let import = rurix_asset::gltf::import_path(source_path, &rurix_asset::gltf::validate::ImportOptions::default())
        .map_err(|e| AssetError::new("IMPORT_ERR", format!("gltf 导入失败: {e}")))?;

    let mesh = merge_imported_meshes(&import.meshes);
    let dag = rurix_geom_build::build_dag(&mesh);
    let rxgb = rurix_geom_build::write_dag(&dag);

    std::fs::create_dir_all(cache_root.join("rxmesh"))?;
    std::fs::write(&cache_file, &rxgb)?;

    Ok(BuildArtifact {
        rel_path: format!("rxmesh/{key}.rxmesh"),
        vertex_count: mesh.positions.len() as u32,
        triangle_count: (mesh.indices.len() / 3) as u32,
        meshlet_count: dag.records.len() as u32,
        lod_level_count: dag.levels.len() as u32,
    })
}

/// 合并多个 ImportedMesh 为单一 TriMesh(顶点索引偏移累加)。
fn merge_imported_meshes(meshes: &[rurix_asset::gltf::validate::ImportedMesh]) -> rurix_geom_build::TriMesh {
    let total_verts: usize = meshes.iter().map(|m| m.positions.len()).sum();
    let total_indices: usize = meshes.iter().map(|m| m.indices.len()).sum();
    let mut positions = Vec::with_capacity(total_verts);
    let mut indices = Vec::with_capacity(total_indices);
    let mut vert_offset = 0u32;
    for m in meshes {
        positions.extend_from_slice(&m.positions);
        indices.extend(m.indices.iter().map(|i| i + vert_offset));
        vert_offset += m.positions.len() as u32;
    }
    rurix_geom_build::TriMesh { positions, indices }
}
