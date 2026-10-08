//! matte — 单张图的裁切 / 抠底 / 蒙版 / 缩放(D-045 设计稿复刻)。
//!
//! 色键规则直接复用 `video_frames`(四角采样、品红族、黑底)——视频截帧与设计稿切图同一套判据,
//! 不各写一份;本模块只补单图才需要的几件:差分抠图(定稿 vs 干净底图)、改图蒙版、等比放入。
//! 全部纯函数、确定性,不触盘(读写由调用方负责)。

use crate::video_frames::{alpha_bbox, apply_chroma_key, ChromaKey, Frame};
use crate::{GenError, Result, GEN_BAD_PARAMS};

/// PNG/JPEG 字节 → RGBA 帧。
pub fn decode(bytes: &[u8]) -> Result<Frame> {
    let img = image::load_from_memory(bytes)
        .map_err(|e| GenError::new(GEN_BAD_PARAMS, format!("图片解码失败: {e}")))?
        .to_rgba8();
    Ok(Frame { width: img.width(), height: img.height(), rgba: img.into_raw() })
}

/// RGBA 帧 → PNG 字节。
pub fn encode(f: &Frame) -> Result<Vec<u8>> {
    crate::mock::encode_png_rgba8(&f.rgba, f.width, f.height)
}

/// 像素矩形 [x, y, w, h] 裁到图内;完全落在图外或面积为 0 → None。
pub fn clamp_rect(f: &Frame, rect: [i64; 4]) -> Option<[u32; 4]> {
    let x0 = rect[0].clamp(0, i64::from(f.width));
    let y0 = rect[1].clamp(0, i64::from(f.height));
    let x1 = (rect[0] + rect[2]).clamp(0, i64::from(f.width));
    let y1 = (rect[1] + rect[3]).clamp(0, i64::from(f.height));
    (x1 > x0 && y1 > y0).then(|| [x0 as u32, y0 as u32, (x1 - x0) as u32, (y1 - y0) as u32])
}

/// 裁切(矩形须已在图内,见 [`clamp_rect`])。
pub fn crop(f: &Frame, r: [u32; 4]) -> Frame {
    let [x, y, w, h] = r;
    let mut rgba = Vec::with_capacity((w * h * 4) as usize);
    for row in y..y + h {
        let start = ((row * f.width + x) * 4) as usize;
        rgba.extend_from_slice(&f.rgba[start..start + (w * 4) as usize]);
    }
    Frame { width: w, height: h, rgba }
}

/// 高质量缩放(Lanczos3;精灵渲染是最近邻采样,素材须先缩到目标像素尺寸)。
pub fn resize(f: &Frame, w: u32, h: u32) -> Frame {
    if f.width == w && f.height == h {
        return f.clone();
    }
    let img = image::RgbaImage::from_raw(f.width, f.height, f.rgba.clone()).expect("帧字节与尺寸一致");
    let out = image::imageops::resize(&img, w.max(1), h.max(1), image::imageops::FilterType::Lanczos3);
    Frame { width: out.width(), height: out.height(), rgba: out.into_raw() }
}

/// 差分抠图:裁切块与干净底图同位置逐像素比较,差得越多越不透明。
/// `tol` 以下视为背景(alpha 0),`tol + feather` 以上全不透明,其间线性羽化。
/// 两图尺寸须一致。
pub fn diff_matte(crop: &Frame, plate: &Frame, tol: u8, feather: u8) -> Result<Frame> {
    if crop.width != plate.width || crop.height != plate.height {
        return Err(GenError::new(
            GEN_BAD_PARAMS,
            format!(
                "差分抠图尺寸不符: {}x{} vs {}x{}",
                crop.width, crop.height, plate.width, plate.height
            ),
        ));
    }
    let feather = i32::from(feather.max(1));
    let tol = i32::from(tol);
    let mut out = crop.clone();
    for (p, b) in out.rgba.chunks_exact_mut(4).zip(plate.rgba.chunks_exact(4)) {
        let d = (0..3).map(|i| (i32::from(p[i]) - i32::from(b[i])).abs()).max().unwrap_or(0);
        let a = ((d - tol) * 255 / feather).clamp(0, 255);
        p[3] = (i32::from(p[3]) * a / 255) as u8;
    }
    Ok(out)
}

/// 色键抠图(四角采样 / 品红 / 黑底;与视频截帧同一规则)。
pub fn key_matte(f: &Frame, mode: ChromaKey) -> Frame {
    let mut frames = [f.clone()];
    apply_chroma_key(&mut frames, mode);
    let [out] = frames;
    out
}

/// 抠图质量:不透明像素占比、边缘一圈的不透明占比(切到别的元素或背景没抠干净时偏高)。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MatteQuality {
    pub opaque_ratio: f64,
    pub border_opaque_ratio: f64,
}

pub fn quality(f: &Frame) -> MatteQuality {
    let total = (f.width * f.height).max(1) as f64;
    let opaque = f.rgba.chunks_exact(4).filter(|p| p[3] >= 128).count() as f64;
    let (mut border, mut border_opaque) = (0usize, 0usize);
    for y in 0..f.height {
        for x in 0..f.width {
            if x == 0 || y == 0 || x + 1 == f.width || y + 1 == f.height {
                border += 1;
                if f.rgba[((y * f.width + x) * 4 + 3) as usize] >= 128 {
                    border_opaque += 1;
                }
            }
        }
    }
    MatteQuality {
        opaque_ratio: opaque / total,
        border_opaque_ratio: border_opaque as f64 / border.max(1) as f64,
    }
}

/// 改图蒙版(OpenAI images/edits 语义):矩形区(外扩 pad)alpha=0 = 允许重绘,其余 alpha=255 保持。
pub fn edit_mask(width: u32, height: u32, rects: &[[i64; 4]], pad: i64) -> Frame {
    let mut rgba = vec![0u8; (width * height * 4) as usize];
    for p in rgba.chunks_exact_mut(4) {
        p[3] = 255;
    }
    let canvas = Frame { width, height, rgba: Vec::new() };
    for r in rects {
        let Some([x, y, w, h]) = clamp_rect(&canvas, [r[0] - pad, r[1] - pad, r[2] + 2 * pad, r[3] + 2 * pad]) else {
            continue;
        };
        for row in y..y + h {
            for col in x..x + w {
                rgba[((row * width + col) * 4 + 3) as usize] = 0;
            }
        }
    }
    Frame { width, height, rgba }
}

/// 蒙版外逐像素恢复原图(改图模型可能轻微改动蒙版外区域;复刻要求蒙版外与定稿一致)。
pub fn restore_outside_mask(edited: &Frame, original: &Frame, mask: &Frame) -> Result<Frame> {
    if edited.width != original.width
        || edited.height != original.height
        || mask.width != original.width
        || mask.height != original.height
    {
        return Err(GenError::new(GEN_BAD_PARAMS, "恢复蒙版外区域:三图尺寸须一致"));
    }
    let mut out = edited.clone();
    for ((p, o), m) in out
        .rgba
        .chunks_exact_mut(4)
        .zip(original.rgba.chunks_exact(4))
        .zip(mask.rgba.chunks_exact(4))
    {
        if m[3] != 0 {
            p.copy_from_slice(o);
        }
    }
    Ok(out)
}

/// 去掉四周全透明边,返回 (紧致帧, 在原帧中的 [x,y,w,h]);全透明 → None。
pub fn trim(f: &Frame) -> Option<(Frame, [u32; 4])> {
    let b = alpha_bbox(f)?;
    Some((crop(f, b), b))
}

/// 等比放入 w×h 透明画布并居中(重绘元素按定稿 bbox 落位用)。
pub fn fit_into(f: &Frame, w: u32, h: u32) -> Frame {
    let s = (f64::from(w) / f64::from(f.width.max(1))).min(f64::from(h) / f64::from(f.height.max(1)));
    let (nw, nh) = (
        ((f64::from(f.width) * s).round() as u32).clamp(1, w.max(1)),
        ((f64::from(f.height) * s).round() as u32).clamp(1, h.max(1)),
    );
    let scaled = resize(f, nw, nh);
    let mut rgba = vec![0u8; (w * h * 4) as usize];
    let (ox, oy) = ((w - nw) / 2, (h - nh) / 2);
    for row in 0..nh {
        let src = (row * nw * 4) as usize;
        let dst = (((oy + row) * w + ox) * 4) as usize;
        rgba[dst..dst + (nw * 4) as usize].copy_from_slice(&scaled.rgba[src..src + (nw * 4) as usize]);
    }
    Frame { width: w, height: h, rgba }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn solid(w: u32, h: u32, c: [u8; 4]) -> Frame {
        Frame { width: w, height: h, rgba: c.iter().copied().cycle().take((w * h * 4) as usize).collect() }
    }

    #[test]
    fn crop_and_clamp() {
        let mut f = solid(4, 4, [0, 0, 0, 255]);
        f.rgba[((1 * 4 + 2) * 4) as usize] = 9;
        let r = clamp_rect(&f, [2, 1, 10, 10]).unwrap();
        assert_eq!(r, [2, 1, 2, 3]);
        let c = crop(&f, r);
        assert_eq!((c.width, c.height), (2, 3));
        assert_eq!(c.rgba[0], 9);
        assert!(clamp_rect(&f, [5, 5, 2, 2]).is_none());
    }

    #[test]
    fn diff_matte_separates_foreground() {
        let plate = solid(2, 1, [10, 10, 10, 255]);
        let mut crop_f = plate.clone();
        crop_f.rgba[4..8].copy_from_slice(&[250, 10, 10, 255]);
        let m = diff_matte(&crop_f, &plate, 12, 24).unwrap();
        assert_eq!(m.rgba[3], 0);
        assert_eq!(m.rgba[7], 255);
        assert!(diff_matte(&crop_f, &solid(1, 1, [0; 4]), 1, 1).is_err());
    }

    #[test]
    fn mask_marks_padded_rects_transparent() {
        let m = edit_mask(10, 10, &[[4, 4, 2, 2]], 1);
        let a = |x: u32, y: u32| m.rgba[((y * 10 + x) * 4 + 3) as usize];
        assert_eq!(a(3, 3), 0);
        assert_eq!(a(6, 6), 0);
        assert_eq!(a(2, 2), 255);
        assert_eq!(a(7, 7), 255);
    }

    #[test]
    fn restore_keeps_unmasked_pixels() {
        let orig = solid(2, 1, [1, 2, 3, 255]);
        let edited = solid(2, 1, [9, 9, 9, 255]);
        let mask = edit_mask(2, 1, &[[1, 0, 1, 1]], 0);
        let out = restore_outside_mask(&edited, &orig, &mask).unwrap();
        assert_eq!(&out.rgba[0..4], &[1, 2, 3, 255]);
        assert_eq!(&out.rgba[4..8], &[9, 9, 9, 255]);
    }

    #[test]
    fn fit_into_centers_and_keeps_aspect() {
        let f = solid(4, 2, [255, 0, 0, 255]);
        let out = fit_into(&f, 8, 8);
        assert_eq!((out.width, out.height), (8, 8));
        assert_eq!(out.rgba[3], 0);
        let (_, b) = trim(&out).unwrap();
        assert_eq!(b, [0, 2, 8, 4]);
    }

    #[test]
    fn quality_reports_border_coverage() {
        let q = quality(&solid(3, 3, [0, 0, 0, 255]));
        assert_eq!(q.opaque_ratio, 1.0);
        assert_eq!(q.border_opaque_ratio, 1.0);
        let q2 = quality(&solid(3, 3, [0, 0, 0, 0]));
        assert_eq!(q2.opaque_ratio, 0.0);
    }

    #[test]
    fn key_matte_reuses_video_rule() {
        let mut f = solid(16, 16, [255, 0, 255, 255]);
        f.rgba[(8 * 16 + 8) * 4..(8 * 16 + 8) * 4 + 4].copy_from_slice(&[20, 200, 20, 255]);
        let m = key_matte(&f, ChromaKey::Magenta);
        assert_eq!(m.rgba[3], 0);
        assert_eq!(m.rgba[(8 * 16 + 8) * 4 + 3], 255);
    }
}
