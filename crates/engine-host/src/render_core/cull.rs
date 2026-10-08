//! 屏外裁剪(02 §3.3),自 viewport.rs 逐字搬来。输入是本帧的 view_proj(`frame_vp`);
//! Godot 腿传 `ViewSetup.view_proj`(同一矩阵),两后端的可见集因此相同。

use super::math::{trs_model, M4};
use super::sprite::sprite_render_transform;

/// 屏外裁剪的边距倍数:只裁「停车位」级别的远离(四角全在 3 倍视口范围之外),
/// 贴边进出的实体(右侧刷出的僵尸、飞出屏的豌豆、开走的小推车)不裁——
/// 可见集每变一次 pass 会话就要按新的贴图槽签名重建,逐帧进出会造成重建抖动。
#[cfg_attr(not(feature = "backend-rurix"), allow(dead_code))]
const OFFSCREEN_CULL_MARGIN: f32 = 3.0;

/// Sprite 实体是否停在远屏外(保守四角裁剪):渲染态 quad 四角经 view_proj 投到裁剪空间,
/// 四角同侧越界(全在 x>3w / x<-3w / y>3w / y<-3w)或全在相机后方即视为屏外。
/// 用途:2D 游戏对象池把闲置实体停在屏外(如 y=-60),此前仍逐个占 draw 槽,
/// 128 槽预算被池子吃掉;屏外精灵不进 renderables 后,槽位只留给真正可见的实体。
/// 非 Sprite 实体(3D 网格/cube)不裁,行为不变。
#[cfg_attr(not(feature = "backend-rurix"), allow(dead_code))]
pub(crate) fn sprite_offscreen(e: &forge_scene::Entity, vp: &M4) -> bool {
    let Some(tr) = sprite_render_transform(e) else {
        return false;
    };
    let model = trs_model(&tr);
    let mut outside = [true; 5]; // +x, -x, +y, -y, behind
    for (lx, ly) in [(-0.5f32, -0.5f32), (0.5, -0.5), (0.5, 0.5), (-0.5, 0.5)] {
        let local = [lx, ly, 0.0, 1.0];
        let mut world = [0.0f32; 4];
        for r in 0..4 {
            world[r] = (0..4).map(|k| model[r][k] * local[k]).sum();
        }
        let mut clip = [0.0f32; 4];
        for r in 0..4 {
            clip[r] = (0..4).map(|k| vp[r][k] * world[k]).sum();
        }
        let w = clip[3];
        if w > 1e-6 {
            outside[4] = false;
        }
        let bound = w.abs().max(1e-6) * OFFSCREEN_CULL_MARGIN;
        if clip[0] <= bound {
            outside[0] = false;
        }
        if clip[0] >= -bound {
            outside[1] = false;
        }
        if clip[1] <= bound {
            outside[2] = false;
        }
        if clip[1] >= -bound {
            outside[3] = false;
        }
    }
    outside.iter().any(|o| *o)
}
