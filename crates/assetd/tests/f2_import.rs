//! assetd 库层集成测试(G-F2-1):导入/缓存/构建状态全链。

use std::path::PathBuf;

use assetd::import::import_assets;
use assetd::meta::MetaDoc;
use assetd::project::ForgeProject;
use assetd::status::build_status;
use assetd::{meta_path_for, BuildState};

/// 临时测试项目(每次用唯一目录,不撞车)。
fn tmp_project(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("assetd_test_{}_{}", tag, std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// 上游 rurix 仓根(conformance gltf 样本所在)。
fn rurix_root() -> PathBuf {
    PathBuf::from("H:/rurix")
}

/// 复制 conformance 样本到临时目录。
fn copy_conformance(name: &str, dest_dir: &std::path::Path) -> PathBuf {
    let src = rurix_root()
        .join("conformance")
        .join("asset")
        .join("gltf")
        .join("accept")
        .join(name);
    let dst = dest_dir.join(name);
    std::fs::copy(&src, &dst).unwrap();
    dst
}

#[test]
fn import_gltf_builds_rxmesh_and_meta() {
    let root = tmp_project("import_gltf");
    let project = ForgeProject::with_defaults(root.clone());
    let src = copy_conformance("tri_min.gltf", &root);

    let out = import_assets(
        &project,
        &[src.to_string_lossy().into()],
        "Meshes",
        None,
    )
    .unwrap();
    assert_eq!(out.failed.len(), 0, "导入失败: {:?}", out.failed);
    assert_eq!(out.imported.len(), 1);
    let one = &out.imported[0];
    assert_eq!(one.atype.as_str(), "mesh");
    assert!(!one.guid.is_empty());
    assert!(!one.cache_hit, "首导入不应命中缓存");
    let artifact = one.artifact.as_ref().expect("应有 .rxmesh 产物");
    assert!(artifact.starts_with("rxmesh/"));
    assert_eq!(one.vertex_count, Some(3));
    assert_eq!(one.triangle_count, Some(1));

    // .meta 在磁盘上。
    let meta_path = meta_path_for(&project.content_root(), &one.asset_path);
    assert!(meta_path.is_file(), ".meta 未落盘");
    let meta = MetaDoc::load(&meta_path).unwrap();
    assert_eq!(meta.guid, one.guid);
    assert_eq!(meta.atype, "mesh");
    assert_eq!(meta.importer, "gltf");
    assert_eq!(meta.build_state.as_deref(), Some("current"));
    assert_eq!(
        meta.provenance.as_ref().map(|p| p.origin.as_str()),
        Some("user-import")
    );

    // .rxmesh 产物在 .forge/cache/。
    let cache_file = project.cache_root().join(artifact);
    assert!(cache_file.is_file(), ".rxmesh 未落盘");
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn second_import_cache_hit_zero_rebuild() {
    let root = tmp_project("cache_hit");
    let project = ForgeProject::with_defaults(root.clone());
    let src = copy_conformance("tri_min.gltf", &root);

    let first = import_assets(&project, &[src.to_string_lossy().into()], "Meshes", None).unwrap();
    assert_eq!(first.imported[0].cache_hit, false);

    // 二次导入同一路径:源已存在,复制后字节相同 → cache_key 相同 → 命中。
    let second = import_assets(&project, &[src.to_string_lossy().into()], "Meshes", None).unwrap();
    assert_eq!(second.imported[0].cache_hit, true, "二次导入应命中缓存");
    assert_eq!(second.imported[0].guid, first.imported[0].guid, "GUID 不变");

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn build_status_reports_current_after_import() {
    let root = tmp_project("status");
    let project = ForgeProject::with_defaults(root.clone());
    let src = copy_conformance("quad_indexed.glb", &root);

    let out = import_assets(&project, &[src.to_string_lossy().into()], "Meshes", None).unwrap();
    assert_eq!(out.failed.len(), 0);
    let path = &out.imported[0].asset_path;

    let status = build_status(&project, &[path.clone()]).unwrap();
    assert_eq!(status.len(), 1);
    assert_eq!(status[0].state, BuildState::Current);
    assert!(!status[0].hash.is_empty());

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn build_status_stale_after_meta_tamper() {
    let root = tmp_project("stale");
    let project = ForgeProject::with_defaults(root.clone());
    let src = copy_conformance("tri_min.gltf", &root);
    let out = import_assets(&project, &[src.to_string_lossy().into()], "Meshes", None).unwrap();
    let path = out.imported[0].asset_path.clone();

    // 改 importSettings → 缓存键变 → 应报 stale(未重建)。
    let meta_path = meta_path_for(&project.content_root(), &path);
    let mut meta = MetaDoc::load(&meta_path).unwrap();
    meta.import_settings.insert("normals".into(), serde_json::json!("recompute"));
    meta.save(&meta_path).unwrap();

    let status = build_status(&project, &[path.clone()]).unwrap();
    assert_eq!(status[0].state, BuildState::Stale, "改 importSettings 后应 stale");

    let _ = std::fs::remove_dir_all(&root);
}
