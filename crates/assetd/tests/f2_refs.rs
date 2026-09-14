//! wave.2 引用防护与移动 redirector 集成测试。

use std::path::PathBuf;

use assetd::import::import_assets;
use assetd::meta::MetaDoc;
use assetd::ops::{delete_assets, fix_redirectors, move_asset, reimport_assets, set_meta};
use assetd::project::ForgeProject;
use assetd::refs::RefGraph;
use assetd::status::build_status;
use assetd::{meta_path_for, BuildState};

fn tmp_project(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("assetd_w2_{}_{}", tag, std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn copy_conformance(name: &str, dest_dir: &std::path::Path) -> PathBuf {
    let src = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures").join(name);
    let dst = dest_dir.join(name);
    std::fs::copy(&src, &dst).unwrap();
    dst
}

#[test]
fn delete_blocked_by_scene_ref() {
    let root = tmp_project("del_block");
    let project = ForgeProject::with_defaults(root.clone());
    let src = copy_conformance("tri_min.gltf", &root);
    let out = import_assets(&project, &[src.to_string_lossy().into()], "Meshes", None).unwrap();
    let mesh_path = out.imported[0].asset_path.clone();
    let mesh_guid = out.imported[0].guid.clone();

    // 建一个 .rxscene 引用该网格 GUID,并写 .meta 使其进入已知 GUID 表。
    let scene_dir = project.content_root().join("Scenes");
    std::fs::create_dir_all(&scene_dir).unwrap();
    let scene_file = scene_dir.join("Main.rxscene");
    std::fs::write(
        &scene_file,
        format!(r#"{{"entities":[{{"components":[{{"props":{{"mesh":"{mesh_guid}"}},"type":"MeshRenderer"}}]}}]}}"#),
    )
    .unwrap();
    let scene_guid = assetd::new_guid();
    let scene_meta = assetd::meta::MetaDoc::new("Scenes/Main.rxscene", scene_guid.clone()).unwrap();
    let scene_meta_path = assetd::meta_path_for(&project.content_root(), "Scenes/Main.rxscene");
    scene_meta.save(&scene_meta_path).unwrap();

    // 重建引用图。
    let graph = RefGraph::rebuild(&project).unwrap();
    let refs = graph.referenced_by(&mesh_guid);
    assert!(!refs.is_empty(), "场景应引用网格");

    // 删除被阻断。
    let out = delete_assets(&project, &[mesh_path.clone()], false).unwrap();
    assert_eq!(out.deleted.len(), 0);
    assert_eq!(out.blocked_by_refs.len(), 1);
    let blockers = &out.blocked_by_refs[0].1;
    assert!(!blockers.is_empty(), "阻断清单为空");

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn move_writes_redirector_and_fix_clears() {
    let root = tmp_project("move_redir");
    let project = ForgeProject::with_defaults(root.clone());
    let src = copy_conformance("tri_min.gltf", &root);
    let out = import_assets(&project, &[src.to_string_lossy().into()], "Meshes", None).unwrap();
    let mesh_path = out.imported[0].asset_path.clone();
    let mesh_guid = out.imported[0].guid.clone();

    // 移动到 Prefabs。
    let mv = move_asset(&project, &mesh_path, "Prefabs", None).unwrap();
    assert!(mv.moved);
    let (g, old, new) = mv.redirector.unwrap();
    assert_eq!(g, mesh_guid);
    assert_eq!(old, "Meshes/tri_min.gltf");
    assert_eq!(new, "Prefabs/tri_min.gltf");

    // 磁盘上新文件在,旧文件不在。
    assert!(project.content_root().join(&new).is_file());
    assert!(!project.content_root().join(&old).is_file());

    // redirector 在图里。
    let graph = RefGraph::load(&project).unwrap();
    assert!(graph.redirector_for(&mesh_guid).is_some());

    // fix_redirectors 清除。
    let fixed = fix_redirectors(&project, None).unwrap();
    assert_eq!(fixed.len(), 1);
    assert_eq!(fixed[0], mesh_guid);
    let graph2 = RefGraph::load(&project).unwrap();
    assert!(graph2.redirector_for(&mesh_guid).is_none());

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn set_meta_then_reimport_rebuilds() {
    let root = tmp_project("reimport");
    let project = ForgeProject::with_defaults(root.clone());
    let src = copy_conformance("tri_min.gltf", &root);
    let out = import_assets(&project, &[src.to_string_lossy().into()], "Meshes", None).unwrap();
    let mesh_path = out.imported[0].asset_path.clone();

    // 改 importSettings → stale。
    let mut patch = serde_json::Map::new();
    patch.insert("normals".into(), serde_json::json!("recompute"));
    set_meta(&project, &mesh_path, &patch).unwrap();
    let st = build_status(&project, &[mesh_path.clone()]).unwrap();
    assert_eq!(st[0].state, BuildState::Stale);

    // reimport → 重建 → current。
    let rebuilt = reimport_assets(&project, &[mesh_path.clone()]).unwrap();
    assert_eq!(rebuilt.len(), 1);
    let st2 = build_status(&project, &[mesh_path.clone()]).unwrap();
    // reimport 后 .meta buildState 应更新为 current。
    let meta_path = meta_path_for(&project.content_root(), &mesh_path);
    let meta = MetaDoc::load(&meta_path).unwrap();
    assert_eq!(meta.build_state.as_deref(), Some("current"));

    let _ = std::fs::remove_dir_all(&root);
}
