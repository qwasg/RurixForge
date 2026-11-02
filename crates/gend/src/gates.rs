//! Five-Stage Objective Verification Gates (Gate 1~5) for 3D Character Pipeline
//!
//! Enforces strict, objective mathematical constraints and anti-facade invariants
//! (Redline I-5) across the complete character production lifecycle:
//!   Gate 1: Concept Art (resolution >= 256x256, foreground silhouette coverage >= 1%, contrast variance)
//!   Gate 2: Raw GLB (glTF 2.0 magic bytes, valid JSON/BIN chunks, TRIANGLES mode, TEXCOORD_0)
//!   Gate 3: Polycount & Geometry (triangles <= targetFaces, non-degenerate surface area > 1e-10)
//!   Gate 4: Rigging & Skinning (non-empty skins, matching IBMs, 4-bone normalized weights sum = 1.0, palette bounds)
//!   Gate 5: Motion, Deformation & Publication (Idle/Walk clips, timestamp monotonicity, Forward-Kinematics LBS vertex delta >= 0.01, .rxmodel + .rxprefab publication)

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::path::Path;

// ---------------------------------------------------------------------------
// Error Codes & Structure
// ---------------------------------------------------------------------------

pub const GATE1_IMAGE_INVALID: &str = "GATE1_IMAGE_INVALID";
pub const GATE2_MODEL_INVALID: &str = "GATE2_MODEL_INVALID";
pub const GATE3_POLYCOUNT_EXCEEDED: &str = "GATE3_POLYCOUNT_EXCEEDED";
pub const GATE3_DEGENERATE_MESH: &str = "GATE3_DEGENERATE_MESH";
pub const GATE4_RIGGING_INVALID: &str = "GATE4_RIGGING_INVALID";
pub const GATE5_ANIMATION_INVALID: &str = "GATE5_ANIMATION_INVALID";
pub const GATE5_PUBLISH_FAILED: &str = "GATE5_PUBLISH_FAILED";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GateError {
    pub gate: String,
    pub error_code: String,
    pub message: String,
    pub details: Value,
}

impl GateError {
    pub fn new(gate: impl Into<String>, error_code: impl Into<String>, message: impl Into<String>, details: Value) -> Self {
        Self {
            gate: gate.into(),
            error_code: error_code.into(),
            message: message.into(),
            details,
        }
    }
}

impl std::fmt::Display for GateError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "[{} - {}] {}", self.gate, self.error_code, self.message)
    }
}

impl std::error::Error for GateError {}

impl From<GateError> for crate::GenError {
    fn from(e: GateError) -> Self {
        crate::GenError::new(crate::GEN_BACKEND_ERROR, e.to_string())
    }
}

pub type GateResult<T> = std::result::Result<T, GateError>;

// ---------------------------------------------------------------------------
// Metrics Data Structures
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConceptArtMetrics {
    pub width: u32,
    pub height: u32,
    pub has_alpha: bool,
    pub foreground_coverage: f64,
    pub contrast_variance: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RawGlbMetrics {
    pub byte_length: usize,
    pub mesh_count: usize,
    pub primitive_count: usize,
    pub total_vertices: usize,
    pub has_texcoord_0: bool,
    pub generator: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PolyMetrics {
    pub total_triangles: u32,
    pub total_vertices: u32,
    pub target_faces: u32,
    pub degenerate_triangles: u32,
    pub min_triangle_area: f64,
    pub surface_area: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RiggingMetrics {
    pub joint_count: usize,
    pub vertex_count: usize,
    pub max_influences_per_vertex: usize,
    pub normalized_weights: bool,
    pub all_joints_within_palette: bool,
    pub ibm_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MotionMetrics {
    pub idle_duration: f32,
    pub walk_duration: f32,
    pub max_vertex_displacement: f32,
    pub threshold_epsilon: f32,
    pub deformation_verified: bool,
    pub published_model_path: Option<String>,
    pub published_prefab_path: Option<String>,
}

// ---------------------------------------------------------------------------
// Gate 1: Concept Art Validator
// ---------------------------------------------------------------------------

/// Validates concept art image existence, minimum resolution (>=256x256),
/// and non-empty foreground silhouette (alpha coverage or luminance variance).
pub fn validate_gate_1_concept_art(path: &Path) -> GateResult<ConceptArtMetrics> {
    let gate = "Gate 1 (Concept Art)";
    if !path.is_file() {
        return Err(GateError::new(
            gate,
            GATE1_IMAGE_INVALID,
            format!("Concept art file does not exist: {}", path.display()),
            json!({ "path": path.to_string_lossy(), "reason": "file_not_found" }),
        ));
    }
    let bytes = std::fs::read(path).map_err(|e| {
        GateError::new(
            gate,
            GATE1_IMAGE_INVALID,
            format!("Failed to read concept art file: {e}"),
            json!({ "error": e.to_string() }),
        )
    })?;
    validate_gate_1_concept_art_bytes(&bytes)
}

pub fn validate_concept_art(path: &Path) -> GateResult<ConceptArtMetrics> {
    validate_gate_1_concept_art(path)
}

pub fn validate_gate_1_concept_art_bytes(bytes: &[u8]) -> GateResult<ConceptArtMetrics> {
    let gate = "Gate 1 (Concept Art)";
    if bytes.is_empty() {
        return Err(GateError::new(
            gate,
            GATE1_IMAGE_INVALID,
            "Concept art image file is 0 bytes",
            json!({ "reason": "file_empty" }),
        ));
    }

    let img = image::load_from_memory(bytes).map_err(|e| {
        GateError::new(
            gate,
            GATE1_IMAGE_INVALID,
            format!("Concept art format decode failed: {e}"),
            json!({ "error": e.to_string() }),
        )
    })?;

    let (width, height) = (img.width(), img.height());
    if width < 256 || height < 256 {
        return Err(GateError::new(
            gate,
            GATE1_IMAGE_INVALID,
            format!("Image resolution {width}x{height} below minimum required 256x256"),
            json!({ "width": width, "height": height, "minWidth": 256, "minHeight": 256 }),
        ));
    }

    let rgba = img.to_rgba8();
    let total_pixels = (width as usize) * (height as usize);
    let mut non_transparent = 0usize;
    let mut has_variable_alpha = false;
    let mut alpha_ref: Option<u8> = None;

    let mut sum_lum = 0.0f64;
    let mut sum_lum_sq = 0.0f64;

    for p in rgba.pixels() {
        let [r, g, b, a] = p.0;
        if a > 10 {
            non_transparent += 1;
        }
        match alpha_ref {
            None => alpha_ref = Some(a),
            Some(first) => {
                if a != first {
                    has_variable_alpha = true;
                }
            }
        }
        let lum = 0.299 * (r as f64) + 0.587 * (g as f64) + 0.114 * (b as f64);
        sum_lum += lum;
        sum_lum_sq += lum * lum;
    }

    let foreground_coverage = (non_transparent as f64) / (total_pixels as f64);
    let mean_lum = sum_lum / (total_pixels as f64);
    let contrast_variance = (sum_lum_sq / (total_pixels as f64)) - (mean_lum * mean_lum);

    if foreground_coverage < 0.01 {
        return Err(GateError::new(
            gate,
            GATE1_IMAGE_INVALID,
            format!("Concept art foreground coverage ({:.2}%) is empty/transparent", foreground_coverage * 100.0),
            json!({ "foregroundCoverage": foreground_coverage, "threshold": 0.01 }),
        ));
    }

    // For opaque canvas: check contrast variance to prevent solid blank white/black squares
    if !has_variable_alpha && contrast_variance < 10.0 {
        return Err(GateError::new(
            gate,
            GATE1_IMAGE_INVALID,
            format!("Concept art is flat/solid color (contrast variance {:.2} < 10.0)", contrast_variance),
            json!({ "contrastVariance": contrast_variance, "threshold": 10.0 }),
        ));
    }

    Ok(ConceptArtMetrics {
        width,
        height,
        has_alpha: has_variable_alpha,
        foreground_coverage,
        contrast_variance,
    })
}

// ---------------------------------------------------------------------------
// Gate 2: Raw GLB Validator
// ---------------------------------------------------------------------------

pub const GLTF_MAGIC: u32 = 0x4654_6C67; // "glTF"
pub const GLTF_JSON_CHUNK: u32 = 0x4E4F_534A; // "JSON"
pub const GLTF_BIN_CHUNK: u32 = 0x004E_4942; // "BIN\0"

/// Validates glTF 2.0 Binary magic bytes, JSON/BIN chunk layout, mesh existence,
/// TRIANGLES mode, and mandatory TEXCOORD_0 UV attribute.
pub fn validate_gate_2_raw_glb(path: &Path) -> GateResult<RawGlbMetrics> {
    let gate = "Gate 2 (Raw GLB)";
    if !path.is_file() {
        return Err(GateError::new(
            gate,
            GATE2_MODEL_INVALID,
            format!("GLB file does not exist: {}", path.display()),
            json!({ "path": path.to_string_lossy(), "reason": "file_not_found" }),
        ));
    }
    let bytes = std::fs::read(path).map_err(|e| {
        GateError::new(
            gate,
            GATE2_MODEL_INVALID,
            format!("Failed to read GLB file: {e}"),
            json!({ "error": e.to_string() }),
        )
    })?;
    validate_gate_2_raw_glb_bytes(&bytes)
}

pub fn validate_raw_glb(path: &Path) -> GateResult<RawGlbMetrics> {
    validate_gate_2_raw_glb(path)
}

pub fn validate_gate_2_raw_glb_bytes(bytes: &[u8]) -> GateResult<RawGlbMetrics> {
    let gate = "Gate 2 (Raw GLB)";
    if bytes.len() < 12 {
        return Err(GateError::new(
            gate,
            GATE2_MODEL_INVALID,
            format!("GLB buffer too short: {} bytes (minimum 12)", bytes.len()),
            json!({ "byteLength": bytes.len(), "reason": "buffer_too_short" }),
        ));
    }

    let magic = u32::from_le_bytes(bytes[0..4].try_into().unwrap());
    if magic != GLTF_MAGIC {
        return Err(GateError::new(
            gate,
            GATE2_MODEL_INVALID,
            format!("Invalid GLB magic: {magic:#x}, expected {GLTF_MAGIC:#x} ('glTF')"),
            json!({ "magic": magic, "expected": GLTF_MAGIC }),
        ));
    }

    let version = u32::from_le_bytes(bytes[4..8].try_into().unwrap());
    if version != 2 {
        return Err(GateError::new(
            gate,
            GATE2_MODEL_INVALID,
            format!("Unsupported glTF version: {version}, expected 2"),
            json!({ "version": version }),
        ));
    }

    let declared_length = u32::from_le_bytes(bytes[8..12].try_into().unwrap()) as usize;
    if declared_length > bytes.len() {
        return Err(GateError::new(
            gate,
            GATE2_MODEL_INVALID,
            format!("GLB truncated: declared {declared_length} bytes, actual {} bytes", bytes.len()),
            json!({ "declaredLength": declared_length, "actualLength": bytes.len() }),
        ));
    }

    let json_val = parse_glb_json_doc(bytes).ok_or_else(|| {
        GateError::new(
            gate,
            GATE2_MODEL_INVALID,
            "Failed to parse glTF JSON chunk in GLB",
            json!({ "reason": "invalid_json_chunk" }),
        )
    })?;

    let meshes = json_val.get("meshes").and_then(Value::as_array).ok_or_else(|| {
        GateError::new(
            gate,
            GATE2_MODEL_INVALID,
            "GLB contains zero meshes",
            json!({ "reason": "zero_meshes" }),
        )
    })?;

    if meshes.is_empty() {
        return Err(GateError::new(
            gate,
            GATE2_MODEL_INVALID,
            "GLB contains zero meshes",
            json!({ "reason": "zero_meshes" }),
        ));
    }

    let accessors = json_val.get("accessors").and_then(Value::as_array);
    let mut primitive_count = 0usize;
    let mut total_vertices = 0usize;
    let mut has_texcoord_0 = false;

    for mesh in meshes {
        let prims = mesh.get("primitives").and_then(Value::as_array).ok_or_else(|| {
            GateError::new(
                gate,
                GATE2_MODEL_INVALID,
                "Mesh contains zero primitives",
                json!({ "reason": "zero_primitives" }),
            )
        })?;

        if prims.is_empty() {
            return Err(GateError::new(
                gate,
                GATE2_MODEL_INVALID,
                "Mesh contains zero primitives",
                json!({ "reason": "zero_primitives" }),
            ));
        }

        for prim in prims {
            primitive_count += 1;
            let mode = prim.get("mode").and_then(Value::as_u64).unwrap_or(4);
            if mode != 4 {
                return Err(GateError::new(
                    gate,
                    GATE2_MODEL_INVALID,
                    format!("Primitive mode is not TRIANGLES (mode={mode}, expected 4)"),
                    json!({ "mode": mode, "expected": 4 }),
                ));
            }

            let attrs = prim.get("attributes").and_then(Value::as_object).ok_or_else(|| {
                GateError::new(
                    gate,
                    GATE2_MODEL_INVALID,
                    "Primitive missing attributes object",
                    json!({ "reason": "missing_attributes" }),
                )
            })?;

            if !attrs.contains_key("POSITION") {
                return Err(GateError::new(
                    gate,
                    GATE2_MODEL_INVALID,
                    "Primitive missing mandatory POSITION attribute",
                    json!({ "reason": "missing_position" }),
                ));
            }

            if attrs.contains_key("TEXCOORD_0") {
                has_texcoord_0 = true;
            }

            if let (Some(pos_acc_idx), Some(accs)) = (attrs.get("POSITION").and_then(Value::as_u64), accessors) {
                if let Some(pos_acc) = accs.get(pos_acc_idx as usize) {
                    if let Some(count) = pos_acc.get("count").and_then(Value::as_u64) {
                        total_vertices += count as usize;
                    }
                }
            }
        }
    }

    if !has_texcoord_0 {
        return Err(GateError::new(
            gate,
            GATE2_MODEL_INVALID,
            "Primitive is missing mandatory TEXCOORD_0 UV attribute",
            json!({ "reason": "missing_texcoord_0" }),
        ));
    }

    let generator = json_val
        .pointer("/asset/generator")
        .and_then(Value::as_str)
        .map(str::to_string);

    Ok(RawGlbMetrics {
        byte_length: bytes.len(),
        mesh_count: meshes.len(),
        primitive_count,
        total_vertices,
        has_texcoord_0,
        generator,
    })
}

// ---------------------------------------------------------------------------
// Gate 3: Polycount & Surface Non-Degeneracy Validator
// ---------------------------------------------------------------------------

/// Validates target triangle budget (triangles <= targetFaces) and non-degeneracy
/// (zero duplicate index triangles, triangle surface area > 1e-10).
pub fn validate_gate_3_polycount(path: &Path, target_faces: u32) -> GateResult<PolyMetrics> {
    let gate = "Gate 3 (Polycount & Geometry)";
    let bytes = std::fs::read(path).map_err(|e| {
        GateError::new(
            gate,
            GATE3_DEGENERATE_MESH,
            format!("Failed to read GLB: {e}"),
            json!({ "error": e.to_string() }),
        )
    })?;
    validate_gate_3_polycount_bytes(&bytes, target_faces)
}

pub fn validate_polycount(path: &Path, target_faces: u32) -> GateResult<PolyMetrics> {
    validate_gate_3_polycount(path, target_faces)
}

pub fn validate_gate_3_polycount_bytes(bytes: &[u8], target_faces: u32) -> GateResult<PolyMetrics> {
    let gate = "Gate 3 (Polycount & Geometry)";
    let parsed = ParsedGlb::from_bytes(bytes).map_err(|e| {
        GateError::new(
            gate,
            GATE3_DEGENERATE_MESH,
            format!("GLB parse error: {e}"),
            json!({ "error": e }),
        )
    })?;

    let (triangles, vertices, degenerate_count, min_area, total_area) = parsed.evaluate_geometry()?;

    if triangles == 0 {
        return Err(GateError::new(
            gate,
            GATE3_DEGENERATE_MESH,
            "Mesh contains 0 triangles",
            json!({ "triangles": 0, "reason": "empty_mesh" }),
        ));
    }

    if triangles > target_faces {
        return Err(GateError::new(
            gate,
            GATE3_POLYCOUNT_EXCEEDED,
            format!("Triangle count {triangles} exceeds target budget {target_faces}"),
            json!({ "triangles": triangles, "targetFaces": target_faces, "excess": triangles - target_faces }),
        ));
    }

    if degenerate_count > 0 {
        return Err(GateError::new(
            gate,
            GATE3_DEGENERATE_MESH,
            format!("Mesh contains {degenerate_count} degenerate triangles (area <= 1e-10 or duplicate indices)"),
            json!({ "degenerateCount": degenerate_count, "triangles": triangles, "minArea": min_area }),
        ));
    }

    Ok(PolyMetrics {
        total_triangles: triangles,
        total_vertices: vertices,
        target_faces,
        degenerate_triangles: degenerate_count,
        min_triangle_area: min_area,
        surface_area: total_area,
    })
}

// ---------------------------------------------------------------------------
// Gate 4: Rigging & Skinning Validator
// ---------------------------------------------------------------------------

/// Validates non-empty skins, matching IBM count, 4-bone influence limit,
/// non-negative weights normalized to 1.0 (|sum - 1.0| < 1e-4), and palette bounds.
pub fn validate_gate_4_rigging(path: &Path) -> GateResult<RiggingMetrics> {
    let gate = "Gate 4 (Rigging & Skinning)";
    let bytes = std::fs::read(path).map_err(|e| {
        GateError::new(
            gate,
            GATE4_RIGGING_INVALID,
            format!("Failed to read GLB: {e}"),
            json!({ "error": e.to_string() }),
        )
    })?;
    validate_gate_4_rigging_bytes(&bytes)
}

pub fn validate_rigging(path: &Path) -> GateResult<RiggingMetrics> {
    validate_gate_4_rigging(path)
}

pub fn validate_gate_4_rigging_bytes(bytes: &[u8]) -> GateResult<RiggingMetrics> {
    let gate = "Gate 4 (Rigging & Skinning)";
    let parsed = ParsedGlb::from_bytes(bytes).map_err(|e| {
        GateError::new(
            gate,
            GATE4_RIGGING_INVALID,
            format!("GLB parse error: {e}"),
            json!({ "error": e }),
        )
    })?;

    let skins = parsed.json.get("skins").and_then(Value::as_array).ok_or_else(|| {
        GateError::new(
            gate,
            GATE4_RIGGING_INVALID,
            "No skins found in model (character must be skinned)",
            json!({ "reason": "no_skins" }),
        )
    })?;

    if skins.is_empty() {
        return Err(GateError::new(
            gate,
            GATE4_RIGGING_INVALID,
            "No skins found in model (character must be skinned)",
            json!({ "reason": "no_skins" }),
        ));
    }

    let skin = &skins[0];
    let joints = skin.get("joints").and_then(Value::as_array).ok_or_else(|| {
        GateError::new(
            gate,
            GATE4_RIGGING_INVALID,
            "Skin contains empty joint list",
            json!({ "reason": "empty_joints" }),
        )
    })?;

    if joints.is_empty() {
        return Err(GateError::new(
            gate,
            GATE4_RIGGING_INVALID,
            "Skin contains empty joint list",
            json!({ "reason": "empty_joints" }),
        ));
    }

    let joint_count = joints.len();

    let ibm_acc_idx = skin.get("inverseBindMatrices").and_then(Value::as_u64).ok_or_else(|| {
        GateError::new(
            gate,
            GATE4_RIGGING_INVALID,
            "Skin missing inverseBindMatrices accessor",
            json!({ "reason": "missing_ibms" }),
        )
    })?;

    let ibm_floats = parsed.get_accessor_floats(ibm_acc_idx as usize)?;
    let ibm_count = ibm_floats.len() / 16;
    if ibm_count != joint_count {
        return Err(GateError::new(
            gate,
            GATE4_RIGGING_INVALID,
            format!("Joint count ({joint_count}) does not match IBM count ({ibm_count})"),
            json!({ "jointCount": joint_count, "ibmCount": ibm_count }),
        ));
    }

    // Verify primitive skinning attributes
    let meshes = parsed.json.get("meshes").and_then(Value::as_array).ok_or_else(|| {
        GateError::new(gate, GATE4_RIGGING_INVALID, "No meshes in GLB", json!({}))
    })?;
    let prim = meshes[0].pointer("/primitives/0").ok_or_else(|| {
        GateError::new(gate, GATE4_RIGGING_INVALID, "No primitives in mesh", json!({}))
    })?;
    let attrs = prim.get("attributes").and_then(Value::as_object).ok_or_else(|| {
        GateError::new(gate, GATE4_RIGGING_INVALID, "No attributes in primitive", json!({}))
    })?;

    let joints_acc_idx = attrs.get("JOINTS_0").and_then(Value::as_u64).ok_or_else(|| {
        GateError::new(
            gate,
            GATE4_RIGGING_INVALID,
            "Primitive missing JOINTS_0 attribute",
            json!({ "reason": "missing_joints_0" }),
        )
    })?;

    let weights_acc_idx = attrs.get("WEIGHTS_0").and_then(Value::as_u64).ok_or_else(|| {
        GateError::new(
            gate,
            GATE4_RIGGING_INVALID,
            "Primitive missing WEIGHTS_0 attribute",
            json!({ "reason": "missing_weights_0" }),
        )
    })?;

    let joints_vec = parsed.get_accessor_u16(joints_acc_idx as usize)?;
    let weights_vec = parsed.get_accessor_floats(weights_acc_idx as usize)?;

    if joints_vec.len() != weights_vec.len() {
        return Err(GateError::new(
            gate,
            GATE4_RIGGING_INVALID,
            "JOINTS_0 count does not match WEIGHTS_0 count",
            json!({ "jointsCount": joints_vec.len(), "weightsCount": weights_vec.len() }),
        ));
    }

    let vertex_count = weights_vec.len() / 4;
    for v in 0..vertex_count {
        let w = [
            weights_vec[v * 4],
            weights_vec[v * 4 + 1],
            weights_vec[v * 4 + 2],
            weights_vec[v * 4 + 3],
        ];
        let j = [
            joints_vec[v * 4] as usize,
            joints_vec[v * 4 + 1] as usize,
            joints_vec[v * 4 + 2] as usize,
            joints_vec[v * 4 + 3] as usize,
        ];

        let sum = w[0] + w[1] + w[2] + w[3];
        if (sum - 1.0).abs() > 1e-4 {
            return Err(GateError::new(
                gate,
                GATE4_RIGGING_INVALID,
                format!("Vertex {v} weights sum to {sum:.5} (must equal 1.0)"),
                json!({ "vertexIndex": v, "weights": w, "sum": sum }),
            ));
        }

        for k in 0..4 {
            if w[k] < -1e-6 {
                return Err(GateError::new(
                    gate,
                    GATE4_RIGGING_INVALID,
                    format!("Negative skin weight detected at vertex {v}, influence {k}: {}", w[k]),
                    json!({ "vertexIndex": v, "influence": k, "weight": w[k] }),
                ));
            }
            if w[k] > 0.0 && j[k] >= joint_count {
                return Err(GateError::new(
                    gate,
                    GATE4_RIGGING_INVALID,
                    format!("Joint index {} out of bounds of skeleton palette ({joint_count})", j[k]),
                    json!({ "vertexIndex": v, "joint": j[k], "jointCount": joint_count }),
                ));
            }
        }
    }

    Ok(RiggingMetrics {
        joint_count,
        vertex_count,
        max_influences_per_vertex: 4,
        normalized_weights: true,
        all_joints_within_palette: true,
        ibm_count,
    })
}

// ---------------------------------------------------------------------------
// Gate 5: Motion Tracks, FK Deformation & Publication Validator
// ---------------------------------------------------------------------------

pub const DEFAULT_DEFORMATION_EPSILON: f32 = 0.01;

/// Validates independent Idle and Walk clips, timestamp monotonicity,
/// forward-kinematics geometric vertex deformation (delta >= epsilon),
/// and atomic publication of .rxmodel + .rxprefab.
pub fn validate_gate_5_motion_and_publish(
    glb_path: &Path,
    epsilon: f32,
    published_dir: Option<&Path>,
) -> GateResult<MotionMetrics> {
    let gate = "Gate 5 (Motion & Publication)";
    let bytes = std::fs::read(glb_path).map_err(|e| {
        GateError::new(
            gate,
            GATE5_ANIMATION_INVALID,
            format!("Failed to read GLB: {e}"),
            json!({ "error": e.to_string() }),
        )
    })?;
    validate_gate_5_motion_and_publish_bytes(&bytes, epsilon, published_dir)
}

pub fn validate_motion_and_publish(glb_path: &Path, published_model: &Path) -> GateResult<MotionMetrics> {
    let pub_dir = if published_model.is_dir() {
        published_model
    } else {
        published_model.parent().unwrap_or(published_model)
    };
    validate_gate_5_motion_and_publish(glb_path, DEFAULT_DEFORMATION_EPSILON, Some(pub_dir))
}

pub fn validate_gate_5_motion_and_publish_bytes(
    bytes: &[u8],
    epsilon: f32,
    published_dir: Option<&Path>,
) -> GateResult<MotionMetrics> {
    let gate = "Gate 5 (Motion & Publication)";
    let parsed = ParsedGlb::from_bytes(bytes).map_err(|e| {
        GateError::new(
            gate,
            GATE5_ANIMATION_INVALID,
            format!("GLB parse error: {e}"),
            json!({ "error": e }),
        )
    })?;

    let anims = parsed.json.get("animations").and_then(Value::as_array).ok_or_else(|| {
        GateError::new(
            gate,
            GATE5_ANIMATION_INVALID,
            "GLB contains zero animations",
            json!({ "reason": "no_animations" }),
        )
    })?;

    let mut idle_anim: Option<&Value> = None;
    let mut walk_anim: Option<&Value> = None;

    for a in anims {
        if let Some(name) = a.get("name").and_then(Value::as_str) {
            if name.eq_ignore_ascii_case("Idle") {
                idle_anim = Some(a);
            } else if name.eq_ignore_ascii_case("Walk") {
                walk_anim = Some(a);
            }
        }
    }

    if idle_anim.is_none() || walk_anim.is_none() {
        let found: Vec<_> = anims
            .iter()
            .filter_map(|a| a.get("name").and_then(Value::as_str))
            .collect();
        return Err(GateError::new(
            gate,
            GATE5_ANIMATION_INVALID,
            format!("Missing required animation tracks (found {found:?}, required ['Idle', 'Walk'])"),
            json!({ "found": found, "required": ["Idle", "Walk"] }),
        ));
    }

    let idle = idle_anim.unwrap();
    let walk = walk_anim.unwrap();

    let (idle_duration, idle_monotonic) = parsed.check_anim_track(idle)?;
    let (walk_duration, walk_monotonic) = parsed.check_anim_track(walk)?;

    if !idle_monotonic || !walk_monotonic {
        return Err(GateError::new(
            gate,
            GATE5_ANIMATION_INVALID,
            "Animation track has non-monotonic or duplicate timestamps",
            json!({ "idleMonotonic": idle_monotonic, "walkMonotonic": walk_monotonic }),
        ));
    }

    if idle_duration <= 0.0 || walk_duration <= 0.0 {
        return Err(GateError::new(
            gate,
            GATE5_ANIMATION_INVALID,
            "Animation clip has non-positive duration",
            json!({ "idleDuration": idle_duration, "walkDuration": walk_duration }),
        ));
    }

    // Forward-Kinematics Linear Blend Skinning Deformation Evaluation
    let idle_verts = parsed.evaluate_skinned_vertices("Idle", 0.0)?;
    let walk_verts = parsed.evaluate_skinned_vertices("Walk", 0.4 * walk_duration)?;

    if idle_verts.len() != walk_verts.len() {
        return Err(GateError::new(
            gate,
            GATE5_ANIMATION_INVALID,
            "Vertex count mismatch between Idle and Walk evaluations",
            json!({ "idleVerts": idle_verts.len(), "walkVerts": walk_verts.len() }),
        ));
    }

    let mut max_delta = 0.0f32;
    for i in 0..idle_verts.len() {
        let dx = walk_verts[i][0] - idle_verts[i][0];
        let dy = walk_verts[i][1] - idle_verts[i][1];
        let dz = walk_verts[i][2] - idle_verts[i][2];
        let dist = (dx * dx + dy * dy + dz * dz).sqrt();
        if dist > max_delta {
            max_delta = dist;
        }
    }

    if max_delta < epsilon {
        return Err(GateError::new(
            gate,
            GATE5_ANIMATION_INVALID,
            format!("Vertex deformation difference between Idle and Walk ({max_delta:.5}) below threshold {epsilon}"),
            json!({ "maxDelta": max_delta, "threshold": epsilon }),
        ));
    }

    // Publication Package Verification (if published directory specified)
    let mut model_path_str = None;
    let mut prefab_path_str = None;

    if let Some(dir) = published_dir {
        let model_file = dir.join("model.rxmodel");
        let prefab_file = dir.join("template.rxprefab");

        if !model_file.is_file() || !prefab_file.is_file() {
            return Err(GateError::new(
                gate,
                GATE5_PUBLISH_FAILED,
                format!("Published package missing model.rxmodel or template.rxprefab in {}", dir.display()),
                json!({ "directory": dir.to_string_lossy() }),
            ));
        }

        let prefab_str = std::fs::read_to_string(&prefab_file).map_err(|e| {
            GateError::new(gate, GATE5_PUBLISH_FAILED, format!("Failed to read template.rxprefab: {e}"), json!({}))
        })?;
        let prefab_json: Value = serde_json::from_str(&prefab_str).map_err(|e| {
            GateError::new(gate, GATE5_PUBLISH_FAILED, format!("Invalid JSON in template.rxprefab: {e}"), json!({}))
        })?;

        let root_entity = prefab_json
            .get("entities")
            .and_then(Value::as_array)
            .and_then(|arr| arr.iter().find(|e| e.get("id").and_then(Value::as_u64) == Some(1)))
            .ok_or_else(|| {
                GateError::new(gate, GATE5_PUBLISH_FAILED, "Root entity (id=1) missing in template.rxprefab", json!({}))
            })?;

        let components = root_entity.get("components").and_then(Value::as_object).ok_or_else(|| {
            GateError::new(gate, GATE5_PUBLISH_FAILED, "Root entity missing components object", json!({}))
        })?;

        if !components.contains_key("Animator") || !components.contains_key("CharacterController") {
            return Err(GateError::new(
                gate,
                GATE5_PUBLISH_FAILED,
                "template.rxprefab root entity missing Animator or CharacterController component",
                json!({ "components": components.keys().collect::<Vec<_>>() }),
            ));
        }

        model_path_str = Some(model_file.to_string_lossy().to_string());
        prefab_path_str = Some(prefab_file.to_string_lossy().to_string());
    }

    Ok(MotionMetrics {
        idle_duration,
        walk_duration,
        max_vertex_displacement: max_delta,
        threshold_epsilon: epsilon,
        deformation_verified: true,
        published_model_path: model_path_str,
        published_prefab_path: prefab_path_str,
    })
}

// ---------------------------------------------------------------------------
// Internal Helper: Lightweight glTF/GLB Binary Parser & FK Skinning Engine
// ---------------------------------------------------------------------------

struct ParsedGlb<'a> {
    pub json: Value,
    pub bin: &'a [u8],
}

impl<'a> ParsedGlb<'a> {
    pub fn from_bytes(bytes: &'a [u8]) -> std::result::Result<Self, String> {
        if bytes.len() < 12 {
            return Err("Buffer too short".into());
        }
        let magic = u32::from_le_bytes(bytes[0..4].try_into().unwrap());
        if magic != GLTF_MAGIC {
            return Err("Invalid magic".into());
        }
        let length = u32::from_le_bytes(bytes[8..12].try_into().unwrap()) as usize;
        let mut offset = 12;
        let mut json_val: Option<Value> = None;
        let mut bin_slice: &'a [u8] = &[];

        while offset < length && offset + 8 <= bytes.len() {
            let chunk_len = u32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap()) as usize;
            let chunk_type = u32::from_le_bytes(bytes[offset + 4..offset + 8].try_into().unwrap());
            if offset + 8 + chunk_len > bytes.len() {
                break;
            }
            let chunk_data = &bytes[offset + 8..offset + 8 + chunk_len];

            if chunk_type == GLTF_JSON_CHUNK {
                json_val = serde_json::from_slice(chunk_data).ok();
            } else if chunk_type == GLTF_BIN_CHUNK {
                bin_slice = chunk_data;
            }
            offset += 8 + chunk_len;
        }

        let json = json_val.ok_or_else(|| "Missing JSON chunk".to_string())?;
        Ok(Self { json, bin: bin_slice })
    }

    pub fn get_accessor_floats(&self, acc_idx: usize) -> GateResult<Vec<f32>> {
        let acc = self.json.pointer(&format!("/accessors/{acc_idx}")).ok_or_else(|| {
            GateError::new("GLB", GATE2_MODEL_INVALID, format!("Accessor {acc_idx} not found"), json!({}))
        })?;
        let view_idx = acc.get("bufferView").and_then(Value::as_u64).unwrap_or(0) as usize;
        let view = self.json.pointer(&format!("/bufferViews/{view_idx}")).ok_or_else(|| {
            GateError::new("GLB", GATE2_MODEL_INVALID, format!("BufferView {view_idx} not found"), json!({}))
        })?;

        let view_offset = view.get("byteOffset").and_then(Value::as_u64).unwrap_or(0) as usize;
        let acc_offset = acc.get("byteOffset").and_then(Value::as_u64).unwrap_or(0) as usize;
        let count = acc.get("count").and_then(Value::as_u64).unwrap_or(0) as usize;
        let type_str = acc.get("type").and_then(Value::as_str).unwrap_or("SCALAR");

        let dim = match type_str {
            "SCALAR" => 1,
            "VEC2" => 2,
            "VEC3" => 3,
            "VEC4" => 4,
            "MAT4" => 16,
            _ => 1,
        };

        let start = view_offset + acc_offset;
        let total_floats = count * dim;
        let byte_len = total_floats * 4;

        if start + byte_len > self.bin.len() {
            return Err(GateError::new("GLB", GATE2_MODEL_INVALID, "Accessor out of binary bounds", json!({})));
        }

        let slice = &self.bin[start..start + byte_len];
        let mut floats = Vec::with_capacity(total_floats);
        for chunk in slice.chunks_exact(4) {
            floats.push(f32::from_le_bytes(chunk.try_into().unwrap()));
        }
        Ok(floats)
    }

    pub fn get_accessor_u16(&self, acc_idx: usize) -> GateResult<Vec<u16>> {
        let acc = self.json.pointer(&format!("/accessors/{acc_idx}")).ok_or_else(|| {
            GateError::new("GLB", GATE2_MODEL_INVALID, format!("Accessor {acc_idx} not found"), json!({}))
        })?;
        let view_idx = acc.get("bufferView").and_then(Value::as_u64).unwrap_or(0) as usize;
        let view = self.json.pointer(&format!("/bufferViews/{view_idx}")).ok_or_else(|| {
            GateError::new("GLB", GATE2_MODEL_INVALID, format!("BufferView {view_idx} not found"), json!({}))
        })?;

        let view_offset = view.get("byteOffset").and_then(Value::as_u64).unwrap_or(0) as usize;
        let acc_offset = acc.get("byteOffset").and_then(Value::as_u64).unwrap_or(0) as usize;
        let count = acc.get("count").and_then(Value::as_u64).unwrap_or(0) as usize;
        let type_str = acc.get("type").and_then(Value::as_str).unwrap_or("SCALAR");
        let comp_type = acc.get("componentType").and_then(Value::as_u64).unwrap_or(5123);

        let dim = match type_str {
            "SCALAR" => 1,
            "VEC2" => 2,
            "VEC3" => 3,
            "VEC4" => 4,
            _ => 1,
        };

        let start = view_offset + acc_offset;
        let total_elems = count * dim;
        let mut result = Vec::with_capacity(total_elems);

        if comp_type == 5121 {
            // UNSIGNED_BYTE
            if start + total_elems > self.bin.len() {
                return Err(GateError::new("GLB", GATE2_MODEL_INVALID, "Buffer overflow", json!({})));
            }
            for &b in &self.bin[start..start + total_elems] {
                result.push(b as u16);
            }
        } else if comp_type == 5123 {
            // UNSIGNED_SHORT
            let byte_len = total_elems * 2;
            if start + byte_len > self.bin.len() {
                return Err(GateError::new("GLB", GATE2_MODEL_INVALID, "Buffer overflow", json!({})));
            }
            for chunk in self.bin[start..start + byte_len].chunks_exact(2) {
                result.push(u16::from_le_bytes(chunk.try_into().unwrap()));
            }
        } else if comp_type == 5125 {
            // UNSIGNED_INT
            let byte_len = total_elems * 4;
            if start + byte_len > self.bin.len() {
                return Err(GateError::new("GLB", GATE2_MODEL_INVALID, "Buffer overflow", json!({})));
            }
            for chunk in self.bin[start..start + byte_len].chunks_exact(4) {
                let v = u32::from_le_bytes(chunk.try_into().unwrap());
                result.push(v as u16);
            }
        }
        Ok(result)
    }

    pub fn evaluate_geometry(&self) -> GateResult<(u32, u32, u32, f64, f64)> {
        let prim = self
            .json
            .pointer("/meshes/0/primitives/0")
            .ok_or_else(|| GateError::new("GLB", GATE3_DEGENERATE_MESH, "No primitives", json!({})))?;

        let pos_acc_idx = prim
            .pointer("/attributes/POSITION")
            .and_then(Value::as_u64)
            .ok_or_else(|| GateError::new("GLB", GATE3_DEGENERATE_MESH, "Missing POSITION", json!({})))?
            as usize;
        let positions_flat = self.get_accessor_floats(pos_acc_idx)?;
        let vertex_count = (positions_flat.len() / 3) as u32;

        let indices: Vec<u32> = if let Some(idx_acc_idx) = prim.get("indices").and_then(Value::as_u64) {
            let u16_indices = self.get_accessor_u16(idx_acc_idx as usize)?;
            u16_indices.into_iter().map(|x| x as u32).collect()
        } else {
            (0..vertex_count).collect()
        };

        let triangle_count = (indices.len() / 3) as u32;
        let mut degenerate_count = 0u32;
        let mut min_area = f64::MAX;
        let mut total_area = 0.0f64;

        for t in 0..triangle_count as usize {
            let i0 = indices[t * 3] as usize;
            let i1 = indices[t * 3 + 1] as usize;
            let i2 = indices[t * 3 + 2] as usize;

            if i0 == i1 || i1 == i2 || i0 == i2 {
                degenerate_count += 1;
                continue;
            }

            if (i0 * 3 + 2 >= positions_flat.len())
                || (i1 * 3 + 2 >= positions_flat.len())
                || (i2 * 3 + 2 >= positions_flat.len())
            {
                degenerate_count += 1;
                continue;
            }

            let v0 = [
                positions_flat[i0 * 3] as f64,
                positions_flat[i0 * 3 + 1] as f64,
                positions_flat[i0 * 3 + 2] as f64,
            ];
            let v1 = [
                positions_flat[i1 * 3] as f64,
                positions_flat[i1 * 3 + 1] as f64,
                positions_flat[i1 * 3 + 2] as f64,
            ];
            let v2 = [
                positions_flat[i2 * 3] as f64,
                positions_flat[i2 * 3 + 1] as f64,
                positions_flat[i2 * 3 + 2] as f64,
            ];

            let e1 = [v1[0] - v0[0], v1[1] - v0[1], v1[2] - v0[2]];
            let e2 = [v2[0] - v0[0], v2[1] - v0[1], v2[2] - v0[2]];
            let cross = [
                e1[1] * e2[2] - e1[2] * e2[1],
                e1[2] * e2[0] - e1[0] * e2[2],
                e1[0] * e2[1] - e1[1] * e2[0],
            ];
            let area = 0.5 * (cross[0] * cross[0] + cross[1] * cross[1] + cross[2] * cross[2]).sqrt();

            if area < min_area {
                min_area = area;
            }
            total_area += area;

            if area <= 1e-10 {
                degenerate_count += 1;
            }
        }

        if triangle_count == 0 {
            min_area = 0.0;
        }

        Ok((triangle_count, vertex_count, degenerate_count, min_area, total_area))
    }

    pub fn check_anim_track(&self, anim: &Value) -> GateResult<(f32, bool)> {
        let samplers = anim.get("samplers").and_then(Value::as_array).ok_or_else(|| {
            GateError::new("Gate 5", GATE5_ANIMATION_INVALID, "No samplers in animation", json!({}))
        })?;

        let mut max_time = 0.0f32;
        let mut monotonic = true;

        for s in samplers {
            let input_acc_idx = s.get("input").and_then(Value::as_u64).unwrap_or(0) as usize;
            let times = self.get_accessor_floats(input_acc_idx)?;
            if times.len() < 2 {
                return Ok((0.0, false));
            }
            for w in times.windows(2) {
                if w[1] <= w[0] {
                    monotonic = false;
                }
            }
            if let Some(&last) = times.last() {
                if last > max_time {
                    max_time = last;
                }
            }
        }

        Ok((max_time, monotonic))
    }

    pub fn evaluate_skinned_vertices(&self, clip_name: &str, sample_time: f32) -> GateResult<Vec<[f32; 3]>> {
        let anims = self.json.get("animations").and_then(Value::as_array).ok_or_else(|| {
            GateError::new("Gate 5", GATE5_ANIMATION_INVALID, "No animations", json!({}))
        })?;

        let anim = anims
            .iter()
            .find(|a| a.get("name").and_then(Value::as_str).unwrap_or("").eq_ignore_ascii_case(clip_name))
            .ok_or_else(|| {
                GateError::new("Gate 5", GATE5_ANIMATION_INVALID, format!("Clip '{clip_name}' not found"), json!({}))
            })?;

        let skin = self.json.pointer("/skins/0").ok_or_else(|| {
            GateError::new("Gate 5", GATE5_ANIMATION_INVALID, "No skin for FK evaluation", json!({}))
        })?;

        let joint_nodes: Vec<usize> = skin
            .get("joints")
            .and_then(Value::as_array)
            .unwrap_or(&vec![])
            .iter()
            .filter_map(Value::as_u64)
            .map(|x| x as usize)
            .collect();

        let ibm_acc_idx = skin.get("inverseBindMatrices").and_then(Value::as_u64).unwrap_or(0) as usize;
        let ibm_floats = self.get_accessor_floats(ibm_acc_idx)?;

        let nodes = self.json.get("nodes").and_then(Value::as_array).ok_or_else(|| {
            GateError::new("Gate 5", GATE5_ANIMATION_INVALID, "No nodes in document", json!({}))
        })?;

        // 1. Compute local transforms for each joint node at sample_time
        let mut local_transforms: HashMap<usize, [f32; 16]> = HashMap::new();
        let samplers = anim.get("samplers").and_then(Value::as_array).cloned().unwrap_or_default();
        let channels = anim.get("channels").and_then(Value::as_array).cloned().unwrap_or_default();

        for &node_id in &joint_nodes {
            let node = &nodes[node_id];
            let mut translation = [
                node.pointer("/translation/0").and_then(Value::as_f64).unwrap_or(0.0) as f32,
                node.pointer("/translation/1").and_then(Value::as_f64).unwrap_or(0.0) as f32,
                node.pointer("/translation/2").and_then(Value::as_f64).unwrap_or(0.0) as f32,
            ];
            let mut rotation = [
                node.pointer("/rotation/0").and_then(Value::as_f64).unwrap_or(0.0) as f32,
                node.pointer("/rotation/1").and_then(Value::as_f64).unwrap_or(0.0) as f32,
                node.pointer("/rotation/2").and_then(Value::as_f64).unwrap_or(0.0) as f32,
                node.pointer("/rotation/3").and_then(Value::as_f64).unwrap_or(1.0) as f32,
            ];
            let scale = [
                node.pointer("/scale/0").and_then(Value::as_f64).unwrap_or(1.0) as f32,
                node.pointer("/scale/1").and_then(Value::as_f64).unwrap_or(1.0) as f32,
                node.pointer("/scale/2").and_then(Value::as_f64).unwrap_or(1.0) as f32,
            ];

            // Sample animated channels targeting this node
            for ch in &channels {
                if ch.pointer("/target/node").and_then(Value::as_u64) == Some(node_id as u64) {
                    let path = ch.pointer("/target/path").and_then(Value::as_str).unwrap_or("");
                    let sampler_idx = ch.get("sampler").and_then(Value::as_u64).unwrap_or(0) as usize;
                    if sampler_idx < samplers.len() {
                        let sampler = &samplers[sampler_idx];

                        let input_acc = sampler.get("input").and_then(Value::as_u64).unwrap_or(0) as usize;
                        let output_acc = sampler.get("output").and_then(Value::as_u64).unwrap_or(0) as usize;

                        let times = self.get_accessor_floats(input_acc)?;
                        let outputs = self.get_accessor_floats(output_acc)?;

                        if path == "rotation" && outputs.len() >= times.len() * 4 {
                            rotation = sample_vec4_slerp(&times, &outputs, sample_time);
                        } else if path == "translation" && outputs.len() >= times.len() * 3 {
                            translation = sample_vec3_lerp(&times, &outputs, sample_time);
                        }
                    }
                }
            }

            local_transforms.insert(node_id, make_affine_matrix(translation, rotation, scale));
        }

        // 2. Build parent map
        let mut parent_map: HashMap<usize, usize> = HashMap::new();
        for (p_id, node) in nodes.iter().enumerate() {
            if let Some(children) = node.get("children").and_then(Value::as_array) {
                for c in children {
                    if let Some(c_id) = c.as_u64() {
                        parent_map.insert(c_id as usize, p_id);
                    }
                }
            }
        }

        // 3. Compute global transforms
        let mut global_transforms: HashMap<usize, [f32; 16]> = HashMap::new();
        for &node_id in &joint_nodes {
            compute_global_matrix(node_id, &local_transforms, &parent_map, &mut global_transforms);
        }

        // 4. Compute skinning matrices: M_j = Global_j * IBM_j
        let mut skin_matrices: Vec<[f32; 16]> = Vec::with_capacity(joint_nodes.len());
        for (j_idx, &node_id) in joint_nodes.iter().enumerate() {
            let glob = global_transforms.get(&node_id).copied().unwrap_or(IDENTITY_MAT4);
            let mut ibm = [0.0f32; 16];
            if j_idx * 16 + 16 <= ibm_floats.len() {
                ibm.copy_from_slice(&ibm_floats[j_idx * 16..j_idx * 16 + 16]);
            } else {
                ibm = IDENTITY_MAT4;
            }
            skin_matrices.push(mat4_mul(glob, ibm));
        }

        // 5. Deform primitive vertices
        let prim = self.json.pointer("/meshes/0/primitives/0").unwrap();
        let pos_acc_idx = prim.pointer("/attributes/POSITION").and_then(Value::as_u64).unwrap() as usize;
        let joints_acc_idx = prim.pointer("/attributes/JOINTS_0").and_then(Value::as_u64).unwrap() as usize;
        let weights_acc_idx = prim.pointer("/attributes/WEIGHTS_0").and_then(Value::as_u64).unwrap() as usize;

        let positions = self.get_accessor_floats(pos_acc_idx)?;
        let joint_indices = self.get_accessor_u16(joints_acc_idx)?;
        let joint_weights = self.get_accessor_floats(weights_acc_idx)?;

        let num_verts = positions.len() / 3;
        let mut deformed = Vec::with_capacity(num_verts);

        for v in 0..num_verts {
            let p = [positions[v * 3], positions[v * 3 + 1], positions[v * 3 + 2]];
            let mut accum = [0.0f32; 3];

            for k in 0..4 {
                let w = joint_weights[v * 4 + k];
                if w > 0.0 {
                    let j = joint_indices[v * 4 + k] as usize;
                    if j < skin_matrices.len() {
                        let transformed = mat4_transform_point(skin_matrices[j], p);
                        accum[0] += w * transformed[0];
                        accum[1] += w * transformed[1];
                        accum[2] += w * transformed[2];
                    }
                }
            }
            deformed.push(accum);
        }

        Ok(deformed)
    }
}

// ---------------------------------------------------------------------------
// Math & Linear Algebra Primitives for Forward Kinematics
// ---------------------------------------------------------------------------

pub const IDENTITY_MAT4: [f32; 16] = [
    1.0, 0.0, 0.0, 0.0,
    0.0, 1.0, 0.0, 0.0,
    0.0, 0.0, 1.0, 0.0,
    0.0, 0.0, 0.0, 1.0,
];

fn parse_glb_json_doc(bytes: &[u8]) -> Option<Value> {
    if bytes.len() < 20 {
        return None;
    }
    let chunk_len = u32::from_le_bytes(bytes[12..16].try_into().ok()?) as usize;
    let chunk_type = u32::from_le_bytes(bytes[16..20].try_into().ok()?);
    if chunk_type != GLTF_JSON_CHUNK || bytes.len() < 20 + chunk_len {
        return None;
    }
    serde_json::from_slice(&bytes[20..20 + chunk_len]).ok()
}

fn make_affine_matrix(t: [f32; 3], q: [f32; 4], s: [f32; 3]) -> [f32; 16] {
    let [x, y, z, w] = q;
    let xx = x * x;
    let yy = y * y;
    let zz = z * z;
    let xy = x * y;
    let xz = x * z;
    let yz = y * z;
    let wx = w * x;
    let wy = w * y;
    let wz = w * z;

    [
        (1.0 - 2.0 * (yy + zz)) * s[0],
        (2.0 * (xy + wz)) * s[0],
        (2.0 * (xz - wy)) * s[0],
        0.0,
        (2.0 * (xy - wz)) * s[1],
        (1.0 - 2.0 * (xx + zz)) * s[1],
        (2.0 * (yz + wx)) * s[1],
        0.0,
        (2.0 * (xz + wy)) * s[2],
        (2.0 * (yz - wx)) * s[2],
        (1.0 - 2.0 * (xx + yy)) * s[2],
        0.0,
        t[0],
        t[1],
        t[2],
        1.0,
    ]
}

fn mat4_mul(a: [f32; 16], b: [f32; 16]) -> [f32; 16] {
    let mut out = [0.0f32; 16];
    for col in 0..4 {
        for row in 0..4 {
            let mut sum = 0.0f32;
            for k in 0..4 {
                sum += a[k * 4 + row] * b[col * 4 + k];
            }
            out[col * 4 + row] = sum;
        }
    }
    out
}

fn mat4_transform_point(m: [f32; 16], p: [f32; 3]) -> [f32; 3] {
    [
        m[0] * p[0] + m[4] * p[1] + m[8] * p[2] + m[12],
        m[1] * p[0] + m[5] * p[1] + m[9] * p[2] + m[13],
        m[2] * p[0] + m[6] * p[1] + m[10] * p[2] + m[14],
    ]
}

fn compute_global_matrix(
    node_id: usize,
    locals: &HashMap<usize, [f32; 16]>,
    parents: &HashMap<usize, usize>,
    globals: &mut HashMap<usize, [f32; 16]>,
) -> [f32; 16] {
    if let Some(&m) = globals.get(&node_id) {
        return m;
    }
    let local = locals.get(&node_id).copied().unwrap_or(IDENTITY_MAT4);
    let global = if let Some(&parent_id) = parents.get(&node_id) {
        let parent_global = compute_global_matrix(parent_id, locals, parents, globals);
        mat4_mul(parent_global, local)
    } else {
        local
    };
    globals.insert(node_id, global);
    global
}

fn sample_vec3_lerp(times: &[f32], values: &[f32], t: f32) -> [f32; 3] {
    if times.is_empty() {
        return [0.0, 0.0, 0.0];
    }
    if t <= times[0] {
        return [values[0], values[1], values[2]];
    }
    if t >= *times.last().unwrap() {
        let n = times.len() - 1;
        return [values[n * 3], values[n * 3 + 1], values[n * 3 + 2]];
    }
    let mut k = 0;
    while k < times.len() - 1 && times[k + 1] < t {
        k += 1;
    }
    let dt = times[k + 1] - times[k];
    let alpha = if dt.abs() > 1e-9 { (t - times[k]) / dt } else { 0.0 };
    [
        values[k * 3] + alpha * (values[(k + 1) * 3] - values[k * 3]),
        values[k * 3 + 1] + alpha * (values[(k + 1) * 3 + 1] - values[k * 3 + 1]),
        values[k * 3 + 2] + alpha * (values[(k + 1) * 3 + 2] - values[k * 3 + 2]),
    ]
}

fn sample_vec4_slerp(times: &[f32], values: &[f32], t: f32) -> [f32; 4] {
    if times.is_empty() {
        return [0.0, 0.0, 0.0, 1.0];
    }
    if t <= times[0] {
        return [values[0], values[1], values[2], values[3]];
    }
    if t >= *times.last().unwrap() {
        let n = times.len() - 1;
        return [values[n * 4], values[n * 4 + 1], values[n * 4 + 2], values[n * 4 + 3]];
    }
    let mut k = 0;
    while k < times.len() - 1 && times[k + 1] < t {
        k += 1;
    }
    let dt = times[k + 1] - times[k];
    let alpha = if dt.abs() > 1e-9 { (t - times[k]) / dt } else { 0.0 };
    let q0 = [values[k * 4], values[k * 4 + 1], values[k * 4 + 2], values[k * 4 + 3]];
    let q1 = [
        values[(k + 1) * 4],
        values[(k + 1) * 4 + 1],
        values[(k + 1) * 4 + 2],
        values[(k + 1) * 4 + 3],
    ];
    quat_slerp(q0, q1, alpha)
}

fn quat_slerp(q0: [f32; 4], mut q1: [f32; 4], t: f32) -> [f32; 4] {
    let mut dot = q0[0] * q1[0] + q0[1] * q1[1] + q0[2] * q1[2] + q0[3] * q1[3];
    if dot < 0.0 {
        q1 = [-q1[0], -q1[1], -q1[2], -q1[3]];
        dot = -dot;
    }
    if dot > 0.9995 {
        let res = [
            q0[0] + t * (q1[0] - q0[0]),
            q0[1] + t * (q1[1] - q0[1]),
            q0[2] + t * (q1[2] - q0[2]),
            q0[3] + t * (q1[3] - q0[3]),
        ];
        let len = (res[0] * res[0] + res[1] * res[1] + res[2] * res[2] + res[3] * res[3]).sqrt();
        if len > 1e-9 {
            return [res[0] / len, res[1] / len, res[2] / len, res[3] / len];
        }
        return q0;
    }
    let theta_0 = dot.clamp(-1.0, 1.0).acos();
    let sin_theta_0 = theta_0.sin();
    if sin_theta_0.abs() < 1e-9 {
        return q0;
    }
    let theta = theta_0 * t;
    let sin_theta = theta.sin();
    let s0 = (theta_0 - theta).sin() / sin_theta_0;
    let s1 = sin_theta / sin_theta_0;
    [
        s0 * q0[0] + s1 * q1[0],
        s0 * q0[1] + s1 * q1[1],
        s0 * q0[2] + s1 * q1[2],
        s0 * q0[3] + s1 * q1[3],
    ]
}

// ---------------------------------------------------------------------------
// Unit & Matrix Tests (Positive + Adversarial for Gates 1–5)
// ---------------------------------------------------------------------------

pub mod synthetic {
    use super::*;

    pub fn create_test_image_file(dir: &Path, name: &str, w: u32, h: u32, pattern: &str) -> std::path::PathBuf {
        let p = dir.join(name);
        let bytes = create_test_image_bytes(w, h, pattern);
        std::fs::write(&p, bytes).unwrap();
        p
    }

    pub fn create_test_image_bytes(width: u32, height: u32, pattern: &str) -> Vec<u8> {
        let mut img = image::RgbaImage::new(width, height);
        match pattern {
            "silhouette" => {
                let (cx, cy) = (width / 2, height / 2);
                let radius = (width.min(height) / 4) as i32;
                for y in 0..height {
                    for x in 0..width {
                        let dx = x as i32 - cx as i32;
                        let dy = y as i32 - cy as i32;
                        if dx * dx + dy * dy < radius * radius {
                            img.put_pixel(x, y, image::Rgba([200, 100, 50, 255]));
                        } else {
                            img.put_pixel(x, y, image::Rgba([0, 0, 0, 0]));
                        }
                    }
                }
            }
            "transparent" => {
                // all zeros (coverage 0.0)
            }
            "flat_opaque" => {
                for p in img.pixels_mut() {
                    *p = image::Rgba([128, 128, 128, 255]);
                }
            }
            "noise" => {
                for (i, p) in img.pixels_mut().enumerate() {
                    let v = ((i * 37 + 17) % 256) as u8;
                    *p = image::Rgba([v, v, v, 255]);
                }
            }
            _ => {
                for p in img.pixels_mut() {
                    *p = image::Rgba([120, 140, 180, 255]);
                }
            }
        }
        let mut bytes = Vec::new();
        let mut cursor = std::io::Cursor::new(&mut bytes);
        img.write_to(&mut cursor, image::ImageFormat::Png).unwrap();
        bytes
    }

    #[derive(Debug, Clone)]
    pub struct SyntheticGlbConfig {
        pub corrupt_magic: bool,
        pub version: u32,
        pub empty_meshes: bool,
        pub triangle_count: usize,
        pub target_mode: u32,
        pub omit_uv0: bool,
        pub duplicate_triangle_indices: bool,
        pub zero_area_triangles: bool,
        pub include_skin: bool,
        pub unnormalized_weights: bool,
        pub mismatch_joints_ibms: bool,
        pub palette_overflow: bool,
        pub include_animations: bool,
        pub include_idle: bool,
        pub include_walk: bool,
        pub non_monotonic_anim: bool,
        pub static_motion: bool,
    }

    impl Default for SyntheticGlbConfig {
        fn default() -> Self {
            Self {
                corrupt_magic: false,
                version: 2,
                empty_meshes: false,
                triangle_count: 32,
                target_mode: 4, // TRIANGLES
                omit_uv0: false,
                duplicate_triangle_indices: false,
                zero_area_triangles: false,
                include_skin: false,
                unnormalized_weights: false,
                mismatch_joints_ibms: false,
                palette_overflow: false,
                include_animations: false,
                include_idle: true,
                include_walk: true,
                non_monotonic_anim: false,
                static_motion: false,
            }
        }
    }

    pub fn build_synthetic_glb(cfg: &SyntheticGlbConfig) -> Vec<u8> {
        if cfg.corrupt_magic {
            return b"BADMAGIC\x00\x00\x00\x00".to_vec();
        }

        let mut bin = Vec::new();
        let mut buffer_views = Vec::new();
        let mut accessors = Vec::new();

        let mut add_view = |data: &[u8], target: Option<u32>| -> usize {
            let offset = bin.len();
            let pad = (4 - (offset % 4)) % 4;
            bin.extend(std::iter::repeat(0).take(pad));
            let start = bin.len();
            bin.extend_from_slice(data);
            let idx = buffer_views.len();
            let mut view = json!({
                "buffer": 0,
                "byteOffset": start,
                "byteLength": data.len(),
            });
            if let Some(t) = target {
                view["target"] = json!(t);
            }
            buffer_views.push(view);
            idx
        };

        if cfg.empty_meshes {
            let gltf_doc = json!({
                "asset": { "version": "2.0", "generator": "RurixForge-Test" },
                "meshes": [],
                "nodes": [{ "name": "Empty" }],
                "buffers": [{ "byteLength": 0 }],
            });
            let mut json_bytes = serde_json::to_vec(&gltf_doc).unwrap();
            let json_pad = (4 - (json_bytes.len() % 4)) % 4;
            json_bytes.extend(std::iter::repeat(b' ').take(json_pad));
            let total_len = 12 + 8 + json_bytes.len();
            let mut out = Vec::with_capacity(total_len);
            out.extend_from_slice(&GLTF_MAGIC.to_le_bytes());
            out.extend_from_slice(&cfg.version.to_le_bytes());
            out.extend_from_slice(&(total_len as u32).to_le_bytes());
            out.extend_from_slice(&(json_bytes.len() as u32).to_le_bytes());
            out.extend_from_slice(&GLTF_JSON_CHUNK.to_le_bytes());
            out.extend_from_slice(&json_bytes);
            return out;
        }

        let mut positions = Vec::new();
        let mut uvs = Vec::new();
        let mut joints = Vec::new();
        let mut weights = Vec::new();
        let mut indices = Vec::new();

        let layers = 8;
        let slices = ((cfg.triangle_count + layers * 2 - 1) / (layers * 2)).max(4);

        for l in 0..=layers {
            let v = l as f32 / layers as f32;
            let y = (v - 0.5) * 2.0;
            let r = 0.25f32;
            for s in 0..slices {
                let u = s as f32 / slices as f32;
                let angle = u * std::f32::consts::TAU;
                let x = if cfg.zero_area_triangles { 0.0 } else { angle.cos() * r };
                let z = if cfg.zero_area_triangles { 0.0 } else { angle.sin() * r };
                positions.extend_from_slice(&[x, y, z]);
                uvs.extend_from_slice(&[u, v]);

                if cfg.include_skin {
                    let j0 = if cfg.palette_overflow { 99u16 } else if y < 0.0 { 2u16 } else { 1u16 };
                    joints.extend_from_slice(&[j0, 0, 0, 0]);
                    if cfg.unnormalized_weights {
                        weights.extend_from_slice(&[0.25f32, 0.25, 0.0, 0.0]); // sum = 0.5 != 1.0
                    } else {
                        weights.extend_from_slice(&[0.85f32, 0.15, 0.0, 0.0]); // sum = 1.0
                    }
                }
            }
        }

        let mut created_triangles = 0;
        'outer: for l in 0..layers {
            for s in 0..slices {
                if created_triangles >= cfg.triangle_count {
                    break 'outer;
                }
                let i0 = (l * slices + s) as u16;
                let i1 = (l * slices + ((s + 1) % slices)) as u16;
                let i2 = ((l + 1) * slices + s) as u16;
                let i3 = ((l + 1) * slices + ((s + 1) % slices)) as u16;

                if cfg.duplicate_triangle_indices && l == 0 && s == 0 {
                    indices.extend_from_slice(&[i0, i0, i0]); // duplicate indices
                } else {
                    indices.extend_from_slice(&[i0, i1, i2]);
                }
                created_triangles += 1;

                if created_triangles < cfg.triangle_count {
                    indices.extend_from_slice(&[i1, i3, i2]);
                    created_triangles += 1;
                }
            }
        }

        let pos_bytes: Vec<u8> = positions.iter().flat_map(|f| f.to_le_bytes()).collect();
        let uv_bytes: Vec<u8> = uvs.iter().flat_map(|f| f.to_le_bytes()).collect();
        let idx_bytes: Vec<u8> = indices.iter().flat_map(|i| i.to_le_bytes()).collect();

        let v_view = add_view(&pos_bytes, Some(34962));
        let uv_view = add_view(&uv_bytes, Some(34962));
        let idx_view = add_view(&idx_bytes, Some(34963));

        let mut min_pos = [f32::MAX, f32::MAX, f32::MAX];
        let mut max_pos = [f32::MIN, f32::MIN, f32::MIN];
        for chunk in positions.chunks_exact(3) {
            for i in 0..3 {
                min_pos[i] = min_pos[i].min(chunk[i]);
                max_pos[i] = max_pos[i].max(chunk[i]);
            }
        }
        if positions.is_empty() {
            min_pos = [0.0, 0.0, 0.0];
            max_pos = [0.0, 0.0, 0.0];
        }

        let acc_pos = accessors.len();
        accessors.push(json!({
            "bufferView": v_view,
            "byteOffset": 0,
            "componentType": 5126,
            "count": positions.len() / 3,
            "type": "VEC3",
            "min": min_pos,
            "max": max_pos,
        }));

        let acc_uv = accessors.len();
        accessors.push(json!({
            "bufferView": uv_view,
            "byteOffset": 0,
            "componentType": 5126,
            "count": uvs.len() / 2,
            "type": "VEC2"
        }));

        let acc_idx = accessors.len();
        accessors.push(json!({
            "bufferView": idx_view,
            "byteOffset": 0,
            "componentType": 5123,
            "count": indices.len(),
            "type": "SCALAR"
        }));

        let mut attributes = json!({ "POSITION": acc_pos });
        if !cfg.omit_uv0 {
            attributes["TEXCOORD_0"] = json!(acc_uv);
        }

        if cfg.include_skin {
            let j_bytes: Vec<u8> = joints.iter().flat_map(|j| j.to_le_bytes()).collect();
            let w_bytes: Vec<u8> = weights.iter().flat_map(|w| w.to_le_bytes()).collect();
            let j_view = add_view(&j_bytes, Some(34962));
            let w_view = add_view(&w_bytes, Some(34962));

            let acc_j = accessors.len();
            accessors.push(json!({
                "bufferView": j_view,
                "byteOffset": 0,
                "componentType": 5123,
                "count": joints.len() / 4,
                "type": "VEC4"
            }));

            let acc_w = accessors.len();
            accessors.push(json!({
                "bufferView": w_view,
                "byteOffset": 0,
                "componentType": 5126,
                "count": weights.len() / 4,
                "type": "VEC4"
            }));

            attributes["JOINTS_0"] = json!(acc_j);
            attributes["WEIGHTS_0"] = json!(acc_w);
        }

        let primitive = json!({
            "attributes": attributes,
            "indices": acc_idx,
            "mode": cfg.target_mode,
        });

        let mut nodes = vec![
            json!({ "name": "MeshNode", "mesh": 0 }),
            json!({ "name": "Hips", "children": [2, 3], "translation": [0.0, 0.0, 0.0] }),
            json!({ "name": "Spine", "translation": [0.0, 0.5, 0.0] }),
            json!({ "name": "Leg", "translation": [0.0, -0.5, 0.0] }),
        ];

        let mut skins = Vec::new();
        if cfg.include_skin {
            nodes[0]["skin"] = json!(0);
            let ibm_count = if cfg.mismatch_joints_ibms { 2 } else { 3 }; // Joints: [1, 2, 3] = 3 joints
            let mut ibm_data = Vec::new();
            for _ in 0..ibm_count {
                ibm_data.extend_from_slice(&IDENTITY_MAT4);
            }
            let ibm_bytes: Vec<u8> = ibm_data.iter().flat_map(|f| f.to_le_bytes()).collect();
            let ibm_view = add_view(&ibm_bytes, None);
            let acc_ibm = accessors.len();
            accessors.push(json!({
                "bufferView": ibm_view,
                "byteOffset": 0,
                "componentType": 5126,
                "count": ibm_count,
                "type": "MAT4"
            }));

            skins.push(json!({
                "joints": [1, 2, 3],
                "inverseBindMatrices": acc_ibm,
            }));
        }

        let mut animations = Vec::new();
        if cfg.include_animations {
            let mut tracks = Vec::new();
            if cfg.include_idle {
                tracks.push(("Idle", 1.5f32, false));
            }
            if cfg.include_walk {
                tracks.push(("Walk", 1.0f32, true));
            }

            for (name, _, is_walk) in tracks {
                let times = if cfg.non_monotonic_anim && is_walk {
                    vec![0.0f32, 0.6, 0.3, 1.0] // Non-monotonic
                } else {
                    vec![0.0f32, 0.25, 0.5, 0.75, 1.0]
                };

                let t_bytes: Vec<u8> = times.iter().flat_map(|t| t.to_le_bytes()).collect();
                let t_view = add_view(&t_bytes, None);
                let acc_time = accessors.len();
                accessors.push(json!({
                    "bufferView": t_view,
                    "byteOffset": 0,
                    "componentType": 5126,
                    "count": times.len(),
                    "type": "SCALAR"
                }));

                let amp = if cfg.static_motion {
                    0.00001f32 // Fake frozen motion
                } else if is_walk {
                    0.5f32 // Significant walk rotation
                } else {
                    0.02f32 // Idle breathing
                };

                let mut rotations = Vec::new();
                for (idx, _) in times.iter().enumerate() {
                    let angle = if is_walk { (idx as f32) * amp } else { 0.0 };
                    let qx = (angle / 2.0).sin();
                    let qw = (angle / 2.0).cos();
                    rotations.extend_from_slice(&[qx, 0.0, 0.0, qw]);
                }

                let r_bytes: Vec<u8> = rotations.iter().flat_map(|f| f.to_le_bytes()).collect();
                let r_view = add_view(&r_bytes, None);
                let acc_rot = accessors.len();
                accessors.push(json!({
                    "bufferView": r_view,
                    "byteOffset": 0,
                    "componentType": 5126,
                    "count": times.len(),
                    "type": "VEC4"
                }));

                animations.push(json!({
                    "name": name,
                    "samplers": [
                        { "input": acc_time, "interpolation": "LINEAR", "output": acc_rot }
                    ],
                    "channels": [
                        { "sampler": 0, "target": { "node": 3, "path": "rotation" } }
                    ]
                }));
            }
        }

        let mut gltf_doc = json!({
            "asset": { "version": "2.0", "generator": "RurixForge-Test" },
            "meshes": [{ "primitives": [primitive] }],
            "nodes": nodes,
            "accessors": accessors,
            "bufferViews": buffer_views,
            "buffers": [{ "byteLength": bin.len() }],
        });

        if !skins.is_empty() {
            gltf_doc["skins"] = json!(skins);
        }
        if !animations.is_empty() {
            gltf_doc["animations"] = json!(animations);
        }

        let pad = (4 - (bin.len() % 4)) % 4;
        bin.extend(std::iter::repeat(0).take(pad));
        gltf_doc["buffers"][0]["byteLength"] = json!(bin.len());

        let mut json_bytes = serde_json::to_vec(&gltf_doc).unwrap();
        let json_pad = (4 - (json_bytes.len() % 4)) % 4;
        json_bytes.extend(std::iter::repeat(b' ').take(json_pad));

        let total_len = 12 + 8 + json_bytes.len() + 8 + bin.len();
        let mut out = Vec::with_capacity(total_len);

        out.extend_from_slice(&GLTF_MAGIC.to_le_bytes());
        out.extend_from_slice(&cfg.version.to_le_bytes());
        out.extend_from_slice(&(total_len as u32).to_le_bytes());

        out.extend_from_slice(&(json_bytes.len() as u32).to_le_bytes());
        out.extend_from_slice(&GLTF_JSON_CHUNK.to_le_bytes());
        out.extend_from_slice(&json_bytes);

        out.extend_from_slice(&(bin.len() as u32).to_le_bytes());
        out.extend_from_slice(&GLTF_BIN_CHUNK.to_le_bytes());
        out.extend_from_slice(&bin);

        out
    }
}
pub use synthetic::{build_synthetic_glb, create_test_image_bytes, create_test_image_file, SyntheticGlbConfig};

#[cfg(test)]
pub mod tests {
    use super::*;
    use std::path::PathBuf;

    fn temp_test_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("rurix-gate-test-{tag}-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    // ========================================================================
    // GATE 1 UNIT TESTS (7 cases)
    // ========================================================================

    #[test]
    fn test_g1_valid_concept_art_png() {
        let dir = temp_test_dir("g1_valid");
        let path = create_test_image_file(&dir, "valid.png", 512, 512, "silhouette");
        let res = validate_gate_1_concept_art(&path);
        assert!(res.is_ok(), "Expected valid concept art: {:?}", res.err());
        let m = res.unwrap();
        assert_eq!(m.width, 512);
        assert_eq!(m.height, 512);
        assert!(m.has_alpha);
        assert!(m.foreground_coverage > 0.01);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn test_g1_minimum_dimension_art() {
        let dir = temp_test_dir("g1_min_dim");
        let path = create_test_image_file(&dir, "min.png", 256, 256, "silhouette");
        let res = validate_gate_1_concept_art(&path);
        assert!(res.is_ok(), "Expected valid 256x256 art: {:?}", res.err());
        let m = res.unwrap();
        assert_eq!(m.width, 256);
        assert_eq!(m.height, 256);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn test_g1_reject_missing_file() {
        let res = validate_gate_1_concept_art(Path::new("missing_concept_art_file.png"));
        assert!(res.is_err());
        assert_eq!(res.unwrap_err().error_code, GATE1_IMAGE_INVALID);
    }

    #[test]
    fn test_g1_reject_zero_byte_file() {
        let dir = temp_test_dir("g1_zero");
        let p = dir.join("zero.png");
        std::fs::write(&p, b"").unwrap();
        let res = validate_gate_1_concept_art(&p);
        assert!(res.is_err());
        assert_eq!(res.unwrap_err().error_code, GATE1_IMAGE_INVALID);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn test_g1_reject_sub_resolution() {
        let bytes = create_test_image_bytes(128, 128, "silhouette");
        let res = validate_gate_1_concept_art_bytes(&bytes);
        assert!(res.is_err());
        let err = res.unwrap_err();
        assert_eq!(err.error_code, GATE1_IMAGE_INVALID);
        assert!(err.message.contains("256x256"));
    }

    #[test]
    fn test_g1_reject_empty_silhouette() {
        let bytes = create_test_image_bytes(512, 512, "transparent");
        let res = validate_gate_1_concept_art_bytes(&bytes);
        assert!(res.is_err());
        let err = res.unwrap_err();
        assert_eq!(err.error_code, GATE1_IMAGE_INVALID);
        assert!(err.message.contains("empty/transparent"));
    }

    #[test]
    fn test_g1_reject_flat_color_canvas() {
        let bytes = create_test_image_bytes(512, 512, "flat_opaque");
        let res = validate_gate_1_concept_art_bytes(&bytes);
        assert!(res.is_err());
        let err = res.unwrap_err();
        assert_eq!(err.error_code, GATE1_IMAGE_INVALID);
        assert!(err.message.contains("flat/solid color"));
    }

    // ========================================================================
    // GATE 2 UNIT TESTS (6 cases)
    // ========================================================================

    #[test]
    fn test_g2_valid_raw_glb() {
        let glb = build_synthetic_glb(&SyntheticGlbConfig::default());
        let res = validate_gate_2_raw_glb_bytes(&glb);
        assert!(res.is_ok(), "Expected valid GLB: {:?}", res.err());
        let m = res.unwrap();
        assert!(m.has_texcoord_0);
        assert_eq!(m.primitive_count, 1);
    }

    #[test]
    fn test_g2_reject_corrupt_magic() {
        let mut cfg = SyntheticGlbConfig::default();
        cfg.corrupt_magic = true;
        let glb = build_synthetic_glb(&cfg);
        let res = validate_gate_2_raw_glb_bytes(&glb);
        assert!(res.is_err());
        assert_eq!(res.unwrap_err().error_code, GATE2_MODEL_INVALID);
    }

    #[test]
    fn test_g2_reject_unsupported_version() {
        let mut cfg = SyntheticGlbConfig::default();
        cfg.version = 1;
        let glb = build_synthetic_glb(&cfg);
        let res = validate_gate_2_raw_glb_bytes(&glb);
        assert!(res.is_err());
        let err = res.unwrap_err();
        assert_eq!(err.error_code, GATE2_MODEL_INVALID);
        assert!(err.message.contains("version"));
    }

    #[test]
    fn test_g2_reject_zero_meshes() {
        let mut cfg = SyntheticGlbConfig::default();
        cfg.empty_meshes = true;
        let glb = build_synthetic_glb(&cfg);
        let res = validate_gate_2_raw_glb_bytes(&glb);
        assert!(res.is_err());
        assert_eq!(res.unwrap_err().error_code, GATE2_MODEL_INVALID);
    }

    #[test]
    fn test_g2_reject_non_triangles_mode() {
        let mut cfg = SyntheticGlbConfig::default();
        cfg.target_mode = 1; // LINES
        let glb = build_synthetic_glb(&cfg);
        let res = validate_gate_2_raw_glb_bytes(&glb);
        assert!(res.is_err());
        let err = res.unwrap_err();
        assert_eq!(err.error_code, GATE2_MODEL_INVALID);
        assert!(err.message.contains("TRIANGLES"));
    }

    #[test]
    fn test_g2_reject_missing_uv0() {
        let mut cfg = SyntheticGlbConfig::default();
        cfg.omit_uv0 = true;
        let glb = build_synthetic_glb(&cfg);
        let res = validate_gate_2_raw_glb_bytes(&glb);
        assert!(res.is_err());
        let err = res.unwrap_err();
        assert_eq!(err.error_code, GATE2_MODEL_INVALID);
        assert!(err.message.contains("TEXCOORD_0"));
    }

    // ========================================================================
    // GATE 3 UNIT TESTS (6 cases)
    // ========================================================================

    #[test]
    fn test_g3_polycount_within_budget() {
        let mut cfg = SyntheticGlbConfig::default();
        cfg.triangle_count = 100;
        let glb = build_synthetic_glb(&cfg);
        let res = validate_gate_3_polycount_bytes(&glb, 500);
        assert!(res.is_ok(), "Expected valid polycount: {:?}", res.err());
        let m = res.unwrap();
        assert_eq!(m.total_triangles, 100);
        assert_eq!(m.target_faces, 500);
        assert_eq!(m.degenerate_triangles, 0);
    }

    #[test]
    fn test_g3_polycount_exact_budget() {
        let mut cfg = SyntheticGlbConfig::default();
        cfg.triangle_count = 200;
        let glb = build_synthetic_glb(&cfg);
        let res = validate_gate_3_polycount_bytes(&glb, 200);
        assert!(res.is_ok());
        let m = res.unwrap();
        assert_eq!(m.total_triangles, 200);
        assert_eq!(m.target_faces, 200);
    }

    #[test]
    fn test_g3_reject_polycount_exceeded() {
        let mut cfg = SyntheticGlbConfig::default();
        cfg.triangle_count = 500;
        let glb = build_synthetic_glb(&cfg);
        let res = validate_gate_3_polycount_bytes(&glb, 100);
        assert!(res.is_err());
        let err = res.unwrap_err();
        assert_eq!(err.error_code, GATE3_POLYCOUNT_EXCEEDED);
    }

    #[test]
    fn test_g3_reject_zero_triangles() {
        let mut cfg = SyntheticGlbConfig::default();
        cfg.triangle_count = 0;
        let glb = build_synthetic_glb(&cfg);
        let res = validate_gate_3_polycount_bytes(&glb, 100);
        assert!(res.is_err());
        assert_eq!(res.unwrap_err().error_code, GATE3_DEGENERATE_MESH);
    }

    #[test]
    fn test_g3_reject_duplicate_indices() {
        let mut cfg = SyntheticGlbConfig::default();
        cfg.duplicate_triangle_indices = true;
        let glb = build_synthetic_glb(&cfg);
        let res = validate_gate_3_polycount_bytes(&glb, 1000);
        assert!(res.is_err());
        let err = res.unwrap_err();
        assert_eq!(err.error_code, GATE3_DEGENERATE_MESH);
        assert!(err.message.contains("degenerate"));
    }

    #[test]
    fn test_g3_reject_collinear_zero_area() {
        let mut cfg = SyntheticGlbConfig::default();
        cfg.zero_area_triangles = true;
        let glb = build_synthetic_glb(&cfg);
        let res = validate_gate_3_polycount_bytes(&glb, 1000);
        assert!(res.is_err());
        let err = res.unwrap_err();
        assert_eq!(err.error_code, GATE3_DEGENERATE_MESH);
        assert!(err.message.contains("degenerate"));
    }

    // ========================================================================
    // GATE 4 UNIT TESTS (5 cases)
    // ========================================================================

    #[test]
    fn test_g4_valid_humanoid_rigging() {
        let mut cfg = SyntheticGlbConfig::default();
        cfg.include_skin = true;
        let glb = build_synthetic_glb(&cfg);
        let res = validate_gate_4_rigging_bytes(&glb);
        assert!(res.is_ok(), "Expected valid rigging: {:?}", res.err());
        let m = res.unwrap();
        assert_eq!(m.joint_count, 3);
        assert!(m.normalized_weights);
        assert!(m.all_joints_within_palette);
    }

    #[test]
    fn test_g4_reject_unskinned_mesh() {
        let mut cfg = SyntheticGlbConfig::default();
        cfg.include_skin = false;
        let glb = build_synthetic_glb(&cfg);
        let res = validate_gate_4_rigging_bytes(&glb);
        assert!(res.is_err());
        let err = res.unwrap_err();
        assert_eq!(err.error_code, GATE4_RIGGING_INVALID);
        assert!(err.message.contains("No skins found"));
    }

    #[test]
    fn test_g4_reject_mismatched_joints_ibms() {
        let mut cfg = SyntheticGlbConfig::default();
        cfg.include_skin = true;
        cfg.mismatch_joints_ibms = true;
        let glb = build_synthetic_glb(&cfg);
        let res = validate_gate_4_rigging_bytes(&glb);
        assert!(res.is_err());
        let err = res.unwrap_err();
        assert_eq!(err.error_code, GATE4_RIGGING_INVALID);
        assert!(err.message.contains("does not match IBM count"));
    }

    #[test]
    fn test_g4_reject_unnormalized_weights() {
        let mut cfg = SyntheticGlbConfig::default();
        cfg.include_skin = true;
        cfg.unnormalized_weights = true;
        let glb = build_synthetic_glb(&cfg);
        let res = validate_gate_4_rigging_bytes(&glb);
        assert!(res.is_err());
        let err = res.unwrap_err();
        assert_eq!(err.error_code, GATE4_RIGGING_INVALID);
        assert!(err.message.contains("must equal 1.0"));
    }

    #[test]
    fn test_g4_reject_palette_overflow() {
        let mut cfg = SyntheticGlbConfig::default();
        cfg.include_skin = true;
        cfg.palette_overflow = true;
        let glb = build_synthetic_glb(&cfg);
        let res = validate_gate_4_rigging_bytes(&glb);
        assert!(res.is_err());
        let err = res.unwrap_err();
        assert_eq!(err.error_code, GATE4_RIGGING_INVALID);
        assert!(err.message.contains("palette"));
    }

    // ========================================================================
    // GATE 5 UNIT TESTS (6 cases)
    // ========================================================================

    #[test]
    fn test_g5_valid_motion_and_publish() {
        let mut cfg = SyntheticGlbConfig::default();
        cfg.include_skin = true;
        cfg.include_animations = true;
        cfg.include_idle = true;
        cfg.include_walk = true;
        let glb = build_synthetic_glb(&cfg);

        let dir = temp_test_dir("g5_pub_ok");
        std::fs::write(dir.join("model.rxmodel"), b"{}").unwrap();
        std::fs::write(
            dir.join("template.rxprefab"),
            serde_json::to_vec(&json!({
                "version": 1,
                "entities": [{
                    "id": 1,
                    "components": {
                        "Animator": { "type": "Animator" },
                        "CharacterController": { "type": "CharacterController" }
                    }
                }]
            })).unwrap(),
        ).unwrap();

        let res = validate_gate_5_motion_and_publish_bytes(&glb, 0.01, Some(&dir));
        assert!(res.is_ok(), "Expected valid motion and publication: {:?}", res.err());
        let m = res.unwrap();
        assert!(m.deformation_verified);
        assert!(m.max_vertex_displacement >= 0.01);
        assert!(m.published_model_path.is_some());
        assert!(m.published_prefab_path.is_some());

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn test_g5_reject_missing_walk_track() {
        let mut cfg = SyntheticGlbConfig::default();
        cfg.include_skin = true;
        cfg.include_animations = true;
        cfg.include_idle = true;
        cfg.include_walk = false;
        let glb = build_synthetic_glb(&cfg);
        let res = validate_gate_5_motion_and_publish_bytes(&glb, 0.01, None);
        assert!(res.is_err());
        let err = res.unwrap_err();
        assert_eq!(err.error_code, GATE5_ANIMATION_INVALID);
        assert!(err.message.contains("Missing required animation tracks"));
    }

    #[test]
    fn test_g5_reject_non_monotonic_times() {
        let mut cfg = SyntheticGlbConfig::default();
        cfg.include_skin = true;
        cfg.include_animations = true;
        cfg.non_monotonic_anim = true;
        let glb = build_synthetic_glb(&cfg);
        let res = validate_gate_5_motion_and_publish_bytes(&glb, 0.01, None);
        assert!(res.is_err());
        let err = res.unwrap_err();
        assert_eq!(err.error_code, GATE5_ANIMATION_INVALID);
        assert!(err.message.contains("non-monotonic"));
    }

    #[test]
    fn test_g5_reject_stationary_deformation() {
        let mut cfg = SyntheticGlbConfig::default();
        cfg.include_skin = true;
        cfg.include_animations = true;
        cfg.static_motion = true;
        let glb = build_synthetic_glb(&cfg);
        let res = validate_gate_5_motion_and_publish_bytes(&glb, 0.01, None);
        assert!(res.is_err());
        let err = res.unwrap_err();
        assert_eq!(err.error_code, GATE5_ANIMATION_INVALID);
        assert!(err.message.contains("below threshold"));
    }

    #[test]
    fn test_g5_reject_missing_published_files() {
        let mut cfg = SyntheticGlbConfig::default();
        cfg.include_skin = true;
        cfg.include_animations = true;
        let glb = build_synthetic_glb(&cfg);

        let dir = temp_test_dir("g5_missing_files");
        // Don't create model.rxmodel or template.rxprefab
        let res = validate_gate_5_motion_and_publish_bytes(&glb, 0.01, Some(&dir));
        assert!(res.is_err());
        assert_eq!(res.unwrap_err().error_code, GATE5_PUBLISH_FAILED);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn test_g5_reject_missing_ecs_components() {
        let mut cfg = SyntheticGlbConfig::default();
        cfg.include_skin = true;
        cfg.include_animations = true;
        let glb = build_synthetic_glb(&cfg);

        let dir = temp_test_dir("g5_missing_ecs");
        std::fs::write(dir.join("model.rxmodel"), b"{}").unwrap();
        std::fs::write(
            dir.join("template.rxprefab"),
            serde_json::to_vec(&json!({
                "version": 1,
                "entities": [{
                    "id": 1,
                    "components": {
                        "ModelRenderer": { "type": "ModelRenderer" }
                        // Missing Animator and CharacterController
                    }
                }]
            })).unwrap(),
        ).unwrap();

        let res = validate_gate_5_motion_and_publish_bytes(&glb, 0.01, Some(&dir));
        assert!(res.is_err());
        let err = res.unwrap_err();
        assert_eq!(err.error_code, GATE5_PUBLISH_FAILED);
        assert!(err.message.contains("Animator"));

        std::fs::remove_dir_all(&dir).ok();
    }
}
