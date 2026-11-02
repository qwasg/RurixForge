//! Unified 3D geometry extraction for .glb, .rxmodel, and .rxmesh.
//! Converts arbitrary supported 3D assets into a canonical `MeshGeometry` representation
//! with synthesized surface normals and exact axis-aligned bounding boxes.

use std::collections::BTreeSet;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::model::ModelBundle;
use crate::project::ForgeProject;
use crate::{AssetError, Result};

/// Axis-Aligned Bounding Box (AABB) in 3D model coordinate space.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AABB {
    pub min: [f32; 3],
    pub max: [f32; 3],
}

impl AABB {
    pub fn new(min: [f32; 3], max: [f32; 3]) -> Self {
        Self { min, max }
    }

    /// Compute bounding box enclosing all provided points.
    pub fn from_points(points: &[[f32; 3]]) -> Self {
        if points.is_empty() {
            return Self {
                min: [0.0, 0.0, 0.0],
                max: [0.0, 0.0, 0.0],
            };
        }
        let mut min = points[0];
        let mut max = points[0];
        for p in &points[1..] {
            for i in 0..3 {
                min[i] = min[i].min(p[i]);
                max[i] = max[i].max(p[i]);
            }
        }
        Self { min, max }
    }

    /// Extents along each axis [width (X), height (Y), depth (Z)].
    pub fn dimensions(&self) -> [f32; 3] {
        [
            (self.max[0] - self.min[0]).max(0.0),
            (self.max[1] - self.min[1]).max(0.0),
            (self.max[2] - self.min[2]).max(0.0),
        ]
    }

    /// Geometric center of the bounding box.
    pub fn center(&self) -> [f32; 3] {
        [
            0.5 * (self.min[0] + self.max[0]),
            0.5 * (self.min[1] + self.max[1]),
            0.5 * (self.min[2] + self.max[2]),
        ]
    }

    /// Bounding sphere radius encompassing the AABB from its center.
    /// Clamped to 1.0 for degenerate zero-dimension meshes to prevent division by zero.
    pub fn radius(&self) -> f32 {
        let d = self.dimensions();
        let r = 0.5 * (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt();
        if r < 1e-6 {
            1.0
        } else {
            r
        }
    }

    /// Expands bounding box to include a given point.
    pub fn expand(&mut self, point: [f32; 3]) {
        for i in 0..3 {
            self.min[i] = self.min[i].min(point[i]);
            self.max[i] = self.max[i].max(point[i]);
        }
    }
}

/// Alias for BoundingBox3D matching blueprint documentation.
pub type BoundingBox3D = AABB;

/// Unified 3D mesh representation feeding the CPU software rasterizer.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MeshGeometry {
    /// Vertex positions in model space.
    pub positions: Vec<[f32; 3]>,
    /// Triangle indices (length is a multiple of 3; indices point into `positions`).
    pub indices: Vec<u32>,
    /// Surface normal vector per vertex.
    pub normals: Vec<[f32; 3]>,
    /// Names or identifiers of materials referenced by the mesh.
    pub materials: Vec<String>,
    /// Axis-aligned bounding box of the entire mesh.
    pub bounding_box: AABB,
    /// Edge vertex index pairs (for wireframe rendering).
    #[serde(default)]
    pub edges: Vec<(usize, usize)>,
}

impl MeshGeometry {
    /// Create an empty mesh with safe defaults.
    pub fn empty() -> Self {
        Self {
            positions: Vec::new(),
            indices: Vec::new(),
            normals: Vec::new(),
            materials: Vec::new(),
            bounding_box: AABB::new([0.0; 3], [0.0; 3]),
            edges: Vec::new(),
        }
    }

    /// Synthetic reference cube matching `tools/e2e/test_world_build_e2e.py` reference implementation.
    pub fn cube(dimensions: [f32; 3], _color: [u8; 3]) -> Self {
        let dx = dimensions[0] / 2.0;
        let dy = dimensions[1] / 2.0;
        let dz = dimensions[2] / 2.0;

        let positions = vec![
            [-dx, -dy, -dz],
            [dx, -dy, -dz],
            [dx, dy, -dz],
            [-dx, dy, -dz],
            [-dx, -dy, dz],
            [dx, -dy, dz],
            [dx, dy, dz],
            [-dx, dy, dz],
        ];

        let edges = vec![
            (0, 1), (1, 2), (2, 3), (3, 0),
            (4, 5), (5, 6), (6, 7), (7, 4),
            (0, 4), (1, 5), (2, 6), (3, 7),
        ];

        let indices = vec![
            // Front (-Z)
            0, 2, 1, 0, 3, 2,
            // Back (+Z)
            4, 5, 6, 4, 6, 7,
            // Left (-X)
            0, 7, 3, 0, 4, 7,
            // Right (+X)
            1, 2, 6, 1, 6, 5,
            // Top (+Y)
            3, 7, 6, 3, 6, 2,
            // Bottom (-Y)
            0, 1, 5, 0, 5, 4,
        ];

        let normals = compute_normals(&positions, &indices);
        let bounding_box = AABB::new([-dx, -dy, -dz], [dx, dy, dz]);

        Self {
            positions,
            indices,
            normals,
            materials: vec!["mat_concrete".into(), "mat_glass".into()],
            bounding_box,
            edges,
        }
    }
}

/// Area-weighted triangle face normal synthesis accumulated at vertices and normalized.
pub fn compute_normals(positions: &[[f32; 3]], indices: &[u32]) -> Vec<[f32; 3]> {
    let mut out = vec![[0.0f32; 3]; positions.len()];
    for tri in indices.chunks_exact(3) {
        let i0 = tri[0] as usize;
        let i1 = tri[1] as usize;
        let i2 = tri[2] as usize;
        if i0 >= positions.len() || i1 >= positions.len() || i2 >= positions.len() {
            continue;
        }
        let p0 = positions[i0];
        let p1 = positions[i1];
        let p2 = positions[i2];
        let e1 = [p1[0] - p0[0], p1[1] - p0[1], p1[2] - p0[2]];
        let e2 = [p2[0] - p0[0], p2[1] - p0[1], p2[2] - p0[2]];
        // Cross product e1 x e2
        let n = [
            e1[1] * e2[2] - e1[2] * e2[1],
            e1[2] * e2[0] - e1[0] * e2[2],
            e1[0] * e2[1] - e1[1] * e2[0],
        ];
        for &idx in tri {
            out[idx as usize][0] += n[0];
            out[idx as usize][1] += n[1];
            out[idx as usize][2] += n[2];
        }
    }
    for norm in &mut out {
        let len = (norm[0] * norm[0] + norm[1] * norm[1] + norm[2] * norm[2]).sqrt();
        if len > 1e-12 {
            norm[0] /= len;
            norm[1] /= len;
            norm[2] /= len;
        } else {
            *norm = [0.0, 1.0, 0.0];
        }
    }
    out
}

// ---------------------------------------------------------------------------
// 4x4 Matrix Mathematics (Row-Major)
// ---------------------------------------------------------------------------

pub type Mat4 = [[f32; 4]; 4];

pub fn mat4_identity() -> Mat4 {
    [
        [1.0, 0.0, 0.0, 0.0],
        [0.0, 1.0, 0.0, 0.0],
        [0.0, 0.0, 1.0, 0.0],
        [0.0, 0.0, 0.0, 1.0],
    ]
}

pub fn mat4_mul(a: &Mat4, b: &Mat4) -> Mat4 {
    let mut out = [[0.0; 4]; 4];
    for i in 0..4 {
        for j in 0..4 {
            out[i][j] = a[i][0] * b[0][j]
                + a[i][1] * b[1][j]
                + a[i][2] * b[2][j]
                + a[i][3] * b[3][j];
        }
    }
    out
}

pub fn mat4_from_col_major(m: &[f32; 16]) -> Mat4 {
    [
        [m[0], m[4], m[8], m[12]],
        [m[1], m[5], m[9], m[13]],
        [m[2], m[6], m[10], m[14]],
        [m[3], m[7], m[11], m[15]],
    ]
}

pub fn mat4_from_trs(translation: [f32; 3], rotation: [f32; 4], scale: [f32; 3]) -> Mat4 {
    let [qx, qy, qz, qw] = rotation;
    let x2 = qx + qx;
    let y2 = qy + qy;
    let z2 = qz + qz;
    let xx = qx * x2;
    let xy = qx * y2;
    let xz = qx * z2;
    let yy = qy * y2;
    let yz = qy * z2;
    let zz = qz * z2;
    let wx = qw * x2;
    let wy = qw * y2;
    let wz = qw * z2;

    let r00 = 1.0 - (yy + zz);
    let r01 = xy - wz;
    let r02 = xz + wy;

    let r10 = xy + wz;
    let r11 = 1.0 - (xx + zz);
    let r12 = yz - wx;

    let r20 = xz - wy;
    let r21 = yz + wx;
    let r22 = 1.0 - (xx + yy);

    [
        [r00 * scale[0], r01 * scale[1], r02 * scale[2], translation[0]],
        [r10 * scale[0], r11 * scale[1], r12 * scale[2], translation[1]],
        [r20 * scale[0], r21 * scale[1], r22 * scale[2], translation[2]],
        [0.0, 0.0, 0.0, 1.0],
    ]
}

pub fn mat4_transform_point(m: &Mat4, p: [f32; 3]) -> [f32; 3] {
    [
        m[0][0] * p[0] + m[0][1] * p[1] + m[0][2] * p[2] + m[0][3],
        m[1][0] * p[0] + m[1][1] * p[1] + m[1][2] * p[2] + m[1][3],
        m[2][0] * p[0] + m[2][1] * p[1] + m[2][2] * p[2] + m[2][3],
    ]
}

pub fn mat4_transform_vector(m: &Mat4, v: [f32; 3]) -> [f32; 3] {
    let x = m[0][0] * v[0] + m[0][1] * v[1] + m[0][2] * v[2];
    let y = m[1][0] * v[0] + m[1][1] * v[1] + m[1][2] * v[2];
    let z = m[2][0] * v[0] + m[2][1] * v[1] + m[2][2] * v[2];
    let len = (x * x + y * y + z * z).sqrt();
    if len > 1e-12 {
        [x / len, y / len, z / len]
    } else {
        [0.0, 1.0, 0.0]
    }
}

// ---------------------------------------------------------------------------
// Format Extractors
// ---------------------------------------------------------------------------

/// Extract geometry from raw glTF or GLB binary bytes.
pub fn extract_from_glb_bytes(bytes: &[u8]) -> Result<MeshGeometry> {
    let gltf = gltf::Gltf::from_slice_without_validation(bytes)
        .map_err(|e| AssetError::new("GLTF_PARSE_ERR", format!("Invalid glTF/GLB: {e}")))?;

    let mut buffers = Vec::new();
    for buffer in gltf.buffers() {
        let data = match buffer.source() {
            gltf::buffer::Source::Bin => gltf
                .blob
                .as_ref()
                .map(|b| b.as_slice())
                .ok_or_else(|| AssetError::new("GLTF_PARSE_ERR", "Missing GLB binary chunk"))?,
            gltf::buffer::Source::Uri(uri) => {
                if uri.starts_with("data:") {
                    let (_, data) = uri
                        .split_once(',')
                        .ok_or_else(|| AssetError::new("GLTF_PARSE_ERR", "Malformed data URI"))?;
                    let decoded = base64::Engine::decode(
                        &base64::engine::general_purpose::STANDARD,
                        data,
                    )
                    .map_err(|e| AssetError::new("GLTF_PARSE_ERR", format!("Base64 error: {e}")))?;
                    // Leak or store decoded data
                    // For embedded buffers in gltf, we can safely decode into vector
                    let boxed = decoded.into_boxed_slice();
                    Box::leak(boxed)
                } else {
                    return Err(AssetError::new(
                        "GLTF_PARSE_ERR",
                        "External URI buffer not accessible from memory slice",
                    ));
                }
            }
        };
        buffers.push(data);
    }

    let mut all_positions = Vec::new();
    let mut all_indices = Vec::new();
    let mut all_normals = Vec::new();
    let mut material_set = BTreeSet::new();

    // Traverse node hierarchy
    fn visit_node<'a>(
        node: &gltf::Node<'a>,
        parent_mat: &Mat4,
        buffers: &[&'a [u8]],
        all_positions: &mut Vec<[f32; 3]>,
        all_indices: &mut Vec<u32>,
        all_normals: &mut Vec<[f32; 3]>,
        material_set: &mut BTreeSet<String>,
    ) {
        let node_col_mat = node.transform().matrix();
        let local_mat = [
            [node_col_mat[0][0], node_col_mat[1][0], node_col_mat[2][0], node_col_mat[3][0]],
            [node_col_mat[0][1], node_col_mat[1][1], node_col_mat[2][1], node_col_mat[3][1]],
            [node_col_mat[0][2], node_col_mat[1][2], node_col_mat[2][2], node_col_mat[3][2]],
            [node_col_mat[0][3], node_col_mat[1][3], node_col_mat[2][3], node_col_mat[3][3]],
        ];
        let world_mat = mat4_mul(parent_mat, &local_mat);

        if let Some(mesh) = node.mesh() {
            for prim in mesh.primitives() {
                if prim.mode() != gltf::mesh::Mode::Triangles {
                    continue;
                }
                let get_buffer = |b: gltf::Buffer<'_>| buffers.get(b.index()).copied();
                let reader = prim.reader(get_buffer);
                let Some(positions) = reader.read_positions() else {
                    continue;
                };

                let base_index = all_positions.len() as u32;
                let mut prim_positions = Vec::new();
                for pos in positions {
                    let transformed = mat4_transform_point(&world_mat, pos);
                    prim_positions.push(transformed);
                }

                let prim_indices: Vec<u32> = reader
                    .read_indices()
                    .map(|idx| idx.into_u32().collect())
                    .unwrap_or_else(|| (0..prim_positions.len() as u32).collect());

                let prim_normals: Vec<[f32; 3]> = if let Some(norms) = reader.read_normals() {
                    norms
                        .map(|n| mat4_transform_vector(&world_mat, n))
                        .collect()
                } else {
                    compute_normals(&prim_positions, &prim_indices)
                };

                if let Some(mat) = prim.material().name() {
                    material_set.insert(mat.to_string());
                }

                all_positions.extend(prim_positions);
                all_normals.extend(prim_normals);
                all_indices.extend(prim_indices.iter().map(|i| i + base_index));
            }
        }

        for child in node.children() {
            visit_node(
                &child,
                &world_mat,
                buffers,
                all_positions,
                all_indices,
                all_normals,
                material_set,
            );
        }
    }

    let identity = mat4_identity();
    if let Some(scene) = gltf.default_scene().or_else(|| gltf.scenes().next()) {
        for node in scene.nodes() {
            visit_node(
                &node,
                &identity,
                &buffers,
                &mut all_positions,
                &mut all_indices,
                &mut all_normals,
                &mut material_set,
            );
        }
    } else {
        // Fallback: iterate all meshes directly if no scene is defined
        for mesh in gltf.meshes() {
            for prim in mesh.primitives() {
                if prim.mode() != gltf::mesh::Mode::Triangles {
                    continue;
                }
                let get_buffer = |b: gltf::Buffer<'_>| buffers.get(b.index()).copied();
                let reader = prim.reader(get_buffer);
                let Some(positions) = reader.read_positions() else {
                    continue;
                };
                let base_index = all_positions.len() as u32;
                let prim_pos: Vec<[f32; 3]> = positions.collect();
                let prim_indices: Vec<u32> = reader
                    .read_indices()
                    .map(|idx| idx.into_u32().collect())
                    .unwrap_or_else(|| (0..prim_pos.len() as u32).collect());
                let prim_normals: Vec<[f32; 3]> = reader
                    .read_normals()
                    .map(|n| n.collect())
                    .unwrap_or_else(|| compute_normals(&prim_pos, &prim_indices));

                if let Some(mat) = prim.material().name() {
                    material_set.insert(mat.to_string());
                }
                all_positions.extend(prim_pos);
                all_normals.extend(prim_normals);
                all_indices.extend(prim_indices.iter().map(|i| i + base_index));
            }
        }
    }

    if material_set.is_empty() {
        material_set.insert("default".to_string());
    }

    let bounding_box = AABB::from_points(&all_positions);

    Ok(MeshGeometry {
        positions: all_positions,
        indices: all_indices,
        normals: all_normals,
        materials: material_set.into_iter().collect(),
        bounding_box,
        edges: Vec::new(),
    })
}

/// Extract geometry from an in-memory ModelBundle (.rxmodel).
pub fn extract_from_model_bundle(bundle: &ModelBundle) -> Result<MeshGeometry> {
    let mut all_positions = Vec::new();
    let mut all_indices = Vec::new();
    let mut all_normals = Vec::new();
    let mut material_set = BTreeSet::new();

    for m in &bundle.materials {
        material_set.insert(m.name.clone());
    }

    fn traverse_node(
        node_idx: usize,
        bundle: &ModelBundle,
        parent_mat: &Mat4,
        all_positions: &mut Vec<[f32; 3]>,
        all_indices: &mut Vec<u32>,
        all_normals: &mut Vec<[f32; 3]>,
    ) {
        if node_idx >= bundle.nodes.len() {
            return;
        }
        let node = &bundle.nodes[node_idx];
        let local_mat = if let Some(m) = &node.matrix {
            mat4_from_col_major(m)
        } else {
            mat4_from_trs(node.translation, node.rotation, node.scale)
        };
        let world_mat = mat4_mul(parent_mat, &local_mat);

        for &prim_idx in &node.primitives {
            if prim_idx >= bundle.primitives.len() {
                continue;
            }
            let prim = &bundle.primitives[prim_idx];
            let base_index = all_positions.len() as u32;

            let mut prim_pos = Vec::with_capacity(prim.positions.len());
            for pos in &prim.positions {
                prim_pos.push(mat4_transform_point(&world_mat, *pos));
            }

            let prim_normals: Vec<[f32; 3]> = if prim.normals.len() == prim.positions.len() {
                prim.normals
                    .iter()
                    .map(|n| mat4_transform_vector(&world_mat, *n))
                    .collect()
            } else {
                compute_normals(&prim_pos, &prim.indices)
            };

            all_positions.extend(prim_pos);
            all_normals.extend(prim_normals);
            all_indices.extend(prim.indices.iter().map(|i| i + base_index));
        }

        for &child in &node.children {
            traverse_node(
                child,
                bundle,
                &world_mat,
                all_positions,
                all_indices,
                all_normals,
            );
        }
    }

    let identity = mat4_identity();
    if !bundle.roots.is_empty() {
        for &root in &bundle.roots {
            traverse_node(
                root,
                bundle,
                &identity,
                &mut all_positions,
                &mut all_indices,
                &mut all_normals,
            );
        }
    } else {
        // Fallback: iterate all primitives directly
        for prim in &bundle.primitives {
            let base_index = all_positions.len() as u32;
            let prim_normals: Vec<[f32; 3]> = if prim.normals.len() == prim.positions.len() {
                prim.normals.clone()
            } else {
                compute_normals(&prim.positions, &prim.indices)
            };
            all_positions.extend(prim.positions.clone());
            all_normals.extend(prim_normals);
            all_indices.extend(prim.indices.iter().map(|i| i + base_index));
        }
    }

    if material_set.is_empty() {
        material_set.insert("default".to_string());
    }

    let bounding_box = AABB::from_points(&all_positions);

    Ok(MeshGeometry {
        positions: all_positions,
        indices: all_indices,
        normals: all_normals,
        materials: material_set.into_iter().collect(),
        bounding_box,
        edges: Vec::new(),
    })
}

/// Extract geometry from raw .rxmodel JSON bytes.
pub fn extract_from_rxmodel_bytes(bytes: &[u8]) -> Result<MeshGeometry> {
    let bundle: ModelBundle = serde_json::from_slice(bytes)
        .map_err(|e| AssetError::new("MODEL_PARSE_ERR", format!("Invalid .rxmodel JSON: {e}")))?;
    extract_from_model_bundle(&bundle)
}

/// Extract geometry from raw .rxmesh binary DAG bytes.
pub fn extract_from_rxmesh_bytes(bytes: &[u8], materials: Vec<String>) -> Result<MeshGeometry> {
    let dag = rurix_geom_build::read_dag(bytes)
        .map_err(|e| AssetError::new("RXMESH_PARSE_ERR", format!("Invalid .rxmesh binary DAG: {e}")))?;

    let positions = dag.vertices.clone();
    let indices: Vec<u32> = dag.triangle_indices.iter().map(|&i| i as u32).collect();
    let normals = compute_normals(&positions, &indices);
    let bounding_box = AABB::from_points(&positions);

    let materials = if materials.is_empty() {
        vec!["default".to_string()]
    } else {
        materials
    };

    Ok(MeshGeometry {
        positions,
        indices,
        normals,
        materials,
        bounding_box,
        edges: Vec::new(),
    })
}

/// Load and extract unified geometry from any supported 3D file on disk.
pub fn extract_geometry_from_file(_project: &ForgeProject, abs_path: &Path) -> Result<MeshGeometry> {
    if !abs_path.is_file() {
        // If it's a directory (such as Content/Models/<guid>/), check for model.rxmodel inside
        let candidate = abs_path.join("model.rxmodel");
        if candidate.is_file() {
            let bytes = std::fs::read(&candidate)?;
            return extract_from_rxmodel_bytes(&bytes);
        }
        return Err(AssetError::new(
            "ASSET_NOT_FOUND",
            format!("3D asset file not found: {}", abs_path.display()),
        ));
    }

    let bytes = std::fs::read(abs_path)?;
    let ext = abs_path
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();

    match ext.as_str() {
        "glb" | "gltf" => extract_from_glb_bytes(&bytes),
        "rxmodel" => extract_from_rxmodel_bytes(&bytes),
        "rxmesh" => {
            // Try to find material slots in matching .meta file
            let meta_path = abs_path.with_extension("meta");
            let materials = if meta_path.is_file() {
                crate::meta::MetaDoc::load(&meta_path)
                    .ok()
                    .and_then(|m| m.import_settings.get("materialSlots").cloned())
                    .and_then(|v| v.as_array().cloned())
                    .map(|arr| {
                        arr.into_iter()
                            .filter_map(|x| x.as_str().map(String::from))
                            .collect()
                    })
                    .unwrap_or_default()
            } else {
                Vec::new()
            };
            extract_from_rxmesh_bytes(&bytes, materials)
        }
        _ => {
            // Check magic bytes: if starts with "glTF", parse as GLB
            if bytes.starts_with(b"glTF") {
                extract_from_glb_bytes(&bytes)
            } else if serde_json::from_slice::<ModelBundle>(&bytes).is_ok() {
                extract_from_rxmodel_bytes(&bytes)
            } else {
                Err(AssetError::new(
                    "UNSUPPORTED_FORMAT",
                    format!("Unsupported 3D asset file extension: .{ext}"),
                ))
            }
        }
    }
}
