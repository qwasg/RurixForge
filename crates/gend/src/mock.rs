//! local-mock 适配器(D-F5-A):**确定性占位生成器,非 AI 模型**。
//! seed + prompt 哈希驱动正弦条纹(木纹风)+ 噪声扰动,同 seed 同 prompt 复跑字节一致
//! (纯函数 + image crate PNG 编码器确定性输出)。CI 恒绿腿;backendId=local-mock 如实进
//! provenance,不伪装远程。

use serde_json::{json, Value};

use crate::backends::{GenBackend, GenCandidate, GenRequest, MAX_BATCH, SIZES};
use crate::config::GenConfig;
use crate::keystore::Keystore;
use crate::{fnv1a64, GenError, Result, GEN_BAD_PARAMS};

pub struct LocalMock;

pub const LOCAL_MOCK_ID: &str = "local-mock";

impl GenBackend for LocalMock {
    fn id(&self) -> &str {
        LOCAL_MOCK_ID
    }

    fn kind(&self) -> &str {
        "local"
    }

    /// local 类:条目存在且 enabled 即 configured(无需密钥/端点,D-F5-A)。
    fn configured(&self, cfg: &GenConfig, _keys: &Keystore) -> bool {
        cfg.entry(LOCAL_MOCK_ID)
            .map(|e| e.kind == "local" && e.enabled)
            .unwrap_or(false)
    }

    fn capabilities(&self) -> Value {
        json!({
            "kinds": ["text2img", "texture-set", "variations"],
            "sizes": SIZES,
            "maxBatch": MAX_BATCH,
            // 诚实标注:占位确定性生成器,非 AI 模型。
            "model": "deterministic-placeholder (非 AI 模型)",
        })
    }

    fn generate(&self, req: &GenRequest, _cfg: &GenConfig, _keys: &Keystore) -> Result<Vec<GenCandidate>> {
        validate_size(req.size)?;
        if req.n == 0 || req.n > MAX_BATCH {
            return Err(GenError::new(
                GEN_BAD_PARAMS,
                format!("n 须 1..={MAX_BATCH},实: {}", req.n),
            ));
        }
        let mut out = Vec::with_capacity(req.n as usize);
        for i in 0..req.n {
            let seed = req.seed.wrapping_add(u64::from(i));
            let png = render_map(&req.prompt, "albedo", req.size, seed)?;
            out.push(GenCandidate { png_bytes: png, seed });
        }
        Ok(out)
    }
}

/// 尺寸白名单校验(256/512/1024)。
pub fn validate_size(size: u32) -> Result<()> {
    if !SIZES.contains(&size) {
        return Err(GenError::new(
            GEN_BAD_PARAMS,
            format!("size 须为 {SIZES:?} 之一,实: {size}"),
        ));
    }
    Ok(())
}

/// 纹理槽位闭集(05 §7 maps: [albedo,normal,roughness,ao?])。
pub const MAP_KINDS: [&str; 4] = ["albedo", "normal", "roughness", "ao"];

/// 按槽位渲染确定性 PNG(同 (prompt,map,size,seed) → 同字节)。
pub fn render_map(prompt: &str, map: &str, size: u32, seed: u64) -> Result<Vec<u8>> {
    validate_size(size)?;
    render_map_sized(prompt, map, size, size, seed)
}

/// 任意尺寸渲染(variations 跟随源图尺寸用;跳过尺寸白名单,槽位闭集仍校验)。
pub fn render_map_sized(prompt: &str, map: &str, w: u32, h: u32, seed: u64) -> Result<Vec<u8>> {
    if !MAP_KINDS.contains(&map) {
        return Err(GenError::new(GEN_BAD_PARAMS, format!("未知纹理槽位: {map}")));
    }
    if w == 0 || h == 0 || w > 4096 || h > 4096 {
        return Err(GenError::new(GEN_BAD_PARAMS, format!("尺寸非法: {w}x{h}")));
    }
    let base = fnv1a64(prompt.as_bytes()) ^ seed;
    let mut px = vec![0u8; (w * h * 3) as usize];
    for y in 0..h {
        for x in 0..w {
            let i = ((y * w + x) * 3) as usize;
            let (r, g, b) = pixel(map, x, y, w.max(h), base);
            px[i] = r;
            px[i + 1] = g;
            px[i + 2] = b;
        }
    }
    encode_png(&px, w, h)
}

/// variations 种子派生:源图字节 + prompt + strength + 序号(strength 扰动 seed 派生重生成)。
pub fn variation_seed(source_bytes: &[u8], prompt: &str, strength: f32, idx: u32) -> u64 {
    crate::hash_parts(&[
        source_bytes,
        prompt.as_bytes(),
        &strength.to_bits().to_le_bytes(),
        &idx.to_le_bytes(),
    ])
}

/// splitmix64 单步(确定性 PRNG)。
fn next_rand(state: &mut u64) -> u64 {
    *state = state.wrapping_add(0x9e37_79b9_7f4a_7c15);
    let mut z = *state;
    z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    z ^ (z >> 31)
}

/// 像素噪声 [0,1)(坐标的纯哈希函数,与遍历顺序无关)。
fn noise01(x: u32, y: u32, salt: u64) -> f64 {
    let mut st = salt ^ (u64::from(x) << 32) ^ u64::from(y);
    (next_rand(&mut st) >> 11) as f64 / (1u64 << 53) as f64
}

/// 条纹高度场 [0,1](木纹:正弦条纹 + 低频扰动 + 细噪声)。
fn stripe_field(x: u32, y: u32, size: u32, base: u64) -> f64 {
    let s = f64::from(size);
    let fx = f64::from(x) / s;
    let fy = f64::from(y) / s;
    let freq = 6.0 + (base % 7) as f64; // 6..12 条
    let wobble = noise01(x / 16, y / 64, base ^ 0x11) * 6.0;
    let grain = noise01(x, y, base ^ 0x22) * 0.25;
    (0.5 + 0.5 * (fy * freq * std::f64::consts::TAU + wobble + fx * 0.8).sin() + grain) / 1.25
}

/// 单像素着色(各槽位确定性)。
fn pixel(map: &str, x: u32, y: u32, size: u32, base: u64) -> (u8, u8, u8) {
    match map {
        // 木纹 albedo:深棕 ↔ 浅棕。
        "albedo" => {
            let v = stripe_field(x, y, size, base);
            let r = 96.0 + v * 120.0;
            let g = 60.0 + v * 82.0;
            let b = 32.0 + v * 48.0;
            (r as u8, g as u8, b as u8)
        }
        // 法线:高度场梯度 → 切线空间法线色(蓝底)。
        "normal" => {
            let h0 = stripe_field(x, y, size, base);
            let hx = stripe_field(x.wrapping_add(1), y, size, base);
            let hy = stripe_field(x, y.wrapping_add(1), size, base);
            let nx = (hx - h0) * 3.0;
            let ny = (hy - h0) * 3.0;
            let inv = 1.0 / (1.0 + nx * nx + ny * ny).sqrt();
            (
                ((-nx * inv) * 0.5 * 255.0 + 128.0) as u8,
                ((-ny * inv) * 0.5 * 255.0 + 128.0) as u8,
                (inv * 0.5 * 255.0 + 128.0) as u8,
            )
        }
        // 粗糙度:条纹驱动的灰度。
        "roughness" => {
            let v = 120.0 + stripe_field(x, y, size, base) * 110.0;
            let g = v as u8;
            (g, g, g)
        }
        // AO:低频灰度(整体偏亮,缝隙略暗)。
        _ => {
            let v = 200.0 + stripe_field(x, y, size, base ^ 0x77) * 50.0;
            let g = v.min(255.0) as u8;
            (g, g, g)
        }
    }
}

/// RGB8 → PNG 字节(image crate 编码器;同输入字节 → 同输出字节)。
fn encode_png(px: &[u8], w: u32, h: u32) -> Result<Vec<u8>> {
    use image::ImageEncoder;
    let mut buf = Vec::new();
    image::codecs::png::PngEncoder::new(&mut buf)
        .write_image(px, w, h, image::ExtendedColorType::Rgb8)
        .map_err(|e| GenError::new(crate::GEN_BACKEND_ERROR, format!("PNG 编码失败: {e}")))?;
    Ok(buf)
}

/// RGBA8 → PNG 字节(公开:agentd 把 viewport_frame 的 rgba8 回读帧转 PNG 回注视觉模型)。
pub fn encode_png_rgba8(px: &[u8], w: u32, h: u32) -> Result<Vec<u8>> {
    use image::ImageEncoder;
    if px.len() != (w as usize) * (h as usize) * 4 {
        return Err(GenError::new(
            crate::GEN_BAD_PARAMS,
            format!("rgba8 字节数不符: {} ≠ {w}x{h}x4", px.len()),
        ));
    }
    let mut buf = Vec::new();
    image::codecs::png::PngEncoder::new(&mut buf)
        .write_image(px, w, h, image::ExtendedColorType::Rgba8)
        .map_err(|e| GenError::new(crate::GEN_BACKEND_ERROR, format!("PNG 编码失败: {e}")))?;
    Ok(buf)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deterministic_same_seed_same_prompt() {
        let a = render_map("wood grain 木纹", "albedo", 256, 42).unwrap();
        let b = render_map("wood grain 木纹", "albedo", 256, 42).unwrap();
        assert_eq!(a, b, "同 seed 同 prompt 须字节一致");
        assert_eq!(&a[..8], b"\x89PNG\r\n\x1a\n", "须为 PNG 魔数");
    }

    #[test]
    fn different_seed_differs() {
        let a = render_map("wood", "albedo", 256, 1).unwrap();
        let b = render_map("wood", "albedo", 256, 2).unwrap();
        assert_ne!(a, b, "不同 seed 须不同字节");
    }

    #[test]
    fn different_prompt_differs() {
        let a = render_map("oak", "albedo", 256, 42).unwrap();
        let b = render_map("walnut", "albedo", 256, 42).unwrap();
        assert_ne!(a, b, "不同 prompt 须不同字节");
    }

    #[test]
    fn map_kinds_render_and_decode() {
        for m in MAP_KINDS {
            let png = render_map("p", m, 256, 7).unwrap();
            let img = image::load_from_memory(&png).expect("产物须可解码");
            assert_eq!((img.width(), img.height()), (256, 256));
        }
    }

    #[test]
    fn size_whitelist_enforced() {
        assert!(render_map("p", "albedo", 300, 1).is_err());
        assert!(render_map("p", "albedo", 1024, 1).is_ok());
    }

    #[test]
    fn mock_generate_n_candidates_sequential_seeds() {
        let m = LocalMock;
        let req = GenRequest {
            prompt: "p".into(),
            negative_prompt: None,
            size: 256,
            seed: 42,
            n: 4,
        };
        let cfg = GenConfig::default();
        let ks = Keystore::load_from(std::path::Path::new("no-such-ks.json"));
        let out = m.generate(&req, &cfg, &ks).unwrap();
        assert_eq!(out.len(), 4);
        for (i, c) in out.iter().enumerate() {
            assert_eq!(c.seed, 42 + i as u64);
        }
        // 越界 n 如实拒绝。
        let bad = GenRequest { n: 5, ..req.clone() };
        assert_eq!(m.generate(&bad, &cfg, &ks).unwrap_err().code, GEN_BAD_PARAMS);
    }
}
