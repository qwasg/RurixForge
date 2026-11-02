//! crates/forge-agentd/src/world_build/survey.rs
//!
//! Survey Facts Extraction Engine for the `world_build` workflow.
//! Performs deterministic, side-effect-free workspace inspection:
//! - forge.toml (engine mode, render backend/method/driver, entry scene)
//! - Active .rxscene (Light, Camera, Environment, renderers, ground bounds)
//! - Content/ materials (.rxmat), models (.rxmodel, .glb, .gltf), textures
//! - Multi-criteria art style inference
//! - Atomic facts persistence to .forge/world_build/<slug>/facts.json

use std::path::{Path, PathBuf};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use crate::events::now_rfc3339;

#[derive(Debug)]
pub enum SurveyError {
    Io(std::io::Error),
    Json(String),
    SceneParse(String),
    InvalidSlug(String),
}

impl std::fmt::Display for SurveyError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SurveyError::Io(e) => write!(f, "I/O error during survey: {e}"),
            SurveyError::Json(e) => write!(f, "JSON serialization error: {e}"),
            SurveyError::SceneParse(e) => write!(f, "Failed to parse scene: {e}"),
            SurveyError::InvalidSlug(e) => write!(f, "Invalid slug for persistence: {e}"),
        }
    }
}

impl std::error::Error for SurveyError {}

/// Comprehensive survey facts extracted from the workspace.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorldBuildFacts {
    pub has_project: bool,
    pub project_name: String,
    pub game_mode: String,                  // "3d" | "2d"
    pub render_backend: String,             // "rurix" | "godot"
    pub render_method: Option<String>,      // "forward_plus" | "mobile" | "gl_compatibility"
    pub render_driver: Option<String>,      // "d3d12" | "vulkan" | "opengl3"
    pub active_scene_path: Option<String>,  // e.g. "Content/Scenes/Main.rxscene"
    pub scene_summary: SceneFacts,
    pub lighting: LightingFacts,
    pub camera: Option<CameraFact>,
    pub environment: EnvironmentFacts,
    pub existing_models: Vec<ModelAssetFact>,
    pub existing_materials: Vec<MaterialAssetFact>,
    pub detected_art_style: ArtStyleFact,
    pub scan_warnings: Vec<String>,
    pub surveyed_at: String,                // RFC3339 timestamp
}

/// Structural overview of the active scene.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SceneFacts {
    pub name: String,
    pub mode: String,
    pub gravity: [f32; 3],
    pub entity_count: usize,
    pub light_count: usize,
    pub camera_count: usize,
    pub mesh_renderer_count: usize,
    pub model_renderer_count: usize,
    pub ground_bounds: Option<GroundBoundsFact>,
}

/// Inferred ground plane / terrain boundary on X-Z.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GroundBoundsFact {
    pub center: [f32; 3],
    pub half_extents: [f32; 3],
}

/// Consolidated lighting metrics from all scene lights.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LightingFacts {
    pub lights: Vec<LightFact>,
    pub ambient_light: [f32; 3],
    pub dominant_direction: Option<[f32; 3]>,
    pub has_directional_light: bool,
}

/// An individual Light component on an entity.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LightFact {
    pub entity_id: u64,
    pub entity_name: String,
    pub kind: String,                      // "directional" | "point" | "spot"
    pub color: [f32; 3],                   // Linear RGB
    pub intensity: f32,
    pub cast_shadow: bool,
    pub position: [f32; 3],
    pub direction: Option<[f32; 3]>,
}

/// Primary camera perspective parameters.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CameraFact {
    pub entity_id: u64,
    pub entity_name: String,
    pub projection: String,                // "perspective" | "orthographic"
    pub fov: f32,
    pub near: f32,
    pub far: f32,
    pub ortho_size: Option<f32>,
    pub position: [f32; 3],
    pub target: [f32; 3],
}

/// Environment, skybox, ambient, and fog parameters.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EnvironmentFacts {
    pub background: String,                // "clearColor" | "color" | "sky"
    pub background_color: [f32; 4],
    pub ambient_source: String,            // "bg" | "disabled" | "color" | "sky"
    pub ambient_color: [f32; 4],
    pub ambient_energy: f32,
    pub fog_enabled: bool,
    pub fog_mode: Option<String>,          // "exponential" | "depth"
    pub fog_color: Option<[f32; 4]>,
    pub fog_density: Option<f32>,
    pub tonemap: Option<String>,
    pub exposure: Option<f32>,
}

/// Discovered 3D model asset in Content/.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelAssetFact {
    pub path: String,                      // Relative to Content/
    pub guid: Option<String>,
    pub format: String,                    // "rxmodel" | "glb" | "gltf"
    pub name: String,
    pub kind: Option<String>,              // "prop" | "building" | "character"
    pub polycount: Option<usize>,
    pub has_prefab: bool,
}

/// Discovered material asset in Content/.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MaterialAssetFact {
    pub path: String,                      // Relative to Content/
    pub guid: Option<String>,
    pub name: String,
    pub shader: String,
    pub base_color: Option<[f32; 4]>,
    pub roughness: Option<f32>,
    pub metallic: Option<f32>,
    pub emissive: Option<[f32; 3]>,
    pub texture_slots: Vec<String>,
}

/// Inferred artistic style, color palette, and mood.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ArtStyleFact {
    pub primary_style: String,             // e.g. "cyberpunk"
    pub confidence: f32,                   // 0.0 - 1.0
    pub palette: Vec<String>,              // Hex color strings, e.g. ["#0f172a", "#38bdf8", "#f43f5e"]
    pub lighting_mood: String,             // e.g. "night_neon" | "golden_hour"
    pub recommended_presets: Vec<String>,
    pub reasoning: String,
}

/// Main entry point for surveying a project workspace.
pub fn survey_workspace(
    _workspace_root: &Path,
    project_root: &Path,
    user_prompt: &str,
) -> Result<WorldBuildFacts, SurveyError> {
    let now = now_rfc3339();
    let mut warnings = Vec::new();

    // 1. Scan forge.toml
    let manifest_path = project_root.join("forge.toml");
    let has_project = manifest_path.is_file();
    let (project, render_cfg) = if has_project {
        match assetd::project::ForgeProject::load(project_root) {
            Ok(p) => {
                let r = p.render;
                (p, r)
            }
            Err(e) => {
                warnings.push(format!("Failed to parse forge.toml: {e}"));
                (
                    assetd::project::ForgeProject::with_defaults(project_root.to_path_buf()),
                    assetd::project::RenderConfig::default(),
                )
            }
        }
    } else {
        (
            assetd::project::ForgeProject::with_defaults(project_root.to_path_buf()),
            assetd::project::RenderConfig::default(),
        )
    };

    // Apply environment overrides for render backend if present
    let (backend_str, method_str, driver_str) = {
        let eb = std::env::var("FORGE_RENDER_BACKEND")
            .ok()
            .filter(|s| !s.trim().is_empty());
        let em = std::env::var("FORGE_RENDER_METHOD")
            .ok()
            .filter(|s| !s.trim().is_empty());
        let ed = std::env::var("FORGE_RENDER_DRIVER")
            .ok()
            .filter(|s| !s.trim().is_empty());
        let (b, m, d) = render_cfg.as_strs();
        (
            eb.unwrap_or_else(|| b.to_string()),
            em.or_else(|| m.map(|s| s.to_string())),
            ed.or_else(|| d.map(|s| s.to_string())),
        )
    };

    // 2. Resolve and parse active .rxscene
    let scene_rel = project.entry_scene.clone();
    let scene_abs = project_root.join(&scene_rel);
    let (active_scene_path, scene_opt) = if scene_abs.is_file() {
        match std::fs::read_to_string(&scene_abs) {
            Ok(content) => match forge_scene::Scene::from_json(&content) {
                Ok(scene) => (Some(scene_rel), Some(scene)),
                Err(e) => {
                    warnings.push(format!("Failed to parse {scene_rel}: {e}"));
                    (Some(scene_rel), None)
                }
            },
            Err(e) => {
                warnings.push(format!("Failed to read {scene_rel}: {e}"));
                (None, None)
            }
        }
    } else {
        let fallback = find_first_scene(project_root);
        let scene = fallback.as_ref().and_then(|rel| {
            std::fs::read_to_string(project_root.join(rel))
                .ok()
                .and_then(|c| forge_scene::Scene::from_json(&c).ok())
        });
        (fallback, scene)
    };

    // 3. Extract Scene, Lighting, Camera, Environment
    let (scene_summary, lighting, camera, environment) = match scene_opt {
        Some(scene) => extract_scene_components(&scene),
        None => default_empty_components(),
    };

    // 4. Scan Content/ for materials, models
    let existing_materials = scan_materials(project_root, &mut warnings);
    let existing_models = scan_models(project_root, &mut warnings);

    // 5. Infer Art Style
    let detected_art_style = infer_art_style(
        user_prompt,
        &existing_materials,
        &lighting,
        &environment,
        &existing_models,
    );

    Ok(WorldBuildFacts {
        has_project,
        project_name: project.name,
        game_mode: project.mode.as_str().to_string(),
        render_backend: backend_str,
        render_method: method_str,
        render_driver: driver_str,
        active_scene_path,
        scene_summary,
        lighting,
        camera,
        environment,
        existing_models,
        existing_materials,
        detected_art_style,
        scan_warnings: warnings,
        surveyed_at: now,
    })
}

/// Persists WorldBuildFacts atomically into `.forge/world_build/<slug>/facts.json`.
pub fn persist_facts(
    workspace_root: &Path,
    slug: &str,
    facts: &WorldBuildFacts,
) -> Result<PathBuf, SurveyError> {
    if slug.is_empty()
        || slug.contains("..")
        || slug.contains('/')
        || slug.contains('\\')
        || slug.contains(':')
    {
        return Err(SurveyError::InvalidSlug(format!("Invalid slug: {slug:?}")));
    }

    let target_dir = workspace_root.join(".forge").join("world_build").join(slug);
    std::fs::create_dir_all(&target_dir).map_err(SurveyError::Io)?;

    let target_file = target_dir.join("facts.json");
    let tmp_file = target_dir.join("facts.json.tmp");

    let json_bytes = serde_json::to_vec_pretty(facts)
        .map_err(|e| SurveyError::Json(e.to_string()))?;

    std::fs::write(&tmp_file, &json_bytes).map_err(SurveyError::Io)?;
    std::fs::rename(&tmp_file, &target_file).map_err(SurveyError::Io)?;

    Ok(target_file)
}

/// Find first available .rxscene under Content/Scenes/
fn find_first_scene(project_root: &Path) -> Option<String> {
    let scenes_dir = project_root.join("Content").join("Scenes");
    if let Ok(entries) = std::fs::read_dir(scenes_dir) {
        for entry in entries.flatten() {
            let p = entry.path();
            if p.is_file() && p.extension().map_or(false, |ext| ext == "rxscene") {
                if let Ok(rel) = p.strip_prefix(project_root) {
                    return Some(rel.to_string_lossy().replace('\\', "/"));
                }
            }
        }
    }
    None
}

/// Component extractor iterating over forge_scene::Scene entities
fn extract_scene_components(
    scene: &forge_scene::Scene,
) -> (SceneFacts, LightingFacts, Option<CameraFact>, EnvironmentFacts) {
    let mut lights = Vec::new();
    let mut camera = None;
    let mut env = None;
    let mut mesh_renderer_count = 0;
    let mut model_renderer_count = 0;
    let mut ground_bounds = None;

    for entity in &scene.entities {
        let trans = entity.transform;

        // Check Light
        if let Some(comp) = entity.component("Light") {
            if comp.enabled {
                let kind = comp
                    .props
                    .get("kind")
                    .and_then(|v| v.as_str())
                    .unwrap_or("point")
                    .to_string();
                let color = comp
                    .props
                    .get("color")
                    .and_then(|v| v.as_array())
                    .and_then(|arr| {
                        if arr.len() >= 3 {
                            Some([
                                arr[0].as_f64()? as f32,
                                arr[1].as_f64()? as f32,
                                arr[2].as_f64()? as f32,
                            ])
                        } else {
                            None
                        }
                    })
                    .unwrap_or([1.0, 1.0, 1.0]);
                let intensity = comp
                    .props
                    .get("intensity")
                    .and_then(|v| v.as_f64())
                    .unwrap_or(1.0) as f32;
                let cast_shadow = comp
                    .props
                    .get("castShadow")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);

                let dir = quat_forward(trans.rotation);

                lights.push(LightFact {
                    entity_id: entity.id,
                    entity_name: entity.name.clone(),
                    kind,
                    color,
                    intensity,
                    cast_shadow,
                    position: trans.translation,
                    direction: Some(dir),
                });
            }
        }

        // Check Camera (pick first enabled)
        if camera.is_none() {
            if let Some(comp) = entity.component("Camera") {
                if comp.enabled {
                    let projection = comp
                        .props
                        .get("projection")
                        .and_then(|v| v.as_str())
                        .unwrap_or("perspective")
                        .to_string();
                    let fov = comp
                        .props
                        .get("fov")
                        .and_then(|v| v.as_f64())
                        .unwrap_or(60.0) as f32;
                    let near = comp
                        .props
                        .get("near")
                        .and_then(|v| v.as_f64())
                        .unwrap_or(0.1) as f32;
                    let far = comp
                        .props
                        .get("far")
                        .and_then(|v| v.as_f64())
                        .unwrap_or(500.0) as f32;
                    let ortho_size = comp
                        .props
                        .get("orthoSize")
                        .and_then(|v| v.as_f64())
                        .map(|v| v as f32);

                    let dir = quat_forward(trans.rotation);
                    let target = [
                        trans.translation[0] + dir[0] * 10.0,
                        trans.translation[1] + dir[1] * 10.0,
                        trans.translation[2] + dir[2] * 10.0,
                    ];

                    camera = Some(CameraFact {
                        entity_id: entity.id,
                        entity_name: entity.name.clone(),
                        projection,
                        fov,
                        near,
                        far,
                        ortho_size,
                        position: trans.translation,
                        target,
                    });
                }
            }
        }

        // Check Environment (pick first enabled)
        if env.is_none() {
            if let Some(comp) = entity.component("Environment") {
                if comp.enabled {
                    let background = comp
                        .props
                        .get("background")
                        .and_then(|v| v.as_str())
                        .unwrap_or("clearColor")
                        .to_string();
                    let background_color = parse_f32_4(&comp.props, "backgroundColor")
                        .unwrap_or([0.0, 0.0, 0.0, 1.0]);
                    let ambient_source = comp
                        .props
                        .get("ambientSource")
                        .and_then(|v| v.as_str())
                        .unwrap_or("bg")
                        .to_string();
                    let ambient_color =
                        parse_f32_4(&comp.props, "ambientColor").unwrap_or([0.0, 0.0, 0.0, 1.0]);
                    let ambient_energy = comp
                        .props
                        .get("ambientEnergy")
                        .and_then(|v| v.as_f64())
                        .unwrap_or(1.0) as f32;
                    let fog_enabled = comp
                        .props
                        .get("fogEnabled")
                        .and_then(|v| v.as_bool())
                        .unwrap_or(false);
                    let fog_mode = comp
                        .props
                        .get("fogMode")
                        .and_then(|v| v.as_str())
                        .map(str::to_string);
                    let fog_color = parse_f32_4(&comp.props, "fogLightColor");
                    let fog_density = comp
                        .props
                        .get("fogDensity")
                        .and_then(|v| v.as_f64())
                        .map(|v| v as f32);
                    let tonemap = comp
                        .props
                        .get("tonemap")
                        .and_then(|v| v.as_str())
                        .map(str::to_string);
                    let exposure = comp
                        .props
                        .get("exposure")
                        .and_then(|v| v.as_f64())
                        .map(|v| v as f32);

                    env = Some(EnvironmentFacts {
                        background,
                        background_color,
                        ambient_source,
                        ambient_color,
                        ambient_energy,
                        fog_enabled,
                        fog_mode,
                        fog_color,
                        fog_density,
                        tonemap,
                        exposure,
                    });
                }
            }
        }

        // Count renderers & detect ground bounds
        if entity
            .component("MeshRenderer")
            .map_or(false, |c| c.enabled)
        {
            mesh_renderer_count += 1;
        }
        if entity
            .component("ModelRenderer")
            .map_or(false, |c| c.enabled)
        {
            model_renderer_count += 1;
        }
        let lower_name = entity.name.to_ascii_lowercase();
        if lower_name.contains("ground")
            || lower_name.contains("floor")
            || lower_name.contains("terrain")
        {
            ground_bounds = Some(GroundBoundsFact {
                center: trans.translation,
                half_extents: [
                    (trans.scale[0] * 0.5).abs(),
                    (trans.scale[1] * 0.5).abs(),
                    (trans.scale[2] * 0.5).abs(),
                ],
            });
        }
    }

    let has_directional = lights.iter().any(|l| l.kind == "directional");
    let dominant_dir = lights
        .iter()
        .filter(|l| l.kind == "directional")
        .max_by(|a, b| {
            a.intensity
                .partial_cmp(&b.intensity)
                .unwrap_or(std::cmp::Ordering::Equal)
        })
        .and_then(|l| l.direction);

    let ambient_light = if let Some(e) = &env {
        [
            e.ambient_color[0] * e.ambient_energy,
            e.ambient_color[1] * e.ambient_energy,
            e.ambient_color[2] * e.ambient_energy,
        ]
    } else {
        [0.1, 0.1, 0.1]
    };

    let lighting_facts = LightingFacts {
        lights,
        ambient_light,
        dominant_direction: dominant_dir,
        has_directional_light: has_directional,
    };

    let scene_facts = SceneFacts {
        name: scene.name.clone(),
        mode: scene.mode.clone(),
        gravity: scene.gravity,
        entity_count: scene.entities.len(),
        light_count: lighting_facts.lights.len(),
        camera_count: if camera.is_some() { 1 } else { 0 },
        mesh_renderer_count,
        model_renderer_count,
        ground_bounds,
    };

    let environment_facts = env.unwrap_or_else(default_environment);

    (scene_facts, lighting_facts, camera, environment_facts)
}

/// Computes forward vector from a quaternion [x, y, z, w]
fn quat_forward(q: [f32; 4]) -> [f32; 3] {
    let (x, y, z, w) = (q[0], q[1], q[2], q[3]);
    [
        -2.0 * (x * z + w * y),
        -2.0 * (y * z - w * x),
        -(1.0 - 2.0 * (x * x + y * y)),
    ]
}

fn parse_f32_4(val: &Value, key: &str) -> Option<[f32; 4]> {
    let arr = val.get(key)?.as_array()?;
    if arr.len() >= 4 {
        Some([
            arr[0].as_f64()? as f32,
            arr[1].as_f64()? as f32,
            arr[2].as_f64()? as f32,
            arr[3].as_f64()? as f32,
        ])
    } else {
        None
    }
}

fn default_environment() -> EnvironmentFacts {
    EnvironmentFacts {
        background: "clearColor".into(),
        background_color: [0.0, 0.0, 0.0, 1.0],
        ambient_source: "bg".into(),
        ambient_color: [0.1, 0.1, 0.15, 1.0],
        ambient_energy: 1.0,
        fog_enabled: false,
        fog_mode: None,
        fog_color: None,
        fog_density: None,
        tonemap: Some("linear".into()),
        exposure: Some(1.0),
    }
}

fn default_empty_components() -> (
    SceneFacts,
    LightingFacts,
    Option<CameraFact>,
    EnvironmentFacts,
) {
    (
        SceneFacts {
            name: "Default".into(),
            mode: "3d".into(),
            gravity: [0.0, -9.81, 0.0],
            entity_count: 0,
            light_count: 0,
            camera_count: 0,
            mesh_renderer_count: 0,
            model_renderer_count: 0,
            ground_bounds: None,
        },
        LightingFacts {
            lights: Vec::new(),
            ambient_light: [0.1, 0.1, 0.1],
            dominant_direction: None,
            has_directional_light: false,
        },
        None,
        default_environment(),
    )
}

/// Recursively scan Content/ for .rxmat files
pub fn scan_materials(project_root: &Path, warnings: &mut Vec<String>) -> Vec<MaterialAssetFact> {
    let content_dir = project_root.join("Content");
    if !content_dir.is_dir() {
        return Vec::new();
    }
    let mut materials = Vec::new();
    let mut dirs = vec![content_dir.clone()];

    while let Some(dir) = dirs.pop() {
        let entries = match std::fs::read_dir(&dir) {
            Ok(e) => e,
            Err(e) => {
                warnings.push(format!("Failed to read dir {:?}: {e}", dir));
                continue;
            }
        };
        for entry in entries.flatten() {
            let p = entry.path();
            if p.is_dir() {
                dirs.push(p);
            } else if p.is_file() && p.extension().map_or(false, |ext| ext == "rxmat") {
                let rel = p
                    .strip_prefix(&content_dir)
                    .unwrap_or(&p)
                    .to_string_lossy()
                    .replace('\\', "/");
                let name = p
                    .file_stem()
                    .map(|s| s.to_string_lossy().to_string())
                    .unwrap_or_else(|| rel.clone());

                let mut guid = None;
                let meta_path = p.with_file_name(format!("{}.meta", p.file_name().unwrap().to_string_lossy()));
                if let Ok(meta_text) = std::fs::read_to_string(&meta_path) {
                    if let Ok(val) = serde_json::from_str::<Value>(&meta_text) {
                        guid = val.get("guid").and_then(|g| g.as_str()).map(str::to_string);
                    }
                }

                let mut shader = "pbr-default".to_string();
                let mut base_color = None;
                let mut roughness = None;
                let mut metallic = None;
                let mut emissive = None;
                let mut texture_slots = Vec::new();

                if let Ok(text) = std::fs::read_to_string(&p) {
                    if let Ok(val) = serde_json::from_str::<Value>(&text) {
                        if let Some(s) = val.get("shader").and_then(|s| s.as_str()) {
                            shader = s.to_string();
                        }
                        if let Some(g) = val.get("guid").and_then(|g| g.as_str()) {
                            if guid.is_none() {
                                guid = Some(g.to_string());
                            }
                        }
                        if let Some(params) = val.get("params").and_then(|p| p.as_object()) {
                            if let Some(bc) = params.get("baseColor").or_else(|| params.get("albedo")) {
                                if let Some(arr) = bc.as_array() {
                                    if arr.len() >= 4 {
                                        base_color = Some([
                                            arr[0].as_f64().unwrap_or(1.0) as f32,
                                            arr[1].as_f64().unwrap_or(1.0) as f32,
                                            arr[2].as_f64().unwrap_or(1.0) as f32,
                                            arr[3].as_f64().unwrap_or(1.0) as f32,
                                        ]);
                                    }
                                }
                            }
                            if let Some(r) = params.get("roughness").and_then(|r| r.as_f64()) {
                                roughness = Some(r as f32);
                            }
                            if let Some(m) = params.get("metallic").and_then(|m| m.as_f64()) {
                                metallic = Some(m as f32);
                            }
                            if let Some(em) = params.get("emissive").or_else(|| params.get("emission")) {
                                if let Some(arr) = em.as_array() {
                                    if arr.len() >= 3 {
                                        emissive = Some([
                                            arr[0].as_f64().unwrap_or(0.0) as f32,
                                            arr[1].as_f64().unwrap_or(0.0) as f32,
                                            arr[2].as_f64().unwrap_or(0.0) as f32,
                                        ]);
                                    }
                                }
                            }
                            for key in params.keys() {
                                if key.to_ascii_lowercase().contains("texture") || key.to_ascii_lowercase().contains("map") {
                                    texture_slots.push(key.clone());
                                }
                            }
                        }
                        if let Some(textures) = val.get("textures").and_then(|t| t.as_object()) {
                            for k in textures.keys() {
                                if !texture_slots.contains(k) {
                                    texture_slots.push(k.clone());
                                }
                            }
                        }
                    }
                }

                materials.push(MaterialAssetFact {
                    path: rel,
                    guid,
                    name,
                    shader,
                    base_color,
                    roughness,
                    metallic,
                    emissive,
                    texture_slots,
                });
            }
        }
    }
    materials.sort_by(|a, b| a.path.cmp(&b.path));
    materials
}

/// Recursively scan Content/ for 3D model files (.rxmodel, .glb, .gltf)
pub fn scan_models(project_root: &Path, warnings: &mut Vec<String>) -> Vec<ModelAssetFact> {
    let content_dir = project_root.join("Content");
    if !content_dir.is_dir() {
        return Vec::new();
    }
    let mut models = Vec::new();
    let mut dirs = vec![content_dir.clone()];

    while let Some(dir) = dirs.pop() {
        let entries = match std::fs::read_dir(&dir) {
            Ok(e) => e,
            Err(e) => {
                warnings.push(format!("Failed to read dir {:?}: {e}", dir));
                continue;
            }
        };
        for entry in entries.flatten() {
            let p = entry.path();
            if p.is_dir() {
                dirs.push(p);
            } else if p.is_file() {
                let ext = p.extension().and_then(|s| s.to_str()).unwrap_or("").to_ascii_lowercase();
                if matches!(ext.as_str(), "rxmodel" | "glb" | "gltf") {
                    let rel = p
                        .strip_prefix(&content_dir)
                        .unwrap_or(&p)
                        .to_string_lossy()
                        .replace('\\', "/");
                    let name = p
                        .file_stem()
                        .map(|s| s.to_string_lossy().to_string())
                        .unwrap_or_else(|| rel.clone());

                    let mut guid = None;
                    let mut kind = None;
                    let mut polycount = None;
                    let has_prefab = p.with_file_name("template.rxprefab").is_file()
                        || p.with_extension("rxprefab").is_file();

                    if ext == "rxmodel" {
                        if let Ok(text) = std::fs::read_to_string(&p) {
                            if let Ok(val) = serde_json::from_str::<Value>(&text) {
                                guid = val.get("guid").and_then(|g| g.as_str()).map(str::to_string);
                                kind = val.get("kind").and_then(|k| k.as_str()).map(str::to_string);
                                polycount = val.get("polycount")
                                    .or_else(|| val.get("triangleCount"))
                                    .and_then(|c| c.as_u64())
                                    .map(|c| c as usize);
                            }
                        }
                    }

                    models.push(ModelAssetFact {
                        path: rel,
                        guid,
                        format: ext,
                        name,
                        kind,
                        polycount,
                        has_prefab,
                    });
                }
            }
        }
    }
    models.sort_by(|a, b| a.path.cmp(&b.path));
    models
}

/// Deterministic multi-criteria art style inference
pub fn infer_art_style(
    user_prompt: &str,
    materials: &[MaterialAssetFact],
    _lighting: &LightingFacts,
    environment: &EnvironmentFacts,
    _models: &[ModelAssetFact],
) -> ArtStyleFact {
    let prompt_lower = user_prompt.to_ascii_lowercase();

    // 1. Keyword detection in user prompt
    let has_cyber = prompt_lower.contains("cyber")
        || prompt_lower.contains("neon")
        || prompt_lower.contains("scifi")
        || prompt_lower.contains("future")
        || prompt_lower.contains("harbor")
        || prompt_lower.contains("dock");
    let has_medieval = prompt_lower.contains("medieval")
        || prompt_lower.contains("fantasy")
        || prompt_lower.contains("castle")
        || prompt_lower.contains("dungeon")
        || prompt_lower.contains("village");
    let has_lowpoly = prompt_lower.contains("lowpoly")
        || prompt_lower.contains("low-poly")
        || prompt_lower.contains("cartoon")
        || prompt_lower.contains("stylized");

    // 2. Material PBR parameter signals
    let mut emissive_count = 0;
    let mut high_roughness_count = 0;
    let mut high_metallic_count = 0;

    for mat in materials {
        if let Some(em) = mat.emissive {
            if em[0] > 0.1 || em[1] > 0.1 || em[2] > 0.1 {
                emissive_count += 1;
            }
        }
        if let Some(r) = mat.roughness {
            if r > 0.7 {
                high_roughness_count += 1;
            }
        }
        if let Some(m) = mat.metallic {
            if m > 0.5 {
                high_metallic_count += 1;
            }
        }
    }

    // 3. Lighting signals
    let is_night_or_fog = environment.fog_enabled || environment.ambient_energy < 0.5;

    // 4. Decision matrix
    if has_cyber || (emissive_count >= 2 && is_night_or_fog) {
        ArtStyleFact {
            primary_style: "cyberpunk".into(),
            confidence: if has_cyber { 0.95 } else { 0.75 },
            palette: vec![
                "#0a0f1d".into(),
                "#00f0ff".into(),
                "#ff0055".into(),
                "#ffe600".into(),
            ],
            lighting_mood: "night_neon".into(),
            recommended_presets: vec!["cyberpunk_neon".into(), "dark_scifi".into()],
            reasoning: "Prompt specifies cyberpunk/neon themes; scene features emissive lighting cues and high-contrast ambient settings.".into(),
        }
    } else if has_medieval || (high_roughness_count > high_metallic_count && !is_night_or_fog) {
        ArtStyleFact {
            primary_style: "medieval_fantasy".into(),
            confidence: if has_medieval { 0.90 } else { 0.65 },
            palette: vec![
                "#3b2f2f".into(),
                "#8b7355".into(),
                "#556b2f".into(),
                "#d4af37".into(),
            ],
            lighting_mood: "golden_hour".into(),
            recommended_presets: vec!["medieval_village".into(), "fantasy_keep".into()],
            reasoning: "Medieval/fantasy motifs identified with natural stone/wood material textures and warm sun direction.".into(),
        }
    } else if has_lowpoly {
        ArtStyleFact {
            primary_style: "stylized_lowpoly".into(),
            confidence: 0.90,
            palette: vec![
                "#60a5fa".into(),
                "#34d399".into(),
                "#f87171".into(),
                "#fbbf24".into(),
            ],
            lighting_mood: "bright_day".into(),
            recommended_presets: vec!["lowpoly_toon".into(), "vibrant_clean".into()],
            reasoning: "User requested stylized/low-poly aesthetic with flat shading and simplified geometries.".into(),
        }
    } else {
        ArtStyleFact {
            primary_style: "modern_urban".into(),
            confidence: 0.70,
            palette: vec![
                "#1f2937".into(),
                "#6b7280".into(),
                "#e5e7eb".into(),
                "#3b82f6".into(),
            ],
            lighting_mood: "overcast_day".into(),
            recommended_presets: vec!["modern_city".into(), "industrial_harbor".into()],
            reasoning: "Defaulted to modern urban architectural layout with neutral concrete/glass material palette.".into(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_survey_workspace_mock_project() {
        let tmp = std::env::temp_dir().join(format!("forge-survey-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(tmp.join("Content").join("Scenes")).unwrap();
        std::fs::create_dir_all(tmp.join("Content").join("Materials")).unwrap();
        std::fs::create_dir_all(tmp.join("Content").join("Models")).unwrap();

        // Write forge.toml
        std::fs::write(
            tmp.join("forge.toml"),
            r#"
[project]
name = "CyberHarbor"
mode = "3d"
entry_scene = "Content/Scenes/Main.rxscene"

[render]
backend = "rurix"
method = "forward_plus"
driver = "vulkan"
"#,
        )
        .unwrap();

        // Write Main.rxscene
        let scene_json = r#"{
  "name": "Main",
  "mode": "3d",
  "gravity": [0.0, -9.81, 0.0],
  "entities": [
    {
      "id": 1,
      "name": "SunLight",
      "transform": {
        "translation": [0.0, 50.0, 0.0],
        "rotation": [0.7071, 0.0, 0.0, 0.7071],
        "scale": [1.0, 1.0, 1.0]
      },
      "components": [
        {
          "type": "Light",
          "enabled": true,
          "props": {
            "kind": "directional",
            "color": [1.0, 0.95, 0.8],
            "intensity": 2.5,
            "castShadow": true
          }
        }
      ]
    },
    {
      "id": 2,
      "name": "MainCamera",
      "transform": {
        "translation": [0.0, 10.0, 30.0],
        "rotation": [0.0, 0.0, 0.0, 1.0],
        "scale": [1.0, 1.0, 1.0]
      },
      "components": [
        {
          "type": "Camera",
          "enabled": true,
          "props": {
            "projection": "perspective",
            "fov": 60.0,
            "near": 0.1,
            "far": 1000.0
          }
        }
      ]
    },
    {
      "id": 3,
      "name": "GroundFloor",
      "transform": {
        "translation": [0.0, 0.0, 0.0],
        "rotation": [0.0, 0.0, 0.0, 1.0],
        "scale": [100.0, 1.0, 100.0]
      },
      "components": [
        {
          "type": "MeshRenderer",
          "enabled": true,
          "props": {}
        }
      ]
    }
  ]
}"#;
        std::fs::write(tmp.join("Content/Scenes/Main.rxscene"), scene_json).unwrap();

        // Write a test material
        let mat_json = r#"{
  "guid": "mat_neon_01",
  "shader": "pbr-default",
  "params": {
    "baseColor": [0.0, 0.8, 1.0, 1.0],
    "roughness": 0.2,
    "metallic": 0.8,
    "emissive": [0.0, 0.9, 1.0]
  }
}"#;
        std::fs::write(tmp.join("Content/Materials/M_Neon.rxmat"), mat_json).unwrap();

        // Write a test model
        let model_json = r#"{
  "guid": "mdl_dock_01",
  "name": "DockCranes",
  "kind": "building",
  "polycount": 3500
}"#;
        std::fs::write(tmp.join("Content/Models/DockCranes.rxmodel"), model_json).unwrap();

        // Execute survey
        let facts = survey_workspace(&tmp, &tmp, "Build a cyberpunk harbor").unwrap();
        assert!(facts.has_project);
        assert_eq!(facts.project_name, "CyberHarbor");
        assert_eq!(facts.game_mode, "3d");
        assert_eq!(facts.render_backend, "rurix");
        assert_eq!(facts.scene_summary.name, "Main");
        assert_eq!(facts.scene_summary.light_count, 1);
        assert_eq!(facts.scene_summary.camera_count, 1);
        assert!(facts.lighting.has_directional_light);
        assert_eq!(facts.existing_materials.len(), 1);
        assert_eq!(facts.existing_materials[0].name, "M_Neon");
        assert_eq!(facts.existing_models.len(), 1);
        assert_eq!(facts.existing_models[0].name, "DockCranes");
        assert_eq!(facts.detected_art_style.primary_style, "cyberpunk");

        // Test atomic persistence
        let target_file = persist_facts(&tmp, "cyber-harbor-1", &facts).unwrap();
        assert!(target_file.is_file());
        assert!(tmp.join(".forge/world_build/cyber-harbor-1/facts.json").is_file());

        // Test path traversal rejection
        assert!(persist_facts(&tmp, "../escape", &facts).is_err());
        assert!(persist_facts(&tmp, "nested/slug", &facts).is_err());

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_art_style_inference_varieties() {
        let empty_mats = Vec::new();
        let empty_models = Vec::new();
        let empty_lighting = LightingFacts {
            lights: Vec::new(),
            ambient_light: [0.1, 0.1, 0.1],
            dominant_direction: None,
            has_directional_light: false,
        };
        let env = default_environment();

        let s1 = infer_art_style("Medieval castle with torchlight", &empty_mats, &empty_lighting, &env, &empty_models);
        assert_eq!(s1.primary_style, "medieval_fantasy");

        let s2 = infer_art_style("Lowpoly stylized island", &empty_mats, &empty_lighting, &env, &empty_models);
        assert_eq!(s2.primary_style, "stylized_lowpoly");

        let s3 = infer_art_style("Modern residential district", &empty_mats, &empty_lighting, &env, &empty_models);
        assert_eq!(s3.primary_style, "modern_urban");
    }
}
