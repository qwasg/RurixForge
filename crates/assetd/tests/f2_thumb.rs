//! assetd 缩略图测试(F2 wave.3):贴图原图直出 data URL;非贴图 NO_THUMBNAIL。

use std::path::PathBuf;

use assetd::import::import_assets;
use assetd::project::ForgeProject;
use assetd::thumb::thumbnail_data_url;

/// 1×1 RGBA PNG(image crate 现编;wave.4 曾踩"硬编码字节头合法但 IDAT 损坏"的坑:
/// image_dimensions 只读头会放过,全量解码才暴露——测试样本必须真编码)。
fn png_1x1() -> Vec<u8> {
    let img = image::RgbaImage::from_pixel(1, 1, image::Rgba([255, 0, 0, 255]));
    let mut buf = std::io::Cursor::new(Vec::new());
    img.write_to(&mut buf, image::ImageFormat::Png).unwrap();
    buf.into_inner()
}

fn tmp_project(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("assetd_test_{}_{}", tag, std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn texture_thumbnail_is_original_bytes_data_url() {
    let root = tmp_project("thumb_png");
    let project = ForgeProject::with_defaults(root.clone());
    let src = root.join("dot.png");
    let png = png_1x1();
    std::fs::write(&src, &png).unwrap();

    let out = import_assets(&project, &[src.to_string_lossy().into()], "Textures", None).unwrap();
    assert_eq!(out.failed.len(), 0, "导入失败: {:?}", out.failed);
    let asset_path = out.imported[0].asset_path.clone();

    let (url, bytes) = thumbnail_data_url(&project, &asset_path).unwrap();
    assert_eq!(bytes, png.len() as u64);
    assert!(url.starts_with("data:image/png;base64,"), "前缀不符: {url}");

    // 解码回原文逐字节一致(原图直出,无重编码)。
    use base64::Engine as _;
    let decoded = base64::engine::general_purpose::STANDARD
        .decode(url.strip_prefix("data:image/png;base64,").unwrap())
        .unwrap();
    assert_eq!(decoded, png);
}

#[test]
fn non_texture_thumbnail_is_no_thumbnail() {
    let root = tmp_project("thumb_mesh");
    let project = ForgeProject::with_defaults(root.clone());
    // 用 png 字节冒充 mesh 不行;直接用真实 gltf conformance 样本。
    let src = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/tri_min.gltf");
    let dst = root.join("tri_min.gltf");
    std::fs::copy(&src, &dst).unwrap();
    let out = import_assets(&project, &[dst.to_string_lossy().into()], "Meshes", None).unwrap();
    assert_eq!(out.failed.len(), 0);

    let err = thumbnail_data_url(&project, &out.imported[0].asset_path).unwrap_err();
    assert_eq!(err.code, "NO_THUMBNAIL", "应为 NO_THUMBNAIL: {err}");
}
