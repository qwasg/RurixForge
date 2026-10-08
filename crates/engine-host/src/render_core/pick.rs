//! 拾取(02 §3.2):射线 vs 单位立方体 OBB、视口点选,自 viewport.rs 逐字搬来。
//! 点选永远走 CPU(02 §3 P4),与渲染后端无关;场景有 ModelRenderer 时先做模型三角形求交。

use forge_scene::{Scene, Transform};

use super::camera::EditorCamera;
use super::math::{m3_apply, m3_transpose, quat_to_mat3, v3_sub, V3};
use super::sprite::{is_renderable, sprite_render_transform};

// ─────────────────────────── 射线 vs 单位立方体 OBB ───────────────────────────

/// 射线(世界系,dir 归一)与实体单位立方体 OBB 求交;命中返回参数 t(≥0,与世界射线同参)。
/// 局部化不除 d 的长度,保持参数一致(仿射变换保参数)。
pub fn ray_unit_cube(origin: V3, dir: V3, tr: &Transform) -> Option<f32> {
    let r = quat_to_mat3(tr.rotation);
    let rt = m3_transpose(r);
    let rel = v3_sub(origin, tr.translation);
    let o_l = m3_apply(rt, rel);
    let d_l = m3_apply(rt, dir);
    let s = tr.scale;
    let mut tmin = f32::NEG_INFINITY;
    let mut tmax = f32::INFINITY;
    for i in 0..3 {
        if s[i].abs() < 1e-9 {
            return None; // 零缩放实体不可见亦不可选
        }
        let o = o_l[i] / s[i];
        let d = d_l[i] / s[i];
        if d.abs() < 1e-12 {
            if o.abs() > 0.5 {
                return None;
            }
            continue;
        }
        let mut t1 = (-0.5 - o) / d;
        let mut t2 = (0.5 - o) / d;
        if t1 > t2 {
            std::mem::swap(&mut t1, &mut t2);
        }
        tmin = tmin.max(t1);
        tmax = tmax.min(t2);
        if tmin > tmax {
            return None;
        }
    }
    if tmax < 0.0 {
        return None;
    }
    Some(if tmin >= 0.0 { tmin } else { tmax })
}

/// 点选:像素坐标(左上原点) → 最近命中实体 (id, 命中点)。
pub fn pick_entity(
    scene: &Scene,
    cam: &EditorCamera,
    px: f32,
    py: f32,
    width: u32,
    height: u32,
) -> Option<(u64, V3)> {
    let aspect = width as f32 / height.max(1) as f32;
    let nx = 2.0 * px / width.max(1) as f32 - 1.0;
    let ny = 1.0 - 2.0 * py / height.max(1) as f32;
    let (origin, dir) = cam.ray(nx, ny, aspect);
    let mut best: Option<(u64, f32)> = if scene.entities.iter().any(|e|e.component("ModelRenderer").is_some_and(|c|c.enabled)) {super::model::pick(scene,origin,dir)}else{None};
    for e in &scene.entities {
        if !is_renderable(e) {
            continue;
        }
        // F-GAME-3/4:Sprite 实体按渲染态变换(帧尺寸 + pivot 锚定)参与点选,与画面一致。
        let tr = sprite_render_transform(e).unwrap_or(e.transform);
        if let Some(t) = ray_unit_cube(origin, dir, &tr) {
            if best.is_none_or(|(_, bt)| t < bt) {
                best = Some((e.id, t));
            }
        }
    }
    best.map(|(id, t)| {
        let p = [
            origin[0] + dir[0] * t,
            origin[1] + dir[1] * t,
            origin[2] + dir[2] * t,
        ];
        (id, p)
    })
}
