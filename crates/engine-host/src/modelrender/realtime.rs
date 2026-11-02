//! RXS-0408..0410: Forge scene adapter; GPU rendering belongs to Rurix.
use crate::{
    modelrt,
    render_core::camera::model_eye,
    viewport::{m4_mul, EditorCamera, FramePixels, M4},
};
use assetd::model::{ModelBundle, ModelMaterial};
use forge_scene::Scene;
use rurix_render::realtime as rt;
use serde_json::{json, Value};
use std::{
    cell::RefCell,
    collections::HashMap,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc, Mutex, OnceLock,
    },
};

static EPOCH: AtomicU64 = AtomicU64::new(1);
static METRICS: OnceLock<Mutex<Value>> = OnceLock::new();
thread_local! {static STATE:RefCell<Option<State>>=const {RefCell::new(None)};}
struct InstanceBinding {
    entity: u64,
    node: usize,
    selected_node: Option<usize>,
    model: Arc<ModelBundle>,
    id: u64,
    previous: rt::Mat4,
}
struct MaterialBinding {
    entity: u64,
    primitive_material: usize,
    base: ModelMaterial,
    texture_map: Vec<usize>,
}
struct State {
    signature: u64,
    session: rt::RendererSession,
    bindings: Vec<InstanceBinding>,
    materials: Vec<MaterialBinding>,
    material_hash: u64,
    lighting_hash: u64,
    environment_hash: u64,
    previous_vp: rt::Mat4,
    camera_signature: Option<u64>,
    revision: u64,
    external_generation: Option<u64>,
    draws: usize,
    frames: u64,
    decal_materials: Vec<rt::Material>,
    decal_ids: Vec<(u64, usize)>,
    volume_hash: u64,
}
pub fn reset() {
    EPOCH.fetch_add(1, Ordering::Relaxed);
}
/// Keep GPU teardown on the ordinary Rust stack, before Windows TLS callbacks.
/// NVIDIA/Vulkan DLL unloading under the thread-exit loader lock can deadlock.
/// Every thread that uses this adapter must own this guard for its whole loop.
pub(crate) struct RealtimeThreadGuard;
impl Drop for RealtimeThreadGuard {
    fn drop(&mut self) {
        release_thread_resources();
    }
}
pub(crate) fn release_thread_resources() {
    STATE.with(|state| {
        let renderer = state.borrow_mut().take();
        drop(renderer);
    });
}

pub fn metrics() -> Value {
    METRICS
        .get_or_init(|| Mutex::new(json!({"status":"not_rendered"})))
        .lock()
        .unwrap()
        .clone()
}
fn component<'a>(scene: &'a Scene, kind: &str) -> Option<&'a Value> {
    scene
        .entities
        .iter()
        .find_map(|e| e.component(kind).filter(|c| c.enabled).map(|c| &c.props))
}
fn number(v: &Value, key: &str, default: f32) -> f32 {
    v.get(key)
        .and_then(Value::as_f64)
        .map(|v| v as f32)
        .unwrap_or(default)
}
fn vector<const N: usize>(v: &Value, key: &str, default: [f32; N]) -> [f32; N] {
    v.get(key)
        .and_then(Value::as_array)
        .filter(|a| a.len() == N)
        .map(|a| std::array::from_fn(|i| a[i].as_f64().map(|v| v as f32).unwrap_or(default[i])))
        .unwrap_or(default)
}
fn schema_default(kind: &str, field: &str) -> Value {
    let text = forge_scene::find_spec(kind)
        .and_then(|spec| spec.fields.iter().find(|f| f.name == field))
        .and_then(|f| f.default)
        .expect("renderer field must have a registered default");
    serde_json::from_str(text).expect("registered renderer default must be valid JSON")
}
fn default_number(kind: &str, field: &str) -> f32 {
    schema_default(kind, field)
        .as_f64()
        .expect("renderer default must be numeric") as f32
}
fn default_vector<const N: usize>(kind: &str, field: &str) -> [f32; N] {
    let value = schema_default(kind, field);
    let array = value.as_array().expect("renderer default must be a vector");
    std::array::from_fn(|i| array[i].as_f64().expect("numeric vector default") as f32)
}
fn linear(v: f32) -> f32 {
    if v <= 0.04045 {
        v / 12.92
    } else {
        ((v + 0.055) / 1.055).powf(2.4)
    }
}
fn cols(m: M4) -> rt::Mat4 {
    std::array::from_fn(|i| m[i % 4][i / 4])
}
fn hash(value: &impl serde::Serialize) -> u64 {
    crate::meshres::fnv1a64(&serde_json::to_vec(value).unwrap())
}
pub fn enabled(scene: &Scene) -> bool {
    component(scene, "RenderSettings")
        .and_then(|v| v.get("rurixRealtime"))
        .and_then(Value::as_bool)
        .unwrap_or(false)
}
fn scene_signature(scene: &Scene) -> u64 {
    let identities: Vec<_> = scene
        .entities
        .iter()
        .filter_map(|e| {
            e.component("ModelRenderer").filter(|c| c.enabled).map(|c| {
                json!([
                    e.id,
                    c.props.get("model"),
                    c.props.get("revision"),
                    c.props.get("nodeId")
                ])
            })
        })
        .collect();
    let decals: Vec<_> = scene
        .entities
        .iter()
        .filter_map(|e| {
            e.component("Decal")
                .filter(|c| c.enabled)
                .map(|c| json!([e.id, c.props]))
        })
        .collect();
    hash(&(
        crate::rpc::project_root().to_string_lossy(),
        &scene.scene_guid,
        &scene.name,
        EPOCH.load(Ordering::Relaxed),
        identities,
        decals,
        [
            "depthPrepass",
            "tileLights",
            "giEnabled",
            "reflectionsEnabled",
            "shadowsEnabled",
        ]
        .map(|key| component(scene, "RenderSettings").and_then(|v| v.get(key))),
    ))
}
fn translated_material(m: &ModelMaterial, texture_map: &[usize]) -> Result<rt::Material, String> {
    let mut textures = [None; 5];
    for (slot, index) in [
        m.base_color_texture,
        m.normal_texture,
        m.metallic_roughness_texture,
        m.occlusion_texture,
        m.emissive_texture,
    ]
    .into_iter()
    .enumerate()
    {
        textures[slot] = index
            .map(|i| {
                texture_map
                    .get(i)
                    .copied()
                    .ok_or_else(|| format!("REALTIME_MATERIAL: texture {i} missing"))
            })
            .transpose()?;
    }
    Ok(rt::Material {
        base_color: m.base_color,
        emissive: m.emissive,
        metallic: m.metallic,
        roughness: m.roughness,
        normal_scale: m.normal_scale,
        occlusion_strength: m.occlusion_strength,
        alpha_cutoff: m.alpha_cutoff,
        alpha_mode: match m.alpha_mode.as_str() {
            "OPAQUE" => rt::AlphaMode::Opaque,
            "MASK" => rt::AlphaMode::Mask,
            "BLEND" => rt::AlphaMode::Blend,
            _ => return Err("REALTIME_MATERIAL: unknown alpha mode".into()),
        },
        unlit: m.unlit,
        double_sided: m.double_sided,
        textures,
    })
}
fn material_values(
    scene: &Scene,
    bindings: &[MaterialBinding],
) -> Result<(Vec<rt::Material>, u64), String> {
    let mut materials = Vec::with_capacity(bindings.len());
    let mut fingerprints = Vec::new();
    for b in bindings {
        let entity = scene
            .entity(b.entity)
            .ok_or("REALTIME_SCENE: material entity disappeared")?;
        let props = &entity
            .component("ModelRenderer")
            .ok_or("REALTIME_SCENE: model disappeared")?
            .props;
        let overrides = props.get("materialOverrides").unwrap_or(&Value::Null);
        let m = crate::material_override::apply(&b.base, overrides, b.primitive_material)?;
        fingerprints.push(json!([
            b.entity,
            m.base_color,
            m.emissive,
            m.metallic,
            m.roughness,
            m.normal_scale,
            m.occlusion_strength,
            m.alpha_mode,
            m.alpha_cutoff,
            m.unlit,
            m.double_sided
        ]));
        materials.push(translated_material(&m, &b.texture_map)?);
    }
    Ok((materials, hash(&fingerprints)))
}
fn lights(scene: &Scene) -> Result<(Vec<rt::Light>, u64), String> {
    let mut out = Vec::new();
    let mut fingerprints = Vec::new();
    for e in &scene.entities {
        let Some(c) = e.component("Light").filter(|c| c.enabled) else {
            continue;
        };
        let p = &c.props;
        let params = e
            .component("LightParams")
            .filter(|c| c.enabled)
            .map(|c| &c.props)
            .unwrap_or(&Value::Null);
        let world = modelrt::entity_world(scene, e)?;
        fingerprints.push(json!([e.id, world, p, params]));
        let kind = match p.get("kind").and_then(Value::as_str).unwrap_or("point") {
            "point" => rt::LightKind::Point,
            "spot" => rt::LightKind::Spot,
            "rect" => rt::LightKind::Rect,
            other => {
                return Err(format!(
                    "REALTIME_UNSUPPORTED: light kind {other}; use point, spot or rect"
                ))
            }
        };
        let color = vector(p, "color", [1., 1., 1.]);
        let angle = number(
            params,
            "spotAngle",
            default_number("LightParams", "spotAngle"),
        );
        out.push(rt::Light {
            id: e.id,
            kind,
            position: modelrt::point(world, [0.; 3]),
            direction: modelrt::norm(modelrt::vector(world, [0., 0., -1.])),
            color: [linear(color[0]), linear(color[1]), linear(color[2])],
            intensity: number(p, "intensity", 100.),
            range: number(params, "range", default_number("LightParams", "range")),
            radius: number(params, "radius", default_number("LightParams", "radius")),
            inner_cos: number(
                params,
                "innerAngle",
                default_number("LightParams", "innerAngle"),
            )
            .to_radians()
            .cos(),
            outer_cos: angle.to_radians().cos(),
            rect_size: vector(params, "rectSize", [0.5, 0.12]),
            cast_shadow: p.get("castShadow").and_then(Value::as_bool).unwrap_or(true),
        });
    }
    Ok((out, hash(&fingerprints)))
}
fn environment(scene: &Scene) -> (rt::Environment, u64) {
    let Some(v) = component(scene, "Environment") else {
        return (rt::Environment::default(), hash(&Value::Null));
    };
    let c: [f32; 4] = vector(
        v,
        "ambientColor",
        default_vector("Environment", "ambientColor"),
    );
    let fog: [f32; 4] = vector(
        v,
        "fogLightColor",
        default_vector("Environment", "fogLightColor"),
    );
    let fog_energy = number(
        v,
        "fogLightEnergy",
        default_number("Environment", "fogLightEnergy"),
    );
    let env = rt::Environment {
        tonemap: if v.get("tonemap").and_then(Value::as_str) == Some("linear") {
            rt::Tonemap::Linear
        } else {
            rt::Tonemap::Aces
        },
        ambient_color: [linear(c[0]), linear(c[1]), linear(c[2])],
        ambient_energy: number(
            v,
            "ambientEnergy",
            default_number("Environment", "ambientEnergy"),
        ),
        exposure: number(v, "exposure", 1.),
        bloom_strength: if v
            .get("glowEnabled")
            .and_then(Value::as_bool)
            .unwrap_or(false)
        {
            number(
                v,
                "glowIntensity",
                default_number("Environment", "glowIntensity"),
            ) * number(
                v,
                "glowStrength",
                default_number("Environment", "glowStrength"),
            )
        } else {
            0.
        },
        bloom_threshold: number(v, "glowHdrThreshold", 1.),
        fog_color: [linear(fog[0]), linear(fog[1]), linear(fog[2])].map(|c| c * fog_energy),
        fog_density: if v
            .get("fogEnabled")
            .and_then(Value::as_bool)
            .unwrap_or(false)
        {
            number(v, "fogDensity", default_number("Environment", "fogDensity"))
        } else {
            0.
        },
    };
    (env, hash(v))
}
fn animated_worlds(
    scene: &Scene,
    e: &forge_scene::Entity,
    model: &ModelBundle,
) -> Result<Vec<M4>, String> {
    let a = modelrt::ancestor_component(scene, e, "Animator")
        .map(|a| &a.props)
        .unwrap_or(&Value::Null);
    modelrt::node_worlds(
        model,
        a.get("clip").and_then(Value::as_str).unwrap_or(""),
        number(a, "time", 0.),
        a.get("loop").and_then(Value::as_bool).unwrap_or(true),
    )
}
fn instance_world(scene: &Scene, b: &InstanceBinding, worlds: &[M4]) -> Result<rt::Mat4, String> {
    let e = scene
        .entity(b.entity)
        .ok_or("REALTIME_SCENE: instance disappeared")?;
    let mut world = modelrt::entity_world(scene, e)?;
    if let Some(ni) = b.selected_node {
        let rest = modelrt::node_worlds(&b.model, "", 0., false)?;
        world = m4_mul(world, modelrt::inverse(rest[ni])?);
    }
    Ok(cols(m4_mul(world, worlds[b.node])))
}

fn add_decals(
    scene: &Scene,
    snapshot: &mut rt::SceneSnapshot,
) -> Result<(Vec<rt::Material>, Vec<(u64, usize)>), String> {
    let mut materials = Vec::new();
    let mut ids = Vec::new();
    let root = crate::rpc::project_root();
    for e in &scene.entities {
        let Some(c) = e.component("Decal").filter(|c| c.enabled) else {
            continue;
        };
        let p = &c.props;
        let mut m = rt::Material::default();
        m.base_color = vector(p, "modulate", [1.; 4]);
        for i in 0..3 {
            m.base_color[i] = linear(m.base_color[i]);
        }
        m.emissive = [number(p, "emissionEnergy", 1.); 3];
        m.roughness = 1.;
        for (field, slots) in [
            ("textureAlbedo", vec![0]),
            ("textureNormal", vec![1]),
            ("textureOrm", vec![2, 3]),
            ("textureEmission", vec![4]),
        ] {
            if let Some(guid) = p
                .get(field)
                .and_then(Value::as_str)
                .filter(|s| !s.is_empty())
            {
                let t = crate::render_core::assets::load_tex_static_cached(&root, guid)
                    .ok_or_else(|| format!("REALTIME_DECAL: missing texture {guid}"))?;
                let id = snapshot.textures.len();
                snapshot.textures.push(rt::Texture {
                    width: t.w,
                    height: t.h,
                    rgba8: Arc::new(t.rgba.to_vec()),
                    wrap_s: 33071,
                    wrap_t: 33071,
                });
                for slot in slots {
                    m.textures[slot] = Some(id);
                }
            }
        }
        if m.textures[4].is_none() {
            m.emissive = [0.; 3];
        }
        ids.push((e.id, snapshot.materials.len() + materials.len()));
        materials.push(m);
    }
    snapshot.materials.extend(materials.iter().cloned());
    Ok((materials, ids))
}
fn volumes(
    scene: &Scene,
    decal_ids: &[(u64, usize)],
) -> Result<(Vec<rt::FogVolume>, Vec<rt::Decal>, u64), String> {
    let mut fogs = Vec::new();
    let mut decals = Vec::new();
    let mut fingerprints = Vec::new();
    let fog_enabled = component(scene, "Environment")
        .and_then(|v| v.get("volumetricFogEnabled"))
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let anisotropy = number(
        component(scene, "Environment").unwrap_or(&Value::Null),
        "volumetricFogAnisotropy",
        default_number("Environment", "volumetricFogAnisotropy"),
    );
    fingerprints.push(json!([anisotropy, fog_enabled]));
    for e in &scene.entities {
        if let Some(c) = e
            .component("FogVolume")
            .filter(|c| c.enabled && fog_enabled)
        {
            let p = &c.props;
            let world = modelrt::entity_world(scene, e)?;
            let a = vector(p, "albedo", [1.; 4]);
            let emission = vector(p, "emission", [0., 0., 0., 1.]);
            let shape = match p.get("shape").and_then(Value::as_str).unwrap_or("box") {
                "box" => rt::FogShape::Box,
                "ellipsoid" => rt::FogShape::Ellipsoid,
                other => return Err(format!("REALTIME_UNSUPPORTED: fog shape {other}")),
            };
            fogs.push(rt::FogVolume {
                id: e.id,
                transform: cols(world),
                size: vector(p, "size", [2.; 3]),
                shape,
                density: number(p, "density", 1.),
                albedo: [linear(a[0]), linear(a[1]), linear(a[2])],
                emission: [emission[0], emission[1], emission[2]],
                anisotropy,
                edge_fade: number(p, "edgeFade", default_number("FogVolume", "edgeFade")),
            });
            fingerprints.push(json!([e.id, world, p]));
        }
    }
    for &(id, material) in decal_ids {
        let e = scene.entity(id).ok_or("REALTIME_DECAL: entity missing")?;
        let p = &e
            .component("Decal")
            .ok_or("REALTIME_DECAL: component missing")?
            .props;
        let world = modelrt::entity_world(scene, e)?;
        decals.push(rt::Decal {
            id,
            transform: cols(world),
            size: vector(p, "size", [2.; 3]),
            material,
            opacity: number(p, "albedoMix", 1.),
        });
        fingerprints.push(json!([id, world, p]));
    }
    Ok((fogs, decals, hash(&fingerprints)))
}

fn build(scene: &Scene, signature: u64) -> Result<State, String> {
    let mut snapshot = rt::SceneSnapshot::default();
    let mut bindings = Vec::new();
    let mut material_bindings = Vec::new();
    let mut texture_maps: HashMap<String, Vec<usize>> = HashMap::new();
    let mut geometry: HashMap<String, (Arc<Vec<rt::Vertex>>, Arc<Vec<u32>>)> = HashMap::new();
    for e in &scene.entities {
        let Some(c) = e.component("ModelRenderer").filter(|c| c.enabled) else {
            continue;
        };
        let reference = c
            .props
            .get("model")
            .and_then(Value::as_str)
            .ok_or("REALTIME_MODEL: missing reference")?;
        let model = modelrt::load_revision(
            reference,
            c.props
                .get("revision")
                .and_then(Value::as_u64)
                .filter(|&r| r > 0),
        )?;
        let model_key = format!("{}:{}:{}", model.guid, model.revision, model.source_hash);
        let texture_map = texture_maps
            .entry(model_key.clone())
            .or_insert_with(|| {
                model
                    .textures
                    .iter()
                    .map(|t| {
                        let id = snapshot.textures.len();
                        snapshot.textures.push(rt::Texture {
                            width: t.width,
                            height: t.height,
                            rgba8: Arc::new(t.rgba.clone()),
                            wrap_s: t.wrap_s,
                            wrap_t: t.wrap_t,
                        });
                        id
                    })
                    .collect()
            })
            .clone();
        let selected = c
            .props
            .get("nodeId")
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
            .map(|id| {
                model
                    .nodes
                    .iter()
                    .position(|n| n.id == id)
                    .ok_or("REALTIME_MODEL: node missing")
            })
            .transpose()?;
        let worlds = animated_worlds(scene, e, &model)?;
        let mut stack = selected
            .map(|i| vec![i])
            .unwrap_or_else(|| model.roots.clone());
        let mut visited = std::collections::HashSet::new();
        while let Some(ni) = stack.pop() {
            if !visited.insert(ni) {
                continue;
            }
            let node = model.nodes.get(ni).ok_or("REALTIME_MODEL: bad node")?;
            if node.skin.is_some() {
                return Err("REALTIME_UNSUPPORTED: skinned meshes need the GPU skinning lane; rigid node animation is supported".into());
            }
            if selected.is_none() {
                stack.extend(&node.children)
            }
            for &pi in &node.primitives {
                let primitive = model
                    .primitives
                    .get(pi)
                    .ok_or("REALTIME_MODEL: bad primitive")?;
                let key = format!("{model_key}:{pi}");
                if !geometry.contains_key(&key) {
                    if primitive.normals.len() != primitive.positions.len()
                        || primitive.uv0.len() != primitive.positions.len()
                        || primitive.tangents.len() != primitive.positions.len()
                    {
                        return Err("REALTIME_MODEL: positions/normals/UV/tangents must have identical counts".into());
                    }
                    let vertices = primitive
                        .positions
                        .iter()
                        .enumerate()
                        .map(|(i, &position)| rt::Vertex {
                            position,
                            normal: primitive.normals[i],
                            uv: primitive.uv0[i],
                            tangent: primitive.tangents[i],
                        })
                        .collect();
                    geometry.insert(
                        key.clone(),
                        (Arc::new(vertices), Arc::new(primitive.indices.clone())),
                    );
                }
                let (vertices, indices) = geometry[&key].clone();
                let base = primitive
                    .material
                    .and_then(|m| model.materials.get(m))
                    .cloned()
                    .unwrap_or_else(crate::render_core::model::default_material);
                let material_index = material_bindings.len();
                material_bindings.push(MaterialBinding {
                    entity: e.id,
                    primitive_material: primitive.material.unwrap_or(0),
                    base,
                    texture_map: texture_map.clone(),
                });
                let id = hash(&(e.id, ni, pi));
                let mut binding = InstanceBinding {
                    entity: e.id,
                    node: ni,
                    selected_node: selected,
                    model: model.clone(),
                    id,
                    previous: rt::IDENTITY,
                };
                let world = instance_world(scene, &binding, &worlds)?;
                binding.previous = world;
                let mesh = snapshot.meshes.len();
                snapshot.meshes.push(rt::Mesh {
                    id,
                    vertices,
                    indices,
                    material: material_index,
                });
                snapshot.instances.push(rt::Instance {
                    id,
                    mesh,
                    transform: world,
                    previous_transform: world,
                });
                bindings.push(binding);
            }
        }
    }
    let (materials, material_hash) = material_values(scene, &material_bindings)?;
    let (lights, lighting_hash) = lights(scene)?;
    let (environment, environment_hash) = environment(scene);
    snapshot.materials = materials;
    snapshot.lights = lights;
    snapshot.environment = environment;
    snapshot.revision = 1;
    let (decal_materials, decal_ids) = add_decals(scene, &mut snapshot)?;
    let (fog_volumes, decals, volume_hash) = volumes(scene, &decal_ids)?;
    snapshot.fog_volumes = fog_volumes;
    snapshot.decals = decals;
    let draws = bindings.len();
    let mut config = rt::RendererConfig::default();
    config.depth_prepass = component(scene, "RenderSettings")
        .and_then(|v| v.get("depthPrepass"))
        .and_then(Value::as_bool)
        .unwrap_or(false);
    config.gi_enabled = component(scene, "RenderSettings")
        .and_then(|v| v.get("giEnabled"))
        .and_then(Value::as_bool)
        .unwrap_or(true);
    config.reflections_enabled = component(scene, "RenderSettings")
        .and_then(|v| v.get("reflectionsEnabled"))
        .and_then(Value::as_bool)
        .unwrap_or(true);
    config.shadows_enabled = component(scene, "RenderSettings")
        .and_then(|v| v.get("shadowsEnabled"))
        .and_then(Value::as_bool)
        .unwrap_or(true);
    config.tile_lights = component(scene, "RenderSettings")
        .and_then(|v| v.get("tileLights"))
        .and_then(Value::as_bool)
        .unwrap_or(true);
    let session = rt::RendererSession::new(snapshot, config)?;
    Ok(State {
        signature,
        session,
        bindings,
        materials: material_bindings,
        material_hash,
        lighting_hash,
        environment_hash,
        previous_vp: rt::IDENTITY,
        camera_signature: None,
        revision: 1,
        external_generation: None,
        draws,
        frames: 0,
        decal_materials,
        decal_ids,
        volume_hash,
    })
}
fn apply_scene_updates(scene: &Scene, s: &mut State) -> Result<(), String> {
    let revision = s.revision + 1;
    let mut delta = rt::SceneDelta {
        revision,
        ..Default::default()
    };
    let mut worlds: HashMap<u64, Vec<M4>> = HashMap::new();
    let mut pending_transforms = Vec::new();
    for (index, binding) in s.bindings.iter().enumerate() {
        if !worlds.contains_key(&binding.entity) {
            let entity = scene
                .entity(binding.entity)
                .ok_or("REALTIME_SCENE: entity missing")?;
            worlds.insert(
                binding.entity,
                animated_worlds(scene, entity, &binding.model)?,
            );
        }
        let world = instance_world(scene, binding, &worlds[&binding.entity])?;
        if world != binding.previous {
            delta.transforms.push(rt::TransformUpdate {
                instance_id: binding.id,
                transform: world,
            });
            pending_transforms.push((index, world));
        }
    }
    let (mut materials, material_hash) = material_values(scene, &s.materials)?;
    if material_hash != s.material_hash {
        materials.extend(s.decal_materials.iter().cloned());
        delta.materials = Some(materials);
    }
    let (lights, lighting_hash) = lights(scene)?;
    if lighting_hash != s.lighting_hash {
        delta.lights = Some(lights);
    }
    let (environment, environment_hash) = environment(scene);
    if environment_hash != s.environment_hash {
        delta.environment = Some(environment);
    }
    let (fogs, decals, volume_hash) = volumes(scene, &s.decal_ids)?;
    if volume_hash != s.volume_hash {
        delta.fog_volumes = Some(fogs);
        delta.decals = Some(decals);
    }
    // A rejected edit must stay rejected on every subsequent frame. The native
    // session validates transactionally; commit adapter fingerprints only then.
    s.session.apply_delta(delta)?;
    for (index, world) in pending_transforms {
        s.bindings[index].previous = world;
    }
    s.material_hash = material_hash;
    s.lighting_hash = lighting_hash;
    s.environment_hash = environment_hash;
    s.volume_hash = volume_hash;
    s.revision = revision;
    Ok(())
}
fn camera_signature(scene: &Scene, camera: &EditorCamera, scene_camera: bool) -> u64 {
    if scene_camera {
        let selected = scene.entities.iter().find_map(|entity| {
            entity
                .components
                .iter()
                .find(|c| c.ctype == "Camera" && c.enabled)
                .map(|c| {
                    json!([
                        entity.id,
                        c.props.get("projection"),
                        c.props.get("near"),
                        c.props.get("far")
                    ])
                })
        });
        hash(&("scene", selected))
    } else {
        hash(&("editor", camera.ortho))
    }
}
fn authoring_diagnostics(scene: &Scene) -> Vec<Value> {
    let mut diagnostics = Vec::new();
    for entity in &scene.entities {
        for c in entity.components.iter().filter(|c| c.enabled) {
            let mut warn = |field: &str, reason: &str| {
                diagnostics.push(json!({
                    "severity":"warning", "code":"REALTIME_UNSUPPORTED_SETTING", "entity":entity.id,
                    "component":c.ctype, "field":field, "message":reason,
                }))
            };
            let p = &c.props;
            match c.ctype.as_str() {
                "Environment" => {
                    for key in ["ssrEnabled","ssaoEnabled","ssilEnabled","sdfgiEnabled"] {
                        if p.get(key).and_then(Value::as_bool)==Some(true) { warn(key,"This setting is not consumed by the Rurix realtime pipeline; RayQuery effects have separate controls."); }
                    }
                    if p.get("glowEnabled").and_then(Value::as_bool)==Some(true) &&
                        p.get("glowBlendMode").and_then(Value::as_str).unwrap_or("screen")!="additive" {
                        warn("glowBlendMode","Rurix realtime currently composites HDR bloom additively.");
                    }
                    if p.get("volumetricFogEnabled").and_then(Value::as_bool)==Some(true) &&
                        number(p,"volumetricFogDensity",default_number("Environment","volumetricFogDensity"))!=0. {
                        warn("volumetricFogDensity","Only authored local FogVolume density is consumed; global volumetric density is unsupported.");
                    }
                    for key in ["fogHeightDensity","fogAerialPerspective"] {
                        if number(p,key,0.)!=0. { warn(key,"Rurix realtime currently supports exponential distance fog and local volumes."); }
                    }
                    for key in ["backgroundMode","ambientSource"] {
                        if p.get(key).and_then(Value::as_str).is_some_and(|v| v=="sky") { warn(key,"Sky radiance assets are not consumed; the realtime environment uses its ambient color."); }
                    }
                }
                "CameraAttributes" => {
                    for key in ["autoExposureEnabled","dofBlurFarEnabled","dofBlurNearEnabled"] {
                        if p.get(key).and_then(Value::as_bool)==Some(true) { warn(key,"Rurix realtime uses fixed exposure and currently has no depth-of-field pass."); }
                    }
                }
                "ReflectionProbe" => warn("enabled","Rurix realtime traces the current scene; baked/local reflection-probe settings are not consumed."),
                "LightParams" => {
                    for key in ["negative","attenuation","spotAttenuation","specular","indirectEnergy","volumetricFogEnergy","shadowOpacity","size"] {
                        if let Some(value)=p.get(key) {
                            let default=forge_scene::find_spec("LightParams").and_then(|s|s.fields.iter().find(|f|f.name==key))
                                .and_then(|f|f.default).and_then(|v|serde_json::from_str::<Value>(v).ok());
                            if default.as_ref()!=Some(value) { warn(key,"This nondefault LightParams field is not implemented by the realtime light model."); }
                        }
                    }
                }
                _ => {}
            }
        }
    }
    diagnostics
}
pub fn render(
    scene: &Scene,
    cam: &EditorCamera,
    _selected: Option<u64>,
    width: u32,
    height: u32,
    want_readback: bool,
    want_stats: bool,
    vp: Option<M4>,
) -> Result<FramePixels, String> {
    if scene
        .entities
        .iter()
        .flat_map(|e| &e.components)
        .any(|c| c.enabled && matches!(c.ctype.as_str(), "MeshRenderer" | "ParticleEmitter"))
    {
        return Err("REALTIME_UNSUPPORTED: MeshRenderer/ParticleEmitter require an implemented realtime geometry adapter".into());
    }
    if let Some(tone) = component(scene, "Environment")
        .and_then(|v| v.get("tonemap"))
        .and_then(Value::as_str)
    {
        if !matches!(tone, "linear" | "aces") {
            return Err(format!("REALTIME_UNSUPPORTED: tonemap {tone}"));
        }
    }
    let started = std::time::Instant::now();
    let signature = scene_signature(scene);
    let current = cols(vp.unwrap_or_else(|| cam.view_proj(width as f32 / height as f32)));
    STATE.with(|cell|{
        let mut cache=cell.borrow_mut();if cache.as_ref().is_none_or(|s|s.signature!=signature){*cache=Some(build(scene,signature)?);}
        let s=cache.as_mut().unwrap();
        let update_started=std::time::Instant::now();
        apply_scene_updates(scene, s)?;
        let scene_update_ms=update_started.elapsed().as_secs_f64()*1000.;
        let mut output_slot=None;
        #[cfg(windows)] {
            let exact=crate::viewport::rurix::FORCE_EXACT_SIZE.load(Ordering::Relaxed);
            if !exact {
                if let Some(outputs)=crate::share::native_output_infos(){
                    let generation=outputs[0].generation;
                    if s.external_generation!=Some(generation){
                        let luid=crate::share::adapter_luid().ok_or("REALTIME_NATIVE: adapter LUID missing")?;
                        s.session.attach_external_outputs(outputs.map(|o|rt::ExternalOutput{handle:o.handle as usize,size:o.size,width:o.width,height:o.height,row_pitch:o.row_pitch,adapter_luid:luid}))?;
                        s.external_generation=Some(generation);
                    }
                    output_slot=crate::share::vk_import_info().map(|o|o.slot);
                }else if s.external_generation.take().is_some(){s.session.detach_external_outputs();}
            }else if s.external_generation.take().is_some(){s.session.detach_external_outputs();}
        }
        let settings=component(scene,"RenderSettings").unwrap_or(&Value::Null);
        let scale=number(settings,"renderScale",2./3.);
        let eye=model_eye(scene,cam,vp.is_some());
        let camera_signature=camera_signature(scene, cam, vp.is_some());
        let cut=s.frames==0 || s.camera_signature!=Some(camera_signature) || current.iter().zip(s.previous_vp).any(|(a,b)|(a-b).abs()>2.0);
        let result=s.session.render(&rt::FrameInput{view_projection:current,previous_view_projection:s.previous_vp,eye,output_extent:[width,height],render_scale:scale,delta_seconds:1./60.,reset_history:cut,readback:want_readback,allow_offscreen:crate::viewport::rurix::FORCE_EXACT_SIZE.load(Ordering::Relaxed)&&!want_readback,output_slot})?;
        s.previous_vp=current;s.camera_signature=Some(camera_signature);s.frames+=1;
        let imported=result.external_output.is_some();let nonzero=if want_stats{result.rgba8.chunks_exact(4).filter(|p|p[0]!=0||p[1]!=0||p[2]!=0).count()}else{0};
        let mut stage_totals:std::collections::BTreeMap<&str,(f64,u32)>=Default::default();
        for(name,ms)in &result.passes{let v=stage_totals.entry(name).or_default();v.0+=ms;v.1+=1;}
        let stages:Vec<_>=stage_totals.into_iter().map(|(name,(gpu_ms,passes))|json!({"name":name,"gpuMs":gpu_ms,"passes":passes})).collect();
        let value=json!({"renderer":"rurix-realtime","frame":s.frames,"sceneRevision":s.revision,"outputExtent":[width,height],"renderScale":scale,"internalExtent":result.internal_extent,"frameGeneration":false,
            "device":result.device_name,"gpuMs":result.gpu_ms,"gpuPassSumMs":result.gpu_ms,"gpuTimingScope":"sum_of_pass_timestamps_excludes_uploads_tlas_and_some_barriers","stages":stages,"cpuRecordMs":result.cpu_record_ms,"cpuSubmitMs":result.cpu_submit_ms,"cpuWaitMs":result.cpu_wait_ms,
            "adapterWallMs":started.elapsed().as_secs_f64()*1000.,"sceneUpdateMs":scene_update_ms,"allocatedBytes":result.allocated_bytes,
            "memoryHeaps":result.heaps.iter().map(|h|json!({"heapIndex":h.heap_index,"budgetBytes":h.budget_bytes,"driverUsageBytes":h.driver_usage_bytes,"sessionLedgerBytes":h.ledger_bytes})).collect::<Vec<_>>(),
            "triangles":result.triangle_count,"validationErrors":result.validation_errors,"historyReset":result.history_reset,"pixelReadback":want_readback,"nativeOutput":imported,
            "diagnostics":authoring_diagnostics(scene)});
        *METRICS.get_or_init(||Mutex::new(Value::Null)).lock().unwrap()=value;
        Ok(FramePixels{width:result.width,height:result.height,rgba8:result.rgba8,device_name:result.device_name,draws:s.draws,truncated:false,nonzero,triangles:result.triangle_count,mesh_fallbacks:0,mesh_classes:s.draws,imported})
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn author_rgb_light_color_is_not_replaced_with_white() {
        let mut scene = Scene::new("colored-light");
        scene.entities.push(forge_scene::Entity {
            entity_guid: None,
            id: 1,
            name: "neon".into(),
            transform: Default::default(),
            components: vec![forge_scene::Component::new(
                "Light",
                json!({"kind":"point","color":[0.0,0.5,1.0],"intensity":120.0,"castShadow":true}),
            )],
        });
        let (lights, _) = lights(&scene).unwrap();
        assert_eq!(lights[0].color[0], 0.);
        assert_eq!(lights[0].color[2], 1.);
        assert!((lights[0].color[1] - 0.21404114).abs() < 1e-6);
    }
    #[test]
    fn realtime_is_an_explicit_scene_choice() {
        assert!(!enabled(&Scene::new("legacy")));
    }
    #[test]
    fn rejected_scene_updates_remain_rejected_until_corrected() {
        let mut authored = Scene::new("transaction");
        authored.entities.push(forge_scene::Entity {
            entity_guid: None,
            id: 1,
            name: "light".into(),
            transform: Default::default(),
            components: vec![forge_scene::Component::new(
                "Light",
                json!({"intensity":10.0}),
            )],
        });
        let (initial_lights, lighting_hash) = lights(&authored).unwrap();
        let (initial_environment, environment_hash) = environment(&authored);
        let (_, material_hash) = material_values(&authored, &[]).unwrap();
        let (_, _, volume_hash) = volumes(&authored, &[]).unwrap();
        let vertices = [[0., 0., 0.], [1., 0., 0.], [0., 1., 0.]]
            .map(|position| rt::Vertex {
                position,
                normal: [0., 0., 1.],
                tangent: [1., 0., 0., 1.],
                uv: [0., 0.],
            })
            .to_vec();
        let session = rt::RendererSession::new(
            rt::SceneSnapshot {
                revision: 1,
                meshes: vec![rt::Mesh {
                    id: 1,
                    vertices: Arc::new(vertices),
                    indices: Arc::new(vec![0, 1, 2]),
                    material: 0,
                }],
                instances: vec![rt::Instance {
                    id: 1,
                    mesh: 0,
                    transform: rt::IDENTITY,
                    previous_transform: rt::IDENTITY,
                }],
                materials: vec![rt::Material::default()],
                lights: initial_lights,
                environment: initial_environment,
                ..Default::default()
            },
            rt::RendererConfig::default(),
        )
        .unwrap();
        let mut state = State {
            signature: 0,
            session,
            bindings: vec![],
            materials: vec![],
            material_hash,
            lighting_hash,
            environment_hash,
            previous_vp: rt::IDENTITY,
            camera_signature: None,
            revision: 1,
            external_generation: None,
            draws: 1,
            frames: 0,
            decal_materials: vec![],
            decal_ids: vec![],
            volume_hash,
        };
        authored.entities[0].components[0].props["intensity"] = json!(-1.0);
        for _ in 0..2 {
            assert!(apply_scene_updates(&authored, &mut state)
                .unwrap_err()
                .contains("REALTIME_LIGHT"));
            assert_eq!(state.lighting_hash, lighting_hash);
            assert_eq!(state.revision, 1);
        }
        authored.entities[0].components[0].props["intensity"] = json!(20.0);
        apply_scene_updates(&authored, &mut state).unwrap();
        assert_ne!(state.lighting_hash, lighting_hash);
        assert_eq!(state.revision, 2);
    }
    #[test]
    fn camera_and_scene_identity_reset_history_without_resetting_continuous_motion() {
        let mut scene = Scene::new("first-shot");
        let camera = EditorCamera::default();
        let identity = scene_signature(&scene);
        scene.name = "second-shot".into();
        assert_ne!(identity, scene_signature(&scene));
        assert_ne!(
            camera_signature(&scene, &camera, false),
            camera_signature(&scene, &camera, true)
        );
        let mut moved = camera;
        moved.target[0] += 0.01;
        assert_eq!(
            camera_signature(&scene, &camera, false),
            camera_signature(&scene, &moved, false)
        );
    }
    #[test]
    fn environment_rgba_and_registered_defaults_are_consumed() {
        let mut scene = Scene::new("environment-defaults");
        scene.entities.push(forge_scene::Entity {
            entity_guid: None,
            id: 1,
            name: "environment".into(),
            transform: Default::default(),
            components: vec![forge_scene::Component::new(
                "Environment",
                json!({
                    "ambientColor":[0.0,0.5,1.0,1.0],
                    "fogLightColor":[1.0,0.5,0.0,1.0]
                }),
            )],
        });
        let (env, _) = environment(&scene);
        assert_eq!(env.ambient_color, [0.0, linear(0.5), 1.0]);
        assert_eq!(env.fog_color, [1.0, linear(0.5), 0.0]);
        assert_eq!(
            env.ambient_energy,
            default_number("Environment", "ambientEnergy")
        );
        scene.entities[0].components[0].props["fogLightEnergy"] = json!(0.6);
        assert_eq!(
            environment(&scene).0.fog_color,
            [1.0, linear(0.5), 0.0].map(|v| v * 0.6)
        );
    }
    #[test]
    fn volume_enable_edge_fade_and_unsupported_controls_are_explicit() {
        let mut scene = Scene::new("volume-controls");
        scene.entities.push(forge_scene::Entity {
            entity_guid: None,
            id: 1,
            name: "smoke".into(),
            transform: Default::default(),
            components: vec![forge_scene::Component::new(
                "FogVolume",
                json!({"edgeFade":0.12}),
            )],
        });
        assert!(volumes(&scene, &[]).unwrap().0.is_empty());
        scene.entities.push(forge_scene::Entity {
            entity_guid: None,
            id: 2,
            name: "environment".into(),
            transform: Default::default(),
            components: vec![forge_scene::Component::new(
                "Environment",
                json!({"volumetricFogEnabled":true,"volumetricFogDensity":0.0,"ssaoEnabled":true}),
            )],
        });
        assert_eq!(volumes(&scene, &[]).unwrap().0[0].edge_fade, 0.12);
        let diagnostics = authoring_diagnostics(&scene);
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0]["field"], "ssaoEnabled");
    }
    #[test]
    fn transform_conversion_keeps_translation_in_last_column() {
        let mut m = modelrt::IDENTITY;
        m[0][3] = 3.;
        m[1][3] = 7.;
        assert_eq!(&cols(m)[12..15], &[3., 7., 0.]);
    }
}
