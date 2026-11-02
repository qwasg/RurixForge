//! CPU 预览光栅(rig_preview):把一帧形变画成 RGBA 图,可叠网格 / 骨骼 / 权重热力图,
//! 并可与参考图比较剪影 IoU。最近邻采样、直通 alpha、品红色键与视口同规则。

use crate::autogen::is_magenta;
use crate::compile::{CompiledRig, Skin};
use crate::doc::PartBlend;
use crate::eval::Deformed;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Overlay {
    None,
    Mesh,
    Bones,
    /// 指定骨骼下标的权重热力图。
    Weights(usize),
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PreviewOpts {
    pub width: u32,
    pub height: u32,
    pub background: [u8; 4],
    pub overlay: Overlay,
    pub chroma_key: bool,
}

impl Default for PreviewOpts {
    fn default() -> Self {
        Self { width: 512, height: 512, background: [0, 0, 0, 0], overlay: Overlay::None, chroma_key: true }
    }
}

/// 画布 → 输出图的等比适配变换:(scale, offset_x, offset_y)。
pub fn fit_transform(canvas: [f32; 2], out_w: u32, out_h: u32) -> (f32, f32, f32) {
    let s = (out_w as f32 / canvas[0]).min(out_h as f32 / canvas[1]);
    let ox = (out_w as f32 - canvas[0] * s) * 0.5;
    let oy = (out_h as f32 - canvas[1] * s) * 0.5;
    (s, ox, oy)
}

struct Canvas<'a> {
    px: &'a mut [u8],
    w: u32,
    h: u32,
}

impl Canvas<'_> {
    fn blend(&mut self, x: i32, y: i32, c: [f32; 4], mode: PartBlend) {
        if x < 0 || y < 0 || x >= self.w as i32 || y >= self.h as i32 || c[3] <= 0.0 {
            return;
        }
        let i = ((y as u32 * self.w + x as u32) * 4) as usize;
        let d = [
            self.px[i] as f32 / 255.0,
            self.px[i + 1] as f32 / 255.0,
            self.px[i + 2] as f32 / 255.0,
            self.px[i + 3] as f32 / 255.0,
        ];
        let sa = c[3].clamp(0.0, 1.0);
        let out = match mode {
            PartBlend::Alpha => {
                let oa = sa + d[3] * (1.0 - sa);
                if oa <= 0.0 {
                    [0.0; 4]
                } else {
                    let f = |k: usize| (c[k] * sa + d[k] * d[3] * (1.0 - sa)) / oa;
                    [f(0), f(1), f(2), oa]
                }
            }
            PartBlend::Additive => [
                (d[0] + c[0] * sa).min(1.0),
                (d[1] + c[1] * sa).min(1.0),
                (d[2] + c[2] * sa).min(1.0),
                (d[3] + sa).min(1.0),
            ],
            PartBlend::Multiply => {
                let f = |k: usize| d[k] * (1.0 - sa + c[k] * sa);
                [f(0), f(1), f(2), d[3].max(sa)]
            }
        };
        for k in 0..4 {
            self.px[i + k] = (out[k].clamp(0.0, 1.0) * 255.0).round() as u8;
        }
    }

    fn line(&mut self, a: [f32; 2], b: [f32; 2], c: [f32; 4]) {
        let n = ((b[0] - a[0]).abs().max((b[1] - a[1]).abs()).ceil() as i32).max(1);
        for k in 0..=n {
            let t = k as f32 / n as f32;
            self.blend(
                (a[0] + (b[0] - a[0]) * t).round() as i32,
                (a[1] + (b[1] - a[1]) * t).round() as i32,
                c,
                PartBlend::Alpha,
            );
        }
    }

    fn disc(&mut self, p: [f32; 2], r: f32, c: [f32; 4]) {
        let ri = r.ceil() as i32;
        for dy in -ri..=ri {
            for dx in -ri..=ri {
                if (dx * dx + dy * dy) as f32 <= r * r {
                    self.blend(p[0].round() as i32 + dx, p[1].round() as i32 + dy, c, PartBlend::Alpha);
                }
            }
        }
    }
}

fn heat(w: f32) -> [f32; 3] {
    let w = w.clamp(0.0, 1.0);
    // 蓝 → 绿 → 红
    if w < 0.5 {
        [0.0, w * 2.0, 1.0 - w * 2.0]
    } else {
        [(w - 0.5) * 2.0, 1.0 - (w - 0.5) * 2.0, 0.0]
    }
}

/// 渲染一帧预览,返回 RGBA8(width × height)。
pub fn render_preview(
    rig: &CompiledRig,
    d: &Deformed,
    atlas: &[u8],
    aw: u32,
    ah: u32,
    opts: &PreviewOpts,
) -> Vec<u8> {
    let (w, h) = (opts.width.max(1), opts.height.max(1));
    let mut px = vec![0u8; (w * h * 4) as usize];
    for c in px.chunks_exact_mut(4) {
        c.copy_from_slice(&opts.background);
    }
    let (s, ox, oy) = fit_transform(rig.doc.canvas.size, w, h);
    let to_out = |p: [f32; 2]| [p[0] * s + ox, p[1] * s + oy];
    let atlas_ok = atlas.len() >= (aw as usize) * (ah as usize) * 4 && aw > 0 && ah > 0;
    let mut cv = Canvas { px: &mut px, w, h };

    for dp in &d.parts {
        let uvs = &rig.doc.parts[dp.part].mesh.uvs;
        let weights: Option<Vec<f32>> = match opts.overlay {
            Overlay::Weights(b) => Some(match &rig.parts[dp.part].skin {
                Skin::None => vec![0.0; uvs.len()],
                Skin::Rigid(rb) => vec![if *rb == b { 1.0 } else { 0.0 }; uvs.len()],
                Skin::Weighted(ws) => ws
                    .iter()
                    .map(|inf| inf.iter().filter(|(k, _)| *k == b).map(|(_, w)| *w).sum())
                    .collect(),
            }),
            _ => None,
        };
        for t in &rig.parts[dp.part].tris {
            let (i0, i1, i2) = (t[0] as usize, t[1] as usize, t[2] as usize);
            let (p0, p1, p2) = (to_out(dp.positions[i0]), to_out(dp.positions[i1]), to_out(dp.positions[i2]));
            let area = (p1[0] - p0[0]) * (p2[1] - p0[1]) - (p2[0] - p0[0]) * (p1[1] - p0[1]);
            if area.abs() < 1e-6 {
                continue;
            }
            let minx = p0[0].min(p1[0]).min(p2[0]).floor().max(0.0) as i32;
            let maxx = p0[0].max(p1[0]).max(p2[0]).ceil().min(w as f32 - 1.0) as i32;
            let miny = p0[1].min(p1[1]).min(p2[1]).floor().max(0.0) as i32;
            let maxy = p0[1].max(p1[1]).max(p2[1]).ceil().min(h as f32 - 1.0) as i32;
            for y in miny..=maxy {
                for x in minx..=maxx {
                    let q = [x as f32 + 0.5, y as f32 + 0.5];
                    let w0 = ((p1[0] - q[0]) * (p2[1] - q[1]) - (p2[0] - q[0]) * (p1[1] - q[1])) / area;
                    let w1 = ((p2[0] - q[0]) * (p0[1] - q[1]) - (p0[0] - q[0]) * (p2[1] - q[1])) / area;
                    let w2 = 1.0 - w0 - w1;
                    if w0 < -1e-5 || w1 < -1e-5 || w2 < -1e-5 {
                        continue;
                    }
                    let u = uvs[i0][0] * w0 + uvs[i1][0] * w1 + uvs[i2][0] * w2;
                    let v = uvs[i0][1] * w0 + uvs[i1][1] * w1 + uvs[i2][1] * w2;
                    let mut c = if atlas_ok {
                        let tx = (u.floor() as i64).clamp(0, aw as i64 - 1) as u32;
                        let ty = (v.floor() as i64).clamp(0, ah as i64 - 1) as u32;
                        let i = ((ty * aw + tx) * 4) as usize;
                        let texel = &atlas[i..i + 4];
                        if texel[3] < 5 || (opts.chroma_key && is_magenta(texel)) {
                            continue;
                        }
                        [
                            texel[0] as f32 / 255.0,
                            texel[1] as f32 / 255.0,
                            texel[2] as f32 / 255.0,
                            texel[3] as f32 / 255.0 * dp.opacity,
                        ]
                    } else {
                        [0.8, 0.8, 0.8, dp.opacity]
                    };
                    if let Some(ws) = &weights {
                        let wt = ws[i0] * w0 + ws[i1] * w1 + ws[i2] * w2;
                        let hc = heat(wt);
                        c = [c[0] * 0.35 + hc[0] * 0.65, c[1] * 0.35 + hc[1] * 0.65, c[2] * 0.35 + hc[2] * 0.65, c[3].max(0.6)];
                    }
                    cv.blend(x, y, c, dp.blend);
                }
            }
        }
    }

    match opts.overlay {
        Overlay::Mesh => {
            for dp in &d.parts {
                for t in &rig.parts[dp.part].tris {
                    for (a, b) in [(t[0], t[1]), (t[1], t[2]), (t[2], t[0])] {
                        cv.line(to_out(dp.positions[a as usize]), to_out(dp.positions[b as usize]), [0.1, 1.0, 0.9, 0.8]);
                    }
                }
            }
        }
        Overlay::Bones | Overlay::Weights(_) => {
            for (i, b) in rig.doc.bones.iter().enumerate() {
                let m = d.bone_world.get(i).copied().unwrap_or_default();
                let head = to_out(m.apply([0.0, 0.0]));
                let tail = to_out(m.apply([b.length, 0.0]));
                let col = match opts.overlay {
                    Overlay::Weights(sel) if sel == i => [1.0, 0.9, 0.1, 1.0],
                    _ => [1.0, 0.45, 0.1, 0.95],
                };
                cv.line(head, tail, col);
                cv.disc(head, 3.0, col);
            }
        }
        Overlay::None => {}
    }
    px
}

/// 不透明掩膜(alpha ≥ 32 且非品红族)。
pub fn opaque_mask(rgba: &[u8], chroma_key: bool) -> Vec<bool> {
    rgba.chunks_exact(4).map(|p| p[3] >= 32 && !(chroma_key && is_magenta(p))).collect()
}

/// 两张同尺寸 RGBA 图的剪影 IoU(两者都空时为 1)。
pub fn silhouette_iou(a: &[u8], b: &[u8], chroma_key: bool) -> f32 {
    let (ma, mb) = (opaque_mask(a, chroma_key), opaque_mask(b, chroma_key));
    let (mut inter, mut uni) = (0u64, 0u64);
    for (x, y) in ma.iter().zip(&mb) {
        if *x && *y {
            inter += 1;
        }
        if *x || *y {
            uni += 1;
        }
    }
    if uni == 0 {
        1.0
    } else {
        inter as f32 / uni as f32
    }
}
