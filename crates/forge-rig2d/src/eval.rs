//! 每帧形变求值(固定顺序契约,E-08-005):
//! 1. 参数夹取 → 2. keyform 多线性插值 → 3. 骨骼 FK(静止 ⊕ keyform ⊕ clip/逻辑增量)
//! → 4. 部件顶点 = 基础 + keyform 偏移 → LBS/刚性蒙皮 → 父形变器链 → 5. 按 drawOrder 稳定排序。

use std::collections::BTreeMap;

use crate::compile::{CompiledRig, Skin, Target};
use crate::doc::{split_target, Interp, PartBlend, RigClip, RigDeformerKind, RigTrack, TargetKind};
use crate::math::{lerp, Affine};

/// 骨骼局部增量。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BoneDelta {
    pub position: [f32; 2],
    pub rotation: f32,
    pub scale: [f32; 2],
}

impl Default for BoneDelta {
    fn default() -> Self {
        Self { position: [0.0, 0.0], rotation: 0.0, scale: [1.0, 1.0] }
    }
}

/// 一帧的完整输入:参数值(按 doc.parameters 序)+ 骨骼增量 + 部件透明度乘子。
#[derive(Debug, Clone, PartialEq)]
pub struct Pose {
    pub params: Vec<f32>,
    pub bones: Vec<BoneDelta>,
    pub part_opacity: Vec<f32>,
}

impl Pose {
    pub fn rest(rig: &CompiledRig) -> Pose {
        Pose {
            params: rig.default_params(),
            bones: vec![BoneDelta::default(); rig.doc.bones.len()],
            part_opacity: vec![1.0; rig.doc.parts.len()],
        }
    }

    pub fn set_param(&mut self, rig: &CompiledRig, id: &str, v: f32) -> bool {
        match rig.param_index.get(id) {
            Some(&i) => {
                self.params[i] = rig.doc.parameters[i].clamp(v);
                true
            }
            None => false,
        }
    }

    pub fn param_map(&self, rig: &CompiledRig) -> BTreeMap<String, f32> {
        rig.doc.parameters.iter().zip(&self.params).map(|(p, v)| (p.id.clone(), *v)).collect()
    }
}

/// clip 时间归一化(循环取模;非循环夹取)。
pub fn clip_time(clip: &RigClip, t: f32) -> f32 {
    if clip.looped {
        t.rem_euclid(clip.duration)
    } else {
        t.clamp(0.0, clip.duration)
    }
}

/// 轨道在时间 t 的值(t 已归一化)。
pub fn sample_track(track: &RigTrack, t: f32) -> f32 {
    let k = &track.keys;
    if k.is_empty() {
        return 0.0;
    }
    if t <= k[0][0] {
        return k[0][1];
    }
    let last = k[k.len() - 1];
    if t >= last[0] {
        return last[1];
    }
    let i = k.partition_point(|key| key[0] <= t).saturating_sub(1);
    let (a, b) = (k[i], k[(i + 1).min(k.len() - 1)]);
    let span = b[0] - a[0];
    if span <= 1e-9 {
        return b[1];
    }
    let u = (t - a[0]) / span;
    let u = match track.interp {
        Interp::Linear => u,
        Interp::Step => 0.0,
        Interp::Smooth => u * u * (3.0 - 2.0 * u),
    };
    lerp(a[1], b[1], u)
}

/// 把 clip 在时间 t 的全部轨道叠到 pose 上(参数绝对写入;骨骼平移/旋转相加、缩放相乘;
/// 部件透明度相乘)。未知 clip 返回 false(调用方发 rig.warn)。
pub fn apply_clip(rig: &CompiledRig, pose: &mut Pose, clip: &str, time: f32) -> bool {
    let Some(c) = rig.doc.clips.get(clip) else { return false };
    let t = clip_time(c, time);
    for tr in &c.tracks {
        let v = sample_track(tr, t);
        let Some((kind, id, ch)) = split_target(&tr.target) else { continue };
        match kind {
            TargetKind::Param => {
                pose.set_param(rig, id, v);
            }
            TargetKind::Bone => {
                if let Some(&b) = rig.bone_index.get(id) {
                    let d = &mut pose.bones[b];
                    match ch {
                        Some("rotation") => d.rotation += v,
                        Some("x") => d.position[0] += v,
                        Some("y") => d.position[1] += v,
                        Some("sx") => d.scale[0] *= v,
                        Some("sy") => d.scale[1] *= v,
                        _ => {}
                    }
                }
            }
            TargetKind::Part => {
                if let Some(&p) = rig.part_index.get(id) {
                    pose.part_opacity[p] *= v.clamp(0.0, 1.0);
                }
            }
            TargetKind::Deformer => {}
        }
    }
    true
}

/// 渲染端 / 编辑态用:缺省 → clip(time) → 覆盖参数(绝对)。
/// 返回 pose 与未识别的名字(参数或 clip),供调用方告警。
pub fn pose_from(
    rig: &CompiledRig,
    overrides: &BTreeMap<String, f32>,
    clip: Option<&str>,
    time: f32,
) -> (Pose, Vec<String>) {
    let mut pose = Pose::rest(rig);
    let mut unknown = vec![];
    if let Some(c) = clip.filter(|c| !c.is_empty()) {
        if !apply_clip(rig, &mut pose, c, time) {
            unknown.push(format!("clip:{c}"));
        }
    }
    for (k, v) in overrides {
        if !pose.set_param(rig, k, *v) {
            unknown.push(format!("param:{k}"));
        }
    }
    (pose, unknown)
}

#[derive(Debug, Clone, PartialEq)]
pub struct DeformedPart {
    /// doc.parts 下标。
    pub part: usize,
    /// 画布像素坐标(与 doc.parts[part].mesh.uvs 一一对应)。
    pub positions: Vec<[f32; 2]>,
    pub opacity: f32,
    pub draw_order: i32,
    pub blend: PartBlend,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Deformed {
    /// 按最终 drawOrder 稳定排序(先画在前);不可见 / 透明度为 0 的部件已剔除。
    pub parts: Vec<DeformedPart>,
    /// 骨骼当帧世界矩阵(编辑器 overlay 用)。
    pub bone_world: Vec<Affine>,
    /// 画布像素包围盒 [minX, minY, maxX, maxY](无部件时全 0)。
    pub bbox: [f32; 4],
}

/// keyform 组的角点权重:(form 下标, 权重)。
pub fn corner_weights(keys: &[Vec<f32>], values: &[f32]) -> Vec<(usize, f32)> {
    let mut out = vec![(0usize, 1.0f32)];
    for (axis, ks) in keys.iter().enumerate() {
        let n = ks.len();
        let v = values[axis];
        let (i, t) = if n == 1 {
            (0, 0.0)
        } else if v <= ks[0] {
            (0, 0.0)
        } else if v >= ks[n - 1] {
            (n - 2, 1.0)
        } else {
            let i = ks.partition_point(|k| *k <= v).saturating_sub(1).min(n - 2);
            (i, (v - ks[i]) / (ks[i + 1] - ks[i]))
        };
        let mut next = Vec::with_capacity(out.len() * 2);
        for (idx, w) in out {
            let base = idx * n;
            if n == 1 || t <= 0.0 {
                next.push((base + i, w));
            } else if t >= 1.0 {
                next.push((base + i + 1, w));
            } else {
                next.push((base + i, w * (1.0 - t)));
                next.push((base + i + 1, w * t));
            }
        }
        out = next;
    }
    out
}

#[derive(Default, Clone)]
struct RotAcc {
    angle: f32,
    offset: [f32; 2],
    scale: [f32; 2],
}

/// 求值一帧形变。
pub fn evaluate(rig: &CompiledRig, pose: &Pose) -> Deformed {
    let doc = &rig.doc;
    let np = doc.parts.len();
    let nb = doc.bones.len();
    let nd = doc.deformers.len();

    // 2. keyform 累加
    let mut part_off: Vec<Option<Vec<[f32; 2]>>> = vec![None; np];
    let mut part_op = vec![1.0f32; np];
    let mut part_order: Vec<Option<(f32, i32)>> = vec![None; np];
    let mut warp_off: Vec<Option<Vec<[f32; 2]>>> = vec![None; nd];
    let mut rot = vec![RotAcc { scale: [1.0, 1.0], ..Default::default() }; nd];
    let mut bone_kf = vec![BoneDelta::default(); nb];

    for g in &rig.groups {
        let src = &doc.keyforms[g.src];
        let values: Vec<f32> = g.params.iter().map(|&i| pose.params[i]).collect();
        let corners = corner_weights(&src.keys, &values);
        match g.target {
            Target::Part(p) => {
                let nverts = doc.parts[p].mesh.vertices.len();
                let mut op = 0.0;
                for &(fi, w) in &corners {
                    let f = &src.forms[fi];
                    if let Some(o) = &f.offsets {
                        let acc = part_off[p].get_or_insert_with(|| vec![[0.0, 0.0]; nverts]);
                        for (a, d) in acc.iter_mut().zip(o) {
                            a[0] += d[0] * w;
                            a[1] += d[1] * w;
                        }
                    }
                    op += f.opacity.unwrap_or(1.0) * w;
                    if let Some(order) = f.draw_order {
                        if part_order[p].map_or(true, |(bw, _)| w > bw) {
                            part_order[p] = Some((w, order));
                        }
                    }
                }
                part_op[p] *= op.clamp(0.0, 1.0);
            }
            Target::Deformer(d) => match &doc.deformers[d].kind {
                RigDeformerKind::Warp { .. } => {
                    let npts = rig.deformer_rest_points[d].len();
                    for &(fi, w) in &corners {
                        if let Some(o) = &src.forms[fi].offsets {
                            let acc = warp_off[d].get_or_insert_with(|| vec![[0.0, 0.0]; npts]);
                            for (a, o) in acc.iter_mut().zip(o) {
                                a[0] += o[0] * w;
                                a[1] += o[1] * w;
                            }
                        }
                    }
                }
                RigDeformerKind::Rotation { .. } => {
                    let mut s = [0.0, 0.0];
                    for &(fi, w) in &corners {
                        let f = &src.forms[fi];
                        rot[d].angle += f.rotation.unwrap_or(0.0) * w;
                        let p = f.position.unwrap_or([0.0, 0.0]);
                        rot[d].offset[0] += p[0] * w;
                        rot[d].offset[1] += p[1] * w;
                        let sc = f.scale.unwrap_or([1.0, 1.0]);
                        s[0] += sc[0] * w;
                        s[1] += sc[1] * w;
                    }
                    rot[d].scale[0] *= s[0];
                    rot[d].scale[1] *= s[1];
                }
            },
            Target::Bone(b) => {
                let mut s = [0.0, 0.0];
                for &(fi, w) in &corners {
                    let f = &src.forms[fi];
                    bone_kf[b].rotation += f.rotation.unwrap_or(0.0) * w;
                    let p = f.position.unwrap_or([0.0, 0.0]);
                    bone_kf[b].position[0] += p[0] * w;
                    bone_kf[b].position[1] += p[1] * w;
                    let sc = f.scale.unwrap_or([1.0, 1.0]);
                    s[0] += sc[0] * w;
                    s[1] += sc[1] * w;
                }
                bone_kf[b].scale[0] *= s[0];
                bone_kf[b].scale[1] *= s[1];
            }
        }
    }

    // 3. 骨骼 FK
    let mut bone_world = vec![Affine::IDENTITY; nb];
    let mut skin = vec![Affine::IDENTITY; nb];
    for &i in &rig.bone_order {
        let k = &bone_kf[i];
        let p = pose.bones.get(i).copied().unwrap_or_default();
        let delta = Affine::trs(
            [k.position[0] + p.position[0], k.position[1] + p.position[1]],
            k.rotation + p.rotation,
            [k.scale[0] * p.scale[0], k.scale[1] * p.scale[1]],
        );
        let local = rig.bone_local_rest[i].mul(&delta);
        bone_world[i] = match rig.bone_parent[i] {
            Some(par) => bone_world[par].mul(&local),
            None => local,
        };
        skin[i] = bone_world[i].mul(&rig.bone_rest_inv[i]);
    }

    // 形变器:warp 控制点 / rotation 仿射
    let warp_pts: Vec<Vec<[f32; 2]>> = (0..nd)
        .map(|d| match &warp_off[d] {
            Some(off) => rig.deformer_rest_points[d]
                .iter()
                .zip(off)
                .map(|(r, o)| [r[0] + o[0], r[1] + o[1]])
                .collect(),
            None => rig.deformer_rest_points[d].clone(),
        })
        .collect();
    let rot_aff: Vec<Affine> = (0..nd)
        .map(|d| match &doc.deformers[d].kind {
            RigDeformerKind::Rotation { origin } => {
                let r = &rot[d];
                Affine::translate(origin[0] + r.offset[0], origin[1] + r.offset[1])
                    .mul(&Affine::rotate_deg(r.angle))
                    .mul(&Affine::scale(r.scale[0], r.scale[1]))
                    .mul(&Affine::translate(-origin[0], -origin[1]))
            }
            RigDeformerKind::Warp { .. } => Affine::IDENTITY,
        })
        .collect();
    let map_deformer = |mut d: usize, mut p: [f32; 2]| -> [f32; 2] {
        loop {
            p = match &doc.deformers[d].kind {
                RigDeformerKind::Warp { rect, grid } => warp_map(*rect, *grid, &warp_pts[d], p),
                RigDeformerKind::Rotation { .. } => rot_aff[d].apply(p),
            };
            match rig.deformer_parent[d] {
                Some(par) => d = par,
                None => return p,
            }
        }
    };

    // 4. 部件
    let mut out = Vec::with_capacity(np);
    let mut bbox = [f32::INFINITY, f32::INFINITY, f32::NEG_INFINITY, f32::NEG_INFINITY];
    for (pi, part) in doc.parts.iter().enumerate() {
        let opacity = part.opacity * part_op[pi] * pose.part_opacity.get(pi).copied().unwrap_or(1.0);
        if !part.visible || opacity <= 0.0 {
            continue;
        }
        let cp = &rig.parts[pi];
        let mut positions = part.mesh.vertices.clone();
        if let Some(off) = &part_off[pi] {
            for (v, o) in positions.iter_mut().zip(off) {
                v[0] += o[0];
                v[1] += o[1];
            }
        }
        match &cp.skin {
            Skin::None => {}
            Skin::Rigid(b) => {
                for v in positions.iter_mut() {
                    *v = skin[*b].apply(*v);
                }
            }
            Skin::Weighted(ws) => {
                for (v, inf) in positions.iter_mut().zip(ws) {
                    if inf.is_empty() {
                        continue;
                    }
                    let mut m = Affine::ZERO;
                    for &(b, w) in inf {
                        m = m.scaled_add(&skin[b], w);
                    }
                    *v = m.apply(*v);
                }
            }
        }
        if let Some(d) = cp.deformer {
            for v in positions.iter_mut() {
                *v = map_deformer(d, *v);
            }
        }
        for v in &positions {
            bbox[0] = bbox[0].min(v[0]);
            bbox[1] = bbox[1].min(v[1]);
            bbox[2] = bbox[2].max(v[0]);
            bbox[3] = bbox[3].max(v[1]);
        }
        out.push(DeformedPart {
            part: pi,
            positions,
            opacity: opacity.clamp(0.0, 1.0),
            draw_order: part_order[pi].map(|(_, o)| o).unwrap_or(part.draw_order),
            blend: part.blend,
        });
    }
    // 5. 稳定排序
    out.sort_by_key(|p| (p.draw_order, p.part));
    if out.is_empty() {
        bbox = [0.0; 4];
    }
    Deformed { parts: out, bone_world, bbox }
}

/// warp 网格双线性映射(网格外按边缘单元外推,静止网格下为恒等)。
pub fn warp_map(rect: [f32; 4], grid: [u32; 2], pts: &[[f32; 2]], p: [f32; 2]) -> [f32; 2] {
    let (c, r) = (grid[0] as usize, grid[1] as usize);
    let u = (p[0] - rect[0]) / rect[2] * c as f32;
    let v = (p[1] - rect[1]) / rect[3] * r as f32;
    let ci = (u.floor() as isize).clamp(0, c as isize - 1) as usize;
    let ri = (v.floor() as isize).clamp(0, r as isize - 1) as usize;
    let fu = u - ci as f32;
    let fv = v - ri as f32;
    let at = |i: usize, j: usize| pts[j * (c + 1) + i];
    let (p00, p10, p01, p11) = (at(ci, ri), at(ci + 1, ri), at(ci, ri + 1), at(ci + 1, ri + 1));
    let w00 = (1.0 - fu) * (1.0 - fv);
    let w10 = fu * (1.0 - fv);
    let w01 = (1.0 - fu) * fv;
    let w11 = fu * fv;
    [
        p00[0] * w00 + p10[0] * w10 + p01[0] * w01 + p11[0] * w11,
        p00[1] * w00 + p10[1] * w10 + p01[1] * w01 + p11[1] * w11,
    ]
}

/// 渲染用展开顶点(三角形列表,已按 drawOrder 排好)。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RigVertex {
    /// 画布像素。
    pub pos: [f32; 2],
    /// 图集像素。
    pub uv: [f32; 2],
    pub alpha: f32,
    /// 该顶点所属部件在绘制序中的名次(0 起;渲染端据此做 z 微偏)。
    pub layer: u32,
    pub blend: PartBlend,
}

/// 把 Deformed 展开为非索引三角形列表。
pub fn expand_triangles(rig: &CompiledRig, d: &Deformed) -> Vec<RigVertex> {
    let mut out = Vec::new();
    for (layer, dp) in d.parts.iter().enumerate() {
        let uvs = &rig.doc.parts[dp.part].mesh.uvs;
        for t in &rig.parts[dp.part].tris {
            for &k in t {
                let k = k as usize;
                out.push(RigVertex {
                    pos: dp.positions[k],
                    uv: uvs[k],
                    alpha: dp.opacity,
                    layer: layer as u32,
                    blend: dp.blend,
                });
            }
        }
    }
    out
}
