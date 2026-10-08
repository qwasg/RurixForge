//! 金值单测(02 §3.8 第 2 条):锁定 CPU 渲染逻辑的逐位结果。
//!
//! - 期望值由**搬迁前**的现有实现算出(`f32::to_bits`),断言一律比较位模式,不用近似。
//! - 被测函数一律经**原路径**调用(`crate::viewport::…`、`crate::modelrender::bounds`);
//!   搬迁进 render_core 之后这些路径是 re-export,本文件不改一行仍须全绿。
//! - 覆盖:透视 / 正交、2d / 3d 场景、非方形宽高比(横 / 竖)、Parent 链实体。
//! - 精灵图集夹具:金值是用 viewport 测试的 test-v5-* 夹具抓的;之后改用逐字节相同、只换 GUID 的
//!   私有副本 golden-*(原因见 `sprite_fixture`),金值表未改、仍全绿,即两套夹具输出逐位相同。

use forge_scene::{Component, Entity, Scene, Transform};
use serde_json::{json, Value};

use crate::viewport::EditorCamera;

/// 一组金值:(用例标签, 位模式序列)。`None` 记为空序列(同一标签的 `Some` 长度恒定,无歧义)。
type Out = Vec<(String, Vec<u32>)>;

fn b3(v: [f32; 3]) -> Vec<u32> {
    v.iter().map(|f| f.to_bits()).collect()
}

fn b4x4(m: [[f32; 4]; 4]) -> Vec<u32> {
    m.iter().flatten().map(|f| f.to_bits()).collect()
}

fn bray(r: ([f32; 3], [f32; 3])) -> Vec<u32> {
    let mut v = b3(r.0);
    v.extend(b3(r.1));
    v
}

fn btr(t: &Transform) -> Vec<u32> {
    t.translation.iter().chain(&t.rotation).chain(&t.scale).map(|f| f.to_bits()).collect()
}

fn check(actual: Out, expected: &[(&str, &[u32])]) {
    assert_eq!(actual.len(), expected.len(), "golden case count");
    for ((al, ab), (el, eb)) in actual.iter().zip(expected) {
        assert_eq!(al, el, "golden case order");
        assert_eq!(ab.as_slice(), *eb, "{al}: bits differ, actual = {ab:x?}");
    }
}

/// 与 render_scene_frame / pick_entity 同式的宽高比。
fn aspect(w: u32, h: u32) -> f32 {
    w as f32 / h.max(1) as f32
}

fn tr(t: [f32; 3], r: [f32; 4], s: [f32; 3]) -> Transform {
    Transform { translation: t, rotation: r, scale: s }
}

fn ent(id: u64, t: Transform, components: Vec<Component>) -> Entity {
    Entity { entity_guid: None, id, name: format!("e{id}"), transform: t, components }
}

fn comp(ctype: &str, props: Value) -> Component {
    Component::new(ctype, props)
}

fn disabled(ctype: &str, props: Value) -> Component {
    let mut c = Component::new(ctype, props);
    c.enabled = false;
    c
}

/// 屏幕归一化坐标(y 向上为正)采样点。
const NDC: [(f32, f32); 4] = [(0.0, 0.0), (0.5, -0.25), (-1.0, 1.0), (0.9, 0.9)];

fn editor_cases() -> Vec<(&'static str, EditorCamera, f32)> {
    let d = EditorCamera::default();
    let custom = EditorCamera {
        target: [1.25, -0.5, 3.0],
        yaw_deg: -127.5,
        pitch_deg: -33.0,
        dist: 4.75,
        fov_y_deg: 72.0,
        ..d
    };
    let pole = EditorCamera { pitch_deg: 89.0, dist: 2.0, ..d };
    let ortho2d = EditorCamera {
        target: [0.0, 0.0, 0.0],
        yaw_deg: 0.0,
        pitch_deg: 0.0,
        dist: 10.0,
        ortho: true,
        ortho_half_h: 6.2,
        ..d
    };
    let ortho_tilt = EditorCamera {
        target: [-3.0, 1.0, 2.0],
        yaw_deg: 45.0,
        pitch_deg: 30.0,
        dist: 20.0,
        ortho: true,
        ortho_half_h: 3.0,
        ..d
    };
    vec![
        ("default@960x540", d, aspect(960, 540)),
        ("default@640x480", d, aspect(640, 480)),
        ("custom@360x600", custom, aspect(360, 600)),
        ("pole@1x1", pole, aspect(1, 1)),
        ("ortho2d@1280x720", ortho2d, aspect(1280, 720)),
        ("ortho_tilt@2520x1080", ortho_tilt, aspect(2520, 1080)),
    ]
}

fn scene_cam_cases() -> Vec<(&'static str, Scene, f32)> {
    // 3d 透视,相机含 roll;首个实体不是相机。
    let mut persp = Scene::new("golden-persp");
    persp.entities.push(ent(1, Transform::default(), vec![comp("MeshRenderer", json!({}))]));
    persp.entities.push(ent(
        2,
        tr([3.0, 2.5, 8.0], [0.1, 0.3, 0.05, 0.95], [1.0; 3]),
        vec![comp("Camera", json!({"fov": 55.0, "near": 0.3, "far": 250.0}))],
    ));
    // 2d 正交。
    let mut ortho = Scene::with_mode("golden-ortho", "2d");
    ortho.entities.push(ent(
        1,
        tr([0.0, 0.0, 10.0], [0.0, 0.0, 0.0, 1.0], [1.0; 3]),
        vec![comp("Camera", json!({"projection": "orthographic", "orthoSize": 6.2, "near": 0.1, "far": 100.0}))],
    ));
    // 正交 + roll:view_proj 丢 roll,ray 保留 roll(现状不对称)。
    let mut roll = Scene::with_mode("golden-roll", "2d");
    roll.entities.push(ent(
        4,
        tr([1.5, -2.0, 12.0], [0.0, 0.0, 0.258819, 0.9659258], [1.0; 3]),
        vec![comp("Camera", json!({"projection": "orthographic", "orthoSize": 3.75}))],
    ));
    // Parent 链:场景相机只用本地 transform(不走 Parent);fov/near/far 取缺省。
    let mut parented = Scene::new("golden-parent");
    parented.entities.push(ent(10, tr([10.0, 0.0, 0.0], [0.0, 0.3826834, 0.0, 0.9238795], [2.0; 3]), vec![]));
    parented.entities.push(ent(
        11,
        tr([0.0, 1.5, 6.0], [-0.13052619, 0.0, 0.0, 0.9914449], [1.0; 3]),
        vec![comp("Parent", json!({"entity": 10})), comp("Camera", json!({}))],
    ));
    // 选实体看"任一启用 Camera",取 props 却取首个 Camera 组件(即使它被禁用)。
    let mut quirk = Scene::new("golden-quirk");
    quirk.entities.push(ent(1, Transform::default(), vec![disabled("Camera", json!({"fov": 20.0}))]));
    quirk.entities.push(ent(
        2,
        tr([0.0, 3.0, 5.0], [0.2, -0.4, 0.1, 0.9], [1.0; 3]),
        vec![disabled("Camera", json!({"fov": 20.0})), comp("Camera", json!({"fov": 90.0}))],
    ));
    let none = Scene::new("golden-none");
    vec![
        ("persp_roll@960x540", persp.clone(), aspect(960, 540)),
        ("persp_roll@540x960", persp, aspect(540, 960)),
        ("ortho2d@1280x720", ortho, aspect(1280, 720)),
        ("ortho_roll@640x480", roll, aspect(640, 480)),
        ("parented@320x180", parented, aspect(320, 180)),
        ("quirk@1x1", quirk, aspect(1, 1)),
        ("none@960x540", none, aspect(960, 540)),
    ]
}


/// 金值测试私有的图集夹具:把 rpc::tests::test_project_root 落盘的 test-v5-*(viewport 测试夹具)
/// 逐字节复制到 Content/GoldenSprites,只换 GUID(golden-*)。不直接共用 test-v5-*:贴图缓存
/// `load_tex_static_cached` 的首次加载不是原子的(两个线程同时首载会各泄漏一份、后写者覆盖),
/// viewport 的 sprite_variants 测试断言常驻贴图指针不变,与本文件并发首载同一 GUID 会偶发失败。
/// guid 映射有 2 s 缓存,并发测试可能先用旧映射建过一次,这里轮询到能解析为止。
fn sprite_fixture() {
    static COPIED: std::sync::OnceLock<()> = std::sync::OnceLock::new();
    COPIED.get_or_init(|| {
        let root = crate::rpc::tests::test_project_root();
        let (src, dst) = (root.join("Content/Sprites"), root.join("Content/GoldenSprites"));
        std::fs::create_dir_all(&dst).unwrap();
        for n in ["a", "b"] {
            std::fs::copy(src.join(format!("variant-{n}.png")), dst.join(format!("golden-{n}.png"))).unwrap();
            let doc = std::fs::read_to_string(src.join(format!("variant-{n}.rxsprite"))).unwrap();
            let renamed = doc.replace(&format!("\"test-v5-texture-{n}\""), &format!("\"golden-texture-{n}\""));
            assert_ne!(renamed, doc, "fixture doc must reference its texture by GUID");
            std::fs::write(dst.join(format!("golden-{n}.rxsprite")), renamed).unwrap();
            // .meta 最后写:guid 映射只收"有 .meta 且源文件存在"的条目。
            for (file, guid, kind) in [("png", "texture", "texture"), ("rxsprite", "sprite", "sprite")] {
                let meta = format!("guid: golden-{guid}-{n}\ntype: {kind}\nimporter: {kind}\nbuild_state: current\n");
                std::fs::write(dst.join(format!("golden-{n}.{file}.meta")), meta).unwrap();
            }
        }
    });
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(15);
    while crate::viewport::sprite_doc_cached("golden-sprite-a").is_none()
        || crate::viewport::sprite_doc_cached("golden-sprite-b").is_none()
    {
        assert!(std::time::Instant::now() < deadline, "sprite fixture never became resolvable");
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
}

fn sprite(id: u64, t: Transform, props: Value) -> Entity {
    ent(id, t, vec![comp("Sprite", props)])
}

fn sprite_cases() -> Vec<(&'static str, Entity)> {
    let variants = json!(["golden-sprite-a", "golden-sprite-b"]);
    vec![
        (
            "fallback_default_ppu",
            sprite(1, tr([1.0, 2.0, 0.0], [0.0, 0.0, 0.258819, 0.9659258], [2.0, 3.0, 1.0]), json!({"texture": "", "sprite": ""})),
        ),
        (
            "fallback_ppu_clamp",
            sprite(2, tr([-4.5, 0.25, -1.0], [0.1, 0.2, 0.3, 0.9], [0.5, -2.0, 0.0]), json!({"pixelsPerUnit": 0.5})),
        ),
        (
            "atlas_a_frame0",
            sprite(
                3,
                tr([0.3, -0.7, 0.0], [0.0, 0.0, 0.5, 0.8660254], [1.5, 1.5, 1.0]),
                json!({"sprite": "golden-sprite-a", "frame": 0, "pixelsPerUnit": 256}),
            ),
        ),
        (
            "variants_a_frame127",
            sprite(
                4,
                tr([2.0, 1.0, 0.5], [0.0, 0.0, 0.0, 1.0], [1.0, 2.0, 1.0]),
                json!({"spriteVariants": variants, "variantStride": 128, "frame": 127, "pixelsPerUnit": 64}),
            ),
        ),
        (
            "variants_b_frame255",
            sprite(
                5,
                tr([-1.0, -1.0, 0.0], [0.0, 0.0, -0.3826834, 0.9238795], [3.0, 0.5, 2.0]),
                json!({"spriteVariants": variants, "variantStride": 128, "frame": 255, "pixelsPerUnit": 32}),
            ),
        ),
        (
            "texture_direct",
            sprite(6, tr([0.0, 0.0, -2.0], [0.2, -0.4, 0.1, 0.9], [1.0, 1.0, 1.0]), json!({"texture": "golden-texture-a", "pixelsPerUnit": 4})),
        ),
        ("mesh_is_none", ent(7, Transform::default(), vec![comp("MeshRenderer", json!({}))])),
        ("disabled_is_none", ent(8, Transform::default(), vec![disabled("Sprite", json!({"texture": "golden-texture-a"}))])),
    ]
}

type RayCase = (&'static str, [f32; 3], [f32; 3], Transform);

fn ray_cube_cases() -> Vec<RayCase> {
    let id = [0.0, 0.0, 0.0, 1.0];
    vec![
        ("front", [0.0, 0.0, 0.0], [0.0, 0.0, -1.0], tr([0.0, 0.0, -5.0], id, [1.0; 3])),
        (
            "rotated_scaled",
            [0.3, 0.2, 4.0],
            [0.0835, -0.2226, -0.9713],
            tr([1.0, -2.0, -6.0], [0.2, -0.4, 0.1, 0.9], [2.0, 0.5, 3.0]),
        ),
        ("inside_returns_tmax", [1.0, -2.0, -6.0], [0.6, 0.0, 0.8], tr([1.0, -2.0, -6.0], [0.2, -0.4, 0.1, 0.9], [2.0, 0.5, 3.0])),
        ("parallel_inside_slab", [0.2, 0.1, 5.0], [0.0, 0.0, -1.0], tr([0.0; 3], id, [1.0; 3])),
        ("parallel_outside_slab", [0.7, 0.1, 5.0], [0.0, 0.0, -1.0], tr([0.0; 3], id, [1.0; 3])),
        ("zero_scale", [0.0, 0.0, 5.0], [0.0, 0.0, -1.0], tr([0.0; 3], id, [1.0, 0.0, 1.0])),
        ("behind", [0.0, 0.0, 0.0], [0.0, 0.0, 1.0], tr([0.0, 0.0, -5.0], id, [1.0; 3])),
        (
            "negative_scale",
            [0.0, 0.0, 2.0],
            [0.1, 0.1, -0.99],
            tr([0.5, 0.5, -3.0], [0.0, 0.3826834, 0.0, 0.9238795], [-1.5, 2.0, 0.75]),
        ),
    ]
}


const GOLDEN_MODEL: &str = "golden-render-core-model";
const ID_ROT: [f32; 4] = [0.0, 0.0, 0.0, 1.0];

/// 两节点(body → tip)、一个四边形图元、一段 tip 平移动画的静态模型;无蒙皮、无贴图。
fn golden_model() -> assetd::model::ModelBundle {
    use assetd::model::*;
    let node = |id: &str, children: Vec<usize>, t: [f32; 3], r: [f32; 4], s: [f32; 3]| ModelNode {
        id: id.into(),
        name: id.into(),
        children,
        primitives: vec![0],
        translation: t,
        rotation: r,
        scale: s,
        matrix: None,
        skin: None,
        collision: false,
    };
    ModelBundle {
        version: 1,
        guid: GOLDEN_MODEL.into(),
        revision: 1,
        name: "golden".into(),
        source_id: "golden-render-core".into(),
        source_hash: "golden".into(),
        kind: "prop".into(),
        roots: vec![0],
        primitives: vec![ModelPrimitive {
            id: "quad".into(),
            positions: vec![[-0.5, -0.25, 0.0], [0.75, -0.25, 0.1], [0.75, 0.5, 0.0], [-0.5, 0.5, -0.1]],
            normals: vec![[0.0, 0.0, 1.0]; 4],
            tangents: vec![[1.0, 0.0, 0.0, 1.0]; 4],
            uv0: vec![[0.0, 1.0], [1.0, 1.0], [1.0, 0.0], [0.0, 0.0]],
            indices: vec![0, 1, 2, 0, 2, 3],
            joints: vec![],
            weights: vec![],
            material: Some(0),
        }],
        nodes: vec![
            node("body", vec![1], [0.25, 0.0, 0.0], [0.0, 0.0, 0.1305262, 0.9914449], [1.0; 3]),
            node("tip", vec![], [0.0, 1.0, 0.0], ID_ROT, [0.5; 3]),
        ],
        materials: vec![ModelMaterial {
            guid: String::new(),
            name: "golden".into(),
            base_color: [0.8, 0.6, 0.4, 1.0],
            metallic: 0.1,
            roughness: 0.7,
            emissive: [0.0; 3],
            base_color_texture: None,
            normal_texture: None,
            metallic_roughness_texture: None,
            occlusion_texture: None,
            emissive_texture: None,
            normal_scale: 1.0,
            occlusion_strength: 1.0,
            double_sided: true,
            alpha_mode: "OPAQUE".into(),
            alpha_cutoff: 0.5,
            unlit: false,
        }],
        textures: vec![],
        skins: vec![],
        animations: vec![ModelAnimation {
            name: "sway".into(),
            duration: 2.0,
            channels: vec![ModelAnimationChannel {
                node: 1,
                path: "translation".into(),
                times: vec![0.0, 2.0],
                values: vec![[0.0, 1.0, 0.0, 0.0], [1.0, 1.0, 0.0, 0.0]],
                interpolation: "LINEAR".into(),
            }],
        }],
        idle_clip: String::new(),
        walk_clip: String::new(),
    }
}

fn model_renderer(props: Value) -> Component {
    comp("ModelRenderer", props)
}

/// 模型场景:Parent 链上的模型实体(Animator 挂在父实体上)、Parent 链上的旧式 cube、
/// 已解析图集精灵、带 Parent 的相机实体。调用前须 sprite_fixture()。
fn model_scene() -> Scene {
    crate::modelrt::prime(golden_model());
    let mut s = Scene::new("golden-model");
    s.entities.push(ent(
        20,
        tr([1.0, 0.5, -3.0], [0.0, 0.3826834, 0.0, 0.9238795], [1.5; 3]),
        vec![comp("Animator", json!({"clip": "sway", "time": 0.5, "loop": true}))],
    ));
    s.entities.push(ent(
        21,
        tr([0.5, 0.0, 0.25], [0.1, 0.0, 0.0, 0.995], [1.0, 2.0, 1.0]),
        vec![comp("Parent", json!({"entity": 20})), model_renderer(json!({"model": GOLDEN_MODEL}))],
    ));
    s.entities.push(ent(
        22,
        tr([-1.0, 0.0, 0.0], ID_ROT, [0.5; 3]),
        vec![comp("Parent", json!({"entity": 20})), comp("MeshRenderer", json!({"mesh": "cube"}))],
    ));
    s.entities.push(sprite(
        23,
        tr([-2.0, 1.0, -4.0], [0.0, 0.0, 0.258819, 0.9659258], [1.0; 3]),
        json!({"sprite": "golden-sprite-a", "frame": 0, "pixelsPerUnit": 2, "tint": [1.0, 0.5, 0.25, 1.0], "flipX": true}),
    ));
    s.entities.push(ent(
        24,
        tr([0.0, 1.0, 4.0], [-0.13052619, 0.0, 0.0, 0.9914449], [1.0; 3]),
        vec![comp("Parent", json!({"entity": 20})), comp("Camera", json!({}))],
    ));
    s
}

fn pick_scene_3d() -> Scene {
    let mut s = Scene::new("golden-pick-3d");
    s.entities.push(ent(1, tr([0.0, 0.0, -4.0], ID_ROT, [1.0; 3]), vec![comp("MeshRenderer", json!({"mesh": "cube", "material": "m"}))]));
    s.entities.push(ent(2, tr([1.2, 0.3, -6.0], [0.2, -0.4, 0.1, 0.9], [2.0, 0.5, 1.5]), vec![comp("MeshRenderer", json!({}))]));
    s.entities.push(ent(3, tr([0.0, 0.0, -2.0], ID_ROT, [1.0; 3]), vec![disabled("MeshRenderer", json!({}))]));
    s.entities.push(sprite(4, tr([-1.5, 0.0, -3.0], ID_ROT, [1.0; 3]), json!({"texture": ""})));
    s.entities.push(ent(6, tr([50.0, 0.0, 0.0], ID_ROT, [1.0; 3]), vec![]));
    s.entities.push(ent(
        5,
        tr([0.0, -1.2, -5.0], [0.0, 0.0, 0.258819, 0.9659258], [3.0, 0.4, 1.0]),
        vec![comp("Parent", json!({"entity": 6})), comp("MeshRenderer", json!({}))],
    ));
    s
}

/// 2d 精灵场景;调用前须 sprite_fixture()。
fn pick_scene_2d() -> Scene {
    let mut s = Scene::with_mode("golden-pick-2d", "2d");
    s.entities.push(sprite(1, tr([0.0, 0.0, 0.0], ID_ROT, [2.0, 2.0, 1.0]), json!({"texture": ""})));
    s.entities.push(sprite(2, tr([0.5, 0.5, 1.0], ID_ROT, [1.0; 3]), json!({"sprite": "golden-sprite-a", "frame": 0, "pixelsPerUnit": 2})));
    s.entities.push(sprite(
        3,
        tr([-3.0, -2.0, 0.5], [0.0, 0.0, 0.258819, 0.9659258], [1.0; 3]),
        json!({"spriteVariants": ["golden-sprite-a", "golden-sprite-b"], "variantStride": 128, "frame": 255, "pixelsPerUnit": 4}),
    ));
    s
}


fn bounds_cases() -> Vec<(&'static str, Scene)> {
    sprite_fixture();
    let model = model_scene();
    // nodeId 子树:world × inverse(rest[tip]),实体走 Parent 链。
    let mut node = Scene::new("golden-node");
    node.entities.push(ent(40, tr([2.0, 0.0, 0.0], [0.0, 0.0, 0.1305262, 0.9914449], [2.0; 3]), vec![]));
    node.entities.push(ent(
        41,
        tr([1.0, -0.5, 0.5], ID_ROT, [1.0; 3]),
        vec![comp("Parent", json!({"entity": 40})), model_renderer(json!({"model": GOLDEN_MODEL, "nodeId": "tip"}))],
    ));
    // 无 ModelRenderer:全部走 legacy_draw(cube + 图集精灵)。
    let mut legacy = Scene::new("golden-legacy");
    legacy.entities.push(ent(50, tr([0.0, 0.5, 0.0], [0.2, -0.4, 0.1, 0.9], [1.0, 2.0, 0.5]), vec![comp("MeshRenderer", json!({}))]));
    legacy.entities.push(sprite(
        51,
        tr([3.0, 0.0, -1.0], ID_ROT, [1.0; 3]),
        json!({"spriteVariants": ["golden-sprite-a", "golden-sprite-b"], "variantStride": 128, "frame": 127, "pixelsPerUnit": 8}),
    ));
    vec![("model_parent_anim", model), ("node_select", node), ("legacy_only", legacy)]
}

fn bounds_err_cases() -> Vec<(&'static str, Scene)> {
    crate::modelrt::prime(golden_model());
    let mut empty = Scene::new("golden-empty");
    empty.entities.push(ent(1, Transform::default(), vec![comp("Camera", json!({}))]));
    let mut missing_node = Scene::new("golden-missing-node");
    missing_node.entities.push(ent(2, Transform::default(), vec![model_renderer(json!({"model": GOLDEN_MODEL, "nodeId": "nope"}))]));
    let mut cycle = Scene::new("golden-cycle");
    cycle.entities.push(ent(30, Transform::default(), vec![comp("Parent", json!({"entity": 31})), model_renderer(json!({"model": GOLDEN_MODEL}))]));
    cycle.entities.push(ent(31, Transform::default(), vec![comp("Parent", json!({"entity": 30}))]));
    let mut no_model = Scene::new("golden-no-model");
    no_model.entities.push(ent(3, Transform::default(), vec![model_renderer(json!({}))]));
    vec![("empty", empty), ("missing_node", missing_node), ("parent_cycle", cycle), ("model_prop_missing", no_model)]
}

fn editor_out() -> Out {
    let mut out = Vec::new();
    for (name, cam, a) in editor_cases() {
        out.push((format!("{name}/eye"), b3(cam.eye())));
        out.push((format!("{name}/view_proj"), b4x4(cam.view_proj(a))));
        for (i, (nx, ny)) in NDC.iter().enumerate() {
            out.push((format!("{name}/ray{i}"), bray(cam.ray(*nx, *ny, a))));
        }
    }
    out
}

fn scene_cam_out() -> Out {
    let mut out = Vec::new();
    for (name, scene, a) in scene_cam_cases() {
        let vp = crate::viewport::scene_camera_view_proj(&scene, a);
        out.push((format!("{name}/view_proj"), vp.map(b4x4).unwrap_or_default()));
        for (i, (nx, ny)) in NDC.iter().enumerate() {
            let ray = crate::viewport::scene_camera_ray(&scene, *nx, *ny, a);
            out.push((format!("{name}/ray{i}"), ray.map(bray).unwrap_or_default()));
        }
    }
    out
}

fn sprite_out() -> Out {
    sprite_fixture();
    sprite_cases()
        .into_iter()
        .map(|(name, e)| (name.to_string(), crate::viewport::sprite_render_transform(&e).map(|t| btr(&t)).unwrap_or_default()))
        .collect()
}

fn ray_cube_out() -> Out {
    ray_cube_cases()
        .into_iter()
        .map(|(name, o, d, t)| (name.to_string(), crate::viewport::ray_unit_cube(o, d, &t).map(|t| vec![t.to_bits()]).unwrap_or_default()))
        .collect()
}

/// 世界点 → 像素(左上原点),只用来生成查询点;view_proj 的 y 已翻回"向上为正"。
fn project_px(cam: &EditorCamera, p: [f32; 3], w: u32, h: u32) -> (f32, f32) {
    let m = cam.view_proj(aspect(w, h));
    let c: Vec<f32> = (0..4).map(|r| m[r][0] * p[0] + m[r][1] * p[1] + m[r][2] * p[2] + m[r][3]).collect();
    ((c[0] / c[3] + 1.0) * 0.5 * w as f32, (1.0 - c[1] / c[3]) * 0.5 * h as f32)
}

fn pick_out() -> Out {
    sprite_fixture();
    let d = EditorCamera::default();
    let front = EditorCamera { target: [0.0, 0.0, -4.0], yaw_deg: 0.0, pitch_deg: 0.0, dist: 4.0, ..d };
    let tilt = EditorCamera { target: [0.5, -0.5, -4.5], yaw_deg: 20.0, pitch_deg: 15.0, dist: 6.0, fov_y_deg: 60.0, ..d };
    let ortho = EditorCamera { target: [0.0; 3], yaw_deg: 0.0, pitch_deg: 0.0, dist: 10.0, ortho: true, ortho_half_h: 5.0, ..d };
    let model_cam = EditorCamera { target: [0.5, 0.5, -3.0], yaw_deg: -10.0, pitch_deg: 10.0, dist: 6.0, ..d };
    let cases = vec![
        ("3d/front@640x360", pick_scene_3d(), front, 640u32, 360u32),
        ("3d/tilt@360x640", pick_scene_3d(), tilt, 360, 640),
        ("2d/ortho@1280x720", pick_scene_2d(), ortho, 1280, 720),
        ("model/persp@800x600", model_scene(), model_cam, 800, 600),
    ];
    let mut out = Vec::new();
    for (name, scene, cam, w, h) in cases {
        let mut queries = vec![(w as f32 * 0.5, h as f32 * 0.5), (3.0, 4.0), (w as f32 * 0.8, h as f32 * 0.3)];
        for e in &scene.entities {
            let (x, y) = project_px(&cam, e.transform.translation, w, h);
            queries.push((x, y));
            queries.push((x + 7.5, y - 3.25));
            // Parent 链实体再补一个世界原点查询(模型腿三角形求交走世界空间)。
            if e.component("Parent").is_some() {
                if let Ok(m) = crate::modelrt::entity_world(&scene, e) {
                    queries.push(project_px(&cam, crate::modelrt::point(m, [0.0; 3]), w, h));
                }
            }
        }
        for (i, (px, py)) in queries.into_iter().enumerate() {
            let hit = crate::viewport::pick_entity(&scene, &cam, px, py, w, h).map(|(id, p)| {
                let mut v = vec![id as u32, (id >> 32) as u32];
                v.extend(b3(p));
                v
            });
            out.push((format!("{name}/q{i}"), hit.unwrap_or_default()));
        }
    }
    out
}

fn bounds_out() -> Out {
    bounds_cases()
        .into_iter()
        .map(|(name, s)| {
            let (c, r) = crate::modelrender::bounds(&s).expect(name);
            let mut v = b3(c);
            v.push(r.to_bits());
            (name.to_string(), v)
        })
        .collect()
}

// ─────────────────────────── 金值(搬迁前由现有实现算出;f32::to_bits) ───────────────────────────

const EDITOR: &[(&str, &[u32])] = &[
    ("default@960x540/eye", &[0x4091daa8, 0x40973533, 0x40d04d20]),
    ("default@960x540/view_proj", &[0x3f7cf626, 0x00000000, 0xbf312025, 0xb49a678d, 0xbf13d4f5, 0x3ff25dca, 0xbf532032, 0xbf725dbf, 0xbf01a93d, 0xbef064bc, 0xbf392cdb, 0x4112f870, 0xbf01a5eb, 0xbef05e94, 0xbf39281d, 0x4113c17a]),
    ("default@960x540/ray0", &[0x4091daa8, 0x40973533, 0x40d04d20, 0xbf01a5eb, 0xbef05e94, 0xbf39281d]),
    ("default@960x540/ray1", &[0x4091daa8, 0x40973533, 0x40d04d20, 0xbdfee6ff, 0xbf0696d0, 0xbf576bf9]),
    ("default@960x540/ray2", &[0x4091daa8, 0x40973533, 0x40d04d20, 0xbf733251, 0xbd2b62bf, 0xbe9e7380]),
    ("default@960x540/ray3", &[0x4091daa8, 0x40973533, 0x40d04d20, 0xbbce3ac6, 0xbd99e644, 0xbf7f4566]),
    ("default@640x480/eye", &[0x4091daa8, 0x40973533, 0x40d04d20]),
    ("default@640x480/view_proj", &[0x3fa8a41a, 0x00000000, 0xbf6c2adc, 0xb4cddf67, 0xbf13d4f5, 0x3ff25dca, 0xbf532032, 0xbf725dbf, 0xbf01a93d, 0xbef064bc, 0xbf392cdb, 0x4112f870, 0xbf01a5eb, 0xbef05e94, 0xbf39281d, 0x4113c17a]),
    ("default@640x480/ray0", &[0x4091daa8, 0x40973533, 0x40d04d20, 0xbf01a5eb, 0xbef05e94, 0xbf39281d]),
    ("default@640x480/ray1", &[0x4091daa8, 0x40973533, 0x40d04d20, 0xbe563021, 0xbf0b120c, 0xbf502770]),
    ("default@640x480/ray2", &[0x4091daa8, 0x40973533, 0x40d04d20, 0xbf66b20b, 0xbd3ac245, 0xbedcb85c]),
    ("default@640x480/ray3", &[0x4091daa8, 0x40973533, 0x40d04d20, 0xbe0728c4, 0xbda601bd, 0xbf7ce90d]),
    ("custom@360x600/eye", &[0xbff48a46, 0xc04591fd, 0x3f132bc0]),
    ("custom@360x600/view_proj", &[0xbfb2bfdc, 0x00000000, 0x3fe8f369, 0xc06db535, 0xbf183fbb, 0x3f93c11c, 0xbee9a643, 0x402c228a, 0x3f2a5987, 0x3f0b7109, 0x3f02b6c5, 0x4026fd92, 0x3f2a552b, 0x3f0b6d77, 0x3f02b36c, 0x402a2c7f]),
    ("custom@360x600/ray0", &[0xbff48a46, 0xc04591fd, 0x3f132bc0, 0x3f2a552b, 0x3f0b6d77, 0x3f02b36c]),
    ("custom@360x600/ray1", &[0xbff48a46, 0xc04591fd, 0x3f132bc0, 0x3f1683f6, 0x3ec13be2, 0x3f372806]),
    ("custom@360x600/ray2", &[0xbff48a46, 0xc04591fd, 0x3f132bc0, 0x3ef0f20f, 0x3f6163fd, 0xbd6e12f4]),
    ("custom@360x600/ray3", &[0xbff48a46, 0xc04591fd, 0x3f132bc0, 0x3dea7c29, 0x3f5e815f, 0x3ef651a7]),
    ("pole@1x1/eye", &[0x3ca40224, 0x401ffb02, 0x3cea3a6c]),
    ("pole@1x1/view_proj", &[0x3fe0dacd, 0x00000000, 0xbf9d71e8, 0xb1893f9a, 0xbf9d6bc5, 0x3d194cce, 0xbfe0d20a, 0xbc994cce, 0xbc240658, 0xbf7ffc94, 0xbc6a406d, 0x401ccf92, 0xbc240225, 0xbf7ff606, 0xbc6a3a6e, 0x401ffec1]),
    ("pole@1x1/ray0", &[0x3ca40224, 0x401ffb02, 0x3cea3a6c, 0xbc240225, 0xbf7ff606, 0xbc6a3a6e]),
    ("pole@1x1/ray1", &[0x3ca40224, 0x401ffb02, 0x3cea3a6c, 0x3e759348, 0xbf783028, 0xbd5046d3]),
    ("pole@1x1/ray2", &[0x3ca40224, 0x401ffb02, 0x3cea3a6c, 0xbf0cecad, 0xbf53f0ff, 0xbddc20ad]),
    ("pole@1x1/ray3", &[0x3ca40224, 0x401ffb02, 0x3cea3a6c, 0x3da3f1dd, 0xbf5a7ff2, 0xbf03cf5d]),
    ("ortho2d@1280x720/eye", &[0x00000000, 0x00000000, 0x41200000]),
    ("ortho2d@1280x720/view_proj", &[0x3db9ce74, 0x00000000, 0x00000000, 0x00000000, 0x00000000, 0x3e25294b, 0x00000000, 0x00000000, 0x00000000, 0x00000000, 0xbb0315c9, 0x3ca3097f, 0x00000000, 0x00000000, 0x00000000, 0x3f800000]),
    ("ortho2d@1280x720/ray0", &[0x00000000, 0x00000000, 0x41200000, 0x00000000, 0x00000000, 0xbf800000]),
    ("ortho2d@1280x720/ray1", &[0x40b05b05, 0xbfc66666, 0x41200000, 0x00000000, 0x00000000, 0xbf800000]),
    ("ortho2d@1280x720/ray2", &[0xc1305b05, 0x40c66666, 0x41200000, 0x00000000, 0x00000000, 0xbf800000]),
    ("ortho2d@1280x720/ray3", &[0x411eb851, 0x40b28f5b, 0x41200000, 0x00000000, 0x00000000, 0xbf800000]),
    ("ortho_tilt@2520x1080/eye", &[0x4113f58c, 0x41300000, 0x4163f58c]),
    ("ortho_tilt@2520x1080/view_proj", &[0x3dcee116, 0x00000000, 0xbdcee116, 0x3f014cad, 0xbdf15bef, 0x3e93cd3a, 0xbdf15bef, 0xbed02436, 0xbaa08bb9, 0xba8315c9, 0xbaa08bb9, 0x3d2286ad, 0x00000000, 0x00000000, 0x00000000, 0x3f800000]),
    ("ortho_tilt@2520x1080/ray0", &[0x4113f58c, 0x41300000, 0x4163f58c, 0xbf1cc470, 0xbf000000, 0xbf1cc470]),
    ("ortho_tilt@2520x1080/ray1", &[0x413fccbf, 0x41259b92, 0x41409a95, 0xbf1cc470, 0xbf000000, 0xbf1cc470]),
    ("ortho_tilt@2520x1080/ray2", &[0x404f2bab, 0x415991b8, 0x419117a0, 0xbf1cc470, 0xbf000000, 0xbf1cc470]),
    ("ortho_tilt@2520x1080/ray3", &[0x414bf648, 0x4155698c, 0x410d68c8, 0xbf1cc470, 0xbf000000, 0xbf1cc470]),
];

const SCENE_CAM: &[(&str, &[u32])] = &[
    ("persp_roll@960x540/view_proj", &[0x3f606f52, 0x00000000, 0xbf21b466, 0x401b154f, 0x3e371195, 0x3ff2bfe5, 0x3e7e162a, 0xc0e86721, 0xbf13eb39, 0x3e233885, 0xbf4d4d12, 0x40ee6850, 0xbf13bdc8, 0x3e230660, 0xbf4d0e00, 0x40f7b8ac]),
    ("persp_roll@960x540/ray0", &[0x40400000, 0x40200000, 0x41000000, 0xbf13bdc7, 0x3e23065f, 0xbf4d0e02]),
    ("persp_roll@960x540/ray1", &[0x40400000, 0x40200000, 0x41000000, 0xbe34000b, 0x3dbf593d, 0xbf7ae052]),
    ("persp_roll@960x540/ray2", &[0x40400000, 0x40200000, 0x41000000, 0xbf6d00a5, 0x3eb7f835, 0xbdf09d48]),
    ("persp_roll@960x540/ray3", &[0x40400000, 0x40200000, 0x41000000, 0x3d7f2097, 0x3f09cb50, 0xbf57295c]),
    ("persp_roll@540x960/view_proj", &[0x403154cc, 0x00000000, 0xbfff8888, 0x40f511df, 0x3e371195, 0x3ff2bfe5, 0x3e7e162a, 0xc0e86721, 0xbf13eb39, 0x3e233885, 0xbf4d4d12, 0x40ee6850, 0xbf13bdc8, 0x3e230660, 0xbf4d0e00, 0x40f7b8ac]),
    ("persp_roll@540x960/ray0", &[0x40400000, 0x40200000, 0x41000000, 0xbf13bdc7, 0x3e23065f, 0xbf4d0e02]),
    ("persp_roll@540x960/ray1", &[0x40400000, 0x40200000, 0x41000000, 0xbee3ac1a, 0x3d5c98b5, 0xbf64e1f1]),
    ("persp_roll@540x960/ray2", &[0x40400000, 0x40200000, 0x41000000, 0xbf375572, 0x3f08a146, 0xbee64722]),
    ("persp_roll@540x960/ray3", &[0x40400000, 0x40200000, 0x41000000, 0xbeaaa6a7, 0x3f14140d, 0xbf3e999b]),
    ("ortho2d@1280x720/view_proj", &[0x3db9ce74, 0x00000000, 0x00000000, 0x00000000, 0x00000000, 0x3e25294b, 0x00000000, 0x00000000, 0x00000000, 0x00000000, 0xbc240106, 0x3dcaf478, 0x00000000, 0x00000000, 0x00000000, 0x3f800000]),
    ("ortho2d@1280x720/ray0", &[0x00000000, 0x00000000, 0x41200000, 0x00000000, 0x00000000, 0xbf800000]),
    ("ortho2d@1280x720/ray1", &[0x40b05b05, 0xbfc66666, 0x41200000, 0x00000000, 0x00000000, 0xbf800000]),
    ("ortho2d@1280x720/ray2", &[0xc1305b05, 0x40c66666, 0x41200000, 0x00000000, 0x00000000, 0xbf800000]),
    ("ortho2d@1280x720/ray3", &[0x411eb851, 0x40b28f5b, 0x41200000, 0x00000000, 0x00000000, 0xbf800000]),
    ("ortho_roll@640x480/view_proj", &[0x3e4ccccd, 0x00000000, 0x00000000, 0xbe99999a, 0x00000000, 0x3e888889, 0x00000000, 0x3f088889, 0x00000000, 0x00000000, 0xbb031925, 0x3cc30234, 0x00000000, 0x00000000, 0x00000000, 0x3f800000]),
    ("ortho_roll@640x480/ray0", &[0x3fc00000, 0xc0000000, 0x41400000, 0x00000000, 0x00000000, 0xbf800000]),
    ("ortho_roll@640x480/ray1", &[0x40844833, 0xbfc7ec4d, 0x41400000, 0x00000000, 0x00000000, 0xbf800000]),
    ("ortho_roll@640x480/ray2", &[0xc0969066, 0xbfa04ecc, 0x41400000, 0x00000000, 0x00000000, 0xbf800000]),
    ("ortho_roll@640x480/ray3", &[0x406d6a50, 0x404b0fbd, 0x41400000, 0x00000000, 0x00000000, 0xbf800000]),
    ("parented@320x180/view_proj", &[0x3f796a53, 0x00000000, 0x00000000, 0x00000000, 0x00000000, 0x3fd625ef, 0xbee585f7, 0x3e388061, 0x00000000, 0xbe848ab6, 0xbf775394, 0x40c2b853, 0x00000000, 0xbe8483ed, 0xbf7746ea, 0x40c5e18e]),
    ("parented@320x180/ray0", &[0x00000000, 0x3fc00000, 0x40c00000, 0x00000000, 0xbe8483ee, 0xbf7746ea]),
    ("parented@320x180/ray1", &[0x00000000, 0x3fc00000, 0x40c00000, 0x3ee7ddfa, 0xbeb3ed39, 0xbf51c43e]),
    ("parented@320x180/ray2", &[0x00000000, 0x3fc00000, 0x40c00000, 0xbf2a13be, 0x3e46161e, 0xbf38d131]),
    ("parented@320x180/ray3", &[0x00000000, 0x3fc00000, 0x40c00000, 0x3f224a18, 0x3e2ad3fc, 0xbf415301]),
    ("quirk@1x1/view_proj", &[0x40748c11, 0x00000000, 0x40861b3f, 0xc1a7a20f, 0xbfe7663c, 0x40a3ba3a, 0x3fd2fb5b, 0xc1bcba37, 0x3f2ab366, 0x3edce82d, 0xbf1ba397, 0x3fd29d63, 0x3f2aaaa9, 0x3edcdcde, 0xbf1b9b9f, 0x3fdf5f68]),
    ("quirk@1x1/ray0", &[0x00000000, 0x40400000, 0x40a00000, 0x3f2aaaaa, 0x3edcdcdc, 0xbf1b9b9c]),
    ("quirk@1x1/ray1", &[0x00000000, 0x40400000, 0x40a00000, 0x3f3c8fa2, 0x3ec86b5f, 0xbf0d3461]),
    ("quirk@1x1/ray2", &[0x00000000, 0x40400000, 0x40a00000, 0x3ef3982e, 0x3f11cbc0, 0xbf2b986d]),
    ("quirk@1x1/ray3", &[0x00000000, 0x40400000, 0x40a00000, 0x3f33bcda, 0x3f104827, 0xbeded40e]),
    ("none@960x540/view_proj", &[]),
    ("none@960x540/ray0", &[]),
    ("none@960x540/ray1", &[]),
    ("none@960x540/ray2", &[]),
    ("none@960x540/ray3", &[]),
];

const SPRITE_XFORM: &[(&str, &[u32])] = &[
    ("fallback_default_ppu", &[0x3f800000, 0x40000000, 0x00000000, 0x00000000, 0x00000000, 0x3e8483ed, 0x3f7746ea, 0x40000000, 0x40400000, 0x3f800000]),
    ("fallback_ppu_clamp", &[0xc0900000, 0x3e800000, 0xbf800000, 0x3dcccccd, 0x3e4ccccd, 0x3e99999a, 0x3f666666, 0x3f000000, 0xc0000000, 0x3a83126f]),
    ("atlas_a_frame0", &[0x3e946763, 0xbf31b333, 0x00000000, 0x00000000, 0x00000000, 0x3f000000, 0x3f5db3d7, 0x3c400000, 0x3cc00000, 0x3f800000]),
    ("variants_a_frame127", &[0x40018000, 0x3f840000, 0x3f000000, 0x00000000, 0x00000000, 0x00000000, 0x3f800000, 0x3dc00000, 0x3e000000, 0x3f800000]),
    ("variants_b_frame255", &[0xbf8a9b4a, 0xbf7345a7, 0x00000000, 0x00000000, 0x00000000, 0xbec3ef14, 0x3f6c835e, 0x3ec00000, 0x3dc00000, 0x40000000]),
    ("texture_direct", &[0x00000000, 0x00000000, 0xc0000000, 0x3e4ccccd, 0xbecccccd, 0x3dcccccd, 0x3f666666, 0x40000000, 0x3f800000, 0x3f800000]),
    ("mesh_is_none", &[]),
    ("disabled_is_none", &[]),
];

const RAY_CUBE: &[(&str, &[u32])] = &[
    ("front", &[0x40900000]),
    ("rotated_scaled", &[0x411841c8]),
    ("inside_returns_tmax", &[0x3f808102]),
    ("parallel_inside_slab", &[0x40900000]),
    ("parallel_outside_slab", &[]),
    ("zero_scale", &[]),
    ("behind", &[]),
    ("negative_scale", &[0x408ebace]),
];

const PICK: &[(&str, &[u32])] = &[
    ("3d/front@640x360/q0", &[0x00000001, 0x00000000, 0x00000000, 0x00000000, 0xc0600000]),
    ("3d/front@640x360/q1", &[]),
    ("3d/front@640x360/q2", &[]),
    ("3d/front@640x360/q3", &[0x00000001, 0x00000000, 0x00000000, 0x00000000, 0xc0600000]),
    ("3d/front@640x360/q4", &[0x00000001, 0x00000000, 0x3d8b4543, 0x3cf16700, 0xc0600000]),
    ("3d/front@640x360/q5", &[0x00000002, 0x00000000, 0x3f83fbf0, 0x3e83fbf0, 0xc0a4faea]),
    ("3d/front@640x360/q6", &[0x00000002, 0x00000000, 0x3f915acc, 0x3e9acb03, 0xc0a59aee]),
    ("3d/front@640x360/q7", &[0x00000001, 0x00000000, 0x00000000, 0x00000000, 0xc0600000]),
    ("3d/front@640x360/q8", &[0x00000001, 0x00000000, 0x3d8b4543, 0x3cf16700, 0xc0600000]),
    ("3d/front@640x360/q9", &[0x00000004, 0x00000000, 0xbfa00000, 0x00000000, 0xc0200000]),
    ("3d/front@640x360/q10", &[0x00000004, 0x00000000, 0xbf99c856, 0x3cac6e24, 0xc0200000]),
    ("3d/front@640x360/q11", &[0x00000001, 0x00000000, 0xffc00000, 0xffc00000, 0xffc00000]),
    ("3d/front@640x360/q12", &[0x00000001, 0x00000000, 0xffc00000, 0xffc00000, 0xffc00000]),
    ("3d/front@640x360/q13", &[0x00000005, 0x00000000, 0x00000000, 0xbf8a3d71, 0xc0900000]),
    ("3d/front@640x360/q14", &[0x00000005, 0x00000000, 0x3db30fe8, 0xbf8563f2, 0xc0900000]),
    ("3d/front@640x360/q15", &[]),
    ("3d/tilt@360x640/q0", &[0x00000001, 0x00000000, 0x3f000000, 0xbefffffc, 0xc0900000]),
    ("3d/tilt@360x640/q1", &[]),
    ("3d/tilt@360x640/q2", &[0x00000002, 0x00000000, 0x3f9fae2f, 0x3f4a2b80, 0xc0d480a5]),
    ("3d/tilt@360x640/q3", &[0x00000001, 0x00000000, 0x3e807990, 0x3dd9fd40, 0xc0600000]),
    ("3d/tilt@360x640/q4", &[0x00000001, 0x00000000, 0x3ea75920, 0x3e1079d8, 0xc0600000]),
    ("3d/tilt@360x640/q5", &[0x00000002, 0x00000000, 0x3fac9bf0, 0x3ec63fc2, 0xc0a64169]),
    ("3d/tilt@360x640/q6", &[0x00000002, 0x00000000, 0x3fb66f1a, 0x3ed8a3f2, 0xc0a70cbe]),
    ("3d/tilt@360x640/q7", &[0x00000004, 0x00000000, 0xbf800000, 0xbed92ef4, 0xc04bf5a6]),
    ("3d/tilt@360x640/q8", &[0x00000004, 0x00000000, 0xbf800000, 0xbed7d4fc, 0xc0530882]),
    ("3d/tilt@360x640/q9", &[0x00000004, 0x00000000, 0xbf800000, 0x3e076034, 0xc0204a5f]),
    ("3d/tilt@360x640/q10", &[0x00000004, 0x00000000, 0xbf800000, 0x3e12d92c, 0xc0259f92]),
    ("3d/tilt@360x640/q11", &[]),
    ("3d/tilt@360x640/q12", &[]),
    ("3d/tilt@360x640/q13", &[0x00000005, 0x00000000, 0x3e55bc80, 0xbf8159cf, 0xc0900000]),
    ("3d/tilt@360x640/q14", &[0x00000005, 0x00000000, 0x3e997fa8, 0xbf7619b6, 0xc0900000]),
    ("3d/tilt@360x640/q15", &[]),
    ("2d/ortho@1280x720/q0", &[0x00000001, 0x00000000, 0x00000000, 0x00000000, 0x3f000000]),
    ("2d/ortho@1280x720/q1", &[]),
    ("2d/ortho@1280x720/q2", &[]),
    ("2d/ortho@1280x720/q3", &[0x00000001, 0x00000000, 0x00000000, 0x00000000, 0x3f000000]),
    ("2d/ortho@1280x720/q4", &[0x00000001, 0x00000000, 0x3dd55556, 0x3d38e390, 0x3f000000]),
    ("2d/ortho@1280x720/q5", &[0x00000002, 0x00000000, 0x3efffffa, 0x3f000002, 0x3fc00000]),
    ("2d/ortho@1280x720/q6", &[0x00000002, 0x00000000, 0x3f1aaaa8, 0x3f0b8e3b, 0x3fc00000]),
    ("2d/ortho@1280x720/q7", &[0x00000003, 0x00000000, 0xc0400000, 0xbfffffff, 0x3f800000]),
    ("2d/ortho@1280x720/q8", &[0x00000003, 0x00000000, 0xc0395555, 0xbffa38e5, 0x3f800000]),
    ("model/persp@800x600/q0", &[0x00000016, 0x00000000, 0x3e8f11dc, 0x3f395610, 0xbfdfe2a0]),
    ("model/persp@800x600/q1", &[]),
    ("model/persp@800x600/q2", &[]),
    ("model/persp@800x600/q3", &[]),
    ("model/persp@800x600/q4", &[]),
    ("model/persp@800x600/q5", &[]),
    ("model/persp@800x600/q6", &[]),
    ("model/persp@800x600/q7", &[0x00000015, 0x00000000, 0x3fe8d605, 0x3efa9784, 0xc054eb18]),
    ("model/persp@800x600/q8", &[0x00000016, 0x00000000, 0xbf753d8a, 0x3e0c04c8, 0x3e800000]),
    ("model/persp@800x600/q9", &[0x00000016, 0x00000000, 0xbf6d6fea, 0x3e17dcd8, 0x3e800000]),
    ("model/persp@800x600/q10", &[0x00000016, 0x00000000, 0xbddcff14, 0x3f1b1409, 0xbfba66a4]),
    ("model/persp@800x600/q11", &[0x00000017, 0x00000000, 0xbfffffff, 0x3f800002, 0xc07ffffa]),
    ("model/persp@800x600/q12", &[0x00000017, 0x00000000, 0xbffd253c, 0x3f81a5ea, 0xc087fca8]),
    ("model/persp@800x600/q13", &[]),
    ("model/persp@800x600/q14", &[]),
    ("model/persp@800x600/q15", &[]),
];

const BOUNDS: &[(&str, &[u32])] = &[
    ("model_parent_anim", &[0xbe671bc8, 0x3ffbd60d, 0xc0362838, 0x408b1985]),
    ("node_select", &[0x408bc222, 0xbe118320, 0x3f800000, 0x3fe17548]),
    ("legacy_only", &[0x3faeaaab, 0x3f000000, 0xbdcdcdd0, 0x4025682d]),
];


// ─────────────────────────── 断言(逐位全等) ───────────────────────────

#[test]
fn golden_editor_camera_view_proj_and_ray() {
    check(editor_out(), EDITOR);
}

#[test]
fn golden_scene_camera_view_proj_and_ray() {
    check(scene_cam_out(), SCENE_CAM);
}

#[test]
fn golden_sprite_render_transform() {
    check(sprite_out(), SPRITE_XFORM);
}

#[test]
fn golden_ray_unit_cube() {
    check(ray_cube_out(), RAY_CUBE);
}

#[test]
fn golden_pick_entity() {
    check(pick_out(), PICK);
}

#[test]
fn golden_modelrender_bounds() {
    check(bounds_out(), BOUNDS);
    let errors: Vec<(&str, Option<String>)> =
        bounds_err_cases().into_iter().map(|(n, s)| (n, crate::modelrender::bounds(&s).err())).collect();
    let expected = [
        ("empty", "model has no visible vertices"),
        ("missing_node", "MODEL_NODE_NOT_FOUND: nope"),
        ("parent_cycle", "MODEL_INVALID: entity parent cycle"),
        ("model_prop_missing", "ModelRenderer.model missing"),
    ];
    assert_eq!(errors, expected.map(|(n, e)| (n, Some(e.to_string()))).to_vec());
}

// ─────────────────────────── 搬迁后追加:模型腿 eye 与 ViewSetup ───────────────────────────
//
// MODEL_EYE 的位模式同样是**搬迁前**抓的:当时逐字拷贝 modelrender::render 开头的 eye 表达式求值。
// 搬迁后该表达式成了 render_core::camera::model_eye,这里锁定两者逐位相同。

/// 模型腿 eye 的用例:PIE(有 vp)取相机实体世界位置(走 Parent 链),否则 / 取不到时退 cam.eye()。
fn model_eye_cases() -> Vec<(&'static str, Scene, EditorCamera, bool)> {
    let cam = EditorCamera { target: [0.5, 0.5, -3.0], yaw_deg: -10.0, pitch_deg: 10.0, dist: 6.0, ..EditorCamera::default() };
    let mut cycle = Scene::new("golden-eye-cycle");
    cycle.entities.push(ent(60, tr([1.0; 3], ID_ROT, [1.0; 3]), vec![comp("Parent", json!({"entity": 61})), comp("Camera", json!({}))]));
    cycle.entities.push(ent(61, Transform::default(), vec![comp("Parent", json!({"entity": 60}))]));
    // 首个 Camera 组件被禁用的实体被跳过(即使它还有启用的第二个 Camera)——与场景相机选法不同。
    let mut first_disabled = Scene::new("golden-eye-first-disabled");
    first_disabled.entities.push(ent(
        70,
        tr([9.0, 9.0, 9.0], ID_ROT, [1.0; 3]),
        vec![disabled("Camera", json!({})), comp("Camera", json!({}))],
    ));
    first_disabled.entities.push(ent(71, tr([-2.0, 3.0, 1.0], [0.0, 0.3826834, 0.0, 0.9238795], [2.0; 3]), vec![]));
    first_disabled.entities.push(ent(
        72,
        tr([0.5, 0.25, 4.0], ID_ROT, [1.0; 3]),
        vec![comp("Parent", json!({"entity": 71})), comp("Camera", json!({}))],
    ));
    vec![
        ("model_scene/pie", model_scene(), cam, true),
        ("model_scene/edit", model_scene(), cam, false),
        ("no_camera/pie", pick_scene_3d(), cam, true),
        ("camera_cycle/pie", cycle, cam, true),
        ("first_camera_disabled/pie", first_disabled, cam, true),
    ]
}

const MODEL_EYE: &[(&str, &[u32])] = &[
    ("model_scene/pie", &[0x40a7c3b6, 0x40000000, 0x3f9f0edc]),
    ("model_scene/edit", &[0xbf06abe6, 0x3fc55c9f, 0x40346bc4]),
    ("no_camera/pie", &[0xbf06abe6, 0x3fc55c9f, 0x40346bc4]),
    ("camera_cycle/pie", &[0xbf06abe6, 0x3fc55c9f, 0x40346bc4]),
    ("first_camera_disabled/pie", &[0x408ba592, 0x40600000, 0x40be6456]),
];


#[test]
fn golden_model_leg_eye() {
    let out: Out = model_eye_cases()
        .into_iter()
        .map(|(n, s, c, pie)| (n.to_string(), b3(crate::render_core::camera::model_eye(&s, &c, pie))))
        .collect();
    check(out, MODEL_EYE);
}

use crate::render_core::camera::{editor_view, scene_view, view_basis, Projection, ViewSetup, ViewSource};
use crate::render_core::math::{look_at_rh, m4_mul, orthographic_vk, perspective_vk, M4};

/// 只用 ViewSetup 的分解字段、按 rurix 同算法重算 view_proj(Godot 腿拿到的正是这些字段)。
fn rebuild_view_proj(v: &ViewSetup) -> M4 {
    let mut proj = match v.projection {
        Projection::Orthographic { half_h } => orthographic_vk(half_h, v.aspect.max(1e-6), v.near, v.far),
        Projection::Perspective { fov_y_deg } => perspective_vk(fov_y_deg.to_radians(), v.aspect.max(1e-6), v.near, v.far),
    };
    proj[1][1] = -proj[1][1];
    m4_mul(proj, look_at_rh(v.eye, v.center, [0.0, 1.0, 0.0]))
}

/// view_basis 必须与 look_at_rh 的前三行(s, u, -f)逐位一致。
fn assert_basis_matches_look_at(label: &str, v: &ViewSetup) {
    let (s, u, f) = view_basis(v);
    let view = look_at_rh(v.eye, v.center, [0.0, 1.0, 0.0]);
    for i in 0..3 {
        assert_eq!(s[i].to_bits(), view[0][i].to_bits(), "{label}: right[{i}]");
        assert_eq!(u[i].to_bits(), view[1][i].to_bits(), "{label}: up[{i}]");
        assert_eq!((-f[i]).to_bits(), view[2][i].to_bits(), "{label}: -forward[{i}]");
    }
}

/// 02 §3.8 第 4 条:ViewSetup.view_proj 与 cam.view_proj(aspect) / scene_camera_view_proj 逐位相等;
/// 另外用分解字段重算也须逐位相等,保证 Godot 腿按字段建相机时与 rurix 同源。
#[test]
fn view_setup_view_proj_is_bitwise_rurix() {
    for (name, cam, a) in editor_cases() {
        let v = editor_view(&cam, a);
        assert_eq!(v.source, ViewSource::Editor, "{name}");
        assert_eq!(v.aspect.to_bits(), a.to_bits(), "{name}: aspect");
        assert_eq!(b4x4(v.view_proj), b4x4(cam.view_proj(a)), "{name}: editor_view.view_proj");
        assert_eq!(b4x4(rebuild_view_proj(&v)), b4x4(v.view_proj), "{name}: rebuilt from fields");
        assert_basis_matches_look_at(name, &v);
    }
    let camera_ids = [Some(2), Some(2), Some(1), Some(4), Some(11), Some(2), None];
    let cases = scene_cam_cases();
    assert_eq!(cases.len(), camera_ids.len());
    for ((name, scene, a), id) in cases.into_iter().zip(camera_ids) {
        let v = scene_view(&scene, a);
        let rurix = crate::viewport::scene_camera_view_proj(&scene, a);
        assert_eq!(v.map(|v| b4x4(v.view_proj)), rurix.map(b4x4), "{name}: scene_view.view_proj");
        assert_eq!(v.map(|v| v.source), id.map(|entity| ViewSource::SceneCamera { entity }), "{name}: source");
        if let Some(v) = v {
            assert_eq!(v.aspect.to_bits(), a.to_bits(), "{name}: aspect");
            assert_eq!(b4x4(rebuild_view_proj(&v)), b4x4(v.view_proj), "{name}: rebuilt from fields");
            assert_basis_matches_look_at(name, &v);
        }
    }
}
