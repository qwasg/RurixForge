//! 模型腿的 CPU 部分(02 §3.2),自 modelrender.rs 整体搬来:`collect`(含 `legacy_draw`、材质贴图打包
//! 与预算记账——不拆函数,拆开会改变预算超限时的报错时机)、`bounds`、`pick`。
//! push constants 打包 `pc` 也随 `collect` 搬来:它是 `collect` / `legacy_draw` 的被调方,留在 rurix 侧
//! 会让 render_core 反向依赖 rurix 模块(违反 02 §3 P3)。GPU 会话、上传与出帧仍在 modelrender.rs。

use std::sync::{Arc, Mutex, OnceLock};

use assetd::model::{ModelBundle, ModelMaterial};
use forge_scene::{Entity, Scene};

use crate::modelrt;

use super::assets::{cube_mesh_bytes, load_tex_static_cached, material_albedo_guid};
use super::math::{m4_mul, trs_model};
use super::sprite::{resolve_sprite_render, sprite_render_transform, SpriteRenderInfo};

pub(crate) const MAX_DRAWS: usize = 2048;
pub(crate) const MAX_VERTEX_BYTES: usize = 256 * 1024 * 1024;
const MAX_TEXTURE_BYTES: usize = 256 * 1024 * 1024;
#[cfg_attr(not(feature = "backend-rurix"), allow(dead_code))]
pub(crate) struct Draw {
    pub(crate) owner: u64,
    pub(crate) key: String,
    pub(crate) vertices: Vec<u8>,
    pub(crate) textures: Arc<Vec<u8>>,
    pub(crate) pc: Vec<u8>,
    pub(crate) graph: Option<Arc<crate::shader::Material>>,
    pub(crate) blend: bool,
    distance: f32,
}

pub(crate) static PACKED: OnceLock<Mutex<std::collections::HashMap<String, Arc<Vec<u8>>>>> = OnceLock::new();

pub fn bounds(scene: &Scene) -> Result<([f32; 3], f32), String> {
    let draws = collect(scene, [0.; 3], None)?;
    let mut min = [f32::INFINITY; 3];
    let mut max = [f32::NEG_INFINITY; 3];
    for d in draws {
        for v in d.vertices.chunks_exact(48) {
            for i in 0..3 {
                let f = f32::from_le_bytes(v[i * 4..i * 4 + 4].try_into().unwrap());
                min[i] = min[i].min(f);
                max[i] = max[i].max(f);
            }
        }
    }
    if !min[0].is_finite() {
        return Err("model has no visible vertices".into());
    }
    Ok((
        std::array::from_fn(|i| (min[i] + max[i]) * 0.5),
        (0..3)
            .map(|i| (max[i] - min[i]).powi(2))
            .sum::<f32>()
            .sqrt()
            * 0.5,
    ))
}
pub fn pick(scene: &Scene, origin: [f32; 3], dir: [f32; 3]) -> Option<(u64, f32)> {
    let draws = collect(scene, origin, None).ok()?;
    let mut best = None;
    let sub = |a: [f32; 3], b: [f32; 3]| std::array::from_fn(|i| a[i] - b[i]);
    let cross = |a: [f32; 3], b: [f32; 3]| {
        [
            a[1] * b[2] - a[2] * b[1],
            a[2] * b[0] - a[0] * b[2],
            a[0] * b[1] - a[1] * b[0],
        ]
    };
    let dot = |a: [f32; 3], b: [f32; 3]| a.iter().zip(b).map(|(a, b)| a * b).sum::<f32>();
    for d in draws {
        for triangle in d.vertices.chunks_exact(144) {
            let p = |vi: usize| {
                std::array::from_fn(|i| {
                    f32::from_le_bytes(
                        triangle[vi * 48 + i * 4..vi * 48 + i * 4 + 4]
                            .try_into()
                            .unwrap(),
                    )
                })
            };
            let a = p(0);
            let e1 = sub(p(1), a);
            let e2 = sub(p(2), a);
            let h = cross(dir, e2);
            let det = dot(e1, h);
            if det.abs() < 1e-8 {
                continue;
            }
            let s = sub(origin, a);
            let u = dot(s, h) / det;
            if !(0. ..=1.).contains(&u) {
                continue;
            }
            let q = cross(s, e1);
            let v = dot(dir, q) / det;
            if v < 0. || u + v > 1. {
                continue;
            }
            let t = dot(e2, q) / det;
            if t >= 0. && best.is_none_or(|(_, old)| t < old) {
                best = Some((d.owner, t));
            }
        }
    }
    best
}

pub fn default_material() -> ModelMaterial {
    ModelMaterial {
        guid: String::new(),
        name: "default".into(),
        base_color: [0.65, 0.7, 0.75, 1.],
        metallic: 0.,
        roughness: 0.8,
        emissive: [0.; 3],
        base_color_texture: None,
        normal_texture: None,
        metallic_roughness_texture: None,
        occlusion_texture: None,
        emissive_texture: None,
        normal_scale: 1.,
        occlusion_strength: 1.,
        double_sided: true,
        alpha_mode: "OPAQUE".into(),
        alpha_cutoff: 0.5,
        unlit: false,
    }
}
fn material_bytes(model: &ModelBundle, m: &ModelMaterial) -> Result<Arc<Vec<u8>>, String> {
    let key = format!(
        "{}:{}:{}:{:?}",
        model.guid,
        model.revision,
        model.source_hash,
        [
            m.base_color_texture,
            m.normal_texture,
            m.metallic_roughness_texture,
            m.occlusion_texture,
            m.emissive_texture
        ]
    );
    let cache = PACKED.get_or_init(|| Mutex::new(std::collections::HashMap::new()));
    if let Some(p) = cache.lock().unwrap().get(&key).cloned() {
        return Ok(p);
    }
    let length = packed_length(model, m)?;
    if length > MAX_TEXTURE_BYTES {
        return Err("MODEL_BUDGET: material textures exceed 256 MiB".into());
    }
    let mut data = vec![0u8; 5 * 8 * 4];
    data.reserve(length - data.len());
    for (s, index) in [
        m.base_color_texture,
        m.normal_texture,
        m.metallic_roughness_texture,
        m.occlusion_texture,
        m.emissive_texture,
    ]
    .iter()
    .enumerate()
    {
        if let Some(i) = index {
            let t = model.textures.get(*i).ok_or("texture index out of range")?;
            let meta = [
                (data.len() / 4) as u32,
                t.width,
                t.height,
                t.wrap_s,
                t.wrap_t,
                t.mag_filter.or(t.min_filter).unwrap_or(9729),
                0,
                0,
            ];
            for (j, x) in meta.iter().enumerate() {
                data[(s * 8 + j) * 4..(s * 8 + j + 1) * 4].copy_from_slice(&x.to_le_bytes());
            }
            data.extend_from_slice(&t.rgba);
        }
    }
    let data = Arc::new(data);
    let mut cache = cache.lock().unwrap();
    if cache.values().map(|v| v.len()).sum::<usize>() + data.len() > MAX_TEXTURE_BYTES {
        cache.clear();
    }
    cache.insert(key, data.clone());
    Ok(data)
}
fn packed_length(model: &ModelBundle, m: &ModelMaterial) -> Result<usize, String> {
    [
        m.base_color_texture,
        m.normal_texture,
        m.metallic_roughness_texture,
        m.occlusion_texture,
        m.emissive_texture,
    ]
    .into_iter()
    .flatten()
    .try_fold(160usize, |n, i| {
        n.checked_add(
            model
                .textures
                .get(i)
                .ok_or("texture index out of range")?
                .rgba
                .len(),
        )
        .ok_or_else(|| "MODEL_BUDGET: texture length overflow".into())
    })
}
pub(crate) fn charge(used: &mut usize, amount: usize, limit: usize, kind: &str) -> Result<(), String> {
    let next = used
        .checked_add(amount)
        .ok_or_else(|| format!("MODEL_BUDGET: {kind} size overflow"))?;
    if next > limit {
        return Err(format!("MODEL_BUDGET: {kind} exceeds {limit} bytes"));
    }
    *used = next;
    Ok(())
}
pub(crate) fn pc(m: &ModelMaterial, eye: [f32; 3], selected: bool) -> Vec<u8> {
    let mut b = Vec::new();
    for f in m.base_color.into_iter().chain(m.emissive).chain([
        m.roughness,
        eye[0],
        eye[1],
        eye[2],
        m.metallic,
        m.normal_scale,
        m.occlusion_strength,
        if m.alpha_mode == "MASK" {
            1.
        } else if m.alpha_mode == "BLEND" {
            2.
        } else {
            0.
        },
        m.alpha_cutoff,
        if m.unlit { 1. } else { 0. },
        if m.double_sided { 1. } else { 0. },
        0.,
        if selected { 1. } else { 0. },
    ]) {
        b.extend_from_slice(&f.to_le_bytes());
    }
    b
}
pub(crate) fn collect(scene: &Scene, eye: [f32; 3], selected: Option<u64>) -> Result<Vec<Draw>, String> {
    let mut draws = Vec::new();
    let (mut vertex_used, mut texture_used) = (0usize, 0usize);
    let mut texture_keys = std::collections::HashSet::new();
    for e in &scene.entities {
        if let Some(c) = e.component("ModelRenderer").filter(|c| c.enabled) {
            let reference = c
                .props
                .get("model")
                .and_then(|v| v.as_str())
                .ok_or("ModelRenderer.model missing")?;
            let model = modelrt::load_revision(
                reference,
                c.props
                    .get("revision")
                    .and_then(|v| v.as_u64())
                    .filter(|r| *r > 0),
            )?;
            let a = modelrt::ancestor_component(scene, e, "Animator");
            let clip = a
                .and_then(|a| a.props.get("clip"))
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let time = a
                .and_then(|a| a.props.get("time"))
                .and_then(|v| v.as_f64())
                .unwrap_or(0.) as f32;
            let looped = a
                .and_then(|a| a.props.get("loop"))
                .and_then(|v| v.as_bool())
                .unwrap_or(true);
            let worlds = modelrt::node_worlds(&model, clip, time, looped)?;
            let node_id = c.props.get("nodeId").and_then(|v| v.as_str()).unwrap_or("");
            let selected_node = if node_id.is_empty() {
                None
            } else {
                Some(
                    model
                        .nodes
                        .iter()
                        .position(|n| n.id == node_id)
                        .ok_or_else(|| format!("MODEL_NODE_NOT_FOUND: {node_id}"))?,
                )
            };
            let world = modelrt::entity_world(scene, e)?;
            let world = if let Some(ni) = selected_node {
                let rest = modelrt::node_worlds(&model, "", 0., false)?;
                m4_mul(world, modelrt::inverse(rest[ni])?)
            } else {
                world
            };
            let mut stack = selected_node
                .map(|i| vec![i])
                .unwrap_or_else(|| model.roots.clone());
            let mut visited = std::collections::HashSet::new();
            while let Some(ni) = stack.pop() {
                if !visited.insert(ni) {
                    continue;
                }
                let n = model.nodes.get(ni).ok_or("root node out of range")?;
                if selected_node.is_none() {
                    stack.extend(&n.children);
                }
                for &pi in &n.primitives {
                    let p = model.primitives.get(pi).ok_or("primitive out of range")?;
                    let default = default_material();
                    let m = p
                        .material
                        .and_then(|i| model.materials.get(i))
                        .unwrap_or(&default);
                    let overridden = crate::material_override::apply(
                        m,
                        c.props
                            .get("materialOverrides")
                            .unwrap_or(&serde_json::Value::Null),
                        p.material.unwrap_or(0),
                    )?;
                    let m = &overridden;
                    if draws.len() >= MAX_DRAWS {
                        return Err("MODEL_BUDGET: more than 2048 draw primitives".into());
                    }
                    charge(
                        &mut vertex_used,
                        p.indices
                            .len()
                            .checked_mul(48)
                            .ok_or("MODEL_BUDGET: vertex length overflow")?,
                        MAX_VERTEX_BYTES,
                        "vertex data",
                    )?;
                    let texture_key = format!(
                        "{}:{}:{}:{:?}",
                        model.guid,
                        model.revision,
                        model.source_hash,
                        [
                            m.base_color_texture,
                            m.normal_texture,
                            m.metallic_roughness_texture,
                            m.occlusion_texture,
                            m.emissive_texture
                        ]
                    );
                    if texture_keys.insert(texture_key) {
                        charge(
                            &mut texture_used,
                            packed_length(&model, m)?,
                            MAX_TEXTURE_BYTES,
                            "texture data",
                        )?;
                    }
                    let vertices = modelrt::vertices(&model, ni, pi, &worlds, world)?;
                    let distance = vertex_distance(&vertices, eye);
                    draws.push(Draw {
                        owner: e.id,
                        key: format!(
                            "{}:{}:{}:{}:{}:{}",
                            e.id, model.guid, model.revision, model.source_hash, ni, pi
                        ),
                        vertices,
                        textures: material_bytes(&model, m)?,
                        graph: crate::shader::model(e, &p.material.unwrap_or(0).to_string())?,
                        pc: pc(m, eye, selected == Some(e.id)),
                        blend: m.alpha_mode == "BLEND",
                        distance,
                    });
                }
            }
        } else if let Some(d) = legacy_draw(
            scene,
            e,
            eye,
            selected == Some(e.id),
            &mut vertex_used,
            &mut texture_used,
        )? {
            draws.push(d);
        }
    }
    if draws.len() > MAX_DRAWS {
        return Err(format!(
            "MODEL_BUDGET: {} draws exceeds {MAX_DRAWS}",
            draws.len()
        ));
    }
    if draws.iter().map(|d| d.vertices.len()).sum::<usize>() > MAX_VERTEX_BYTES {
        return Err("MODEL_BUDGET: vertex data exceeds 256 MiB".into());
    }
    draws.sort_by(|a, b| {
        a.blend.cmp(&b.blend).then_with(|| {
            if a.blend {
                b.distance.total_cmp(&a.distance)
            } else {
                std::cmp::Ordering::Equal
            }
        })
    });
    Ok(draws)
}
fn vertex_distance(vertices: &[u8], eye: [f32; 3]) -> f32 {
    let mut center = [0.; 3];
    let n = vertices.len() / 48;
    for v in vertices.chunks_exact(48) {
        for i in 0..3 {
            center[i] +=
                f32::from_le_bytes(v[i * 4..i * 4 + 4].try_into().unwrap()) / n.max(1) as f32;
        }
    }
    (0..3).map(|i| (center[i] - eye[i]).powi(2)).sum()
}
fn legacy_draw(
    scene: &Scene,
    e: &Entity,
    eye: [f32; 3],
    selected: bool,
    vertex_used: &mut usize,
    texture_used: &mut usize,
) -> Result<Option<Draw>, String> {
    let sp = e.component("Sprite").filter(|c| c.enabled);
    let info = if let Some(sp) = sp {
        resolve_sprite_render(sp, &crate::rpc::project_root())
    } else {
        e.component("MeshRenderer")
            .filter(|c| c.enabled)
            .and_then(|c| c.props["material"].as_str())
            .and_then(|mat| material_albedo_guid(mat, &crate::rpc::project_root()))
            .and_then(|guid| load_tex_static_cached(&crate::rpc::project_root(), &guid))
            .map(|tex| SpriteRenderInfo {
                tex,
                uv_rect: [0., 0., 1., 1.],
                frame_px: [tex.w as f32, tex.h as f32],
                pivot: [0.5, 0.5],
            })
    };
    if let Some(info) = info {
        charge(vertex_used, 6 * 48, MAX_VERTEX_BYTES, "vertex data")?;
        charge(
            texture_used,
            160 + info.tex.rgba.len(),
            MAX_TEXTURE_BYTES,
            "texture data",
        )?;
        let mut mat = default_material();
        mat.unlit = true;
        mat.base_color = sp
            .and_then(|s| s.props.get("tint"))
            .and_then(|v| serde_json::from_value(v.clone()).ok())
            .unwrap_or([1.; 4]);
        mat.alpha_mode = "MASK".into();
        mat.alpha_cutoff = 0.02;
        let local = sprite_render_transform(e).unwrap_or(e.transform);
        let world = m4_mul(
            modelrt::entity_world(scene, e)?,
            m4_mul(
                modelrt::inverse(trs_model(&e.transform))?,
                trs_model(&local),
            ),
        );
        let mut vertices = Vec::new();
        for (corner, uv) in [
            ([-0.5, -0.5], [0., 1.]),
            ([0.5, -0.5], [1., 1.]),
            ([0.5, 0.5], [1., 0.]),
            ([-0.5, -0.5], [0., 1.]),
            ([0.5, 0.5], [1., 0.]),
            ([-0.5, 0.5], [0., 0.]),
        ] {
            let p = modelrt::point(world, [corner[0], corner[1], 0.]);
            let mut uv = uv;
            for i in 0..2 {
                if sp.is_some_and(|s| {
                    s.props[if i == 0 { "flipX" } else { "flipY" }].as_bool() == Some(true)
                }) {
                    uv[i] = 1. - uv[i];
                }
                uv[i] = info.uv_rect[i] + uv[i] * info.uv_rect[i + 2];
            }
            for f in [p[0], p[1], p[2], 0., 0., 1., uv[0], uv[1], 1., 0., 0., 1.] {
                vertices.extend_from_slice(&f.to_le_bytes());
            }
        }
        let mut textures = vec![0; 160];
        for (i, v) in [40, info.tex.w, info.tex.h, 33071, 33071, 9728, 0, 0]
            .iter()
            .enumerate()
        {
            textures[i * 4..i * 4 + 4].copy_from_slice(&u32::to_le_bytes(*v));
        }
        textures.extend_from_slice(info.tex.rgba);
        let mut params = pc(&mat, eye, selected);
        params[72..76].copy_from_slice(&1f32.to_le_bytes());
        return Ok(Some(Draw {
            owner: e.id,
            key: format!("legacy-sprite:{}", e.id),
            vertices,
            textures: Arc::new(textures),
            graph: crate::shader::sprite(e)?,
            pc: params,
            blend: false,
            distance: 0.,
        }));
    }
    let Some(c) = e.component("MeshRenderer").filter(|c| c.enabled) else {
        return Ok(None);
    };
    if let Some(graph)=crate::shader::mesh(e)?{let world=modelrt::entity_world(scene,e)?;let nm=modelrt::normal_matrix(world)?;let mut vertices=Vec::new();for v in crate::shader::mesh_vertices(e)?.chunks_exact(12){let p=modelrt::point(world,[v[0],v[1],v[2]]);let n=modelrt::norm(modelrt::vector(nm,[v[3],v[4],v[5]]));let t=modelrt::norm(modelrt::vector(nm,[v[8],v[9],v[10]]));for f in[p[0],p[1],p[2],n[0],n[1],n[2],v[6],v[7],t[0],t[1],t[2],v[11]]{vertices.extend_from_slice(&f.to_le_bytes());}}
        return Ok(Some(Draw{owner:e.id,key:format!("graph-mesh:{}",e.id),vertices,textures:Arc::new(vec![0;160]),pc:pc(&default_material(),eye,selected),graph:Some(graph),blend:true,distance:0.}));}
    let reference = c
        .props
        .get("mesh")
        .and_then(|v| v.as_str())
        .unwrap_or("cube");
    let owned;
    let data = if reference == "cube" || reference.is_empty() {
        cube_mesh_bytes()
    } else {
        owned = crate::meshres::load_mesh_cached(&crate::rpc::project_root(), reference)?;
        &owned.bytes
    };
    let world = modelrt::entity_world(scene, e)?;
    charge(
        vertex_used,
        (data.len() / 24) * 48,
        MAX_VERTEX_BYTES,
        "vertex data",
    )?;
    charge(texture_used, 160, MAX_TEXTURE_BYTES, "texture data")?;
    let nm = modelrt::normal_matrix(world)?;
    let mut vertices = Vec::new();
    for v in data.chunks_exact(24) {
        let f = |i: usize| f32::from_le_bytes(v[i * 4..i * 4 + 4].try_into().unwrap());
        let p = modelrt::point(world, [f(0), f(1), f(2)]);
        let n = modelrt::norm(modelrt::vector(nm, [f(3), f(4), f(5)]));
        for x in [p[0], p[1], p[2], n[0], n[1], n[2], 0., 0., 1., 0., 0., 1.] {
            vertices.extend_from_slice(&x.to_le_bytes());
        }
    }
    Ok(Some(Draw {
        owner: e.id,
        key: format!("legacy:{}:{reference}", e.id),
        vertices,
        textures: Arc::new(vec![0; 160]),
        graph: None,
        pc: pc(&default_material(), eye, selected),
        blend: false,
        distance: 0.,
    }))
}
