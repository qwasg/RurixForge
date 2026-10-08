//! 贴图处理(08 §4.2 纹理器最小面):png/jpg 解码尺寸 + resize/格式转换(image crate)。
//! 输出 = 同目录确定性命名 `<stem>@<w>x<h>.<ext>` 新资产(幂等:同参数覆盖同路径),
//! 原资产不动;provenance.detail 记 derivedFrom + ops(派生事实,I-7)。

use std::path::Path;

use serde_json::Value;

use crate::import::import_assets;
use crate::meta::MetaDoc;
use crate::project::ForgeProject;
use crate::{meta_path_for, normalize_rel, AssetError, Result};

/// 解码贴图尺寸(png/jpg;image 读头部,不全量解码)。
/// 解码贴图并求平均 RGBA(0-1;视口无纹理采样管线的保真着色替身,解码一次缓存)。
/// 直均含背景色:精灵类图背景会渗入,先按此近似交付(F-GAME-2)。
pub fn decode_average_rgba(path: &Path) -> Result<[f32; 4]> {
    let img = image::open(path)
        .map_err(|e| AssetError::new("DECODE_ERR", format!("贴图解码失败 {}: {e}", path.display())))?;
    let img = img.to_rgba8();
    let (w, h) = img.dimensions();
    let mut acc = [0u64; 4];
    for px in img.pixels() {
        for k in 0..4 {
            acc[k] += px.0[k] as u64;
        }
    }
    let n = (w as u64 * h as u64).max(1);
    Ok([
        (acc[0] / n) as f32 / 255.0,
        (acc[1] / n) as f32 / 255.0,
        (acc[2] / n) as f32 / 255.0,
        (acc[3] / n) as f32 / 255.0,
    ])
}

pub fn decode_size(path: &Path) -> Result<(u32, u32)> {
    image::image_dimensions(path)
        .map_err(|e| AssetError::new("DECODE_ERR", format!("贴图解码失败 {}: {e}", path.display())))
}

/// 全量解码贴图为 RGBA8(视口贴图精灵管线用;F-GAME-2)。
pub fn decode_rgba(path: &Path) -> Result<(u32, u32, Vec<u8>)> {
    let img = image::open(path)
        .map_err(|e| AssetError::new("DECODE_ERR", format!("贴图解码失败 {}: {e}", path.display())))?;
    let rgba = img.into_rgba8();
    let (w, h) = rgba.dimensions();
    Ok((w, h, rgba.into_raw()))
}

/// Decode already-read image bytes without reopening a verified derived page.
pub fn decode_rgba_bytes(bytes: &[u8]) -> Result<(u32, u32, Vec<u8>)> {
    let rgba=image::load_from_memory(bytes)
        .map_err(|e|AssetError::new("DECODE_ERR",format!("贴图字节解码失败: {e}")))?
        .into_rgba8();
    let (w,h)=rgba.dimensions();
    Ok((w,h,rgba.into_raw()))
}

/// texture_process 返回。
#[derive(Debug, Clone)]
pub struct ProcessOutcome {
    pub output_rel: String,
    pub guid: String,
    pub width: u32,
    pub height: u32,
    pub bytes: u64,
}

/// texture_process:`ops` = { resize?: { width, height } | { max }, format?: "png"|"jpg" }。
/// resize 二选一:width/height 精确;max 等比缩放到最长边 ≤ max(仅缩小,不放大)。
pub fn process_texture(
    project: &ForgeProject,
    rel_path: &str,
    ops: &Value,
) -> Result<ProcessOutcome> {
    let rel = normalize_rel(rel_path)?;
    let meta_path = meta_path_for(&project.content_root(), &rel);
    if !meta_path.is_file() {
        return Err(AssetError::new("NO_META", format!("缺 .meta: {rel}")));
    }
    let meta = MetaDoc::load(&meta_path)?;
    if meta.atype != "texture" {
        return Err(AssetError::new(
            "WRONG_TYPE",
            format!("texture_process 仅接受 texture,当前: {}", meta.atype),
        ));
    }
    let src_abs = project.content_root().join(&rel);
    let img = image::open(&src_abs)
        .map_err(|e| AssetError::new("DECODE_ERR", format!("贴图解码失败 {rel}: {e}")))?;
    let (src_w, src_h) = (img.width(), img.height());

    // resize(缺省 = 原尺寸)。
    let resize = ops.get("resize").cloned().unwrap_or(Value::Null);
    let (mut w, mut h) = (src_w, src_h);
    let mut processed = img;
    if let Some(rw) = resize.get("width").and_then(Value::as_u64) {
        let rh = resize
            .get("height")
            .and_then(Value::as_u64)
            .ok_or_else(|| AssetError::new("INVALID_OPS", "resize.width 须配 height"))?;
        if rw == 0 || rh == 0 || rw > 8192 || rh > 8192 {
            return Err(AssetError::new("INVALID_OPS", format!("resize 尺寸越界: {rw}x{rh}")));
        }
        w = rw as u32;
        h = rh as u32;
        processed = processed.resize_exact(w, h, image::imageops::FilterType::Triangle);
    } else if let Some(max) = resize.get("max").and_then(Value::as_u64) {
        if max == 0 || max > 8192 {
            return Err(AssetError::new("INVALID_OPS", format!("resize.max 越界: {max}")));
        }
        if src_w > max as u32 || src_h > max as u32 {
            processed = processed.resize(max as u32, max as u32, image::imageops::FilterType::Triangle);
            w = processed.width();
            h = processed.height();
        }
    } else if !resize.is_null() {
        return Err(AssetError::new("INVALID_OPS", "resize 须为 {width,height} 或 {max}"));
    }

    // format(缺省 = 源扩展名)。
    let src_ext = Path::new(&rel)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("png")
        .to_ascii_lowercase();
    let fmt = ops
        .get("format")
        .and_then(Value::as_str)
        .unwrap_or(&src_ext)
        .to_ascii_lowercase();
    let (out_ext, img_fmt) = match fmt.as_str() {
        "png" => ("png", image::ImageFormat::Png),
        "jpg" | "jpeg" => ("jpg", image::ImageFormat::Jpeg),
        other => {
            return Err(AssetError::new("INVALID_OPS", format!("format 仅支持 png/jpg: {other}")))
        }
    };

    // 确定性输出路径(幂等:同参数覆盖同一路径)。
    let stem = rel.rsplit('/').next().unwrap_or(&rel);
    let stem = stem.rsplit_once('.').map(|(s, _)| s).unwrap_or(stem);
    let folder = rel.rsplit_once('/').map(|(f, _)| f).unwrap_or("");
    let out_name = format!("{stem}@{w}x{h}.{out_ext}");
    let out_rel = if folder.is_empty() {
        out_name.clone()
    } else {
        format!("{folder}/{out_name}")
    };
    let out_abs = project.content_root().join(&out_rel);
    processed
        .save_with_format(&out_abs, img_fmt)
        .map_err(|e| AssetError::new("ENCODE_ERR", format!("贴图编码失败 {out_rel}: {e}")))?;
    let bytes = std::fs::metadata(&out_abs)?.len();

    // 输出登记为新资产(同目录;源已在 Content 内,import 走直接登记路径)。
    let out = import_assets(project, &[out_abs.to_string_lossy().into()], folder, None)?;
    let one = out
        .imported
        .into_iter()
        .next()
        .ok_or_else(|| AssetError::new("IMPORT_ERR", format!("输出登记失败: {:?}", out.failed)))?;

    // provenance.detail 记派生事实(覆盖 import 的默认 detail=null)。
    let out_meta_path = meta_path_for(&project.content_root(), &out_rel);
    let mut out_meta = MetaDoc::load(&out_meta_path)?;
    if let Some(p) = out_meta.provenance.as_mut() {
        p.detail = Some(serde_json::json!({
            "derivedFrom": meta.guid,
            "tool": "texture_process",
            "ops": ops,
        }));
    }
    out_meta.save(&out_meta_path)?;

    Ok(ProcessOutcome {
        output_rel: one.asset_path,
        guid: one.guid,
        width: w,
        height: h,
        bytes,
    })
}
