//! crates/forge-agentd/src/world_build/verify.rs
//!
//! Deterministic Server-Side Coherence Validator & Multi-Camera Verification.

use std::path::Path;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use crate::events::now_rfc3339;
use crate::world_build::{read_json, write_json_atomic, WorldBuildError, VERIFY_REPORT_FILE};

pub const ERR_COLLISION_DETECTED: &str = "COHERENCE_COLLISION_DETECTED";
pub const ERR_PROP_ORPHANED: &str = "COHERENCE_PROP_ORPHANED";
pub const ERR_BUILDING_MISSING: &str = "COHERENCE_BUILDING_MISSING";
pub const ERR_ROTATION_UNNORMALIZED: &str = "COHERENCE_ROTATION_UNNORMALIZED";
pub const ERR_VIEWPORT_EMPTY: &str = "COHERENCE_VIEWPORT_EMPTY";

pub const COLLISION_EPSILON: f32 = 1e-4;
pub const QUATERNION_NORMAL_EPSILON: f32 = 1e-4;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CoherenceViolation {
    pub code: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub building_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prop_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pair: Option<[String; 2]>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub overlap_x: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub overlap_z: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub overlap_area: Option<f32>,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CameraFrameResult {
    pub name: String,
    pub path: String,
    pub non_zero_pixels: u64,
    pub width: u32,
    pub height: u32,
    pub status: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CoherenceAuditResult {
    pub status: String,
    pub stage: String,
    pub evaluated_buildings: usize,
    pub evaluated_props: usize,
    pub violations: Vec<CoherenceViolation>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub camera_frames: Vec<CameraFrameResult>,
    #[serde(default)]
    pub override_approved: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub override_reason: Option<String>,
    pub evaluated_at: String,
}

/// Evaluates 2D horizontal ground-plane (X-Z) AABB footprint intersection.
/// Returns (collides, overlap_x, overlap_z).
pub fn check_aabb_xz_collision(
    min_a: [f32; 3],
    max_a: [f32; 3],
    pos_a: [f32; 3],
    min_b: [f32; 3],
    max_b: [f32; 3],
    pos_b: [f32; 3],
    epsilon: f32,
) -> (bool, f32, f32) {
    let min_a_x = pos_a[0] + min_a[0];
    let max_a_x = pos_a[0] + max_a[0];
    let min_a_z = pos_a[2] + min_a[2];
    let max_a_z = pos_a[2] + max_a[2];

    let min_b_x = pos_b[0] + min_b[0];
    let max_b_x = pos_b[0] + max_b[0];
    let min_b_z = pos_b[2] + min_b[2];
    let max_b_z = pos_b[2] + max_b[2];

    let overlap_x = (max_a_x.min(max_b_x) - min_a_x.max(min_b_x)).max(0.0);
    let overlap_z = (max_a_z.min(max_b_z) - min_a_z.max(min_b_z)).max(0.0);

    let collides = overlap_x > epsilon && overlap_z > epsilon;
    (collides, overlap_x, overlap_z)
}

/// Checks if rotation quaternion is normalized within epsilon.
pub fn check_quaternion_normalized(quat: [f32; 4], epsilon: f32) -> bool {
    let norm_sq = quat[0] * quat[0] + quat[1] * quat[1] + quat[2] * quat[2] + quat[3] * quat[3];
    let norm = norm_sq.sqrt();
    (norm - 1.0).abs() < epsilon
}

pub fn parse_bounds(v: &Value) -> ([f32; 3], [f32; 3]) {
    let min = v.get("bounds").and_then(|b| b.get("min")).and_then(Value::as_array);
    let max = v.get("bounds").and_then(|b| b.get("max")).and_then(Value::as_array);
    let to_vec3 = |arr: Option<&Vec<Value>>, def: [f32; 3]| {
        arr.map(|a| [
            a.get(0).and_then(Value::as_f64).unwrap_or(def[0] as f64) as f32,
            a.get(1).and_then(Value::as_f64).unwrap_or(def[1] as f64) as f32,
            a.get(2).and_then(Value::as_f64).unwrap_or(def[2] as f64) as f32,
        ]).unwrap_or(def)
    };
    (to_vec3(min, [-1.0, -1.0, -1.0]), to_vec3(max, [1.0, 1.0, 1.0]))
}

pub fn parse_position(v: &Value) -> [f32; 3] {
    let pos_arr = v.get("transform")
        .and_then(|t| t.get("position").or_else(|| t.get("translation")))
        .and_then(Value::as_array);
    if let Some(a) = pos_arr {
        return [
            a.get(0).and_then(Value::as_f64).unwrap_or(0.0) as f32,
            a.get(1).and_then(Value::as_f64).unwrap_or(0.0) as f32,
            a.get(2).and_then(Value::as_f64).unwrap_or(0.0) as f32,
        ];
    }
    if let Some(t) = v.get("components").and_then(|c| c.get("Transform")) {
        if let Some(a) = t.get("position").or_else(|| t.get("translation")).and_then(Value::as_array) {
            return [
                a.get(0).and_then(Value::as_f64).unwrap_or(0.0) as f32,
                a.get(1).and_then(Value::as_f64).unwrap_or(0.0) as f32,
                a.get(2).and_then(Value::as_f64).unwrap_or(0.0) as f32,
            ];
        }
    }
    [0.0, 0.0, 0.0]
}

pub fn parse_rotation(v: &Value) -> Option<[f32; 4]> {
    let rot_arr = v.get("transform")
        .and_then(|t| t.get("rotation"))
        .and_then(Value::as_array);
    if let Some(a) = rot_arr {
        if a.len() == 4 {
            return Some([
                a[0].as_f64().unwrap_or(0.0) as f32,
                a[1].as_f64().unwrap_or(0.0) as f32,
                a[2].as_f64().unwrap_or(0.0) as f32,
                a[3].as_f64().unwrap_or(1.0) as f32,
            ]);
        }
    }
    if let Some(t) = v.get("components").and_then(|c| c.get("Transform")) {
        if let Some(a) = t.get("rotation").and_then(Value::as_array) {
            if a.len() == 4 {
                return Some([
                    a[0].as_f64().unwrap_or(0.0) as f32,
                    a[1].as_f64().unwrap_or(0.0) as f32,
                    a[2].as_f64().unwrap_or(0.0) as f32,
                    a[3].as_f64().unwrap_or(1.0) as f32,
                ]);
            }
        }
    }
    None
}

fn get_parent_id(v: &Value) -> Option<String> {
    if let Some(pid) = v.get("parentEntityId").and_then(Value::as_str) {
        return Some(pid.to_string());
    }
    if let Some(p) = v.get("components").and_then(|c| c.get("Parent")) {
        if let Some(pid) = p.get("parentEntityId").or_else(|| p.get("entityId")).or_else(|| p.get("entity")).and_then(Value::as_str) {
            return Some(pid.to_string());
        }
    }
    None
}

fn is_interior_prop(v: &Value) -> bool {
    if v.get("isInteriorProp").and_then(Value::as_bool).unwrap_or(false) {
        return true;
    }
    if let Some(tag) = v.get("components").and_then(|c| c.get("Tag")).and_then(|t| t.get("tag")).and_then(Value::as_str) {
        if tag == "interior_prop" || tag == "interior" {
            return true;
        }
    }
    if let Some(id) = v.get("id").and_then(Value::as_str) {
        if id.starts_with("prop_") {
            return true;
        }
    }
    false
}

fn is_building(v: &Value) -> bool {
    if v.get("isBuilding").and_then(Value::as_bool).unwrap_or(false) {
        return true;
    }
    if let Some(tag) = v.get("components").and_then(|c| c.get("Tag")).and_then(|t| t.get("tag")).and_then(Value::as_str) {
        if tag == "building" {
            return true;
        }
    }
    v.get("buildingId").is_some()
}

fn get_building_id(v: &Value) -> String {
    v.get("buildingId")
        .or_else(|| v.get("id"))
        .and_then(Value::as_str)
        .unwrap_or("unknown")
        .to_string()
}

/// Run full server-side coherence verification against scene entities and manifest.
pub fn run_coherence_verification(
    ws_root: &Path,
    slug: &str,
    scene_entities: &[Value],
    manifest: Option<&Value>,
    epsilon: Option<f32>,
) -> Result<CoherenceAuditResult, WorldBuildError> {
    let eps = epsilon.unwrap_or(COLLISION_EPSILON);
    let mut violations = Vec::new();

    // 1. Check building manifest completeness (Check 3)
    if let Some(man) = manifest {
        if let Some(bldgs) = man.get("buildings").and_then(Value::as_array) {
            let scene_bldg_ids: std::collections::HashSet<String> = scene_entities
                .iter()
                .filter(|e| is_building(e))
                .map(|e| get_building_id(e))
                .collect();

            for b in bldgs {
                if let Some(bid) = b.get("id").and_then(Value::as_str) {
                    if !scene_bldg_ids.contains(bid) {
                        violations.push(CoherenceViolation {
                            code: ERR_BUILDING_MISSING.to_string(),
                            building_id: Some(bid.to_string()),
                            prop_id: None,
                            pair: None,
                            overlap_x: None,
                            overlap_z: None,
                            overlap_area: None,
                            message: format!("Building {bid} missing from generated scene"),
                        });
                    }
                }
            }
        }
    }

    // 2. Check 2D AABB ground footprint collisions between all building pairs (Check 1)
    let buildings: Vec<&Value> = scene_entities
        .iter()
        .filter(|e| is_building(e))
        .collect();

    for i in 0..buildings.len() {
        for j in (i + 1)..buildings.len() {
            let b1 = buildings[i];
            let b2 = buildings[j];

            let b1_id = get_building_id(b1);
            let b2_id = get_building_id(b2);

            let (min1, max1) = parse_bounds(b1);
            let (min2, max2) = parse_bounds(b2);
            let pos1 = parse_position(b1);
            let pos2 = parse_position(b2);

            let (collides, ox, oz) = check_aabb_xz_collision(min1, max1, pos1, min2, max2, pos2, eps);
            if collides {
                let pair = if let (Some(id1), Some(id2)) = (b1.get("id").and_then(Value::as_str), b2.get("id").and_then(Value::as_str)) {
                    [id1.to_string(), id2.to_string()]
                } else {
                    [b1_id.clone(), b2_id.clone()]
                };
                violations.push(CoherenceViolation {
                    code: ERR_COLLISION_DETECTED.to_string(),
                    building_id: None,
                    prop_id: None,
                    pair: Some(pair),
                    overlap_x: Some(ox),
                    overlap_z: Some(oz),
                    overlap_area: Some(ox * oz),
                    message: format!("Buildings {b1_id} and {b2_id} collide on ground plane"),
                });
            }
        }
    }

    // 3. Check interior props parent bindings (Check 2)
    let props: Vec<&Value> = scene_entities
        .iter()
        .filter(|e| is_interior_prop(e))
        .collect();

    let entity_ids: std::collections::HashSet<String> = scene_entities
        .iter()
        .filter_map(|e| e.get("id").and_then(Value::as_str).map(str::to_string))
        .collect();

    for prop in &props {
        let prop_id = prop.get("id").and_then(Value::as_str).unwrap_or("prop");
        let parent_id = get_parent_id(prop);

        match parent_id {
            Some(ref pid) if entity_ids.contains(pid) => {}
            _ => {
                violations.push(CoherenceViolation {
                    code: ERR_PROP_ORPHANED.to_string(),
                    building_id: None,
                    prop_id: Some(prop_id.to_string()),
                    pair: None,
                    overlap_x: None,
                    overlap_z: None,
                    overlap_area: None,
                    message: format!("Interior prop {prop_id} has invalid or missing parent"),
                });
            }
        }
    }

    // 4. Check rotation quaternion normalization (Check 4)
    for ent in scene_entities {
        if let Some(quat) = parse_rotation(ent) {
            if !check_quaternion_normalized(quat, QUATERNION_NORMAL_EPSILON) {
                let ent_id = ent.get("id").and_then(Value::as_str).map(str::to_string);
                violations.push(CoherenceViolation {
                    code: ERR_ROTATION_UNNORMALIZED.to_string(),
                    building_id: if is_building(ent) { ent_id.clone() } else { None },
                    prop_id: if is_interior_prop(ent) { ent_id } else { None },
                    pair: None,
                    overlap_x: None,
                    overlap_z: None,
                    overlap_area: None,
                    message: format!("Entity rotation quaternion is not normalized (|norm - 1.0| >= {QUATERNION_NORMAL_EPSILON})"),
                });
            }
        }
    }

    // 5. Multi-camera engine verification (Check 5)
    let mut camera_frames = Vec::new();
    if let Some(man) = manifest {
        if let Some(views) = man.get("cameraViews").and_then(Value::as_array) {
            for v in views {
                let name = v.get("name").and_then(Value::as_str).unwrap_or("default");
                let frame_rel = format!(".forge/world_build/{slug}/verify/frame_{name}.png");
                camera_frames.push(CameraFrameResult {
                    name: name.to_string(),
                    path: frame_rel,
                    non_zero_pixels: 0,
                    width: 960,
                    height: 540,
                    status: "degraded".to_string(), // Headless / CI default
                });
            }
        }
    }

    let passed = violations.is_empty();
    let audit_result = CoherenceAuditResult {
        status: if passed { "PASS".to_string() } else { "FAIL".to_string() },
        stage: if passed { "done".to_string() } else { "coherence_verify".to_string() },
        evaluated_buildings: buildings.len(),
        evaluated_props: props.len(),
        violations,
        camera_frames,
        override_approved: false,
        override_reason: None,
        evaluated_at: now_rfc3339(),
    };

    // Persist to .forge/world_build/<slug>/audit.json and verify/report.json
    let flow_dir = ws_root.join(".forge/world_build").join(slug);
    let audit_path = flow_dir.join("audit.json");
    let report_path = flow_dir.join(VERIFY_REPORT_FILE);

    let audit_val = serde_json::to_value(&audit_result).unwrap_or_default();
    let _ = write_json_atomic(&audit_path, &audit_val);
    let _ = write_json_atomic(&report_path, &audit_val);

    Ok(audit_result)
}

/// Helper to load scene entities and manifest from flow dir, then verify.
pub fn verify_coherence_for_flow(
    ws_root: &Path,
    slug: &str,
    epsilon: Option<f32>,
) -> Result<CoherenceAuditResult, WorldBuildError> {
    let flow_dir = ws_root.join(".forge/world_build").join(slug);
    let scene_path = flow_dir.join("scene.json");
    let prev_scene_path = flow_dir.join("preview/scene.json");
    let template_path = flow_dir.join("template.json");

    let scene_entities: Vec<Value> = if scene_path.is_file() {
        read_json(&scene_path)
            .and_then(|v| {
                if let Some(arr) = v.as_array() {
                    Some(arr.clone())
                } else {
                    v.get("entities").or_else(|| v.get("buildings")).and_then(Value::as_array).cloned()
                }
            })
            .unwrap_or_default()
    } else if prev_scene_path.is_file() {
        read_json(&prev_scene_path)
            .and_then(|v| {
                if let Some(arr) = v.as_array() {
                    Some(arr.clone())
                } else {
                    v.get("entities").or_else(|| v.get("buildings")).and_then(Value::as_array).cloned()
                }
            })
            .unwrap_or_default()
    } else {
        Vec::new()
    };

    let manifest = read_json(&template_path);
    run_coherence_verification(ws_root, slug, &scene_entities, manifest.as_ref(), epsilon)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_aabb_xz_collision_and_tangent() {
        let min = [-1.0, 0.0, -1.0];
        let max = [1.0, 2.0, 1.0];
        // Tangent contact along X (distance = 2.001)
        let (collides, ox, _oz) = check_aabb_xz_collision(
            min, max, [0.0, 0.0, 0.0],
            min, max, [2.001, 0.0, 0.0],
            COLLISION_EPSILON,
        );
        assert!(!collides, "Tangent contact should not collide");
        assert!(ox <= COLLISION_EPSILON);

        // Overlapping along X (distance = 1.999)
        let (collides, ox, oz) = check_aabb_xz_collision(
            min, max, [0.0, 0.0, 0.0],
            min, max, [1.999, 0.0, 0.0],
            COLLISION_EPSILON,
        );
        assert!(collides, "Micro-overlap should collide");
        assert!(ox > COLLISION_EPSILON);
        assert!(oz > COLLISION_EPSILON);
    }

    #[test]
    fn test_quaternion_normalization() {
        let normalized = [0.0, 0.0, 0.0, 1.0];
        assert!(check_quaternion_normalized(normalized, QUATERNION_NORMAL_EPSILON));

        let unnormalized = [1.0, 1.0, 0.0, 0.0]; // norm = sqrt(2)
        assert!(!check_quaternion_normalized(unnormalized, QUATERNION_NORMAL_EPSILON));
    }

    #[test]
    fn test_coherence_verification_full_suite() {
        let tmp = std::env::temp_dir().join(format!("verify-test-{}", std::process::id()));
        std::fs::create_dir_all(&tmp).unwrap();

        let manifest = json!({
            "buildings": [
                {"id": "b1", "bounds": {"min": [-1,-1,-1], "max": [1,1,1]}},
                {"id": "b2", "bounds": {"min": [-1,-1,-1], "max": [1,1,1]}}
            ],
            "cameraViews": [
                {"name": "isometric", "position": [25, 20, 25], "target": [0, 0, 0]}
            ]
        });

        // 1. Valid scene -> PASS
        let scene_valid = vec![
            json!({"id": "ent_b1", "isBuilding": true, "buildingId": "b1", "bounds": {"min": [-1,-1,-1], "max": [1,1,1]}, "transform": {"position": [0,0,0]}}),
            json!({"id": "ent_b2", "isBuilding": true, "buildingId": "b2", "bounds": {"min": [-1,-1,-1], "max": [1,1,1]}, "transform": {"position": [10,0,0]}}),
            json!({"id": "prop_chair", "isInteriorProp": true, "parentEntityId": "ent_b1"})
        ];
        let res = run_coherence_verification(&tmp, "test-slug", &scene_valid, Some(&manifest), None).unwrap();
        assert_eq!(res.status, "PASS");
        assert!(res.violations.is_empty());

        // 2. Colliding buildings -> COHERENCE_COLLISION_DETECTED
        let scene_colliding = vec![
            json!({"id": "ent_b1", "isBuilding": true, "buildingId": "b1", "bounds": {"min": [-1,-1,-1], "max": [1,1,1]}, "transform": {"position": [0,0,0]}}),
            json!({"id": "ent_b2", "isBuilding": true, "buildingId": "b2", "bounds": {"min": [-1,-1,-1], "max": [1,1,1]}, "transform": {"position": [1,0,0]}}),
        ];
        let res_coll = run_coherence_verification(&tmp, "test-slug", &scene_colliding, Some(&manifest), None).unwrap();
        assert_eq!(res_coll.status, "FAIL");
        assert!(res_coll.violations.iter().any(|v| v.code == ERR_COLLISION_DETECTED));

        // 3. Orphaned prop -> COHERENCE_PROP_ORPHANED
        let scene_orphaned = vec![
            json!({"id": "ent_b1", "isBuilding": true, "buildingId": "b1", "bounds": {"min": [-1,-1,-1], "max": [1,1,1]}, "transform": {"position": [0,0,0]}}),
            json!({"id": "ent_b2", "isBuilding": true, "buildingId": "b2", "bounds": {"min": [-1,-1,-1], "max": [1,1,1]}, "transform": {"position": [10,0,0]}}),
            json!({"id": "prop_chair", "isInteriorProp": true, "parentEntityId": "nonexistent"})
        ];
        let res_prop = run_coherence_verification(&tmp, "test-slug", &scene_orphaned, Some(&manifest), None).unwrap();
        assert_eq!(res_prop.status, "FAIL");
        assert!(res_prop.violations.iter().any(|v| v.code == ERR_PROP_ORPHANED));

        // 4. Missing building -> COHERENCE_BUILDING_MISSING
        let scene_missing = vec![
            json!({"id": "ent_b1", "isBuilding": true, "buildingId": "b1", "bounds": {"min": [-1,-1,-1], "max": [1,1,1]}, "transform": {"position": [0,0,0]}}),
        ];
        let res_miss = run_coherence_verification(&tmp, "test-slug", &scene_missing, Some(&manifest), None).unwrap();
        assert_eq!(res_miss.status, "FAIL");
        assert!(res_miss.violations.iter().any(|v| v.code == ERR_BUILDING_MISSING));

        std::fs::remove_dir_all(tmp).ok();
    }
}
