//! Persistent native Raster/Compute/RayQuery frame (RXS-0409/0410).
//! Safe self-referential ownership drops the Vulkan session before its graph.
use super::{scene, shaders, types::*};
use rurix_rt::{render_exec as rex, vk};
use self_cell::self_cell;

const FRAME: u32 = 0;
const VERTICES: u32 = 1;
const INSTANCES: u32 = 2;
const MATERIALS: u32 = 3;
const TEXTURES: u32 = 4;
const LIGHTS: u32 = 5;
const GPOS: u32 = 6;
const GNRM: u32 = 7;
const GBASE: u32 = 8;
const GEMIT: u32 = 9;
const MOTION: u32 = 10;
const DEPTH: u32 = 11;
const DIRECT: u32 = 12;
const GI: u32 = 13;
const REFLECTION: u32 = 14;
const HDR: u32 = 15;
const HISTORY0: u32 = 16;
const HISTORY1: u32 = 17;
const HPOS0: u32 = 18;
const HPOS1: u32 = 19;
const BLOOM0: u32 = 20;
const BLOOM1: u32 = 21;
const FINAL: u32 = 22;
const FOG_VOLUMES: u32 = 23;
const DECALS: u32 = 24;
const FOG_IMAGE: u32 = 25;
const DRAW_REFS: u32 = 26;
const SIGNAL_HISTORY0: u32 = 27;
const SIGNAL_HISTORY1: u32 = 28;
const FILTER_GI0: u32 = 29;
const FILTER_REFL0: u32 = 30;
const FILTER_GI1: u32 = 31;
const FILTER_REFL1: u32 = 32;
const TILE_MASKS: u32 = 33;
const EXTERNAL0: u32 = 34;
const FRAME_BYTES: u64 = 256;

struct Assets {
    scene: SceneSnapshot,
    packed: scene::PackedScene,
    instances: Vec<u8>,
    lights: Vec<u8>,
    fog: Vec<u8>,
    decals: Vec<u8>,
    extent: [u32; 2],
    internal: [u32; 2],
    external: Option<[ExternalOutput; 2]>,
    depth_prepass: bool,
    programs: &'static shaders::Programs,
}
struct Descriptors<'a> {
    resources: Vec<rex::ResourceDesc<'a>>,
    passes: Vec<rex::Pass<'a>>,
    plans: Vec<Vec<(u32, rex::TargetState)>>,
    readbacks: Vec<rex::Readback>,
    blas: Vec<&'a [f32]>,
    as_instances: Vec<vk::RayQueryInstanceDesc>,
    transforms: Vec<[f32; 12]>,
    temporal_pass: u32,
    signal_temporal_pass: u32,
    signal_spatial_passes: [u32; 3],
    bloom_pass: u32,
    post_pass: u32,
    pack_pass: Option<u32>,
}
self_cell!(
    struct GraphCell {
        owner: Assets,
        #[covariant]
        dependent: Descriptors,
    }
);
struct BarrierRefs<'a> {
    refs: Vec<&'a [(u32, rex::TargetState)]>,
}
self_cell!(
    struct BarrierCell {
        owner: GraphCell,
        #[covariant]
        dependent: BarrierRefs,
    }
);
type NativeSession<'a> = rex::DeviceFrameSession<'a>;
self_cell!(
    struct SessionCell {
        owner: BarrierCell,
        #[not_covariant]
        dependent: NativeSession,
    }
);

fn buffer<'a>(
    data: Option<&'a [u8]>,
    size: u64,
    device: bool,
    uniform: bool,
) -> rex::ResourceDesc<'a> {
    rex::ResourceDesc::Buffer(rex::BufferDesc {
        size: size.max(4),
        usage: rex::BufferUsage {
            storage: !uniform,
            uniform,
            ..Default::default()
        },
        data,
        device_local: device,
    })
}
fn image<'a>(w: u32, h: u32, format: rex::TexFormat, color: bool) -> rex::ResourceDesc<'a> {
    rex::ResourceDesc::Texture(rex::TextureDesc {
        width: w,
        height: h,
        format,
        usage: rex::TextureUsage {
            storage: !format.is_depth(),
            color,
            depth: format.is_depth(),
            ..Default::default()
        },
        data: None,
    })
}
fn uniform() -> Option<rex::UniformRef> {
    Some(rex::UniformRef {
        res: FRAME,
        offset: 0,
        size: FRAME_BYTES,
    })
}
fn compute<'a>(
    name: &'static str,
    spv: &'a [u8],
    extent: [u32; 2],
    bindings: rex::Bindings,
) -> rex::Pass<'a> {
    rex::Pass::Compute(rex::ComputePass {
        name,
        spirv: spv,
        entry: None,
        dispatch: rex::DispatchSpec::Direct([extent[0].div_ceil(8), extent[1].div_ceil(8), 1]),
        bindings,
    })
}
fn images_bind(images: Vec<u32>) -> rex::Bindings {
    rex::Bindings {
        storage_images: images,
        uniform: uniform(),
        ..Default::default()
    }
}
fn image_plan(images: &[u32]) -> Vec<(u32, rex::TargetState)> {
    images
        .iter()
        .map(|&i| (i, rex::TargetState::StorageImageReadWrite))
        .chain([(FRAME, rex::TargetState::UniformRead)])
        .collect()
}
fn raster_bind() -> rex::Bindings {
    rex::Bindings {
        storage_buffers: vec![VERTICES, INSTANCES, MATERIALS, TEXTURES, LIGHTS, DRAW_REFS],
        uniform: uniform(),
        ..Default::default()
    }
}
fn lighting_bind(out0: u32, out1: u32) -> rex::Bindings {
    rex::Bindings {
        accel_structs: vec![0],
        storage_buffers: vec![VERTICES, INSTANCES, MATERIALS, TEXTURES, LIGHTS],
        storage_images: vec![GPOS, GNRM, GBASE, GEMIT, out0, out1],
        uniform: uniform(),
        ..Default::default()
    }
}
fn signal_temporal_bind(previous: u32, current: u32) -> rex::Bindings {
    rex::Bindings {
        storage_buffers: vec![previous, current],
        storage_images: vec![GI, REFLECTION, GPOS, GNRM, MOTION, FILTER_GI0, FILTER_REFL0],
        uniform: uniform(),
        ..Default::default()
    }
}
fn signal_spatial_bind(
    history: u32,
    src_d: u32,
    src_s: u32,
    dst_d: u32,
    dst_s: u32,
    step: u32,
) -> rex::Bindings {
    rex::Bindings {
        storage_buffers: vec![history],
        storage_images: vec![src_d, src_s, dst_d, dst_s],
        uniform: uniform(),
        push_constants: step.to_le_bytes().to_vec(),
        ..Default::default()
    }
}
fn scene_plan() -> Vec<(u32, rex::TargetState)> {
    [VERTICES, INSTANCES, MATERIALS, TEXTURES, LIGHTS]
        .into_iter()
        .map(|i| (i, rex::TargetState::ShaderRead))
        .chain([(FRAME, rex::TargetState::UniformRead)])
        .collect()
}

fn describe(a: &Assets) -> Descriptors<'_> {
    let [w, h] = a.internal;
    let [ow, oh] = a.extent;
    let hw = w.div_ceil(2);
    let hh = h.div_ceil(2);
    let bw = ow.div_ceil(2);
    let bh = oh.div_ceil(2);
    let p = a.programs;
    let mut resources = vec![
        buffer(None, FRAME_BYTES, false, true),
        buffer(
            Some(&a.packed.vertices),
            a.packed.vertices.len() as u64,
            true,
            false,
        ),
        buffer(Some(&a.instances), a.instances.len() as u64, false, false),
        buffer(
            Some(&a.packed.materials),
            a.packed.materials.len() as u64,
            false,
            false,
        ),
        buffer(
            Some(&a.packed.textures),
            a.packed.textures.len() as u64,
            true,
            false,
        ),
        buffer(Some(&a.lights), a.lights.len() as u64, false, false),
        image(w, h, rex::TexFormat::Rgba32Float, true),
        image(w, h, rex::TexFormat::Rgba16Float, true),
        image(w, h, rex::TexFormat::Rgba16Float, true),
        image(w, h, rex::TexFormat::Rgba16Float, true),
        image(w, h, rex::TexFormat::Rgba16Float, true),
        image(w, h, rex::TexFormat::Depth32Float, false),
        image(w, h, rex::TexFormat::Rgba16Float, false),
        image(hw, hh, rex::TexFormat::Rgba16Float, false),
        image(hw, hh, rex::TexFormat::Rgba16Float, false),
        image(w, h, rex::TexFormat::Rgba16Float, true),
        image(ow, oh, rex::TexFormat::Rgba16Float, false),
        image(ow, oh, rex::TexFormat::Rgba16Float, false),
        image(ow, oh, rex::TexFormat::Rgba32Float, false),
        image(ow, oh, rex::TexFormat::Rgba32Float, false),
        image(bw, bh, rex::TexFormat::Rgba16Float, false),
        image(bw, bh, rex::TexFormat::Rgba16Float, false),
        image(ow, oh, rex::TexFormat::Rgba8Unorm, false),
        buffer(Some(&a.fog), a.fog.len() as u64, false, false),
        buffer(Some(&a.decals), a.decals.len() as u64, false, false),
        image(hw, hh, rex::TexFormat::Rgba16Float, false),
        buffer(
            Some(&a.packed.opaque_references),
            a.packed.opaque_references.len() as u64,
            true,
            false,
        ),
        buffer(None, u64::from(hw) * u64::from(hh) * 80, true, false),
        buffer(None, u64::from(hw) * u64::from(hh) * 80, true, false),
        image(hw, hh, rex::TexFormat::Rgba16Float, false),
        image(hw, hh, rex::TexFormat::Rgba16Float, false),
        image(hw, hh, rex::TexFormat::Rgba16Float, false),
        image(hw, hh, rex::TexFormat::Rgba16Float, false),
        buffer(
            None,
            u64::from(w.div_ceil(8)) * u64::from(h.div_ceil(8)) * 32,
            true,
            false,
        ),
    ];
    if let Some(outputs) = a.external {
        for output in outputs {
            resources.push(buffer(None, output.size, true, false));
        }
    }
    let mut passes = Vec::new();
    let mut plans = Vec::new();
    if a.depth_prepass {
        passes.push(rex::Pass::Raster(rex::RasterPass {
            name: "realtime_depth_prepass",
            vs_spirv: &p.depth_vs,
            fs_spirv: &p.depth_fs,
            vertex: rex::VertexData::Pull,
            draw: rex::DrawSpec::Direct {
                vertex_count: (a.packed.opaque_references.len() / 8) as u32,
                instance_count: 1,
                first_vertex: 0,
                first_instance: 0,
            },
            colors: Vec::new(),
            depth: Some(rex::DepthAttachmentRef {
                res: DEPTH,
                clear: Some(1.),
            }),
            viewport: None,
            bindings: raster_bind(),
            conservative: None,
        }));
        let mut plan = scene_plan();
        plan.extend([
            (DRAW_REFS, rex::TargetState::ShaderRead),
            (DEPTH, rex::TargetState::DepthAttachmentWrite),
        ]);
        plans.push(plan);
    }
    let raster = rex::RasterPass {
        name: "realtime_gbuffer_batch",
        vs_spirv: &p.vs,
        fs_spirv: &p.gbuffer,
        vertex: rex::VertexData::Pull,
        draw: rex::DrawSpec::Direct {
            vertex_count: (a.packed.opaque_references.len() / 8) as u32,
            instance_count: 1,
            first_vertex: 0,
            first_instance: 0,
        },
        colors: [GPOS, GNRM, GBASE, GEMIT, MOTION]
            .into_iter()
            .map(|res| rex::ColorAttachmentRef {
                res,
                clear: Some([0.; 4]),
            })
            .collect(),
        depth: Some(rex::DepthAttachmentRef {
            res: DEPTH,
            clear: if a.depth_prepass { None } else { Some(1.) },
        }),
        viewport: None,
        bindings: raster_bind(),
        conservative: None,
    };
    passes.push(rex::Pass::Raster(raster));
    let mut plan = scene_plan();
    plan.push((DRAW_REFS, rex::TargetState::ShaderRead));
    plan.extend(
        [GPOS, GNRM, GBASE, GEMIT, MOTION].map(|r| (r, rex::TargetState::ColorAttachmentWrite)),
    );
    plan.push((DEPTH, rex::TargetState::DepthAttachmentWrite));
    plans.push(plan);
    let decal = rex::Bindings {
        storage_buffers: vec![VERTICES, INSTANCES, MATERIALS, TEXTURES, DECALS],
        storage_images: vec![GPOS, GNRM, GBASE, GEMIT],
        uniform: uniform(),
        ..Default::default()
    };
    let mut plan = image_plan(&decal.storage_images);
    plan.extend(
        [VERTICES, INSTANCES, MATERIALS, TEXTURES, DECALS]
            .map(|r| (r, rex::TargetState::ShaderRead)),
    );
    passes.push(compute("realtime_decal_project", &p.decal, [w, h], decal));
    plans.push(plan);
    let tile_bind = rex::Bindings {
        storage_buffers: vec![LIGHTS, TILE_MASKS],
        storage_images: vec![GPOS],
        uniform: uniform(),
        ..Default::default()
    };
    passes.push(compute(
        "realtime_tile_light_lists",
        &p.tile_lights,
        [w, h],
        tile_bind,
    ));
    plans.push(vec![
        (LIGHTS, rex::TargetState::ShaderRead),
        (TILE_MASKS, rex::TargetState::StorageWrite),
        (GPOS, rex::TargetState::StorageImageReadWrite),
        (FRAME, rex::TargetState::UniformRead),
    ]);
    let mut direct = lighting_bind(DIRECT, HDR);
    direct.storage_buffers.push(TILE_MASKS);
    let mut plan = image_plan(&direct.storage_images);
    plan.extend(scene_plan());
    plan.push((TILE_MASKS, rex::TargetState::ShaderRead));
    passes.push(compute("realtime_direct_shadow", &p.direct, [w, h], direct));
    plans.push(plan);
    let indirect = lighting_bind(GI, REFLECTION);
    let mut plan = image_plan(&indirect.storage_images);
    plan.extend(scene_plan());
    passes.push(compute(
        "realtime_indirect_reflection",
        &p.indirect,
        [hw, hh],
        indirect,
    ));
    plans.push(plan);
    let signal_temporal_pass = passes.len() as u32;
    let bind = signal_temporal_bind(SIGNAL_HISTORY0, SIGNAL_HISTORY1);
    let mut plan = image_plan(&bind.storage_images);
    plan.extend([
        (SIGNAL_HISTORY0, rex::TargetState::StorageReadWrite),
        (SIGNAL_HISTORY1, rex::TargetState::StorageReadWrite),
    ]);
    plans.push(plan);
    passes.push(compute(
        "realtime_signal_temporal",
        &p.signal_temporal,
        [hw, hh],
        bind,
    ));
    let signal_spatial_passes = [
        passes.len() as u32,
        passes.len() as u32 + 1,
        passes.len() as u32 + 2,
    ];
    for (step, src_d, src_s, dst_d, dst_s, name) in [
        (
            1u32,
            FILTER_GI0,
            FILTER_REFL0,
            FILTER_GI1,
            FILTER_REFL1,
            "realtime_signal_spatial_1",
        ),
        (
            2,
            FILTER_GI1,
            FILTER_REFL1,
            FILTER_GI0,
            FILTER_REFL0,
            "realtime_signal_spatial_2",
        ),
        (
            4,
            FILTER_GI0,
            FILTER_REFL0,
            FILTER_GI1,
            FILTER_REFL1,
            "realtime_signal_spatial_4",
        ),
    ] {
        let bind = signal_spatial_bind(SIGNAL_HISTORY1, src_d, src_s, dst_d, dst_s, step);
        let mut plan = image_plan(&bind.storage_images);
        plan.push((SIGNAL_HISTORY1, rex::TargetState::ShaderRead));
        plan.push((SIGNAL_HISTORY0, rex::TargetState::ShaderRead));
        plans.push(plan);
        passes.push(compute(name, &p.signal_spatial, [hw, hh], bind));
    }
    let fog = rex::Bindings {
        accel_structs: vec![0],
        storage_buffers: vec![
            VERTICES,
            INSTANCES,
            MATERIALS,
            TEXTURES,
            LIGHTS,
            FOG_VOLUMES,
        ],
        storage_images: vec![GPOS, FOG_IMAGE],
        uniform: uniform(),
        ..Default::default()
    };
    let mut plan = image_plan(&fog.storage_images);
    plan.extend(scene_plan());
    plan.push((FOG_VOLUMES, rex::TargetState::ShaderRead));
    passes.push(compute(
        "realtime_local_volumetric_fog",
        &p.fog,
        [hw, hh],
        fog,
    ));
    plans.push(plan);
    let bind = images_bind(vec![
        DIRECT,
        FILTER_GI1,
        FILTER_REFL1,
        GPOS,
        GNRM,
        HDR,
        FOG_IMAGE,
    ]);
    plans.push(image_plan(&bind.storage_images));
    passes.push(compute(
        "realtime_hdr_combine_fog",
        &p.combine,
        [w, h],
        bind,
    ));
    for (ii, inst) in a.scene.instances.iter().enumerate() {
        let mesh = &a.scene.meshes[inst.mesh];
        if a.scene.materials[mesh.material].alpha_mode != AlphaMode::Blend {
            continue;
        }
        let range = a.packed.ranges[inst.mesh];
        let raster = rex::RasterPass {
            name: "realtime_thin_glass",
            vs_spirv: &p.glass_vs,
            fs_spirv: &p.glass,
            vertex: rex::VertexData::Pull,
            draw: rex::DrawSpec::Direct {
                vertex_count: range.1,
                instance_count: 1,
                first_vertex: range.0,
                first_instance: ii as u32,
            },
            colors: vec![
                rex::ColorAttachmentRef {
                    res: HDR,
                    clear: None,
                },
                rex::ColorAttachmentRef {
                    res: MOTION,
                    clear: None,
                },
            ],
            depth: Some(rex::DepthAttachmentRef {
                res: DEPTH,
                clear: None,
            }),
            viewport: None,
            bindings: raster_bind(),
            conservative: None,
        };
        passes.push(rex::Pass::RasterWithBlend(raster, rex::BlendMode::Alpha));
        let mut plan = scene_plan();
        plan.extend([
            (HDR, rex::TargetState::ColorAttachmentWrite),
            (MOTION, rex::TargetState::ColorAttachmentWrite),
            (DEPTH, rex::TargetState::DepthAttachmentWrite),
        ]);
        plans.push(plan);
    }
    let temporal_pass = passes.len() as u32;
    let bind = images_bind(vec![HDR, GPOS, MOTION, HISTORY0, HPOS0, HISTORY1, HPOS1]);
    plans.push(image_plan(&bind.storage_images));
    passes.push(compute(
        "realtime_temporal_upscale",
        &p.temporal,
        [ow, oh],
        bind,
    ));
    let bloom_pass = passes.len() as u32;
    for (mode, src, dst, name) in [
        (0u32, HISTORY1, BLOOM0, "realtime_bloom_extract"),
        (1, BLOOM0, BLOOM1, "realtime_bloom_horizontal"),
        (2, BLOOM1, BLOOM0, "realtime_bloom_vertical"),
    ] {
        let mut bind = images_bind(vec![src, dst]);
        bind.push_constants = mode.to_le_bytes().to_vec();
        plans.push(image_plan(&bind.storage_images));
        passes.push(compute(name, &p.bloom, [bw, bh], bind));
    }
    let post_pass = passes.len() as u32;
    let bind = images_bind(vec![HISTORY1, BLOOM0, FINAL]);
    plans.push(image_plan(&bind.storage_images));
    passes.push(compute("realtime_aces_sdr", &p.post, [ow, oh], bind));
    let mut pack_pass = None;
    if let Some(outputs) = a.external {
        pack_pass = Some(passes.len() as u32);
        let bind = rex::Bindings {
            storage_buffers: vec![EXTERNAL0],
            storage_images: vec![FINAL],
            push_constants: (outputs[0].row_pitch / 4).to_le_bytes().to_vec(),
            ..Default::default()
        };
        plans.push(vec![
            (FINAL, rex::TargetState::StorageImageReadWrite),
            (EXTERNAL0, rex::TargetState::StorageWrite),
            (EXTERNAL0 + 1, rex::TargetState::StorageWrite),
        ]);
        passes.push(compute("realtime_external_pack", &p.pack, [ow, oh], bind));
    }
    let mut readbacks = vec![rex::Readback::Texture { res: FINAL }];
    #[cfg(test)]
    readbacks.extend(
        [GPOS, GNRM, GBASE, GEMIT, DIRECT, GI, REFLECTION, HDR]
            .map(|res| rex::Readback::Texture { res }),
    );
    #[cfg(test)]
    readbacks.extend([
        rex::Readback::Buffer {
            res: LIGHTS,
            offset: 0,
            size: a.lights.len() as u64,
        },
        rex::Readback::Buffer {
            res: FRAME,
            offset: 0,
            size: FRAME_BYTES,
        },
    ]);
    #[cfg(test)]
    readbacks
        .extend([FILTER_GI1, FILTER_REFL1, FOG_IMAGE].map(|res| rex::Readback::Texture { res }));
    #[cfg(test)]
    readbacks.extend(
        [SIGNAL_HISTORY0, SIGNAL_HISTORY1].map(|res| rex::Readback::Buffer {
            res,
            offset: 0,
            size: u64::from(hw) * u64::from(hh) * 80,
        }),
    );
    #[cfg(test)]
    readbacks.extend([MOTION, HPOS0, HPOS1].map(|res| rex::Readback::Texture { res }));
    #[cfg(test)]
    readbacks.push(rex::Readback::Buffer {
        res: TILE_MASKS,
        offset: 0,
        size: u64::from(w.div_ceil(8)) * u64::from(h.div_ceil(8)) * 32,
    });
    Descriptors {
        resources,
        passes,
        plans,
        readbacks,
        blas: a.packed.triangles.iter().map(Vec::as_slice).collect(),
        as_instances: a
            .scene
            .instances
            .iter()
            .enumerate()
            .map(|(i, v)| vk::RayQueryInstanceDesc {
                blas: v.mesh as u32,
                custom_index: i as u32,
                mask: 255,
                sbt_record_offset: 0,
            })
            .collect(),
        transforms: a
            .scene
            .instances
            .iter()
            .map(|i| scene::as_transform(&i.transform))
            .collect(),
        temporal_pass,
        signal_temporal_pass,
        signal_spatial_passes,
        bloom_pass,
        post_pass,
        pack_pass,
    }
}

fn build(a: Assets) -> Result<SessionCell, String> {
    let graph = GraphCell::new(a, describe);
    let barriers = BarrierCell::new(graph, |g| BarrierRefs {
        refs: g
            .borrow_dependent()
            .plans
            .iter()
            .map(Vec::as_slice)
            .collect(),
    });
    SessionCell::try_new(barriers, |owner| {
        let graph = owner.borrow_owner();
        let d = graph.borrow_dependent();
        let a = graph.borrow_owner();
        let acceleration = [rex::AccelStructDesc {
            scene: vk::RayQuerySceneDesc {
                blas_triangles: &d.blas,
                instances: &d.as_instances,
            },
            transforms: Some(&d.transforms),
            updatable_blas: &[],
        }];
        let imports = a
            .external
            .map(|outs| vec![(EXTERNAL0, outs[0].handle), (EXTERNAL0 + 1, outs[1].handle)])
            .unwrap_or_default();
        let mut native = rex::DeviceFrameSession::new_with_imported_d3d12_textures(
            &d.resources,
            &d.passes,
            &owner.borrow_dependent().refs,
            &d.readbacks,
            2,
            &acceleration,
            &[],
            &imports,
        )?;
        if let Some(outputs) = a.external {
            native.enable_imported_buffer_roundtrip()?;
            let actual = native
                .physical_device_luid()
                .ok_or("REALTIME_OUTPUT: actual device LUID unavailable")?;
            if outputs.iter().any(|o| o.adapter_luid != actual) {
                return Err(
                    "REALTIME_OUTPUT: shared buffers and Vulkan device have different LUIDs".into(),
                );
            }
        }
        Ok(native)
    })
}

/// Thread-affine persistent renderer. Construction validates scene data; the
/// first render creates the device graph for that extent. GPU errors are never
/// replaced by a CPU image or a proxy scene.
pub struct RendererSession {
    scene: SceneSnapshot,
    config: RendererConfig,
    native: Option<SessionCell>,
    extent: [u32; 2],
    internal: [u32; 2],
    external: Option<[ExternalOutput; 2]>,
    frame: u64,
    generation: u64,
    reset: bool,
    transforms_dirty: bool,
    lighting_reactive: f32,
}
impl RendererSession {
    pub fn new(scene: SceneSnapshot, config: RendererConfig) -> Result<Self, String> {
        scene::validate(&scene, &config)?;
        if !scene
            .instances
            .iter()
            .any(|i| scene.materials[scene.meshes[i.mesh].material].alpha_mode != AlphaMode::Blend)
        {
            return Err("REALTIME_SCENE: at least one opaque or masked draw is required".into());
        }
        Ok(Self {
            scene,
            config,
            native: None,
            extent: [0; 2],
            internal: [0; 2],
            external: None,
            frame: 0,
            generation: 0,
            reset: true,
            transforms_dirty: true,
            lighting_reactive: 0.,
        })
    }
    pub fn replace_scene(&mut self, scene: SceneSnapshot) -> Result<(), String> {
        scene::validate(&scene, &self.config)?;
        self.native.take();
        self.scene = scene;
        self.reset = true;
        self.transforms_dirty = true;
        Ok(())
    }
    /// Caller must wait for both external consumers before changing/detaching
    /// handles. Handles remain caller-owned until detach/session destruction.
    pub fn attach_external_outputs(&mut self, outputs: [ExternalOutput; 2]) -> Result<(), String> {
        let a = outputs[0];
        let b = outputs[1];
        for o in outputs {
            if o.handle == 0
                || o.width == 0
                || o.height == 0
                || o.row_pitch
                    < o.width
                        .checked_mul(4)
                        .ok_or("REALTIME_OUTPUT: width overflow")?
                || o.row_pitch % 4 != 0
                || o.size < u64::from(o.row_pitch) * u64::from(o.height)
            {
                return Err("REALTIME_OUTPUT: invalid shared buffer layout".into());
            }
        }
        if a.handle == b.handle
            || a.width != b.width
            || a.height != b.height
            || a.row_pitch != b.row_pitch
            || a.adapter_luid != b.adapter_luid
        {
            return Err(
                "REALTIME_OUTPUT: slots must be distinct and share extent, stride and adapter"
                    .into(),
            );
        }
        if self.external != Some(outputs) {
            self.native.take();
            self.external = Some(outputs);
            self.generation += 1;
            self.reset = true;
        }
        Ok(())
    }
    pub fn detach_external_outputs(&mut self) {
        self.native.take();
        self.external = None;
        self.generation += 1;
        self.reset = true;
    }
    pub fn apply_delta(&mut self, delta: SceneDelta) -> Result<(), String> {
        let mut next = self.scene.clone();
        for update in &delta.transforms {
            let inst = next
                .instances
                .iter_mut()
                .find(|i| i.id == update.instance_id)
                .ok_or("REALTIME_DELTA: unknown instance")?;
            inst.previous_transform = inst.transform;
            inst.transform = update.transform;
        }
        let (light_reset, reactive) = delta
            .lights
            .as_ref()
            .map(|lights| lighting_transition(&self.scene.lights, lights))
            .unwrap_or((false, 0.));
        let changed_lighting = light_reset
            || delta.environment.is_some()
            || delta.fog_volumes.is_some()
            || delta.decals.is_some();
        let material_changed = delta.materials.is_some();
        let rebuild = delta.materials.as_ref().is_some_and(|m| {
            m.len() != next.materials.len()
                || m.iter()
                    .zip(&next.materials)
                    .any(|(a, b)| a.textures != b.textures || a.alpha_mode != b.alpha_mode)
        });
        if let Some(l) = delta.lights {
            next.lights = l;
        }
        if let Some(m) = delta.materials {
            if m.len() != next.materials.len() {
                return Err(
                    "REALTIME_DELTA: material topology change requires replace_scene".into(),
                );
            }
            next.materials = m;
        }
        if let Some(e) = delta.environment {
            next.environment = e;
        }
        if let Some(f) = delta.fog_volumes {
            next.fog_volumes = f;
        }
        if let Some(d) = delta.decals {
            next.decals = d;
        }
        next.revision = delta.revision;
        scene::validate(&next, &self.config)?;
        self.scene = next;
        if rebuild {
            self.native.take();
        }
        self.transforms_dirty |= !delta.transforms.is_empty();
        self.reset |= changed_lighting;
        self.lighting_reactive = self
            .lighting_reactive
            .max(reactive)
            .max(if material_changed { 0.5 } else { 0. });
        Ok(())
    }
    pub fn render(&mut self, input: &FrameInput) -> Result<FrameOutput, String> {
        let [ow, oh] = input.output_extent;
        if ow == 0
            || oh == 0
            || ow > 8192
            || oh > 8192
            || !input.render_scale.is_finite()
            || !(0.5..=1.).contains(&input.render_scale)
            || !input
                .view_projection
                .iter()
                .chain(&input.previous_view_projection)
                .chain(&input.eye)
                .all(|f| f.is_finite())
        {
            return Err("REALTIME_FRAME: invalid extent, scale or camera".into());
        }
        if !input.readback && input.output_slot.is_none() && !input.allow_offscreen {
            return Err("REALTIME_OUTPUT: readback=false requires an attached output slot or explicit offscreen intent".into());
        }
        if self.external.is_some() && input.output_slot.is_none() {
            return Err("REALTIME_OUTPUT: attached shared buffers require an explicit slot; wait for consumers and detach before headless rendering".into());
        }
        if let Some(slot) = input.output_slot {
            if slot >= 2 || self.external.is_none() {
                return Err("REALTIME_OUTPUT: invalid or unattached output slot".into());
            }
        }
        if let Some(ext) = self.external {
            if [ext[0].width, ext[0].height] != input.output_extent {
                return Err("REALTIME_OUTPUT: extent differs from attached buffers".into());
            }
        }
        let internal = if input.render_scale == 1. {
            input.output_extent
        } else {
            [
                (((ow as f32 * input.render_scale) as u32) / 2 * 2).max(2),
                (((oh as f32 * input.render_scale) as u32) / 2 * 2).max(2),
            ]
        };
        if self.native.is_none() || self.extent != input.output_extent || self.internal != internal
        {
            self.native.take();
            let packed = scene::pack(&self.scene, &self.config)?;
            let instances = scene::instances(&self.scene, &packed.ranges);
            let lights = scene::lights(&self.scene, self.config.max_lights);
            self.native = Some(build(Assets {
                scene: self.scene.clone(),
                packed,
                instances,
                lights,
                fog: scene::fog_volumes(&self.scene),
                decals: scene::decals(&self.scene),
                extent: input.output_extent,
                internal,
                external: self.external,
                depth_prepass: self.config.depth_prepass,
                programs: shaders::programs()?,
            })?);
            self.extent = input.output_extent;
            self.internal = internal;
            self.reset = true;
            self.transforms_dirty = false;
            self.frame = 0;
        }
        let reset = self.reset || input.reset_history;
        let current = if self.frame % 2 == 0 {
            HISTORY1
        } else {
            HISTORY0
        };
        let previous = if current == HISTORY1 {
            HISTORY0
        } else {
            HISTORY1
        };
        let curpos = if current == HISTORY1 { HPOS1 } else { HPOS0 };
        let prevpos = if current == HISTORY1 { HPOS0 } else { HPOS1 };
        let native = self.native.as_mut().unwrap();
        let assets = native.borrow_owner().borrow_owner().borrow_owner();
        let desc = native.borrow_owner().borrow_owner().borrow_dependent();
        let frame_data = frame_bytes(
            &self.scene,
            &self.config,
            input,
            internal,
            self.frame,
            reset,
            self.lighting_reactive,
        );
        let mut update = rex::FrameUpdate {
            buffer_uploads: vec![
                (rex::StableResourceId(u64::from(FRAME) + 1), 0, frame_data),
                (
                    rex::StableResourceId(u64::from(INSTANCES) + 1),
                    0,
                    scene::instances(&self.scene, &assets.packed.ranges),
                ),
                (
                    rex::StableResourceId(u64::from(LIGHTS) + 1),
                    0,
                    scene::lights(&self.scene, self.config.max_lights),
                ),
                (
                    rex::StableResourceId(u64::from(MATERIALS) + 1),
                    0,
                    scene::pack_materials(&self.scene, &scene::texture_map(&self.scene)),
                ),
                (
                    rex::StableResourceId(u64::from(FOG_VOLUMES) + 1),
                    0,
                    scene::fog_volumes(&self.scene),
                ),
                (
                    rex::StableResourceId(u64::from(DECALS) + 1),
                    0,
                    scene::decals(&self.scene),
                ),
            ],
            readback_subset: if input.readback {
                Some((0..desc.readbacks.len() as u32).collect())
            } else {
                None
            },
            ..Default::default()
        };
        if self.transforms_dirty {
            update.tlas_update = Some((
                0,
                self.scene
                    .instances
                    .iter()
                    .enumerate()
                    .map(|(i, v)| vk::RayQueryTransformedInstanceDesc {
                        blas: v.mesh as u32,
                        custom_index: i as u32,
                        mask: 255,
                        sbt_record_offset: 0,
                        transform: scene::as_transform(&v.transform),
                    })
                    .collect(),
                vk::TlasBuildAction::Refit,
            ));
        }
        let signal_current = if self.frame % 2 == 0 {
            SIGNAL_HISTORY1
        } else {
            SIGNAL_HISTORY0
        };
        let signal_previous = if signal_current == SIGNAL_HISTORY1 {
            SIGNAL_HISTORY0
        } else {
            SIGNAL_HISTORY1
        };
        update.binding_overrides.push((
            desc.signal_temporal_pass,
            signal_temporal_bind(signal_previous, signal_current),
        ));
        update.binding_overrides.push((
            desc.signal_spatial_passes[0],
            signal_spatial_bind(
                signal_current,
                FILTER_GI0,
                FILTER_REFL0,
                FILTER_GI1,
                FILTER_REFL1,
                1,
            ),
        ));
        update.binding_overrides.push((
            desc.signal_spatial_passes[1],
            signal_spatial_bind(
                signal_current,
                FILTER_GI1,
                FILTER_REFL1,
                FILTER_GI0,
                FILTER_REFL0,
                2,
            ),
        ));
        update.binding_overrides.push((
            desc.signal_spatial_passes[2],
            signal_spatial_bind(
                signal_current,
                FILTER_GI0,
                FILTER_REFL0,
                FILTER_GI1,
                FILTER_REFL1,
                4,
            ),
        ));
        update.binding_overrides.push((
            desc.temporal_pass,
            images_bind(vec![HDR, GPOS, MOTION, previous, prevpos, current, curpos]),
        ));
        let mut bloom = images_bind(vec![current, BLOOM0]);
        bloom.push_constants = 0u32.to_le_bytes().to_vec();
        update.binding_overrides.push((desc.bloom_pass, bloom));
        update
            .binding_overrides
            .push((desc.post_pass, images_bind(vec![current, BLOOM0, FINAL])));
        if let Some(pi) = desc.pack_pass {
            let slot = input.output_slot.unwrap_or(0);
            let output = self.external.unwrap()[slot];
            update.binding_overrides.push((
                pi,
                rex::Bindings {
                    storage_buffers: vec![EXTERNAL0 + slot as u32],
                    storage_images: vec![FINAL],
                    push_constants: (output.row_pitch / 4).to_le_bytes().to_vec(),
                    ..Default::default()
                },
            ));
        }
        let triangles = assets.packed.triangle_count;
        let mark_external = self.external.is_some() && self.frame > 0;
        let (result, device) = native.with_dependent_mut(|_, gpu| {
            if mark_external {
                gpu.mark_external_consumer_complete()?;
            }
            let proof = gpu.next_provenance_with_update(&update)?;
            let result = gpu.execute_with_frame_update(&proof, &update)?;
            Ok::<_, String>((result, gpu.device_caps().device_name.clone()))
        })?;
        #[cfg(test)]
        if let Ok(path) = std::env::var("RURIX_REALTIME_PROBE_DIR") {
            let path = std::path::PathBuf::from(path);
            std::fs::create_dir_all(&path).map_err(|e| e.to_string())?;
            for (i, data) in result.readbacks.iter().enumerate() {
                let name = [
                    "final",
                    "position",
                    "normal",
                    "base",
                    "emission",
                    "direct",
                    "gi",
                    "reflection",
                    "hdr",
                    "lights_buffer",
                    "frame_buffer",
                    "filtered_gi",
                    "filtered_reflection",
                    "fog",
                    "signal_history0",
                    "signal_history1",
                    "motion",
                    "history_position0",
                    "history_position1",
                    "tile_masks",
                ][i];
                std::fs::write(
                    path.join(format!(
                        "scene{}_frame{}_{}.raw",
                        self.scene.revision, self.frame, name
                    )),
                    data,
                )
                .map_err(|e| e.to_string())?;
            }
        }
        if result.telemetry.validation_error_count != 0 {
            return Err(format!(
                "REALTIME_VALIDATION: {} errors",
                result.telemetry.validation_error_count
            ));
        }
        self.frame += 1;
        self.reset = false;
        self.transforms_dirty = false;
        self.lighting_reactive = 0.;
        for inst in &mut self.scene.instances {
            inst.previous_transform = inst.transform;
        }
        let telemetry = &result.telemetry;
        let passes = telemetry
            .passes
            .iter()
            .map(|p| (p.name.clone(), p.gpu_ns / 1e6))
            .collect::<Vec<_>>();
        let gpu_ms = passes.iter().map(|(_, t)| t).sum();
        let rgba8 = if input.readback {
            result
                .readbacks
                .first()
                .cloned()
                .ok_or("REALTIME_READBACK: requested image absent")?
        } else {
            Vec::new()
        };
        if input.readback && rgba8.len() != ow as usize * oh as usize * 4 {
            return Err("REALTIME_READBACK: unexpected byte count".into());
        }
        Ok(FrameOutput {
            #[cfg(test)]
            debug_readbacks: result.readbacks.clone(),
            rgba8,
            width: ow,
            height: oh,
            internal_extent: internal,
            device_name: device,
            gpu_ms,
            passes,
            triangle_count: triangles,
            history_reset: reset,
            external_output: input.output_slot.map(|slot| ExternalFrame {
                slot,
                generation: self.generation,
            }),
            validation_errors: telemetry.validation_error_count,
            allocated_bytes: telemetry.allocations.iter().map(|a| a.bytes).sum(),
            heaps: telemetry.heaps.clone(),
            cpu_record_ms: telemetry.cpu_record_ns as f64 / 1e6,
            cpu_submit_ms: telemetry.cpu_submit_ns as f64 / 1e6,
            cpu_wait_ms: telemetry.cpu_fence_wait_ns as f64 / 1e6,
        })
    }
}
fn halton(mut i: u64, base: u64) -> f32 {
    let (mut f, mut r) = (1f32, 0f32);
    while i > 0 {
        f /= base as f32;
        r += f * (i % base) as f32;
        i /= base;
    }
    r
}
fn frame_bytes(
    scene: &SceneSnapshot,
    cfg: &RendererConfig,
    input: &FrameInput,
    internal: [u32; 2],
    index: u64,
    reset: bool,
    lighting_reactive: f32,
) -> Vec<u8> {
    let mut v = Vec::with_capacity(FRAME_BYTES as usize);
    scene::floats(&mut v, &input.view_projection);
    scene::floats(&mut v, &input.previous_view_projection);
    let e = &scene.environment;
    for x in [
        [
            input.eye[0],
            input.eye[1],
            input.eye[2],
            index as f32 * input.delta_seconds,
        ],
        [
            internal[0] as f32,
            internal[1] as f32,
            input.output_extent[0] as f32,
            input.output_extent[1] as f32,
        ],
        [
            e.ambient_color[0] * e.ambient_energy,
            e.ambient_color[1] * e.ambient_energy,
            e.ambient_color[2] * e.ambient_energy,
            if cfg.tile_lights { 1. } else { 0. },
        ],
        [
            e.fog_color[0],
            e.fog_color[1],
            e.fog_color[2],
            e.fog_density,
        ],
        [
            e.exposure,
            e.bloom_threshold,
            e.bloom_strength,
            index as f32,
        ],
        [
            halton(index % 16 + 1, 2) - 0.5,
            halton(index % 16 + 1, 3) - 0.5,
            if reset { 1. } else { 0. },
            input.delta_seconds,
        ],
        [
            scene.lights.len() as f32,
            if cfg.gi_enabled { 1. } else { 0. },
            if cfg.reflections_enabled { 1. } else { 0. },
            if cfg.shadows_enabled { 1. } else { 0. },
        ],
        [
            if e.tonemap == Tonemap::Aces { 1. } else { 0. },
            scene.fog_volumes.len() as f32,
            scene.decals.len() as f32,
            lighting_reactive,
        ],
    ] {
        scene::floats(&mut v, &x);
    }
    v
}

fn lighting_transition(old: &[Light], new: &[Light]) -> (bool, f32) {
    if old.len() != new.len() {
        return (true, 1.);
    }
    let mut reactive = 0f32;
    for (a, b) in old.iter().zip(new) {
        if a.id != b.id || a.kind != b.kind || a.cast_shadow != b.cast_shadow {
            return (true, 1.);
        }
        let energy_a = a.color.iter().copied().fold(0., f32::max) * a.intensity;
        let energy_b = b.color.iter().copied().fold(0., f32::max) * b.intensity;
        let change = (energy_a - energy_b).abs() / energy_a.max(energy_b).max(1e-4);
        if change > 0.25 {
            return (true, 1.);
        }
        if a.position != b.position
            || a.direction != b.direction
            || change > 1e-5
            || a.color != b.color
        {
            reactive = reactive.max(0.35);
        }
    }
    (false, reactive)
}
#[cfg(test)]
mod history_tests {
    use super::*;
    #[test]
    fn smooth_light_motion_keeps_history_but_switch_off_resets() {
        let a = Light::default();
        let mut b = a.clone();
        b.position[0] += 0.01;
        assert_eq!(
            lighting_transition(&[a.clone()], &[b.clone()]),
            (false, 0.35)
        );
        b.intensity = 0.;
        assert!(lighting_transition(&[a], &[b]).0);
    }
}
