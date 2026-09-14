//! Standard 3D assets use a dedicated UV/PBR pipeline. Legacy-only scenes stay on viewport's path.
use crate::{
    modelrt,
    viewport::{self, EditorCamera, FramePixels, M4},
};
use assetd::model::{ModelBundle, ModelMaterial};
use forge_scene::{Entity, Scene};
use rurix_rt::{render_exec as rex, vk};
use std::sync::{Arc, Mutex, OnceLock};
const ATTRS: [(u32, u32, u32); 4] = [(0, 106, 0), (1, 106, 12), (2, 103, 24), (3, 109, 32)];
// Vulkan formats: R32G32B32_SFLOAT=106, R32G32_SFLOAT=103, R32G32B32A32_SFLOAT=109.
const CLEAR: [f32; 4] = [0.035, 0.045, 0.06, 1.];
const MAX_DRAWS: usize = 2048;
const MAX_VERTEX_BYTES: usize = 256 * 1024 * 1024;
const MAX_TEXTURE_BYTES: usize = 256 * 1024 * 1024;
struct Draw {
    owner: u64,
    key: String,
    vertices: Vec<u8>,
    textures: Arc<Vec<u8>>,
    pc: Vec<u8>,
    blend: bool,
    distance: f32,
}
struct Renderer {
    session: Option<rex::DeviceFrameSession<'static>>,
    sig: u64,
    device: String,
    draw_passes: Vec<u32>,
    vertex_resources: Vec<u32>,
    texture_resources: Vec<u32>,
    resources: *mut [rex::ResourceDesc<'static>],
    passes: *mut [rex::Pass<'static>],
    plans: *mut [Vec<(u32, rex::TargetState)>],
    barriers: *mut [&'static [(u32, rex::TargetState)]],
    readbacks: *mut [rex::Readback],
}
// Device and graph ownership are exclusively held behind STATE's mutex.
unsafe impl Send for Renderer {}
impl Drop for Renderer {
    fn drop(&mut self) {
        self.session.take();
        unsafe {
            // Session no longer borrows these stable allocations. Reclaim every graph on reload.
            drop(Box::from_raw(self.resources));
            drop(Box::from_raw(self.passes));
            drop(Box::from_raw(self.barriers));
            drop(Box::from_raw(self.plans));
            drop(Box::from_raw(self.readbacks));
        }
    }
}
static STATE: OnceLock<Mutex<Option<Renderer>>> = OnceLock::new();
static PACKED: OnceLock<Mutex<std::collections::HashMap<String, Arc<Vec<u8>>>>> = OnceLock::new();
pub fn invalidate() {
    if let Some(s) = STATE.get() {
        *s.lock().unwrap() = None;
    }
    if let Some(p) = PACKED.get() {
        p.lock().unwrap().clear();
    }
}
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
const VS: &str = r#"
struct Camera{vp:mat4x4<f32>};
@group(0) @binding(1) var<uniform> camera:Camera;
struct Out{@builtin(position) clip:vec4<f32>,@location(0) pos:vec3<f32>,@location(1) n:vec3<f32>,@location(2) uv:vec2<f32>,@location(3) tangent:vec4<f32>};
@vertex fn main(@location(0) p:vec3<f32>,@location(1) n:vec3<f32>,@location(2) uv:vec2<f32>,@location(3) tangent:vec4<f32>)->Out{
var o:Out;o.clip=camera.vp*vec4<f32>(p,1.);o.pos=p;o.n=n;o.uv=uv;o.tangent=tangent;return o;}
"#;
const FS: &str = r#"
@group(0) @binding(0) var<storage,read> tex:array<u32>;
struct Pc{base:vec4<f32>,emission_rough:vec4<f32>,eye_metal:vec4<f32>,controls:vec4<f32>,flags:vec4<f32>};
var<push_constant> pc:Pc;
fn wrap(u:f32,kind:u32)->f32{if(kind==33071u){return clamp(u,0.,1.);}if(kind==33648u){let v=u-floor(u/2.)*2.;return select(v,2.-v,v>1.);}return fract(u);}
fn coord(i:i32,n:i32,mode:u32)->i32{if(mode==33071u){return clamp(i,0,n-1);}if(mode==33648u){let p=((i%(2*n))+2*n)%(2*n);return select(p,2*n-p-1,p>=n);}return ((i%n)+n)%n;}
fn pixel(b:u32,x:i32,y:i32)->vec4<f32>{let w=tex[b+1u];let h=tex[b+2u];let p=tex[tex[b]+u32(coord(y,i32(h),tex[b+4u]))*w+u32(coord(x,i32(w),tex[b+3u]))];return vec4<f32>(f32(p&255u),f32((p>>8u)&255u),f32((p>>16u)&255u),f32((p>>24u)&255u))/255.;}
fn sample_tex(slot:u32,uv:vec2<f32>)->vec4<f32>{let b=slot*8u;let w=tex[b+1u];let h=tex[b+2u];if(w==0u){return vec4<f32>(1.);}let u=vec2<f32>(wrap(uv.x,tex[b+3u]),wrap(uv.y,tex[b+4u]));let xy=u*vec2<f32>(f32(w),f32(h))-vec2<f32>(0.5);let ip=vec2<i32>(floor(xy));let f=fract(xy);let mode=tex[b+5u];if(mode==9728u||mode==9984u||mode==9986u){return pixel(b,i32(floor(xy.x+0.5)),i32(floor(xy.y+0.5)));}return mix(mix(pixel(b,ip.x,ip.y),pixel(b,ip.x+1,ip.y),f.x),mix(pixel(b,ip.x,ip.y+1),pixel(b,ip.x+1,ip.y+1),f.x),f.y);}
fn linear(c:vec3<f32>)->vec3<f32>{return select(c/12.92,pow((c+vec3<f32>(0.055))/1.055,vec3<f32>(2.4)),c>vec3<f32>(0.04045));}
@fragment fn main(@location(0) pos:vec3<f32>,@location(1) normal:vec3<f32>,@location(2) uv:vec2<f32>,@location(3) tangent:vec4<f32>,@builtin(front_facing) front:bool)->@location(0) vec4<f32>{
if(!front && pc.flags.y<0.5){discard;}
let texel=sample_tex(0u,uv);let base=pc.base*vec4<f32>(linear(texel.rgb),texel.a);
if(pc.controls.z>0.5 && pc.controls.z<1.5 && base.a<pc.controls.w){discard;}
if(pc.controls.z>1.5 && base.a<=0.){discard;}
if(pc.flags.z>0.5 && texel.g<0.5*min(texel.r,texel.b)){discard;}
if(pc.flags.z>0.5){return vec4<f32>(texel.rgb*pc.base.rgb,pc.base.a);}
var n=normalize(normal);if(!front){n=-n;}
if(tex[9u]>0u){let t=normalize(tangent.xyz-dot(tangent.xyz,n)*n);let b=cross(n,t)*tangent.w;var nm=sample_tex(1u,uv).xyz*2.-vec3<f32>(1.);nm=vec3<f32>(nm.xy*pc.controls.x,nm.z);n=normalize(t*nm.x+b*nm.y+n*nm.z);}
let mr=sample_tex(2u,uv);let rough=clamp(pc.emission_rough.w*mr.g,0.045,1.);let metal=clamp(pc.eye_metal.w*mr.b,0.,1.);
let l=normalize(vec3<f32>(0.45,0.8,0.35));let v=normalize(pc.eye_metal.xyz-pos);let h=normalize(l+v);let nv=max(dot(n,v),0.001);let nl=max(dot(n,l),0.);let nh=max(dot(n,h),0.);let hv=max(dot(h,v),0.);
let a=rough*rough;let a2=a*a;let d=a2/(3.14159265*pow(nh*nh*(a2-1.)+1.,2.));let k=pow(rough+1.,2.)/8.;let g=(nv/(nv*(1.-k)+k))*(nl/(nl*(1.-k)+k));let f0=mix(vec3<f32>(0.04),base.rgb,metal);let f=f0+(vec3<f32>(1.)-f0)*pow(1.-hv,5.);let spec=d*g*f/max(4.*nv*nl,0.001);let diffuse=(vec3<f32>(1.)-f)*(1.-metal)*base.rgb/3.14159265;
let ao=mix(1.,sample_tex(3u,uv).r,pc.controls.y);let emission=pc.emission_rough.xyz*linear(sample_tex(4u,uv).rgb);
var color=base.rgb*0.14*ao+(diffuse+spec)*nl*3.+emission;
if(pc.flags.x>0.5){color=base.rgb+emission;}
if(pc.flags.w>0.5){color=mix(color,vec3<f32>(1.,0.4,0.06),0.25);}
color=color/(vec3<f32>(1.)+color);color=pow(max(color,vec3<f32>(0.)),vec3<f32>(1./2.2));return vec4<f32>(color,select(1.,base.a,pc.controls.z>1.5));
}
"#;
const BLEND: &str = r#"
@group(0) @binding(0) var dst:texture_storage_2d<rgba8unorm,read_write>;
@group(0) @binding(1) var src:texture_storage_2d<rgba8unorm,read>;
@compute @workgroup_size(8,8,1) fn main(@builtin(global_invocation_id) id:vec3<u32>){let size=textureDimensions(dst);if(id.x>=size.x||id.y>=size.y){return;}let p=vec2<i32>(id.xy);let s=textureLoad(src,p);let d=textureLoad(dst,p);let rgb=pow(pow(s.rgb,vec3<f32>(2.2))*s.a+pow(d.rgb,vec3<f32>(2.2))*(1.-s.a),vec3<f32>(1./2.2));textureStore(dst,p,vec4<f32>(rgb,1.));}
"#;
fn extra_shaders() -> Result<(&'static [u8], &'static [u8], &'static [u8]), String> {
    static S: OnceLock<Result<(&'static [u8], &'static [u8], &'static [u8]), String>> =
        OnceLock::new();
    S.get_or_init(|| {
        Ok((
            viewport::compile_wgsl(
                "@vertex fn main()->@builtin(position) vec4<f32>{return vec4<f32>(2.,2.,2.,1.);}",
                "model_clear_vs",
            )?,
            viewport::compile_wgsl(
                "@fragment fn main()->@location(0) vec4<f32>{return vec4<f32>(0.);}",
                "model_clear_fs",
            )?,
            viewport::compile_wgsl(BLEND, "model_alpha_composite")?,
        ))
    })
    .clone()
}
fn shader() -> Result<(&'static [u8], &'static [u8]), String> {
    static S: OnceLock<Result<(&'static [u8], &'static [u8]), String>> = OnceLock::new();
    S.get_or_init(|| {
        Ok((
            viewport::compile_wgsl(VS, "model_vs")?,
            viewport::compile_wgsl(FS, "model_fs")?,
        ))
    })
    .clone()
}
fn default_material() -> ModelMaterial {
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
fn charge(used: &mut usize, amount: usize, limit: usize, kind: &str) -> Result<(), String> {
    let next = used
        .checked_add(amount)
        .ok_or_else(|| format!("MODEL_BUDGET: {kind} size overflow"))?;
    if next > limit {
        return Err(format!("MODEL_BUDGET: {kind} exceeds {limit} bytes"));
    }
    *used = next;
    Ok(())
}
fn pc(m: &ModelMaterial, eye: [f32; 3], selected: bool) -> Vec<u8> {
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
fn collect(scene: &Scene, eye: [f32; 3], selected: Option<u64>) -> Result<Vec<Draw>, String> {
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
                viewport::m4_mul(world, modelrt::inverse(rest[ni])?)
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
        viewport::resolve_sprite_render(sp)
    } else {
        e.component("MeshRenderer")
            .filter(|c| c.enabled)
            .and_then(|c| c.props["material"].as_str())
            .and_then(|mat| viewport::material_albedo_guid(mat, &crate::rpc::project_root()))
            .and_then(|guid| viewport::load_tex_static_cached(&crate::rpc::project_root(), &guid))
            .map(|tex| viewport::SpriteRenderInfo {
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
        let local = viewport::sprite_render_transform(e).unwrap_or(e.transform);
        let world = viewport::m4_mul(
            modelrt::entity_world(scene, e)?,
            viewport::m4_mul(
                modelrt::inverse(viewport::trs_model(&e.transform))?,
                viewport::trs_model(&local),
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
            pc: params,
            blend: false,
            distance: 0.,
        }));
    }
    let Some(c) = e.component("MeshRenderer").filter(|c| c.enabled) else {
        return Ok(None);
    };
    let reference = c
        .props
        .get("mesh")
        .and_then(|v| v.as_str())
        .unwrap_or("cube");
    let owned;
    let data = if reference == "cube" || reference.is_empty() {
        viewport::cube_mesh_bytes()
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
        pc: pc(&default_material(), eye, selected),
        blend: false,
        distance: 0.,
    }))
}
fn build(width: u32, height: u32, draws: &[Draw], sig: u64) -> Result<Renderer, String> {
    if !vk::vulkan_available() {
        return Err("DEV_ENV_DEGRADE: vulkan loader unavailable".into());
    }
    let caps = rex::probe_device_caps().map_err(|e| e.to_string())?;
    let (vs, fs) = shader()?;
    let (clear_vs, clear_fs, blend_cs) = extra_shaders()?;
    let mut resources = vec![
        rex::ResourceDesc::Buffer(rex::BufferDesc {
            size: 64,
            usage: rex::BufferUsage {
                uniform: true,
                ..Default::default()
            },
            data: None,
            device_local: false,
        }),
        rex::ResourceDesc::Texture(rex::TextureDesc {
            width,
            height,
            format: rex::TexFormat::Rgba8Unorm,
            usage: rex::TextureUsage {
                color: true,
                storage: true,
                ..Default::default()
            },
            data: None,
        }),
        rex::ResourceDesc::Texture(rex::TextureDesc {
            width,
            height,
            format: rex::TexFormat::Depth32Float,
            usage: rex::TextureUsage {
                depth: true,
                ..Default::default()
            },
            data: None,
        }),
    ];
    let mut vertex_resources = Vec::new();
    let mut texture_resources = Vec::new();
    let mut shared_textures = std::collections::HashMap::new();
    for d in draws {
        vertex_resources.push(resources.len() as u32);
        resources.push(rex::ResourceDesc::Buffer(rex::BufferDesc {
            size: d.vertices.len() as u64,
            usage: rex::BufferUsage {
                vertex: true,
                ..Default::default()
            },
            data: None,
            device_local: false,
        }));
        let identity = Arc::as_ptr(&d.textures) as usize;
        let tex = *shared_textures.entry(identity).or_insert_with(|| {
            let index = resources.len() as u32;
            resources.push(rex::ResourceDesc::Buffer(rex::BufferDesc {
                size: d.textures.len() as u64,
                usage: rex::BufferUsage {
                    storage: true,
                    ..Default::default()
                },
                data: None,
                device_local: false,
            }));
            index
        });
        texture_resources.push(tex);
    }
    let scratch_res = resources.len() as u32;
    let mut passes = Vec::new();
    let mut plans = Vec::new();
    let mut draw_passes = Vec::new();
    passes.push(rex::Pass::Raster(rex::RasterPass {
        blend: rex::BlendMode::Opaque,
        name: "model_clear",
        vs_spirv: clear_vs,
        fs_spirv: clear_fs,
        vertex: rex::VertexData::Pull,
        draw: rex::DrawSpec::Direct {
            vertex_count: 3,
            instance_count: 1,
            first_vertex: 0,
            first_instance: 0,
        },
        colors: vec![rex::ColorAttachmentRef {
            res: 1,
            clear: Some(CLEAR),
        }],
        depth: Some(rex::DepthAttachmentRef {
            res: 2,
            clear: Some(1.),
        }),
        viewport: None,
        bindings: rex::Bindings::default(),
        conservative: None,
    }));
    plans.push(vec![
        (1, rex::TargetState::ColorAttachmentWrite),
        (2, rex::TargetState::DepthAttachmentWrite),
    ]);
    for (i, d) in draws.iter().enumerate() {
        let vb = vertex_resources[i];
        let tex = texture_resources[i];
        draw_passes.push(passes.len() as u32);
        passes.push(rex::Pass::Raster(rex::RasterPass {
        blend: rex::BlendMode::Opaque,
            name: "forge_model_pbr",
            vs_spirv: vs,
            fs_spirv: fs,
            vertex: rex::VertexData::Resource {
                res: vb,
                offset: 0,
                stride: 48,
                attrs: &ATTRS,
            },
            draw: rex::DrawSpec::Direct {
                vertex_count: (d.vertices.len() / 48) as u32,
                instance_count: 1,
                first_vertex: 0,
                first_instance: 0,
            },
            colors: vec![rex::ColorAttachmentRef {
                res: if d.blend { scratch_res } else { 1 },
                clear: if d.blend { Some([0.; 4]) } else { None },
            }],
            depth: Some(rex::DepthAttachmentRef {
                res: 2,
                clear: None,
            }),
            viewport: None,
            bindings: rex::Bindings {
                storage_buffers: vec![tex],
                uniform: Some(rex::UniformRef {
                    res: 0,
                    offset: 0,
                    size: 64,
                }),
                push_constants: d.pc.clone(),
                ..Default::default()
            },
            conservative: None,
        }));
        plans.push(vec![
            (
                if d.blend { scratch_res } else { 1 },
                rex::TargetState::ColorAttachmentWrite,
            ),
            (2, rex::TargetState::DepthAttachmentWrite),
        ]);
        if d.blend {
            passes.push(rex::Pass::Compute(rex::ComputePass {
                name: "model_alpha_over",
                spirv: blend_cs,
                entry: None,
                dispatch: rex::DispatchSpec::Direct([width.div_ceil(8), height.div_ceil(8), 1]),
                bindings: rex::Bindings {
                    storage_images: vec![1, scratch_res],
                    ..Default::default()
                },
            }));
            plans.push(vec![
                (1, rex::TargetState::StorageImageReadWrite),
                (scratch_res, rex::TargetState::StorageImageReadWrite),
            ]);
        }
    }
    resources.push(rex::ResourceDesc::Texture(rex::TextureDesc {
        width,
        height,
        format: rex::TexFormat::Rgba8Unorm,
        usage: rex::TextureUsage {
            color: true,
            storage: true,
            ..Default::default()
        },
        data: None,
    }));
    let resources = Box::leak(resources.into_boxed_slice());
    let passes = Box::leak(passes.into_boxed_slice());
    let plans = Box::leak(plans.into_boxed_slice());
    let plans_ptr = plans as *mut [_];
    let barriers = Box::leak(
        plans
            .iter()
            .map(Vec::as_slice)
            .collect::<Vec<_>>()
            .into_boxed_slice(),
    );
    let readbacks = Box::leak(vec![rex::Readback::Texture { res: 1 }].into_boxed_slice());
    let mut result = Renderer {
        session: None,
        sig,
        device: caps.device_name,
        draw_passes,
        vertex_resources,
        texture_resources,
        resources,
        passes,
        plans: plans_ptr,
        barriers,
        readbacks,
    };
    result.session = Some(
        rex::DeviceFrameSession::new(resources, passes, barriers, readbacks, 2)
            .map_err(|e| format!("model render session: {e}"))?,
    );
    Ok(result)
}
pub fn render(
    scene: &Scene,
    cam: &EditorCamera,
    selected: Option<u64>,
    width: u32,
    height: u32,
    want_readback: bool,
    want_stats: bool,
    vp: Option<M4>,
) -> Result<FramePixels, String> {
    let eye = if vp.is_some() {
        scene
            .entities
            .iter()
            .find(|e| e.component("Camera").is_some_and(|c| c.enabled))
            .and_then(|e| modelrt::entity_world(scene, e).ok())
            .map(|m| modelrt::point(m, [0.; 3]))
            .unwrap_or_else(|| cam.eye())
    } else {
        cam.eye()
    };
    let mut draws = collect(scene, eye, selected)?;
    if draws.is_empty() {
        return Err("MODEL_EMPTY: scene contains no model triangles".into());
    }
    let mut signature = Vec::new();
    signature.extend_from_slice(&width.to_le_bytes());
    signature.extend_from_slice(&height.to_le_bytes());
    for d in &draws {
        signature.extend_from_slice(d.key.as_bytes());
        signature.push(u8::from(d.blend));
        signature.extend_from_slice(&d.vertices.len().to_le_bytes());
        let texture_identity = if d.key.starts_with("legacy") {
            crate::meshres::fnv1a64(&d.textures)
        } else {
            Arc::as_ptr(&d.textures) as usize as u64
        };
        signature.extend_from_slice(&texture_identity.to_le_bytes());
    }
    let sig = crate::meshres::fnv1a64(&signature);
    let mut guard = STATE.get_or_init(|| Mutex::new(None)).lock().unwrap();
    let rebuilt = guard.as_ref().is_none_or(|s| s.sig != sig);
    if rebuilt {
        *guard = Some(build(width, height, &draws, sig)?);
    }
    let r = guard.as_mut().unwrap();
    let mut update = rex::FrameUpdate::default();
    update.buffer_uploads.push((
        rex::StableResourceId(1),
        0,
        viewport::m4_col_bytes(vp.unwrap_or_else(|| cam.view_proj(width as f32 / height as f32)))
            .to_vec(),
    ));
    let triangles = draws.iter().map(|d| d.vertices.len() / 48 / 3).sum();
    let mut uploaded_textures = std::collections::HashSet::new();
    for (i, d) in draws.iter_mut().enumerate() {
        update.buffer_uploads.push((
            rex::StableResourceId(u64::from(r.vertex_resources[i]) + 1),
            0,
            std::mem::take(&mut d.vertices),
        ));
        if rebuilt && uploaded_textures.insert(r.texture_resources[i]) {
            update.buffer_uploads.push((
                rex::StableResourceId(u64::from(r.texture_resources[i]) + 1),
                0,
                d.textures.as_ref().clone(),
            ));
        }
        update
            .push_constant_overrides
            .push((r.draw_passes[i], std::mem::take(&mut d.pc)));
    }
    update.readback_subset = if want_readback { Some(vec![0]) } else { None };
    let session = r.session.as_mut().unwrap();
    let provenance = session
        .next_provenance_with_update(&update)
        .map_err(|e| e.to_string())?;
    let result = session
        .execute_with_frame_update(&provenance, &update)
        .map_err(|e| e.to_string())?;
    let rgba8 = if want_readback {
        result
            .readbacks
            .into_iter()
            .next()
            .ok_or("model readback missing")?
    } else {
        Vec::new()
    };
    let nonzero = if want_stats {
        let bg = CLEAR.map(|x| (x * 255. + 0.5) as u8);
        rgba8.chunks_exact(4).filter(|p| p[..3] != bg[..3]).count()
    } else {
        0
    };
    Ok(FramePixels {
        width,
        height,
        rgba8,
        device_name: r.device.clone(),
        draws: draws.len(),
        truncated: false,
        nonzero,
        triangles,
        mesh_fallbacks: 0,
        mesh_classes: draws.len(),
        imported: false,
    })
}
#[cfg(test)]
mod tests {
    use super::*;
    use assetd::model::*;
    use forge_scene::{Component, Transform};
    fn fixture() -> ModelBundle {
        let mut mat = default_material();
        mat.base_color = [1.; 4];
        mat.base_color_texture = Some(0);
        mat.unlit = true;
        let identity = [
            1., 0., 0., 0., 0., 1., 0., 0., 0., 0., 1., 0., 0., 0., 0., 1.,
        ];
        ModelBundle {
            version: 1,
            guid: "test-model-gpu-uv-skin".into(),
            revision: 1,
            name: "uv skin fixture".into(),
            source_id: "self-contained-test".into(),
            source_hash: "first".into(),
            kind: "role".into(),
            idle_clip: "walk".into(),
            walk_clip: "walk".into(),
            roots: vec![0, 1],
            primitives: vec![ModelPrimitive {
                id: "quad".into(),
                positions: vec![
                    [-0.7, -0.7, 0.],
                    [0.7, -0.7, 0.],
                    [0.7, 0.7, 0.],
                    [-0.7, 0.7, 0.],
                ],
                normals: vec![[0., 0., 1.]; 4],
                tangents: vec![[1., 0., 0., 1.]; 4],
                uv0: vec![[0., 1.], [1., 1.], [1., 0.], [0., 0.]],
                indices: vec![0, 1, 2, 0, 2, 3],
                joints: vec![[0, 0, 0, 0]; 4],
                weights: vec![[1., 0., 0., 0.]; 4],
                material: Some(0),
            }],
            nodes: vec![
                ModelNode {
                    id: "mesh".into(),
                    name: "mesh".into(),
                    children: vec![],
                    primitives: vec![0],
                    translation: [0.; 3],
                    rotation: [0., 0., 0., 1.],
                    scale: [1.; 3],
                    matrix: None,
                    skin: Some(0),
                    collision: false,
                },
                ModelNode {
                    id: "bone".into(),
                    name: "bone".into(),
                    children: vec![],
                    primitives: vec![],
                    translation: [0.; 3],
                    rotation: [0., 0., 0., 1.],
                    scale: [1.; 3],
                    matrix: None,
                    skin: None,
                    collision: false,
                },
            ],
            materials: vec![mat],
            textures: vec![ModelTexture {
                id: "checker".into(),
                guid: "checker".into(),
                asset_path: "".into(),
                width: 2,
                height: 2,
                rgba: vec![
                    255, 0, 0, 255, 0, 255, 0, 255, 0, 0, 255, 255, 255, 255, 0, 255,
                ],
                wrap_s: 33071,
                wrap_t: 33071,
                mag_filter: Some(9729),
                min_filter: Some(9729),
            }],
            skins: vec![ModelSkin {
                name: "skin".into(),
                joints: vec![1],
                inverse_bind_matrices: vec![identity],
                skeleton: Some(1),
            }],
            animations: vec![ModelAnimation {
                name: "walk".into(),
                duration: 2.,
                channels: vec![ModelAnimationChannel {
                    node: 1,
                    path: "translation".into(),
                    times: vec![0., 2.],
                    values: vec![[0., 0., 0., 0.], [1., 0., 0., 0.]],
                    interpolation: "LINEAR".into(),
                }],
            }],
        }
    }
    #[test]
    fn gpu_uv_skin_instances_and_same_size_reload() {
        let model = fixture();
        crate::modelrt::prime(model.clone());
        let mut scene = Scene::new("gpu-fixture");
        scene.entities.push(Entity {
            id: 1,
            name: "animated".into(),
            transform: Transform::default(),
            components: vec![
                Component::new("ModelRenderer", serde_json::json!({"model":model.guid})),
                Component::new(
                    "Animator",
                    serde_json::json!({"clip":"walk","time":0,"loop":false}),
                ),
            ],
        });
        let cam = EditorCamera {
            target: [0.; 3],
            yaw_deg: 0.,
            pitch_deg: 0.,
            dist: 3.,
            ..Default::default()
        };
        let a = render(&scene, &cam, None, 128, 128, true, true, None)
            .expect("real GPU required for model acceptance");
        assert!(a.nonzero > 100);
        assert_eq!(a.triangles, 2);
        assert_eq!(a.mesh_fallbacks, 0);
        let red = a
            .rgba8
            .chunks_exact(4)
            .filter(|p| {
                u16::from(p[0]) * 2 > u16::from(p[1]) * 3
                    && u16::from(p[0]) * 2 > u16::from(p[2]) * 3
            })
            .count();
        let blue = a
            .rgba8
            .chunks_exact(4)
            .filter(|p| {
                u16::from(p[2]) > u16::from(p[0]) * 2 && u16::from(p[2]) > u16::from(p[1]) * 2
            })
            .count();
        assert!(
            red > 10 && blue > 10,
            "UV checker must preserve distinct colored regions red={red} blue={blue}"
        );
        scene.entities[0].component_mut("Animator").unwrap().props["time"] = serde_json::json!(1.);
        let b = render(&scene, &cam, None, 128, 128, true, true, None).unwrap();
        assert_ne!(
            a.rgba8, b.rgba8,
            "joint animation must move actual vertices"
        );
        let mut second = scene.entities[0].clone();
        second.id = 2;
        second.transform.translation = [-1., 0., 0.];
        second.component_mut("Animator").unwrap().props["time"] = serde_json::json!(0.);
        scene.entities.push(second);
        let c = render(&scene, &cam, None, 128, 128, true, true, None).unwrap();
        assert_eq!(c.draws, 2);
        assert_eq!(c.triangles, 4);
        let mut changed = model;
        changed.revision = 2;
        changed.source_hash = "changed-same-size".into();
        changed.textures[0].rgba = vec![
            255, 0, 255, 255, 255, 0, 255, 255, 255, 0, 255, 255, 255, 0, 255, 255,
        ];
        crate::modelrt::prime(changed);
        let d = render(&scene, &cam, None, 128, 128, true, true, None).unwrap();
        assert_ne!(c.rgba8, d.rgba8, "same-size texture revision must refresh");
        assert!(
            d.rgba8
                .chunks_exact(4)
                .any(|p| p[0] > 100 && p[2] > 100 && p[1] < 20),
            "PBR must retain purple; no sprite chroma key"
        );
        eprintln!("MODEL_GPU_ACCEPTANCE device={} draws={} triangles={} nonzero={} UV_colors=({}, {}) animation_changed=true revision_changed=true",d.device_name,d.draws,d.triangles,d.nonzero,red,blue);
        // Explicit nearest sampler keeps the four texel colors (plus clear), not interpolated colors.
        let mut nearest = fixture();
        nearest.revision = 3;
        nearest.textures[0].mag_filter = Some(9728);
        crate::modelrt::prime(nearest);
        scene.entities.truncate(1);
        scene.entities[0].component_mut("Animator").unwrap().props["time"] = serde_json::json!(0.);
        let nearest_frame = render(&scene, &cam, None, 128, 128, true, true, None).unwrap();
        let unique = nearest_frame
            .rgba8
            .chunks_exact(4)
            .map(|p| p.to_vec())
            .collect::<std::collections::HashSet<_>>();
        assert_eq!(
            unique.len(),
            5,
            "nearest sampler must preserve four exact texels plus background"
        );
        // Two PBR layers are composited on GPU with alpha-over, in depth order.
        let mut transparent = fixture();
        transparent.revision = 4;
        transparent.materials[0].base_color_texture = None;
        transparent.materials[0].base_color = [1., 0., 0., 0.5];
        transparent.materials[0].alpha_mode = "BLEND".into();
        crate::modelrt::prime(transparent);
        let mut background = fixture();
        background.guid = "test-model-blue-background".into();
        background.materials[0].base_color_texture = None;
        background.materials[0].base_color = [0., 0., 1., 1.];
        crate::modelrt::prime(background.clone());
        let mut blue_entity = scene.entities[0].clone();
        blue_entity.id = 3;
        blue_entity.transform.translation = [0., 0., -0.1];
        blue_entity.component_mut("ModelRenderer").unwrap().props["model"] =
            serde_json::json!(background.guid);
        scene.entities.push(blue_entity);
        let blended = render(&scene, &cam, None, 128, 128, true, true, None).unwrap();
        let center = &blended.rgba8[(64 * 128 + 64) * 4..(64 * 128 + 64) * 4 + 4];
        assert!(
            center[0] > 70 && center[2] > 70 && center[1] < 20,
            "GPU alpha blend red over blue: {center:?}"
        );
        // More than the old 8 mesh classes / 128 draw ceiling remain visible and never become cubes.
        let mut many = Scene::new("many");
        for i in 0..129 {
            let mut e = scene.entities[1].clone();
            e.id = i + 100;
            e.transform.translation = [(i % 13) as f32 * 0.02, 0., -0.1];
            many.entities.push(e);
        }
        let dense = render(&many, &cam, None, 64, 64, true, true, None).unwrap();
        assert_eq!(dense.draws, 129);
        assert_eq!(dense.triangles, 258);
        assert!(!dense.truncated);
        eprintln!("MODEL_GPU_EXTENDED alpha_center={center:?} nearest_colors={} draws_over_128={} fallbacks={}",unique.len(),dense.draws,dense.mesh_fallbacks);
        invalidate();
    }
    #[test]
    fn shaders_compile() {
        shader().unwrap();
    }
    #[test]
    fn editable_node_transforms_match_visuals_and_static_mesh_collision() {
        let mut model = fixture();
        model.guid = "test-map-node-proxy".into();
        model.kind = "map".into();
        model.nodes[0].skin = None;
        model.nodes[0].translation = [1., 0., 0.];
        model.nodes[0].collision = true;
        model.nodes[1].primitives = vec![0];
        model.nodes[1].translation = [100., 0., 0.];
        crate::modelrt::prime(model.clone());
        let mut scene = Scene::new("map-proxy");
        scene.entities = vec![
            Entity {
                id: 1,
                name: "map".into(),
                transform: Transform {
                    translation: [2., 0., 0.],
                    ..Default::default()
                },
                components: vec![Component::new(
                    "Collider",
                    serde_json::json!({"shape":"mesh","model":model.guid}),
                )],
            },
            Entity {
                id: 2,
                name: "child".into(),
                transform: Transform {
                    translation: [1., 0., 0.],
                    ..Default::default()
                },
                components: vec![
                    Component::new("Parent", serde_json::json!({"entity":1})),
                    Component::new(
                        "ModelNode",
                        serde_json::json!({"model":model.guid,"nodeId":"mesh"}),
                    ),
                    Component::new(
                        "ModelRenderer",
                        serde_json::json!({"model":model.guid,"nodeId":"mesh"}),
                    ),
                ],
            },
        ];
        assert!(
            (bounds(&scene).unwrap().0[0] - 3.).abs() < 1e-5,
            "source local transform must be applied once"
        );
        scene.entities[1].transform.translation = [5., 0., 0.];
        assert!((bounds(&scene).unwrap().0[0] - 7.).abs() < 1e-5);
        let body = crate::character::body_desc(&scene, &scene.entities[0])
            .unwrap()
            .unwrap();
        let rurix_physics::ShapeDesc::StaticMesh {
            vertices,
            triangles,
        } = body.shape
        else {
            panic!("expected static mesh")
        };
        assert_eq!(
            triangles.len(),
            2,
            "unmarked helper node excluded from collision"
        );
        assert!(
            (vertices[0][0] - 6.3).abs() < 1e-5,
            "collider follows edited node, got {:?}",
            vertices[0]
        );
        assert_eq!(
            pick(&scene, [7., 0., 3.], [0., 0., -1.]).unwrap().0,
            2,
            "picking selects actual model child"
        );
    }
    #[test]
    fn budgets_fail_before_growth() {
        let mut used = MAX_VERTEX_BYTES - 4;
        assert!(charge(&mut used, 8, MAX_VERTEX_BYTES, "vertex")
            .unwrap_err()
            .contains("MODEL_BUDGET"));
        assert_eq!(used, MAX_VERTEX_BYTES - 4);
    }
    #[test]
    fn material_pc_layout() {
        assert_eq!(pc(&default_material(), [0.; 3], false).len(), 80);
    }
}

