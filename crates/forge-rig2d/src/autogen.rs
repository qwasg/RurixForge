//! agent 辅助算法:自动网格(轮廓 → 简化 → 约束 Delaunay)、自动权重(骨段距离)、
//! 模板骨架(bust / fullbody 标准参数 + 骨骼 + 物理 + 自动项 + 基础骨骼 keyform)。

use std::collections::{BTreeMap, HashMap};

use spade::{ConstrainedDelaunayTriangulation, Point2, Triangulation};

use crate::doc::{
    AutoBlink, AutoBreath, Keyform, KeyformGroup, PhysicsInput, PhysicsInputKind, PhysicsOutput, PhysicsRig,
    PhysicsSegment, RigAuto, RigBone, RigDoc, RigMesh, RigParam,
};
use crate::math::{dist_point_segment, Affine};
use crate::{RigError, Result};

// ───────────────────────────── 自动网格 ─────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MeshAutoOpts {
    /// 轮廓简化容差(像素)。
    pub max_error: f32,
    /// 内部撒点间距(像素)。
    pub interior_spacing: f32,
    /// 不透明判定阈值 0..1。
    pub alpha_threshold: f32,
    /// 品红族视为透明(与视口色键同规则)。
    pub chroma_key: bool,
    /// 小于此像素数的连通块丢弃(噪点)。
    pub min_area: u32,
    /// 掩膜外扩像素(防止抗锯齿边被裁)。
    pub dilate: u32,
    /// 单部件顶点上限(超出则自动放大间距重试)。
    pub max_vertices: usize,
}

impl Default for MeshAutoOpts {
    fn default() -> Self {
        Self {
            max_error: 2.0,
            interior_spacing: 24.0,
            alpha_threshold: 0.05,
            chroma_key: true,
            min_area: 16,
            dilate: 1,
            max_vertices: 1500,
        }
    }
}

/// 视口色键同规则:g < 0.5·min(r,b)(品红族)。
pub fn is_magenta(px: &[u8]) -> bool {
    let (r, g, b) = (px[0] as f32, px[1] as f32, px[2] as f32);
    g < 0.5 * r.min(b)
}

fn pixel_inside(rgba: &[u8], w: u32, x: u32, y: u32, opts: &MeshAutoOpts) -> bool {
    let i = ((y * w + x) * 4) as usize;
    let px = &rgba[i..i + 4];
    px[3] as f32 > opts.alpha_threshold * 255.0 && !(opts.chroma_key && is_magenta(px))
}

/// 掩膜(带 1px 透明边框):mw = rw + 2,mh = rh + 2。
fn build_mask(rgba: &[u8], w: u32, h: u32, region: [u32; 4], opts: &MeshAutoOpts) -> (Vec<bool>, usize, usize) {
    let [rx, ry, rw, rh] = region;
    let (mw, mh) = (rw as usize + 2, rh as usize + 2);
    let mut m = vec![false; mw * mh];
    for y in 0..rh {
        for x in 0..rw {
            let (ax, ay) = (rx + x, ry + y);
            if ax < w && ay < h && pixel_inside(rgba, w, ax, ay, opts) {
                m[(y as usize + 1) * mw + x as usize + 1] = true;
            }
        }
    }
    // 去噪:小连通块
    let mut label = vec![0u32; mw * mh];
    let mut next = 1u32;
    for start in 0..m.len() {
        if !m[start] || label[start] != 0 {
            continue;
        }
        let mut stack = vec![start];
        let mut comp = vec![];
        label[start] = next;
        while let Some(i) = stack.pop() {
            comp.push(i);
            let (x, y) = (i % mw, i / mw);
            let mut push = |j: usize| {
                if m[j] && label[j] == 0 {
                    label[j] = next;
                    stack.push(j);
                }
            };
            if x > 0 {
                push(i - 1);
            }
            if x + 1 < mw {
                push(i + 1);
            }
            if y > 0 {
                push(i - mw);
            }
            if y + 1 < mh {
                push(i + mw);
            }
        }
        if (comp.len() as u32) < opts.min_area {
            for i in comp {
                m[i] = false;
            }
        }
        next += 1;
    }
    // 外扩(保留 1px 透明边框不被填满)
    for _ in 0..opts.dilate {
        let src = m.clone();
        for y in 1..mh - 1 {
            for x in 1..mw - 1 {
                let i = y * mw + x;
                if !src[i] && (src[i - 1] || src[i + 1] || src[i - mw] || src[i + mw]) {
                    m[i] = true;
                }
            }
        }
    }
    (m, mw, mh)
}

/// 像素边界环(格点坐标;外轮廓顺时针、孔逆时针,Y 向下)。
fn trace_loops(m: &[bool], mw: usize, mh: usize) -> Vec<Vec<[i32; 2]>> {
    let inside = |x: i32, y: i32| x >= 0 && y >= 0 && (x as usize) < mw && (y as usize) < mh && m[y as usize * mw + x as usize];
    let mut out: HashMap<(i32, i32), Vec<(i32, i32)>> = HashMap::new();
    for y in 0..mh as i32 {
        for x in 0..mw as i32 {
            if !inside(x, y) {
                continue;
            }
            if !inside(x, y - 1) {
                out.entry((x, y)).or_default().push((x + 1, y));
            }
            if !inside(x + 1, y) {
                out.entry((x + 1, y)).or_default().push((x + 1, y + 1));
            }
            if !inside(x, y + 1) {
                out.entry((x + 1, y + 1)).or_default().push((x, y + 1));
            }
            if !inside(x - 1, y) {
                out.entry((x, y + 1)).or_default().push((x, y));
            }
        }
    }
    let mut starts: Vec<(i32, i32)> = out.keys().copied().collect();
    starts.sort_unstable_by_key(|&(x, y)| (y, x));
    let mut loops = vec![];
    for s in starts {
        while out.get(&s).is_some_and(|v| !v.is_empty()) {
            let mut lp = vec![[s.0, s.1]];
            let mut cur = s;
            let mut dir = (0i32, 0i32);
            loop {
                let outs = out.get_mut(&cur).expect("edge graph");
                // 鞍点:优先右转(保持对角相接的像素块分离)
                let pick = if outs.len() > 1 {
                    let right = (-dir.1, dir.0);
                    outs.iter().position(|&(nx, ny)| (nx - cur.0, ny - cur.1) == right).unwrap_or(0)
                } else {
                    0
                };
                let nxt = outs.swap_remove(pick);
                dir = (nxt.0 - cur.0, nxt.1 - cur.1);
                cur = nxt;
                if cur == s {
                    break;
                }
                lp.push([cur.0, cur.1]);
            }
            loops.push(lp);
        }
    }
    loops
}

fn dp_simplify(pts: &[[f32; 2]], eps: f32) -> Vec<[f32; 2]> {
    if pts.len() < 3 {
        return pts.to_vec();
    }
    let mut keep = vec![false; pts.len()];
    keep[0] = true;
    keep[pts.len() - 1] = true;
    let mut stack = vec![(0usize, pts.len() - 1)];
    while let Some((a, b)) = stack.pop() {
        let mut best = (0.0f32, 0usize);
        for i in a + 1..b {
            let d = dist_point_segment(pts[i], pts[a], pts[b]);
            if d > best.0 {
                best = (d, i);
            }
        }
        if best.0 > eps {
            keep[best.1] = true;
            stack.push((a, best.1));
            stack.push((best.1, b));
        }
    }
    pts.iter().zip(keep).filter(|(_, k)| *k).map(|(p, _)| *p).collect()
}

/// 闭环简化:在离首点最远的点处切成两段分别 DP。
fn simplify_loop(lp: &[[f32; 2]], eps: f32) -> Vec<[f32; 2]> {
    if lp.len() <= 4 {
        return lp.to_vec();
    }
    let far = (1..lp.len())
        .max_by(|&i, &j| {
            let di = (lp[i][0] - lp[0][0]).powi(2) + (lp[i][1] - lp[0][1]).powi(2);
            let dj = (lp[j][0] - lp[0][0]).powi(2) + (lp[j][1] - lp[0][1]).powi(2);
            di.total_cmp(&dj)
        })
        .unwrap_or(lp.len() / 2);
    let mut a: Vec<[f32; 2]> = lp[..=far].to_vec();
    let mut b: Vec<[f32; 2]> = lp[far..].to_vec();
    b.push(lp[0]);
    a = dp_simplify(&a, eps);
    b = dp_simplify(&b, eps);
    a.pop();
    b.pop();
    a.extend(b);
    a
}

fn loop_area(lp: &[[f32; 2]]) -> f32 {
    let mut s = 0.0;
    for i in 0..lp.len() {
        let (p, q) = (lp[i], lp[(i + 1) % lp.len()]);
        s += p[0] * q[1] - q[0] * p[1];
    }
    s * 0.5
}

/// even-odd 点在多边形集合内。
pub fn point_in_loops(loops: &[Vec<[f32; 2]>], p: [f32; 2]) -> bool {
    let mut inside = false;
    for lp in loops {
        let n = lp.len();
        let mut j = n - 1;
        for i in 0..n {
            let (a, b) = (lp[i], lp[j]);
            if (a[1] > p[1]) != (b[1] > p[1]) && p[0] < (b[0] - a[0]) * (p[1] - a[1]) / (b[1] - a[1]) + a[0] {
                inside = !inside;
            }
            j = i;
        }
    }
    inside
}

fn dist_to_loops(loops: &[Vec<[f32; 2]>], p: [f32; 2]) -> f32 {
    let mut d = f32::INFINITY;
    for lp in loops {
        for i in 0..lp.len() {
            d = d.min(dist_point_segment(p, lp[i], lp[(i + 1) % lp.len()]));
        }
    }
    d
}

/// 从图集区域生成贴合轮廓的三角网格。
///
/// - `region`:图集像素 [x, y, w, h];
/// - `canvas_offset`:画布坐标 = 图集坐标 + offset(部件摆放平移)。
///
/// 返回的 mesh:vertices 为画布像素、uvs 为图集像素。
pub fn mesh_auto(
    rgba: &[u8],
    w: u32,
    h: u32,
    region: [u32; 4],
    canvas_offset: [f32; 2],
    opts: &MeshAutoOpts,
) -> Result<RigMesh> {
    if rgba.len() < (w as usize) * (h as usize) * 4 {
        return Err(RigError::new("RIG_MESH_FAILED", "图集像素缓冲长度不足"));
    }
    if region[2] == 0 || region[3] == 0 || region[0] >= w || region[1] >= h {
        return Err(RigError::new("RIG_MESH_FAILED", format!("区域非法: {region:?}")));
    }
    let (m, mw, mh) = build_mask(rgba, w, h, region, opts);
    let raw = trace_loops(&m, mw, mh);
    // 格点坐标(含 1px 边框)→ 图集坐标
    let to_atlas = |p: [i32; 2]| [region[0] as f32 + p[0] as f32 - 1.0, region[1] as f32 + p[1] as f32 - 1.0];
    let mut loops: Vec<Vec<[f32; 2]>> = raw
        .iter()
        .map(|lp| {
            let pts: Vec<[f32; 2]> = lp.iter().map(|&p| to_atlas(p)).collect();
            simplify_loop(&pts, opts.max_error.max(0.5))
        })
        .filter(|lp| lp.len() >= 3 && loop_area(lp).abs() >= 4.0)
        .collect();
    if loops.is_empty() {
        return Err(RigError::new("RIG_MESH_FAILED", format!("区域 {region:?} 内无不透明像素(检查 alphaThreshold / 色键)")));
    }
    loops.sort_by(|a, b| loop_area(b).abs().total_cmp(&loop_area(a).abs()));

    let mut spacing = opts.interior_spacing.max(4.0);
    for _attempt in 0..6 {
        let mesh = triangulate(&loops, region, spacing)?;
        if mesh.vertices.len() <= opts.max_vertices {
            return Ok(RigMesh {
                vertices: mesh.vertices.iter().map(|v| [v[0] + canvas_offset[0], v[1] + canvas_offset[1]]).collect(),
                uvs: mesh.vertices,
                triangles: mesh.triangles,
            });
        }
        spacing *= 1.5;
    }
    Err(RigError::new(
        "RIG_MESH_FAILED",
        format!("顶点数超过上限 {}(请调大 maxError / interiorSpacing)", opts.max_vertices),
    ))
}

struct RawMesh {
    vertices: Vec<[f32; 2]>,
    triangles: Vec<[u32; 3]>,
}

fn triangulate(loops: &[Vec<[f32; 2]>], region: [u32; 4], spacing: f32) -> Result<RawMesh> {
    let fail = |e: String| RigError::new("RIG_MESH_FAILED", e);
    let mut cdt: ConstrainedDelaunayTriangulation<Point2<f64>> = ConstrainedDelaunayTriangulation::new();
    for lp in loops {
        let hs: Vec<_> = lp
            .iter()
            .map(|p| cdt.insert(Point2::new(p[0] as f64, p[1] as f64)))
            .collect::<std::result::Result<_, _>>()
            .map_err(|e| fail(format!("三角化插点失败: {e:?}")))?;
        for i in 0..hs.len() {
            let (a, b) = (hs[i], hs[(i + 1) % hs.len()]);
            if a != b && cdt.can_add_constraint(a, b) {
                cdt.add_constraint(a, b);
            }
        }
    }
    // 内部撒点(交错网格,离边界 ≥ 0.5 间距)
    let (x0, y0) = (region[0] as f32, region[1] as f32);
    let (x1, y1) = (x0 + region[2] as f32, y0 + region[3] as f32);
    let mut row = 0;
    let mut y = y0 + spacing * 0.5;
    while y < y1 {
        let mut x = x0 + spacing * if row % 2 == 0 { 0.5 } else { 1.0 };
        while x < x1 {
            let p = [x, y];
            if point_in_loops(loops, p) && dist_to_loops(loops, p) > spacing * 0.5 {
                cdt.insert(Point2::new(x as f64, y as f64)).map_err(|e| fail(format!("三角化插点失败: {e:?}")))?;
            }
            x += spacing;
        }
        y += spacing * 0.866;
        row += 1;
    }
    let vertices: Vec<[f32; 2]> = cdt.vertices().map(|v| [v.position().x as f32, v.position().y as f32]).collect();
    let mut triangles = vec![];
    for f in cdt.inner_faces() {
        let vs = f.vertices();
        let idx = [vs[0].fix().index(), vs[1].fix().index(), vs[2].fix().index()];
        let c = [
            (vertices[idx[0]][0] + vertices[idx[1]][0] + vertices[idx[2]][0]) / 3.0,
            (vertices[idx[0]][1] + vertices[idx[1]][1] + vertices[idx[2]][1]) / 3.0,
        ];
        if point_in_loops(loops, c) {
            triangles.push([idx[0] as u32, idx[1] as u32, idx[2] as u32]);
        }
    }
    if triangles.is_empty() {
        return Err(fail("三角化结果为空".into()));
    }
    // 压缩未被引用的顶点
    let mut used = vec![u32::MAX; vertices.len()];
    let mut vs = vec![];
    for t in triangles.iter_mut() {
        for k in t.iter_mut() {
            let i = *k as usize;
            if used[i] == u32::MAX {
                used[i] = vs.len() as u32;
                vs.push(vertices[i]);
            }
            *k = used[i];
        }
    }
    Ok(RawMesh { vertices: vs, triangles })
}

/// 矩形网格(导入 PSD / 部件表时的初始 2 三角形网格)。
pub fn rect_mesh(atlas_rect: [f32; 4], canvas_pos: [f32; 2]) -> RigMesh {
    let [ax, ay, w, h] = atlas_rect;
    let [cx, cy] = canvas_pos;
    RigMesh {
        vertices: vec![[cx, cy], [cx + w, cy], [cx + w, cy + h], [cx, cy + h]],
        uvs: vec![[ax, ay], [ax + w, ay], [ax + w, ay + h], [ax, ay + h]],
        triangles: vec![[0, 1, 2], [0, 2, 3]],
    }
}

/// 从既有网格推断 图集→画布 平移(取首顶点;mesh_auto 重建时保持摆放)。
pub fn mesh_canvas_offset(mesh: &RigMesh) -> [f32; 2] {
    match (mesh.vertices.first(), mesh.uvs.first()) {
        (Some(v), Some(u)) => [v[0] - u[0], v[1] - u[1]],
        _ => [0.0, 0.0],
    }
}

/// 网格 UV 包围盒(图集像素整数区域)。
pub fn mesh_uv_region(mesh: &RigMesh) -> [u32; 4] {
    let mut b = [f32::INFINITY, f32::INFINITY, f32::NEG_INFINITY, f32::NEG_INFINITY];
    for u in &mesh.uvs {
        b[0] = b[0].min(u[0]);
        b[1] = b[1].min(u[1]);
        b[2] = b[2].max(u[0]);
        b[3] = b[3].max(u[1]);
    }
    if !b[0].is_finite() {
        return [0, 0, 0, 0];
    }
    let x = b[0].floor().max(0.0) as u32;
    let y = b[1].floor().max(0.0) as u32;
    [x, y, (b[2].ceil() as u32).saturating_sub(x), (b[3].ceil() as u32).saturating_sub(y)]
}

// ───────────────────────────── 自动权重 ─────────────────────────────

#[derive(Debug, Clone, PartialEq)]
pub struct WeightOpts {
    pub falloff: f32,
    pub smooth: bool,
    /// 仅考虑这些骨骼(None = 全部)。
    pub bones: Option<Vec<String>>,
}

impl Default for WeightOpts {
    fn default() -> Self {
        Self { falloff: 2.0, smooth: true, bones: None }
    }
}

/// 骨骼静止线段(起点, 终点)。
pub fn bone_segment(b: &RigBone) -> ([f32; 2], [f32; 2]) {
    let dir = Affine::rotate_deg(b.rotation).apply([b.length, 0.0]);
    (b.position, [b.position[0] + dir[0], b.position[1] + dir[1]])
}

fn top_k_normalize(ws: &mut Vec<(usize, f32)>) {
    ws.sort_by(|a, b| b.1.total_cmp(&a.1).then(a.0.cmp(&b.0)));
    ws.truncate(crate::doc::MAX_INFLUENCES);
    let sum: f32 = ws.iter().map(|w| w.1).sum();
    if sum > 0.0 {
        for w in ws.iter_mut() {
            w.1 /= sum;
        }
    }
    ws.retain(|w| w.1 >= 0.01);
    let sum: f32 = ws.iter().map(|w| w.1).sum();
    if sum > 0.0 {
        for w in ws.iter_mut() {
            w.1 = (w.1 / sum * 1000.0).round() / 1000.0;
        }
    }
}

/// 按骨段距离反比生成部件顶点权重(每顶点 ≤4 影响,归一化,可选一次拉普拉斯平滑)。
pub fn weights_auto(doc: &RigDoc, part_id: &str, opts: &WeightOpts) -> Result<Vec<Vec<(String, f32)>>> {
    let part = doc.part(part_id).ok_or_else(|| RigError::invalid(format!("部件不存在: {part_id}")))?;
    let bones: Vec<(usize, &RigBone)> = doc
        .bones
        .iter()
        .enumerate()
        .filter(|(_, b)| opts.bones.as_ref().is_none_or_contains(&b.id))
        .collect();
    if bones.is_empty() {
        return Err(RigError::invalid("没有可用骨骼:先 add_bone 或 merge_scaffold".into()));
    }
    let segs: Vec<_> = bones.iter().map(|(_, b)| bone_segment(b)).collect();
    let falloff = opts.falloff.clamp(0.5, 8.0);
    let mut per: Vec<Vec<(usize, f32)>> = part
        .mesh
        .vertices
        .iter()
        .map(|&v| {
            let mut ws: Vec<(usize, f32)> = segs
                .iter()
                .enumerate()
                .map(|(k, (a, b))| (k, 1.0 / (dist_point_segment(v, *a, *b) + 1.0).powf(falloff)))
                .collect();
            top_k_normalize(&mut ws);
            ws
        })
        .collect();
    if opts.smooth {
        let n = per.len();
        let mut adj: Vec<Vec<usize>> = vec![vec![]; n];
        for t in &part.mesh.triangles {
            for (a, b) in [(t[0], t[1]), (t[1], t[2]), (t[2], t[0])] {
                adj[a as usize].push(b as usize);
                adj[b as usize].push(a as usize);
            }
        }
        let src = per.clone();
        for i in 0..n {
            if adj[i].is_empty() {
                continue;
            }
            let mut acc: BTreeMap<usize, f32> = BTreeMap::new();
            for &(k, w) in &src[i] {
                *acc.entry(k).or_default() += 0.5 * w;
            }
            let share = 0.5 / adj[i].len() as f32;
            for &j in &adj[i] {
                for &(k, w) in &src[j] {
                    *acc.entry(k).or_default() += share * w;
                }
            }
            let mut ws: Vec<(usize, f32)> = acc.into_iter().collect();
            top_k_normalize(&mut ws);
            per[i] = ws;
        }
    }
    Ok(per
        .into_iter()
        .map(|ws| ws.into_iter().map(|(k, w)| (bones[k].1.id.clone(), w)).collect())
        .collect())
}

trait OptContains {
    fn is_none_or_contains(&self, s: &str) -> bool;
}
impl OptContains for Option<&Vec<String>> {
    fn is_none_or_contains(&self, s: &str) -> bool {
        match self {
            None => true,
            Some(v) => v.iter().any(|x| x == s),
        }
    }
}

// ───────────────────────────── 模板骨架 ─────────────────────────────

/// 模板产物(merge_scaffold 合并进文档,已存在的 id 跳过)。
#[derive(Debug, Clone, PartialEq)]
pub struct Scaffold {
    pub parameters: Vec<RigParam>,
    pub bones: Vec<RigBone>,
    pub keyforms: Vec<KeyformGroup>,
    pub physics: Vec<PhysicsRig>,
    pub auto: RigAuto,
}

pub const TEMPLATES: &[&str] = &["bust", "fullbody"];

fn p(id: &str, min: f32, max: f32, default: f32) -> RigParam {
    RigParam { id: id.into(), min, max, default, repeat: false }
}

fn bone(id: &str, parent: Option<&str>, pos: [f32; 2], rot: f32, len: f32) -> RigBone {
    RigBone { id: id.into(), parent: parent.map(Into::into), position: pos, rotation: rot, length: len }
}

fn bone_rot_group(bone: &str, param: &str, keys: [f32; 3], rots: [f32; 3]) -> KeyformGroup {
    KeyformGroup {
        target: format!("bone:{bone}"),
        params: vec![param.into()],
        keys: vec![keys.to_vec()],
        forms: rots.iter().map(|r| Keyform { rotation: Some(*r), ..Default::default() }).collect(),
    }
}

/// 标准参数 / 骨骼 / 物理 / 自动项(位置按画布尺寸比例给出,agent 需按部件实际位置 set_bone 微调)。
pub fn scaffold(template: &str, canvas: [f32; 2]) -> Result<Scaffold> {
    let [w, h] = canvas;
    let at = |x: f32, y: f32| [w * x, h * y];
    let parameters = vec![
        p("ParamAngleX", -30.0, 30.0, 0.0),
        p("ParamAngleY", -30.0, 30.0, 0.0),
        p("ParamAngleZ", -30.0, 30.0, 0.0),
        p("ParamEyeLOpen", 0.0, 1.0, 1.0),
        p("ParamEyeROpen", 0.0, 1.0, 1.0),
        p("ParamEyeBallX", -1.0, 1.0, 0.0),
        p("ParamEyeBallY", -1.0, 1.0, 0.0),
        p("ParamBrowLY", -1.0, 1.0, 0.0),
        p("ParamBrowRY", -1.0, 1.0, 0.0),
        p("ParamMouthOpenY", 0.0, 1.0, 0.0),
        p("ParamMouthForm", -1.0, 1.0, 0.0),
        p("ParamBodyAngleX", -10.0, 10.0, 0.0),
        p("ParamBodyAngleZ", -10.0, 10.0, 0.0),
        p("ParamBreath", 0.0, 1.0, 0.0),
        p("ParamHairFront", -1.0, 1.0, 0.0),
        p("ParamHairBack", -1.0, 1.0, 0.0),
    ];
    let mut bones;
    match template {
        "bust" => {
            bones = vec![
                bone("root", None, at(0.5, 0.98), -90.0, h * 0.2),
                bone("body", Some("root"), at(0.5, 0.78), -90.0, h * 0.26),
                bone("neck", Some("body"), at(0.5, 0.52), -90.0, h * 0.06),
                bone("head", Some("neck"), at(0.5, 0.46), -90.0, h * 0.3),
                bone("arm_l", Some("body"), at(0.33, 0.58), 100.0, h * 0.3),
                bone("arm_r", Some("body"), at(0.67, 0.58), 80.0, h * 0.3),
            ];
        }
        "fullbody" => {
            bones = vec![
                bone("root", None, at(0.5, 1.0), -90.0, h * 0.05),
                bone("hip", Some("root"), at(0.5, 0.55), -90.0, h * 0.05),
                bone("body", Some("hip"), at(0.5, 0.5), -90.0, h * 0.16),
                bone("neck", Some("body"), at(0.5, 0.33), -90.0, h * 0.03),
                bone("head", Some("neck"), at(0.5, 0.3), -90.0, h * 0.2),
                bone("arm_l", Some("body"), at(0.4, 0.36), 100.0, h * 0.15),
                bone("forearm_l", Some("arm_l"), at(0.38, 0.51), 95.0, h * 0.13),
                bone("arm_r", Some("body"), at(0.6, 0.36), 80.0, h * 0.15),
                bone("forearm_r", Some("arm_r"), at(0.62, 0.51), 85.0, h * 0.13),
                bone("leg_l", Some("hip"), at(0.45, 0.55), 90.0, h * 0.22),
                bone("shin_l", Some("leg_l"), at(0.45, 0.77), 90.0, h * 0.22),
                bone("leg_r", Some("hip"), at(0.55, 0.55), 90.0, h * 0.22),
                bone("shin_r", Some("leg_r"), at(0.55, 0.77), 90.0, h * 0.22),
            ];
        }
        other => {
            return Err(RigError::invalid(format!("未知模板 {other},可选: {}", TEMPLATES.join(" | "))));
        }
    }
    let head = bones.iter().find(|b| b.id == "head").expect("template has head").clone();
    let hair_root = [head.position[0], head.position[1] - head.length * 0.85];
    bones.push(bone("hair_front", Some("head"), hair_root, 90.0, head.length * 0.5));
    bones.push(bone("hair_back", Some("head"), hair_root, 90.0, head.length * 1.0));

    let keyforms = vec![
        bone_rot_group("head", "ParamAngleZ", [-30.0, 0.0, 30.0], [-15.0, 0.0, 15.0]),
        bone_rot_group("body", "ParamBodyAngleZ", [-10.0, 0.0, 10.0], [-8.0, 0.0, 8.0]),
        bone_rot_group("hair_front", "ParamHairFront", [-1.0, 0.0, 1.0], [-15.0, 0.0, 15.0]),
        bone_rot_group("hair_back", "ParamHairBack", [-1.0, 0.0, 1.0], [-12.0, 0.0, 12.0]),
        KeyformGroup {
            target: "bone:body".into(),
            params: vec!["ParamBreath".into()],
            keys: vec![vec![0.0, 1.0]],
            forms: vec![
                Keyform::default(),
                Keyform { scale: Some([1.0, 1.015]), ..Default::default() },
            ],
        },
    ];
    let hair_inputs = vec![
        PhysicsInput { param: "ParamAngleX".into(), weight: 0.6, kind: PhysicsInputKind::X },
        PhysicsInput { param: "ParamBodyAngleX".into(), weight: 0.4, kind: PhysicsInputKind::X },
        PhysicsInput { param: "ParamAngleZ".into(), weight: 0.5, kind: PhysicsInputKind::Angle },
    ];
    let seg = |len: f32| PhysicsSegment { length: len, damping: 0.1, stiffness: 20.0 };
    let physics = vec![
        PhysicsRig {
            id: "hair_front".into(),
            inputs: hair_inputs.clone(),
            segments: vec![seg(head.length * 0.5)],
            outputs: vec![PhysicsOutput { param: "ParamHairFront".into(), segment: 0, scale: 1.0 / 30.0 }],
            gravity: 980.0,
            input_scale: 30.0,
        },
        PhysicsRig {
            id: "hair_back".into(),
            inputs: hair_inputs,
            segments: vec![seg(head.length * 0.5), seg(head.length * 0.5)],
            outputs: vec![PhysicsOutput { param: "ParamHairBack".into(), segment: 1, scale: 1.0 / 30.0 }],
            gravity: 980.0,
            input_scale: 30.0,
        },
    ];
    let auto = RigAuto {
        blink: Some(AutoBlink {
            params: vec!["ParamEyeLOpen".into(), "ParamEyeROpen".into()],
            interval_sec: [2.5, 6.0],
            close_sec: 0.08,
            hold_sec: 0.04,
            open_sec: 0.12,
        }),
        breath: Some(AutoBreath { param: "ParamBreath".into(), period_sec: 3.2, amplitude: 1.0 }),
    };
    Ok(Scaffold { parameters, bones, keyforms, physics, auto })
}
