//! D-045:Text 组件 → CPU 光栅化贴图(两个渲染后端共用,不加新着色器)。
//!
//! Text 与 Sprite 走同一条 2D 绘制腿:这里把组件属性排版、光栅化成一张 RGBA 贴图,包成
//! `SpriteRenderInfo`(整图、居中锚)交给精灵路径——rurix 视口、Godot、点选、裁剪自然生效。
//! 字体 = 项目内字体资产(font GUID → .ttf/.otf/.ttc);缺失或解析失败不画,并登记问题
//! (`drain_issues` 由 viewport.frame 转成宿主事件 TEXT_FONT_MISSING,I-5:不偷换系统字体)。
//!
//! 贴图按「属性 + 字体文件身份 + 资产代次」缓存;`TexGpu` 沿用进程级 'static 纪律,
//! 改字会产生新贴图(旧的不回收),所以编辑器改字只在提交时发生,不逐键重画。

use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};

use serde_json::Value;

use super::assets::{content_guid_map_cached, TexGpu, ASSET_GENERATION};
use super::sprite::SpriteRenderInfo;

/// 单张文字贴图边长上限(像素);超出按上限裁,防止误填超大字号把内存吃光。
const MAX_TEX_SIDE: u32 = 4096;
/// 字号上限。
const MAX_SIZE_PX: f32 = 512.0;

/// 排版参数(从组件属性读出,带注册表同款缺省)。
#[derive(Debug, Clone, PartialEq)]
pub struct TextStyle {
    pub text: String,
    pub font: String,
    pub size: f32,
    pub color: [f32; 4],
    pub align: Align,
    pub valign: VAlign,
    pub box_size: [f32; 2],
    pub wrap: bool,
    pub line_height: f32,
    pub letter_spacing: f32,
    pub outline_color: [f32; 4],
    pub outline_width: f32,
    pub shadow_color: [f32; 4],
    pub shadow_offset: [f32; 2],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Align {
    Left,
    Center,
    Right,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VAlign {
    Top,
    Middle,
    Bottom,
}

fn num(c: &forge_scene::Component, k: &str, d: f64) -> f32 {
    c.props.get(k).and_then(Value::as_f64).filter(|v| v.is_finite()).unwrap_or(d) as f32
}

fn arr<const N: usize>(c: &forge_scene::Component, k: &str, d: [f32; N]) -> [f32; N] {
    let a = c.props.get(k).and_then(Value::as_array);
    std::array::from_fn(|i| {
        a.and_then(|a| a.get(i))
            .and_then(Value::as_f64)
            .filter(|v| v.is_finite())
            .map(|v| v as f32)
            .unwrap_or(d[i])
    })
}

impl TextStyle {
    pub fn from_component(c: &forge_scene::Component) -> Self {
        let s = |k: &str| c.props.get(k).and_then(Value::as_str).unwrap_or("").to_string();
        TextStyle {
            text: s("text"),
            font: s("font"),
            size: num(c, "size", 32.0).clamp(1.0, MAX_SIZE_PX),
            color: arr(c, "color", [1.0; 4]),
            align: match c.props.get("align").and_then(Value::as_str) {
                Some("center") => Align::Center,
                Some("right") => Align::Right,
                _ => Align::Left,
            },
            valign: match c.props.get("verticalAlign").and_then(Value::as_str) {
                Some("middle") => VAlign::Middle,
                Some("bottom") => VAlign::Bottom,
                _ => VAlign::Top,
            },
            box_size: arr(c, "boxSize", [0.0; 2]).map(|v| v.max(0.0)),
            wrap: c.props.get("wrap").and_then(Value::as_bool).unwrap_or(false),
            line_height: num(c, "lineHeight", 1.2).clamp(0.5, 4.0),
            letter_spacing: num(c, "letterSpacing", 0.0),
            outline_color: arr(c, "outlineColor", [0.0, 0.0, 0.0, 1.0]),
            outline_width: num(c, "outlineWidth", 0.0).clamp(0.0, 32.0),
            shadow_color: arr(c, "shadowColor", [0.0; 4]),
            shadow_offset: arr(c, "shadowOffset", [0.0; 2]).map(|v| v.clamp(-64.0, 64.0)),
        }
    }

    /// 内容指纹(贴图缓存键与渲染签名用)。
    pub fn fingerprint(&self, font_identity: u64) -> u64 {
        let mut b = Vec::with_capacity(self.text.len() + 128);
        b.extend_from_slice(self.text.as_bytes());
        b.push(0xff);
        b.extend_from_slice(self.font.as_bytes());
        b.extend_from_slice(&font_identity.to_le_bytes());
        for v in [self.size, self.line_height, self.letter_spacing, self.outline_width]
            .into_iter()
            .chain(self.color)
            .chain(self.box_size)
            .chain(self.outline_color)
            .chain(self.shadow_color)
            .chain(self.shadow_offset)
        {
            b.extend_from_slice(&v.to_bits().to_le_bytes());
        }
        b.push(self.align as u8);
        b.push(self.valign as u8);
        b.push(u8::from(self.wrap));
        crate::meshres::fnv1a64(&b)
    }
}

// ---------------- 字体 ----------------

struct LoadedFont {
    font: fontdue::Font,
    /// 路径 + 文件长度 + 修改时间的哈希(换字体文件即换身份)。
    identity: u64,
}

fn font_identity(path: &std::path::Path) -> Option<u64> {
    let meta = std::fs::metadata(path).ok()?;
    let mtime = meta
        .modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let mut b = path.to_string_lossy().as_bytes().to_vec();
    b.extend_from_slice(&meta.len().to_le_bytes());
    b.extend_from_slice(&mtime.to_le_bytes());
    Some(crate::meshres::fnv1a64(&b))
}

/// 字体文件 → fontdue 字体(.ttc 取集合第 0 个)。
pub fn parse_font(bytes: &[u8]) -> Result<fontdue::Font, String> {
    fontdue::Font::from_bytes(
        bytes,
        fontdue::FontSettings { collection_index: 0, scale: 64.0, ..Default::default() },
    )
    .map_err(|e| e.to_string())
}

fn load_font(path: &std::path::Path) -> Result<Arc<LoadedFont>, String> {
    static CACHE: OnceLock<Mutex<HashMap<u64, Arc<LoadedFont>>>> = OnceLock::new();
    let identity = font_identity(path).ok_or_else(|| format!("字体文件不可读: {}", path.display()))?;
    let cache = CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    if let Some(f) = cache.lock().unwrap().get(&identity) {
        return Ok(f.clone());
    }
    let bytes = std::fs::read(path).map_err(|e| format!("读字体失败 {}: {e}", path.display()))?;
    let font = parse_font(&bytes)?;
    let loaded = Arc::new(LoadedFont { font, identity });
    cache.lock().unwrap().insert(identity, loaded.clone());
    Ok(loaded)
}

// ---------------- 问题登记 ----------------

fn issues() -> &'static Mutex<Vec<(String, String)>> {
    static ISSUES: OnceLock<Mutex<Vec<(String, String)>>> = OnceLock::new();
    ISSUES.get_or_init(|| Mutex::new(Vec::new()))
}

/// 同一 (字体, 原因) 只登记一次直到被取走,免得逐帧刷屏。
fn report(font: &str, reason: String) {
    let mut v = issues().lock().unwrap();
    if !v.iter().any(|(f, r)| f == font && *r == reason) && v.len() < 64 {
        v.push((font.to_string(), reason));
    }
}

/// 取走已登记的文字问题(viewport.frame 转成宿主事件)。
pub fn drain_issues() -> Vec<(String, String)> {
    std::mem::take(&mut *issues().lock().unwrap())
}

// ---------------- 排版与光栅化 ----------------

/// 光栅化结果(RGBA8,straight alpha)。
#[derive(Debug, Clone, PartialEq)]
pub struct Raster {
    pub w: u32,
    pub h: u32,
    pub rgba: Vec<u8>,
}

fn advance(font: &fontdue::Font, ch: char, px: f32, spacing: f32) -> f32 {
    font.metrics(ch, px).advance_width + spacing
}

/// 按框宽贪心折行:优先在空白处断,单词过长(或 CJK 无空格)时按字断。
fn layout_lines(font: &fontdue::Font, st: &TextStyle) -> Vec<String> {
    let max_w = if st.wrap && st.box_size[0] > 0.0 { Some(st.box_size[0]) } else { None };
    let mut lines = Vec::new();
    for para in st.text.split('\n') {
        let Some(max_w) = max_w else {
            lines.push(para.to_string());
            continue;
        };
        let mut line = String::new();
        let mut width = 0.0f32;
        let mut last_space: Option<usize> = None;
        for ch in para.chars() {
            let a = advance(&font, ch, st.size, st.letter_spacing);
            if width + a > max_w && !line.is_empty() {
                if let Some(cut) = last_space.filter(|&i| i > 0) {
                    let rest: String = line[cut..].trim_start().to_string();
                    lines.push(line[..cut].trim_end().to_string());
                    line = rest;
                } else {
                    lines.push(std::mem::take(&mut line));
                }
                width = line.chars().map(|c| advance(font, c, st.size, st.letter_spacing)).sum();
                last_space = None;
            }
            if ch.is_whitespace() {
                last_space = Some(line.len());
            }
            line.push(ch);
            width += a;
        }
        lines.push(line);
    }
    lines
}

/// 覆盖度掩码按半径膨胀(描边用;圆盘结构元)。
fn dilate(mask: &[f32], w: usize, h: usize, r: f32) -> Vec<f32> {
    let ri = r.ceil() as i32;
    let mut out = vec![0f32; mask.len()];
    let offsets: Vec<(i32, i32, f32)> = (-ri..=ri)
        .flat_map(|dy| (-ri..=ri).map(move |dx| (dx, dy)))
        .filter_map(|(dx, dy)| {
            let d = ((dx * dx + dy * dy) as f32).sqrt();
            (d <= r + 0.5).then(|| (dx, dy, (r + 0.5 - d).clamp(0.0, 1.0)))
        })
        .collect();
    for y in 0..h as i32 {
        for x in 0..w as i32 {
            let mut m = 0f32;
            for &(dx, dy, wgt) in &offsets {
                let (sx, sy) = (x + dx, y + dy);
                if sx >= 0 && sy >= 0 && (sx as usize) < w && (sy as usize) < h {
                    m = m.max(mask[sy as usize * w + sx as usize] * wgt);
                }
            }
            out[y as usize * w + x as usize] = m;
        }
    }
    out
}

/// 「源 over 目标」(straight alpha)。
fn over(dst: &mut [f32; 4], color: [f32; 4], coverage: f32) {
    let sa = (color[3] * coverage).clamp(0.0, 1.0);
    if sa <= 0.0 {
        return;
    }
    let da = dst[3];
    let oa = sa + da * (1.0 - sa);
    for i in 0..3 {
        dst[i] = (color[i] * sa + dst[i] * da * (1.0 - sa)) / oa;
    }
    dst[3] = oa;
}

/// 排版 + 光栅化(纯函数;贴图左上为原点)。
pub fn rasterize(font: &fontdue::Font, st: &TextStyle) -> Raster {
    let lines = layout_lines(font, st);
    let lm = font.horizontal_line_metrics(st.size);
    let ascent = lm.map(|m| m.ascent).unwrap_or(st.size * 0.8);
    let line_h = st.size * st.line_height;
    let widths: Vec<f32> = lines
        .iter()
        .map(|l| l.chars().map(|c| advance(font, c, st.size, st.letter_spacing)).sum::<f32>() - if l.is_empty() { 0.0 } else { st.letter_spacing })
        .collect();
    let content_w = widths.iter().cloned().fold(0.0f32, f32::max);
    let content_h = line_h * lines.len().max(1) as f32;
    let pad = (st.outline_width + st.shadow_offset[0].abs().max(st.shadow_offset[1].abs())).ceil() + 1.0;
    let w = if st.box_size[0] > 0.0 { st.box_size[0] } else { content_w + 2.0 * pad };
    let h = if st.box_size[1] > 0.0 { st.box_size[1] } else { content_h + 2.0 * pad };
    let (w, h) = ((w.ceil() as u32).clamp(1, MAX_TEX_SIDE), (h.ceil() as u32).clamp(1, MAX_TEX_SIDE));
    let inner_x0 = if st.box_size[0] > 0.0 { 0.0 } else { pad };
    let inner_w = w as f32 - 2.0 * inner_x0;
    let top = match st.valign {
        _ if st.box_size[1] <= 0.0 => pad,
        VAlign::Top => 0.0,
        VAlign::Middle => (h as f32 - content_h) / 2.0,
        VAlign::Bottom => h as f32 - content_h,
    };
    let (wu, hu) = (w as usize, h as usize);
    let mut fill = vec![0f32; wu * hu];
    for (li, line) in lines.iter().enumerate() {
        let x0 = inner_x0
            + match st.align {
                Align::Left => 0.0,
                Align::Center => (inner_w - widths[li]) / 2.0,
                Align::Right => inner_w - widths[li],
            };
        // 行内居中:行高大于字号时把多出的空间均分到上下。
        let baseline = top + li as f32 * line_h + (line_h - st.size) / 2.0 + ascent;
        let mut pen = x0;
        for ch in line.chars() {
            let (m, bitmap) = font.rasterize(ch, st.size);
            let gx = (pen + m.xmin as f32).round() as i32;
            let gy = (baseline - m.height as f32 - m.ymin as f32).round() as i32;
            for by in 0..m.height {
                for bx in 0..m.width {
                    let (x, y) = (gx + bx as i32, gy + by as i32);
                    if x >= 0 && y >= 0 && (x as usize) < wu && (y as usize) < hu {
                        let i = y as usize * wu + x as usize;
                        fill[i] = fill[i].max(bitmap[by * m.width + bx] as f32 / 255.0);
                    }
                }
            }
            pen += m.advance_width + st.letter_spacing;
        }
    }
    let body = if st.outline_width > 0.0 { dilate(&fill, wu, hu, st.outline_width) } else { fill.clone() };
    let (sdx, sdy) = (st.shadow_offset[0].round() as i32, st.shadow_offset[1].round() as i32);
    let mut px = vec![[0f32; 4]; wu * hu];
    for y in 0..hu {
        for x in 0..wu {
            let i = y * wu + x;
            if st.shadow_color[3] > 0.0 {
                let (sx, sy) = (x as i32 - sdx, y as i32 - sdy);
                if sx >= 0 && sy >= 0 && (sx as usize) < wu && (sy as usize) < hu {
                    over(&mut px[i], st.shadow_color, body[sy as usize * wu + sx as usize]);
                }
            }
            if st.outline_width > 0.0 {
                over(&mut px[i], st.outline_color, body[i]);
            }
            over(&mut px[i], st.color, fill[i]);
        }
    }
    let mut rgba = Vec::with_capacity(wu * hu * 4);
    for p in px {
        for v in p {
            rgba.push((v.clamp(0.0, 1.0) * 255.0 + 0.5) as u8);
        }
    }
    Raster { w, h, rgba }
}

/// Text 组件 → 精灵渲染解析(整图、居中锚)。字体缺失/坏 → None 并登记问题。
pub(crate) fn resolve_text_render(c: &forge_scene::Component, project_root: &std::path::Path) -> Option<SpriteRenderInfo> {
    let tex = text_texture(c, project_root)?;
    Some(SpriteRenderInfo {
        tex,
        uv_rect: super::sprite::FULL_UV_RECT,
        frame_px: [tex.w as f32, tex.h as f32],
        pivot: [0.5, 0.5],
    })
}

/// Text 贴图的稳定身份键(Godot 贴图缓存键 / 渲染签名用);解析不了 → None。
pub fn text_key(c: &forge_scene::Component, project_root: &std::path::Path) -> Option<String> {
    let st = TextStyle::from_component(c);
    let path = font_path(&st.font, project_root)?;
    let id = font_identity(&path)?;
    Some(format!("text:{:016x}", st.fingerprint(id)))
}

fn font_path(guid: &str, _project_root: &std::path::Path) -> Option<std::path::PathBuf> {
    if guid.trim().is_empty() {
        return None;
    }
    content_guid_map_cached().get(guid).cloned()
}

fn text_texture(c: &forge_scene::Component, project_root: &std::path::Path) -> Option<&'static TexGpu> {
    let st = TextStyle::from_component(c);
    if st.text.is_empty() {
        return None;
    }
    if st.font.trim().is_empty() {
        report("", "Text 组件未指定字体(font 为空)".into());
        return None;
    }
    let Some(path) = font_path(&st.font, project_root) else {
        report(&st.font, "字体 GUID 在 Content 中找不到".into());
        return None;
    };
    let font = match load_font(&path) {
        Ok(f) => f,
        Err(e) => {
            report(&st.font, format!("字体解析失败: {e}"));
            return None;
        }
    };
    static CACHE: OnceLock<Mutex<HashMap<(u64, u64), &'static TexGpu>>> = OnceLock::new();
    let generation = ASSET_GENERATION.load(std::sync::atomic::Ordering::Relaxed);
    let key = (st.fingerprint(font.identity), generation);
    let cache = CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    if let Some(t) = cache.lock().unwrap().get(&key) {
        return Some(*t);
    }
    let r = rasterize(&font.font, &st);
    let rgba: &'static [u8] = Box::leak(r.rgba.into_boxed_slice());
    let tex: &'static TexGpu = Box::leak(Box::new(TexGpu { w: r.w, h: r.h, rgba }));
    cache.lock().unwrap().insert(key, tex);
    Some(tex)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 测试用系统字体(Windows 自带 Arial;不在时跳过,不伪造通过)。
    fn test_font() -> Option<fontdue::Font> {
        for p in ["C:/Windows/Fonts/arial.ttf", "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf"] {
            if let Ok(b) = std::fs::read(p) {
                return parse_font(&b).ok();
            }
        }
        None
    }

    fn style(text: &str) -> TextStyle {
        let c = forge_scene::Component::new("Text", serde_json::json!({ "text": text, "size": 32.0 }));
        TextStyle::from_component(&c)
    }

    #[test]
    fn style_reads_defaults_and_clamps() {
        let c = forge_scene::Component::new("Text", serde_json::json!({ "size": 99999.0, "align": "center" }));
        let st = TextStyle::from_component(&c);
        assert_eq!(st.size, MAX_SIZE_PX);
        assert_eq!(st.align, Align::Center);
        assert_eq!(st.color, [1.0; 4]);
        assert_eq!(st.valign, VAlign::Top);
    }

    #[test]
    fn fingerprint_changes_with_text_and_color() {
        let a = style("Start");
        let mut b = a.clone();
        b.text = "Quit".into();
        assert_ne!(a.fingerprint(1), b.fingerprint(1));
        let mut c = a.clone();
        c.color[0] = 0.5;
        assert_ne!(a.fingerprint(1), c.fingerprint(1));
        assert_eq!(a.fingerprint(1), a.clone().fingerprint(1));
    }

    #[test]
    fn rasterize_is_deterministic_and_has_ink() {
        let Some(font) = test_font() else { return };
        let st = style("Start Game");
        let a = rasterize(&font, &st);
        let b = rasterize(&font, &st);
        assert_eq!(a, b);
        assert!(a.w > 100 && a.h >= 32, "{}x{}", a.w, a.h);
        assert!(a.rgba.chunks_exact(4).any(|p| p[3] > 200), "须有不透明字形像素");
        assert!(a.rgba.chunks_exact(4).any(|p| p[3] == 0), "须有透明背景");
    }

    #[test]
    fn box_size_fixes_canvas_and_wraps() {
        let Some(font) = test_font() else { return };
        let mut st = style("one two three four five six");
        st.box_size = [120.0, 200.0];
        st.wrap = true;
        let r = rasterize(&font, &st);
        assert_eq!((r.w, r.h), (120, 200));
        assert!(layout_lines(&font, &st).len() >= 3);
    }

    #[test]
    fn outline_and_shadow_add_coverage() {
        let Some(font) = test_font() else { return };
        let mut plain = style("A");
        plain.box_size = [64.0, 64.0];
        let base = rasterize(&font, &plain);
        let mut fancy = plain.clone();
        fancy.outline_width = 3.0;
        fancy.shadow_color = [0.0, 0.0, 0.0, 1.0];
        fancy.shadow_offset = [4.0, 4.0];
        let r = rasterize(&font, &fancy);
        let ink = |r: &Raster| r.rgba.chunks_exact(4).filter(|p| p[3] > 128).count();
        assert!(ink(&r) > ink(&base));
    }
}
