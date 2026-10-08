//! Stage 4 的 3D 抽取(01 §5):模型腿(ModelRenderer / ModelNode 与腿内的旧 MeshRenderer)、Light。
//! 遍历顺序、错误文本与 rurix `render_core::model::collect` / `legacy_draw` 相同,调用同一批中立函数
//! (`modelrt::load_revision` / `node_worlds` / `entity_world` / `inverse` / `from_cols`、`material_override::apply`),
//! 但不做 CPU 蒙皮:输出节点矩阵与骨骼矩阵(jointWorld × inverseBind),顶点留在模型空间,
//! 由 Godot 的 skeleton_* 在 GPU 上蒙皮(01 §5.5;两种做法的 profile 与选型见 02 §9.5 Stage 4 step 7)。
//! rurix 专有的 GPU 预算(MODEL_BUDGET:2048 draw / 256 MiB)不适用于 Godot 腿(Capabilities.max_draws = None)。

use std::sync::Arc;

use forge_scene::{Entity, Scene};
use serde_json::Value;

use super::list::{
    mesh_albedo_tex, tex_fp, ExtractStats, Fp, ItemBody, ItemKey, LightItem, LightKind, MeshData, MeshRef, ModelData,
    ModelPrim, RenderItem,
};
use super::math::{m4_mul, trs_model};
use crate::modelrt;

/// 模型腿全部 item(未排序,sub 由调用方统一分配)。
pub(super) fn model_items(
    scene: &Scene,
    selected: Option<u64>,
    root: &std::path::Path,
    stats: &mut ExtractStats,
) -> Result<Vec<RenderItem>, String> {
    let mut items = Vec::new();
    for (order, e) in scene.entities.iter().enumerate() {
        let order = order as u32;
        if let Some(c) = e.component("ModelRenderer").filter(|c| c.enabled) {
            model_renderer(scene, e, c, order, selected == Some(e.id), stats, &mut items)?;
        } else if let Some(sprite) = super::list::sprite_item(scene, e, order, selected == Some(e.id), root, true)? {
            stats.triangles += 2;
            items.push(sprite);
        } else if let Some(it) = legacy_item(scene, e, order, selected == Some(e.id), root, stats)? {
            items.push(it);
        }
    }
    Ok(items)
}

fn model_renderer(
    scene: &Scene,
    e: &Entity,
    c: &forge_scene::Component,
    order: u32,
    selected: bool,
    stats: &mut ExtractStats,
    items: &mut Vec<RenderItem>,
) -> Result<(), String> {
    let reference = c.props.get("model").and_then(|v| v.as_str()).ok_or("ModelRenderer.model missing")?;
    let model = modelrt::load_revision(reference, c.props.get("revision").and_then(|v| v.as_u64()).filter(|r| *r > 0))?;
    let a = modelrt::ancestor_component(scene, e, "Animator");
    let clip = a.and_then(|a| a.props.get("clip")).and_then(|v| v.as_str()).unwrap_or("");
    let time = a.and_then(|a| a.props.get("time")).and_then(|v| v.as_f64()).unwrap_or(0.) as f32;
    let looped = a.and_then(|a| a.props.get("loop")).and_then(|v| v.as_bool()).unwrap_or(true);
    let worlds = modelrt::node_worlds(&model, clip, time, looped)?;
    let node_id = c.props.get("nodeId").and_then(|v| v.as_str()).unwrap_or("");
    let selected_node = if node_id.is_empty() {
        None
    } else {
        Some(model.nodes.iter().position(|n| n.id == node_id).ok_or_else(|| format!("MODEL_NODE_NOT_FOUND: {node_id}"))?)
    };
    let world = modelrt::entity_world(scene, e)?;
    let world = if let Some(ni) = selected_node {
        let rest = modelrt::node_worlds(&model, "", 0., false)?;
        m4_mul(world, modelrt::inverse(rest[ni])?)
    } else {
        world
    };
    let data = ModelData { key: format!("{}:{}:{}", model.guid, model.revision, model.source_hash), bundle: Arc::clone(&model) };
    let overrides = c.props.get("materialOverrides").unwrap_or(&Value::Null);
    let mut stack = selected_node.map(|i| vec![i]).unwrap_or_else(|| model.roots.clone());
    let mut visited = std::collections::HashSet::new();
    while let Some(ni) = stack.pop() {
        if !visited.insert(ni) {
            continue;
        }
        let n = model.nodes.get(ni).ok_or("root node out of range")?;
        if selected_node.is_none() {
            stack.extend(&n.children);
        }
        for &pi in &n.primitives {
            let p = model.primitives.get(pi).ok_or("primitive out of range")?;
            let default = super::model::default_material();
            let m = p.material.and_then(|i| model.materials.get(i)).unwrap_or(&default);
            let m = crate::material_override::apply(m, overrides, p.material.unwrap_or(0))?;
            let graph=crate::shader::model(e,&p.material.unwrap_or(0).to_string())?;
            let material_fp = crate::meshres::fnv1a64(&serde_json::to_vec(&m).unwrap_or_default()) ^ graph.as_ref().map_or(0,|g|crate::meshres::fnv1a64(g.key.as_bytes()));
            // 与 modelrt::vertices 同式:非蒙皮 = entity × worlds[node];蒙皮 = entity × Σ w·(worlds[j] × IB)。
            let (item_world, pose) = match n.skin {
                Some(si) => {
                    let s = model.skins.get(si).ok_or("skin index out of range")?;
                    if s.joints.len() != s.inverse_bind_matrices.len() {
                        return Err("skin joint/bind count mismatch".into());
                    }
                    let palette = s
                        .joints
                        .iter()
                        .zip(&s.inverse_bind_matrices)
                        .map(|(&j, &ib)| worlds.get(j).map(|&w| m4_mul(w, modelrt::from_cols(ib))).ok_or("joint node out of range"))
                        .collect::<Result<Vec<_>, _>>()?;
                    (world, Some(Arc::new(palette)))
                }
                None => (m4_mul(world, worlds[ni]), None),
            };
            stats.triangles += p.indices.len() / 3;
            stats.model_prims += 1;
            stats.skinned_prims += usize::from(n.skin.is_some());
            let content = Fp::new("model")
                .s(&data.key)
                .u(ni as u64)
                .u(pi as u64)
                .u(material_fp)
                .u(n.skin.map_or(u64::MAX, |s| s as u64))
                .u(u64::from(selected))
                .done();
            let body = ItemBody::Model(ModelPrim {
                model: data.clone(),
                node: ni,
                prim: pi,
                material: Arc::new(m),
                material_fp,
                graph,
                skin: n.skin,
                selected,
            });
            items.push(RenderItem { key: ItemKey { entity: e.id, sub: 0 }, order, world: item_world, content, body, pose });
        }
    }
    Ok(())
}


/// 模型腿里的非 ModelRenderer 实体(rurix `legacy_draw` 同判据):
/// Sprite 能解析 → 跳过(Stage 6);MeshRenderer + albedo 贴图 → LegacyQuad;其余 MeshRenderer → LegacyMesh(缺省 PBR)。
fn legacy_item(
    scene: &Scene,
    e: &Entity,
    order: u32,
    selected: bool,
    root: &std::path::Path,
    stats: &mut ExtractStats,
) -> Result<Option<RenderItem>, String> {
    let key = ItemKey { entity: e.id, sub: 0 };
    let sprite = e.component("Sprite").filter(|c| c.enabled);
    if let Some(sp) = sprite {
        if super::sprite::resolve_sprite_render(sp, root).is_some() {
            stats.skipped_sprites += 1;
            return Ok(None);
        }
    }
    let Some(c) = e.component("MeshRenderer").filter(|c| c.enabled) else {
        return Ok(None);
    };
    if let Some(graph)=crate::shader::mesh(e)?{let vertices=Arc::new(crate::shader::mesh_vertices(e)?);let mut fingerprint=graph.key.as_bytes().to_vec();for value in vertices.iter(){fingerprint.extend_from_slice(&value.to_le_bytes());}let content=crate::meshres::fnv1a64(&fingerprint);stats.triangles+=vertices.len()/36;return Ok(Some(RenderItem{key,order,world:modelrt::entity_world(scene,e)?,content,body:ItemBody::GraphMesh{vertices,graph},pose:None}));}
    // legacy_draw:有启用的 Sprite(即使没解析出来)就不走 MeshRenderer.material 的贴图分支。
    if let Some(tex) = sprite.is_none().then(|| mesh_albedo_tex(e, root)).flatten() {
        // legacy_draw:world = entity_world × inverse(trs(本地)) × trs(渲染态)(非 Sprite 的渲染态就是本地 transform)。
        let local = super::sprite::sprite_render_transform(e).unwrap_or(e.transform);
        let world = m4_mul(modelrt::entity_world(scene, e)?, m4_mul(modelrt::inverse(trs_model(&e.transform))?, trs_model(&local)));
        stats.triangles += 2;
        let tint = [1.0f32; 4];
        let content = tex_fp("legacy-quad", &tex, &[tint[0], tint[1], tint[2], tint[3], f32::from(u8::from(selected))]);
        return Ok(Some(RenderItem { key, order, world, content, body: ItemBody::LegacyQuad { tex, tint, selected }, pose: None }));
    }
    let reference = c.props.get("mesh").and_then(|v| v.as_str()).unwrap_or("cube");
    let mesh = if reference == "cube" || reference.is_empty() {
        let v = super::assets::cube_mesh_bytes();
        MeshData { id: MeshRef::Cube, vertices: v, vertex_count: (v.len() / 24) as u32, fallback: false }
    } else {
        // rurix legacy_draw 用 load_mesh_cached(...)?:解析失败整帧报错(模型腿不回退 cube),这里同样传播。
        let m = crate::meshres::load_mesh_static_cached(root, reference)?;
        MeshData { id: MeshRef::Asset { reference: reference.to_string() }, vertices: &m.bytes, vertex_count: m.vertex_count, fallback: false }
    };
    let world = modelrt::entity_world(scene, e)?;
    stats.triangles += mesh.vertex_count as usize / 3;
    let id = match &mesh.id {
        MeshRef::Cube => "cube".to_string(),
        MeshRef::Asset { reference } => format!("asset:{reference}"),
    };
    let content = Fp::new("legacy-mesh").s(&id).u(u64::from(selected)).done();
    Ok(Some(RenderItem { key, order, world, content, body: ItemBody::LegacyMesh { mesh, selected }, pose: None }))
}

fn f32x3(v: Option<&Value>) -> Option<[f32; 3]> {
    let a = v?.as_array()?;
    if a.len() != 3 {
        return None;
    }
    let mut out = [0.0f32; 3];
    for (o, x) in out.iter_mut().zip(a) {
        *o = x.as_f64()? as f32;
    }
    Some(out)
}

/// 启用的 Light 实体(01 §5.3)。rurix 两条腿都不读 Light(02 §3.7),这里给 Godot 腿用;
/// 世界矩阵走 Parent 链(modelrt::entity_world;环 → 退回本地 transform)。
pub(super) fn lights(scene: &Scene, stats: &mut ExtractStats) -> Vec<LightItem> {
    let mut out = Vec::new();
    for e in &scene.entities {
        let Some(c) = e.component("Light").filter(|c| c.enabled) else {
            continue;
        };
        let kind_s = c.props.get("kind").and_then(Value::as_str).unwrap_or("directional");
        let (kind, kind_known) = match kind_s {
            "directional" => (LightKind::Directional, true),
            "point" => (LightKind::Point, true),
            "spot" => (LightKind::Spot, true),
            _ => (LightKind::Directional, false),
        };
        stats.unknown_light_kinds += usize::from(!kind_known);
        out.push(LightItem {
            key: ItemKey { entity: e.id, sub: 0 },
            kind,
            kind_known,
            color: f32x3(c.props.get("color")).unwrap_or([1.0; 3]),
            intensity: c.props.get("intensity").and_then(Value::as_f64).unwrap_or(1.0) as f32,
            cast_shadow: c.props.get("castShadow").and_then(Value::as_bool).unwrap_or(false),
            world: modelrt::entity_world(scene, e).unwrap_or_else(|_| trs_model(&e.transform)),
            params: e.component("LightParams").filter(|p| p.enabled).map(|p| super::env::Props::of("LightParams", p)),
        });
    }
    out.sort_by_key(|l| l.key.entity);
    for i in 1..out.len() {
        if out[i].key.entity == out[i - 1].key.entity {
            out[i].key.sub = out[i - 1].key.sub + 1;
        }
    }
    stats.lights = out.len();
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::render_core::list::{extract, LightKind, Leg, MODEL_CLEAR_RGBA};
    use assetd::model::*;
    use forge_scene::{Component, Transform};

    fn node(id: &str, prims: Vec<usize>, t: [f32; 3], skin: Option<usize>, children: Vec<usize>) -> ModelNode {
        ModelNode { id: id.into(), name: id.into(), children, primitives: prims, translation: t, rotation: [0., 0., 0., 1.],
                    scale: [1.; 3], matrix: None, skin, collision: false }
    }

    /// 两个 primitive:0 号挂在带蒙皮的 "skinned" 节点(关节 = "bone"),1 号挂在普通子节点 "prop"。
    pub(crate) fn fixture(guid: &str) -> ModelBundle {
        let mut mat = super::super::model::default_material();
        mat.guid = "m0".into();
        let quad = |id: &str| ModelPrimitive {
            id: id.into(),
            positions: vec![[-0.5, -0.5, 0.], [0.5, -0.5, 0.], [0.5, 0.5, 0.], [-0.5, 0.5, 0.]],
            normals: vec![[0., 0., 1.]; 4], tangents: vec![[1., 0., 0., 1.]; 4],
            uv0: vec![[0., 1.], [1., 1.], [1., 0.], [0., 0.]], indices: vec![0, 1, 2, 0, 2, 3],
            joints: vec![[0, 0, 0, 0]; 4], weights: vec![[1., 0., 0., 0.]; 4], material: Some(0),
        };
        let ib = [1., 0., 0., 0., 0., 1., 0., 0., 0., 0., 1., 0., -0.25, 0., 0., 1.];
        ModelBundle {
            version: 1, guid: guid.into(), revision: 1, name: "fixture".into(), source_id: "t".into(), source_hash: "h".into(),
            kind: "prop".into(), idle_clip: String::new(), walk_clip: String::new(), roots: vec![0, 1],
            primitives: vec![quad("a"), quad("b")],
            nodes: vec![
                node("skinned", vec![0], [0.; 3], Some(0), vec![]),
                node("bone", vec![], [0.25, 0., 0.], None, vec![2]),
                node("prop", vec![1], [0., 1., 0.], None, vec![]),
            ],
            materials: vec![mat], textures: vec![],
            skins: vec![ModelSkin { name: "s".into(), joints: vec![1], inverse_bind_matrices: vec![ib], skeleton: Some(1) }],
            animations: vec![ModelAnimation { name: "walk".into(), duration: 2., channels: vec![ModelAnimationChannel {
                node: 1, path: "translation".into(), times: vec![0., 2.], values: vec![[0.25, 0., 0., 0.], [1.25, 0., 0., 0.]],
                interpolation: "LINEAR".into() }] }],
        }
    }

    fn entity(id: u64, comps: Vec<Component>, t: [f32; 3]) -> Entity {
        Entity { entity_guid: None, id, name: format!("e{id}"), transform: Transform { translation: t, ..Transform::default() }, components: comps }
    }

    fn snap(scene: Scene, selected: Option<u64>) -> crate::render::snapshot::RenderSnapshot {
        let mut st = crate::rpc::HostState::new();
        st.scene = scene;
        crate::render::snapshot(&st, crate::render::SnapshotParams { scene_camera: false,
            width: 64, height: 64, selected, want_readback: true, want_stats: true,
            requester: crate::render::FrameRequester::ViewportFrame,
        }, 3)
    }

    fn pos(v: &[u8], i: usize) -> [f32; 3] {
        std::array::from_fn(|k| f32::from_le_bytes(v[i * 48 + k * 4..i * 48 + k * 4 + 4].try_into().unwrap()))
    }

    #[test]
    fn model_matrices_reproduce_rurix_collect_vertices() {
        let model = fixture("test-extract3d-model");
        modelrt::prime(model.clone());
        let mut s = Scene::new("m");
        s.entities.push(entity(1, vec![Component::new("Parent", serde_json::json!({"entity": 2}))], [0.; 3]));
        s.entities[0].components.push(Component::new("ModelRenderer", serde_json::json!({"model": model.guid,
            "materialOverrides": {"0": {"baseColor": [1.0, 0.0, 0.0, 1.0]}}})));
        s.entities[0].components.push(Component::new("Animator", serde_json::json!({"clip": "walk", "time": 1.0, "loop": true})));
        s.entities.push(entity(2, vec![], [3., 0., 0.]));
        let snap = snap(s, Some(1));
        let l = extract(&snap).unwrap();
        assert_eq!((l.leg, l.clear_rgba, l.items.len()), (Leg::Model, MODEL_CLEAR_RGBA, 2));
        assert_eq!((l.stats.model_prims, l.stats.skinned_prims, l.stats.triangles), (2, 1, 4));
        let draws = super::super::model::collect(&snap.scene, [0.; 3], None).unwrap();
        for (it, d) in l.items.iter().zip(&draws) {
            let ItemBody::Model(p) = &it.body else { panic!() };
            assert!(p.selected && p.material.base_color == [1.0, 0.0, 0.0, 1.0], "覆盖 + 选中");
            let prim = &model.primitives[p.prim];
            for (k, &idx) in prim.indices.iter().enumerate() {
                let local = prim.positions[idx as usize];
                let got = match &it.pose {
                    None => modelrt::point(it.world, local),
                    Some(pal) => modelrt::point(m4_mul(it.world, pal[0]), local),
                };
                let want = pos(&d.vertices, k);
                let tol = if it.pose.is_some() { 1e-5 } else { 0.0 };
                assert!((0..3).all(|c| (got[c] - want[c]).abs() <= tol), "prim {} v{k}: {got:?} vs {want:?}", p.prim);
            }
        }
    }

    #[test]
    fn node_id_selects_subtree_and_lights_follow_parent_chain() {
        let model = fixture("test-extract3d-node");
        modelrt::prime(model.clone());
        let mut s = Scene::new("m");
        s.entities.push(entity(1, vec![Component::new("ModelRenderer", serde_json::json!({"model": model.guid, "nodeId": "prop"}))], [1., 0., 0.]));
        s.entities.push(entity(5, vec![Component::new("Light", serde_json::json!({"kind": "spot", "color": [1.0, 0.5, 0.25], "intensity": 2.0, "castShadow": true})),
                                       Component::new("Parent", serde_json::json!({"entity": 1}))], [0., 2., 0.]));
        s.entities.push(entity(4, vec![Component::new("Light", serde_json::json!({"kind": "area"}))], [0.; 3]));
        let l = extract(&snap(s, None)).unwrap();
        assert_eq!(l.items.len(), 1, "nodeId 只取该节点(不含子节点)");
        let ItemBody::Model(p) = &l.items[0].body else { panic!() };
        assert_eq!((p.node, p.prim, p.selected), (2, 1, false));
        // world = entity × inverse(rest[prop]) × worlds[prop] = entity(平移 1,0,0)。
        assert!((l.items[0].world[0][3] - 1.0).abs() < 1e-6 && l.items[0].world[1][3].abs() < 1e-6);
        assert_eq!(l.lights.iter().map(|x| (x.key.entity, x.kind, x.kind_known)).collect::<Vec<_>>(),
                   [(4, LightKind::Directional, false), (5, LightKind::Spot, true)]);
        assert_eq!((l.stats.lights, l.stats.unknown_light_kinds), (2, 1));
        let spot = &l.lights[1];
        assert_eq!((spot.color, spot.intensity, spot.cast_shadow), ([1.0, 0.5, 0.25], 2.0, true));
        assert_eq!((spot.world[0][3], spot.world[1][3]), (1.0, 2.0), "灯走 Parent 链");
    }

    /// 蒙皮方案 profile(Stage 4 step 7,02 §9.5):同一个 150×150 顶点、8 骨骼链的蒙皮网格,
    /// GPU 蒙皮只需每帧抽姿态(extract:node_worlds + 调色板),CPU 蒙皮要每帧把全部顶点变换一遍(= rurix collect)。
    /// `cargo test -p engine-host --lib -- --ignored skinning_profile --nocapture`
    #[test]
    #[ignore]
    fn skinning_profile() {
        let n = 150usize;
        let bones = 8usize;
        let mut m = fixture("test-skin-profile");
        let mut prim = m.primitives[0].clone();
        prim.positions.clear();
        prim.normals.clear();
        prim.tangents.clear();
        prim.uv0.clear();
        prim.joints.clear();
        prim.weights.clear();
        prim.indices.clear();
        for y in 0..n {
            for x in 0..n {
                let (u, v) = (x as f32 / (n - 1) as f32, y as f32 / (n - 1) as f32);
                prim.positions.push([u * 4.0, v, 0.0]);
                prim.normals.push([0.0, 0.0, 1.0]);
                prim.tangents.push([1.0, 0.0, 0.0, 1.0]);
                prim.uv0.push([u, v]);
                let b = ((u * (bones - 1) as f32).floor() as usize).min(bones - 2);
                let t = u * (bones - 1) as f32 - b as f32;
                prim.joints.push([b as u16, b as u16 + 1, 0, 0]);
                prim.weights.push([1.0 - t, t, 0.0, 0.0]);
            }
        }
        for y in 0..n - 1 {
            for x in 0..n - 1 {
                let i = (y * n + x) as u32;
                prim.indices.extend([i, i + 1, i + n as u32 + 1, i, i + n as u32 + 1, i + n as u32]);
            }
        }
        m.primitives = vec![prim];
        m.nodes.truncate(1);
        m.nodes[0].children.clear();
        m.roots = vec![0];
        let first = m.nodes.len();
        for b in 0..bones {
            m.nodes.push(node(&format!("b{b}"), vec![], [if b == 0 { 0.0 } else { 4.0 / (bones - 1) as f32 }, 0.0, 0.0], None,
                              if b + 1 < bones { vec![first + b + 1] } else { vec![] }));
        }
        m.roots.push(first);
        let joints: Vec<usize> = (first..first + bones).collect();
        let ib: Vec<[f32; 16]> = (0..bones)
            .map(|b| [1., 0., 0., 0., 0., 1., 0., 0., 0., 0., 1., 0., -(b as f32) * 4.0 / (bones - 1) as f32, 0., 0., 1.])
            .collect();
        m.skins = vec![ModelSkin { name: "s".into(), joints: joints.clone(), inverse_bind_matrices: ib, skeleton: Some(first) }];
        m.animations = vec![ModelAnimation { name: "walk".into(), duration: 2.0, channels: joints.iter().map(|&j| ModelAnimationChannel {
            node: j, path: "rotation".into(), times: vec![0., 1., 2.],
            values: vec![[0., 0., 0., 1.], [0., 0., 0.2588, 0.9659], [0., 0., 0., 1.]], interpolation: "LINEAR".into() }).collect() }];
        modelrt::prime(m.clone());
        let mut s = Scene::new("profile");
        s.entities.push(entity(1, vec![Component::new("ModelRenderer", serde_json::json!({"model": m.guid})),
                                       Component::new("Animator", serde_json::json!({"clip": "walk", "time": 0.3, "loop": true}))], [0.; 3]));
        let snap = snap(s, None);
        let iters = 20u32;
        let t0 = std::time::Instant::now();
        let mut bones_bytes = 0usize;
        for _ in 0..iters {
            let l = extract(&snap).unwrap();
            bones_bytes = l.items.iter().filter_map(|i| i.pose.as_ref()).map(|p| p.len() * 12 * 4).sum();
        }
        let gpu_ms = t0.elapsed().as_secs_f64() * 1000.0 / iters as f64;
        let t1 = std::time::Instant::now();
        let mut vertex_bytes = 0usize;
        for _ in 0..iters {
            let d = super::super::model::collect(&snap.scene, [0.; 3], None).unwrap();
            vertex_bytes = d.iter().map(|d| d.vertices.len()).sum();
        }
        let cpu_ms = t1.elapsed().as_secs_f64() * 1000.0 / iters as f64;
        eprintln!("SKINNING_PROFILE vertices={} triangles={} bones={bones} gpu_path(extract)={gpu_ms:.3}ms/frame upload={bones_bytes}B \
                   cpu_path(collect)={cpu_ms:.3}ms/frame upload={vertex_bytes}B",
                  n * n, 2 * (n - 1) * (n - 1));
        assert!(cpu_ms > gpu_ms, "CPU 蒙皮应比只抽姿态贵");
    }
}
