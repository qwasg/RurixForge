//! RurixBackend(02 §4.4):1:1 包住现有实现。同样八个实参、同一线程、持同样的锁调用;
//! 包装层不碰像素,FramePixels 原样返回。唯一新增的运行时动作是首帧写一次 OnceLock。

use std::sync::OnceLock;

use crate::render::backend::{
    BackendInfo, BackendKind, Capabilities, ConfigSource, FrameInput, FramePath, ImmediateRender, LegSet, MaxDraws,
    RenderBackend, StatsCaps,
};
use crate::viewport::FramePixels;

pub struct RurixBackend {
    info: BackendInfo,
    caps: Capabilities,
    /// 首帧 device_name;热路径只多一次原子读,不加锁。
    first_device: OnceLock<String>,
}

impl RurixBackend {
    pub fn new() -> Self {
        RurixBackend {
            info: BackendInfo {
                kind: BackendKind::Rurix,
                method: None,
                driver: None,
                source: ConfigSource::Default,
                godot_version: None,
            },
            caps: Capabilities {
                legs: LegSet { sprite_mesh: true, model: true, sentinels_v6: true },
                pipelined: false,
                preview: true,
                // 在 new() 里读一次(FORGE_GPU_PARTICLES)。
                particles: crate::gpu_particles::enabled(),
                cpu_rgba8: true,
                // 共享 buffer(share.rs)与 import 档(viewport::current_import)都只在 Windows 编译进来。
                shared_d3d12: cfg!(windows),
                zero_copy: cfg!(windows),
                stats: StatsCaps { nonzero: true, triangles: true, truncated: true, mesh_fallbacks: true, mesh_classes: true },
                max_draws: MaxDraws {
                    sprite_mesh: Some(crate::viewport::MAX_DRAW_SLOTS as u32),
                    model: Some(crate::render_core::model::MAX_DRAWS as u32),
                    sentinels_v6: Some(crate::sentinels_v6_render::FRAME_SLOTS as u32),
                },
            },
            first_device: OnceLock::new(),
        }
    }
}

impl Default for RurixBackend {
    fn default() -> Self {
        Self::new()
    }
}

impl ImmediateRender for RurixBackend {
    fn render(&self, i: FrameInput<'_>) -> Result<FramePixels, String> {
        let r = crate::viewport::render_scene_frame(
            i.scene, i.cam, i.selected, i.width, i.height, i.want_readback, i.want_stats, i.vp_override,
        );
        if let Ok(f) = &r {
            if self.first_device.get().is_none() {
                let _ = self.first_device.set(f.device_name.clone());
            }
        }
        r
    }

    fn preview(&self, i: FrameInput<'_>) -> Result<FramePixels, String> {
        crate::modelrender::render(
            i.scene, i.cam, i.selected, i.width, i.height, i.want_readback, i.want_stats, i.vp_override,
        )
    }
}

impl RenderBackend for RurixBackend {
    fn info(&self) -> &BackendInfo {
        &self.info
    }

    fn capabilities(&self) -> &Capabilities {
        &self.caps
    }

    fn path(&self) -> FramePath<'_> {
        FramePath::Immediate(self)
    }

    /// asset.reload 已直接调三处 invalidate(rpc.rs asset_reload 首行),顺序不动。
    fn invalidate_assets(&self) {}

    fn device_name(&self) -> Option<String> {
        self.first_device.get().cloned()
    }
}
