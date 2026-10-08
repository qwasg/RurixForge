//! 字体资产(D-045,08 E-08-004):.ttf / .otf / .ttc 入库为 `font` 类型,供 Text 组件按 GUID 引用。
//!
//! 只做三件事:校验字体能解析(坏文件导入时就拒,不等渲染时才发现)、读族名/字形数给 agent 选字体、
//! 列出系统字体目录供导入。字体文件有授权约束——系统字体复制进项目只用于本地制作,
//! 发行前须确认授权(导入时写进 provenance,如实提醒,不替用户做判断)。

use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::{AssetError, Result};

/// 字体扩展名白名单。
pub const FONT_EXTS: [&str; 3] = ["ttf", "otf", "ttc"];

/// 字体文件摘要。
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct FontInfo {
    /// 族名(name 表 family;读不到 → 文件名)。
    pub family: String,
    /// 子族(Regular / Bold …)。
    pub subfamily: String,
    pub glyph_count: u16,
    /// .ttc 集合内字体数(单字体 = 1;Text 取第 0 个)。
    pub faces: u32,
    /// 是否含常用汉字(「中」U+4E2D)——给中文界面选字体用。
    pub has_cjk: bool,
}

pub fn is_font_path(p: &Path) -> bool {
    p.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| FONT_EXTS.contains(&e.to_ascii_lowercase().as_str()))
}

fn name_of(face: &ttf_parser::Face, id: u16) -> Option<String> {
    face.names()
        .into_iter()
        .filter(|n| n.name_id == id && n.is_unicode())
        .find_map(|n| n.to_string())
}

/// 解析字体字节(.ttc 取第 0 个面)。
pub fn probe_bytes(bytes: &[u8], fallback_name: &str) -> Result<FontInfo> {
    let faces = ttf_parser::fonts_in_collection(bytes).unwrap_or(1);
    let face = ttf_parser::Face::parse(bytes, 0)
        .map_err(|e| AssetError::new("FONT_INVALID", format!("字体解析失败({fallback_name}): {e}")))?;
    Ok(FontInfo {
        family: name_of(&face, ttf_parser::name_id::FAMILY).unwrap_or_else(|| fallback_name.to_string()),
        subfamily: name_of(&face, ttf_parser::name_id::SUBFAMILY).unwrap_or_default(),
        glyph_count: face.number_of_glyphs(),
        faces,
        has_cjk: face.glyph_index('中').is_some(),
    })
}

pub fn probe(path: &Path) -> Result<FontInfo> {
    let bytes = std::fs::read(path)?;
    let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("font");
    probe_bytes(&bytes, stem)
}

/// 系统字体目录(Windows:%WINDIR%\Fonts 与用户级 LocalAppData\Microsoft\Windows\Fonts)。
pub fn system_font_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Ok(w) = std::env::var("WINDIR") {
        dirs.push(PathBuf::from(w).join("Fonts"));
    }
    if let Ok(l) = std::env::var("LOCALAPPDATA") {
        dirs.push(PathBuf::from(l).join("Microsoft").join("Windows").join("Fonts"));
    }
    for d in ["/usr/share/fonts", "/Library/Fonts", "/System/Library/Fonts"] {
        dirs.push(PathBuf::from(d));
    }
    dirs.into_iter().filter(|d| d.is_dir()).collect()
}

/// 列出目录(递归 3 层)下的字体文件;解析失败的跳过。`limit` 防系统字体过多撑爆响应。
pub fn list_fonts(dirs: &[PathBuf], limit: usize) -> Vec<(PathBuf, FontInfo)> {
    let mut out = Vec::new();
    let mut stack: Vec<(PathBuf, u32)> = dirs.iter().map(|d| (d.clone(), 0)).collect();
    while let Some((dir, depth)) = stack.pop() {
        let Ok(rd) = std::fs::read_dir(&dir) else { continue };
        let mut entries: Vec<PathBuf> = rd.flatten().map(|e| e.path()).collect();
        entries.sort();
        for p in entries {
            if out.len() >= limit {
                return out;
            }
            if p.is_dir() {
                if depth < 3 {
                    stack.push((p, depth + 1));
                }
            } else if is_font_path(&p) {
                if let Ok(info) = probe(&p) {
                    out.push((p, info));
                }
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_garbage_and_detects_extension() {
        assert!(probe_bytes(b"not a font", "x").is_err());
        assert!(is_font_path(Path::new("a/B.TTF")));
        assert!(!is_font_path(Path::new("a/b.png")));
    }

    #[test]
    fn probes_system_font_when_present() {
        let p = Path::new("C:/Windows/Fonts/arial.ttf");
        if !p.is_file() {
            return;
        }
        let info = probe(p).unwrap();
        assert!(info.family.to_lowercase().contains("arial"), "{info:?}");
        assert!(info.glyph_count > 100);
        assert_eq!(info.faces, 1);
    }
}
