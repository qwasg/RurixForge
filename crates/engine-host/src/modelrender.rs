//! Standard 3D assets use a dedicated UV/PBR pipeline. Legacy-only scenes stay on viewport's path.
#[cfg_attr(not(feature = "backend-rurix"), allow(unused_imports))]
use crate::{
    render_core::{
        camera::model_eye,
        model::{collect, Draw, PACKED},
    },
    viewport::{self, EditorCamera, FramePixels, M4},
};
pub use crate::render_core::model::bounds;
#[cfg(feature = "backend-rurix")]
use forge_scene::Scene;
#[cfg(feature = "backend-rurix")]
use rurix_rt::{render_exec as rex, vk};
#[cfg(feature = "backend-rurix")]
use std::sync::{Arc, Mutex, OnceLock};
// rurix 模型腿(PBR 会话、WGSL、逐帧上传)在 modelrender/rurix.rs(02 §5.2),经 `use super::*` 共用上面的导入;
// CPU 部分(collect / bounds / pick)在 render_core::model。
#[cfg(feature = "backend-rurix")]
mod rurix;
#[cfg(feature = "backend-rurix")]
pub use rurix::render;
pub fn invalidate() {
    // rurix 部分:丢弃模型腿会话(与拆分前同一顺序:先会话,再打包贴图缓存)。
    #[cfg(feature = "backend-rurix")]
    rurix::reset_state();
    if let Some(p) = PACKED.get() {
        p.lock().unwrap().clear();
    }
}
#[cfg(all(test, feature = "backend-rurix"))]
mod tests {
    use super::*;
    use super::rurix::*;
    use assetd::model::*;
    use crate::render_core::model::{charge, default_material, pc, pick, MAX_VERTEX_BYTES};
    use forge_scene::{Component, Entity, Transform};
    fn fixture() -> ModelBundle {
        let mut mat = default_material();
        mat.base_color = [1.; 4];
        mat.base_color_texture = Some(0);
        mat.unlit = true;
        let identity = [
            1., 0., 0., 0., 0., 1., 0., 0., 0., 0., 1., 0., 0., 0., 0., 1.,
        ];
        ModelBundle {
            version: 1,
            guid: "test-model-gpu-uv-skin".into(),
            revision: 1,
            name: "uv skin fixture".into(),
            source_id: "self-contained-test".into(),
            source_hash: "first".into(),
            kind: "role".into(),
            idle_clip: "walk".into(),
            walk_clip: "walk".into(),
            roots: vec![0, 1],
            primitives: vec![ModelPrimitive {
                id: "quad".into(),
                positions: vec![
                    [-0.7, -0.7, 0.],
                    [0.7, -0.7, 0.],
                    [0.7, 0.7, 0.],
                    [-0.7, 0.7, 0.],
                ],
                normals: vec![[0., 0., 1.]; 4],
                tangents: vec![[1., 0., 0., 1.]; 4],
                uv0: vec![[0., 1.], [1., 1.], [1., 0.], [0., 0.]],
                indices: vec![0, 1, 2, 0, 2, 3],
                joints: vec![[0, 0, 0, 0]; 4],
                weights: vec![[1., 0., 0., 0.]; 4],
                material: Some(0),
            }],
            nodes: vec![
                ModelNode {
                    id: "mesh".into(),
                    name: "mesh".into(),
                    children: vec![],
                    primitives: vec![0],
                    translation: [0.; 3],
                    rotation: [0., 0., 0., 1.],
                    scale: [1.; 3],
                    matrix: None,
                    skin: Some(0),
                    collision: false,
                },
                ModelNode {
                    id: "bone".into(),
                    name: "bone".into(),
                    children: vec![],
                    primitives: vec![],
                    translation: [0.; 3],
                    rotation: [0., 0., 0., 1.],
                    scale: [1.; 3],
                    matrix: None,
                    skin: None,
                    collision: false,
                },
            ],
            materials: vec![mat],
            textures: vec![ModelTexture {
                id: "checker".into(),
                guid: "checker".into(),
                asset_path: "".into(),
                width: 2,
                height: 2,
                rgba: vec![
                    255, 0, 0, 255, 0, 255, 0, 255, 0, 0, 255, 255, 255, 255, 0, 255,
                ],
                wrap_s: 33071,
                wrap_t: 33071,
                mag_filter: Some(9729),
                min_filter: Some(9729),
            }],
            skins: vec![ModelSkin {
                name: "skin".into(),
                joints: vec![1],
                inverse_bind_matrices: vec![identity],
                skeleton: Some(1),
            }],
            animations: vec![ModelAnimation {
                name: "walk".into(),
                duration: 2.,
                channels: vec![ModelAnimationChannel {
                    node: 1,
                    path: "translation".into(),
                    times: vec![0., 2.],
                    values: vec![[0., 0., 0., 0.], [1., 0., 0., 0.]],
                    interpolation: "LINEAR".into(),
                }],
            }],
        }
    }
    #[test]
    fn gpu_uv_skin_instances_and_same_size_reload() {
        let model = fixture();
        crate::modelrt::prime(model.clone());
        let mut scene = Scene::new("gpu-fixture");
        scene.entities.push(Entity { entity_guid: None,
            id: 1,
            name: "animated".into(),
            transform: Transform::default(),
            components: vec![
                Component::new("ModelRenderer", serde_json::json!({"model":model.guid})),
                Component::new(
                    "Animator",
                    serde_json::json!({"clip":"walk","time":0,"loop":false}),
                ),
            ],
        });
        let cam = EditorCamera {
            target: [0.; 3],
            yaw_deg: 0.,
            pitch_deg: 0.,
            dist: 3.,
            ..Default::default()
        };
        let a = render(&scene, &cam, None, 128, 128, true, true, None)
            .expect("real GPU required for model acceptance");
        assert!(a.nonzero > 100);
        assert_eq!(a.triangles, 2);
        assert_eq!(a.mesh_fallbacks, 0);
        let red = a
            .rgba8
            .chunks_exact(4)
            .filter(|p| {
                u16::from(p[0]) * 2 > u16::from(p[1]) * 3
                    && u16::from(p[0]) * 2 > u16::from(p[2]) * 3
            })
            .count();
        let blue = a
            .rgba8
            .chunks_exact(4)
            .filter(|p| {
                u16::from(p[2]) > u16::from(p[0]) * 2 && u16::from(p[2]) > u16::from(p[1]) * 2
            })
            .count();
        assert!(
            red > 10 && blue > 10,
            "UV checker must preserve distinct colored regions red={red} blue={blue}"
        );
        scene.entities[0].component_mut("Animator").unwrap().props["time"] = serde_json::json!(1.);
        let b = render(&scene, &cam, None, 128, 128, true, true, None).unwrap();
        assert_ne!(
            a.rgba8, b.rgba8,
            "joint animation must move actual vertices"
        );
        let mut second = scene.entities[0].clone();
        second.id = 2;
        second.transform.translation = [-1., 0., 0.];
        second.component_mut("Animator").unwrap().props["time"] = serde_json::json!(0.);
        scene.entities.push(second);
        let c = render(&scene, &cam, None, 128, 128, true, true, None).unwrap();
        assert_eq!(c.draws, 2);
        assert_eq!(c.triangles, 4);
        let mut changed = model;
        changed.revision = 2;
        changed.source_hash = "changed-same-size".into();
        changed.textures[0].rgba = vec![
            255, 0, 255, 255, 255, 0, 255, 255, 255, 0, 255, 255, 255, 0, 255, 255,
        ];
        crate::modelrt::prime(changed);
        let d = render(&scene, &cam, None, 128, 128, true, true, None).unwrap();
        assert_ne!(c.rgba8, d.rgba8, "same-size texture revision must refresh");
        assert!(
            d.rgba8
                .chunks_exact(4)
                .any(|p| p[0] > 100 && p[2] > 100 && p[1] < 20),
            "PBR must retain purple; no sprite chroma key"
        );
        eprintln!("MODEL_GPU_ACCEPTANCE device={} draws={} triangles={} nonzero={} UV_colors=({}, {}) animation_changed=true revision_changed=true",d.device_name,d.draws,d.triangles,d.nonzero,red,blue);
        // Explicit nearest sampler keeps the four texel colors (plus clear), not interpolated colors.
        let mut nearest = fixture();
        nearest.revision = 3;
        nearest.textures[0].mag_filter = Some(9728);
        crate::modelrt::prime(nearest);
        scene.entities.truncate(1);
        scene.entities[0].component_mut("Animator").unwrap().props["time"] = serde_json::json!(0.);
        let nearest_frame = render(&scene, &cam, None, 128, 128, true, true, None).unwrap();
        let unique = nearest_frame
            .rgba8
            .chunks_exact(4)
            .map(|p| p.to_vec())
            .collect::<std::collections::HashSet<_>>();
        assert_eq!(
            unique.len(),
            5,
            "nearest sampler must preserve four exact texels plus background"
        );
        // Two PBR layers are composited on GPU with alpha-over, in depth order.
        let mut transparent = fixture();
        transparent.revision = 4;
        transparent.materials[0].base_color_texture = None;
        transparent.materials[0].base_color = [1., 0., 0., 0.5];
        transparent.materials[0].alpha_mode = "BLEND".into();
        crate::modelrt::prime(transparent);
        let mut background = fixture();
        background.guid = "test-model-blue-background".into();
        background.materials[0].base_color_texture = None;
        background.materials[0].base_color = [0., 0., 1., 1.];
        crate::modelrt::prime(background.clone());
        let mut blue_entity = scene.entities[0].clone();
        blue_entity.id = 3;
        blue_entity.transform.translation = [0., 0., -0.1];
        blue_entity.component_mut("ModelRenderer").unwrap().props["model"] =
            serde_json::json!(background.guid);
        scene.entities.push(blue_entity);
        let blended = render(&scene, &cam, None, 128, 128, true, true, None).unwrap();
        let center = &blended.rgba8[(64 * 128 + 64) * 4..(64 * 128 + 64) * 4 + 4];
        assert!(
            center[0] > 70 && center[2] > 70 && center[1] < 20,
            "GPU alpha blend red over blue: {center:?}"
        );
        // More than the old 8 mesh classes / 128 draw ceiling remain visible and never become cubes.
        let mut many = Scene::new("many");
        for i in 0..129 {
            let mut e = scene.entities[1].clone();
            e.id = i + 100;
            e.transform.translation = [(i % 13) as f32 * 0.02, 0., -0.1];
            many.entities.push(e);
        }
        let dense = render(&many, &cam, None, 64, 64, true, true, None).unwrap();
        assert_eq!(dense.draws, 129);
        assert_eq!(dense.triangles, 258);
        assert!(!dense.truncated);
        eprintln!("MODEL_GPU_EXTENDED alpha_center={center:?} nearest_colors={} draws_over_128={} fallbacks={}",unique.len(),dense.draws,dense.mesh_fallbacks);
        invalidate();
    }
    #[test]
    fn shaders_compile() {
        shader().unwrap();
    }
    #[test]
    fn editable_node_transforms_match_visuals_and_static_mesh_collision() {
        let mut model = fixture();
        model.guid = "test-map-node-proxy".into();
        model.kind = "map".into();
        model.nodes[0].skin = None;
        model.nodes[0].translation = [1., 0., 0.];
        model.nodes[0].collision = true;
        model.nodes[1].primitives = vec![0];
        model.nodes[1].translation = [100., 0., 0.];
        crate::modelrt::prime(model.clone());
        let mut scene = Scene::new("map-proxy");
        scene.entities = vec![
            Entity { entity_guid: None,
                id: 1,
                name: "map".into(),
                transform: Transform {
                    translation: [2., 0., 0.],
                    ..Default::default()
                },
                components: vec![Component::new(
                    "Collider",
                    serde_json::json!({"shape":"mesh","model":model.guid}),
                )],
            },
            Entity { entity_guid: None,
                id: 2,
                name: "child".into(),
                transform: Transform {
                    translation: [1., 0., 0.],
                    ..Default::default()
                },
                components: vec![
                    Component::new("Parent", serde_json::json!({"entity":1})),
                    Component::new(
                        "ModelNode",
                        serde_json::json!({"model":model.guid,"nodeId":"mesh"}),
                    ),
                    Component::new(
                        "ModelRenderer",
                        serde_json::json!({"model":model.guid,"nodeId":"mesh"}),
                    ),
                ],
            },
        ];
        assert!(
            (bounds(&scene).unwrap().0[0] - 3.).abs() < 1e-5,
            "source local transform must be applied once"
        );
        scene.entities[1].transform.translation = [5., 0., 0.];
        assert!((bounds(&scene).unwrap().0[0] - 7.).abs() < 1e-5);
        let body = crate::character::body_desc(&scene, &scene.entities[0])
            .unwrap()
            .unwrap();
        let rurix_physics::ShapeDesc::StaticMesh {
            vertices,
            triangles,
        } = body.shape
        else {
            panic!("expected static mesh")
        };
        assert_eq!(
            triangles.len(),
            2,
            "unmarked helper node excluded from collision"
        );
        assert!(
            (vertices[0][0] - 6.3).abs() < 1e-5,
            "collider follows edited node, got {:?}",
            vertices[0]
        );
        assert_eq!(
            pick(&scene, [7., 0., 3.], [0., 0., -1.]).unwrap().0,
            2,
            "picking selects actual model child"
        );
    }
    #[test]
    fn budgets_fail_before_growth() {
        let mut used = MAX_VERTEX_BYTES - 4;
        assert!(charge(&mut used, 8, MAX_VERTEX_BYTES, "vertex")
            .unwrap_err()
            .contains("MODEL_BUDGET"));
        assert_eq!(used, MAX_VERTEX_BYTES - 4);
    }
    #[test]
    fn material_pc_layout() {
        assert_eq!(pc(&default_material(), [0.; 3], false).len(), 80);
    }
}

