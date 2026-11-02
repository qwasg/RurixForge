//! 3D Asset Inspection and Thumbnail Caching Pipeline.
//!
//! Provides `inspect_mesh` (for legacy .rxmesh binary DAGs) and `inspect_3d_asset`
//! (deterministic 3D inspection for .glb, .gltf, .rxmodel, .rxmesh, with SHA-256
//! content-addressable thumbnail caching under .forge/cache/thumbs/<source_hash>.png).

use std::path::{Path, PathBuf};

use base64::Engine as _;
use serde::{Deserialize, Serialize};

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

// ---------------------------------------------------------------------------
// Milestone 2: inspect_3d_asset Implementation
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Dimensions3D {
    pub x: f32,
    pub y: f32,
    pub z: f32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BoundingBox3D {
    pub min: [f32; 3],
    pub max: [f32; 3],
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Asset3dInspection {
    pub path: String,
    pub polycount: usize,
    pub dimensions: Dimensions3D,
    pub bounding_box: BoundingBox3D,
    pub materials: Vec<String>,
    pub three_view_sheet_uri: String,
    pub data_url: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CachedMetadata {
    pub polycount: usize,
    pub dimensions: Dimensions3D,
    pub bounding_box: BoundingBox3D,
    pub materials: Vec<String>,
}

/// Safely commit a temporary file to a target destination with Windows overwrite tolerance.
fn commit_atomic_file(tmp_path: &Path, target_path: &Path) -> std::io::Result<()> {
    if target_path.exists() {
        let _ = std::fs::remove_file(target_path);
    }
    if std::fs::rename(tmp_path, target_path).is_err() {
        std::fs::copy(tmp_path, target_path)?;
        let _ = std::fs::remove_file(tmp_path);
    }
    Ok(())
}

/// Resolve a 3D model path or GUID within the project.
pub fn resolve_model_path(project: &ForgeProject, input_path: &str) -> Result<PathBuf> {
    let trimmed = input_path.trim().replace('\\', "/");

    // 1. Check if input is a valid UUID GUID
    if uuid::Uuid::parse_str(&trimmed).is_ok() {
        if let Ok(p) = crate::model::model_asset_path(project, &trimmed) {
            if p.is_file() {
                return Ok(p);
            }
        }
        if let Ok(entries) = project.scan_content() {
            for rel in entries {
                let meta_path = meta_path_for(&project.content_root(), &rel);
                if let Ok(meta) = MetaDoc::load(&meta_path) {
                    if meta.guid == trimmed {
                        let full = project.content_root().join(&rel);
                        if full.is_file() {
                            return Ok(full);
                        }
                    }
                }
            }
        }
    }

    // 2. Resolve candidate path relative to Content/ or project root
    let candidate = if trimmed.starts_with("Content/") {
        let rel = trimmed.strip_prefix("Content/").unwrap();
        project.content_root().join(rel)
    } else if trimmed.starts_with(".forge/") {
        project.root.join(&trimmed)
    } else {
        let content_path = project.content_root().join(&trimmed);
        if content_path.exists() {
            content_path
        } else {
            project.root.join(&trimmed)
        }
    };

    if !candidate.exists() {
        return Err(AssetError::new(
            "ASSET_NOT_FOUND",
            format!("3D asset not found: {input_path}"),
        ));
    }

    // Canonicalize candidate & project root for containment security
    let canonical = candidate.canonicalize().map_err(|_| {
        AssetError::new("ASSET_NOT_FOUND", format!("3D asset not found: {input_path}"))
    })?;
    let canonical_root = project.root.canonicalize().map_err(|e| {
        AssetError::new("IO", format!("Cannot canonicalize project root: {e}"))
    })?;

    if !canonical.starts_with(&canonical_root) {
        return Err(AssetError::new(
            "ACCESS_DENIED",
            format!("Asset path escapes project root: {input_path}"),
        ));
    }

    if canonical.is_dir() {
        let model_inside = canonical.join("model.rxmodel");
        if model_inside.is_file() {
            return Ok(model_inside);
        }
    }

    if !canonical.is_file() {
        return Err(AssetError::new(
            "ASSET_NOT_FOUND",
            format!("3D asset path is not a file: {input_path}"),
        ));
    }

    Ok(canonical)
}

/// Inspect a 3D model asset (.glb, .gltf, .rxmodel, .rxmesh).
/// Extracts polycount, dimensions, bounding box, materials, and renders/caches a 4-panel composite showcase sheet.
pub fn inspect_3d_asset(
    project: &ForgeProject,
    path: &str,
    generate_three_view: bool,
    panel_size: u32,
) -> Result<Asset3dInspection> {
    let canonical_path = resolve_model_path(project, path)?;
    let source_bytes = std::fs::read(&canonical_path)?;
    let source_hash = forge_util::hashutil::sha256_hex(&source_bytes);

    let thumbs_dir = project.cache_root().join("thumbs");
    std::fs::create_dir_all(&thumbs_dir)?;

    let cache_png = thumbs_dir.join(format!("{source_hash}.png"));
    let cache_json = thumbs_dir.join(format!("{source_hash}.json"));

    // Check Cache Hit
    let cache_hit = cache_json.is_file() && (!generate_three_view || cache_png.is_file());
    if cache_hit {
        if let Ok(meta_bytes) = std::fs::read(&cache_json) {
            if let Ok(cached) = serde_json::from_slice::<CachedMetadata>(&meta_bytes) {
                let (three_view_sheet_uri, data_url) = if generate_three_view {
                    if let Ok(png_data) = std::fs::read(&cache_png) {
                        let b64 = base64::engine::general_purpose::STANDARD.encode(&png_data);
                        (
                            format!(".forge/cache/thumbs/{source_hash}.png"),
                            format!("data:image/png;base64,{b64}"),
                        )
                    } else {
                        (String::new(), String::new())
                    }
                } else {
                    (String::new(), String::new())
                };

                return Ok(Asset3dInspection {
                    path: path.to_string(),
                    polycount: cached.polycount,
                    dimensions: cached.dimensions,
                    bounding_box: cached.bounding_box,
                    materials: cached.materials,
                    three_view_sheet_uri,
                    data_url,
                });
            }
        }
    }

    // Cache Miss: Extract Geometry
    let mesh = crate::rasterizer::extract_geometry_from_file(project, &canonical_path)?;
    let dims = mesh.bounding_box.dimensions();
    let dimensions = Dimensions3D {
        x: dims[0],
        y: dims[1],
        z: dims[2],
    };
    let bounding_box = BoundingBox3D {
        min: mesh.bounding_box.min,
        max: mesh.bounding_box.max,
    };
    let polycount = mesh.indices.len() / 3;
    let materials = mesh.materials.clone();

    // Sidecar metadata for caching
    let meta_record = CachedMetadata {
        polycount,
        dimensions: dimensions.clone(),
        bounding_box: bounding_box.clone(),
        materials: materials.clone(),
    };

    let nonce = uuid::Uuid::new_v4().to_string();
    std::fs::create_dir_all(project.tmp_root())?;
    let tmp_json = project.tmp_root().join(format!("inspect_{source_hash}_{nonce}.json"));
    let json_bytes = serde_json::to_vec_pretty(&meta_record)
        .map_err(|e| AssetError::new("JSON_SER", e.to_string()))?;
    std::fs::write(&tmp_json, &json_bytes)?;
    let _ = commit_atomic_file(&tmp_json, &cache_json);

    let (three_view_sheet_uri, data_url) = if generate_three_view {
        let panel_size_clamped = panel_size.clamp(64, 512);
        let rasterizer = crate::rasterizer::PureCpuRasterizer::new(panel_size_clamped);
        let sheet_img = rasterizer.render(&mesh);

        let mut png_buf = std::io::Cursor::new(Vec::new());
        sheet_img
            .write_to(&mut png_buf, image::ImageFormat::Png)
            .map_err(|e| AssetError::new("PNG_ENCODE", e.to_string()))?;
        let png_bytes = png_buf.into_inner();

        let tmp_png = project.tmp_root().join(format!("thumb_{source_hash}_{nonce}.png"));
        std::fs::write(&tmp_png, &png_bytes)?;
        let _ = commit_atomic_file(&tmp_png, &cache_png);

        let b64 = base64::engine::general_purpose::STANDARD.encode(&png_bytes);
        (
            format!(".forge/cache/thumbs/{source_hash}.png"),
            format!("data:image/png;base64,{b64}"),
        )
    } else {
        (String::new(), String::new())
    };

    Ok(Asset3dInspection {
        path: path.to_string(),
        polycount,
        dimensions,
        bounding_box,
        materials,
        three_view_sheet_uri,
        data_url,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp_project(tag: &str) -> (PathBuf, ForgeProject) {
        let dir = std::env::temp_dir().join(format!("inspect3d_test_{}_{}", tag, uuid::Uuid::new_v4()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let p = ForgeProject::with_defaults(dir.clone());
        p.ensure_dirs().unwrap();
        (dir, p)
    }

    #[test]
    fn test_inspect_rejects_nonexistent_model() {
        let (_dir, project) = tmp_project("nonexistent");
        let err = inspect_3d_asset(&project, "Content/Models/ghost.glb", true, 64).unwrap_err();
        assert_eq!(err.code, "ASSET_NOT_FOUND");
    }

    #[test]
    fn test_inspect_quad_indexed_glb_fixture() {
        let (_dir, project) = tmp_project("quad_glb");
        let fixture_path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/quad_indexed.glb");
        if fixture_path.is_file() {
            let rel = "Meshes/quad_indexed.glb";
            let target = project.content_root().join(rel);
            std::fs::create_dir_all(target.parent().unwrap()).unwrap();
            std::fs::copy(&fixture_path, &target).unwrap();

            let inspection = inspect_3d_asset(&project, rel, true, 64).unwrap();
            assert_eq!(inspection.polycount, 2);
            assert!(inspection.data_url.starts_with("data:image/png;base64,"));
            assert!(!inspection.three_view_sheet_uri.is_empty());

            // Second call: cache hit
            let cached_inspection = inspect_3d_asset(&project, rel, true, 64).unwrap();
            assert_eq!(cached_inspection.polycount, 2);
            assert_eq!(cached_inspection.data_url, inspection.data_url);
        }
    }
}
