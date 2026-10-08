//! template.preview(02 §4.3):render 调用之前的部分自 rpc::template_preview 逐字搬出,步骤与顺序不变
//! (实例化 → 改写 Animator clip/time → bounds 自动取景 → yaw → viewport_size)。

use forge_scene::Scene;
use serde_json::{json, Value};

use crate::render::backend::FrameInput;
use crate::render_core::camera::EditorCamera;
use crate::rpc::{param_err, viewport_size};

pub struct PreviewJob {
    pub scene: Scene,
    pub cam: EditorCamera,
    pub width: u32,
    pub height: u32,
}

/// 错误与 rpc 的 HResult 同型:(code, message)。
pub fn preview_job(args: &Value) -> Result<PreviewJob, (i64, String)> {
    let(scene,_)=crate::prefab::instantiate(&Scene::new("Template preview"),args).map_err(|e|(-32000,e))?;let mut scene=scene;
    for e in &mut scene.entities{if let Some(a)=e.component_mut("Animator"){if let Some(clip)=args.get("clip").and_then(Value::as_str){a.props["clip"]=json!(clip);}a.props["time"]=args.get("time").cloned().unwrap_or(json!(0.));}}
    let(center,radius)=crate::modelrender::bounds(&scene).map_err(|e|(-32000,e))?;let mut cam=crate::viewport::EditorCamera::default();cam.target=center;cam.dist=(radius*2.8).max(0.1);if let Some(yaw)=args["yaw"].as_f64(){if !yaw.is_finite(){return param_err("yaw must be finite");}cam.yaw_deg=yaw.to_degrees()as f32;}
    let(w,h)=viewport_size(args)?;
    Ok(PreviewJob { scene, cam, width: w, height: h })
}

impl PreviewJob {
    /// Pipelined(§4.3):用预览场景 / 相机构造无 HostState 的快照(编辑态、无场景相机覆盖、readback + stats)。
    pub fn into_snapshot(self, seq: u64) -> crate::render::snapshot::RenderSnapshot {
        use crate::render::snapshot::{FrameRequester, RenderSnapshot, SnapshotParams};
        RenderSnapshot {
            seq,
            scene_rev: 0,
            scene: std::sync::Arc::new(self.scene),
            camera: self.cam,
            play: crate::rpc::PlayState::Edit,
            vp_override: None,
            params: SnapshotParams { scene_camera: false,
                width: self.width,
                height: self.height,
                selected: None,
                want_readback: true,
                want_stats: true,
                requester: FrameRequester::ViewportFrame,
            },
            project_root: std::sync::Arc::new(crate::rpc::project_root()),
            asset_generation: crate::render_core::assets::ASSET_GENERATION.load(std::sync::atomic::Ordering::Relaxed),
        }
    }
    /// 与原 modelrender::render 调用的八个实参一致:selected=None、readback=true、stats=true、vp=None。
    pub fn input(&self) -> FrameInput<'_> {
        FrameInput {
            scene: &self.scene,
            cam: &self.cam,
            selected: None,
            width: self.width,
            height: self.height,
            want_readback: true,
            want_stats: true,
            vp_override: None,
        }
    }
}
