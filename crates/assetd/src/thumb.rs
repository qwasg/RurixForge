//! 缩略图(07 §4 / 08 §3.3):贴图 = 源字节 base64 原图直出(前端 CSS 缩放;
//! 离线缩放缓存 .forge/cache/thumbs 留待性能波);网格/其他类型 = NO_THUMBNAIL
//! (网格三视角离屏渲染 = RD-F2-002,前端图标占位 + tooltip 如实标注,不伪造)。

use base64::Engine as _;

use crate::meta::MetaDoc;
use crate::project::ForgeProject;
use crate::{meta_path_for, normalize_rel, AssetError, Result};

/// 原图直出上限(8 MiB;超过拒绝,防 stdio/HTTP 链路单行过大)。
const MAX_THUMB_BYTES: u64 = 8 * 1024 * 1024;

/// asset_thumbnail:贴图资产 → (data URL, 源字节数)。非贴图/缺 .meta/超上限 = 结构化错误。
pub fn thumbnail_data_url(project: &ForgeProject, rel_path: &str) -> Result<(String, u64)> {
    let rel = normalize_rel(rel_path)?;
    let meta_path = meta_path_for(&project.content_root(), &rel);
    if !meta_path.is_file() {
        return Err(AssetError::new("NO_META", format!("缺 .meta: {rel}")));
    }
    let meta = MetaDoc::load(&meta_path)?;
    if meta.atype != "texture" {
        return Err(AssetError::new(
            "NO_THUMBNAIL",
            format!("类型 {} 无缩略图(网格离屏渲染 = RD-F2-002)", meta.atype),
        ));
    }
    let abs = project.content_root().join(&rel);
    let bytes_len = std::fs::metadata(&abs)?.len();
    if bytes_len > MAX_THUMB_BYTES {
        return Err(AssetError::new(
            "TOO_LARGE",
            format!("源文件 {bytes_len} B 超缩略图上限 {MAX_THUMB_BYTES} B: {rel}"),
        ));
    }
    let bytes = std::fs::read(&abs)?;
    let lower = rel.to_ascii_lowercase();
    let mime = if lower.ends_with(".png") {
        "image/png"
    } else if lower.ends_with(".jpg") || lower.ends_with(".jpeg") {
        "image/jpeg"
    } else {
        return Err(AssetError::new(
            "NO_THUMBNAIL",
            format!("贴图扩展名不支持缩略图: {rel}"),
        ));
    };
    let b64 = base64::engine::general_purpose::STANDARD.encode(&bytes);
    Ok((format!("data:{mime};base64,{b64}"), bytes_len))
}
