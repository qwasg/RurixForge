//! 短锁内抄出的一帧输入(02 §2.5)。rurix 只在推流腿用它(stream::render_loop,§4.5 第 2 项),
//! 字段与原来的 (scene, cam, play, rev, vp_override) 元组同字段、同时机;Pipelined 后端另拿它做 extract。

use std::path::PathBuf;
use std::sync::Arc;

use forge_scene::Scene;

use crate::render::backend::FrameInput;
use crate::render_core::camera::EditorCamera;
use crate::render_core::math::M4;
use crate::rpc::{HostState, PlayState};

/// 谁要这一帧;决定尺寸权威、是否回读、是否做 nonzero 统计。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FrameRequester {
    /// [wsr]:readback=true、stats=false(stream::render_loop)。
    Stream,
    /// viewport.frame:readback = (format != "none")、stats=true(Pipelined 三段式,§4.5)。
    #[cfg_attr(not(test), allow(dead_code))]
    ViewportFrame,
}

#[derive(Debug, Clone, Copy)]
pub struct SnapshotParams {
    pub width: u32,
    pub height: u32,
    pub selected: Option<u64>,
    pub want_readback: bool,
    pub want_stats: bool,
    pub requester: FrameRequester,
    /// D-045:编辑态也用场景 Camera 渲染(viewport.frame{camera:"scene"});play 态恒用场景相机,与它无关。
    pub scene_camera: bool,
}

/// 字段与 render_scene_frame 的显式输入一一对应,并把隐式输入(项目根、资产代次)显式化。
#[cfg_attr(not(test), allow(dead_code))] // seq / project_root / asset_generation 只给 Pipelined 后端读
#[derive(Clone)]
pub struct RenderSnapshot {
    /// FrameBus::next_seq();rurix 路径不读。
    pub seq: u64,
    pub scene_rev: u64,
    /// st.active().clone();Arc 只为跨线程投递。
    pub scene: Arc<Scene>,
    pub camera: EditorCamera,
    pub play: PlayState,
    /// 条件照搬各调用点:play != Edit 时取场景相机(推流腿 / RPC 腿各自的 aspect 算式)。
    pub vp_override: Option<M4>,
    pub params: SnapshotParams,
    pub project_root: Arc<PathBuf>,
    pub asset_generation: u64,
}

/// 调用方已持 HS 守卫;函数内不做 IO、不取任何其他锁。
pub fn snapshot(st: &HostState, p: SnapshotParams, seq: u64) -> RenderSnapshot {
    crate::shader::set_frame_time(if st.play==PlayState::Edit {0.0} else {st.steps as f32*crate::rpc::DT_FIXED});
    let vp_override = if st.play != PlayState::Edit || p.scene_camera {
        let aspect = match p.requester {
            // stream::render_loop 原式。
            FrameRequester::Stream => p.width as f32 / p.height.max(1) as f32,
            // rpc::viewport_frame 原式(h 已钳到 >= 16,两式逐位相同)。
            FrameRequester::ViewportFrame => p.width as f32 / p.height as f32,
        };
        crate::viewport::scene_camera_view_proj(st.active(), aspect)
    } else {
        None
    };
    RenderSnapshot {
        seq,
        scene_rev: st.scene_rev,
        scene: Arc::new(st.active().clone()),
        camera: st.camera,
        play: st.play,
        vp_override,
        params: p,
        project_root: Arc::new(crate::rpc::project_root()),
        asset_generation: crate::render_core::assets::ASSET_GENERATION.load(std::sync::atomic::Ordering::Relaxed),
    }
}

impl RenderSnapshot {
    /// 与 render_scene_frame 的八个实参一一对应(推流腿:selected = cfg.selected、readback = true、stats = false)。
    pub fn input(&self) -> FrameInput<'_> {
        FrameInput {
            scene: &self.scene,
            cam: &self.camera,
            selected: self.params.selected,
            width: self.params.width,
            height: self.params.height,
            want_readback: self.params.want_readback,
            want_stats: self.params.want_stats,
            vp_override: self.vp_override,
        }
    }
}
