//! RenderDelta(02 §2.5):[gmain] 私有的"已应用清单 → 新清单"差量。两份 items 都按 key 升序,归并遍历;
//! world 逐元素按 f32::to_bits 比较(不设容差,NaN 也稳定)。后端中立,不含任何 GPU 句柄。

use std::sync::Arc;

use super::camera::ViewSetup;
use super::env::{SceneEnv, VolumeItem};
use super::list::{ItemKey, LightItem, ParticleItem, RenderItem, RenderList, V6Frame};
use super::math::M4;

#[derive(Debug, Clone, PartialEq, Default)]
pub struct RenderDelta {
    /// 首帧 / leg 变 / asset_generation 变 / mode_2d 变 → 全量重建(removed = 旧的全部,added = 新的全部)。
    pub full: bool,
    pub to_seq: u64,
    pub resize: Option<(u32, u32)>,
    pub view: Option<ViewSetup>,
    pub clear_rgba: Option<[f32; 4]>,
    pub removed: Vec<ItemKey>,
    pub added: Vec<RenderItem>,
    /// 同 key、content 变:换网格 / 材质。
    pub content: Vec<RenderItem>,
    /// 同 key 同 content、只有 world 变:只调 transform 类接口。
    pub moved: Vec<(ItemKey, M4)>,
    /// 同 key 同 content、骨骼姿态变(动画):只调 skeleton_bone_set_transform(可与 moved 同时出现)。
    pub posed: Vec<(ItemKey, Arc<Vec<M4>>)>,
    /// 灯光集合变了(或全量):整体换成这一份(灯很少,不做逐盏差量)。
    pub lights: Option<Vec<LightItem>>,
    /// V6 帧变了(或全量):整体换(MultiMesh 缓冲整块上传;地形 Arc 身份不变就不重建地形网格)。
    pub v6: Option<Arc<V6Frame>>,
    /// 粒子事件变了(或全量):整体换,Godot 侧按槽位对比年龄决定 restart / pre_process。
    pub particles: Option<Vec<ParticleItem>>,
    /// Stage 5:场景级设置变了(或全量):整体换(Environment / CameraAttributes / RenderSettings 各自重配)。
    pub env: Option<SceneEnv>,
    /// Stage 5:体积类实体集合变了(或全量):整体换,Godot 侧按 key + content 自己分"重建 / 只移动 / 删除"。
    pub volumes: Option<Vec<VolumeItem>>,
    /// 共有 key 的绘制序变了(2D sorting / 透明远近序;不透明 3D 只作记录)。
    pub reordered: bool,
}

fn pose_bits(p: &Option<Arc<Vec<M4>>>) -> Option<Vec<u32>> {
    p.as_ref().map(|v| v.iter().flat_map(|m| m.iter().flatten().map(|f| f.to_bits())).collect())
}

fn world_bits(m: &M4) -> [u32; 16] {
    let mut out = [0u32; 16];
    for (i, v) in m.iter().flatten().enumerate() {
        out[i] = v.to_bits();
    }
    out
}

fn view_bits(v: &ViewSetup) -> Vec<u32> {
    let p = match v.projection {
        super::camera::Projection::Perspective { fov_y_deg } => [0, fov_y_deg.to_bits()],
        super::camera::Projection::Orthographic { half_h } => [1, half_h.to_bits()],
    };
    let mut b: Vec<u32> = v.eye.iter().chain(v.center.iter()).map(|f| f.to_bits()).collect();
    b.extend_from_slice(&p);
    b.extend([v.near.to_bits(), v.far.to_bits(), v.aspect.to_bits()]);
    b.extend(world_bits(&v.view_proj));
    b
}

fn draw_order(items: &[RenderItem], keep: impl Fn(&ItemKey) -> bool) -> Vec<ItemKey> {
    let mut v: Vec<(u32, ItemKey)> = items.iter().filter(|i| keep(&i.key)).map(|i| (i.order, i.key)).collect();
    v.sort();
    v.into_iter().map(|(_, k)| k).collect()
}

impl RenderDelta {
    pub fn diff(applied: Option<&RenderList>, next: &RenderList) -> RenderDelta {
        let full = applied.is_none_or(|a| {
            a.leg != next.leg || a.asset_generation != next.asset_generation || a.mode_2d != next.mode_2d || a.canvas_2d != next.canvas_2d
        });
        let mut d = RenderDelta { full, to_seq: next.seq, ..RenderDelta::default() };
        let prev = match applied {
            Some(a) if !full => a,
            _ => {
                d.resize = Some((next.width, next.height));
                d.view = Some(next.view);
                d.clear_rgba = Some(next.clear_rgba);
                d.removed = applied.map(|a| a.items.iter().map(|i| i.key).collect()).unwrap_or_default();
                d.added = next.items.clone();
                d.lights = Some(next.lights.clone());
                d.v6 = next.v6.clone();
                d.particles = Some(next.particles.clone());
                d.env = Some(next.env.clone());
                d.volumes = Some(next.volumes.clone());
                return d;
            }
        };
        if (prev.width, prev.height) != (next.width, next.height) {
            d.resize = Some((next.width, next.height));
        }
        if view_bits(&prev.view) != view_bits(&next.view) {
            d.view = Some(next.view);
        }
        if prev.clear_rgba.map(f32::to_bits) != next.clear_rgba.map(f32::to_bits) {
            d.clear_rgba = Some(next.clear_rgba);
        }
        if prev.lights != next.lights {
            d.lights = Some(next.lights.clone());
        }
        if prev.v6 != next.v6 {
            d.v6 = next.v6.clone();
        }
        if prev.particles != next.particles {
            d.particles = Some(next.particles.clone());
        }
        if prev.env != next.env {
            d.env = Some(next.env.clone());
        }
        if prev.volumes != next.volumes {
            d.volumes = Some(next.volumes.clone());
        }
        let (a, b) = (&prev.items, &next.items);
        let (mut i, mut j) = (0, 0);
        while i < a.len() || j < b.len() {
            match (a.get(i), b.get(j)) {
                (Some(x), Some(y)) if x.key == y.key => {
                    if x.content != y.content || x.body != y.body {
                        d.content.push(y.clone());
                    } else {
                        if world_bits(&x.world) != world_bits(&y.world) {
                            d.moved.push((y.key, y.world));
                        }
                        if pose_bits(&x.pose) != pose_bits(&y.pose) {
                            if let Some(p) = &y.pose {
                                d.posed.push((y.key, Arc::clone(p)));
                            } else {
                                d.content.push(y.clone());
                            }
                        }
                    }
                    i += 1;
                    j += 1;
                }
                (Some(x), Some(y)) if x.key < y.key => {
                    d.removed.push(x.key);
                    i += 1;
                }
                (Some(x), None) => {
                    d.removed.push(x.key);
                    i += 1;
                }
                (_, Some(y)) => {
                    d.added.push(y.clone());
                    j += 1;
                }
                (None, None) => break,
            }
        }
        let common = |k: &ItemKey| a.binary_search_by(|x| x.key.cmp(k)).is_ok() && b.binary_search_by(|x| x.key.cmp(k)).is_ok();
        d.reordered = draw_order(a, common) != draw_order(b, common);
        d
    }

    pub fn is_empty(&self) -> bool {
        !self.full
            && self.resize.is_none()
            && self.view.is_none()
            && self.clear_rgba.is_none()
            && self.removed.is_empty()
            && self.added.is_empty()
            && self.content.is_empty()
            && self.moved.is_empty()
            && self.posed.is_empty()
            && self.lights.is_none()
            && self.v6.is_none()
            && self.particles.is_none()
            && self.env.is_none()
            && self.volumes.is_none()
            && !self.reordered
    }
}


#[cfg(test)]
mod tests {
    use super::*;
    use crate::render_core::list::{ItemBody, MeshData, MeshRef};

    fn item(entity: u64, order: u32, x: f32, color: f32) -> RenderItem {
        let v = crate::render_core::assets::cube_mesh_bytes();
        let mesh = MeshData { id: MeshRef::Cube, vertices: v, vertex_count: 36, fallback: false };
        let mut world = [[0.0f32; 4]; 4];
        for (k, row) in world.iter_mut().enumerate() {
            row[k] = 1.0;
        }
        world[0][3] = x;
        let key = crate::render_core::list::ItemKey { entity, sub: 0 };
        RenderItem { key, order, world, content: color.to_bits() as u64, body: ItemBody::Mesh { mesh, color: [color, 0.0, 0.0, 1.0] }, pose: None }
    }

    fn list(seq: u64, items: Vec<RenderItem>) -> RenderList {
        let cam = crate::render_core::camera::EditorCamera::default();
        RenderList {
            seq, scene_rev: 0, width: 64, height: 32, mode_2d: false, canvas_2d: false, leg: crate::render_core::list::Leg::SpriteMesh,
            view: crate::render_core::camera::editor_view(&cam, 2.0), clear_rgba: [0.1, 0.1, 0.1, 1.0], selected: None,
            want_pixels: true, want_stats: false, items, lights: Vec::new(), v6: None, particles: Vec::new(),
            env: Default::default(), volumes: Vec::new(), stats: Default::default(), asset_generation: 0,
        }
    }

    #[test]
    fn pose_and_light_changes_are_separate_from_content() {
        let mut a = list(1, vec![item(1, 0, 0.0, 0.5)]);
        a.items[0].pose = Some(Arc::new(vec![[[1.0, 0.0, 0.0, 0.0]; 4]]));
        let mut b = list(2, a.items.clone());
        b.items[0].pose = Some(Arc::new(vec![[[2.0, 0.0, 0.0, 0.0]; 4]]));
        b.items[0].world[0][3] = 7.0;
        let d = RenderDelta::diff(Some(&a), &b);
        assert!(d.content.is_empty() && d.moved.len() == 1 && d.posed.len() == 1, "{d:?}");
        assert!(d.lights.is_none());
        let mut c = list(3, b.items.clone());
        c.lights.push(crate::render_core::list::LightItem {
            key: crate::render_core::list::ItemKey { entity: 9, sub: 0 },
            kind: crate::render_core::list::LightKind::Point, kind_known: true, color: [1.0; 3], intensity: 2.0,
            cast_shadow: false, world: b.items[0].world, params: None,
        });
        let d = RenderDelta::diff(Some(&b), &c);
        assert_eq!(d.lights.as_ref().map(Vec::len), Some(1));
        assert!(!d.is_empty() && d.moved.is_empty() && d.posed.is_empty());
        assert!(RenderDelta::diff(Some(&c), &list(4, c.items.clone())).lights == Some(Vec::new()), "灯全删也要下发");
        assert!(RenderDelta::diff(None, &c).lights.is_some(), "全量带灯");
    }

    #[test]
    fn first_frame_and_generation_change_are_full_rebuilds() {
        let a = list(1, vec![item(1, 0, 0.0, 0.5)]);
        let d = RenderDelta::diff(None, &a);
        assert!(d.full && d.resize == Some((64, 32)) && d.view.is_some() && d.added.len() == 1 && d.removed.is_empty());
        let mut b = list(2, vec![item(1, 0, 0.0, 0.5)]);
        b.asset_generation = 1;
        let d = RenderDelta::diff(Some(&a), &b);
        assert!(d.full && d.removed.len() == 1 && d.added.len() == 1, "{d:?}");
    }

    #[test]
    fn merge_walk_classifies_items() {
        let a = list(1, vec![item(1, 0, 0.0, 0.5), item(2, 1, 0.0, 0.5), item(4, 2, 0.0, 0.5)]);
        let b = list(2, vec![item(2, 1, 3.0, 0.5), item(3, 0, 0.0, 0.5), item(4, 2, 0.0, 0.9)]);
        let d = RenderDelta::diff(Some(&a), &b);
        assert!(!d.full && d.resize.is_none() && d.view.is_none() && d.clear_rgba.is_none());
        assert_eq!(d.removed.iter().map(|k| k.entity).collect::<Vec<_>>(), [1]);
        assert_eq!(d.added.iter().map(|i| i.key.entity).collect::<Vec<_>>(), [3]);
        assert_eq!(d.content.iter().map(|i| i.key.entity).collect::<Vec<_>>(), [4]);
        assert_eq!(d.moved.iter().map(|(k, m)| (k.entity, m[0][3])).collect::<Vec<_>>(), [(2, 3.0)]);
        assert!(!d.reordered && !d.is_empty());
        assert!(RenderDelta::diff(Some(&b), &list(3, b.items.clone())).is_empty(), "同内容 = 空差量");
    }

    #[test]
    fn resize_view_clear_and_order_changes_are_reported() {
        let a = list(1, vec![item(1, 0, 0.0, 0.5), item(2, 1, 0.0, 0.5)]);
        let mut b = list(2, vec![item(1, 1, 0.0, 0.5), item(2, 0, 0.0, 0.5)]);
        b.width = 128;
        b.clear_rgba = [0.2, 0.1, 0.1, 1.0];
        b.view.eye[0] += 1.0;
        let d = RenderDelta::diff(Some(&a), &b);
        assert_eq!(d.resize, Some((128, 32)));
        assert!(d.view.is_some() && d.clear_rgba.is_some() && d.reordered);
    }

    /// Stage 5:Environment / 体积类实体只在变化时下发;全量帧恒带(缺省 = 空的 SceneEnv,即 Stage 4 的缺省环境)。
    #[test]
    fn env_and_volume_changes_are_separate_and_full_frames_carry_them() {
        use forge_scene::{Component, Entity, Transform};
        let a = list(1, vec![item(1, 0, 0.0, 0.5)]);
        let full = RenderDelta::diff(None, &a);
        assert_eq!(full.env, Some(SceneEnv::default()));
        assert_eq!(full.volumes, Some(Vec::new()));
        assert!(RenderDelta::diff(Some(&a), &list(2, a.items.clone())).is_empty(), "没有 Stage 5 组件时不下发");
        let mut s = forge_scene::Scene::new("e");
        s.entities.push(Entity { entity_guid: None, id: 3, name: "env".into(), transform: Transform::default(), components: vec![
            Component::new("Environment", serde_json::json!({ "tonemap": "agx" })),
            Component::new("FogVolume", serde_json::json!({})),
        ] });
        let mut stats = Default::default();
        let mut b = list(3, a.items.clone());
        b.env = crate::render_core::env::scene_env(&s, std::path::Path::new("."));
        let d = RenderDelta::diff(Some(&a), &b);
        assert!(d.env.is_some() && d.volumes.is_none() && d.added.is_empty() && d.content.is_empty() && d.lights.is_none());
        let mut c = list(4, a.items.clone());
        c.env = b.env.clone();
        c.volumes = crate::render_core::env::volumes(&s, std::path::Path::new("."), &mut stats);
        let d = RenderDelta::diff(Some(&b), &c);
        assert!(d.env.is_none() && d.volumes.as_ref().is_some_and(|v| v.len() == 1 && v[0].kind == "FogVolume"));
        assert!(!d.is_empty());
    }
}
