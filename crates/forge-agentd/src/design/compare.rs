//! D-045:设计稿 vs 引擎截帧的对比指标(纯函数、确定性)。
//!
//! 全局用 playtest 的 8×8 块亮度 SSIM(与 UltraPlan 回归比对同一口径),再补三样它没有的:
//! 逐通道色差(SSIM 只看亮度,配色错了它看不出)、逐元素区域得分(哪个按钮歪了一眼可查)、
//! 差异热力图(给模型和用户看「差在哪」)。两图尺寸须一致——缩放在调用方做(面积平均)。

use serde::Serialize;

use crate::playtest::ssim_luma;

/// RGBA8 图(straight alpha;对比时 alpha 不参与,截帧恒不透明)。
#[derive(Debug, Clone, PartialEq)]
pub struct Img {
    pub w: u32,
    pub h: u32,
    pub px: Vec<u8>,
}

impl Img {
    pub fn decode(bytes: &[u8]) -> Result<Img, String> {
        let img = image::load_from_memory(bytes)
            .map_err(|e| format!("图片解码失败: {e}"))?
            .to_rgba8();
        Ok(Img { w: img.width(), h: img.height(), px: img.into_raw() })
    }

    pub fn encode_png(&self) -> Result<Vec<u8>, String> {
        gend::mock::encode_png_rgba8(&self.px, self.w, self.h).map_err(|e| e.to_string())
    }

    /// 区域裁切(矩形会被夹到图内;空矩形 → None)。
    pub fn crop(&self, r: [u32; 4]) -> Option<Img> {
        let x0 = r[0].min(self.w);
        let y0 = r[1].min(self.h);
        let x1 = r[0].saturating_add(r[2]).min(self.w);
        let y1 = r[1].saturating_add(r[3]).min(self.h);
        if x1 <= x0 || y1 <= y0 {
            return None;
        }
        let (w, h) = (x1 - x0, y1 - y0);
        let mut px = Vec::with_capacity((w * h * 4) as usize);
        for y in y0..y1 {
            let s = ((y * self.w + x0) * 4) as usize;
            px.extend_from_slice(&self.px[s..s + (w * 4) as usize]);
        }
        Some(Img { w, h, px })
    }

    /// 缩放:缩小用面积平均(不引入振铃,对比更稳),放大用双线性。
    pub fn resize(&self, w: u32, h: u32) -> Img {
        if self.w == w && self.h == h {
            return self.clone();
        }
        let src = image::RgbaImage::from_raw(self.w, self.h, self.px.clone()).expect("尺寸与字节一致");
        let filter = if w < self.w || h < self.h {
            image::imageops::FilterType::Triangle
        } else {
            image::imageops::FilterType::CatmullRom
        };
        let out = image::imageops::resize(&src, w.max(1), h.max(1), filter);
        Img { w: out.width(), h: out.height(), px: out.into_raw() }
    }

    /// 等比缩到长边 ≤ max(给视觉模型的图省上下文)。
    pub fn fit_long_side(&self, max: u32) -> Img {
        let long = self.w.max(self.h);
        if long <= max {
            return self.clone();
        }
        let s = f64::from(max) / f64::from(long);
        self.resize(
            ((f64::from(self.w) * s).round() as u32).max(1),
            ((f64::from(self.h) * s).round() as u32).max(1),
        )
    }
}

/// 两图 SSIM(亮度);小于 8×8 的区域 SSIM 无块可算,退回 1 - 平均亮度差。
pub fn ssim(a: &Img, b: &Img) -> f64 {
    assert_eq!((a.w, a.h), (b.w, b.h), "SSIM 两图尺寸须一致");
    if a.w < 8 || a.h < 8 {
        return 1.0 - mean_abs_luma(a, b) / 255.0;
    }
    ssim_luma(&a.px, &b.px, a.w as usize, a.h as usize)
}

fn luma(p: &[u8]) -> f64 {
    0.299 * f64::from(p[0]) + 0.587 * f64::from(p[1]) + 0.114 * f64::from(p[2])
}

fn mean_abs_luma(a: &Img, b: &Img) -> f64 {
    let n = (a.w * a.h).max(1) as f64;
    a.px.chunks_exact(4).zip(b.px.chunks_exact(4)).map(|(x, y)| (luma(x) - luma(y)).abs()).sum::<f64>() / n
}

/// 平均逐通道色差(0..255;RGB 三通道绝对差的均值)。
pub fn mean_color_diff(a: &Img, b: &Img) -> f64 {
    assert_eq!((a.w, a.h), (b.w, b.h));
    let n = (a.w * a.h).max(1) as f64 * 3.0;
    a.px.chunks_exact(4)
        .zip(b.px.chunks_exact(4))
        .map(|(x, y)| (0..3).map(|i| (f64::from(x[i]) - f64::from(y[i])).abs()).sum::<f64>())
        .sum::<f64>()
        / n
}

/// 差异热力图:原图压暗为底,亮度差越大越红(差 ≥ 64 满红)。
pub fn diff_heatmap(a: &Img, b: &Img) -> Img {
    assert_eq!((a.w, a.h), (b.w, b.h));
    let mut px = Vec::with_capacity(a.px.len());
    for (x, y) in a.px.chunks_exact(4).zip(b.px.chunks_exact(4)) {
        let d = (0..3).map(|i| (i32::from(x[i]) - i32::from(y[i])).abs()).max().unwrap_or(0);
        let t = (f64::from(d) / 64.0).min(1.0);
        let base = luma(x) * 0.35;
        px.push((base * (1.0 - t) + 255.0 * t) as u8);
        px.push((base * (1.0 - t)) as u8);
        px.push((base * (1.0 - t)) as u8);
        px.push(255);
    }
    Img { w: a.w, h: a.h, px }
}

/// 单元素区域得分。
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct RegionScore {
    pub id: String,
    pub kind: String,
    pub source: String,
    pub bbox: [u32; 4],
    pub ssim: f64,
    pub color_diff: f64,
    pub threshold: f64,
    pub passed: bool,
}

/// 区域阈值(按素材来源):切图应与定稿几乎一致;重绘本就不逐像素一致,只要求大体相符;
/// 文字字体与原稿不可能逐像素相同,看位置与颜色是否大致对上。背景(干净底图)不单独打分——
/// 前景都抹掉了,它本来就该与定稿不同,由全局指标兜底。
pub fn region_threshold(source: &str) -> Option<f64> {
    match source {
        "crop" => Some(0.92),
        "regen" => Some(0.60),
        "text" => Some(0.50),
        _ => None,
    }
}

/// 色差上限(0..255):超过即判该区域配色不对,哪怕结构 SSIM 过线。
pub const REGION_COLOR_DIFF_MAX: f64 = 40.0;
/// 全局 SSIM 下限与全局色差上限。
pub const GLOBAL_SSIM_MIN: f64 = 0.85;
pub const GLOBAL_COLOR_DIFF_MAX: f64 = 18.0;

pub fn score_region(mockup: &Img, frame: &Img, id: &str, kind: &str, source: &str, bbox: [u32; 4]) -> Option<RegionScore> {
    let threshold = region_threshold(source)?;
    let (a, b) = (mockup.crop(bbox)?, frame.crop(bbox)?);
    let s = ssim(&a, &b);
    let c = mean_color_diff(&a, &b);
    Some(RegionScore {
        id: id.to_string(),
        kind: kind.to_string(),
        source: source.to_string(),
        bbox,
        ssim: round4(s),
        color_diff: round4(c),
        threshold,
        passed: s >= threshold && c <= REGION_COLOR_DIFF_MAX,
    })
}

pub fn round4(v: f64) -> f64 {
    (v * 10_000.0).round() / 10_000.0
}

#[cfg(test)]
mod tests {
    use super::*;

    fn gradient(w: u32, h: u32, shift: u32) -> Img {
        let mut px = Vec::new();
        for y in 0..h {
            for x in 0..w {
                let v = (((x + shift) * 7 + y * 3) % 256) as u8;
                px.extend_from_slice(&[v, v / 2, 255 - v, 255]);
            }
        }
        Img { w, h, px }
    }

    #[test]
    fn identical_images_score_perfect() {
        let a = gradient(64, 48, 0);
        assert!((ssim(&a, &a) - 1.0).abs() < 1e-9);
        assert_eq!(mean_color_diff(&a, &a), 0.0);
        let r = score_region(&a, &a, "btn", "button", "crop", [8, 8, 32, 16]).unwrap();
        assert!(r.passed);
        assert!(score_region(&a, &a, "bg", "background", "cleanplate", [0, 0, 64, 48]).is_none());
    }

    #[test]
    fn shifted_image_scores_lower() {
        let a = gradient(64, 48, 0);
        let b = gradient(64, 48, 9);
        assert!(ssim(&a, &b) < 0.95);
        assert!(mean_color_diff(&a, &b) > 1.0);
        let heat = diff_heatmap(&a, &b);
        assert_eq!((heat.w, heat.h), (64, 48));
        assert!(heat.px.chunks_exact(4).any(|p| p[0] > 200));
    }

    #[test]
    fn crop_clamps_and_resize_keeps_score_for_same_content() {
        let a = gradient(40, 40, 0);
        assert!(a.crop([30, 30, 50, 50]).is_some_and(|c| c.w == 10 && c.h == 10));
        assert!(a.crop([40, 0, 5, 5]).is_none());
        let half = a.resize(20, 20);
        assert_eq!((half.w, half.h), (20, 20));
        assert!((ssim(&half, &half.clone()) - 1.0).abs() < 1e-9);
        let small = a.fit_long_side(16);
        assert_eq!(small.w.max(small.h), 16);
    }

    #[test]
    fn tiny_regions_fall_back_to_luma_difference() {
        let a = gradient(4, 4, 0);
        assert!((ssim(&a, &a) - 1.0).abs() < 1e-9);
    }
}
