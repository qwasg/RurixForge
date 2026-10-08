//! sentinels_v6_render 的 rurix 部分(02 §5.2,feature `backend-rurix`):V6 原生实例化光栅会话(V6G)、WGSL、
//! 贴图页上传与出帧。自 sentinels_v6_render.rs 整段搬来,函数体逐字不变;导入与 CPU 状态经 `use super::*` 取自
//! 父模块。父模块测试要用的几项升为 pub(super)。

use super::*;

// WGSL uses +Y up. Naga SPIR-V defaults already flip Y for the positive Vulkan viewport.
pub(super) const VS: &str = r#"
struct Camera { view:vec4<f32>, info:vec4<f32> };
struct Item { pos:vec4<f32>, size:vec4<f32>, color:vec4<f32> };
@group(0) @binding(1) var<uniform> cam:Camera;
@group(0) @binding(0) var<storage,read> items:array<Item>;
struct Out { @builtin(position) p:vec4<f32>, @location(0) color:vec4<f32> };
@vertex fn main(@builtin(vertex_index) vi:u32,@builtin(instance_index) ii:u32)->Out {
 var o:Out; if(ii>=u32(cam.info.x)){o.p=vec4<f32>(3.,3.,0.,1.);o.color=vec4<f32>(0.);return o;}
 let a=items[ii]; let face=vi/6u;let k=vi%6u;
 let uv=array<vec2<f32>,6>(vec2<f32>(0.,0.),vec2<f32>(1.,0.),vec2<f32>(1.,1.),vec2<f32>(0.,0.),vec2<f32>(1.,1.),vec2<f32>(0.,1.));
 let t=uv[k];var p:vec3<f32>;var shade:f32;
 if(face==0u){p=vec3<f32>(t.x*a.size.x,t.y*a.size.y,a.size.z);shade=1.;}
 else if(face==1u){p=vec3<f32>(a.size.x,t.x*a.size.y,t.y*a.size.z);shade=0.65;}
 else{p=vec3<f32>(t.x*a.size.x,a.size.y,t.y*a.size.z);shade=0.82;}
 p+=a.pos.xyz;let iso=vec2<f32>((p.x-p.y)*0.5,-(p.x+p.y)*0.25+p.z*1.5);
 let clip=(iso-cam.view.xy)/vec2<f32>(cam.view.z*cam.view.w,cam.view.z);
 o.p=vec4<f32>(clip.x,clip.y,clamp(0.95-(p.x+p.y+p.z*2.)*0.003,0.01,0.99),1.);o.color=vec4<f32>(a.color.rgb*shade,a.color.a);return o;
}"#;
pub(super) const FS: &str =
    r#"@fragment fn main(@location(0) color:vec4<f32>)->@location(0) vec4<f32>{return color;}"#;
struct Renderer {
    width: u32,
    height: u32,
    session: rex::DeviceFrameSession<'static>,
    device: String,
    frames: BTreeMap<String, usize>,
    used: Vec<u64>,
    alpha_bounds: Vec<AlphaBounds>,
    clock: u64,
}
unsafe impl Send for Renderer {}
static GPU: OnceLock<Mutex<Option<Renderer>>> = OnceLock::new();
pub(super) const SMALL_SLOTS: usize = 1024;
const LARGE_SLOTS: usize = 512;
pub(crate) const FRAME_SLOTS: usize = SMALL_SLOTS + LARGE_SLOTS;
pub(super) const PAGE_BYTES: usize = 128 * 1024 * 1024;
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct AlphaBounds(pub(super) [f32; 2]);
impl AlphaBounds {
    pub(super) const FULL: Self = Self([-1., -1.]);

    pub(super) fn from_pixels(pixels: &[u8], tile_size: u32, sample: [u32; 2]) -> Self {
        // Numeric packing is exact in f32: each coordinate is <=512, so
        // x + 1024*y is <=524800 (<2^20). Never carry integer bits through
        // NaNs/subnormals. Unknown dimensions keep the previous full sampling.
        if tile_size == 0 || tile_size > 512 || sample.contains(&0)
            || sample.iter().any(|&side| side > tile_size)
            || pixels.len() != tile_size as usize * tile_size as usize * 4
        {
            return Self::FULL;
        }
        let mut lo = sample;
        let mut hi = [0, 0];
        for y in 0..sample[1] {
            for x in 0..sample[0] {
                if pixels[((y * tile_size + x) * 4 + 3) as usize] != 0 {
                    lo[0] = lo[0].min(x);
                    lo[1] = lo[1].min(y);
                    hi[0] = hi[0].max(x + 1);
                    hi[1] = hi[1].max(y + 1);
                }
            }
        }
        if hi == [0, 0] {
            return Self([0., 0.]);
        }
        // Include alpha=1 and two neighbouring texels around every nonzero
        // edge. The shader currently samples nearest; this conservative guard
        // also leaves room for the existing UV edge/clamp precision.
        for axis in 0..2 {
            lo[axis] = lo[axis].saturating_sub(2);
            hi[axis] = (hi[axis] + 2).min(sample[axis]);
        }
        Self([(lo[0] + 1024 * lo[1]) as f32, (hi[0] + 1024 * hi[1]) as f32])
    }
}
pub(super) fn cache_location(slot: usize) -> (usize, usize) {
    if slot < SMALL_SLOTS {
        (slot / 512, (slot % 512) * 256 * 256 * 4)
    } else {
        let large = slot - SMALL_SLOTS;
        (2 + large / 128, (large % 128) * 512 * 512 * 4)
    }
}
pub(super) const SPRITE_SHADER: &str = r#"
const SPRITE_CLASS:u32=0u;
struct Camera { view:vec4<f32>, info:vec4<f32> };
struct Item { pos:vec4<f32>, size:vec4<f32>, color:vec4<f32>, extent:vec4<f32> };
@group(0) @binding(0) var<storage,read> items:array<Item>;
@group(0) @binding(1) var<storage,read> pixels0:array<u32>;
@group(0) @binding(2) var<storage,read> pixels1:array<u32>;
@group(0) @binding(3) var<storage,read> pixels2:array<u32>;
@group(0) @binding(4) var<storage,read> pixels3:array<u32>;
@group(0) @binding(5) var<storage,read> pixels4:array<u32>;
@group(0) @binding(6) var<storage,read> pixels5:array<u32>;
@group(0) @binding(7) var<uniform> cam:Camera;
struct Out { @builtin(position) p:vec4<f32>, @location(0) uv:vec2<f32>, @location(1) @interpolate(flat) slot:u32, @location(2) color:vec4<f32>, @location(3) @interpolate(flat) shape:vec2<u32>, @location(4) depth_map:vec2<f32>, @location(5) @interpolate(flat) bounds:vec4<u32> };
@vertex fn vertex_main(@builtin(vertex_index) vi:u32,@builtin(instance_index) ii:u32)->Out{
 var o:Out;o.p=vec4<f32>(3.,3.,0.,1.);o.uv=vec2<f32>(0.);o.slot=0u;o.color=vec4<f32>(0.);o.shape=vec2<u32>(256u);o.depth_map=vec2<f32>(0.);o.bounds=vec4<u32>(0u);
 if(ii>=u32(cam.info.y)){return o;}
 let a=items[ii];if(u32(a.pos.w)/4096u!=SPRITE_CLASS){return o;}
 let points=array<vec2<f32>,6>(vec2<f32>(0.,0.),vec2<f32>(1.,0.),vec2<f32>(1.,1.),vec2<f32>(0.,0.),vec2<f32>(1.,1.),vec2<f32>(0.,1.));let uv=points[vi];
 let is_ground=u32(a.pos.w)%4096u>=2048u;var logical=a.pos.xyz;if(is_ground){logical+=vec3<f32>((uv.x-0.5)*a.size.x,(uv.y-0.5)*a.size.x,0.);}
 let foot=vec2<f32>((logical.x-logical.y)*0.5,-(logical.x+logical.y)*0.25+logical.z*1.5);let delta=vec2<f32>((uv.x-a.size.y)*a.size.x,(a.size.z-uv.y)*a.size.x*a.extent.y/a.extent.x);let turn=a.size.w;
 let rotated=select(vec2<f32>(delta.x*cos(turn)-delta.y*sin(turn),delta.x*sin(turn)+delta.y*cos(turn)),vec2<f32>(0.),is_ground);let pt=foot+rotated;let clip=(pt-cam.view.xy)/vec2<f32>(cam.view.z*cam.view.w,cam.view.z);
 let depth_key=logical.x+logical.y+logical.z*2.+select(0.25,0.,is_ground);
 o.p=vec4<f32>(clip.x,clip.y,clamp(0.95-depth_key*0.003,0.01,0.99),1.);o.uv=uv;o.slot=u32(a.pos.w)%2048u;o.color=a.color;o.shape=vec2<u32>(a.extent.xy);o.depth_map=vec2<f32>(depth_key,rotated.y);
 o.bounds=vec4<u32>(vec2<u32>(0u),o.shape);
 if(a.extent.z>=0. && a.extent.w>=0.){let lo=u32(a.extent.z);let hi=u32(a.extent.w);o.bounds=vec4<u32>(lo&1023u,lo>>10u,hi&1023u,hi>>10u);}
 return o;
}
struct FragmentOut { @location(0) color:vec4<f32>, @builtin(frag_depth) depth:f32 };
@fragment fn fragment_main(@location(0) uv:vec2<f32>,@location(1) @interpolate(flat) slot:u32,@location(2) tint:vec4<f32>,@location(3) @interpolate(flat) shape:vec2<u32>,@location(4) depth_map:vec2<f32>,@location(5) @interpolate(flat) bounds:vec4<u32>)->FragmentOut{
 let p=vec2<u32>(clamp(uv,vec2<f32>(0.),vec2<f32>(0.99999))*vec2<f32>(shape));
 if(any(p<bounds.xy)||any(p>=bounds.zw)){discard;}
 var page=slot/512u;var layer=slot%512u;var side=256u;
 if(slot>=1024u){page=2u+(slot-1024u)/128u;layer=(slot-1024u)%128u;side=512u;}
 let offset=layer*side*side+p.y*side+p.x;var rgba:u32;
 switch(page){case 0u:{rgba=pixels0[offset];}case 1u:{rgba=pixels1[offset];}case 2u:{rgba=pixels2[offset];}case 3u:{rgba=pixels3[offset];}case 4u:{rgba=pixels4[offset];}default:{rgba=pixels5[offset];}}
 let c=unpack4x8unorm(rgba)*tint;if(c.a<0.01){discard;}
 // A below-foot texel belongs to a closer point of the same support plane.
 // Keep the authored anchor and depth-test against real walls/floors; do not
 // lift the whole billboard or turn off occlusion to hide corpse cropping.
 var result:FragmentOut;result.color=c;result.depth=clamp(0.95-(depth_map.x+max(0.,-4.*depth_map.y))*0.003,0.01,0.99);return result;
}
"#;
fn build(width: u32, height: u32) -> Result<Renderer, String> {
    let caps = rex::probe_device_caps().map_err(|e| e.to_string())?;
    let vs = crate::viewport::compile_wgsl(VS, "v6-vs")?;
    let fs = crate::viewport::compile_wgsl(FS, "v6-fs")?;
    let sprite_shaders: Vec<_> = (0..3)
        .map(|class| {
            let source =
                SPRITE_SHADER.replace("SPRITE_CLASS:u32=0u", &format!("SPRITE_CLASS:u32={class}u"));
            Ok((
                crate::viewport::compile_wgsl(
                    &source.replace("vertex_main", "main"),
                    "v6-textured-vs",
                )?,
                crate::viewport::compile_wgsl(
                    &source.replace("fragment_main", "main"),
                    "v6-textured-fs",
                )?,
            ))
        })
        .collect::<Result<_, String>>()?;
    let mut resource_list = vec![
        rex::ResourceDesc::Buffer(rex::BufferDesc {
            size: 32,
            usage: rex::BufferUsage {
                uniform: true,
                ..Default::default()
            },
            data: None,
            device_local: false,
        }),
        rex::ResourceDesc::Buffer(rex::BufferDesc {
            size: (MAX * 48) as u64,
            usage: rex::BufferUsage {
                storage: true,
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
        rex::ResourceDesc::Buffer(rex::BufferDesc {
            size: (MAX * 64) as u64,
            usage: rex::BufferUsage {
                storage: true,
                ..Default::default()
            },
            data: None,
            device_local: false,
        }),
        rex::ResourceDesc::Buffer(rex::BufferDesc {
            size: PAGE_BYTES as u64,
            usage: rex::BufferUsage {
                storage: true,
                ..Default::default()
            },
            data: None,
            device_local: false,
        }),
    ];
    for _ in 1..6 {
        resource_list.push(rex::ResourceDesc::Buffer(rex::BufferDesc {
            size: PAGE_BYTES as u64,
            usage: rex::BufferUsage {
                storage: true,
                ..Default::default()
            },
            data: None,
            device_local: false,
        }));
    }
    let resources = Box::leak(resource_list.into_boxed_slice());
    let mut pass_list = vec![rex::Pass::Raster(rex::RasterPass {
        name: "sentinels_v6_instanced_isometric",
        blend: rex::BlendMode::Opaque,
        vs_spirv: vs,
        fs_spirv: fs,
        vertex: rex::VertexData::Pull,
        draw: rex::DrawSpec::Direct {
            vertex_count: 18,
            instance_count: MAX as u32,
            first_vertex: 0,
            first_instance: 0,
        },
        colors: vec![rex::ColorAttachmentRef {
            res: 2,
            clear: Some([0.025, 0.04, 0.06, 1.]),
        }],
        depth: Some(rex::DepthAttachmentRef {
            res: 3,
            clear: Some(1.),
        }),
        viewport: None,
        bindings: rex::Bindings {
            uniform: Some(rex::UniformRef {
                res: 0,
                offset: 0,
                size: 32,
            }),
            storage_buffers: vec![1],
            ..Default::default()
        },
        conservative: None,
    })];
    for (class, (vs, fs)) in sprite_shaders.into_iter().enumerate() {
        pass_list.push(rex::Pass::Raster(rex::RasterPass {
            name: match class {
                0 => "sentinels_v6_real_frame_assets",
                1 => "sentinels_v6_alpha_effects",
                _ => "sentinels_v6_additive_effects",
            },
            blend: match class {
                0 => rex::BlendMode::AlphaDepth,
                1 => rex::BlendMode::Alpha,
                _ => rex::BlendMode::Additive,
            },
            vs_spirv: vs,
            fs_spirv: fs,
            vertex: rex::VertexData::Pull,
            draw: rex::DrawSpec::Direct {
                vertex_count: 6,
                instance_count: MAX as u32,
                first_vertex: 0,
                first_instance: 0,
            },
            colors: vec![rex::ColorAttachmentRef {
                res: 2,
                clear: None,
            }],
            depth: Some(rex::DepthAttachmentRef {
                res: 3,
                clear: None,
            }),
            viewport: None,
            bindings: rex::Bindings {
                uniform: Some(rex::UniformRef {
                    res: 0,
                    offset: 0,
                    size: 32,
                }),
                storage_buffers: vec![4, 5, 6, 7, 8, 9, 10],
                ..Default::default()
            },
            conservative: None,
        }));
    }
    let passes = Box::leak(pass_list.into_boxed_slice());
    let mut barriers: Vec<&'static [(u32, rex::TargetState)]> = vec![Box::leak(
        vec![
            (0, rex::TargetState::UniformRead),
            (1, rex::TargetState::StorageReadWrite),
            (2, rex::TargetState::ColorAttachmentWrite),
            (3, rex::TargetState::DepthAttachmentWrite),
        ]
        .into_boxed_slice(),
    )];
    for _ in 0..3 {
        barriers.push(Box::leak(
            vec![
                (0, rex::TargetState::UniformRead),
                (4, rex::TargetState::StorageReadWrite),
                (5, rex::TargetState::StorageReadWrite),
                (6, rex::TargetState::StorageReadWrite),
                (7, rex::TargetState::StorageReadWrite),
                (8, rex::TargetState::StorageReadWrite),
                (9, rex::TargetState::StorageReadWrite),
                (10, rex::TargetState::StorageReadWrite),
                (2, rex::TargetState::ColorAttachmentWrite),
                (3, rex::TargetState::DepthAttachmentWrite),
            ]
            .into_boxed_slice(),
        ));
    }
    let session = rex::DeviceFrameSession::new(
        resources,
        passes,
        Box::leak(barriers.into_boxed_slice()),
        Box::leak(vec![rex::Readback::Texture { res: 2 }].into_boxed_slice()),
        2,
    )
    .map_err(|e| e.to_string())?;
    Ok(Renderer {
        width,
        height,
        session,
        device: caps.device_name,
        frames: BTreeMap::new(),
        used: vec![0; FRAME_SLOTS],
        alpha_bounds: vec![AlphaBounds::FULL; FRAME_SLOTS],
        clock: 0,
    })
}
fn upload_sprites(
    r: &mut Renderer,
    requests: &[Sprite],
    ground: &[Sprite],
    update: &mut rex::FrameUpdate,
) -> Result<usize, String> {
    r.clock += 1;
    let mut descriptors: HashMap<(&str, &str, usize, u64), Option<Arc<assets::Frame>>> =
        HashMap::new();
    let mut resolved: Vec<(&Sprite, Arc<assets::Frame>)> =
        Vec::with_capacity(requests.len() + ground.len());
    for sprite in ground.iter().chain(requests) {
        let key = (
            sprite.asset.as_str(),
            sprite.action.as_str(),
            sprite.direction,
            (sprite.seconds * 64.).max(0.) as u64,
        );
        if !descriptors.contains_key(&key) {
            descriptors.insert(key, assets::describe(sprite)?.map(Arc::new));
        }
        if let Some(frame) = descriptors[&key].clone() {
            resolved.push((sprite, frame));
        }
    }
    if resolved.len() > MAX {
        return Err("native sprite instance budget exceeded".into());
    }
    let class = |s: &Sprite, f: &assets::Frame| {
        if f.effect || s.transient {
            if f.additive {
                2u32
            } else {
                1u32
            }
        } else {
            0u32
        }
    };
    resolved.sort_by(|(a, af), (b, bf)| {
        class(a, af)
            .cmp(&class(b, bf))
            .then_with(|| b.ground.cmp(&a.ground))
            .then_with(|| (a.x + a.y + a.z * 2.).total_cmp(&(b.x + b.y + b.z * 2.)))
    });
    let needed: BTreeSet<&str> = resolved.iter().map(|(_, f)| f.key.as_str()).collect();
    let small: BTreeSet<_> = resolved
        .iter()
        .filter(|(_, f)| f.tile_size == 256)
        .map(|(_, f)| f.key.as_str())
        .collect();
    let large: BTreeSet<_> = resolved
        .iter()
        .filter(|(_, f)| f.tile_size == 512)
        .map(|(_, f)| f.key.as_str())
        .collect();
    if small.len() > SMALL_SLOTS || large.len() > LARGE_SLOTS {
        return Err(format!(
            "native frame banks exceed budget: {} normal, {} full-density",
            small.len(),
            large.len()
        ));
    }
    if needed.len() > FRAME_SLOTS {
        return Err(format!(
            "{} simultaneous unique native frames exceed cache budget",
            needed.len()
        ));
    }
    let mut buffer = Vec::with_capacity(resolved.len() * 64);
    let mut reserved: BTreeSet<usize> = r
        .frames
        .iter()
        .filter(|(key, _)| needed.contains(key.as_str()))
        .map(|(_, slot)| *slot)
        .collect();
    for (s, f) in &resolved {
        let slot = if let Some(slot) = r.frames.get(&f.key) {
            *slot
        } else {
            let range = if f.tile_size == 512 {
                SMALL_SLOTS..FRAME_SLOTS
            } else {
                0..SMALL_SLOTS
            };
            let free = range
                .filter(|slot| !reserved.contains(slot))
                .min_by_key(|slot| r.used[*slot])
                .ok_or("native frame cache exhausted")?;
            r.frames.retain(|_, slot| *slot != free);
            let pixels = assets::pixels(f)?;
            // Slot reuse must replace the previous frame's bounds at exactly
            // the same time as its pixel upload (including fully empty pages).
            r.alpha_bounds[free] = AlphaBounds::from_pixels(&pixels, f.tile_size, f.sample_size);
            let (page, offset) = cache_location(free);
            update.buffer_uploads.push((
                rex::StableResourceId(6 + page as u64),
                offset as u64,
                pixels,
            ));
            r.frames.insert(f.key.clone(), free);
            reserved.insert(free);
            free
        };
        r.used[slot] = r.clock;
        for value in [
            s.x as f32,
            s.y as f32,
            s.z as f32,
            slot as f32 + if s.ground { 2048. } else { 0. } + class(s, f) as f32 * 4096.,
            if s.ground {
                s.scale as f32
            } else {
                f.span * s.scale as f32
            },
            f.pivot[0],
            f.pivot[1],
            s.rotation,
            s.tint[0],
            s.tint[1],
            s.tint[2],
            s.tint[3],
            f.sample_size[0] as f32,
            f.sample_size[1] as f32,
            r.alpha_bounds[slot].0[0],
            r.alpha_bounds[slot].0[1],
        ] {
            buffer.extend_from_slice(&value.to_le_bytes());
        }
    }
    if !buffer.is_empty() {
        update
            .buffer_uploads
            .push((rex::StableResourceId(5), 0, buffer));
    }
    Ok(resolved.len())
}
pub fn render(
    _scene: &Scene,
    width: u32,
    height: u32,
    want_readback: bool,
    want_stats: bool,
) -> Result<crate::viewport::FramePixels, String> {
    let total_start = std::time::Instant::now();
    let staged = STAGED
        .get()
        .ok_or("no V6 presentation state")?
        .lock()
        .unwrap()
        .clone()
        .ok_or("no V6 world")?;
    let visual_seconds = visual_time(&staged);
    let mut scene = compose(&staged.world, &staged.view, visual_seconds);
    interpolate(&mut scene, &staged);
    let compose_ms = total_start.elapsed().as_secs_f64() * 1000.;
    let prepare_start = std::time::Instant::now();
    let records = &scene.records;
    if records.len() > MAX {
        return Err(format!("V6 geometry capacity exceeded: {}", records.len()));
    }
    let mut data = Vec::with_capacity(records.len() * 48);
    for record in records {
        for value in record {
            data.extend_from_slice(&value.to_le_bytes());
        }
    }
    let mut camera = Vec::with_capacity(32);
    for (i, value) in scene.view.iter().enumerate() {
        camera.extend_from_slice(
            &(if i == 3 {
                width as f32 / height as f32
            } else {
                *value
            })
            .to_le_bytes(),
        );
    }
    for value in [records.len() as f32, 0., 0., 0.] {
        camera.extend_from_slice(&value.to_le_bytes());
    }
    let mut guard = GPU.get_or_init(|| Mutex::new(None)).lock().unwrap();
    if guard.as_ref().map(|r| (r.width, r.height)) != Some((width, height)) {
        *guard = Some(build(width, height)?);
    }
    let r = guard.as_mut().unwrap();
    let mut update = rex::FrameUpdate::default();
    let sprite_count = match upload_sprites(r, &scene.sprites, &scene.terrain.sprites, &mut update)
    {
        Ok(n) => n,
        Err(e) => {
            r.frames.clear();
            r.used.fill(0);
            return Err(e);
        }
    };
    camera[20..24].copy_from_slice(&(sprite_count as f32).to_le_bytes());
    update
        .buffer_uploads
        .push((rex::StableResourceId(1), 0, camera));
    if !data.is_empty() {
        update
            .buffer_uploads
            .push((rex::StableResourceId(2), 0, data));
    }
    update.readback_subset = if want_readback { Some(vec![0]) } else { None };
    let prepare_ms = prepare_start.elapsed().as_secs_f64() * 1000.;
    let execute_start = std::time::Instant::now();
    let provenance = match r.session.next_provenance_with_update(&update) {
        Ok(p) => p,
        Err(e) => {
            r.frames.clear();
            r.used.fill(0);
            return Err(e.to_string());
        }
    };
    let provenance_ns=execute_start.elapsed().as_secs_f64()*1e9;
    let execute_call_start=std::time::Instant::now();
    let result = match r.session.execute_with_frame_update(&provenance, &update) {
        Ok(value) => value,
        Err(e) => {
            r.frames.clear();
            r.used.fill(0);
            return Err(e.to_string());
        }
    };
    let execute_call_ns=execute_call_start.elapsed().as_secs_f64()*1e9;
    let execute_ms = execute_start.elapsed().as_secs_f64() * 1000.;
    crate::sentinels_v6_backend_metrics::record(0,provenance_ns,execute_call_ns,&result.telemetry);
    let stats_start = std::time::Instant::now();
    let rgba8 = if want_readback {
        result
            .readbacks
            .into_iter()
            .next()
            .ok_or("missing native image")?
    } else {
        vec![]
    };
    let nonzero = if want_stats {
        rgba8
            .chunks_exact(4)
            .filter(|p| p[0] > 20 || p[1] > 20 || p[2] > 25)
            .count()
    } else {
        0
    };
    let mesh_classes = scene
        .terrain.sprites
        .iter().chain(&scene.sprites)
        .map(|s| s.asset.as_str())
        .collect::<BTreeSet<_>>()
        .len();
    record_timing(
        0,
        [
            compose_ms,
            prepare_ms,
            execute_ms,
            stats_start.elapsed().as_secs_f64() * 1000.,
            total_start.elapsed().as_secs_f64() * 1000.,
        ],
    );
    Ok(crate::viewport::FramePixels {
        width,
        height,
        rgba8,
        device_name: r.device.clone(),
        draws: 4,
        truncated: false,
        nonzero,
        triangles: records.len() * 6 + sprite_count * 2,
        mesh_fallbacks: scene.fallbacks,
        mesh_classes,
        imported: sprite_count > 0,
    })
}

/// close() 的 GPU 段(原 close 末尾那条语句):丢弃 V6 GPU 会话。
pub(super) fn close_gpu() {
    if let Some(gpu) = GPU.get() {
        *gpu.lock().unwrap() = None;
    }
}
