use assetd::{project::ForgeProject, shader::*};
use serde_json::json;

#[test]
fn graph_drafts_are_conflict_checked_and_materials_reference_guids() {
    let root = std::env::temp_dir().join(format!("forge-shader-test-{}", assetd::new_guid()));
    std::fs::create_dir_all(root.join("Content")).unwrap();
    let project = ForgeProject::with_defaults(root.clone());
    let graph:GraphDoc=serde_json::from_value(json!({"version":1,"id":"test","name":"Draft","domain":"sprite2d","parameters":[{"id":"tint","name":"Tint","type":"color","default":[1,0,0,1]}],"nodes":[],"outputs":{"color":{"param":"tint"}}})).unwrap();
    let saved = save(&project, "Shaders/Test.rxshadergraph", &graph, None).unwrap();
    let guid = saved["guid"].as_str().unwrap();
    assert_eq!(load(&project, guid).unwrap()["graph"]["id"], "test");
    assert_eq!(compile(&project, guid).unwrap()["ok"], true);
    assert_eq!(
        save(&project, "Shaders/Test.rxshadergraph", &graph, None)
            .unwrap_err()
            .code,
        "SHADER_CONFLICT"
    );
    let mut draft = graph.clone();
    draft
        .nodes
        .push(serde_json::from_value(json!({"id":"bad","type":"bad"})).unwrap());
    let bad = save(
        &project,
        "Shaders/Test.rxshadergraph",
        &draft,
        saved["sourceHash"].as_str(),
    )
    .unwrap();
    assert!(!bad["diagnostics"].as_array().unwrap().is_empty());
    assert_eq!(bad["published"], false);
    assert!(create_material(
        &project,
        "Materials/test.rxmat",
        guid,
        &json!({}),
        &json!({})
    )
    .is_err());
    save(
        &project,
        "Shaders/Test.rxshadergraph",
        &graph,
        bad["sourceHash"].as_str(),
    )
    .unwrap();
    assert!(create_material(
        &project,
        "Materials/test.rxmat",
        guid,
        &json!({"unknown":1}),
        &json!({})
    )
    .is_err());
    let material = create_material(
        &project,
        "Materials/test.rxmat",
        guid,
        &json!({"tint":[0,1,0,1]}),
        &json!({}),
    )
    .unwrap();
    assert_eq!(material["material"]["shaderGraph"], guid);
    assert!(load(&project, "../outside.rxshadergraph").is_err());
    // The ordinary asset-import path must compile graphs as well; registration
    // alone must not turn an invalid graph into a supposedly built asset.
    let source = root.join("import.rxshadergraph");
    std::fs::write(&source, serde_json::to_vec(&draft).unwrap()).unwrap();
    let imported = assetd::import::import_assets(
        &project,
        &[source.to_string_lossy().into_owned()],
        "Imported",
        None,
    )
    .unwrap();
    assert_eq!(imported.failed.len(), 1);
    assert!(imported.imported.is_empty());
    let failed_meta = assetd::meta::MetaDoc::load(
        &project
            .content_root()
            .join("Imported/import.rxshadergraph.meta"),
    )
    .unwrap();
    assert_eq!(failed_meta.build_state.as_deref(), Some("failed"));
    std::fs::write(&source, serde_json::to_vec(&graph).unwrap()).unwrap();
    let imported = assetd::import::import_assets(
        &project,
        &[source.to_string_lossy().into_owned()],
        "Imported",
        None,
    )
    .unwrap();
    assert!(imported.failed.is_empty());
    assert_eq!(imported.imported[0].atype, assetd::AssetType::ShaderGraph);
    assert_eq!(imported.imported[0].guid, failed_meta.guid);
    // The directory was uniquely created above and contains only this test's generated artifacts.
    assert!(root.starts_with(std::env::temp_dir()));
    assert!(!std::fs::symlink_metadata(&root)
        .unwrap()
        .file_type()
        .is_symlink());
    std::fs::remove_dir_all(root).unwrap();
}
