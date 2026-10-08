//! Self-contained Blender-shaped GLB fixtures; no Blender installation or external repository.
use assetd::model::{import_model_bundle, inspect_model_source, load_model, ModelManifest};
use assetd::project::ForgeProject;
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::io::Cursor;
use std::path::{Path, PathBuf};

struct Temp(PathBuf);
impl Temp {
    fn new() -> Self {
        let p = std::env::temp_dir().join(format!("model-pipeline-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&p).unwrap();
        Self(p)
    }
}
impl Drop for Temp {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
fn manifest(revision: u64) -> ModelManifest {
    ModelManifest {
        version: 1,
        source_id: "test-blender-source".into(),
        name: "Textured actor".into(),
        kind: "character".into(),
        revision,
        source_blend: Some("Sources/actor.blend".into()),
        object_ids: BTreeMap::new(),
        idle_clip: None,
        walk_clip: None,
    }
}

#[derive(Default)]
struct Bin {
    bytes: Vec<u8>,
    views: Vec<Value>,
    accessors: Vec<Value>,
}
impl Bin {
    fn view(&mut self, bytes: &[u8]) -> usize {
        while self.bytes.len() % 4 != 0 {
            self.bytes.push(0);
        }
        let i = self.views.len();
        self.views
            .push(json!({"buffer":0,"byteOffset":self.bytes.len(),"byteLength":bytes.len()}));
        self.bytes.extend(bytes);
        i
    }
    fn floats(&mut self, values: &[f32], components: usize, kind: &str) -> usize {
        let bytes: Vec<_> = values.iter().flat_map(|v| v.to_le_bytes()).collect();
        let view = self.view(&bytes);
        let i = self.accessors.len();
        let mut a = json!({"bufferView":view,"componentType":5126,"count":values.len()/components,"type":kind});
        if kind == "VEC3" {
            a["min"] = json!([-100., -100., -100.]);
            a["max"] = json!([100., 100., 100.]);
        }
        self.accessors.push(a);
        i
    }
    fn shorts(&mut self, values: &[u16], components: usize, kind: &str) -> usize {
        let bytes: Vec<_> = values.iter().flat_map(|v| v.to_le_bytes()).collect();
        let view = self.view(&bytes);
        let i = self.accessors.len();
        self.accessors.push(json!({"bufferView":view,"componentType":5123,"count":values.len()/components,"type":kind}));
        i
    }
}

fn fixture(scale: f32, pixel: [u8; 4]) -> (Value, Vec<u8>) {
    let mut b = Bin::default();
    let pos = b.floats(&[0., 0., 0., scale, 0., 0., 0., 1., 0.], 3, "VEC3");
    let normal = b.floats(&[0., 0., 1., 0., 0., 1., 0., 0., 1.], 3, "VEC3");
    let uv = b.floats(&[0., 0., 1., 0., 0., 1.], 2, "VEC2");
    let tangent = b.floats(&[1., 0., 0., 1., 1., 0., 0., 1., 1., 0., 0., 1.], 4, "VEC4");
    let joints = b.shorts(&[0; 12], 4, "VEC4");
    let weights = b.floats(&[1., 0., 0., 0., 1., 0., 0., 0., 1., 0., 0., 0.], 4, "VEC4");
    let indices = b.shorts(&[0, 1, 2], 1, "SCALAR");
    let ibm = b.floats(
        &[
            1., 0., 0., 0., 0., 1., 0., 0., 0., 0., 1., 0., 0., 0., 0., 1.,
        ],
        16,
        "MAT4",
    );
    let times = b.floats(&[0., 1.], 1, "SCALAR");
    let translations = b.floats(&[0., 0., 0., 0., 0., 1.], 3, "VEC3");
    let mut image = Cursor::new(Vec::new());
    image::DynamicImage::ImageRgba8(image::RgbaImage::from_pixel(2, 2, image::Rgba(pixel)))
        .write_to(&mut image, image::ImageFormat::Png)
        .unwrap();
    let image_view = b.view(&image.into_inner());
    let primitive = |material| json!({"attributes":{"POSITION":pos,"NORMAL":normal,"TANGENT":tangent,"TEXCOORD_0":uv,"JOINTS_0":joints,"WEIGHTS_0":weights},"indices":indices,"material":material});
    let channel = json!({"samplers":[{"input":times,"output":translations,"interpolation":"LINEAR"}],"channels":[{"sampler":0,"target":{"node":2,"path":"translation"}}]});
    let mut idle = channel.clone();
    idle["name"] = json!("Idle");
    let mut walk = channel;
    walk["name"] = json!("Walk");
    let doc = json!({
        "asset":{"version":"2.0","generator":"assetd self-contained Blender-shaped fixture"},
        "scene":0,"scenes":[{"nodes":[0]}],
        "nodes":[{"name":"Root","extras":{"rurixId":"root-stable"},"translation":[2.,3.,4.],"children":[1,2]}, {"name":"Mesh","extras":{"rurixId":"mesh-stable"},"mesh":0,"skin":0,"translation":[1.,0.,0.]}, {"name":"Bone","extras":{"rurixId":"bone-stable"}}],
        "meshes":[{"name":"Body","primitives":[primitive(0),primitive(1)]}],
        "buffers":[{"byteLength":b.bytes.len()}],"bufferViews":b.views,"accessors":b.accessors,
        "images":[{"bufferView":image_view,"mimeType":"image/png"}],
        "samplers":[{"wrapS":33071,"wrapT":10497,"magFilter":9728,"minFilter":9729}],
        "textures":[{"name":"paint","source":0,"sampler":0}],
        "materials":[{"name":"Paint","extras":{"rurixId":"paint-material"},"pbrMetallicRoughness":{"baseColorFactor":[0.8,0.7,0.6,1.],"baseColorTexture":{"index":0},"metallicFactor":0.6,"roughnessFactor":0.4,"metallicRoughnessTexture":{"index":0}},"normalTexture":{"index":0,"scale":0.75},"occlusionTexture":{"index":0,"strength":0.8},"emissiveTexture":{"index":0},"emissiveFactor":[0.1,0.2,0.3],"doubleSided":true,"alphaMode":"BLEND"},{"name":"Second","pbrMetallicRoughness":{"baseColorFactor":[1.,0.,0.,1.]}}],
        "skins":[{"name":"Rig","joints":[2],"inverseBindMatrices":ibm,"skeleton":2}],
        "animations":[idle,walk]
    });
    (doc, b.bytes)
}

fn glb(doc: &Value, bin: &[u8]) -> Vec<u8> {
    let mut json = serde_json::to_vec(doc).unwrap();
    while json.len() % 4 != 0 {
        json.push(b' ');
    }
    let mut binary = bin.to_vec();
    while binary.len() % 4 != 0 {
        binary.push(0);
    }
    let mut result = Vec::new();
    for n in [
        0x46546c67u32,
        2,
        (12 + 8 + json.len() + 8 + binary.len()) as u32,
        json.len() as u32,
        0x4e4f534a,
    ] {
        result.extend(n.to_le_bytes());
    }
    result.extend(json);
    result.extend((binary.len() as u32).to_le_bytes());
    result.extend(0x004e4942u32.to_le_bytes());
    result.extend(binary);
    result
}
fn write_glb(path: &Path, scale: f32, pixel: [u8; 4]) {
    let (doc, bin) = fixture(scale, pixel);
    std::fs::write(path, glb(&doc, &bin)).unwrap();
}

#[test]
fn publishes_real_pbr_uv_rig_animation_and_character_template() {
    let temp = Temp::new();
    let src = temp.0.join("actor.glb");
    write_glb(&src, 1., [220, 30, 10, 255]);
    let project = ForgeProject::with_defaults(temp.0.join("project"));
    let result = import_model_bundle(&project, &src, &manifest(1)).unwrap();
    assert!(result.changed);
    assert_eq!(result.revision, 1);
    let model = load_model(&project, &result.guid).unwrap();
    assert_eq!(model.primitives.len(), 2);
    assert_eq!(model.primitives[0].uv0[1], [1., 0.]);
    assert_eq!(model.primitives[0].normals[0], [0., 0., 1.]);
    assert_eq!(model.primitives[0].tangents[0], [1., 0., 0., 1.]);
    assert_eq!(model.nodes[0].translation, [2., 3., 4.]);
    assert_eq!(model.nodes[1].translation, [1., 0., 0.]);
    assert_eq!(model.nodes[0].children, vec![1, 2]);
    assert_eq!(model.nodes[1].id, "mesh-stable");
    assert_eq!(model.materials[0].metallic, 0.6);
    assert_eq!(model.materials[0].normal_scale, 0.75);
    assert_eq!(model.materials[0].alpha_mode, "BLEND");
    assert_eq!(model.textures[0].rgba[0..4], [220, 30, 10, 255]);
    assert_eq!(model.textures[0].wrap_s, 33071);
    assert_eq!(model.skins[0].joints, vec![2]);
    assert_eq!(model.primitives[0].weights[0], [1., 0., 0., 0.]);
    assert_eq!(model.animations[0].channels[0].values[1], [0., 0., 1., 0.]);
    assert_eq!(model.animations[1].name, "Walk");
    let prefab: Value = serde_json::from_slice(
        &std::fs::read(project.content_root().join(&result.prefab_path)).unwrap(),
    )
    .unwrap();
    let components = prefab["entities"][0]["components"].as_array().unwrap();
    assert!(components
        .iter()
        .any(|c| c["type"] == "CharacterController"));
    let animator = components.iter().find(|c| c["type"] == "Animator").unwrap();
    assert_eq!(animator["props"]["idleClip"], "Idle");
    assert_eq!(animator["props"]["walkClip"], "Walk");
    let graph = assetd::refs::RefGraph::rebuild(&project).unwrap();
    assert!(!graph.refs(&result.guid).is_empty());
    assert!(!graph.refs(&result.prefab_guid).is_empty());
    assert!(project
        .content_root()
        .join(&model.textures[0].asset_path)
        .is_file());
}

#[test]
fn stable_guids_and_same_size_texture_geometry_update_in_new_revision() {
    let temp = Temp::new();
    let src = temp.0.join("actor.glb");
    write_glb(&src, 1., [220, 30, 10, 255]);
    let project = ForgeProject::with_defaults(temp.0.join("project"));
    let first = import_model_bundle(&project, &src, &manifest(1)).unwrap();
    let old = load_model(&project, &first.guid).unwrap();
    let unchanged = import_model_bundle(&project, &src, &manifest(2)).unwrap();
    assert!(!unchanged.changed);
    assert_eq!(unchanged.revision, 1);
    write_glb(&src, 2., [10, 40, 240, 255]);
    let second = import_model_bundle(&project, &src, &manifest(2)).unwrap();
    let new = load_model(&project, &first.guid).unwrap();
    assert_eq!(first.guid, second.guid);
    assert_eq!(first.prefab_guid, second.prefab_guid);
    assert_eq!(first.asset_guids, second.asset_guids);
    assert_eq!(new.revision, 2);
    assert_ne!(old.source_hash, new.source_hash);
    assert_eq!(old.textures[0].guid, new.textures[0].guid);
    assert_eq!(new.textures[0].width, old.textures[0].width);
    assert_ne!(new.textures[0].rgba, old.textures[0].rgba);
    assert_eq!(new.primitives[0].positions[1][0], 2.);
    let image = image::open(project.content_root().join(&new.textures[0].asset_path))
        .unwrap()
        .into_rgba8();
    assert_eq!(image.get_pixel(0, 0).0, [10, 40, 240, 255]);
    write_glb(&src, 3., [0, 0, 0, 255]);
    let error = import_model_bundle(&project, &src, &manifest(1)).unwrap_err();
    assert_eq!(error.code, "MODEL_REVISION_CONFLICT");
    assert_eq!(load_model(&project, &first.guid).unwrap().revision, 2);
}

#[test]
fn invalid_export_preserves_committed_package_and_can_retry() {
    let temp = Temp::new();
    let src = temp.0.join("actor.glb");
    write_glb(&src, 1., [1, 2, 3, 255]);
    let project = ForgeProject::with_defaults(temp.0.join("project"));
    let first = import_model_bundle(&project, &src, &manifest(1)).unwrap();
    let old = std::fs::read(project.content_root().join(&first.asset_path)).unwrap();
    let prefab = std::fs::read(project.content_root().join(&first.prefab_path)).unwrap();
    std::fs::write(&src, b"partial GLB write").unwrap();
    assert!(import_model_bundle(&project, &src, &manifest(2)).is_err());
    assert_eq!(
        old,
        std::fs::read(project.content_root().join(&first.asset_path)).unwrap()
    );
    assert_eq!(
        prefab,
        std::fs::read(project.content_root().join(&first.prefab_path)).unwrap()
    );
    write_glb(&src, 2., [4, 5, 6, 255]);
    assert!(
        import_model_bundle(&project, &src, &manifest(2))
            .unwrap()
            .changed
    );
}

#[test]
fn gltf_external_dependency_changes_invalidate_and_escape_is_rejected() {
    let temp = Temp::new();
    let src = temp.0.join("actor.gltf");
    let (mut doc, bin) = fixture(1., [1, 2, 3, 255]);
    doc["buffers"][0]["uri"] = json!("data.bin");
    std::fs::write(&src, serde_json::to_vec(&doc).unwrap()).unwrap();
    std::fs::write(temp.0.join("data.bin"), &bin).unwrap();
    let first = inspect_model_source(&src, &manifest(1)).unwrap();
    let (_, changed) = fixture(2., [1, 2, 3, 255]);
    std::fs::write(temp.0.join("data.bin"), changed).unwrap();
    let second = inspect_model_source(&src, &manifest(2)).unwrap();
    assert_ne!(first.source_hash, second.source_hash);
    assert_eq!(second.primitives[0].positions[1][0], 2.);
    doc["buffers"][0]["uri"] = json!("../outside.bin");
    std::fs::write(&src, serde_json::to_vec(&doc).unwrap()).unwrap();
    assert!(inspect_model_source(&src, &manifest(3)).is_err());
    doc["buffers"][0]["uri"] = json!("%2e%2e%2foutside.bin");
    std::fs::write(&src, serde_json::to_vec(&doc).unwrap()).unwrap();
    assert!(inspect_model_source(&src, &manifest(3)).is_err());
}

#[test]
fn map_template_contains_real_collision_binding() {
    let temp = Temp::new();
    let src = temp.0.join("map.glb");
    write_glb(&src, 1., [1, 2, 3, 255]);
    let project = ForgeProject::with_defaults(temp.0.join("project"));
    let mut spec = manifest(1);
    spec.kind = "map".into();
    let out = import_model_bundle(&project, &src, &spec).unwrap();
    let prefab: Value = serde_json::from_slice(
        &std::fs::read(project.content_root().join(&out.prefab_path)).unwrap(),
    )
    .unwrap();
    let components = prefab["entities"][0]["components"].as_array().unwrap();
    let collider = components.iter().find(|c| c["type"] == "Collider").unwrap();
    assert_eq!(collider["props"]["shape"], "mesh");
    assert_eq!(collider["props"]["model"], out.guid);
    assert!(components
        .iter()
        .any(|c| c["type"] == "RigidBody" && c["props"]["kind"] == "static"));
}

#[test]
fn characters_require_weighted_skin_and_distinct_idle_walk_clips() {
    let temp = Temp::new();
    let src = temp.0.join("actor.glb");
    let project = ForgeProject::with_defaults(temp.0.join("project"));
    let (doc, bin) = fixture(1., [1, 2, 3, 255]);
    for missing in ["skin", "weights", "idle", "walk"] {
        let mut invalid = doc.clone();
        match missing {
            "skin" => {
                invalid.as_object_mut().unwrap().remove("skins");
                invalid["nodes"][1].as_object_mut().unwrap().remove("skin");
            }
            "weights" => {
                for p in invalid["meshes"][0]["primitives"].as_array_mut().unwrap() {
                    p["attributes"].as_object_mut().unwrap().remove("WEIGHTS_0");
                }
            }
            "idle" => {
                invalid["animations"].as_array_mut().unwrap().remove(0);
            }
            _ => {
                invalid["animations"].as_array_mut().unwrap().remove(1);
            }
        }
        std::fs::write(&src, glb(&invalid, &bin)).unwrap();
        assert!(
            import_model_bundle(&project, &src, &manifest(1)).is_err(),
            "missing {missing} must reject publication"
        );
        assert!(!project
            .content_root()
            .join(assetd::model::package_path(&manifest(1).source_id))
            .exists());
    }
    let mut named = doc;
    named["animations"][0]["name"] = json!("Standing_01");
    named["animations"][1]["name"] = json!("Locomotion_02");
    std::fs::write(&src, glb(&named, &bin)).unwrap();
    assert!(import_model_bundle(&project, &src, &manifest(1)).is_err());
    let mut spec = manifest(1);
    spec.idle_clip = Some("standing_01".into());
    spec.walk_clip = Some("Locomotion_02".into());
    let out = import_model_bundle(&project, &src, &spec).unwrap();
    let loaded = load_model(&project, &out.guid).unwrap();
    assert_eq!(loaded.idle_clip, "Standing_01");
    assert_eq!(loaded.walk_clip, "Locomotion_02");
    spec.revision = 2;
    spec.walk_clip = spec.idle_clip.clone();
    assert!(import_model_bundle(&project, &src, &spec).is_err());
    assert_eq!(load_model(&project, &out.guid).unwrap().revision, 1);
}

fn template_node_ids(project: &ForgeProject, path: &str) -> BTreeMap<String, (u64, u64)> {
    let doc: Value =
        serde_json::from_slice(&std::fs::read(project.content_root().join(path)).unwrap()).unwrap();
    let mut out = BTreeMap::new();
    for e in doc["entities"].as_array().unwrap() {
        let components = e["components"].as_array().unwrap();
        if let Some(n) = components.iter().find(|c| c["type"] == "ModelNode") {
            let parent = components.iter().find(|c| c["type"] == "Parent").unwrap();
            let category = components.iter().find(|c| c["type"] == "Category").unwrap();
            assert_eq!(category["props"]["category"], "role");
            out.insert(
                n["props"]["nodeId"].as_str().unwrap().into(),
                (
                    e["id"].as_u64().unwrap(),
                    parent["props"]["entity"].as_u64().unwrap(),
                ),
            );
        }
    }
    out
}

#[test]
fn template_node_identity_and_hierarchy_survive_export_node_reordering() {
    let temp = Temp::new();
    let src = temp.0.join("actor.glb");
    let project = ForgeProject::with_defaults(temp.0.join("project"));
    let (mut doc, bin) = fixture(1., [1, 2, 3, 255]);
    doc["nodes"][1]["extras"]["rurixCollision"] = json!(true);
    std::fs::write(&src, glb(&doc, &bin)).unwrap();
    let first = import_model_bundle(&project, &src, &manifest(1)).unwrap();
    let old = template_node_ids(&project, &first.prefab_path);
    assert_eq!(old.len(), 3);
    assert_eq!(old["mesh-stable"].1, old["root-stable"].0);
    assert_eq!(old["bone-stable"].1, old["root-stable"].0);
    assert_eq!(old["root-stable"].1, 1);
    let nodes = doc["nodes"].as_array().unwrap().clone();
    doc["nodes"] = json!([nodes[2], nodes[0], nodes[1]]);
    doc["nodes"][1]["children"] = json!([2, 0]);
    doc["scenes"][0]["nodes"] = json!([1]);
    doc["skins"][0]["joints"] = json!([0]);
    doc["skins"][0]["skeleton"] = json!(0);
    for a in doc["animations"].as_array_mut().unwrap() {
        a["channels"][0]["target"]["node"] = json!(0);
    }
    std::fs::write(&src, glb(&doc, &bin)).unwrap();
    let second = import_model_bundle(&project, &src, &manifest(2)).unwrap();
    assert_eq!(template_node_ids(&project, &second.prefab_path), old);
    let loaded = load_model(&project, &first.guid).unwrap();
    assert!(
        loaded
            .nodes
            .iter()
            .find(|n| n.id == "mesh-stable")
            .unwrap()
            .collision
    );
}

#[test]
fn generic_asset_operations_list_validate_and_protect_model_dependencies() {
    let temp = Temp::new();
    let src = temp.0.join("actor.glb");
    write_glb(&src, 1., [1, 2, 3, 255]);
    let project = ForgeProject::with_defaults(temp.0.join("project"));
    let published = import_model_bundle(&project, &src, &manifest(1)).unwrap();
    let model = load_model(&project, &published.guid).unwrap();
    let listed = project.scan_content().unwrap();
    assert_eq!(assetd::ops::move_asset(&project,&published.asset_path,"Models/Moved",None).unwrap_err().code,"MODEL_MANAGED_ASSET");
    assert!(listed.contains(&published.asset_path));
    assert!(listed.contains(&published.prefab_path));
    assert_eq!(
        assetd::meta::MetaDoc::load(&assetd::meta_path_for(
            &project.content_root(),
            &published.asset_path
        ))
        .unwrap()
        .atype,
        "model"
    );
    // Do not prewarm asset_refs: deletion itself must discover references from a fresh publication.
    let deleted = assetd::ops::delete_assets(
        &project,
        &[
            published.asset_path.clone(),
            model.textures[0].asset_path.clone(),
        ],
        false,
    )
    .unwrap();
    assert!(deleted.deleted.is_empty());
    assert_eq!(deleted.blocked_by_refs.len(), 2);
    let scene = "Scenes/Reference.rxscene";
    std::fs::write(project.content_root().join(scene),serde_json::to_vec(&json!({"entities":[{"components":[{"type":"ModelRenderer","props":{"model":published.guid}},{"type":"ModelNode","props":{"model":published.guid,"nodeId":"mesh-stable"}},{"type":"PrefabInstance","props":{"prefabRef":published.prefab_guid,"model":published.guid}}]}]})).unwrap()).unwrap();
    assetd::meta::MetaDoc::new(scene, "saved-scene".into())
        .unwrap()
        .save(&assetd::meta_path_for(&project.content_root(), scene))
        .unwrap();
    let blocked =
        assetd::ops::delete_assets(&project, &[published.prefab_path.clone()], false).unwrap();
    assert_eq!(blocked.blocked_by_refs.len(), 1);
    let refs = assetd::refs::RefGraph::load(&project).unwrap();
    assert!(refs
        .refs("saved-scene")
        .iter()
        .any(|e| e.to_guid == published.guid && e.edge_type == "scene→model"));
    assert!(refs
        .refs("saved-scene")
        .iter()
        .any(|e| e.to_guid == published.prefab_guid));
    let paths = vec![published.asset_path.clone(), published.prefab_path.clone()];
    assert_eq!(
        assetd::ops::reimport_assets(&project, &paths).unwrap(),
        paths
    );
    assert!(assetd::status::build_status(&project, &paths)
        .unwrap()
        .iter()
        .all(|s| s.state == assetd::BuildState::Current));
    std::fs::write(
        project.content_root().join(&published.prefab_path),
        b"bad template JSON",
    )
    .unwrap();
    assert_eq!(
        assetd::status::build_status(&project, &[published.prefab_path.clone()]).unwrap()[0].state,
        assetd::BuildState::Stale
    );
    assert_eq!(
        assetd::ops::reimport_assets(&project, &[published.prefab_path.clone()])
            .unwrap_err()
            .code,
        "MODEL_SOURCE_MODIFIED"
    );
    // Republishing the same source repairs its damaged derivatives instead of returning a no-op.
    let repaired = import_model_bundle(&project, &src, &manifest(2)).unwrap();
    assert!(repaired.changed);
    assert_eq!(repaired.revision, 2);
    assetd::ops::reimport_assets(&project, &paths).unwrap();
}

#[test]
fn historical_model_revision_retains_removed_node_geometry_and_texture() {
    let temp = Temp::new();
    let src = temp.0.join("actor.glb");
    write_glb(&src, 1., [1, 2, 3, 255]);
    let project = ForgeProject::with_defaults(temp.0.join("project"));
    let first = import_model_bundle(&project, &src, &manifest(1)).unwrap();
    write_glb(&src, 4., [30, 40, 50, 255]);
    import_model_bundle(&project, &src, &manifest(2)).unwrap();
    let historical = assetd::model::load_model_revision(&project, &first.guid, 1).unwrap();
    let current = assetd::model::load_model_revision(&project, &first.guid, 2).unwrap();
    assert_eq!(historical.primitives[0].positions[1][0], 1.);
    assert_eq!(current.primitives[0].positions[1][0], 4.);
    assert_eq!(&historical.textures[0].rgba[..4], &[1, 2, 3, 255]);
    assert_ne!(historical.textures[0].rgba, current.textures[0].rgba);
    assert_eq!(
        assetd::model::load_model_revision(&project, &first.guid, 99)
            .unwrap_err()
            .code,
        "MODEL_REVISION_NOT_FOUND"
    );
}
