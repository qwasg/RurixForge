//! assetd wave.4 集成测试(G-F2-4):材质创建/纹理引用边、贴图解码尺寸、
//! texture_process resize 实测、mesh_inspect 统计。

use std::path::PathBuf;

use assetd::import::import_assets;
use assetd::inspect::inspect_mesh;
use assetd::material::{create_material, validate_rxmat};
use assetd::project::ForgeProject;
use assetd::refs::RefGraph;
use assetd::texture::{decode_size, process_texture};
use serde_json::json;

/// 2×1 PNG(红蓝两像素,手工构造最小合法文件)。
fn png_2x1() -> Vec<u8> {
    // 用 image crate 现编一个,避免硬编码字节错误。
    let img = image::RgbImage::from_fn(2, 1, |x, _| {
        if x == 0 { image::Rgb([255, 0, 0]) } else { image::Rgb([0, 0, 255]) }
    });
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

fn import_png(project: &ForgeProject, root: &PathBuf, name: &str) -> assetd::import::ImportOne {
    let src = root.join(name);
    std::fs::write(&src, png_2x1()).unwrap();
    let out = import_assets(project, &[src.to_string_lossy().into()], "Textures", None).unwrap();
    assert_eq!(out.failed.len(), 0, "导入失败: {:?}", out.failed);
    out.imported.into_iter().next().unwrap()
}

#[test]
fn texture_import_decodes_real_dimensions() {
    let root = tmp_project("tex_dims");
    let project = ForgeProject::with_defaults(root.clone());
    let one = import_png(&project, &root, "dot.png");
    assert_eq!(one.atype.as_str(), "texture");
    assert_eq!(one.width, Some(2), "宽须为解码实测 2");
    assert_eq!(one.height, Some(1), "高须为解码实测 1");
    // 文件级复核:decode_size 与磁盘一致。
    let abs = project.content_root().join(&one.asset_path);
    assert_eq!(decode_size(&abs).unwrap(), (2, 1));
}

#[test]
fn texture_process_resize_exact_produces_new_asset() {
    let root = tmp_project("tex_resize");
    let project = ForgeProject::with_defaults(root.clone());
    let one = import_png(&project, &root, "dot.png");

    let out = process_texture(&project, &one.asset_path, &json!({ "resize": { "width": 4, "height": 4 } }))
        .unwrap();
    assert_eq!((out.width, out.height), (4, 4), "返回尺寸须为 4x4");
    assert!(out.output_rel.ends_with("dot@4x4.png"), "确定性命名: {}", out.output_rel);
    assert!(out.bytes > 0);
    // 实测:输出文件解码尺寸 = 4x4(不靠返回值充数)。
    let out_abs = project.content_root().join(&out.output_rel);
    assert_eq!(decode_size(&out_abs).unwrap(), (4, 4));
    // 原资产不动。
    let src_abs = project.content_root().join(&one.asset_path);
    assert_eq!(decode_size(&src_abs).unwrap(), (2, 1));
    // 幂等:同参再跑覆盖同一路径,GUID 不变。
    let out2 = process_texture(&project, &one.asset_path, &json!({ "resize": { "width": 4, "height": 4 } }))
        .unwrap();
    assert_eq!(out2.output_rel, out.output_rel);
    assert_eq!(out2.guid, out.guid, "幂等覆盖须保 GUID");
}

#[test]
fn texture_process_max_only_shrinks() {
    let root = tmp_project("tex_max");
    let project = ForgeProject::with_defaults(root.clone());
    let one = import_png(&project, &root, "dot.png");
    // max=4 大于源(2x1) → 不放大,输出原尺寸。
    let out = process_texture(&project, &one.asset_path, &json!({ "resize": { "max": 4 } })).unwrap();
    assert_eq!((out.width, out.height), (2, 1), "不放大原则");
}

#[test]
fn material_create_and_texture_ref_edge() {
    let root = tmp_project("mat_create");
    let project = ForgeProject::with_defaults(root.clone());
    let tex = import_png(&project, &root, "wood.png");

    let params = serde_json::json!({ "baseColor": [1.0, 0.8, 0.6, 1.0], "roughness": 0.7 });
    let textures = serde_json::json!({ "albedo": tex.guid });
    let created = create_material(
        &project,
        "",
        "wood_mat",
        None,
        Some(params.as_object().unwrap()),
        Some(textures.as_object().unwrap()),
    )
    .unwrap();
    assert_eq!(created.asset_path, "Materials/wood_mat.rxmat");
    assert_eq!(created.texture_refs, vec![("albedo".to_string(), tex.guid.clone())]);

    // .rxmat 落盘且通过校验。
    let abs = project.content_root().join(&created.asset_path);
    let doc: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&abs).unwrap()).unwrap();
    validate_rxmat(&doc).unwrap();
    assert_eq!(doc["shader"], "pbr-default");
    assert_eq!(doc["textures"]["albedo"], tex.guid.as_str());

    // 引用图重建:material→texture 边。
    let graph = RefGraph::rebuild(&project).unwrap();
    let refs = graph.refs(&created.guid);
    assert!(
        refs.iter().any(|e| e.to_guid == tex.guid && e.edge_type == "material→texture"),
        "缺 material→texture 边: {refs:?}"
    );
}

#[test]
fn material_create_rejects_unknown_texture_guid() {
    let root = tmp_project("mat_bad_guid");
    let project = ForgeProject::with_defaults(root.clone());
    let textures = serde_json::json!({ "albedo": "00000000-0000-0000-0000-000000000000" });
    let err = create_material(&project, "", "bad", None, None, Some(textures.as_object().unwrap()))
        .unwrap_err();
    assert_eq!(err.code, "UNKNOWN_GUID");
}

#[test]
fn mesh_inspect_reads_real_rxmesh_stats() {
    let root = tmp_project("mesh_inspect");
    let project = ForgeProject::with_defaults(root.clone());
    let src = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/tri_min.gltf");
    let dst = root.join("tri_min.gltf");
    std::fs::copy(&src, &dst).unwrap();
    let out = import_assets(&project, &[dst.to_string_lossy().into()], "Meshes", None).unwrap();
    assert_eq!(out.failed.len(), 0);
    let rel = out.imported[0].asset_path.clone();

    let r = inspect_mesh(&project, &rel).unwrap();
    assert_eq!(r.vertices, 3);
    assert_eq!(r.triangles, 1);
    assert!(r.meshlets >= 1, "meshlets={} 须 ≥1", r.meshlets);
    assert!(r.lods >= 1, "lods={} 须 ≥1", r.lods);
    let (min, max) = r.bounds.expect("tri_min 有顶点,bounds 须存在");
    // tri_min 是 XY 平面三角形:z 必为 0;x/y 在 [-1,1] 内。
    assert_eq!(min[2], 0.0);
    assert_eq!(max[2], 0.0);
    assert!(min[0] >= -1.0 && max[0] <= 1.0, "bounds x 越界: {min:?} {max:?}");
    assert!(r.artifact.starts_with("rxmesh/"));

    // 场景资产(非 mesh)→ WRONG_TYPE。
    let scene_err = inspect_mesh(&project, "Scenes/Main.rxscene").unwrap_err();
    assert!(scene_err.code == "WRONG_TYPE" || scene_err.code == "NO_META");
}
