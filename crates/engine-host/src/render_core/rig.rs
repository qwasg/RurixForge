//! F-GAME-5(D-050)Rig2D 渲染求值(后端中立,02 §3):props → `pose_from` → `evaluate` →
//! `expand_triangles` → 交错顶点(pos3 nrm3 uv2,stride 32 B)。rurix 视口腿与 Godot RenderList
//! (`ItemBody::RigMesh`)共用这一份 CPU 结果,点选用同一次求值的形变包围盒。
//!
//! 顶点约定(契约 F):
//! - pos = `to_local_units(画布像素)`(Y 翻转、除 ppu、以 canvas.origin 为原点);flipX / flipY 在 CPU
//!   端取反;z = 绘制名次 × 1e-4(后画者朝相机)。
//! - nrm = (0, 0, opacity):逐部件透明度走法线 z 通道(rurix FS_RIG / Godot 逐顶点颜色读取),
//!   不透明部件法线恰为 (0,0,1)。
//! - uv = 图集像素 / 图集尺寸(0..1)。
//! - 绕序:rex 光栅不做背面剔除(CULL_MODE_NONE),Godot spatial 着色器 cull_disabled;仍把每个三角形
//!   统一成本地坐标系(Y 向上)逆时针——Y 翻转与单轴 flip 会反转作者绕序,统一后任何剔除配置都稳定。

use std::collections::BTreeMap;
use std::sync::Arc;

use forge_rig2d::CompiledRig;
use forge_scene::{Component, Entity, Transform};
use serde_json::Value;

use super::assets::{load_tex_static_cached, rig_doc_cached, TexGpu};
use super::math::{m3_apply, quat_to_mat3, v3_add};

/// 每顶点 f32 数(pos3 + nrm3 + uv2)。
pub const RIG_VERTEX_F32: usize = 8;
/// 动态顶点缓冲最小容量(顶点数;= 256 三角形)。
pub const RIG_MIN_CAPACITY: u32 = 768;

/// 容量桶:`max(768, next_pow2(n))`(rurix 会话签名只认桶,姿态 / 可见部件数变化不重建会话)。
pub fn rig_capacity(vertex_count: u32) -> u32 {
    if vertex_count <= RIG_MIN_CAPACITY {
        RIG_MIN_CAPACITY
    } else {
        vertex_count.next_power_of_two()
    }
}

/// 启用的 Rig2D 组件。
pub(crate) fn rig_component(e: &Entity) -> Option<&Component> {
    e.components.iter().find(|c| c.ctype == "Rig2D" && c.enabled)
}

/// 实体是否以 Rig2D 参与渲染 / 点选(rig GUID 非空)。
pub(crate) fn is_rig(e: &Entity) -> bool {
    rig_component(e).is_some_and(|c| !rig_guid(c).is_empty())
}

pub(crate) fn rig_guid(c: &Component) -> &str {
    c.props.get("rig").and_then(Value::as_str).unwrap_or("")
}

fn num(c: &Component, key: &str, default: f64) -> f64 {
    c.props.get(key).and_then(Value::as_f64).unwrap_or(default)
}

fn flag(c: &Component, key: &str) -> bool {
    c.props.get(key).and_then(Value::as_bool).unwrap_or(false)
}

/// Rig2D.params(dict)→ 参数覆盖表;非数值项忽略。
pub(crate) fn rig_params(c: &Component) -> BTreeMap<String, f32> {
    c.props
        .get("params")
        .and_then(Value::as_object)
        .map(|m| m.iter().filter_map(|(k, v)| v.as_f64().map(|f| (k.clone(), f as f32))).collect())
        .unwrap_or_default()
}

pub(crate) fn rig_tint(c: &Component) -> [f32; 4] {
    let a = c.props.get("tint").and_then(Value::as_array);
    std::array::from_fn(|i| a.and_then(|a| a.get(i)).and_then(Value::as_f64).unwrap_or(1.0) as f32)
}

pub(crate) fn rig_sorting_order(c: &Component) -> f64 {
    num(c, "sortingOrder", 0.0)
}

/// 一帧 CPU 求值结果。
#[derive(Debug, Clone, PartialEq)]
pub struct RigFrame {
    /// 非索引三角形列表,`vertex_count × 8` 个 f32(不含容量填充)。
    pub verts: Vec<f32>,
    pub vertex_count: u32,
    pub capacity: u32,
    /// 本地世界单位包围盒 [minX, minY, maxX, maxY](flip 后;无可见部件时全 0)。
    pub bbox: [f32; 4],
    /// pose_from 未识别的名字(`param:<id>` / `clip:<name>`)。
    pub unknown: Vec<String>,
}

/// 纯函数求值(无 IO;金值测试直接调用)。`atlas` = 图集像素尺寸(uv 归一化用)。
pub fn build_rig_frame(
    rig: &CompiledRig,
    params: &BTreeMap<String, f32>,
    clip: Option<&str>,
    time: f32,
    atlas: [u32; 2],
    flip: [bool; 2],
) -> RigFrame {
    let (pose, unknown) = forge_rig2d::pose_from(rig, params, clip, time);
    let deformed = forge_rig2d::evaluate(rig, &pose);
    let tri_verts = forge_rig2d::expand_triangles(rig, &deformed);
    let (aw, ah) = (atlas[0].max(1) as f32, atlas[1].max(1) as f32);
    let sx = if flip[0] { -1.0 } else { 1.0 };
    let sy = if flip[1] { -1.0 } else { 1.0 };
    let mut verts = Vec::with_capacity(tri_verts.len() * RIG_VERTEX_F32);
    let mut bbox = [f32::INFINITY, f32::INFINITY, f32::NEG_INFINITY, f32::NEG_INFINITY];
    for tri in tri_verts.chunks_exact(3) {
        let mut p = [[0.0f32; 2]; 3];
        for (k, v) in tri.iter().enumerate() {
            let l = rig.to_local_units(v.pos);
            p[k] = [l[0] * sx, l[1] * sy];
        }
        // 统一为本地(Y 向上)逆时针:有符号面积 < 0 → 交换后两个顶点。
        let area = (p[1][0] - p[0][0]) * (p[2][1] - p[0][1]) - (p[2][0] - p[0][0]) * (p[1][1] - p[0][1]);
        let order: [usize; 3] = if area < 0.0 { [0, 2, 1] } else { [0, 1, 2] };
        for k in order {
            let v = &tri[k];
            let q = p[k];
            bbox[0] = bbox[0].min(q[0]);
            bbox[1] = bbox[1].min(q[1]);
            bbox[2] = bbox[2].max(q[0]);
            bbox[3] = bbox[3].max(q[1]);
            verts.extend_from_slice(&[
                q[0],
                q[1],
                v.layer as f32 * 1e-4,
                0.0,
                0.0,
                v.alpha,
                v.uv[0] / aw,
                v.uv[1] / ah,
            ]);
        }
    }
    if verts.is_empty() {
        bbox = [0.0; 4];
    }
    let vertex_count = (verts.len() / RIG_VERTEX_F32) as u32;
    RigFrame { verts, vertex_count, capacity: rig_capacity(vertex_count), bbox, unknown }
}

/// 实体的完整 Rig2D 渲染解析(rurix 腿 / RenderList / 点选共用出口)。
pub struct RigRender {
    pub tex: &'static TexGpu,
    pub tex_guid: String,
    pub frame: RigFrame,
    pub tint: [f32; 4],
    pub flip: [bool; 2],
    pub sorting_order: f64,
}

/// 文档与图集均可解析(不求值;Canvas 判据用)。
pub(crate) fn rig_resolvable(e: &Entity, project_root: &std::path::Path) -> bool {
    rig_component(e)
        .map(rig_guid)
        .filter(|g| !g.is_empty())
        .and_then(rig_doc_cached)
        .is_some_and(|rig| load_tex_static_cached(project_root, &rig.doc.texture).is_some())
}

/// 解析失败(无 Rig2D / GUID 空 / 文档或图集不可解析)→ None(实体本帧不画,不回退 cube)。
pub(crate) fn resolve_rig_render(e: &Entity, project_root: &std::path::Path) -> Option<RigRender> {
    let c = rig_component(e)?;
    let guid = rig_guid(c);
    if guid.is_empty() {
        return None;
    }
    let rig = rig_doc_cached(guid)?;
    let tex = load_tex_static_cached(project_root, &rig.doc.texture)?;
    let flip = [flag(c, "flipX"), flag(c, "flipY")];
    let clip = c.props.get("clip").and_then(Value::as_str);
    let time = num(c, "time", 0.0) as f32;
    let frame = build_rig_frame(&rig, &rig_params(c), clip, time, [tex.w, tex.h], flip);
    Some(RigRender {
        tex,
        tex_guid: rig.doc.texture.clone(),
        frame,
        tint: rig_tint(c),
        flip,
        sorting_order: rig_sorting_order(c),
    })
}

/// 本地包围盒 → 点选用单位立方体等效变换(与 `sprite_render_transform` 并列,画面与点选一致)。
pub(crate) fn bbox_transform(e: &Entity, bbox: [f32; 4]) -> Transform {
    let s = e.transform.scale;
    let (w, h) = ((bbox[2] - bbox[0]).max(1e-3), (bbox[3] - bbox[1]).max(1e-3));
    let center = [(bbox[0] + bbox[2]) * 0.5 * s[0], (bbox[1] + bbox[3]) * 0.5 * s[1], 0.0];
    let off = m3_apply(quat_to_mat3(e.transform.rotation), center);
    Transform {
        translation: v3_add(e.transform.translation, off),
        rotation: e.transform.rotation,
        scale: [s[0] * w, s[1] * h, s[2].max(1e-3)],
    }
}

/// Rig2D 实体点选变换(形变后包围盒);非 rig / 解析失败 → None。
pub(crate) fn rig_pick_transform(e: &Entity) -> Option<Transform> {
    let r = resolve_rig_render(e, &crate::rpc::project_root())?;
    Some(bbox_transform(e, r.frame.bbox))
}

/// 把顶点填充到容量(尾部全 0 = 退化三角形 + opacity 0)并转小端字节(rurix 每帧上传)。
#[cfg_attr(not(feature = "backend-rurix"), allow(dead_code))]
pub fn padded_vertex_bytes(verts: &[f32], capacity: u32) -> Vec<u8> {
    let total = capacity as usize * RIG_VERTEX_F32;
    let mut out = Vec::with_capacity(total * 4);
    for f in verts.iter().take(total) {
        out.extend_from_slice(&f.to_le_bytes());
    }
    out.resize(total * 4, 0);
    out
}

/// RenderList 用:顶点数据共享(Arc,RenderDelta 比较内容位模式)。
pub(crate) fn frame_arc(frame: RigFrame) -> (Arc<Vec<f32>>, u32, u32, [f32; 4]) {
    (Arc::new(frame.verts), frame.vertex_count, frame.capacity, frame.bbox)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use serde_json::json;

    /// 单部件矩形 rig:画布 200×100,origin 底边中点 (100,100),ppu 100;部件 = 左半 [0..100]×[0..100]
    /// (作者绕序在画布 Y 向下坐标里是顺时针视觉 = 本地 Y 向上后逆时针);第二部件右半、opacity 0.5、drawOrder 更高。
    pub(crate) fn rig_doc_json() -> Value {
        json!({
            "version": 1,
            "texture": "rig-test-atlas-guid",
            "canvas": { "size": [200, 100], "origin": [100, 100] },
            "parameters": [ { "id": "ParamX", "min": -50, "max": 50, "default": 0 } ],
            "parts": [
                { "id": "left", "drawOrder": 0,
                  "mesh": { "vertices": [[0,0],[100,0],[100,100],[0,100]],
                            "uvs": [[0,0],[100,0],[100,100],[0,100]],
                            "triangles": [[0,1,2],[0,2,3]] } },
                { "id": "right", "drawOrder": 10, "opacity": 0.5,
                  "mesh": { "vertices": [[100,0],[200,0],[200,100],[100,100]],
                            "uvs": [[100,0],[200,0],[200,100],[100,100]],
                            "triangles": [[0,2,1],[0,3,2]] } }
            ],
            "keyforms": [
                { "target": "part:left", "params": ["ParamX"], "keys": [[-50, 0, 50]],
                  "forms": [ { "offsets": [[-50,0],[-50,0],[-50,0],[-50,0]] }, {}, { "offsets": [[50,0],[50,0],[50,0],[50,0]] } ] }
            ],
            "clips": { "slide": { "duration": 1, "loop": true, "tracks": [ { "target": "param:ParamX", "keys": [[0, 0], [1, 50]] } ] } }
        })
    }

    pub(crate) fn rig() -> CompiledRig {
        CompiledRig::compile(forge_rig2d::parse_rxrig(&rig_doc_json()).unwrap()).unwrap()
    }

    fn signed_area(v: &[f32], t: usize) -> f32 {
        let p = |k: usize| [v[(t * 3 + k) * 8], v[(t * 3 + k) * 8 + 1]];
        let (a, b, c) = (p(0), p(1), p(2));
        (b[0] - a[0]) * (c[1] - a[1]) - (c[0] - a[0]) * (b[1] - a[1])
    }

    /// 金值(golden.rs 同纪律:逐位比较):rest 姿态顶点 = 画布 → 本地单位的精确值,uv 归一化,
    /// z = 名次 × 1e-4,nrm.z = 部件 opacity,容量桶 768。
    #[test]
    fn golden_rig_frame_rest_pose_bitwise() {
        let f = build_rig_frame(&rig(), &BTreeMap::new(), None, 0.0, [200, 100], [false, false]);
        assert_eq!((f.vertex_count, f.capacity), (12, 768));
        assert!(f.unknown.is_empty());
        let b = |x: f32| x.to_bits();
        // 第 1 个顶点:left 部件画布 (0,0) → 本地 (-1, 1),uv (0,0),layer 0,opacity 1。
        assert_eq!(f.verts[..8].iter().map(|x| b(*x)).collect::<Vec<_>>(),
                   [-1.0f32, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0].map(b));
        // right 部件(layer 1)首顶点 (100,0) → (0,1),opacity 0.5,uv (0.5, 0)。
        assert_eq!(f.verts[6 * 8..7 * 8].iter().map(|x| b(*x)).collect::<Vec<_>>(),
                   [0.0f32, 1.0, 1e-4, 0.0, 0.0, 0.5, 0.5, 0.0].map(b));
        assert_eq!(f.bbox.map(b), [-1.0f32, 0.0, 1.0, 1.0].map(b));
        // 全部三角形统一逆时针(right 部件作者绕序相反,亦被统一)。
        for t in 0..4 {
            assert!(signed_area(&f.verts, t) > 0.0, "三角形 {t} 须为逆时针");
        }
    }

    /// params 覆盖驱动 keyform;clip(time) 求值;flipX 取反 x 且绕序仍逆时针;未知名字如实上报。
    #[test]
    fn rig_frame_params_clip_flip_and_unknown() {
        let r = rig();
        let mut p = BTreeMap::new();
        p.insert("ParamX".to_string(), 50.0);
        let f = build_rig_frame(&r, &p, None, 0.0, [200, 100], [false, false]);
        assert_eq!(f.verts[0], -0.5, "ParamX=50 → left 部件右移 50px = 0.5 单位");
        let c = build_rig_frame(&r, &BTreeMap::new(), Some("slide"), 0.5, [200, 100], [false, false]);
        assert_eq!(c.verts[0], -0.75, "clip slide t=0.5 → ParamX=25");
        let fl = build_rig_frame(&r, &BTreeMap::new(), None, 0.0, [200, 100], [true, false]);
        assert_eq!(fl.bbox, [-1.0, 0.0, 1.0, 1.0]);
        for t in 0..4 {
            assert!(signed_area(&fl.verts, t) > 0.0, "flipX 后三角形 {t} 仍须逆时针");
        }
        p.insert("Nope".into(), 1.0);
        let u = build_rig_frame(&r, &p, Some("nosuch"), 0.0, [200, 100], [false, false]);
        assert_eq!(u.unknown, ["clip:nosuch", "param:Nope"]);
    }

    #[test]
    fn capacity_buckets_and_padding() {
        assert_eq!(rig_capacity(0), 768);
        assert_eq!(rig_capacity(768), 768);
        assert_eq!(rig_capacity(769), 1024);
        assert_eq!(rig_capacity(3000), 4096);
        let bytes = padded_vertex_bytes(&[1.0; 16], 768);
        assert_eq!(bytes.len(), 768 * 32);
        assert!(bytes[64..].iter().all(|b| *b == 0), "容量尾部须全 0(退化三角形)");
    }

    #[test]
    fn bbox_transform_matches_entity_trs() {
        let e = Entity {
            entity_guid: None,
            id: 1,
            name: "r".into(),
            transform: Transform { translation: [1.0, 2.0, 0.0], rotation: [0.0, 0.0, 0.0, 1.0], scale: [2.0, 1.0, 1.0] },
            components: vec![],
        };
        let t = bbox_transform(&e, [-1.0, 0.0, 1.0, 1.0]);
        assert_eq!(t.translation, [1.0, 2.5, 0.0]);
        assert_eq!(t.scale, [4.0, 1.0, 1.0]);
    }

    /// 契约 F:RenderList 抽取 `ItemBody::RigMesh`、姿态变形仅触发 `RenderDelta.rig_verts`(不触发 `content` 重建),
    /// 且 `pick_entity` 按同一次求值的形变包围盒命中 / 未命中。
    #[test]
    fn extract_rig_mesh_delta_and_deformed_bbox_pick() {
        use crate::render::{snapshot, FrameRequester, SnapshotParams};
        use crate::render_core::camera::EditorCamera;
        use crate::render_core::delta::RenderDelta;
        use crate::render_core::list::{extract, ItemBody, SpriteBlend};
        use crate::render_core::pick::pick_entity;
        use forge_scene::Scene;

        crate::rpc::tests::test_project_root();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while crate::render_core::assets::rig_doc_cached("test-rig-hero-guid").is_none() {
            assert!(std::time::Instant::now() < deadline, "rig fixture never became resolvable");
            std::thread::sleep(std::time::Duration::from_millis(50));
        }

        let mk_scene = |param_x: f32| {
            let mut s = Scene::with_mode("rig-extract", "2d");
            s.entities.push(Entity {
                entity_guid: None,
                id: 7,
                name: "hero".into(),
                transform: Transform::default(),
                components: vec![Component::new(
                    "Rig2D",
                    json!({
                        "rig": "test-rig-hero-guid",
                        "params": { "ParamX": param_x },
                        "tint": [1.0, 0.5, 0.25, 1.0],
                        "sortingOrder": 3.0
                    }),
                )],
            });
            s
        };
        // 正交相机 target=(0,0,0), half_h=2, 400×200 (aspect=2, half_w=4):
        // 世界 x=-1.25, y=0.5 → px = (-1.25 + 4)/8 * 400 = 137.5, py = (2 - 0.5)/4 * 200 = 75.0。
        let cam = EditorCamera {
            target: [0.0, 0.0, 0.0],
            yaw_deg: 0.0,
            pitch_deg: 0.0,
            dist: 10.0,
            ortho: true,
            ortho_half_h: 2.0,
            ..EditorCamera::default()
        };
        let snap_of = |scene: Scene, seq: u64| {
            let mut st = crate::rpc::HostState::new();
            st.scene = scene;
            st.camera = cam;
            snapshot(
                &st,
                SnapshotParams {
                    scene_camera: false,
                    width: 400,
                    height: 200,
                    selected: None,
                    want_readback: true,
                    want_stats: true,
                    requester: FrameRequester::ViewportFrame,
                },
                seq,
            )
        };

        let rest_scene = mk_scene(0.0);
        let list_rest = extract(&snap_of(rest_scene.clone(), 1)).unwrap();
        assert!(list_rest.canvas_2d, "纯 2D 正交 Rig2D 场景应走 Canvas 路径");
        assert_eq!((list_rest.items.len(), list_rest.stats.renderables, list_rest.stats.triangles), (1, 1, 4));
        let ItemBody::RigMesh(draw) = &list_rest.items[0].body else {
            panic!("Rig2D 应抽取为 ItemBody::RigMesh, 实际 {:?}", list_rest.items[0].body);
        };
        assert_eq!(draw.tex.guid, "test-rig-atlas-guid");
        assert_eq!((draw.vertex_count, draw.capacity, draw.blend), (12, 768, SpriteBlend::Alpha));
        assert_eq!(draw.tint, [1.0, 0.5, 0.25, 1.0]);
        assert_eq!(draw.sorting_order, 3.0);

        let deformed_scene = mk_scene(-50.0);
        let list_deformed = extract(&snap_of(deformed_scene.clone(), 2)).unwrap();
        assert_eq!(list_rest.items[0].content, list_deformed.items[0].content, "姿态变形不得改变 content 指纹");
        let delta = RenderDelta::diff(Some(&list_rest), &list_deformed);
        assert!(delta.content.is_empty(), "姿态变形不得触发 content 重建: {delta:?}");
        assert_eq!(delta.rig_verts.len(), 1, "姿态变形须通过 rig_verts 下发新顶点");
        assert_eq!(delta.rig_verts[0].0.entity, 7);
        // rest 姿态 bbox = [-1, 0, 1, 1]:x=-1.25 在盒外(miss),x=-0.5 在盒内(hit)。
        assert!(pick_entity(&rest_scene, &cam, 137.5, 75.0, 400, 200).is_none());
        assert_eq!(pick_entity(&rest_scene, &cam, 175.0, 75.0, 400, 200).map(|h| h.0), Some(7));
        // ParamX=-50 使 left 部件左移 0.5 单位,形变后 bbox = [-1.5, 0, 1, 1]:x=-1.25 进入盒内(hit)。
        assert_eq!(
            pick_entity(&deformed_scene, &cam, 137.5, 75.0, 400, 200).map(|h| h.0),
            Some(7),
            "形变后包围盒左扩至 -1.5, x=-1.25 处点选须命中"
        );
    }
}
