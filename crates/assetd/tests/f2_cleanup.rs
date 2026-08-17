//! assetd wave.5 集成测试(G-F2-5 前半):cleanup 扫描提案 + move_asset 改名。

use std::path::PathBuf;

use assetd::cleanup::scan_cleanup;
use assetd::import::import_assets;
use assetd::material::create_material;
use assetd::ops::move_asset;
use assetd::project::ForgeProject;
use assetd::refs::RefGraph;

fn tmp_project(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("assetd_test_{}_{}", tag, std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn import_gltf(project: &ForgeProject, root: &PathBuf, dest: &str) -> assetd::import::ImportOne {
    let src = PathBuf::from("H:/rurix/conformance/asset/gltf/accept/tri_min.gltf");
    let dst = root.join("tri_min.gltf");
    std::fs::copy(&src, &dst).unwrap();
    let out = import_assets(project, &[dst.to_string_lossy().into()], dest, None).unwrap();
    assert_eq!(out.failed.len(), 0);
    out.imported.into_iter().next().unwrap()
}

fn import_png_named(project: &ForgeProject, root: &PathBuf, name: &str, dest: &str) -> assetd::import::ImportOne {
    let img = image::RgbaImage::from_pixel(1, 1, image::Rgba([1, 2, 3, 255]));
    let mut buf = std::io::Cursor::new(Vec::new());
    img.write_to(&mut buf, image::ImageFormat::Png).unwrap();
    let src = root.join(name);
    std::fs::write(&src, buf.into_inner()).unwrap();
    let out = import_assets(project, &[src.to_string_lossy().into()], dest, None).unwrap();
    assert_eq!(out.failed.len(), 0);
    out.imported.into_iter().next().unwrap()
}

#[test]
fn cleanup_detects_misplaced_naming_orphan() {
    let root = tmp_project("cleanup_scan");
    let project = ForgeProject::with_defaults(root.clone());
    // 错放:png 放 Meshes/;命名混乱:文件名含空格括号。
    let bad = import_png_named(&project, &root, "my tex (2).png", "Meshes");
    // 正常放置 + 被材质引用(不 orphan、不错放)。
    let tex = import_png_named(&project, &root, "wood.png", "Textures");
    let textures = serde_json::json!({ "albedo": tex.guid });
    create_material(&project, "", "wood_mat", None, None, Some(textures.as_object().unwrap())).unwrap();

    let report = scan_cleanup(&project).unwrap();
    assert!(report.scanned >= 3, "scanned={} 应 ≥3", report.scanned);

    let misplaced = report.proposals.iter().find(|p| p.issue == "misplaced").expect("应有 misplaced 提案");
    assert_eq!(misplaced.asset_path, bad.asset_path);
    assert_eq!(misplaced.dest_folder.as_deref(), Some("Textures"));

    let naming = report.proposals.iter().find(|p| p.issue == "naming").expect("应有 naming 提案");
    assert_eq!(naming.asset_path, bad.asset_path);
    assert_eq!(naming.new_name.as_deref(), Some("my_tex_2.png"), "清洗后文件名");

    // 错放 png 无引用 → orphan;wood.png 被材质引用 → 不 orphan;材质引用贴图但自身无入边 → orphan。
    let orphans: Vec<&str> = report
        .proposals
        .iter()
        .filter(|p| p.issue == "orphan")
        .map(|p| p.asset_path.as_str())
        .collect();
    assert!(orphans.contains(&bad.asset_path.as_str()), "错放 png 应为 orphan: {orphans:?}");
    assert!(!orphans.contains(&tex.asset_path.as_str()), "被引用贴图不应为 orphan");
    assert!(orphans.iter().any(|p| p.contains("wood_mat")), "材质无入边应为 orphan: {orphans:?}");

    // impact 统计齐全。
    let get = |k: &str| report.impact.iter().find(|(i, _)| i == k).map(|(_, n)| *n).unwrap_or(0);
    assert!(get("misplaced") >= 1 && get("naming") >= 1 && get("orphan") >= 1);
}

#[test]
fn move_asset_with_rename_keeps_guid_and_refs() {
    let root = tmp_project("move_rename");
    let project = ForgeProject::with_defaults(root.clone());
    let bad = import_png_named(&project, &root, "my tex (2).png", "Meshes");

    // 改名 + 移目录一步到位(cleanup 提案执行路径)。
    let mv = move_asset(&project, &bad.asset_path, "Textures", Some("my_tex_2.png")).unwrap();
    let (g, old, new) = mv.redirector.unwrap();
    assert_eq!(g, bad.guid, "GUID 不变");
    assert_eq!(old, "Meshes/my tex (2).png");
    assert_eq!(new, "Textures/my_tex_2.png");

    // 新路径 .meta 在,GUID 不变;旧路径文件消失。
    let new_meta = project.content_root().join("Textures/my_tex_2.png.meta");
    assert!(new_meta.is_file());
    assert!(!project.content_root().join("Meshes/my tex (2).png").exists());
    let graph = RefGraph::load(&project).unwrap();
    assert!(graph.redirectors.iter().any(|r| r.guid == bad.guid));

    // 非法新名拒绝。
    let err = move_asset(&project, &new, "Textures", Some("a/b.png")).unwrap_err();
    assert_eq!(err.code, "INVALID_OPS");
}

#[test]
fn move_asset_without_rename_unchanged() {
    let root = tmp_project("move_plain");
    let project = ForgeProject::with_defaults(root.clone());
    let one = import_gltf(&project, &root, "Meshes");
    let mv = move_asset(&project, &one.asset_path, "Prefabs", None).unwrap();
    assert_eq!(mv.redirector.unwrap().2, "Prefabs/tri_min.gltf");
}
