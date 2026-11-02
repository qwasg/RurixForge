//! Scene validation and deterministic GPU packing (RXS-0408).
use super::types::*;
use std::collections::{BTreeMap, BTreeSet};

pub(crate) const MATERIAL_BYTES: usize = 96;
pub(crate) const INSTANCE_BYTES: usize = 144;
pub(crate) const LIGHT_BYTES: usize = 80;
pub(crate) const TEX_HEADER: usize = 24;
pub(crate) const COLOR_LUT_WORDS: usize = 256;

pub(crate) struct PackedScene {
    pub vertices: Vec<u8>,
    pub materials: Vec<u8>,
    pub textures: Vec<u8>,
    pub ranges: Vec<(u32, u32)>,
    pub triangles: Vec<Vec<f32>>,
    pub triangle_count: usize,
    pub opaque_references: Vec<u8>,
}
pub(crate) fn floats(out: &mut Vec<u8>, v: &[f32]) {
    for f in v {
        out.extend_from_slice(&f.to_le_bytes());
    }
}
pub(crate) fn words(out: &mut Vec<u8>, v: &[u32]) {
    for f in v {
        out.extend_from_slice(&f.to_le_bytes());
    }
}
fn finite(v: &[f32]) -> bool {
    v.iter().all(|x| x.is_finite())
}

pub(crate) fn validate(scene: &SceneSnapshot, cfg: &RendererConfig) -> Result<(), String> {
    if scene.instances.is_empty() || scene.meshes.is_empty() || scene.materials.is_empty() {
        return Err("REALTIME_SCENE: geometry/material/instance sets must be nonempty".into());
    }
    if scene.lights.len() > cfg.max_lights || cfg.max_lights == 0 || cfg.max_lights > 256 {
        return Err("REALTIME_LIGHT: light capacity exceeded (maximum 256)".into());
    }
    let mut ids = BTreeSet::new();
    for i in &scene.instances {
        if !ids.insert(i.id)
            || i.mesh >= scene.meshes.len()
            || !finite(&i.transform)
            || !finite(&i.previous_transform)
        {
            return Err(
                "REALTIME_INSTANCE: duplicate ID, invalid mesh or nonfinite transform".into(),
            );
        }
        if i.transform[3] != 0.
            || i.transform[7] != 0.
            || i.transform[11] != 0.
            || i.transform[15] != 1.
        {
            return Err("REALTIME_INSTANCE: transform must be affine column-major".into());
        }
        let m = &i.transform;
        let det = m[0] * (m[5] * m[10] - m[6] * m[9]) - m[4] * (m[1] * m[10] - m[2] * m[9])
            + m[8] * (m[1] * m[6] - m[2] * m[5]);
        if det.abs() < 1e-10 {
            return Err("REALTIME_INSTANCE: singular transform".into());
        }
    }
    let mut bytes = 0u64;
    for m in &scene.meshes {
        if m.material >= scene.materials.len()
            || m.indices.is_empty()
            || m.indices.len() % 3 != 0
            || m.indices.iter().any(|&i| i as usize >= m.vertices.len())
        {
            return Err("REALTIME_MESH: invalid material or triangle indices".into());
        }
        for v in m.vertices.iter() {
            if !finite(&v.position) || !finite(&v.normal) || !finite(&v.uv) || !finite(&v.tangent) {
                return Err("REALTIME_MESH: nonfinite vertex".into());
            }
        }
        bytes = bytes
            .checked_add(m.indices.len() as u64 * 48)
            .ok_or("REALTIME_MESH: size overflow")?;
    }
    if bytes > cfg.max_geometry_bytes {
        return Err(
            "REALTIME_BUDGET: expanded vertex data exceeds explicit geometry budget".into(),
        );
    }
    for m in &scene.materials {
        if !finite(&m.base_color)
            || !finite(&m.emissive)
            || ![
                m.metallic,
                m.roughness,
                m.normal_scale,
                m.occlusion_strength,
                m.alpha_cutoff,
            ]
            .iter()
            .all(|f| f.is_finite())
            || !(0.0..=1.).contains(&m.metallic)
            || !(0.0..=1.).contains(&m.roughness)
            || !(0.0..=1.).contains(&m.base_color[3])
            || m.base_color[..3].iter().any(|f| *f < 0.)
            || m.emissive.iter().any(|f| *f < 0.)
            || m.textures
                .iter()
                .flatten()
                .any(|&i| i >= scene.textures.len())
        {
            return Err("REALTIME_MATERIAL: invalid values or texture reference".into());
        }
    }
    for t in &scene.textures {
        if t.width == 0
            || t.height == 0
            || t.width > 16384
            || t.height > 16384
            || t.rgba8.len() as u64 != u64::from(t.width) * u64::from(t.height) * 4
            || ![10497, 33071, 33648].contains(&t.wrap_s)
            || ![10497, 33071, 33648].contains(&t.wrap_t)
        {
            return Err("REALTIME_TEXTURE: dimensions, bytes or wrap mode invalid".into());
        }
    }
    for l in &scene.lights {
        if !finite(&l.position)
            || !finite(&l.direction)
            || !finite(&l.color)
            || !finite(&[
                l.intensity,
                l.range,
                l.radius,
                l.inner_cos,
                l.outer_cos,
                l.rect_size[0],
                l.rect_size[1],
            ])
            || l.intensity < 0.
            || l.range <= 0.
            || l.radius < 0.
            || l.color.iter().any(|c| *c < 0.)
            || (l.kind != LightKind::Point
                && l.direction.iter().map(|x| x * x).sum::<f32>() < 1e-12)
            || (l.kind == LightKind::Spot
                && (l.inner_cos < l.outer_cos || l.outer_cos < -1. || l.inner_cos > 1.))
            || (l.kind == LightKind::Rect && (l.rect_size[0] <= 0. || l.rect_size[1] <= 0.))
        {
            return Err("REALTIME_LIGHT: invalid light parameters".into());
        }
    }
    let e = &scene.environment;
    if !finite(&e.ambient_color)
        || !finite(&e.fog_color)
        || !finite(&[
            e.ambient_energy,
            e.exposure,
            e.bloom_strength,
            e.bloom_threshold,
            e.fog_density,
        ])
        || e.ambient_energy < 0.
        || e.exposure <= 0.
        || e.bloom_strength < 0.
        || e.bloom_threshold < 0.
        || e.fog_density < 0.
    {
        return Err("REALTIME_ENVIRONMENT: invalid HDR parameters".into());
    }
    if scene.fog_volumes.len() > 32 || scene.decals.len() > 128 {
        return Err("REALTIME_VOLUME: capacity exceeded (32 fog volumes / 128 decals)".into());
    }
    for f in &scene.fog_volumes {
        if !finite(&f.transform)
            || !finite(&f.size)
            || f.size.iter().any(|&x| x <= 0.)
            || !finite(&f.albedo)
            || !finite(&f.emission)
            || !finite(&[f.density, f.anisotropy, f.edge_fade])
            || f.density < 0.
            || f.edge_fade < 0.
            || f.anisotropy.abs() >= 0.95
            || inverse_affine(&f.transform).is_none()
        {
            return Err("REALTIME_FOG: invalid volume".into());
        }
    }
    for d in &scene.decals {
        if !finite(&d.transform)
            || !finite(&d.size)
            || d.size.iter().any(|&x| x <= 0.)
            || d.material >= scene.materials.len()
            || !d.opacity.is_finite()
            || !(0.0..=1.).contains(&d.opacity)
            || inverse_affine(&d.transform).is_none()
        {
            return Err("REALTIME_DECAL: invalid projector".into());
        }
    }
    Ok(())
}

fn srgb_linear(v: f32) -> f32 {
    if v <= 0.04045 {
        v / 12.92
    } else {
        ((v + 0.055) / 1.055).powf(2.4)
    }
}
fn linear_srgb(v: f32) -> f32 {
    if v <= 0.0031308 {
        v * 12.92
    } else {
        1.055 * v.powf(1. / 2.4) - 0.055
    }
}
fn mip(src: &[u8], w: u32, h: u32, kind: u8) -> Vec<u8> {
    let nw = (w / 2).max(1);
    let nh = (h / 2).max(1);
    let mut out = vec![0u8; (nw * nh * 4) as usize];
    for y in 0..nh {
        for x in 0..nw {
            let mut c = [0f32; 4];
            for dy in 0..2 {
                for dx in 0..2 {
                    let k = (((y * 2 + dy).min(h - 1) * w + (x * 2 + dx).min(w - 1)) * 4) as usize;
                    for j in 0..4 {
                        let f = src[k + j] as f32 / 255.;
                        c[j] += if kind == 0 && j < 3 {
                            srgb_linear(f)
                        } else {
                            f
                        };
                    }
                }
            }
            for j in 0..4 {
                c[j] *= 0.25;
            }
            if kind == 1 {
                let mut n = [c[0] * 2. - 1., c[1] * 2. - 1., c[2] * 2. - 1.];
                let len = n.iter().map(|a| a * a).sum::<f32>().sqrt();
                if len > 1e-5 {
                    for a in &mut n {
                        *a /= len;
                    }
                } else {
                    n = [0., 0., 1.];
                }
                for j in 0..3 {
                    c[j] = n[j] * 0.5 + 0.5;
                }
            }
            for j in 0..4 {
                let v = if kind == 0 && j < 3 {
                    linear_srgb(c[j])
                } else {
                    c[j]
                };
                out[((y * nw + x) * 4) as usize + j] = (v.clamp(0., 1.) * 255. + 0.5) as u8;
            }
        }
    }
    out
}

pub(crate) fn pack_materials(scene: &SceneSnapshot, ids: &BTreeMap<(usize, u8), u32>) -> Vec<u8> {
    let mut out = Vec::with_capacity(scene.materials.len() * MATERIAL_BYTES);
    for m in &scene.materials {
        floats(&mut out, &m.base_color);
        floats(
            &mut out,
            &[m.emissive[0], m.emissive[1], m.emissive[2], m.roughness],
        );
        floats(
            &mut out,
            &[
                m.metallic,
                m.normal_scale,
                m.occlusion_strength,
                m.alpha_cutoff,
            ],
        );
        words(
            &mut out,
            &[
                match m.alpha_mode {
                    AlphaMode::Opaque => 0,
                    AlphaMode::Mask => 1,
                    AlphaMode::Blend => 2,
                },
                u32::from(m.unlit),
                u32::from(m.double_sided),
                0,
            ],
        );
        for (j, t) in m.textures.iter().enumerate() {
            words(
                &mut out,
                &[t.map(|i| {
                    ids[&(
                        i,
                        if j == 0 || j == 4 {
                            0
                        } else if j == 1 {
                            1
                        } else {
                            2
                        },
                    )]
                })
                .unwrap_or(u32::MAX)],
            );
        }
        words(&mut out, &[0, 0, 0]);
    }
    out
}
pub(crate) fn texture_map(scene: &SceneSnapshot) -> BTreeMap<(usize, u8), u32> {
    let mut ids = BTreeMap::new();
    for m in &scene.materials {
        for (j, &t) in m.textures.iter().enumerate() {
            if let Some(t) = t {
                let kind = if j == 0 || j == 4 {
                    0
                } else if j == 1 {
                    1
                } else {
                    2
                };
                if !ids.contains_key(&(t, kind)) {
                    let n = ids.len() as u32;
                    ids.insert((t, kind), n);
                }
            }
        }
    }
    ids
}
pub(crate) fn pack(scene: &SceneSnapshot, cfg: &RendererConfig) -> Result<PackedScene, String> {
    validate(scene, cfg)?;
    let mut vertices = Vec::new();
    let mut ranges = Vec::new();
    let mut triangles = Vec::new();
    for m in &scene.meshes {
        let start = (vertices.len() / 48) as u32;
        let mut tri = Vec::new();
        for &idx in m.indices.iter() {
            let v = m.vertices[idx as usize];
            floats(&mut vertices, &v.position);
            floats(&mut vertices, &v.normal);
            floats(&mut vertices, &v.uv);
            floats(&mut vertices, &v.tangent);
            tri.extend_from_slice(&v.position);
        }
        ranges.push((start, m.indices.len() as u32));
        triangles.push(tri);
    }
    let ids = texture_map(scene);
    // Exact 8-bit sRGB inputs share one predecoded f32 table. This preserves the
    // authored bytes and all mip levels while removing per-tap shader powers.
    let mut atlas = vec![0u32; COLOR_LUT_WORDS + ids.len() * TEX_HEADER];
    for (value, slot) in atlas[..COLOR_LUT_WORDS].iter_mut().enumerate() {
        *slot = srgb_linear(value as f32 / 255.).to_bits();
    }
    for (&(tid, kind), &slot) in &ids {
        let t = &scene.textures[tid];
        let b = COLOR_LUT_WORDS + slot as usize * TEX_HEADER;
        let (mut w, mut h) = (t.width, t.height);
        let mut rgba = t.rgba8.as_ref().clone();
        atlas[b] = w;
        atlas[b + 1] = h;
        atlas[b + 2] = t.wrap_s;
        atlas[b + 3] = t.wrap_t;
        atlas[b + 5] = u32::from(kind);
        let mut level = 0;
        loop {
            if level >= 16 {
                return Err("REALTIME_TEXTURE: mip count exceeds 16".into());
            }
            atlas[b + 8 + level] = atlas.len() as u32;
            atlas.extend(
                rgba.chunks_exact(4)
                    .map(|p| u32::from_le_bytes([p[0], p[1], p[2], p[3]])),
            );
            level += 1;
            if atlas.len() as u64 * 4 > cfg.max_texture_bytes {
                return Err(
                    "REALTIME_BUDGET: all mip chains exceed explicit texture budget".into(),
                );
            }
            if w == 1 && h == 1 {
                break;
            }
            rgba = mip(&rgba, w, h, kind);
            w = (w / 2).max(1);
            h = (h / 2).max(1);
        }
        atlas[b + 4] = level as u32;
    }
    let mut textures = Vec::new();
    words(&mut textures, &atlas);
    let materials = pack_materials(scene, &ids);
    let mut opaque_references = Vec::new();
    for (instance, i) in scene.instances.iter().enumerate() {
        if scene.materials[scene.meshes[i.mesh].material].alpha_mode != AlphaMode::Blend {
            let (first, count) = ranges[i.mesh];
            for vertex in first..first + count {
                words(&mut opaque_references, &[vertex, instance as u32]);
            }
        }
    }
    Ok(PackedScene {
        triangle_count: scene
            .instances
            .iter()
            .map(|i| scene.meshes[i.mesh].indices.len() / 3)
            .sum(),
        vertices,
        materials,
        textures,
        ranges,
        triangles,
        opaque_references,
    })
}
pub(crate) fn instances(scene: &SceneSnapshot, ranges: &[(u32, u32)]) -> Vec<u8> {
    let mut out = Vec::with_capacity(scene.instances.len() * INSTANCE_BYTES);
    for (idx, i) in scene.instances.iter().enumerate() {
        floats(&mut out, &i.transform);
        floats(&mut out, &i.previous_transform);
        words(
            &mut out,
            &[
                ranges[i.mesh].0,
                ranges[i.mesh].1,
                scene.meshes[i.mesh].material as u32,
                idx as u32,
            ],
        );
    }
    out
}
pub(crate) fn lights(scene: &SceneSnapshot, capacity: usize) -> Vec<u8> {
    let mut out = Vec::new();
    for l in &scene.lights {
        floats(
            &mut out,
            &[l.position[0], l.position[1], l.position[2], l.range],
        );
        floats(
            &mut out,
            &[
                l.direction[0],
                l.direction[1],
                l.direction[2],
                match l.kind {
                    LightKind::Point => 0.,
                    LightKind::Spot => 1.,
                    LightKind::Rect => 2.,
                },
            ],
        );
        floats(&mut out, &[l.color[0], l.color[1], l.color[2], l.intensity]);
        floats(
            &mut out,
            &[
                l.radius,
                l.inner_cos,
                l.outer_cos,
                if l.cast_shadow { 1. } else { 0. },
            ],
        );
        floats(&mut out, &[l.rect_size[0], l.rect_size[1], 0., 0.]);
    }
    out.resize(capacity * LIGHT_BYTES, 0);
    out
}
pub(crate) fn as_transform(m: &Mat4) -> [f32; 12] {
    [
        m[0], m[4], m[8], m[12], m[1], m[5], m[9], m[13], m[2], m[6], m[10], m[14],
    ]
}
pub(crate) fn inverse_affine(m: &Mat4) -> Option<Mat4> {
    let a = [m[0], m[1], m[2]];
    let b = [m[4], m[5], m[6]];
    let c = [m[8], m[9], m[10]];
    let cross = |a: [f32; 3], b: [f32; 3]| {
        [
            a[1] * b[2] - a[2] * b[1],
            a[2] * b[0] - a[0] * b[2],
            a[0] * b[1] - a[1] * b[0],
        ]
    };
    let mut r0 = cross(b, c);
    let mut r1 = cross(c, a);
    let mut r2 = cross(a, b);
    let det = a[0] * r0[0] + a[1] * r0[1] + a[2] * r0[2];
    if det.abs() < 1e-10 || !det.is_finite() {
        return None;
    }
    for r in [&mut r0, &mut r1, &mut r2] {
        for v in r {
            *v /= det;
        }
    }
    let tr = |r: [f32; 3]| -(r[0] * m[12] + r[1] * m[13] + r[2] * m[14]);
    Some([
        r0[0],
        r1[0],
        r2[0],
        0.,
        r0[1],
        r1[1],
        r2[1],
        0.,
        r0[2],
        r1[2],
        r2[2],
        0.,
        tr(r0),
        tr(r1),
        tr(r2),
        1.,
    ])
}
pub(crate) fn fog_volumes(scene: &SceneSnapshot) -> Vec<u8> {
    let mut out = Vec::new();
    for f in &scene.fog_volumes {
        floats(
            &mut out,
            &inverse_affine(&f.transform).expect("validated fog transform"),
        );
        floats(
            &mut out,
            &[f.size[0] * 0.5, f.size[1] * 0.5, f.size[2] * 0.5, f.density],
        );
        floats(
            &mut out,
            &[f.albedo[0], f.albedo[1], f.albedo[2], f.anisotropy],
        );
        floats(
            &mut out,
            &[
                f.emission[0],
                f.emission[1],
                f.emission[2],
                if f.shape == FogShape::Ellipsoid {
                    1.
                } else {
                    0.
                },
            ],
        );
        floats(&mut out, &[f.edge_fade, 0., 0., 0.]);
    }
    out.resize(32 * 128, 0);
    out
}
pub(crate) fn decals(scene: &SceneSnapshot) -> Vec<u8> {
    let mut out = Vec::new();
    for d in &scene.decals {
        floats(&mut out, &d.transform);
        floats(
            &mut out,
            &inverse_affine(&d.transform).expect("validated decal transform"),
        );
        floats(
            &mut out,
            &[d.size[0] * 0.5, d.size[1] * 0.5, d.size[2] * 0.5, d.opacity],
        );
        words(&mut out, &[d.material as u32, 0, 0, 0]);
    }
    out.resize(128 * 160, 0);
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn color_mip_averages_linear_light() {
        let m = mip(
            &[
                255, 255, 255, 255, 0, 0, 0, 255, 255, 255, 255, 255, 0, 0, 0, 255,
            ],
            2,
            2,
            0,
        );
        assert!((187..=189).contains(&m[0]));
    }
    #[test]
    fn normal_mip_remains_normalized() {
        let m = mip(
            &[
                255, 128, 128, 255, 0, 128, 128, 255, 128, 128, 255, 255, 128, 128, 255, 255,
            ],
            2,
            2,
            1,
        );
        assert!(m[2] >= 253);
    }
    #[test]
    fn affine_layout_matches_vulkan() {
        let mut m = IDENTITY;
        m[12] = 3.;
        m[13] = 4.;
        m[14] = 5.;
        assert_eq!(
            as_transform(&m),
            [1., 0., 0., 3., 0., 1., 0., 4., 0., 0., 1., 5.]
        );
    }
}
