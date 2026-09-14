//! mesh_inspect(05 §3):读缓存 `.rxmesh`(rurix-geom-build read_dag 真实字节)→
//! 顶点/三角形/簇/LOD 统计 + bounds(顶点 min/max);materials 取 .meta importSettings.materialSlots。

use crate::meta::MetaDoc;
use crate::project::ForgeProject;
use crate::{meta_path_for, normalize_rel, AssetError, Result};

/// mesh_inspect 返回。
#[derive(Debug, Clone)]
pub struct MeshInspection {
    pub vertices: u32,
    pub triangles: u32,
    pub meshlets: u32,
    pub lods: u32,
    pub materials: Vec<String>,
    /// (min, max);空网格 = None。
    pub bounds: Option<([f32; 3], [f32; 3])>,
    /// 缓存产物相对 .forge/cache/ 路径。
    pub artifact: String,
}

/// 检查网格资产:缓存产物缺失 → MESH_NOT_BUILT(先 asset_reimport),不静默重建。
pub fn inspect_mesh(project: &ForgeProject, rel_path: &str) -> Result<MeshInspection> {
    let rel = normalize_rel(rel_path)?;
    let meta_path = meta_path_for(&project.content_root(), &rel);
    if !meta_path.is_file() {
        return Err(AssetError::new("NO_META", format!("缺 .meta: {rel}")));
    }
    let meta = MetaDoc::load(&meta_path)?;
    if meta.atype != "mesh" {
        return Err(AssetError::new(
            "WRONG_TYPE",
            format!("mesh_inspect 仅接受 mesh,当前: {}", meta.atype),
        ));
    }
    let source_bytes = std::fs::read(project.content_root().join(&rel))?;
    let key = meta.cache_key(&source_bytes);
    let cache_file = project.cache_root().join("rxmesh").join(format!("{key}.rxmesh"));
    if !cache_file.is_file() {
        return Err(AssetError::new(
            "MESH_NOT_BUILT",
            format!("缓存产物缺失(源或 importSettings 已变),先 asset_reimport: {rel}"),
        ));
    }
    let bytes = std::fs::read(&cache_file)?;
    let dag = rurix_geom_build::read_dag(&bytes)
        .map_err(|e| AssetError::new("BUILD_ERR", format!(".rxmesh 反序列化失败: {e}")))?;

    let bounds = dag.vertices.first().map(|first| {
        let mut min = *first;
        let mut max = *first;
        for v in &dag.vertices[1..] {
            for i in 0..3 {
                min[i] = min[i].min(v[i]);
                max[i] = max[i].max(v[i]);
            }
        }
        (min, max)
    });

    let materials = meta
        .import_settings
        .get("materialSlots")
        .and_then(|v| v.as_array())
        .map(|a| {
            a.iter()
                .filter_map(|s| s.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default();

    Ok(MeshInspection {
        vertices: dag.vertices.len() as u32,
        triangles: (dag.triangle_indices.len() / 3) as u32,
        meshlets: dag.records.len() as u32,
        lods: dag.levels.len() as u32,
        materials,
        bounds,
        artifact: format!("rxmesh/{key}.rxmesh"),
    })
}
