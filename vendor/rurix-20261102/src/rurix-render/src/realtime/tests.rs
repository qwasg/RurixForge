use super::*;
use std::sync::Arc;

fn fixture() -> SceneSnapshot {
    let mut materials = vec![
        Material {
            base_color: [0.35, 0.45, 0.55, 1.],
            roughness: 0.7,
            double_sided: true,
            ..Default::default()
        },
        Material {
            base_color: [0.55, 0.3, 0.1, 1.],
            metallic: 0.8,
            roughness: 0.22,
            double_sided: true,
            ..Default::default()
        },
    ];
    materials.push(Material {
        base_color: [0.05, 0.1, 0.2, 1.],
        emissive: [0., 1., 4.],
        double_sided: true,
        ..Default::default()
    });
    let quad = |id, ps: [[f32; 3]; 4], n: [f32; 3], material| Mesh {
        id,
        vertices: Arc::new(
            ps.into_iter()
                .zip([[0., 0.], [1., 0.], [1., 1.], [0., 1.]])
                .map(|(position, uv)| Vertex {
                    position,
                    normal: n,
                    uv,
                    tangent: [1., 0., 0., 1.],
                })
                .collect(),
        ),
        indices: Arc::new(vec![0, 1, 2, 0, 2, 3]),
        material,
    };
    let meshes = vec![
        quad(
            1,
            [
                [-3., -1., 2.],
                [3., -1., 2.],
                [3., -1., -3.],
                [-3., -1., -3.],
            ],
            [0., 1., 0.],
            0,
        ),
        quad(
            2,
            [
                [-3., -1., -3.],
                [3., -1., -3.],
                [3., 3., -3.],
                [-3., 3., -3.],
            ],
            [0., 0., 1.],
            0,
        ),
        quad(
            3,
            [
                [-0.45, -1., 0.],
                [0.45, -1., 0.],
                [0.45, 0.6, 0.],
                [-0.45, 0.6, 0.],
            ],
            [0., 0., 1.],
            1,
        ),
        quad(
            4,
            [
                [0.8, 0.3, -2.9],
                [1.4, 0.3, -2.9],
                [1.4, 1.3, -2.9],
                [0.8, 1.3, -2.9],
            ],
            [0., 0., 1.],
            2,
        ),
    ];
    let instances = (0..meshes.len())
        .map(|mesh| Instance {
            id: mesh as u64 + 10,
            mesh,
            transform: IDENTITY,
            previous_transform: IDENTITY,
        })
        .collect();
    SceneSnapshot {
        revision: 1,
        meshes,
        materials,
        instances,
        lights: vec![Light {
            position: [-1., 1.5, 1.],
            intensity: 80.,
            range: 10.,
            radius: 0.03,
            ..Default::default()
        }],
        environment: Environment {
            bloom_strength: 0.08,
            ambient_energy: 0.003,
            fog_density: 0.,
            ..Default::default()
        },
        ..Default::default()
    }
}
fn camera() -> Mat4 {
    // Camera [0,0.4,4], looking parallel -Z; right handed ZO perspective.
    let f = 1.7;
    let n = 0.1;
    let z = 30.;
    let mut m = [
        f / 1.5,
        0.,
        0.,
        0.,
        0.,
        f,
        0.,
        0.,
        0.,
        0.,
        z / (n - z),
        -1.,
        0.,
        -0.4 * f,
        -4. * z / (n - z) + n * z / (n - z),
        4.,
    ];
    m[15] = 4.;
    m
}
fn half_float(bits: u16) -> f32 {
    let sign = if bits & 0x8000 != 0 { -1. } else { 1. };
    let exponent = (bits >> 10) & 31;
    let mantissa = bits & 1023;
    if exponent == 0 {
        sign * (mantissa as f32) * 2f32.powi(-24)
    } else if exponent == 31 {
        if mantissa == 0 {
            sign * f32::INFINITY
        } else {
            f32::NAN
        }
    } else {
        sign * (1. + mantissa as f32 / 1024.) * 2f32.powi(i32::from(exponent) - 15)
    }
}
fn hdr_pixels(bytes: &[u8]) -> Vec<[f32; 4]> {
    bytes
        .chunks_exact(8)
        .map(|p| std::array::from_fn(|i| half_float(u16::from_le_bytes([p[i * 2], p[i * 2 + 1]]))))
        .collect()
}
//@ spec: RXS-0408
#[test]
fn scene_rejects_bad_indices_and_texture_lengths() {
    let mut s = fixture();
    s.meshes[0].indices = Arc::new(vec![0, 1, 99]);
    assert!(RendererSession::new(s, Default::default()).is_err());
    let mut s = fixture();
    s.textures.push(Texture {
        width: 2,
        height: 2,
        rgba8: Arc::new(vec![0; 4]),
        wrap_s: 10497,
        wrap_t: 10497,
    });
    assert!(RendererSession::new(s, Default::default()).is_err());
}
//@ spec: RXS-0410
#[test]
fn unsupported_output_and_delta_fail_explicitly() {
    let mut r = RendererSession::new(fixture(), Default::default()).unwrap();
    let frame = FrameInput {
        readback: false,
        ..Default::default()
    };
    assert!(r
        .render(&frame)
        .unwrap_err()
        .contains("requires an attached output"));
    assert!(r
        .apply_delta(SceneDelta {
            transforms: vec![TransformUpdate {
                instance_id: 999,
                transform: IDENTITY
            }],
            ..Default::default()
        })
        .is_err());
}
#[test]
fn material_gpu_layout_matches_declared_stride() {
    let s = fixture();
    let p = super::scene::pack(&s, &Default::default()).unwrap();
    assert_eq!(p.materials.len(), s.materials.len() * 96);
    assert_eq!(
        super::scene::instances(&s, &p.ranges).len(),
        s.instances.len() * 144
    );
}

/// Run explicitly inside ci.gpu_device_lock. No CPU reference image can pass.
//@ spec: RXS-0409
#[test]
#[ignore = "requires exclusive real Vulkan/RayQuery device"]
fn gpu_realtime_scene_and_dynamic_light() {
    let s = fixture();
    let mut config = RendererConfig::default();
    if std::env::var("RURIX_REALTIME_TEST_SHADOWS").as_deref() == Ok("off") {
        config.shadows_enabled = false;
    }
    let mut renderer = RendererSession::new(s.clone(), config).unwrap();
    let matrix = camera();
    let mut input = FrameInput {
        view_projection: matrix,
        previous_view_projection: matrix,
        eye: [0., 0.4, 4.],
        output_extent: [192, 128],
        reset_history: true,
        ..Default::default()
    };
    let first = renderer
        .render(&input)
        .expect("real hardware raster + ray query must execute");
    assert_eq!(first.validation_errors, 0);
    assert_eq!(first.rgba8.len(), 192 * 128 * 4);
    assert!(
        first
            .rgba8
            .chunks_exact(4)
            .filter(|p| p[0] > 25 || p[1] > 25 || p[2] > 25)
            .count()
            > 500,
        "scene must contain visible lit geometry"
    );
    assert_eq!(
        first
            .passes
            .iter()
            .filter(|(name, _)| name.starts_with("realtime_gbuffer"))
            .count(),
        1,
        "opaque instances must share one actual raster pass"
    );
    let direct = hdr_pixels(&first.debug_readbacks[5]);
    let positions = first.debug_readbacks[1]
        .chunks_exact(16)
        .map(|p| f32::from_le_bytes(p[12..16].try_into().unwrap()))
        .collect::<Vec<_>>();
    let lit_opaque = direct
        .iter()
        .zip(&positions)
        .filter(|(d, instance)| **instance > 0. && **instance != 4. && d[0] > 0.02)
        .count();
    assert!(
        lit_opaque > 1000,
        "unoccluded opaque surfaces must receive real direct light, got {lit_opaque}; emissive geometry cannot satisfy this check"
    );
    let mut open_scene = s.clone();
    open_scene.revision = 10;
    let mut no_shadow = RendererSession::new(
        open_scene,
        RendererConfig {
            shadows_enabled: false,
            ..Default::default()
        },
    )
    .unwrap();
    let unshadowed = no_shadow.render(&input).unwrap();
    if std::env::var("RURIX_REALTIME_TEST_SHADOWS").as_deref() != Ok("off") {
        let open_direct = hdr_pixels(&unshadowed.debug_readbacks[5]);
        let blocked = direct
            .iter()
            .zip(&open_direct)
            .filter(|(a, b)| b[0] > 0.03 && a[0] < b[0] * 0.8)
            .count();
        assert!(
            blocked > 100,
            "actual blocker must shadow a nonempty receiver region, got {blocked}"
        );
        println!("REALTIME_SHADOW_WITNESS lit_opaque={lit_opaque} blocked={blocked}");
    }
    let mut off_scene = s.clone();
    off_scene.revision = 20;
    off_scene.lights[0].intensity = 0.;
    let mut off_renderer = RendererSession::new(off_scene, Default::default()).unwrap();
    let lightoff = off_renderer.render(&input).unwrap();
    let sum = |p: &[u8]| {
        p.chunks_exact(4)
            .map(|x| u64::from(x[0]) + u64::from(x[1]) + u64::from(x[2]))
            .sum::<u64>()
    };
    assert!(
        sum(&first.rgba8) > sum(&lightoff.rgba8) * 2,
        "same-camera, same-jitter, same-seed independent light-off reference must be substantially darker"
    );
    input.reset_history = false;
    let second = renderer.render(&input).unwrap();
    assert!(!second.history_reset);
    assert!(second.gpu_ms > 0.);
    assert!(second
        .passes
        .iter()
        .any(|(n, _)| n == "realtime_indirect_reflection"));
    renderer
        .apply_delta(SceneDelta {
            revision: 2,
            lights: Some(vec![Light {
                intensity: 0.,
                ..s.lights[0].clone()
            }]),
            ..Default::default()
        })
        .unwrap();
    let dark = renderer.render(&input).unwrap();
    assert!(dark.history_reset);
    let sum = |p: &[u8]| {
        p.chunks_exact(4)
            .map(|x| u64::from(x[0]) + u64::from(x[1]) + u64::from(x[2]))
            .sum::<u64>()
    };
    assert!(
        sum(&first.rgba8) > sum(&dark.rgba8),
        "switching the actual scene light off must darken the rendered scene"
    );
    let mut transform = IDENTITY;
    transform[12] = 0.7;
    renderer
        .apply_delta(SceneDelta {
            revision: 3,
            transforms: vec![TransformUpdate {
                instance_id: 12,
                transform,
            }],
            ..Default::default()
        })
        .unwrap();
    let moved = renderer.render(&input).unwrap();
    assert_ne!(
        moved.rgba8, dark.rgba8,
        "TLAS+GBuffer instance update must alter real pixels"
    );
    println!(
        "REALTIME_GPU device={} internal={:?} passes={} triangles={} gpu_ms={} allocated={} validation_errors={}",
        first.device_name,
        first.internal_extent,
        first.passes.len(),
        first.triangle_count,
        second.gpu_ms,
        first.allocated_bytes,
        first.validation_errors
    );
    if let Ok(path) = std::env::var("RURIX_REALTIME_PROBE_DIR") {
        let path = std::path::PathBuf::from(path);
        std::fs::create_dir_all(&path).unwrap();
        for (name, data) in [
            ("lit.rgba", &first.rgba8),
            ("dark.rgba", &dark.rgba8),
            ("moved.rgba", &moved.rgba8),
            ("unshadowed.rgba", &unshadowed.rgba8),
            ("lightoff_reference.rgba", &lightoff.rgba8),
        ] {
            std::fs::write(path.join(name), data).unwrap();
        }
    }
}

//@ spec: RXS-0409
#[test]
#[ignore = "requires exclusive real Vulkan/RayQuery device"]
fn gpu_realtime_effects_and_separate_history() {
    let matrix = camera();
    let mut input = FrameInput {
        view_projection: matrix,
        previous_view_projection: matrix,
        eye: [0., 0.4, 4.],
        output_extent: [192, 128],
        ..Default::default()
    };
    let mut base = RendererSession::new(fixture(), Default::default()).unwrap();
    let first = base.render(&input).unwrap();
    let energy = |bytes: &[u8]| {
        hdr_pixels(bytes)
            .iter()
            .map(|p| f64::from(p[0]) + f64::from(p[1]) + f64::from(p[2]))
            .sum::<f64>()
    };
    assert!(
        energy(&first.debug_readbacks[6]) > 0.,
        "real diffuse bounce must contribute"
    );
    assert!(
        energy(&first.debug_readbacks[7]) > 0.,
        "real specular bounce must contribute"
    );
    let mut no_indirect = RendererSession::new(
        fixture(),
        RendererConfig {
            gi_enabled: false,
            reflections_enabled: false,
            ..Default::default()
        },
    )
    .unwrap();
    let off = no_indirect.render(&input).unwrap();
    assert_eq!(energy(&off.debug_readbacks[6]), 0.);
    assert_eq!(energy(&off.debug_readbacks[7]), 0.);
    input.reset_history = false;
    let mut last = first;
    for _ in 0..64 {
        last = base.render(&input).unwrap();
        assert_eq!(last.validation_errors, 0);
    }
    let history_ages = |frame: &FrameOutput| {
        let bytes = if frame.debug_readbacks[14]
            .chunks_exact(80)
            .map(|p| f32::from_le_bytes(p[12..16].try_into().unwrap()))
            .fold(0., f32::max)
            > frame.debug_readbacks[15]
                .chunks_exact(80)
                .map(|p| f32::from_le_bytes(p[12..16].try_into().unwrap()))
                .fold(0., f32::max)
        {
            &frame.debug_readbacks[14]
        } else {
            &frame.debug_readbacks[15]
        };
        bytes
            .chunks_exact(80)
            .map(|p| f32::from_le_bytes(p[12..16].try_into().unwrap()))
            .collect::<Vec<_>>()
    };
    assert!(
        history_ages(&last).iter().filter(|&&age| age > 32.).count() > 100,
        "indirect histories must persist independently of final HDR resolve"
    );
    let mut moving = fixture().lights;
    moving[0].position[0] += 0.01;
    base.apply_delta(SceneDelta {
        revision: 2,
        lights: Some(moving),
        ..Default::default()
    })
    .unwrap();
    let continuous = base.render(&input).unwrap();
    assert!(
        !continuous.history_reset,
        "smooth light motion must not clear every history frame"
    );
    input.reset_history = true;
    let reset = base.render(&input).unwrap();
    assert!(reset.history_reset);
    // Logical frame 66 writes history1. The other allocation may retain old
    // bytes, but the actual selected producer must restart every age at one.
    assert!(
        reset.debug_readbacks[15]
            .chunks_exact(80)
            .all(|p| f32::from_le_bytes(p[12..16].try_into().unwrap()) == 1.),
        "reset must prevent stale indirect samples from contributing"
    );
    let mut volume_scene = fixture();
    volume_scene.revision = 30;
    volume_scene.fog_volumes.push(FogVolume {
        id: 1,
        transform: IDENTITY,
        size: [6., 4., 6.],
        shape: FogShape::Box,
        density: 0.06,
        albedo: [0.6, 0.7, 0.8],
        emission: [0.05, 0.02, 0.],
        anisotropy: 0.2,
        edge_fade: 0.,
    });
    let mut fog = RendererSession::new(volume_scene, Default::default()).unwrap();
    let fog_frame = fog.render(&input).unwrap();
    let fog_pixels = hdr_pixels(&fog_frame.debug_readbacks[13]);
    assert!(
        fog_pixels
            .iter()
            .filter(|p| p[3] < 0.99 && p[0] > 0.)
            .count()
            > 100,
        "actual geometry-bounded local fog must attenuate and scatter light"
    );
    let mut decal_scene = fixture();
    decal_scene.revision = 40;
    let material = decal_scene.materials.len();
    decal_scene.materials.push(Material {
        base_color: [1., 0., 0., 1.],
        roughness: 0.8,
        ..Default::default()
    });
    let mut transform = IDENTITY;
    transform[13] = -0.98;
    decal_scene.decals.push(Decal {
        id: 1,
        transform,
        size: [3., 0.2, 3.],
        material,
        opacity: 1.,
    });
    let mut decal = RendererSession::new(decal_scene, Default::default()).unwrap();
    let decal_frame = decal.render(&input).unwrap();
    let red = hdr_pixels(&decal_frame.debug_readbacks[3])
        .iter()
        .filter(|p| p[0] > 0.95 && p[1] < 0.01 && p[2] < 0.01)
        .count();
    assert!(
        red > 100,
        "projected decal must update actual surface material, got {red}"
    );
    println!(
        "REALTIME_EFFECTS gi/reflection on-off, persistent separate histories, smooth-light continuity, local fog and projected decal PASS; decal_pixels={red}"
    );
}

//@ spec: RXS-0408
//@ spec: RXS-0409
#[test]
#[ignore = "requires exclusive real Vulkan/RayQuery device"]
fn gpu_prepass_texture_domains_and_glass_reactive() {
    let matrix = camera();
    let mut input = FrameInput {
        view_projection: matrix,
        previous_view_projection: matrix,
        eye: [0., 0.4, 4.],
        output_extent: [192, 128],
        ..Default::default()
    };
    let mut textured = fixture();
    textured.textures.push(Texture {
        width: 2,
        height: 2,
        rgba8: Arc::new(vec![128, 128, 128, 255].repeat(4)),
        wrap_s: 10497,
        wrap_t: 10497,
    });
    textured.materials[0].textures[0] = Some(0);
    textured.materials[0].textures[2] = Some(0);
    let mut without = RendererSession::new(textured.clone(), Default::default()).unwrap();
    let base = without.render(&input).unwrap();
    let mut with = RendererSession::new(
        textured,
        RendererConfig {
            depth_prepass: true,
            ..Default::default()
        },
    )
    .unwrap();
    let pre = with.render(&input).unwrap();
    assert!(pre
        .passes
        .iter()
        .any(|(name, _)| name == "realtime_depth_prepass"));
    for stage in [1, 2, 3, 4] {
        assert_eq!(
            base.debug_readbacks[stage], pre.debug_readbacks[stage],
            "depth prepass must preserve actual GBuffer geometry/material bytes, stage {stage}"
        );
    }
    let positions = base.debug_readbacks[1]
        .chunks_exact(16)
        .map(|p| f32::from_le_bytes(p[12..16].try_into().unwrap()))
        .collect::<Vec<_>>();
    let colors = hdr_pixels(&base.debug_readbacks[3]);
    let normals = hdr_pixels(&base.debug_readbacks[2]);
    let expected = (((128f64 / 255. + 0.055) / 1.055).powf(2.4) * 0.35) as f32;
    let expected_rough = 0.7 * 128. / 255.;
    let mut checked = 0;
    for (i, id) in positions.iter().enumerate() {
        if *id == 1. {
            assert!((colors[i][0] - expected).abs() < 0.0001);
            assert!(
                (normals[i][3] - expected_rough).abs() < 0.0003,
                "MR data must not be sRGB-decoded"
            );
            checked += 1;
        }
    }
    assert!(checked > 1000);
    let mut glass_scene = fixture();
    glass_scene.materials[1].alpha_mode = AlphaMode::Blend;
    glass_scene.materials[1].base_color = [0.15, 0.55, 0.8, 0.25];
    glass_scene.materials[1].metallic = 0.;
    glass_scene.environment.bloom_strength = 0.;
    let cfg = RendererConfig {
        gi_enabled: false,
        reflections_enabled: false,
        shadows_enabled: false,
        ..Default::default()
    };
    let mut glass = RendererSession::new(glass_scene.clone(), cfg.clone()).unwrap();
    let mut control = RendererSession::new(glass_scene, cfg).unwrap();
    let before = glass.render(&input).unwrap();
    control.render(&input).unwrap();
    let old_motion = hdr_pixels(&before.debug_readbacks[16]);
    let marked = before.debug_readbacks[18]
        .chunks_exact(16)
        .filter(|p| f32::from_le_bytes(p[12..16].try_into().unwrap()) < 0.)
        .count();
    assert!(
        marked > 100,
        "glass must mark the actual final history reactive"
    );
    let mut transform = IDENTITY;
    transform[12] = 0.7;
    let delta = SceneDelta {
        revision: 2,
        transforms: vec![TransformUpdate {
            instance_id: 12,
            transform,
        }],
        ..Default::default()
    };
    glass.apply_delta(delta.clone()).unwrap();
    control.apply_delta(delta).unwrap();
    input.reset_history = false;
    let moved = glass.render(&input).unwrap();
    input.reset_history = true;
    let reset_control = control.render(&input).unwrap();
    let new_motion = hdr_pixels(&moved.debug_readbacks[16]);
    let mut revealed = 0;
    for i in 0..old_motion.len() {
        if old_motion[i][3] > 0.5 && new_motion[i][3] < 0.5 {
            assert_eq!(
                &moved.rgba8[i * 4..i * 4 + 4],
                &reset_control.rgba8[i * 4..i * 4 + 4],
                "newly revealed glass background must not reuse the old translucent history"
            );
            revealed += 1;
        }
    }
    assert!(revealed > 100);
    println!(
        "REALTIME_PREPASS parity PASS; color/data texture pixels={checked}; glass reactive={marked}, revealed={revealed}"
    );
}

//@ spec: RXS-0409
#[test]
#[ignore = "requires exclusive real Vulkan/RayQuery device"]
fn gpu_positive_anisotropy_scatter_is_forward() {
    let matrix = camera();
    let input = FrameInput {
        view_projection: matrix,
        previous_view_projection: matrix,
        eye: [0., 0.4, 4.],
        output_extent: [192, 128],
        ..Default::default()
    };
    let mut scene = fixture();
    scene.environment.ambient_energy = 0.;
    let mut transform = IDENTITY;
    transform[13] = 0.4;
    scene.fog_volumes.push(FogVolume {
        id: 1,
        transform,
        size: [6., 4., 6.],
        shape: FogShape::Box,
        density: 0.08,
        albedo: [1.; 3],
        emission: [0.; 3],
        anisotropy: 0.6,
        edge_fade: 0.,
    });
    scene.lights[0].range = 20.;
    scene.lights[0].position = [0., 0.4, -4.];
    scene.lights[0].radius = 0.;
    scene.lights[0].cast_shadow = false;
    let cfg = RendererConfig {
        gi_enabled: false,
        reflections_enabled: false,
        shadows_enabled: false,
        ..Default::default()
    };
    let mut forward = RendererSession::new(scene.clone(), cfg.clone()).unwrap();
    let a = forward.render(&input).unwrap();
    scene.lights[0].position = [0., 0.4, 4.];
    let mut backward = RendererSession::new(scene, cfg).unwrap();
    let b = backward.render(&input).unwrap();
    let energy = |bytes: &[u8]| {
        hdr_pixels(bytes)
            .iter()
            .map(|p| f64::from(p[0]) + f64::from(p[1]) + f64::from(p[2]))
            .sum::<f64>()
    };
    let forward_energy = energy(&a.debug_readbacks[13]);
    let backward_energy = energy(&b.debug_readbacks[13]);
    assert!(
        forward_energy > backward_energy * 2.,
        "positive HG anisotropy must favor propagation toward the camera, forward={forward_energy} backward={backward_energy}"
    );
    println!("REALTIME_HG forward={forward_energy} backward={backward_energy}");
}

//@ spec: RXS-0409
#[test]
#[ignore = "requires exclusive real Vulkan/RayQuery device"]
fn gpu_tile_lights_match_exhaustive_and_cover_every_receiver() {
    let mut scene = fixture();
    scene.lights.push(Light {
        id: 2,
        position: [100., 100., 100.],
        range: 1.,
        intensity: 500.,
        ..Default::default()
    });
    scene.lights.push(Light {
        id: 3,
        position: [2., -0.4, -2.],
        range: 1.2,
        intensity: 30.,
        ..Default::default()
    });
    let matrix = camera();
    for extent in [[192, 128], [17, 11]] {
        let input = FrameInput {
            view_projection: matrix,
            previous_view_projection: matrix,
            eye: [0., 0.4, 4.],
            output_extent: extent,
            ..Default::default()
        };
        let mut tiled = RendererSession::new(
            scene.clone(),
            RendererConfig {
                tile_lights: true,
                ..Default::default()
            },
        )
        .unwrap();
        let a = tiled.render(&input).unwrap();
        let mut exhaustive = RendererSession::new(
            scene.clone(),
            RendererConfig {
                tile_lights: false,
                ..Default::default()
            },
        )
        .unwrap();
        let b = exhaustive.render(&input).unwrap();
        let masks = a.debug_readbacks[19]
            .chunks_exact(4)
            .map(|p| u32::from_le_bytes(p.try_into().unwrap()))
            .collect::<Vec<_>>();
        let width = a.internal_extent[0] as usize;
        let tiles_x = width.div_ceil(8);
        let mut required = 0;
        let mut occupied = std::collections::BTreeSet::new();
        for (pixel, bytes) in a.debug_readbacks[1].chunks_exact(16).enumerate() {
            let p = std::array::from_fn::<_, 4, _>(|i| {
                f32::from_le_bytes(bytes[i * 4..i * 4 + 4].try_into().unwrap())
            });
            if p[3] <= 0. {
                continue;
            }
            let tile = (pixel / width / 8) * tiles_x + (pixel % width / 8);
            occupied.insert(tile);
            for (i, light) in scene.lights.iter().enumerate() {
                let d2 = (0..3)
                    .map(|j| (f64::from(p[j]) - f64::from(light.position[j])).powi(2))
                    .sum::<f64>();
                if d2 <= f64::from(light.range).powi(2) {
                    assert_ne!(
                        masks[tile * 8 + i / 32] & (1u32 << (i % 32)),
                        0,
                        "tile mask omitted contributing light {i} at actual GPU receiver {pixel}"
                    );
                    required += 1;
                }
            }
        }
        assert_eq!(a.internal_extent, extent);
        assert!(required > if extent[0] > 100 { 1000 } else { 10 });
        let culled_far = occupied
            .iter()
            .filter(|&&tile| masks[tile * 8] & 2 == 0)
            .count();
        assert!(
            culled_far > if extent[0] > 100 { 100 } else { 0 },
            "a demonstrably out-of-range light must actually be culled"
        );
        assert_eq!(
            a.debug_readbacks[5], b.debug_readbacks[5],
            "tiled and exhaustive actual HDR direct lighting must be byte-identical"
        );
        assert_eq!(
            a.rgba8, b.rgba8,
            "light culling must not change final pixels"
        );
        println!(
        "REALTIME_TILE extent={extent:?} receiver_light_checks={required} occupied_tiles={} far_light_culled_tiles={culled_far} exact_HDR_and_final=PASS",
        occupied.len()
    );
    }
}

fn smith_g1(cosine: f64, alpha: f64) -> f64 {
    if cosine <= 0. {
        0.
    } else {
        2. * cosine / (cosine + (alpha * alpha + (1. - alpha * alpha) * cosine * cosine).sqrt())
    }
}
fn ggx_energy_quadrature(rough: f64, nv: f64, mu_steps: usize, phi_steps: usize) -> f64 {
    let alpha = rough * rough;
    let vx = (1. - nv * nv).sqrt();
    let gv = smith_g1(nv, alpha);
    let mut sum = 0.;
    for y in 0..mu_steps {
        let nl = (y as f64 + 0.5) / mu_steps as f64;
        let radial = (1. - nl * nl).sqrt();
        let gl = smith_g1(nl, alpha);
        for x in 0..phi_steps {
            let phi = (x as f64 + 0.5) * std::f64::consts::TAU / phi_steps as f64;
            let hx = vx + radial * phi.cos();
            let hy = radial * phi.sin();
            let hz = nv + nl;
            let nh = hz / (hx * hx + hy * hy + hz * hz).sqrt();
            let denominator = nh * nh * (alpha * alpha - 1.) + 1.;
            let d = alpha * alpha / (std::f64::consts::PI * denominator * denominator);
            sum += d * gv * gl / (4. * nv);
        }
    }
    sum * std::f64::consts::TAU / (mu_steps * phi_steps) as f64
}
fn gpu_vndf_samples(rough: f32, nv: f32, red: bool) -> Vec<[f32; 4]> {
    use rurix_rt::render_exec as rex;
    let count = 65536u32;
    let shader = super::shaders::vndf_probe_shader(red).unwrap();
    let resources = [rex::ResourceDesc::Buffer(rex::BufferDesc {
        size: u64::from(count) * 16,
        usage: rex::BufferUsage {
            storage: true,
            ..Default::default()
        },
        data: None,
        device_local: true,
    })];
    let mut push = Vec::new();
    push.extend(rough.to_le_bytes());
    push.extend(nv.to_le_bytes());
    push.extend(count.to_le_bytes());
    push.extend(51979u32.to_le_bytes());
    let passes = [rex::Pass::Compute(rex::ComputePass {
        name: "realtime_vndf_oracle",
        spirv: &shader,
        entry: None,
        dispatch: rex::DispatchSpec::Direct([count / 64, 1, 1]),
        bindings: rex::Bindings {
            storage_buffers: vec![0],
            push_constants: push,
            ..Default::default()
        },
    })];
    let plan = [(0, rex::TargetState::StorageWrite)];
    let barriers = [plan.as_slice()];
    let readbacks = [rex::Readback::Buffer {
        res: 0,
        offset: 0,
        size: u64::from(count) * 16,
    }];
    let mut session =
        rex::DeviceFrameSession::new(&resources, &passes, &barriers, &readbacks, 2).unwrap();
    let output = session.execute().unwrap();
    assert_eq!(output.telemetry.validation_error_count, 0);
    output.readbacks[0]
        .chunks_exact(16)
        .map(|p| {
            std::array::from_fn(|i| f32::from_le_bytes(p[i * 4..i * 4 + 4].try_into().unwrap()))
        })
        .collect()
}

//@ spec: RXS-0409
#[test]
#[ignore = "requires exclusive real Vulkan device; independent f64 quadrature"]
fn gpu_vndf_direction_pdf_energy_and_grazing_red() {
    for (rough, nv) in [(0.2f32, 0.1f32), (0.6, 0.05), (0.8, 0.7)] {
        let samples = gpu_vndf_samples(rough, nv, false);
        let rough64 = f64::from(rough);
        let nv64 = f64::from(nv);
        let alpha = rough64 * rough64;
        let view = [(1. - nv64 * nv64).sqrt(), 0., nv64];
        let mut sum = 0.;
        let mut square = 0.;
        for sample in &samples {
            assert!(sample.iter().all(|v| v.is_finite()));
            let h = [
                f64::from(sample[0]),
                f64::from(sample[1]),
                f64::from(sample[2]),
            ];
            let norm = h.iter().map(|v| v * v).sum::<f64>();
            assert!((norm - 1.).abs() < 2e-5 && h[2] >= -1e-6);
            let vh = view.iter().zip(h).map(|(v, h)| v * h).sum::<f64>();
            assert!(
                vh >= -1e-5,
                "visible normal must face the actual outgoing view"
            );
            let light: [f64; 3] = std::array::from_fn(|i| 2. * vh * h[i] - view[i]);
            let nl = light[2];
            let expected = if nl > 0. && vh > 0. && h[2] > 0. {
                let denominator = h[2] * h[2] * (alpha * alpha - 1.) + 1.;
                let d = alpha * alpha / (std::f64::consts::PI * denominator * denominator);
                let gv = smith_g1(nv64, alpha);
                let gl = smith_g1(nl, alpha);
                let brdf = d * gv * gl / (4. * nv64 * nl);
                let pdf = d * gv / (4. * nv64);
                brdf * nl / pdf
            } else {
                0.
            };
            let weight = f64::from(sample[3]);
            assert!(
                weight >= 0. && weight <= 1. + 1e-5,
                "exact visible-normal throughput must be bounded without clamping"
            );
            assert!((weight - expected).abs() < 128. * f64::from(f32::EPSILON) / alpha.max(0.01));
            sum += weight;
            square += weight * weight;
        }
        let mean = sum / samples.len() as f64;
        let variance = (square / samples.len() as f64 - mean * mean).max(0.);
        let coarse = ggx_energy_quadrature(rough64, nv64, 128, 512);
        let reference = ggx_energy_quadrature(rough64, nv64, 256, 1024);
        let bound = 6. * (variance / samples.len() as f64).sqrt()
            + 4. * (reference - coarse).abs()
            + 0.0001;
        assert!((mean-reference).abs()<=bound,"actual VNDF sample mean disagrees with independent BRDF integral: mean={mean} reference={reference} bound={bound}");
        if nv < 0.06 {
            let wrong = gpu_vndf_samples(rough, nv, true);
            let old_mean = wrong.iter().map(|s| f64::from(s[3])).sum::<f64>() / wrong.len() as f64;
            assert!(
                (old_mean - reference).abs() > bound * 2.,
                "NDF proposal with VNDF pdf must be detected as biased"
            );
            println!("REALTIME_VNDF_GRAZING mean={mean} reference={reference} bound={bound} NDF_red={old_mean}");
        }
    }
}

//@ spec: RXS-0409
#[test]
#[ignore = "requires exclusive real Vulkan device; reflected moving-object witness"]
fn gpu_grazing_reflection_motion_then_stop() {
    let mut scene = fixture();
    scene.materials[0].metallic = 1.;
    scene.materials[0].roughness = 0.15;
    scene.materials[0].base_color = [0.7, 0.7, 0.7, 1.];
    scene.environment.bloom_strength = 0.;
    let mut session = RendererSession::new(scene, Default::default()).unwrap();
    let matrix = camera();
    let input = FrameInput {
        view_projection: matrix,
        previous_view_projection: matrix,
        eye: [0., 0.4, 4.],
        output_extent: [192, 128],
        reset_history: false,
        ..Default::default()
    };
    let mut moving_hits = 0;
    let mut stats = Vec::new();
    for frame in 0..56 {
        if (40..48).contains(&frame) {
            let mut transform = IDENTITY;
            transform[12] = -0.15 * (frame - 39) as f32;
            session
                .apply_delta(SceneDelta {
                    revision: frame + 2,
                    transforms: vec![TransformUpdate {
                        instance_id: 13,
                        transform,
                    }],
                    ..Default::default()
                })
                .unwrap();
        }
        let output = session.render(&input).unwrap();
        if frame > 0 {
            assert!(
                !output.history_reset,
                "reflection motion must not globally reset history"
            );
        }
        let active = hdr_pixels(&output.debug_readbacks[7])
            .iter()
            .filter(|p| p[3] < 0.)
            .count();
        if (40..48).contains(&frame) {
            moving_hits += active;
        }
        if frame >= 48 {
            assert_eq!(
                active, 0,
                "stopped reflection hit must stop being motion-reactive"
            );
        }
        stats.push(format!(
            "{{\"frame\":{frame},\"movingReflectionHits\":{active}}}"
        ));
        if [0, 39, 42, 47, 48, 55].contains(&frame) {
            if let Ok(path) = std::env::var("RURIX_REALTIME_MOTION_PREVIEW_DIR") {
                std::fs::create_dir_all(&path).unwrap();
                std::fs::write(
                    std::path::Path::new(&path).join(format!("frame{frame:03}-192x128.rgba")),
                    &output.rgba8,
                )
                .unwrap();
            }
        }
    }
    assert!(
        moving_hits > 0,
        "actual floor rays must hit the moving emissive panel"
    );
    if let Ok(path) = std::env::var("RURIX_REALTIME_MOTION_PREVIEW_DIR") {
        std::fs::write(
            std::path::Path::new(&path).join("frames.json"),
            format!("[{}]", stats.join(",")),
        )
        .unwrap();
    }
    println!("REALTIME_REFLECTION_MOTION actual_moving_hits={moving_hits}; no global reset; stopped hits stable; visual acceptance remains main-scene video");
}

fn edge_fog(shape: FogShape, edge_fade: f32) -> FogVolume {
    FogVolume {
        id: 1,
        transform: IDENTITY,
        size: [4., 4., 5.],
        shape,
        density: 0.3,
        albedo: [0.; 3],
        emission: [1.; 3],
        anisotropy: 0.,
        edge_fade,
    }
}

//@ spec: RXS-0408, RXS-0409
#[test]
fn fog_edge_layout_and_invalid_values() {
    let mut scene = fixture();
    scene.fog_volumes.push(edge_fog(FogShape::Box, 0.12));
    let packed = super::scene::fog_volumes(&scene);
    assert_eq!(packed.len(), 32 * 128);
    assert_eq!(
        f32::from_le_bytes(packed[112..116].try_into().unwrap()),
        0.12
    );
    for edge in [-0.1, f32::NAN, f32::INFINITY] {
        scene.fog_volumes[0].edge_fade = edge;
        assert!(RendererSession::new(scene.clone(), Default::default()).is_err());
    }
}

//@ spec: RXS-0409
#[test]
#[ignore = "requires exclusive real Vulkan device; Godot SDF edge-fade oracle"]
fn gpu_fog_edge_fade_matches_shape_sdf() {
    use rurix_rt::render_exec as rex;
    let shader = super::shaders::compile(&format!(r#"{}
@group(0) @binding(0) var<storage,read_write> output:array<f32>;
@compute @workgroup_size(64) fn main(@builtin(global_invocation_id) id:vec3<u32>){{
 if(id.x>=16u){{return;}}let points=array<vec3<f32>,8>(vec3<f32>(0.),vec3<f32>(0.25,0.,0.),vec3<f32>(0.75,0.,0.),vec3<f32>(0.99,0.,0.),vec3<f32>(0.,0.75,0.),vec3<f32>(0.,0.,0.75),vec3<f32>(0.2,0.3,0.4),vec3<f32>(1.,0.,0.));
 output[id.x]=fog_edge_weight(points[id.x%8u],vec3<f32>(2.,3.,4.),f32(id.x/8u),0.12);
}}"#, include_str!("shaders/fog_density.wgsl")), "fog_edge_oracle").unwrap();
    let resources = [rex::ResourceDesc::Buffer(rex::BufferDesc {
        size: 64,
        usage: rex::BufferUsage {
            storage: true,
            ..Default::default()
        },
        data: None,
        device_local: true,
    })];
    let passes = [rex::Pass::Compute(rex::ComputePass {
        name: "fog_edge_oracle",
        spirv: &shader,
        entry: None,
        dispatch: rex::DispatchSpec::Direct([1, 1, 1]),
        bindings: rex::Bindings {
            storage_buffers: vec![0],
            ..Default::default()
        },
    })];
    let plan = [(0, rex::TargetState::StorageWrite)];
    let barriers = [plan.as_slice()];
    let readbacks = [rex::Readback::Buffer {
        res: 0,
        offset: 0,
        size: 64,
    }];
    let mut gpu =
        rex::DeviceFrameSession::new(&resources, &passes, &barriers, &readbacks, 2).unwrap();
    let output = gpu.execute().unwrap();
    assert_eq!(output.telemetry.validation_error_count, 0);
    let points: [[f64; 3]; 8] = [
        [0., 0., 0.],
        [0.25, 0., 0.],
        [0.75, 0., 0.],
        [0.99, 0., 0.],
        [0., 0.75, 0.],
        [0., 0., 0.75],
        [0.2, 0.3, 0.4],
        [1., 0., 0.],
    ];
    for (i, bytes) in output.readbacks[0].chunks_exact(4).enumerate() {
        let p = points[i % 8];
        let half = [2f64, 3., 4.];
        let inside = if i < 8 {
            (0..3)
                .map(|j| (1. - p[j].abs()) * half[j])
                .fold(f64::INFINITY, f64::min)
        } else {
            let k0 = p.iter().map(|v| v * v).sum::<f64>().sqrt();
            let k1 = (0..3).map(|j| (p[j] / half[j]).powi(2)).sum::<f64>().sqrt();
            if k1 == 0. {
                2.
            } else {
                k0 * (1. - k0) / k1
            }
        };
        let expected = (inside / 2.).clamp(0., 1.).powf(0.12);
        let actual = f32::from_le_bytes(bytes.try_into().unwrap());
        assert!(
            actual.is_finite() && (f64::from(actual) - expected).abs() < 2e-6,
            "SDF fade sample {i}: {actual} vs {expected}"
        );
    }
    drop(gpu);
    let matrix = camera();
    let input = FrameInput {
        view_projection: matrix,
        previous_view_projection: matrix,
        eye: [0., 0.4, 4.],
        output_extent: [192, 128],
        ..Default::default()
    };
    for shape in [FogShape::Box, FogShape::Ellipsoid] {
        let mut scene = fixture();
        scene.environment.ambient_energy = 0.;
        scene.fog_volumes.push(edge_fog(shape, 0.));
        let config = RendererConfig {
            gi_enabled: false,
            reflections_enabled: false,
            shadows_enabled: false,
            ..Default::default()
        };
        let mut hard = RendererSession::new(scene.clone(), config.clone()).unwrap();
        let a = hard.render(&input).unwrap();
        scene.fog_volumes[0].edge_fade = 0.12;
        let mut faded = RendererSession::new(scene, config).unwrap();
        let b = faded.render(&input).unwrap();
        let mut changed = 0;
        for (a, b) in hdr_pixels(&a.debug_readbacks[13])
            .iter()
            .zip(hdr_pixels(&b.debug_readbacks[13]))
        {
            assert!(
                b[3] + 0.0005 >= a[3],
                "fading edge density cannot increase opacity"
            );
            if b[3] > a[3] + 0.002 {
                changed += 1;
            }
        }
        assert!(changed > 100, "actual volume must consume edge_fade");
        println!("REALTIME_FOG_EDGE shape={shape:?} changed_actual_volume_pixels={changed}; analytic GPU SDF samples=16 PASS");
    }
}
