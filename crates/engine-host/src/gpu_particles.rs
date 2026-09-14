//! Opt-in experiment: Rurix render_exec GPU particle trajectories + instanced raster.
//! This is a small Forge adapter, not the complete G35 particle/TSR/OIT system.
//! CPU uploads one event record per emitter. Individual particle positions and
//! colors are written only by the compute shader and consumed by vertex pulling.
//! No particle Sprite entities, CPU particle integration, or per-frame GPU readback.

use forge_scene::Scene;
use rurix_rt::render_exec as rex;
use std::sync::OnceLock;

pub(crate) const MAX_EMITTERS: usize = 64;
pub(crate) const PARTICLES_PER_EMITTER: usize = 64;
const PARTICLES: usize = MAX_EMITTERS * PARTICLES_PER_EMITTER;
const RECORD_BYTES: usize = 32;

pub(crate) fn enabled() -> bool {
    std::env::var("FORGE_GPU_PARTICLES").is_ok_and(|v| v == "on" || v == "1")
}

/// Fixed event array: center.xyz, age, lifetime, kind, stable seed, active.
/// Translation x/y <= -50 denotes an inactive pooled event. Scale is an event
/// payload, not the geometric scale of a mesh: [age_seconds, lifetime_seconds, kind].
pub(crate) fn emitter_bytes(scene: &Scene) -> (Vec<u8>, usize) {
    let mut bytes = vec![0u8; MAX_EMITTERS * RECORD_BYTES];
    let mut active = 0;
    for (slot, entity) in scene
        .entities
        .iter()
        .filter(|e| e.component("ParticleEmitter").is_some_and(|c| c.enabled))
        .take(MAX_EMITTERS)
        .enumerate()
    {
        let t = entity.transform;
        let valid = t
            .translation
            .iter()
            .chain(t.scale.iter())
            .all(|v| v.is_finite())
            && t.translation[0] > -50.
            && t.translation[1] > -50.
            && t.scale[0] >= 0.
            && t.scale[1] > 0.
            && t.scale[0] < t.scale[1]
            && (1. ..=4.).contains(&t.scale[2]);
        if !valid {
            continue;
        }
        active += 1;
        let record = [
            t.translation[0],
            t.translation[1],
            t.translation[2],
            t.scale[0],
            t.scale[1],
            t.scale[2].round(),
            (entity.id % 1_000_000) as f32,
            1.,
        ];
        for (i, value) in record.iter().enumerate() {
            bytes[slot * RECORD_BYTES + i * 4..slot * RECORD_BYTES + i * 4 + 4]
                .copy_from_slice(&value.to_le_bytes());
        }
    }
    (bytes, active)
}

pub(crate) struct ParticlePasses {
    pub emitter_resource: u32,
    pub particle_resource: u32,
}

const COMPUTE: &str = r#"
struct Emitter { center_age: vec4<f32>, life_kind_seed_active: vec4<f32>, };
struct Particle { position_size: vec4<f32>, color: vec4<f32>, };
@group(0) @binding(0) var<storage, read> emitters: array<Emitter>;
@group(0) @binding(1) var<storage, read_write> particles: array<Particle>;
fn hash(v: u32) -> u32 {
    var h = v * 747796405u + 2891336453u;
    h = ((h >> ((h >> 28u) + 4u)) ^ h) * 277803737u;
    return (h >> 22u) ^ h;
}
fn random(v: u32) -> f32 { return f32(hash(v) & 16777215u) / 16777216.0; }
@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let i = gid.x;
    if (i >= 4096u) { return; }
    let e = emitters[i / 64u];
    let life = e.life_kind_seed_active.x;
    let age = e.center_age.w;
    if (e.life_kind_seed_active.w < 0.5 || life <= 0.0 || age >= life) {
        particles[i].position_size = vec4<f32>(-1000.0, -1000.0, 0.0, 0.0);
        particles[i].color = vec4<f32>(0.0);
        return;
    }
    let seed = i + u32(e.life_kind_seed_active.z) * 131u;
    let r0 = random(seed);
    let r1 = random(seed + 77u);
    let r2 = random(seed + 911u);
    let kind = u32(e.life_kind_seed_active.y);
    let progress = clamp(age / life, 0.0, 1.0);
    let angle = f32(i % 64u) * 0.0981747704 + r0 * 0.55;
    var direction = vec2<f32>(cos(angle), sin(angle));
    var offset = direction * age * (0.9 + r1 * 2.8);
    var tint = vec3<f32>(0.3, 0.88, 1.0);
    if (kind == 1u) {
        offset = vec2<f32>(age * (1.0 + 5.0 * r1), (r2 - 0.5) * age * 1.4);
    } else if (kind == 2u) {
        offset.y += age * 0.9 - age * age * 0.55;
        tint = vec3<f32>(0.68, 1.0, 0.32);
    } else if (kind == 3u) {
        offset.x *= 1.65;
        offset.y += sin(age * 5.0 + r2 * 6.2831853) * age * 0.22;
        tint = vec3<f32>(0.25, 0.65, 1.0);
    } else {
        direction = vec2<f32>(cos(angle + age * 1.8), sin(angle + age * 1.8));
        offset = direction * age * (0.8 + r1 * 2.0);
        tint = vec3<f32>(0.83, 0.57, 1.0);
    }
    let size = (0.045 + r2 * 0.055) * (1.0 - progress * progress);
    particles[i].position_size = vec4<f32>(e.center_age.xyz + vec3<f32>(offset, 0.0), size);
    particles[i].color = vec4<f32>(tint, 1.0 - progress);
}
"#;

const VERTEX: &str = r#"
struct Particle { position_size: vec4<f32>, color: vec4<f32>, };
struct Camera { vp: mat4x4<f32>, };
@group(0) @binding(0) var<storage, read> particles: array<Particle>;
@group(0) @binding(1) var<uniform> camera: Camera;
struct Out {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) color: vec4<f32>,
};
@vertex
fn main(@builtin(vertex_index) vi: u32, @builtin(instance_index) ii: u32) -> Out {
    var corners = array<vec2<f32>, 6>(
        vec2<f32>(-1.0,-1.0), vec2<f32>(1.0,-1.0), vec2<f32>(1.0,1.0),
        vec2<f32>(-1.0,-1.0), vec2<f32>(1.0,1.0), vec2<f32>(-1.0,1.0));
    let p = particles[ii];
    let corner = corners[vi];
    var o: Out;
    o.position = camera.vp * vec4<f32>(p.position_size.xyz + vec3<f32>(corner * p.position_size.w, 0.0), 1.0);
    o.uv = corner;
    o.color = p.color;
    return o;
}
"#;

const FRAGMENT: &str = r#"
@fragment
fn main(@location(0) uv: vec2<f32>, @location(1) color: vec4<f32>) -> @location(0) vec4<f32> {
    // Straight alpha is composed by the actual additive Vulkan pipeline.
    let radius = dot(uv, uv);
    if (radius > 1.0 || color.a < 0.01) { discard; }
    let core = pow(max(0.0, 1.0 - radius), 2.0);
    return vec4<f32>(mix(color.rgb * 0.72, vec3<f32>(1.0), core * 0.8), core * color.a);
}
"#;

fn shaders() -> Result<(&'static [u8], &'static [u8], &'static [u8]), String> {
    static SHADERS: OnceLock<Result<(&'static [u8], &'static [u8], &'static [u8]), String>> =
        OnceLock::new();
    SHADERS
        .get_or_init(|| {
            Ok((
                crate::viewport::compile_wgsl(COMPUTE, "gpu-particle-compute")?,
                crate::viewport::compile_wgsl(VERTEX, "gpu-particle-vertex")?,
                crate::viewport::compile_wgsl(FRAGMENT, "gpu-particle-fragment")?,
            ))
        })
        .clone()
}

/// Append after ordinary scene raster passes and before a shared-texture pack.
pub(crate) fn append(
    resources: &mut Vec<rex::ResourceDesc<'static>>,
    passes: &mut Vec<rex::Pass<'static>>,
    barriers: &mut Vec<Vec<(u32, rex::TargetState)>>,
    camera_resource: u32,
    color_resource: u32,
    clear: Option<[f32; 4]>,
) -> Result<ParticlePasses, String> {
    let (compute, vs, fs) = shaders()?;
    let emitter_resource = resources.len() as u32;
    resources.push(rex::ResourceDesc::Buffer(rex::BufferDesc {
        size: (MAX_EMITTERS * RECORD_BYTES) as u64,
        usage: rex::BufferUsage {
            storage: true,
            ..Default::default()
        },
        data: None,
        device_local: false,
    }));
    let particle_resource = resources.len() as u32;
    resources.push(rex::ResourceDesc::Buffer(rex::BufferDesc {
        size: (PARTICLES * RECORD_BYTES) as u64,
        usage: rex::BufferUsage {
            storage: true,
            ..Default::default()
        },
        data: None,
        device_local: false,
    }));
    passes.push(rex::Pass::Compute(rex::ComputePass {
        name: "forge_gpu_particles_compute",
        spirv: compute,
        entry: None,
        dispatch: rex::DispatchSpec::Direct([(PARTICLES / 64) as u32, 1, 1]),
        bindings: rex::Bindings {
            storage_buffers: vec![emitter_resource, particle_resource],
            ..Default::default()
        },
    }));
    barriers.push(vec![
        (emitter_resource, rex::TargetState::StorageReadWrite),
        (particle_resource, rex::TargetState::StorageReadWrite),
    ]);
    passes.push(rex::Pass::Raster(rex::RasterPass {
        blend: rex::BlendMode::Additive,
        name: "forge_gpu_particles_draw",
        vs_spirv: vs,
        fs_spirv: fs,
        vertex: rex::VertexData::Pull,
        draw: rex::DrawSpec::Direct {
            vertex_count: 6,
            instance_count: PARTICLES as u32,
            first_vertex: 0,
            first_instance: 0,
        },
        colors: vec![rex::ColorAttachmentRef {
            res: color_resource,
            clear,
        }],
        depth: None,
        viewport: None,
        bindings: rex::Bindings {
            storage_buffers: vec![particle_resource],
            uniform: Some(rex::UniformRef {
                res: camera_resource,
                offset: 0,
                size: 64,
            }),
            ..Default::default()
        },
        conservative: None,
    }));
    barriers.push(vec![
        (particle_resource, rex::TargetState::StorageReadWrite),
        (camera_resource, rex::TargetState::UniformRead),
        (color_resource, rex::TargetState::ColorAttachmentWrite),
    ]);
    Ok(ParticlePasses {
        emitter_resource,
        particle_resource,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use forge_scene::{Component, Entity, Transform};
    fn scene(age: f32) -> Scene {
        Scene {
            name: "GPU particle evidence".into(),
            next_id: 2,
            mode: "2d".into(),
            gravity: [0.; 3],
            entities: vec![Entity {
                id: 1,
                name: "CS_VFX0".into(),
                transform: Transform {
                    translation: [0., 0., 0.],
                    scale: [age, 2., 3.],
                    ..Transform::default()
                },
                components: vec![Component::new("ParticleEmitter", serde_json::json!({}))],
            }],
        }
    }
    #[test]
    fn inactive_and_invalid_events_do_not_emit() {
        assert_eq!(emitter_bytes(&scene(0.5)).1, 1);
        assert_eq!(emitter_bytes(&scene(2.)).1, 0);
        assert_eq!(emitter_bytes(&scene(f32::NAN)).1, 0);
        let mut hidden = scene(0.);
        hidden.entities[0].transform.translation[0] = -100.;
        assert_eq!(emitter_bytes(&hidden).1, 0);
        assert_eq!(
            emitter_bytes(&hidden).0,
            vec![0; MAX_EMITTERS * RECORD_BYTES]
        );
    }
    #[test]
    fn gpu_particle_shaders_compile_to_spirv() {
        shaders().unwrap();
    }

    #[test]
    #[ignore = "requires FORGE_GPU_PARTICLES=on and a real Vulkan device"]
    fn real_viewport_emitter() {
        assert!(
            enabled(),
            "set FORGE_GPU_PARTICLES=on for this integration test"
        );
        let camera = crate::viewport::EditorCamera {
            target: [0., 0., 0.],
            yaw_deg: 0.,
            pitch_deg: 0.,
            ortho: true,
            ortho_half_h: 5.,
            ..Default::default()
        };
        let active = crate::viewport::render_scene_frame(
            &scene(0.75),
            &camera,
            None,
            512,
            320,
            true,
            true,
            None,
        )
        .expect("actual viewport must execute the GPU emitter passes");
        let inactive = crate::viewport::render_scene_frame(
            &scene(2.1),
            &camera,
            None,
            512,
            320,
            true,
            true,
            None,
        )
        .expect("actual viewport must hide expired GPU emitters");
        assert_ne!(
            active.rgba8, inactive.rgba8,
            "ParticleEmitter component must affect the actual viewport"
        );
        if let Some(dir) = std::env::var_os("FORGE_GPU_PARTICLE_EVIDENCE_DIR") {
            let dir = std::path::PathBuf::from(dir);
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(dir.join("viewport-emitter-active.rgba"), &active.rgba8).unwrap();
            std::fs::write(dir.join("viewport-emitter-expired.rgba"), &inactive.rgba8).unwrap();
            let record = serde_json::json!({"integration":"ParticleEmitter -> viewport DeviceFrameSession",
                "enabledBy":"FORGE_GPU_PARTICLES=on","device":active.device_name,"width":512,"height":320,
                "activeVsExpiredPixelsDiffer":true,"defaultEnabled":false});
            std::fs::write(
                dir.join("viewport-emitter-evidence.json"),
                serde_json::to_vec_pretty(&record).unwrap(),
            )
            .unwrap();
        }
    }

    #[test]
    #[ignore = "requires FORGE_GPU_PARTICLES unset/off and a real Vulkan device"]
    fn real_viewport_emitter_off() {
        assert!(!enabled(), "leave FORGE_GPU_PARTICLES unset or off");
        let camera = crate::viewport::EditorCamera {
            target: [0., 0., 0.],
            yaw_deg: 0.,
            pitch_deg: 0.,
            ortho: true,
            ortho_half_h: 5.,
            ..Default::default()
        };
        let active = crate::viewport::render_scene_frame(
            &scene(0.75),
            &camera,
            None,
            512,
            320,
            true,
            true,
            None,
        )
        .unwrap();
        let expired = crate::viewport::render_scene_frame(
            &scene(2.1),
            &camera,
            None,
            512,
            320,
            true,
            true,
            None,
        )
        .unwrap();
        assert_eq!(
            active.rgba8, expired.rgba8,
            "disabled experiment must not draw event state"
        );
        if let Some(dir) = std::env::var_os("FORGE_GPU_PARTICLE_EVIDENCE_DIR") {
            let dir = std::path::PathBuf::from(dir);
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(dir.join("viewport-experiment-off.rgba"), &active.rgba8).unwrap();
            let record = serde_json::json!({"integration":"ParticleEmitter ignored while experiment is off",
                "enabled":false,"device":active.device_name,"width":512,"height":320,
                "activeAndExpiredFramesEqual":true});
            std::fs::write(
                dir.join("viewport-emitter-off-evidence.json"),
                serde_json::to_vec_pretty(&record).unwrap(),
            )
            .unwrap();
        }
    }

    /// Run explicitly on real Vulkan hardware; no CPU fallback or skipped pass.
    #[test]
    #[ignore = "requires a real Vulkan device; run --ignored --exact gpu_particles::tests::real_gpu_compute_and_draw"]
    fn real_gpu_compute_and_draw() {
        const WIDTH: u32 = 512;
        const HEIGHT: u32 = 320;
        let caps = rex::probe_device_caps().expect("real Vulkan device required");
        let mut resources = vec![
            rex::ResourceDesc::Texture(rex::TextureDesc {
                width: WIDTH,
                height: HEIGHT,
                format: rex::TexFormat::Rgba8Unorm,
                usage: rex::TextureUsage {
                    color: true,
                    ..Default::default()
                },
                data: None,
            }),
            rex::ResourceDesc::Buffer(rex::BufferDesc {
                size: 64,
                usage: rex::BufferUsage {
                    uniform: true,
                    ..Default::default()
                },
                data: None,
                device_local: false,
            }),
        ];
        let mut passes = vec![];
        let mut barriers = vec![];
        let ids = append(
            &mut resources,
            &mut passes,
            &mut barriers,
            1,
            0,
            Some([0., 0., 0., 1.]),
        )
        .unwrap();
        let barrier_refs: Vec<&[(u32, rex::TargetState)]> =
            barriers.iter().map(Vec::as_slice).collect();
        let readbacks = [
            rex::Readback::Texture { res: 0 },
            rex::Readback::Buffer {
                res: ids.particle_resource,
                offset: 0,
                size: (PARTICLES * RECORD_BYTES) as u64,
            },
        ];
        let mut session =
            rex::DeviceFrameSession::new(&resources, &passes, &barrier_refs, &readbacks, 2)
                .expect("real GPU session creation");
        let matrix: [f32; 16] = [
            0.125, 0., 0., 0., 0., 0.2, 0., 0., 0., 0., 1., 0., 0., 0., 0., 1.,
        ];
        let camera: Vec<u8> = matrix.iter().flat_map(|v| v.to_le_bytes()).collect();
        let mut frames = Vec::new();
        let mut particle_buffers = Vec::new();
        for age in [0.25, 0.75, 2.1] {
            let (emitters, active) = emitter_bytes(&scene(age));
            let update = rex::FrameUpdate {
                buffer_uploads: vec![
                    (rex::StableResourceId(2), 0, camera.clone()),
                    (
                        rex::StableResourceId(ids.emitter_resource as u64 + 1),
                        0,
                        emitters,
                    ),
                ],
                readback_subset: Some(vec![0, 1]),
                ..Default::default()
            };
            let proof = session.next_provenance_with_update(&update).unwrap();
            let result = session
                .execute_with_frame_update(&proof, &update)
                .expect("real compute + raster execution");
            let count = result.readbacks[0]
                .chunks_exact(4)
                .filter(|p| p[0] > 0 || p[1] > 0 || p[2] > 0)
                .count();
            if active > 0 {
                assert!(count > 8, "GPU raster must produce actual particles");
            } else {
                assert_eq!(count, 0, "expired emitters must clear their GPU draw");
            }
            frames.push((age, count, result.readbacks[0].clone()));
            particle_buffers.push(result.readbacks[1].clone());
        }
        assert_ne!(
            particle_buffers[0], particle_buffers[1],
            "GPU particle SSBO must change with event age"
        );
        assert_ne!(
            frames[0].2, frames[1].2,
            "GPU draw must consume changed compute positions"
        );
        let first = f32::from_le_bytes(particle_buffers[0][0..4].try_into().unwrap());
        assert!(
            first.is_finite() && first.abs() > 0.01,
            "compute must write a nonzero particle position"
        );
        let evidence = serde_json::json!({"experiment":"forge-rurix-gpu-particles","realGpu":true,
            "device":caps.device_name,"computePass":"forge_gpu_particles_compute",
            "drawPass":"forge_gpu_particles_draw","dispatchWorkgroups":64,"drawInstances":4096,
            "vertexCountPerInstance":6,"cpuParticlePositionsUploaded":false,
            "particleReadback":"test only; runtime viewport uploads only emitter records",
            "ssboChangesWithAge":true,"imageChangesWithAge":true,"expiredEmitterPixels":frames[2].1,
            "frames":frames.iter().map(|(age,pixels,_)|serde_json::json!({"age":age,"nonblackPixels":pixels,
                "width":WIDTH,"height":HEIGHT})).collect::<Vec<_>>(),
            "blend":"straight-alpha additive Vulkan pipeline",
            "limits":["analytic GPU trajectories, not full G35 simulation","2D XY only"]});
        if let Some(dir) = std::env::var_os("FORGE_GPU_PARTICLE_EVIDENCE_DIR") {
            let dir = std::path::PathBuf::from(dir);
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(
                dir.join("gpu-particles-evidence.json"),
                serde_json::to_vec_pretty(&evidence).unwrap(),
            )
            .unwrap();
            for (i, (_, _, pixels)) in frames.iter().enumerate() {
                std::fs::write(dir.join(format!("gpu-particles-{i}.rgba")), pixels).unwrap();
            }
        }
        println!("{evidence}");
    }
}

