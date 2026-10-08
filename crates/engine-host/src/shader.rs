//! Shared graph-material runtime. Drafts are separate from published programs. Every allocation is
//! owned by an Arc or by the renderer; replacing a graph does not leak shader binaries or textures.
use assetd::project::ForgeProject;
use forge_shader::{CompiledGraph, Domain, GraphDoc, ValueType};
use serde_json::{json, Value};
use std::{
    collections::BTreeMap,
    sync::{
        atomic::{AtomicU32, AtomicUsize, Ordering},
        Arc, Mutex, OnceLock,
    },
};

static TIME: AtomicU32 = AtomicU32::new(0);
static LIVE_PROGRAMS: AtomicUsize = AtomicUsize::new(0);
static RURIX_PIPELINE_BUILDS: AtomicUsize = AtomicUsize::new(0);
static GODOT_SHADER_BUILDS: AtomicUsize = AtomicUsize::new(0);
/// Actual backend compilation counters, separate from document publications and uniform uploads.
pub fn record_program_build(backend: &str) {
    match backend {
        "rurix" => &RURIX_PIPELINE_BUILDS,
        "godot" => &GODOT_SHADER_BUILDS,
        _ => return,
    }
    .fetch_add(1, Ordering::Relaxed);
}
pub fn set_frame_time(t: f32) {
    if t.is_finite() {
        TIME.store(t.to_bits(), Ordering::Relaxed);
    }
}
pub fn frame_time() -> f32 {
    f32::from_bits(TIME.load(Ordering::Relaxed))
}
#[derive(Debug)]
pub struct Program {
    pub compiled: CompiledGraph,
    pub sprite_spirv: Vec<u8>,
    pub model_spirv: Vec<u8>,
}
impl Drop for Program {
    fn drop(&mut self) {
        LIVE_PROGRAMS.fetch_sub(1, Ordering::Relaxed);
    }
}
#[derive(Clone, Debug)]
pub struct Texture {
    pub width: u32,
    pub height: u32,
    pub rgba: Arc<Vec<u8>>,
}
impl Texture {
    pub fn white() -> Self {
        Self {
            width: 1,
            height: 1,
            rgba: Arc::new(vec![255; 4]),
        }
    }
}
#[derive(Clone, Debug)]
pub struct Material {
    pub key: String,
    pub program: Arc<Program>,
    pub params: Vec<[f32; 4]>,
    pub textures: Vec<Texture>,
    pub frozen_time: Option<f32>,
}
impl PartialEq for Material {
    fn eq(&self, o: &Self) -> bool {
        self.key == o.key
    }
}
impl Material {
    pub fn time(&self) -> f32 {
        self.frozen_time.unwrap_or_else(frame_time)
    }
    pub fn packed_len(&self, source_len: usize) -> Result<usize, String> {
        let n = 1 + self.textures.len();
        let header = 4 + n * 8 + self.params.len() * 4;
        let size = header
            .checked_mul(4)
            .and_then(|x| x.checked_add(source_len))
            .and_then(|x| {
                self.textures
                    .iter()
                    .try_fold(x, |sum, t| sum.checked_add(t.rgba.len()))
            })
            .ok_or("SHADER_BUDGET: texture overflow")?;
        if size > 64 * 1024 * 1024 {
            return Err("SHADER_BUDGET: material exceeds 64 MiB".into());
        }
        Ok(size)
    }
    /// Shared storage ABI: time, texture headers, numeric vec4 slots, then RGBA8 texels.
    pub fn pack(&self, source: &Texture) -> Result<Vec<u8>, String> {
        let n = 1 + self.textures.len();
        let header = 4 + n * 8 + self.params.len() * 4;
        let size = self.packed_len(source.rgba.len())?;
        let mut data = vec![0u8; header * 4];
        data.reserve(size - data.len());
        data[..4].copy_from_slice(&self.time().to_le_bytes());
        for (i, t) in std::iter::once(source).chain(&self.textures).enumerate() {
            if t.width == 0
                || t.height == 0
                || t.width as usize * t.height as usize * 4 != t.rgba.len()
            {
                return Err("SHADER_TEXTURE: invalid RGBA texture".into());
            }
            for (j, v) in [(data.len() / 4) as u32, t.width, t.height]
                .into_iter()
                .enumerate()
            {
                let p = (4 + i * 8 + j) * 4;
                data[p..p + 4].copy_from_slice(&v.to_le_bytes());
            }
            data.extend_from_slice(&t.rgba);
        }
        for (i, p) in self.params.iter().enumerate() {
            for (j, v) in p.iter().enumerate() {
                let offset = (4 + n * 8 + i * 4 + j) * 4;
                data[offset..offset + 4].copy_from_slice(&v.to_le_bytes());
            }
        }
        Ok(data)
    }
}
#[derive(Default)]
struct State {
    programs: BTreeMap<String, Arc<Program>>,
    previous: BTreeMap<String, Arc<Program>>,
    rejected: BTreeMap<String, String>,
    materials: BTreeMap<String, Arc<Material>>,
    publications: BTreeMap<String, Value>,
    validated: BTreeMap<String, Value>,
}
static STATE: OnceLock<Mutex<State>> = OnceLock::new();
fn state() -> &'static Mutex<State> {
    STATE.get_or_init(|| Mutex::new(State::default()))
}
fn project() -> ForgeProject {
    let root = crate::rpc::project_root();
    ForgeProject::load(&root).unwrap_or_else(|_| ForgeProject::with_defaults(root))
}
fn program(graph: &GraphDoc) -> Result<Arc<Program>, String> {
    let compiled = forge_shader::compile(graph).map_err(|d| serde_json::to_string(&d).unwrap())?;
    let sprite_spirv = forge_shader::compile_spirv(&compiled.wgsl.sprite)?;
    let model_spirv = forge_shader::compile_spirv(&compiled.wgsl.model)?;
    LIVE_PROGRAMS.fetch_add(1, Ordering::Relaxed);
    Ok(Arc::new(Program {
        compiled,
        sprite_spirv,
        model_spirv,
    }))
}
fn graph_reference(p: &ForgeProject, reference: &str) -> Result<String, String> {
    let (rel, path) = assetd::shader::resolve(p, reference).map_err(|e| e.to_string())?;
    if rel.ends_with(".rxmat") {
        let doc: Value = serde_json::from_slice(&std::fs::read(path).map_err(|e| e.to_string())?)
            .map_err(|e| e.to_string())?;
        doc["shaderGraph"]
            .as_str()
            .map(str::to_owned)
            .ok_or("SHADER_MATERIAL_VERSION: requires rxmat v2".into())
    } else {
        Ok(reference.into())
    }
}
/// CPU compilation is synchronous. Backend pipeline validation is reported separately by renderers.
/// Does not lock HostState, reload assets, reset physics, or touch scene entities.
pub fn publish(reference: &str) -> Result<Value, String> {
    let p = project();
    let graph_ref = graph_reference(&p, reference)?;
    let loaded = assetd::shader::load(&p, &graph_ref).map_err(|e| e.to_string())?;
    let path = loaded["path"].as_str().ok_or("SHADER_PATH")?.to_string();
    let graph: GraphDoc =
        serde_json::from_value(loaded["graph"].clone()).map_err(|e| e.to_string())?;
    let candidate = match program(&graph) {
        Ok(p) => p,
        Err(e) => {
            let v = json!({"ok":false,"path":path,"state":"rejected","diagnostics":[{"backend":"compiler","message":e}]});
            state().lock().unwrap().publications.insert(path, v.clone());
            return Ok(v);
        }
    };
    let v = json!({"ok":true,"path":path,"hash":candidate.compiled.hash,"state":"pending-render","validation":{"rurix":"spirv-compiled","godot":"source-generated"},"diagnostics":[]});
    let mut s = state().lock().unwrap();
    // Re-publishing byte-identical, already rendered code must not wait for a renderer
    // cache miss that will never happen. Keep the existing program and its GPU evidence.
    if s.programs
        .get(&path)
        .is_some_and(|p| p.compiled.hash == candidate.compiled.hash)
    {
        if let Some(validation) = s.validated.get(&candidate.compiled.hash).cloned() {
            let mut active = v;
            active["state"] = json!("active");
            active["validation"] = validation;
            // The graph may be identical while a texture under the same asset GUID
            // was reimported. Refresh owned material inputs without touching physics.
            s.materials.clear();
            s.publications.insert(path, active.clone());
            return Ok(active);
        }
    }
    if let Some(previous) = s.programs.insert(path.clone(), candidate) {
        s.previous.entry(path.clone()).or_insert(previous);
    }
    s.rejected.remove(&path);
    s.materials.clear();
    s.publications.insert(path, v.clone());
    Ok(v)
}
pub fn status() -> Value {
    let s = state().lock().unwrap();
    json!({"publications":s.publications.values().collect::<Vec<_>>(),"programs":s.programs.len(),"livePrograms":LIVE_PROGRAMS.load(Ordering::Relaxed),"previousPrograms":s.previous.len(),"validationEntries":s.validated.len(),"materials":s.materials.len(),"frameTime":frame_time(),"backendBuilds":{"rurix":RURIX_PIPELINE_BUILDS.load(Ordering::Relaxed),"godot":GODOT_SHADER_BUILDS.load(Ordering::Relaxed)}})
}
pub fn report_backend(hash: &str, backend: &str, result: Result<(), String>) {
    let mut s = state().lock().unwrap();
    let paths = s
        .publications
        .iter()
        .filter(|(_, v)| v["hash"] == hash)
        .map(|(k, _)| k.clone())
        .collect::<Vec<_>>();
    for path in paths {
        // A candidate can fail in several draws. Only its first failure rolls back;
        // subsequent callbacks must not evict the restored, last valid program.
        if s.publications[&path]["state"] == "backend-rejected" {
            continue;
        }
        if let Some(v) = s.publications.get_mut(&path) {
            match &result {
                Ok(()) => {
                    if v["state"] != "backend-rejected" {
                        v["validation"][backend] = json!("pipeline-validated");
                        v["state"] = json!("active");
                    }
                }
                Err(e) => {
                    v["state"] = json!("backend-rejected");
                    v["ok"] = json!(false);
                    v["diagnostics"] =
                        json!([{"backend":backend,"message":e,"code":"SHADER_BACKEND_COMPILE"}]);
                }
            }
        }
        if result.is_ok() {
            let validation = s.publications[&path]["validation"].clone();
            s.validated.insert(hash.to_owned(), validation);
            s.previous.remove(&path);
        }
        if let Err(error) = &result {
            if let Some(previous) = s.previous.remove(&path) {
                s.programs.insert(path.clone(), previous);
            } else if !path.starts_with("preview:") {
                s.programs.remove(&path);
                s.rejected.insert(path.clone(), error.clone());
            }
            s.materials.retain(|_, m| m.program.compiled.hash != hash);
        }
    }
    prune_validations(&mut s);
}
fn prune_validations(s: &mut State) {
    let hashes = s
        .programs
        .values()
        .chain(s.previous.values())
        .map(|p| p.compiled.hash.as_str())
        .chain(
            s.publications
                .iter()
                .filter(|(p, _)| p.starts_with("preview:"))
                .filter_map(|(_, v)| v["hash"].as_str()),
        )
        .collect::<std::collections::BTreeSet<_>>();
    s.validated.retain(|hash, _| hashes.contains(hash.as_str()));
}

pub fn resolve(
    reference: &str,
    overrides: &Value,
    domain: Domain,
) -> Result<Option<Arc<Material>>, String> {
    if reference.starts_with("preview:") {
        return state()
            .lock()
            .unwrap()
            .materials
            .get(reference)
            .cloned()
            .map(Some)
            .ok_or("SHADER_PREVIEW_EXPIRED".into());
    }
    if reference.is_empty() {
        return Ok(None);
    }
    let request_key = format!("{reference}:{overrides}");
    if let Some(material) = state().lock().unwrap().materials.get(&request_key).cloned() {
        if (domain == Domain::Sprite2d) != (material.program.compiled.domain == Domain::Sprite2d) {
            return Err("SHADER_DOMAIN: incompatible binding".into());
        }
        return Ok(Some(material));
    }
    let p = project();
    let (rel, path) = assetd::shader::resolve(&p, reference).map_err(|e| e.to_string())?;
    let bytes = std::fs::read(&path).map_err(|e| e.to_string())?;
    if bytes.len() > 1024 * 1024 {
        return Err("SHADER_BUDGET: material too large".into());
    }
    let doc: Value = serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
    if doc["version"] != 2 {
        return Ok(None);
    }
    let shader_ref = doc["shaderGraph"]
        .as_str()
        .ok_or("SHADER_MATERIAL: missing shaderGraph")?;
    let (graph_path, _) = assetd::shader::resolve(&p, shader_ref).map_err(|e| e.to_string())?;
    if let Some(error) = state().lock().unwrap().rejected.get(&graph_path) {
        return Err(format!("SHADER_BACKEND_REJECTED: {error}"));
    }
    let existing = state().lock().unwrap().programs.get(&graph_path).cloned();
    let prog = if let Some(prog) = existing {
        prog
    } else {
        let loaded = assetd::shader::load(&p, shader_ref).map_err(|e| e.to_string())?;
        let graph: GraphDoc =
            serde_json::from_value(loaded["graph"].clone()).map_err(|e| e.to_string())?;
        let prog = program(&graph)?;
        {
            let mut s = state().lock().unwrap();
            s.programs.insert(graph_path.clone(), prog.clone());
            s.publications.entry(graph_path.clone()).or_insert_with(||json!({"path":graph_path,"hash":prog.compiled.hash,"state":"pending-render","validation":{"rurix":"spirv-compiled","godot":"source-generated"},"diagnostics":[]}));
        }
        prog
    };
    if (domain == Domain::Sprite2d) != (prog.compiled.domain == Domain::Sprite2d) {
        return Err(
            "SHADER_DOMAIN: sprite and spatial graph bindings cannot be interchanged".into(),
        );
    }
    let key = forge_shader::content_hash(
        format!(
            "{rel}:{}:{}:{overrides}",
            forge_shader::content_hash(&bytes),
            prog.compiled.hash
        )
        .as_bytes(),
    );
    for values in [&doc["params"], &doc["textures"], overrides] {
        if values.is_null() {
            continue;
        }
        let obj = values
            .as_object()
            .ok_or("SHADER_PARAMETER: expected parameter object")?;
        for id in obj.keys() {
            if !prog.compiled.parameters.iter().any(|p| &p.id == id) {
                return Err(format!("SHADER_PARAMETER: unknown parameter {id}"));
            }
        }
    }
    let mut params = Vec::new();
    let mut textures = Vec::new();
    for param in &prog.compiled.parameters {
        let value = overrides
            .get(&param.id)
            .or_else(|| {
                doc[if param.ty == ValueType::Texture2d {
                    "textures"
                } else {
                    "params"
                }]
                .get(&param.id)
            })
            .unwrap_or(&param.default);
        if !forge_shader::validate_parameter(param.ty, value) {
            return Err(format!("SHADER_PARAMETER {}: invalid value", param.id));
        }
        if param.ty == ValueType::Texture2d {
            let id = value.as_str().unwrap();
            if id.is_empty() {
                textures.push(Texture::white());
            } else {
                let (_, abs) = assetd::shader::resolve(&p, id).map_err(|e| e.to_string())?;
                let (width, height, rgba) =
                    assetd::texture::decode_rgba(&abs).map_err(|e| e.to_string())?;
                textures.push(Texture {
                    width,
                    height,
                    rgba: Arc::new(rgba),
                });
            }
        } else {
            let mut v = [0.; 4];
            if let Some(f) = value.as_f64() {
                v[0] = f as f32;
            } else {
                for (i, f) in value.as_array().unwrap().iter().enumerate() {
                    v[i] = f.as_f64().unwrap() as f32;
                }
            }
            params.push(v);
        }
    }
    let mut fingerprint = key.into_bytes();
    for texture in &textures {
        fingerprint.extend_from_slice(&texture.width.to_le_bytes());
        fingerprint.extend_from_slice(&texture.height.to_le_bytes());
        fingerprint.extend_from_slice(forge_shader::content_hash(&texture.rgba).as_bytes());
    }
    let material = Arc::new(Material {
        key: forge_shader::content_hash(&fingerprint),
        program: prog,
        frozen_time: None,
        params,
        textures,
    });
    let mut s = state().lock().unwrap();
    if s.materials.len() >= 256 {
        s.materials.clear();
    }
    s.materials.insert(request_key, material.clone());
    Ok(Some(material))
}
pub fn sprite(entity: &forge_scene::Entity) -> Result<Option<Arc<Material>>, String> {
    let Some(c) = entity.component("Sprite").filter(|c| c.enabled) else {
        return Ok(None);
    };
    resolve(
        c.props
            .get("material")
            .and_then(Value::as_str)
            .unwrap_or(""),
        c.props.get("materialParams").unwrap_or(&Value::Null),
        Domain::Sprite2d,
    )
}
pub fn model(entity: &forge_scene::Entity, slot: &str) -> Result<Option<Arc<Material>>, String> {
    let Some(c) = entity.component("ModelRenderer") else {
        return Ok(None);
    };
    let Some(b) = c.props.get("materialBindings").and_then(|v| v.get(slot)) else {
        return Ok(None);
    };
    resolve(
        b["material"].as_str().unwrap_or(""),
        &b["params"],
        Domain::Pbr3d,
    )
}
pub fn mesh(entity: &forge_scene::Entity) -> Result<Option<Arc<Material>>, String> {
    let Some(c) = entity.component("MeshRenderer").filter(|c| c.enabled) else {
        return Ok(None);
    };
    let reference = c
        .props
        .get("material")
        .and_then(Value::as_str)
        .unwrap_or("");
    // Legacy MeshRenderer materials could be missing or builtin identifiers; the old
    // renderer falls back to its default material. Explicit v2 parse/bind errors still
    // propagate once an actual material document exists.
    if !reference.is_empty() && !reference.starts_with("preview:") {
        match assetd::shader::resolve(&project(), reference) {
            Ok((_, path)) if !path.is_file() => return Ok(None),
            Err(e) if e.code == "SHADER_REFERENCE_NOT_FOUND" => return Ok(None),
            Err(e) => return Err(e.to_string()),
            _ => {}
        }
    }
    resolve(
        reference,
        c.props.get("materialParams").unwrap_or(&Value::Null),
        Domain::Pbr3d,
    )
}
pub fn scene_uses_spatial_graph(scene: &forge_scene::Scene) -> bool {
    scene
        .entities
        .iter()
        .any(|e| mesh(e).ok().flatten().is_some())
}
/// Position, normal, UV, tangent; the same generated geometry is used by both real backends.
pub fn mesh_vertices(entity: &forge_scene::Entity) -> Result<Vec<f32>, String> {
    let reference = entity
        .component("MeshRenderer")
        .and_then(|c| c.props["mesh"].as_str())
        .unwrap_or("cube");
    let mut out = Vec::new();
    if reference == "__shader_preview_sphere" {
        let point = |x: usize, y: usize| {
            let u = x as f32 / 32.;
            let v = y as f32 / 16.;
            let a = u * std::f32::consts::TAU;
            let b = v * std::f32::consts::PI;
            let p = [a.sin() * b.sin(), b.cos(), a.cos() * b.sin()];
            [
                p[0],
                p[1],
                p[2],
                p[0],
                p[1],
                p[2],
                u,
                v,
                a.cos(),
                0.,
                -a.sin(),
                1.,
            ]
        };
        for y in 0..16 {
            for x in 0..32 {
                for (a, b) in [
                    (x, y),
                    (x, y + 1),
                    (x + 1, y),
                    (x + 1, y),
                    (x, y + 1),
                    (x + 1, y + 1),
                ] {
                    out.extend_from_slice(&point(a, b));
                }
            }
        }
    } else if reference == "__shader_preview_plane" {
        for (x, y) in [
            (-1., -1.),
            (1., -1.),
            (1., 1.),
            (-1., -1.),
            (1., 1.),
            (-1., 1.),
        ] {
            out.extend_from_slice(&[
                x,
                y,
                0.,
                0.,
                0.,
                1.,
                (x + 1.) * 0.5,
                1. - (y + 1.) * 0.5,
                1.,
                0.,
                0.,
                1.,
            ]);
        }
    } else {
        let owned;
        let bytes = if reference.is_empty() || reference == "cube" {
            crate::render_core::assets::cube_mesh_bytes()
        } else {
            owned = crate::meshres::load_mesh_cached(&crate::rpc::project_root(), reference)?;
            &owned.bytes
        };
        for v in bytes.chunks_exact(24) {
            let f = |i: usize| f32::from_le_bytes(v[i * 4..i * 4 + 4].try_into().unwrap());
            out.extend_from_slice(&[
                f(0),
                f(1),
                f(2),
                f(3),
                f(4),
                f(5),
                f(0) + 0.5,
                0.5 - f(1),
                1.,
                0.,
                0.,
                1.,
            ]);
        }
    }
    Ok(out)
}
pub fn preview(args: &Value) -> Result<Value, String> {
    let graph: GraphDoc =
        serde_json::from_value(args["graph"].clone()).map_err(|e| e.to_string())?;
    let prog = program(&graph)?;
    let hash = prog.compiled.hash.clone();
    static PREVIEW_SEQUENCE: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
    let reference = format!(
        "preview:{hash}:{}",
        PREVIEW_SEQUENCE.fetch_add(1, Ordering::Relaxed)
    );
    let mut params = Vec::new();
    let mut textures = Vec::new();
    for p in &prog.compiled.parameters {
        if p.ty == ValueType::Texture2d {
            let id = p.default.as_str().unwrap();
            if id.is_empty() {
                textures.push(Texture::white())
            } else {
                let (_, path) =
                    assetd::shader::resolve(&project(), id).map_err(|e| e.to_string())?;
                let (w, h, rgba) =
                    assetd::texture::decode_rgba(&path).map_err(|e| e.to_string())?;
                textures.push(Texture {
                    width: w,
                    height: h,
                    rgba: Arc::new(rgba),
                });
            }
        } else {
            let mut v = [0.; 4];
            if let Some(f) = p.default.as_f64() {
                v[0] = f as f32
            } else {
                for (i, n) in p.default.as_array().unwrap().iter().enumerate() {
                    v[i] = n.as_f64().unwrap() as f32;
                }
            }
            params.push(v);
        }
    }
    let material = Arc::new(Material {
        key: reference.clone(),
        program: prog,
        frozen_time: Some(
            args["time"]
                .as_f64()
                .filter(|v| v.is_finite())
                .unwrap_or(0.) as f32,
        ),
        params,
        textures,
    });
    {
        let mut s = state().lock().unwrap();
        s.materials.insert(reference.clone(), material);
        s.publications.insert(reference.clone(),json!({"path":reference,"hash":hash,"state":"pending-render","validation":{"rurix":"spirv-compiled","godot":"source-generated"},"diagnostics":[]}));
    }
    let shape = if args["shape"] == "sphere" {
        "__shader_preview_sphere"
    } else {
        "__shader_preview_plane"
    };
    let mut scene = forge_scene::Scene::new("Shader Graph Preview");
    scene.entities.push(forge_scene::Entity {
        entity_guid: None,
        id: 1,
        name: "Shader Preview".into(),
        transform: Default::default(),
        components: vec![forge_scene::Component::new(
            "MeshRenderer",
            json!({"mesh":shape,"material":reference}),
        )],
    });
    scene.next_id = 2;
    let camera = crate::viewport::EditorCamera {
        target: [0.; 3],
        yaw_deg: 0.,
        pitch_deg: 0.,
        dist: 4.,
        ortho: true,
        ortho_half_h: 1.25,
        ..Default::default()
    };
    let width = args["width"].as_u64().unwrap_or(384).clamp(32, 768) as u32;
    let height = args["height"].as_u64().unwrap_or(384).clamp(32, 768) as u32;
    let result =
        crate::rpc::render_detached_preview(scene, camera, width, height).map_err(|(_, e)| e);
    let mut s = state().lock().unwrap();
    s.materials.remove(&reference);
    let status = s.publications.remove(&reference).unwrap_or(Value::Null);
    prune_validations(&mut s);
    drop(s);
    let mut frame = result?;
    frame["hash"] = json!(hash);
    frame["validation"] = status["validation"].clone();
    frame["diagnostics"] = status["diagnostics"].clone();
    frame["ok"] = json!(status["state"] == "active");
    Ok(frame)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn test_program(id: &str) -> Arc<Program> {
        program(&serde_json::from_value(json!({"version":1,"id":id,"name":id,"domain":"sprite2d","outputs":{"color":{"const":[1,0,0,1]}}})).unwrap()).unwrap()
    }
    #[test]
    fn graph_storage_contains_frozen_time_params_and_all_texture_headers() {
        let material = Material {
            key: "pack-test".into(),
            program: test_program("pack-test"),
            params: vec![[0.25, 0.5, 0.75, 1.]],
            textures: vec![Texture {
                width: 2,
                height: 1,
                rgba: Arc::new(vec![255, 0, 0, 255, 0, 255, 0, 255]),
            }],
            frozen_time: Some(3.5),
        };
        let bytes = material.pack(&Texture::white()).unwrap();
        let word = |i: usize| u32::from_le_bytes(bytes[i * 4..i * 4 + 4].try_into().unwrap());
        assert_eq!(f32::from_bits(word(0)), 3.5);
        assert_eq!((word(4), word(5), word(6)), (24, 1, 1));
        assert_eq!((word(12), word(13), word(14)), (25, 2, 1));
        assert_eq!(f32::from_bits(word(20)), 0.25);
        assert_eq!(bytes.len(), 27 * 4);
    }
    #[test]
    fn rejected_backend_restores_previous_program_without_reloading_scene() {
        let old = test_program("previous-test");
        let new = test_program("candidate-test");
        let path = "__test__/rollback.rxshadergraph".to_string();
        {
            let mut s = state().lock().unwrap();
            s.previous.insert(path.clone(), old.clone());
            s.programs.insert(path.clone(), new.clone());
            s.publications.insert(
                path.clone(),
                json!({"hash":new.compiled.hash,"state":"pending-render","validation":{}}),
            );
        }
        report_backend(
            &new.compiled.hash,
            "rurix",
            Err("GPU pipeline rejected fixture".into()),
        );
        report_backend(
            &new.compiled.hash,
            "rurix",
            Err("repeated failed draw".into()),
        );
        let mut s = state().lock().unwrap();
        assert!(Arc::ptr_eq(s.programs.get(&path).unwrap(), &old));
        assert_eq!(s.publications[&path]["state"], "backend-rejected");
        s.programs.remove(&path);
        s.publications.remove(&path);
    }
}
