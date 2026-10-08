//! 渲染后端抽象(02 §4)。调用点只经这里取帧:rurix 走 Immediate,1:1 包住 viewport::render_scene_frame
//! 与 modelrender::render(§4.4);Pipelined(Godot)只有 trait 与返回 `RENDER_UNSUPPORTED:` 的空壳。
//! 本模块不 `use rurix_rt`;rurix 专用部分只在 rurix.rs(step 6 的 backend-rurix feature 边界)。

pub(crate) mod backend;
pub(crate) mod bus;
pub(crate) mod preview;
#[cfg(feature = "backend-rurix")]
pub(crate) mod rurix;
pub(crate) mod sink;
pub(crate) mod snapshot;

use serde_json::{json, Value};

pub(crate) use backend::{backend, FrameInput, FramePath};
use backend::{BackendKind, ConfigSource, RenderBackend, RenderDriver, RenderMethod};
pub(crate) use snapshot::{snapshot, FrameRequester, SnapshotParams};

use crate::render_core::list::Leg;
use crate::viewport::FramePixels;

/// rpc::viewport_size 的钳制上限(width 16..=1920、height 16..=1080),render.capabilities 的 maxSize.rpc。
const RPC_MAX_SIZE: [u32; 2] = [1920, 1080];
/// Godot 后端的 gdext 版本(已定决策:godot-rust gdext =0.5.5)。
const GDEXT_VERSION: &str = "0.5.5";

/// 当前后端是否走 Pipelined(rurix 下恒 false)。
pub(crate) fn is_pipelined() -> bool {
    matches!(backend().path(), FramePath::Pipelined(_))
}

/// Pipelined 后端不支持的取帧腿统一报这个前缀(§2.5 错误约定;只由非 rurix 后端产生,I11)。
pub(crate) fn unsupported(what: &str) -> String {
    format!("RENDER_UNSUPPORTED: {what}: Pipelined 后端不走同步取帧腿")
}

/// Pipelined 取帧的锁外前半段(§2.4 第 1 步):先按 Capabilities.legs 拦住本后端没接入的腿,再 extract。
pub(crate) fn pipelined_list(
    b: &dyn RenderBackend,
    snap: &snapshot::RenderSnapshot,
) -> Result<std::sync::Arc<crate::render_core::list::RenderList>, String> {
    let leg = crate::render_core::list::classify(&snap.scene);
    if !b.capabilities().legs.contains(leg) {
        return Err(format!("RENDER_UNSUPPORTED: {}: 当前渲染后端未接入该腿(render.capabilities.legs)", leg.as_str()));
    }
    crate::render_core::list::extract_arc(snap)
}

/// viewport.frame(§4.5 第 9 项):Immediate 原样转发;Pipelined 由 dispatch 分流接管(第 8 项),
/// 走不到这里,防御性返回 RENDER_UNSUPPORTED:。
pub(crate) fn render_frame(input: FrameInput<'_>) -> Result<FramePixels, String> {
    match backend().path() {
        FramePath::Immediate(r) => r.render(input),
        FramePath::Pipelined(_) => Err(unsupported("viewport.frame")),
    }
}

/// template.preview(§4.5 第 10 项):Immediate 原样转发到 preview();Pipelined 同上由 dispatch 接管。
pub(crate) fn render_preview(input: FrameInput<'_>) -> Result<FramePixels, String> {
    match backend().path() {
        FramePath::Immediate(r) => r.preview(input),
        FramePath::Pipelined(_) => Err(unsupported("template.preview")),
    }
}

fn kind_str(k: BackendKind) -> &'static str {
    match k {
        BackendKind::Rurix => "rurix",
        BackendKind::Godot => "godot",
    }
}

/// render.backendInfo(§4.6)。键名 camelCase,枚举值沿用 forge.toml 的 snake_case。
/// frameChannels 只在后端给出时出现(Godot);rurix 的返回与 Stage 2 逐字相同。
pub(crate) fn backend_info_json(b: &dyn RenderBackend) -> Value {
    let info = b.info();
    let mut v = json!({
        "renderBackend": kind_str(info.kind),
        "method": info.method.map(|m| match m {
            RenderMethod::ForwardPlus => "forward_plus",
            RenderMethod::Mobile => "mobile",
            RenderMethod::GlCompatibility => "gl_compatibility",
        }),
        "driver": info.driver.map(|d| match d {
            RenderDriver::D3d12 => "d3d12",
            RenderDriver::Vulkan => "vulkan",
            RenderDriver::Opengl3 => "opengl3",
        }),
        "source": match info.source {
            ConfigSource::Default => "default",
            ConfigSource::ForgeToml => "forge.toml",
            ConfigSource::Env => "env",
            ConfigSource::Cli => "cli",
        },
        "ready": match b.path() {
            FramePath::Immediate(_) => true,
            FramePath::Pipelined(p) => p.ready(),
        },
        "deviceName": b.device_name(),
        "versions": {
            "engineHost": env!("CARGO_PKG_VERSION"),
            "godot": info.godot_version.clone(),
            "gdext": (info.kind == BackendKind::Godot).then_some(GDEXT_VERSION),
        },
    });
    if let Some(fc) = b.frame_channels() {
        v["frameChannels"] = json!({
            "l2": fc.l2,
            "l1Available": fc.l1_available,
            "l1Active": fc.l1_active,
            "l1Reason": fc.l1_reason,
            "adapter": fc.adapter,
            "l1Frames": fc.l1_frames,
            "l2Frames": fc.l2_frames,
            "l1Lag": fc.l1_lag,
            "l2Lag": fc.l2_lag,
            "debugLayer": fc.debug_layer.map(|d| json!({ "errors": d.errors, "warnings": d.warnings, "corruption": d.corruption })),
        });
    }
    v
}

/// render.capabilities(§4.6)。coverage 只在后端声明时出现(Godot 逐阶段接入);rurix 的返回与 Stage 2 逐字相同。
pub(crate) fn capabilities_json(b: &dyn RenderBackend) -> Value {
    let c = b.capabilities();
    let legs: Vec<&str> = [(Leg::SpriteMesh, "sprite_mesh"), (Leg::Model, "model"), (Leg::SentinelsV6, "sentinels_v6")]
        .into_iter()
        .filter(|(leg, _)| c.legs.contains(*leg))
        .map(|(_, name)| name)
        .collect();
    let mut v = json!({
        "renderBackend": kind_str(b.info().kind),
        "pipelined": c.pipelined,
        "legs": legs,
        "preview": c.preview,
        "particles": c.particles,
        "frameExits": { "cpuRgba8": c.cpu_rgba8, "sharedD3d12": c.shared_d3d12, "zeroCopy": c.zero_copy },
        "stats": {
            "nonzero": c.stats.nonzero,
            "triangles": c.stats.triangles,
            "truncated": c.stats.truncated,
            "meshFallbacks": c.stats.mesh_fallbacks,
            "meshClasses": c.stats.mesh_classes,
        },
        "maxDraws": {
            "spriteMesh": c.max_draws.sprite_mesh,
            "model": c.max_draws.model,
            "sentinelsV6": c.max_draws.sentinels_v6,
        },
        "maxSize": { "rpc": RPC_MAX_SIZE, "stream": [crate::stream::MAX_W, crate::stream::MAX_H] },
    });
    if let Some(cov) = b.coverage() {
        let mut c = json!({ "skipped": cov.skipped });
        let list = |v: Vec<(&str, &str)>| Value::Array(v.into_iter().map(|(f, r)| json!({ "feature": f, "reason": r })).collect());
        let unsupported = b.unsupported_features();
        if !unsupported.is_empty() {
            c["unsupported"] = list(unsupported);
        }
        let limited = b.limited_features();
        if !limited.is_empty() {
            c["limited"] = list(limited);
        }
        v["coverage"] = c;
    }
    v
}


#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    use crate::rpc::{dispatch, lock, HostState};

    fn call(st: &Mutex<HostState>, method: &str, params: Value) -> Value {
        dispatch(st, &json!({ "jsonrpc": "2.0", "id": 1, "method": method, "params": params }))
    }

    /// 02 §4.1 表左列 + §4.6 的 maxDraws:取值直接来自各腿的常量。
    #[cfg(feature = "backend-rurix")] // 用到 RurixBackend / gpu_particles(no-default --tests 只要求能编译)
    #[test]
    fn rurix_capabilities_follow_02_table() {
        let b = rurix::RurixBackend::new();
        let c = b.capabilities();
        assert_eq!(b.info().kind, BackendKind::Rurix);
        assert_eq!((b.info().method, b.info().driver, b.info().source), (None, None, ConfigSource::Default));
        assert!(c.legs.contains(Leg::SpriteMesh) && c.legs.contains(Leg::Model) && c.legs.contains(Leg::SentinelsV6));
        assert!(!backend::LegSet::default().contains(Leg::Model));
        assert_eq!((c.pipelined, c.preview, c.cpu_rgba8), (false, true, true));
        assert_eq!((c.shared_d3d12, c.zero_copy), (cfg!(windows), cfg!(windows)));
        assert_eq!(c.particles, crate::gpu_particles::enabled());
        assert_eq!(
            (c.max_draws.sprite_mesh, c.max_draws.model, c.max_draws.sentinels_v6),
            (Some(256), Some(2048), Some(1024 + 512))
        );
        assert!(matches!(b.path(), FramePath::Immediate(_)));
        assert_eq!(b.device_name(), None, "首帧前 deviceName 为 null");
        b.invalidate_assets(); // rurix 空操作
    }

    /// maxSize 与真实钳制同源:rpc::viewport_size 与 stream 的 MAX_W/MAX_H。
    #[test]
    fn max_size_matches_the_real_clamps() {
        let huge = json!({ "width": 100_000, "height": 100_000 });
        assert_eq!(crate::rpc::viewport_size(&huge).unwrap(), (RPC_MAX_SIZE[0], RPC_MAX_SIZE[1]));
        assert_eq!(capabilities_json(backend())["maxSize"], json!({ "rpc": [1920, 1080], "stream": [1280, 720] }));
    }

    /// 02 §4.6 的返回格式;两个方法都不接受参数,其余字段忽略。
    #[cfg(feature = "backend-rurix")] // 用到 RurixBackend / gpu_particles(no-default --tests 只要求能编译)
    #[test]
    fn render_rpcs_follow_02_4_6_and_ignore_params() {
        let st = Mutex::new(HostState::new());
        let particles = backend().capabilities().particles;
        assert_eq!(particles, crate::gpu_particles::enabled());
        for params in [json!({}), Value::Null, json!({ "x": 1 }), json!([1])] {
            let mut info = call(&st, "render.backendInfo", params.clone())["result"].clone();
            // deviceName 是进程级首帧设备名:同进程里别的测试可能已出过帧。
            let device = info.as_object_mut().unwrap().remove("deviceName").unwrap();
            assert!(device.is_null() || device.is_string(), "{device}");
            assert_eq!(
                info,
                json!({
                    "renderBackend": "rurix", "method": null, "driver": null, "source": "default", "ready": true,
                    "versions": { "engineHost": env!("CARGO_PKG_VERSION"), "godot": null, "gdext": null },
                })
            );
            assert_eq!(
                call(&st, "render.capabilities", params)["result"],
                json!({
                    "renderBackend": "rurix",
                    "pipelined": false,
                    "legs": ["sprite_mesh", "model", "sentinels_v6"],
                    "preview": true,
                    "particles": particles,
                    "frameExits": { "cpuRgba8": true, "sharedD3d12": cfg!(windows), "zeroCopy": cfg!(windows) },
                    "stats": { "nonzero": true, "triangles": true, "truncated": true, "meshFallbacks": true, "meshClasses": true },
                    "maxDraws": { "spriteMesh": 256, "model": 2048, "sentinelsV6": 1536 },
                    "maxSize": { "rpc": [1920, 1080], "stream": [1280, 720] },
                })
            );
        }
    }

    /// READONLY_METHODS(不 bump scene_rev)与 GAME_ALLOWED(game 模式可调)。
    #[test]
    fn render_rpcs_are_readonly_and_game_allowed() {
        let st = Mutex::new(HostState::new());
        let rev = lock(&st).scene_rev;
        for game in [false, true] {
            lock(&st).game_mode = game;
            for m in ["render.backendInfo", "render.capabilities"] {
                let r = call(&st, m, json!({}));
                assert!(r.get("error").is_none() && r["result"].is_object(), "{m} game={game}: {r}");
            }
            assert_eq!(lock(&st).scene_rev, rev, "只读方法不得 bump scene_rev");
        }
        let denied = call(&st, "entity.create", json!({ "name": "x" }));
        assert_eq!(denied["error"]["code"], -32601, "game 模式确实生效:{denied}");
    }


    fn bits(m: Option<crate::render_core::math::M4>) -> Option<Vec<u32>> {
        m.map(|m| m.iter().flatten().map(|f| f.to_bits()).collect())
    }

    /// 第 2 项:snapshot() 与推流腿原元组同字段;vp_override 与两个调用点的原式逐位相同;input() 八个实参一一对应。
    #[test]
    fn snapshot_matches_the_original_stream_and_rpc_expressions() {
        use forge_scene::{Component, Entity, Transform};
        let mut st = HostState::new();
        st.scene.entities.push(Entity { entity_guid: None,
            id: 7,
            name: "cam".into(),
            transform: Transform { translation: [1.0, 2.0, 10.0], rotation: [0.0, 0.0, 0.1, 0.995], scale: [1.0; 3] },
            components: vec![Component::new("Camera", json!({ "projection": "orthographic", "orthoSize": 4.5 }))],
        });
        st.scene_rev = 41;
        st.camera.yaw_deg = 12.5;
        let (w, h) = (1280u32, 720u32);
        let p = |requester, selected| SnapshotParams { scene_camera: false,
            width: w, height: h, selected, want_readback: true, want_stats: false, requester,
        };
        let edit = snapshot(&st, p(FrameRequester::Stream, Some(3)), 0);
        assert_eq!(edit.vp_override, None, "编辑态不取场景相机");
        for play in [crate::rpc::PlayState::Running, crate::rpc::PlayState::Paused] {
            st.play = play;
            let s = snapshot(&st, p(FrameRequester::Stream, Some(3)), 5);
            let stream_vp = crate::viewport::scene_camera_view_proj(st.active(), w as f32 / h.max(1) as f32);
            assert!(stream_vp.is_some());
            assert_eq!(bits(s.vp_override), bits(stream_vp), "推流腿原式");
            let r = snapshot(&st, p(FrameRequester::ViewportFrame, None), 6);
            assert_eq!(bits(r.vp_override), bits(crate::viewport::scene_camera_view_proj(st.active(), w as f32 / h as f32)));
            assert_eq!((s.scene_rev, s.play, s.seq, r.seq), (41, play, 5, 6));
            assert_eq!(*s.scene, *st.active());
            assert_eq!(s.camera, st.camera);
            assert_eq!(*s.project_root, crate::rpc::project_root());
            assert_eq!(s.asset_generation, crate::render_core::assets::ASSET_GENERATION.load(std::sync::atomic::Ordering::Relaxed));
            let i = s.input();
            assert!(std::ptr::eq(i.scene, &*s.scene) && std::ptr::eq(i.cam, &s.camera));
            assert_eq!((i.selected, i.width, i.height, i.want_readback, i.want_stats), (Some(3), 1280, 720, true, false));
            assert_eq!(bits(i.vp_override), bits(s.vp_override));
        }
    }

    #[test]
    fn frame_origin_maps_rurix_imported_flag() {
        let mut f = crate::viewport::FramePixels {
            width: 1, height: 1, rgba8: vec![0; 4], device_name: String::new(), draws: 0, truncated: false,
            nonzero: 0, triangles: 0, mesh_fallbacks: 0, mesh_classes: 0, imported: false,
        };
        assert_eq!(sink::FrameOrigin::from_rurix(&f), sink::FrameOrigin::CpuReadback);
        f.imported = true;
        assert_eq!(sink::FrameOrigin::from_rurix(&f), sink::FrameOrigin::SharedZeroCopy);
    }

    /// I10:后端是进程级单例;装好之后再 install 一律拒绝。
    #[cfg(feature = "backend-rurix")] // 用到 RurixBackend / gpu_particles(no-default --tests 只要求能编译)
    #[test]
    fn install_after_backend_is_refused() {
        let first = backend() as *const dyn RenderBackend as *const ();
        assert!(backend::install(Box::new(rurix::RurixBackend::new())).is_err());
        assert_eq!(backend() as *const dyn RenderBackend as *const (), first);
    }

    /// preview_job 是 template_preview 渲染前半段的逐字搬迁:错误与原来同码同文
    /// (找不到模板等依赖项目根的错误由 scratch 契约探针端到端对照)。
    #[test]
    fn preview_job_keeps_template_preview_errors() {
        let e = |args: Value| preview_job_err(&args);
        assert_eq!(e(json!({})), (-32000, "prefabRef required".to_string()));
        assert_eq!(e(json!([1, 2])), (-32000, "prefabRef required".to_string()));
    }

    fn preview_job_err(args: &Value) -> (i64, String) {
        match preview::preview_job(args) {
            Ok(_) => panic!("expected an error"),
            Err(e) => e,
        }
    }

    #[test]
    fn snapshot_scene_camera_flag_forces_scene_view_in_edit_mode() {
        use forge_scene::{Component, Entity, Transform};
        let mut st = HostState::new();
        st.scene.entities.push(Entity { entity_guid: None,
            id: 3,
            name: "cam".into(),
            transform: Transform { translation: [0.0, 0.0, 10.0], rotation: [0.0, 0.0, 0.0, 1.0], scale: [1.0; 3] },
            components: vec![Component::new("Camera", json!({ "projection": "orthographic", "orthoSize": 5.0 }))],
        });
        let p = |scene_camera| SnapshotParams {
            width: 320, height: 200, selected: None, want_readback: true, want_stats: false,
            requester: FrameRequester::ViewportFrame, scene_camera,
        };
        assert_eq!(st.play, crate::rpc::PlayState::Edit);
        assert_eq!(snapshot(&st, p(false), 0).vp_override, None, "编辑态缺省走编辑器相机");
        let forced = snapshot(&st, p(true), 1);
        assert_eq!(
            bits(forced.vp_override),
            bits(crate::viewport::scene_camera_view_proj(st.active(), 320.0 / 200.0)),
            "scene_camera=true 编辑态也取场景相机,宽高比按请求尺寸"
        );
        assert!(forced.vp_override.is_some());
    }

    #[test]
    fn pipelined_shells_report_render_unsupported() {
        assert!(!is_pipelined(), "rurix 走 Immediate");
        assert!(unsupported("stream").starts_with("RENDER_UNSUPPORTED: stream"));
        let st = HostState::new();
        let snap = snapshot(&st, SnapshotParams { scene_camera: false,
            width: 64, height: 64, selected: None, want_readback: true, want_stats: false, requester: FrameRequester::Stream,
        }, 0);
        // Stage 3:extract 已实现;空场景走 SpriteMesh 腿、没有条目。rurix 三腿全开,pipelined_list 不拦。
        let l = pipelined_list(backend(), &snap).expect("空场景可抽");
        assert!(l.items.is_empty() && l.want_pixels && !l.want_stats);
    }
}
