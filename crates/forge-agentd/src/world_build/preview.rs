//! crates/forge-agentd/src/world_build/preview.rs
//!
//! Interactive 3D WebUI Preview Generator & Contract Provider.
//! Generates an offline-first, self-contained Three.js / WebGL 3D preview
//! without any external CDN dependencies (strictly compliant with loopback CSP).
//! Exposes `window.__buildPreview` and bidirectional `postMessage` bridge.

use std::path::{Path, PathBuf};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use crate::events::now_rfc3339;
use crate::world_build::{write_atomic, write_json_atomic, PREVIEW_DIR};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BuildPreviewBuilding {
    pub id: String,
    pub name: String,
    #[serde(rename = "type")]
    pub building_type: String,
    pub transform: TransformConfig,
    pub bounds: BoundsConfig,
    pub materials: Vec<String>,
    pub interior_props_count: usize,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prompt: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parameters: Option<Value>,
    #[serde(default = "default_status")]
    pub status: String,
}

fn default_status() -> String {
    "ready".to_string()
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TransformConfig {
    pub position: [f32; 3],
    pub rotation: [f32; 3],
    pub scale: [f32; 3],
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BoundsConfig {
    pub min: [f32; 3],
    pub max: [f32; 3],
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dimensions: Option<[f32; 3]>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EnvironmentConfig {
    pub skybox: String,
    pub fog: bool,
    pub ambient_intensity: f32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CameraPerspectiveView {
    pub name: String,
    pub position: [f32; 3],
    pub target: [f32; 3],
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BuildPreviewSceneState {
    pub slug: String,
    pub rev: u32,
    pub style: String,
    pub buildings: Vec<BuildPreviewBuilding>,
    pub environment: EnvironmentConfig,
    #[serde(default)]
    pub camera_views: Vec<CameraPerspectiveView>,
    pub updated_at: String,
}

/// Generates standard camera perspective views for a scene.
pub fn default_camera_views(selected_building: Option<&BuildPreviewBuilding>) -> Vec<CameraPerspectiveView> {
    let building_view = if let Some(b) = selected_building {
        let p = b.transform.position;
        CameraPerspectiveView {
            name: "building".to_string(),
            position: [p[0] + 8.0, p[1] + 6.0, p[2] + 8.0],
            target: [p[0], p[1] + 2.0, p[2]],
        }
    } else {
        CameraPerspectiveView {
            name: "building".to_string(),
            position: [12.0, 10.0, 12.0],
            target: [0.0, 4.0, 0.0],
        }
    };

    vec![
        CameraPerspectiveView {
            name: "isometric".to_string(),
            position: [25.0, 20.0, 25.0],
            target: [0.0, 0.0, 0.0],
        },
        CameraPerspectiveView {
            name: "front".to_string(),
            position: [0.0, 10.0, 35.0],
            target: [0.0, 5.0, 0.0],
        },
        CameraPerspectiveView {
            name: "side".to_string(),
            position: [35.0, 10.0, 0.0],
            target: [0.0, 5.0, 0.0],
        },
        CameraPerspectiveView {
            name: "top".to_string(),
            position: [0.0, 50.0, 0.0],
            target: [0.0, 0.0, 0.0],
        },
        building_view,
    ]
}

/// Synthesizes an initial realistic 3D scene manifest when starting from survey facts.
pub fn synthesize_initial_manifest(slug: &str, facts_opt: Option<&Value>) -> Value {
    let style = facts_opt
        .and_then(|f| f.get("artStyle").or_else(|| f.get("detectedArtStyle")))
        .and_then(|v| v.as_str().or_else(|| v.get("name").and_then(Value::as_str)))
        .unwrap_or("cyberpunk_harbor");

    let buildings = vec![
        json!({
            "id": "bldg_terminal_01",
            "name": "港口指挥塔 (Terminal Tower)",
            "type": "command_tower",
            "transform": {
                "position": [0.0, 0.0, 0.0],
                "rotation": [0.0, 0.0, 0.0],
                "scale": [1.0, 1.0, 1.0]
            },
            "bounds": {
                "min": [-4.0, 0.0, -4.0],
                "max": [4.0, 18.0, 4.0],
                "dimensions": [8.0, 18.0, 8.0]
            },
            "materials": ["mat_cyber_neon", "mat_wet_metal"],
            "interiorPropsCount": 6,
            "prompt": "三层赛博朋克指挥塔，顶部带有旋转雷达天线与全息招牌",
            "status": "ready"
        }),
        json!({
            "id": "bldg_warehouse_02",
            "name": "重型货仓 (Cargo Warehouse)",
            "type": "warehouse",
            "transform": {
                "position": [16.0, 0.0, -4.0],
                "rotation": [0.0, 0.0, 0.0],
                "scale": [1.0, 1.0, 1.0]
            },
            "bounds": {
                "min": [-6.0, 0.0, -5.0],
                "max": [6.0, 8.0, 5.0],
                "dimensions": [12.0, 8.0, 10.0]
            },
            "materials": ["mat_weathered_iron", "mat_concrete_pbr"],
            "interiorPropsCount": 8,
            "prompt": "工业重型货运仓库，带有装卸轨道与起重机接口",
            "status": "ready"
        }),
        json!({
            "id": "bldg_dock_03",
            "name": "浮空停泊码头 (Hover Dock)",
            "type": "dock",
            "transform": {
                "position": [-16.0, 0.0, 6.0],
                "rotation": [0.0, 0.0, 0.0],
                "scale": [1.0, 1.0, 1.0]
            },
            "bounds": {
                "min": [-5.0, 0.0, -6.0],
                "max": [5.0, 6.0, 6.0],
                "dimensions": [10.0, 6.0, 12.0]
            },
            "materials": ["mat_wet_metal", "mat_warning_stripes"],
            "interiorPropsCount": 4,
            "prompt": "带有防浪护板与发光指引地灯的飞船停泊栈桥",
            "status": "ready"
        })
    ];

    let camera_views = json!([
        { "name": "isometric", "position": [25.0, 20.0, 25.0], "target": [0.0, 0.0, 0.0] },
        { "name": "front", "position": [0.0, 10.0, 35.0], "target": [0.0, 5.0, 0.0] },
        { "name": "side", "position": [35.0, 10.0, 0.0], "target": [0.0, 5.0, 0.0] },
        { "name": "top", "position": [0.0, 50.0, 0.0], "target": [0.0, 0.0, 0.0] },
        { "name": "building", "position": [8.0, 10.0, 8.0], "target": [0.0, 5.0, 0.0] }
    ]);

    json!({
        "slug": slug,
        "rev": 1,
        "style": style,
        "buildings": buildings,
        "environment": {
            "skybox": "night_cyberpunk",
            "fog": true,
            "ambientIntensity": 0.4
        },
        "cameraViews": camera_views,
        "updatedAt": now_rfc3339()
    })
}

/// Generates the complete preview directory package atomically.
pub fn generate_preview_package(
    ws_root: &Path,
    slug: &str,
    manifest_opt: Option<&Value>,
) -> std::io::Result<PathBuf> {
    let preview_dir = ws_root
        .join(".forge")
        .join("world_build")
        .join(slug)
        .join(PREVIEW_DIR);
    std::fs::create_dir_all(&preview_dir)?;

    let manifest = match manifest_opt {
        Some(m) => m.clone(),
        None => synthesize_initial_manifest(slug, None),
    };

    // 1. Dual-sync: manifest.json and scene.json
    let p_manifest = preview_dir.join("manifest.json");
    let p_scene = preview_dir.join("scene.json");
    write_json_atomic(&p_manifest, &manifest)?;
    write_json_atomic(&p_scene, &manifest)?;

    // 2. preview.js
    let p_js = preview_dir.join("preview.js");
    let js_code = render_js_runtime(slug, &manifest);
    write_atomic(&p_js, &js_code)?;

    // 3. index.html
    let p_html = preview_dir.join("index.html");
    let html_code = render_html_template(slug, &manifest, &js_code);
    write_atomic(&p_html, &html_code)?;

    // 4. probe.json
    let p_probe = preview_dir.join("probe.json");
    let probe_spec = json!({
        "contract": "window.__buildPreview",
        "methods": ["getSceneState", "selectBuilding", "getSelectedBuilding", "applyStylePreset", "getCameraViews"],
        "assertions": [
            { "path": "slug", "op": "eq", "expected": slug },
            { "path": "buildings", "op": "truthy" }
        ]
    });
    write_json_atomic(&p_probe, &probe_spec)?;

    // Register with demo host so preview is immediately browsable at /u/<slug>/ and /u/<token>/
    crate::demo_host::global().register(slug, &preview_dir);
    if let Some(token) = manifest.get("token").and_then(Value::as_str) {
        if !token.is_empty() && token != slug {
            crate::demo_host::global().register(token, &preview_dir);
        }
    }

    Ok(preview_dir)
}

/// Renders the standalone JavaScript runtime containing `window.__buildPreview`
/// and the postMessage bridge.
pub fn render_js_runtime(_slug: &str, manifest: &Value) -> String {
    let manifest_json = serde_json::to_string(manifest).unwrap_or_else(|_| "{}".to_string());
    format!(r#"(function() {{
  'use strict';

  // Active scene state
  let sceneState = {manifest_json};
  let selectedBuildingId = null;

  // Initialize bounds dimensions if missing
  if (Array.isArray(sceneState.buildings)) {{
    sceneState.buildings.forEach(b => {{
      if (b.bounds && !b.bounds.dimensions && b.bounds.min && b.bounds.max) {{
        b.bounds.dimensions = [
          Math.abs(b.bounds.max[0] - b.bounds.min[0]),
          Math.abs(b.bounds.max[1] - b.bounds.min[1]),
          Math.abs(b.bounds.max[2] - b.bounds.min[2])
        ];
      }}
    }});
  }}

  function postToParent(msg) {{
    try {{
      if (window.parent && window.parent !== window) {{
        window.parent.postMessage(msg, '*');
      }}
    }} catch (e) {{
      console.warn('[BuildPreview] postMessage error:', e);
    }}
  }}

  // --- Official window.__buildPreview Contract ---
  const api = {{
    getSceneState: function() {{
      return JSON.parse(JSON.stringify(sceneState));
    }},

    selectBuilding: function(id) {{
      if (id === null) {{
        selectedBuildingId = null;
        updateHighlight();
        postToParent({{
          type: 'BUILD_PREVIEW_SELECTED',
          buildingId: null,
          building: null
        }});
        return true;
      }}
      const found = sceneState.buildings.find(b => b.id === id);
      if (found) {{
        selectedBuildingId = id;
        updateHighlight();
        postToParent({{
          type: 'BUILD_PREVIEW_SELECTED',
          buildingId: id,
          building: JSON.parse(JSON.stringify(found))
        }});
        return true;
      }}
      return false;
    }},

    getSelectedBuilding: function() {{
      return selectedBuildingId;
    }},

    applyStylePreset: function(preset) {{
      if (!preset || typeof preset !== 'string') return false;
      sceneState.style = preset;
      if (preset.includes('medieval')) {{
        sceneState.environment = {{ skybox: 'day_medieval_clear', fog: false, ambientIntensity: 0.75 }};
      }} else if (preset.includes('sci_fi') || preset.includes('scifi')) {{
        sceneState.environment = {{ skybox: 'overcast_clean', fog: false, ambientIntensity: 0.85 }};
      }} else {{
        sceneState.environment = {{ skybox: 'night_cyberpunk', fog: true, ambientIntensity: 0.35 }};
      }}
      updateLightingAndStyle(preset);
      postToParent({{
        type: 'BUILD_PREVIEW_STATE_CHANGED',
        sceneState: api.getSceneState()
      }});
      return true;
    }},

    getCameraViews: function() {{
      const bldg = selectedBuildingId ? sceneState.buildings.find(b => b.id === selectedBuildingId) : null;
      const bPos = bldg ? bldg.transform.position : [0, 4, 0];
      return [
        {{ name: 'isometric', position: [25.0, 20.0, 25.0], target: [0.0, 0.0, 0.0] }},
        {{ name: 'front', position: [0.0, 10.0, 35.0], target: [0.0, 5.0, 0.0] }},
        {{ name: 'side', position: [35.0, 10.0, 0.0], target: [0.0, 5.0, 0.0] }},
        {{ name: 'top', position: [0.0, 50.0, 0.0], target: [0.0, 0.0, 0.0] }},
        {{ name: 'building', position: [bPos[0] + 8.0, bPos[1] + 6.0, bPos[2] + 8.0], target: [bPos[0], bPos[1] + 2.0, bPos[2]] }}
      ];
    }}
  }};

  window.__buildPreview = api;

  // UltraPlan / Web Probe test alias compatibility
  window.__demo = {{
    getState: () => api.getSceneState(),
    selectBuilding: (id) => api.selectBuilding(id),
    getSelectedBuilding: () => api.getSelectedBuilding(),
    applyStylePreset: (preset) => api.applyStylePreset(preset),
    getCameraViews: () => api.getCameraViews()
  }};

  // --- postMessage Bridge Listener ---
  window.addEventListener('message', function(ev) {{
    if (!ev.data || typeof ev.data !== 'object') return;
    const msg = ev.data;
    switch (msg.type) {{
      case 'SELECT_BUILDING':
      case 'select_building':
        api.selectBuilding(msg.buildingId ?? null);
        break;
      case 'SET_CAMERA_VIEW':
      case 'camera_view':
        setCameraPerspective(msg.view || msg.viewName);
        break;
      case 'APPLY_STYLE':
      case 'apply_style':
        api.applyStylePreset(msg.preset || msg.stylePreset);
        break;
      case 'GET_SCENE_STATE':
      case 'request_state':
        postToParent({{
          type: 'BUILD_PREVIEW_STATE_CHANGED',
          sceneState: api.getSceneState()
        }});
        break;
    }}
  }});

  // --- Pure WebGL 3D Renderer (Offline, Zero CDN) ---
  const canvas = document.getElementById('preview-canvas');
  if (!canvas) return;

  const gl = canvas.getContext('webgl', {{ antialias: true, alpha: false }});
  if (!gl) {{
    console.warn('[BuildPreview] WebGL not supported, fallback to 2D');
    return;
  }}

  let cameraAngle = {{ yaw: 0.785, pitch: 0.5, dist: 40.0 }};
  let cameraTarget = [0, 2, 0];
  let isDragging = false;
  let lastMouse = {{ x: 0, y: 0 }};

  function setCameraPerspective(name) {{
    if (name === 'top') {{
      cameraAngle.yaw = 0;
      cameraAngle.pitch = 1.56;
      cameraAngle.dist = 50.0;
      cameraTarget = [0, 0, 0];
    }} else if (name === 'front') {{
      cameraAngle.yaw = 0;
      cameraAngle.pitch = 0.2;
      cameraAngle.dist = 38.0;
      cameraTarget = [0, 5, 0];
    }} else if (name === 'side') {{
      cameraAngle.yaw = 1.57;
      cameraAngle.pitch = 0.2;
      cameraAngle.dist = 38.0;
      cameraTarget = [0, 5, 0];
    }} else if (name === 'building' && selectedBuildingId) {{
      const b = sceneState.buildings.find(x => x.id === selectedBuildingId);
      if (b) {{
        cameraTarget = [...b.transform.position];
        cameraAngle.dist = 20.0;
      }}
    }} else {{
      cameraAngle.yaw = 0.785;
      cameraAngle.pitch = 0.5;
      cameraAngle.dist = 40.0;
      cameraTarget = [0, 2, 0];
    }}
    drawScene();
  }}

  function updateHighlight() {{
    const el = document.getElementById('selected-info');
    if (el) {{
      if (selectedBuildingId) {{
        const b = sceneState.buildings.find(x => x.id === selectedBuildingId);
        el.textContent = '已选中: ' + (b ? b.name : selectedBuildingId);
        el.style.display = 'block';
      }} else {{
        el.textContent = '未选中建筑';
        el.style.display = 'none';
      }}
    }}
    drawScene();
  }}

  function updateLightingAndStyle(preset) {{
    const badge = document.getElementById('style-badge');
    if (badge) badge.textContent = '风格: ' + preset;
    drawScene();
  }}

  // Shaders
  const vsSource = `
    attribute vec3 aPosition;
    attribute vec3 aNormal;
    uniform mat4 uMatrix;
    uniform mat4 uModel;
    varying vec3 vNormal;
    varying vec3 vPos;
    void main() {{
      gl_Position = uMatrix * vec4(aPosition, 1.0);
      vNormal = mat3(uModel) * aNormal;
      vPos = (uModel * vec4(aPosition, 1.0)).xyz;
    }}
  `;

  const fsSource = `
    precision mediump float;
    varying vec3 vNormal;
    varying vec3 vPos;
    uniform vec3 uColor;
    uniform vec3 uLightDir;
    uniform float uAmbient;
    uniform bool uHighlight;
    void main() {{
      vec3 n = normalize(vNormal);
      float diff = max(dot(n, normalize(uLightDir)), 0.0);
      vec3 col = uColor * (uAmbient + diff * 0.7);
      if (uHighlight) {{
        col += vec3(0.25, 0.45, 0.85);
      }}
      gl_FragColor = vec4(col, 1.0);
    }}
  `;

  function createShader(gl, type, source) {{
    const shader = gl.createShader(type);
    gl.shaderSource(shader, source);
    gl.compileShader(shader);
    return shader;
  }}

  const prog = gl.createProgram();
  gl.attachShader(prog, createShader(gl, gl.VERTEX_SHADER, vsSource));
  gl.attachShader(prog, createShader(gl, gl.FRAGMENT_SHADER, fsSource));
  gl.linkProgram(prog);
  gl.useProgram(prog);

  const locPos = gl.getAttribLocation(prog, 'aPosition');
  const locNormal = gl.getAttribLocation(prog, 'aNormal');
  const locMatrix = gl.getUniformLocation(prog, 'uMatrix');
  const locModel = gl.getUniformLocation(prog, 'uModel');
  const locColor = gl.getUniformLocation(prog, 'uColor');
  const locLightDir = gl.getUniformLocation(prog, 'uLightDir');
  const locAmbient = gl.getUniformLocation(prog, 'uAmbient');
  const locHighlight = gl.getUniformLocation(prog, 'uHighlight');

  // Cube geometry with normals
  const cubeVerts = new Float32Array([
    // Front
    -0.5, -0.5,  0.5,   0,  0,  1,
     0.5, -0.5,  0.5,   0,  0,  1,
     0.5,  0.5,  0.5,   0,  0,  1,
    -0.5, -0.5,  0.5,   0,  0,  1,
     0.5,  0.5,  0.5,   0,  0,  1,
    -0.5,  0.5,  0.5,   0,  0,  1,
    // Back
    -0.5, -0.5, -0.5,   0,  0, -1,
    -0.5,  0.5, -0.5,   0,  0, -1,
     0.5,  0.5, -0.5,   0,  0, -1,
    -0.5, -0.5, -0.5,   0,  0, -1,
     0.5,  0.5, -0.5,   0,  0, -1,
     0.5, -0.5, -0.5,   0,  0, -1,
    // Top
    -0.5,  0.5, -0.5,   0,  1,  0,
    -0.5,  0.5,  0.5,   0,  1,  0,
     0.5,  0.5,  0.5,   0,  1,  0,
    -0.5,  0.5, -0.5,   0,  1,  0,
     0.5,  0.5,  0.5,   0,  1,  0,
     0.5,  0.5, -0.5,   0,  1,  0,
    // Bottom
    -0.5, -0.5, -0.5,   0, -1,  0,
     0.5, -0.5, -0.5,   0, -1,  0,
     0.5, -0.5,  0.5,   0, -1,  0,
    -0.5, -0.5, -0.5,   0, -1,  0,
     0.5, -0.5,  0.5,   0, -1,  0,
    -0.5, -0.5,  0.5,   0, -1,  0,
    // Right
     0.5, -0.5, -0.5,   1,  0,  0,
     0.5,  0.5, -0.5,   1,  0,  0,
     0.5,  0.5,  0.5,   1,  0,  0,
     0.5, -0.5, -0.5,   1,  0,  0,
     0.5,  0.5,  0.5,   1,  0,  0,
     0.5, -0.5,  0.5,   1,  0,  0,
    // Left
    -0.5, -0.5, -0.5,  -1,  0,  0,
    -0.5, -0.5,  0.5,  -1,  0,  0,
    -0.5,  0.5,  0.5,  -1,  0,  0,
    -0.5, -0.5, -0.5,  -1,  0,  0,
    -0.5,  0.5,  0.5,  -1,  0,  0,
    -0.5,  0.5, -0.5,  -1,  0,  0
  ]);

  const vbo = gl.createBuffer();
  gl.bindBuffer(gl.ARRAY_BUFFER, vbo);
  gl.bufferData(gl.ARRAY_BUFFER, cubeVerts, gl.STATIC_DRAW);

  // Matrix math
  function mat4Perspective(out, fov, aspect, near, far) {{
    const f = 1.0 / Math.tan(fov / 2);
    const nf = 1 / (near - far);
    out[0] = f / aspect; out[1] = 0; out[2] = 0; out[3] = 0;
    out[4] = 0; out[5] = f; out[6] = 0; out[7] = 0;
    out[8] = 0; out[9] = 0; out[10] = (far + near) * nf; out[11] = -1;
    out[12] = 0; out[13] = 0; out[14] = (2 * far * near) * nf; out[15] = 0;
    return out;
  }}

  function mat4LookAt(out, eye, center, up) {{
    let z0 = eye[0] - center[0], z1 = eye[1] - center[1], z2 = eye[2] - center[2];
    let len = 1 / Math.hypot(z0, z1, z2);
    z0 *= len; z1 *= len; z2 *= len;
    let x0 = up[1] * z2 - up[2] * z1, x1 = up[2] * z0 - up[0] * z2, x2 = up[0] * z1 - up[1] * z0;
    len = 1 / Math.hypot(x0, x1, x2);
    x0 *= len; x1 *= len; x2 *= len;
    let y0 = z1 * x2 - z2 * x1, y1 = z2 * x0 - z0 * x2, y2 = z0 * x1 - z1 * x0;
    out[0] = x0; out[1] = y0; out[2] = z0; out[3] = 0;
    out[4] = x1; out[5] = y1; out[6] = z1; out[7] = 0;
    out[8] = x2; out[9] = y2; out[10] = z2; out[11] = 0;
    out[12] = -(x0 * eye[0] + x1 * eye[1] + x2 * eye[2]);
    out[13] = -(y0 * eye[0] + y1 * eye[1] + y2 * eye[2]);
    out[14] = -(z0 * eye[0] + z1 * eye[1] + z2 * eye[2]);
    out[15] = 1;
    return out;
  }}

  function mat4Multiply(out, a, b) {{
    const a00 = a[0], a01 = a[1], a02 = a[2], a03 = a[3];
    const a10 = a[4], a11 = a[5], a12 = a[6], a13 = a[7];
    const a20 = a[8], a21 = a[9], a22 = a[10], a23 = a[11];
    const a30 = a[12], a31 = a[13], a32 = a[14], a33 = a[15];
    let b0 = b[0], b1 = b[1], b2 = b[2], b3 = b[3];
    out[0] = b0*a00 + b1*a10 + b2*a20 + b3*a30;
    out[1] = b0*a01 + b1*a11 + b2*a21 + b3*a31;
    out[2] = b0*a02 + b1*a12 + b2*a22 + b3*a32;
    out[3] = b0*a03 + b1*a13 + b2*a23 + b3*a33;
    b0 = b[4]; b1 = b[5]; b2 = b[6]; b3 = b[7];
    out[4] = b0*a00 + b1*a10 + b2*a20 + b3*a30;
    out[5] = b0*a01 + b1*a11 + b2*a21 + b3*a31;
    out[6] = b0*a02 + b1*a12 + b2*a22 + b3*a32;
    out[7] = b0*a03 + b1*a13 + b2*a23 + b3*a33;
    b0 = b[8]; b1 = b[9]; b2 = b[10]; b3 = b[11];
    out[8] = b0*a00 + b1*a10 + b2*a20 + b3*a30;
    out[9] = b0*a01 + b1*a11 + b2*a21 + b3*a31;
    out[10] = b0*a02 + b1*a12 + b2*a22 + b3*a32;
    out[11] = b0*a03 + b1*a13 + b2*a23 + b3*a33;
    b0 = b[12]; b1 = b[13]; b2 = b[14]; b3 = b[15];
    out[12] = b0*a00 + b1*a10 + b2*a20 + b3*a30;
    out[13] = b0*a01 + b1*a11 + b2*a21 + b3*a31;
    out[14] = b0*a02 + b1*a12 + b2*a22 + b3*a32;
    out[15] = b0*a03 + b1*a13 + b2*a23 + b3*a33;
    return out;
  }}

  function mat4Identity(out) {{
    for (let i = 0; i < 16; i++) out[i] = (i % 5 === 0) ? 1 : 0;
    return out;
  }}

  function mat4Transform(out, pos, scale) {{
    mat4Identity(out);
    out[0] = scale[0];
    out[5] = scale[1];
    out[10] = scale[2];
    out[12] = pos[0];
    out[13] = pos[1];
    out[14] = pos[2];
    return out;
  }}

  function drawScene() {{
    const w = canvas.clientWidth || 800;
    const h = canvas.clientHeight || 500;
    if (canvas.width !== w || canvas.height !== h) {{
      canvas.width = w;
      canvas.height = h;
      gl.viewport(0, 0, w, h);
    }}

    gl.enable(gl.DEPTH_TEST);

    // Clear color according to style
    if (sceneState.style.includes('medieval')) {{
      gl.clearColor(0.2, 0.35, 0.5, 1.0);
    }} else if (sceneState.style.includes('sci_fi')) {{
      gl.clearColor(0.12, 0.15, 0.2, 1.0);
    }} else {{
      gl.clearColor(0.05, 0.07, 0.12, 1.0);
    }}
    gl.clear(gl.COLOR_BUFFER_BIT | gl.DEPTH_BUFFER_BIT);

    const eyeX = cameraTarget[0] + cameraAngle.dist * Math.cos(cameraAngle.pitch) * Math.sin(cameraAngle.yaw);
    const eyeY = cameraTarget[1] + cameraAngle.dist * Math.sin(cameraAngle.pitch);
    const eyeZ = cameraTarget[2] + cameraAngle.dist * Math.cos(cameraAngle.pitch) * Math.cos(cameraAngle.yaw);

    const proj = new Float32Array(16);
    const view = new Float32Array(16);
    const pv = new Float32Array(16);
    mat4Perspective(proj, 0.8, w / h, 0.5, 500.0);
    mat4LookAt(view, [eyeX, eyeY, eyeZ], cameraTarget, [0, 1, 0]);
    mat4Multiply(pv, proj, view);

    gl.enableVertexAttribArray(locPos);
    gl.enableVertexAttribArray(locNormal);
    gl.bindBuffer(gl.ARRAY_BUFFER, vbo);
    gl.vertexAttribPointer(locPos, 3, gl.FLOAT, false, 24, 0);
    gl.vertexAttribPointer(locNormal, 3, gl.FLOAT, false, 24, 12);

    gl.uniform3f(locLightDir, 0.5, 1.0, 0.7);
    gl.uniform1f(locAmbient, sceneState.environment?.ambientIntensity || 0.4);

    // 1. Draw ground plane
    const mGround = new Float32Array(16);
    mat4Transform(mGround, [0, -0.25, 0], [60, 0.5, 60]);
    const mvpGround = new Float32Array(16);
    mat4Multiply(mvpGround, pv, mGround);
    gl.uniformMatrix4fv(locMatrix, false, mvpGround);
    gl.uniformMatrix4fv(locModel, false, mGround);
    gl.uniform3f(locColor, 0.15, 0.18, 0.22);
    gl.uniform1i(locHighlight, 0);
    gl.drawArrays(gl.TRIANGLES, 0, 36);

    // 2. Draw buildings
    const colors = [
      [0.2, 0.55, 0.85],
      [0.85, 0.45, 0.25],
      [0.3, 0.75, 0.55],
      [0.75, 0.35, 0.85],
      [0.85, 0.75, 0.25]
    ];

    sceneState.buildings.forEach((b, idx) => {{
      const pos = b.transform.position;
      const dims = (b.bounds && b.bounds.dimensions) ? b.bounds.dimensions : [
        Math.abs(b.bounds.max[0] - b.bounds.min[0]),
        Math.abs(b.bounds.max[1] - b.bounds.min[1]),
        Math.abs(b.bounds.max[2] - b.bounds.min[2])
      ];
      const model = new Float32Array(16);
      mat4Transform(model, [pos[0], pos[1] + dims[1] / 2, pos[2]], dims);
      const mvp = new Float32Array(16);
      mat4Multiply(mvp, pv, model);

      gl.uniformMatrix4fv(locMatrix, false, mvp);
      gl.uniformMatrix4fv(locModel, false, model);

      const c = colors[idx % colors.length];
      gl.uniform3f(locColor, c[0], c[1], c[2]);
      gl.uniform1i(locHighlight, b.id === selectedBuildingId ? 1 : 0);
      gl.drawArrays(gl.TRIANGLES, 0, 36);
    }});
  }}

  // Mouse interaction: drag to rotate, wheel to zoom
  canvas.addEventListener('mousedown', (e) => {{
    isDragging = true;
    lastMouse = {{ x: e.clientX, y: e.clientY }};
  }});

  window.addEventListener('mousemove', (e) => {{
    if (!isDragging) return;
    const dx = e.clientX - lastMouse.x;
    const dy = e.clientY - lastMouse.y;
    lastMouse = {{ x: e.clientX, y: e.clientY }};
    cameraAngle.yaw -= dx * 0.01;
    cameraAngle.pitch = Math.max(0.1, Math.min(1.5, cameraAngle.pitch + dy * 0.01));
    drawScene();
  }});

  window.addEventListener('mouseup', () => {{
    isDragging = false;
  }});

  canvas.addEventListener('wheel', (e) => {{
    e.preventDefault();
    cameraAngle.dist = Math.max(10, Math.min(120, cameraAngle.dist + e.deltaY * 0.05));
    drawScene();
  }});

  // Click to select building
  canvas.addEventListener('click', (e) => {{
    const rect = canvas.getBoundingClientRect();
    const x = e.clientX - rect.left;
    const y = e.clientY - rect.top;

    // Approximate screen-to-building hit test
    let clickedId = null;
    let minDist = 999999;
    const w = canvas.width;
    const h = canvas.height;

    sceneState.buildings.forEach(b => {{
      const bPos = b.transform.position;
      const dx = bPos[0] - cameraTarget[0];
      const dz = bPos[2] - cameraTarget[2];
      // Simple 2D projection estimation
      const sx = w / 2 + (dx * Math.cos(cameraAngle.yaw) - dz * Math.sin(cameraAngle.yaw)) * (w / cameraAngle.dist);
      const sy = h / 2 - (bPos[1] + 4 - cameraTarget[1]) * (h / cameraAngle.dist);
      const dist = Math.hypot(x - sx, y - sy);
      if (dist < 80 && dist < minDist) {{
        minDist = dist;
        clickedId = b.id;
      }}
    }});

    api.selectBuilding(clickedId);
  }});

  window.addEventListener('resize', drawScene);

  // Initial draw & ready notification
  drawScene();
  postToParent({{
    type: 'BUILD_PREVIEW_READY',
    slug: sceneState.slug,
    rev: sceneState.rev,
    buildingCount: sceneState.buildings.length,
    sceneState: api.getSceneState()
  }});

}})();
"#)
}

/// Renders the self-contained HTML template with zero CDN scripts.
pub fn render_html_template(slug: &str, manifest: &Value, js_code: &str) -> String {
    let title = format!("3D Preview · {slug}");
    let rev = manifest.get("rev").and_then(Value::as_u64).unwrap_or(1);
    let style = manifest
        .get("style")
        .and_then(Value::as_str)
        .unwrap_or("cyberpunk");

    format!(r#"<!DOCTYPE html>
<html lang="zh-CN">
<head>
  <meta charset="utf-8">
  <meta name="viewport" content="width=device-width, initial-scale=1.0, user-scalable=no">
  <title>{title}</title>
  <style>
    * {{ box-sizing: border-box; margin: 0; padding: 0; }}
    html, body {{ width: 100%; height: 100%; overflow: hidden; background: #0c0e14; color: #e2e8f0; font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, sans-serif; }}
    #preview-canvas {{ width: 100%; height: 100%; display: block; cursor: grab; }}
    #preview-canvas:active {{ cursor: grabbing; }}
    .hud-header {{ position: absolute; top: 12px; left: 14px; pointer-events: none; }}
    .hud-title {{ font-size: 13px; font-weight: 600; color: #f8fafc; text-shadow: 0 1px 3px rgba(0,0,0,0.8); }}
    .hud-meta {{ font-size: 11px; color: #94a3b8; font-family: monospace; margin-top: 2px; }}
    .badge {{ display: inline-block; padding: 2px 6px; border-radius: 4px; font-size: 10.5px; background: rgba(30,41,59,0.8); border: 1px solid rgba(100,116,139,0.4); margin-right: 4px; }}
    .badge-rev {{ color: #38bdf8; }}
    .hud-controls {{ position: absolute; bottom: 12px; left: 14px; display: flex; gap: 6px; z-index: 10; }}
    .cam-btn {{ background: rgba(15,23,42,0.85); color: #cbd5e1; border: 1px solid rgba(71,85,105,0.6); border-radius: 4px; padding: 4px 8px; font-size: 11px; cursor: pointer; transition: all 0.15s; }}
    .cam-btn:hover {{ background: rgba(30,41,59,0.95); color: #f8fafc; border-color: #38bdf8; }}
    #selected-info {{ position: absolute; top: 12px; right: 14px; background: rgba(15,23,42,0.85); border: 1px solid #38bdf8; border-radius: 6px; padding: 6px 10px; font-size: 11px; color: #38bdf8; display: none; z-index: 10; }}
  </style>
</head>
<body>
  <div class="hud-header">
    <div class="hud-title">{title}</div>
    <div class="hud-meta">
      <span class="badge badge-rev">Rev {rev}</span>
      <span class="badge" id="style-badge">风格: {style}</span>
    </div>
  </div>

  <div id="selected-info">未选中建筑</div>

  <div class="hud-controls">
    <button class="cam-btn" onclick="window.postMessage({{type:'SET_CAMERA_VIEW', view:'isometric'}}, '*')">透视 (Iso)</button>
    <button class="cam-btn" onclick="window.postMessage({{type:'SET_CAMERA_VIEW', view:'top'}}, '*')">顶视 (Top)</button>
    <button class="cam-btn" onclick="window.postMessage({{type:'SET_CAMERA_VIEW', view:'front'}}, '*')">正视 (Front)</button>
    <button class="cam-btn" onclick="window.postMessage({{type:'SET_CAMERA_VIEW', view:'side'}}, '*')">侧视 (Side)</button>
  </div>

  <canvas id="preview-canvas"></canvas>

  <script>
{js_code}
  </script>
</body>
</html>
"#)
}
