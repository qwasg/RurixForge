//! D-045:元素清单(`design_layout`)——设计稿拆成「原子元素」的唯一事实源。
//!
//! 复刻是否「原子级」由这份清单决定:每个按钮、图标、面板、文字、装饰各占一条,带像素 bbox、
//! 叠放层级与素材来源。模型写、服务端校验后落 `layout.json`;素材生产、场景编译、验收打分
//! 都按它走,不各自猜。校验失败把原因原样回给模型改(不静默修正)。

use serde::{Deserialize, Serialize};
use serde_json::Value;

pub const MAX_ELEMENTS: usize = 150;
const MAX_ID_CHARS: usize = 48;
const MAX_TEXT_CHARS: usize = 400;

/// 元素种类(映射场景 Category:按钮类 → interaction,角色 → role,其余 → map)。
pub const KINDS: [&str; 9] = ["background", "panel", "button", "icon", "image", "text", "decor", "character", "bar"];
/// 素材来源。
pub const SOURCES: [&str; 4] = ["cleanplate", "crop", "regen", "text"];

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct TextSpec {
    pub content: String,
    /// 字号(定稿像素)。
    pub size: f64,
    pub color: [f64; 4],
    #[serde(default = "default_align")]
    pub align: String,
    #[serde(default = "default_valign")]
    pub vertical_align: String,
    #[serde(default)]
    pub outline_color: Option<[f64; 4]>,
    #[serde(default)]
    pub outline_width: Option<f64>,
    #[serde(default)]
    pub shadow_color: Option<[f64; 4]>,
    #[serde(default)]
    pub shadow_offset: Option<[f64; 2]>,
    #[serde(default)]
    pub letter_spacing: Option<f64>,
    #[serde(default)]
    pub line_height: Option<f64>,
    /// 元素级字体覆盖(缺省用清单顶层 font)。
    #[serde(default)]
    pub font: Option<String>,
}

fn default_align() -> String {
    "center".into()
}
fn default_valign() -> String {
    "middle".into()
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Element {
    pub id: String,
    pub kind: String,
    /// [x, y, w, h](定稿像素,左上原点)。
    pub bbox: [u32; 4],
    pub z: i64,
    pub source: String,
    #[serde(default)]
    pub text: Option<TextSpec>,
    /// regen 元素的单体重绘描述(不写则按 kind + 定稿局部参考出图)。
    #[serde(default)]
    pub regen_prompt: Option<String>,
    /// 一句话说明(给用户看的元素表与验收报告用)。
    #[serde(default)]
    pub note: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Layout {
    pub canvas: Canvas,
    /// 文字元素缺省字体(字体资产 GUID)。
    #[serde(default)]
    pub font: Option<String>,
    pub elements: Vec<Element>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub struct Canvas {
    pub width: u32,
    pub height: u32,
}

impl Element {
    /// 场景 Category 映射。
    pub fn category(&self) -> &'static str {
        match self.kind.as_str() {
            "button" => "interaction",
            "character" => "role",
            _ => "map",
        }
    }

    pub fn font<'a>(&'a self, layout: &'a Layout) -> Option<&'a str> {
        self.text
            .as_ref()
            .and_then(|t| t.font.as_deref())
            .or(layout.font.as_deref())
            .filter(|s| !s.trim().is_empty())
    }
}

fn valid_id(id: &str) -> bool {
    !id.is_empty()
        && id.chars().count() <= MAX_ID_CHARS
        && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
}

fn color_ok(c: &[f64; 4]) -> bool {
    c.iter().all(|v| v.is_finite() && (0.0..=1.0).contains(v))
}

/// 校验并规整清单。`canvas` 须与定稿尺寸一致;`font_exists` 判字体 GUID 是否在项目里。
pub fn validate(raw: &Value, approved: (u32, u32), font_exists: &dyn Fn(&str) -> bool) -> Result<Layout, String> {
    let layout: Layout = serde_json::from_value(raw.clone()).map_err(|e| format!("清单结构不合法: {e}"))?;
    if (layout.canvas.width, layout.canvas.height) != approved {
        return Err(format!(
            "canvas 须等于定稿尺寸 {}x{},实: {}x{}",
            approved.0, approved.1, layout.canvas.width, layout.canvas.height
        ));
    }
    if layout.elements.is_empty() {
        return Err("elements 不可空".into());
    }
    if layout.elements.len() > MAX_ELEMENTS {
        return Err(format!("元素过多: {} > {MAX_ELEMENTS}(合并纯装饰性的细碎元素)", layout.elements.len()));
    }
    let (cw, ch) = approved;
    let mut seen = std::collections::HashSet::new();
    let mut backgrounds = 0;
    for (i, e) in layout.elements.iter().enumerate() {
        let at = format!("elements[{i}]({})", e.id);
        if !valid_id(&e.id) {
            return Err(format!("{at}: id 须为 1..={MAX_ID_CHARS} 个 [A-Za-z0-9_-]"));
        }
        if !seen.insert(e.id.as_str()) {
            return Err(format!("{at}: id 重复"));
        }
        if !KINDS.contains(&e.kind.as_str()) {
            return Err(format!("{at}: kind 须为 {} 之一", KINDS.join("|")));
        }
        if !SOURCES.contains(&e.source.as_str()) {
            return Err(format!("{at}: source 须为 {} 之一", SOURCES.join("|")));
        }
        let [x, y, w, h] = e.bbox;
        if w == 0 || h == 0 || x.saturating_add(w) > cw || y.saturating_add(h) > ch {
            return Err(format!("{at}: bbox {:?} 须为正面积且完全落在画布 {cw}x{ch} 内", e.bbox));
        }
        if e.kind == "background" {
            backgrounds += 1;
            if e.source != "cleanplate" && e.source != "crop" {
                return Err(format!("{at}: 背景的 source 须为 cleanplate 或 crop"));
            }
            if e.bbox != [0, 0, cw, ch] {
                return Err(format!("{at}: 背景须铺满画布 [0,0,{cw},{ch}]"));
            }
        } else if e.source == "cleanplate" {
            return Err(format!("{at}: cleanplate 只用于 kind=background"));
        }
        let is_text = e.kind == "text" || e.source == "text";
        if is_text {
            if e.kind != "text" || e.source != "text" {
                return Err(format!("{at}: 文字元素须 kind=text 且 source=text"));
            }
            let t = e.text.as_ref().ok_or_else(|| format!("{at}: 文字元素缺 text"))?;
            let n = t.content.chars().count();
            if n == 0 || n > MAX_TEXT_CHARS {
                return Err(format!("{at}: text.content 须为 1..={MAX_TEXT_CHARS} 字(逐字照抄设计稿)"));
            }
            if !(t.size.is_finite() && t.size >= 4.0 && t.size <= 512.0) {
                return Err(format!("{at}: text.size 须在 4..=512 像素"));
            }
            if !color_ok(&t.color) {
                return Err(format!("{at}: text.color 须为 0..1 的 [r,g,b,a]"));
            }
            if !matches!(t.align.as_str(), "left" | "center" | "right")
                || !matches!(t.vertical_align.as_str(), "top" | "middle" | "bottom")
            {
                return Err(format!("{at}: text.align / verticalAlign 取值非法"));
            }
            for c in [&t.outline_color, &t.shadow_color].into_iter().flatten() {
                if !color_ok(c) {
                    return Err(format!("{at}: 描边/阴影颜色须为 0..1 的 [r,g,b,a]"));
                }
            }
            let font = e.font(&layout).ok_or_else(|| {
                format!("{at}: 文字元素没有字体——清单顶层写 font(字体 GUID,先 font_list / font_import)")
            })?;
            if !font_exists(font) {
                return Err(format!("{at}: 字体 GUID {font} 不在项目 Content 中(先 font_import)"));
            }
        } else if e.text.is_some() {
            return Err(format!("{at}: 只有 kind=text 的元素可以带 text"));
        }
    }
    if backgrounds != 1 {
        return Err(format!("须恰有 1 个 kind=background 的元素(铺满画布),实: {backgrounds}"));
    }
    Ok(layout)
}

/// 画布 → 截帧尺寸的缩放(超出 1920×1080 时等比缩小;不放大)。
pub fn capture_scale(canvas: Canvas) -> f64 {
    (1920.0 / f64::from(canvas.width))
        .min(1080.0 / f64::from(canvas.height))
        .min(1.0)
}

/// 按比例缩放 bbox(四舍五入,至少 1 像素)。
pub fn scale_bbox(b: [u32; 4], s: f64) -> [u32; 4] {
    let f = |v: u32| (f64::from(v) * s).round() as u32;
    [f(b[0]), f(b[1]), f(b[2]).max(1), f(b[3]).max(1)]
}

/// 元素中心像素 → 世界坐标(正交相机居中,y 向上;ppu 像素每单位)。
pub fn pixel_to_world(bbox: [u32; 4], canvas: (u32, u32), ppu: f64) -> [f64; 2] {
    let cx = f64::from(bbox[0]) + f64::from(bbox[2]) / 2.0;
    let cy = f64::from(bbox[1]) + f64::from(bbox[3]) / 2.0;
    [
        (cx - f64::from(canvas.0) / 2.0) / ppu,
        (f64::from(canvas.1) / 2.0 - cy) / ppu,
    ]
}

/// 元素在定稿上的叠框预览(层级色:背景不画,文字蓝,按钮橙,其余绿)。
pub fn overlay(mockup: &super::compare::Img, layout: &Layout) -> super::compare::Img {
    let mut out = mockup.clone();
    for e in &layout.elements {
        if e.kind == "background" {
            continue;
        }
        let color: [u8; 3] = match e.kind.as_str() {
            "text" => [64, 160, 255],
            "button" => [255, 150, 40],
            _ => [80, 220, 120],
        };
        let [x, y, w, h] = e.bbox;
        for t in 0..2u32 {
            for xx in x..x + w {
                for yy in [y + t, (y + h).saturating_sub(1 + t)] {
                    put(&mut out, xx, yy, color);
                }
            }
            for yy in y..y + h {
                for xx in [x + t, (x + w).saturating_sub(1 + t)] {
                    put(&mut out, xx, yy, color);
                }
            }
        }
    }
    out
}

fn put(img: &mut super::compare::Img, x: u32, y: u32, c: [u8; 3]) {
    if x < img.w && y < img.h {
        let i = ((y * img.w + x) * 4) as usize;
        img.px[i..i + 3].copy_from_slice(&c);
        img.px[i + 3] = 255;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn base() -> Value {
        json!({
            "canvas": {"width": 200, "height": 100},
            "font": "font-guid",
            "elements": [
                {"id": "bg", "kind": "background", "bbox": [0, 0, 200, 100], "z": 0, "source": "cleanplate"},
                {"id": "btn_start", "kind": "button", "bbox": [50, 40, 100, 30], "z": 10, "source": "crop"},
                {"id": "lbl_start", "kind": "text", "bbox": [60, 45, 80, 20], "z": 20, "source": "text",
                 "text": {"content": "开始游戏", "size": 18, "color": [1, 1, 1, 1]}}
            ]
        })
    }

    fn fonts(g: &str) -> bool {
        g == "font-guid"
    }

    #[test]
    fn accepts_valid_layout_and_defaults() {
        let l = validate(&base(), (200, 100), &fonts).unwrap();
        assert_eq!(l.elements.len(), 3);
        let t = l.elements[2].text.as_ref().unwrap();
        assert_eq!(t.align, "center");
        assert_eq!(l.elements[1].category(), "interaction");
        assert_eq!(l.elements[2].font(&l), Some("font-guid"));
    }

    #[test]
    fn rejects_bad_layouts() {
        let check = |f: &dyn Fn(&mut Value), want: &str| {
            let mut v = base();
            f(&mut v);
            let e = validate(&v, (200, 100), &fonts).unwrap_err();
            assert!(e.contains(want), "{e}");
        };
        check(&|v| v["canvas"]["width"] = json!(300), "canvas");
        check(&|v| v["elements"][1]["id"] = json!("bg"), "重复");
        check(&|v| v["elements"][1]["bbox"] = json!([150, 40, 100, 30]), "画布");
        check(&|v| v["elements"][1]["source"] = json!("magic"), "source");
        check(&|v| v["elements"][2]["text"]["content"] = json!(""), "content");
        check(&|v| v["font"] = json!("missing"), "font_import");
        check(&|v| v["elements"][0]["bbox"] = json!([0, 0, 100, 100]), "铺满");
        check(&|v| { v["elements"].as_array_mut().unwrap().remove(0); }, "background");
        check(&|v| v["elements"][1]["source"] = json!("cleanplate"), "cleanplate");
        check(&|v| v["elements"][2]["source"] = json!("crop"), "文字元素");
    }

    #[test]
    fn geometry_helpers() {
        let c = Canvas { width: 3840, height: 2160 };
        assert!((capture_scale(c) - 0.5).abs() < 1e-9);
        assert_eq!(capture_scale(Canvas { width: 800, height: 600 }), 1.0);
        assert_eq!(scale_bbox([10, 20, 30, 1], 0.5), [5, 10, 15, 1]);
        let w = pixel_to_world([0, 0, 200, 100], (200, 100), 100.0);
        assert_eq!(w, [0.0, 0.0]);
        let tl = pixel_to_world([0, 0, 20, 20], (200, 100), 100.0);
        assert_eq!(tl, [-0.9, 0.4]);
    }
}
