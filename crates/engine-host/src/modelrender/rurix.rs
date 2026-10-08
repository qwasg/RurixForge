//! modelrender 的 rurix 部分(02 §5.2,feature `backend-rurix`):模型腿 PBR 会话、WGSL、逐帧顶点上传与出帧。
//! 自 modelrender.rs 整段搬来,函数体逐字不变;导入经 `use super::*` 取自父模块。

use super::*;

const ATTRS: [(u32, u32, u32); 4] = [(0, 106, 0), (1, 106, 12), (2, 103, 24), (3, 109, 32)];
// Vulkan formats: R32G32B32_SFLOAT=106, R32G32_SFLOAT=103, R32G32B32A32_SFLOAT=109.
const CLEAR: [f32; 4] = [0.035, 0.045, 0.06, 1.];
struct Renderer {
    session: Option<rex::DeviceFrameSession<'static>>,
    sig: u64,
    device: String,
    draw_passes: Vec<u32>,
    vertex_resources: Vec<u32>,
    texture_resources: Vec<u32>,
    _programs: Vec<Arc<crate::shader::Program>>,
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
/// invalidate() 的 GPU 段(原函数第一句):丢弃模型腿会话,下一帧重建。
pub(super) fn reset_state() {
    if let Some(s) = STATE.get() {
        *s.lock().unwrap() = None;
    }
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
pub(super) fn shader() -> Result<(&'static [u8], &'static [u8]), String> {
    static S: OnceLock<Result<(&'static [u8], &'static [u8]), String>> = OnceLock::new();
    S.get_or_init(|| {
        Ok((
            viewport::compile_wgsl(VS, "model_vs")?,
            viewport::compile_wgsl(FS, "model_fs")?,
        ))
    })
    .clone()
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
            fs_spirv: if let Some(g)=&d.graph { unsafe { std::slice::from_raw_parts(g.program.model_spirv.as_ptr(),g.program.model_spirv.len()) } } else {fs},
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
        _programs: draws.iter().filter_map(|d|d.graph.as_ref().map(|g|g.program.clone())).collect(),
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
    let eye = model_eye(scene, cam, vp.is_some());
    let mut draws = collect(scene, eye, selected)?;
    if draws.is_empty() {
        return Err("MODEL_EMPTY: scene contains no model triangles".into());
    }
    for d in &mut draws {if let Some(g)=&d.graph {
        let word=|i:usize|u32::from_le_bytes(d.textures[i*4..i*4+4].try_into().unwrap());
        let (offset,w,h)=(word(0)as usize*4,word(1),word(2));
        let source=if w>0&&h>0&&offset+w as usize*h as usize*4<=d.textures.len(){crate::shader::Texture{width:w,height:h,rgba:Arc::new(d.textures[offset..offset+w as usize*h as usize*4].to_vec())}}else{crate::shader::Texture::white()};
        d.textures=Arc::new(g.pack(&source)?);d.key.push_str(&g.program.compiled.program_hash);d.blend=true;
        for i in 0..4{d.pc[i*4..i*4+4].copy_from_slice(&1f32.to_le_bytes());}
    }}
    let mut signature = Vec::new();
    signature.extend_from_slice(&width.to_le_bytes());
    signature.extend_from_slice(&height.to_le_bytes());
    for d in &draws {
        signature.extend_from_slice(d.key.as_bytes());
        signature.push(u8::from(d.blend));
        signature.extend_from_slice(&d.vertices.len().to_le_bytes());
        let texture_identity = if d.graph.is_some() {d.textures.len() as u64} else if d.key.starts_with("legacy") {
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
        let candidate=build(width,height,&draws,sig).map_err(|error|{for d in &draws{if let Some(g)=&d.graph{crate::shader::report_backend(&g.program.compiled.hash,"rurix",Err(error.clone()));}}error})?;
        if draws.iter().any(|d| d.graph.is_some()) { crate::shader::record_program_build("rurix"); }
        *guard=Some(candidate);
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
        if (rebuilt || d.graph.is_some()) && uploaded_textures.insert(r.texture_resources[i]) {
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
    for d in &draws {if let Some(g)=&d.graph {crate::shader::report_backend(&g.program.compiled.hash,"rurix",Ok(()));}}
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
