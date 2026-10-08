//! 02 §5.3 start_core(lib 入口):进程内以 AcceptMode::Thread、端口 0 启动核心 → 连 RPC 端口 →
//! host.ping / render.backendInfo;再次 start_core 必须以"重复安装"失败,且不建状态、不绑端口。
//! 另含编译期检查:外部 crate 能用 lib 导出的类型实现 RenderBackend / ImmediateRender / PipelinedRender。

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::Arc;
use std::time::Duration;

use engine_host::{
    start_core, AcceptMode, BackendInfo, BackendKind, Capabilities, Channel, ConfigSource, ControlMsg, CoreConfig,
    CoreErrorKind, FrameBus, FrameInput, FrameOrigin, FrameOut, FramePath, FramePixels, FrameRequest, ImmediateRender,
    Leg, LegSet, MaxDraws, PipelinedRender, RenderBackend, RenderList, RurixBackend, StatsCaps,
};
use serde_json::{json, Value};

/// 模拟 Stage 3 的 GodotBackend:只用 lib 导出的类型。
struct ProbeBackend {
    info: BackendInfo,
    caps: Capabilities,
    bus: FrameBus,
}

impl RenderBackend for ProbeBackend {
    fn info(&self) -> &BackendInfo {
        &self.info
    }
    fn capabilities(&self) -> &Capabilities {
        &self.caps
    }
    fn path(&self) -> FramePath<'_> {
        FramePath::Pipelined(self)
    }
    fn invalidate_assets(&self) {}
    fn device_name(&self) -> Option<String> {
        None
    }
}

impl ImmediateRender for ProbeBackend {
    fn render(&self, input: FrameInput<'_>) -> Result<FramePixels, String> {
        Err(format!("RENDER_UNSUPPORTED: probe {}x{} eye={:?}", input.width, input.height, input.cam.eye()))
    }
    fn preview(&self, input: FrameInput<'_>) -> Result<FramePixels, String> {
        self.render(input)
    }
}

impl PipelinedRender for ProbeBackend {
    fn next_seq(&self) -> u64 {
        1
    }
    fn submit(&self, _ch: Channel, _list: Arc<RenderList>, req: Option<FrameRequest>) -> Result<(), String> {
        if let Some(r) = req {
            let (width, height) = r.size;
            let pixels = FramePixels {
                width, height, rgba8: vec![255; (width * height * 4) as usize], device_name: "probe".into(), draws: 0,
                truncated: false, nonzero: 0, triangles: 0, mesh_fallbacks: 0, mesh_classes: 0, imported: false,
            };
            let out = FrameOut { seq: r.min_seq, scene_rev: 0, pixels, origin: FrameOrigin::CpuReadback };
            r.reply.try_send(Ok(Arc::new(out))).map_err(|e| e.to_string())?;
        }
        Ok(())
    }
    fn bus(&self, _ch: Channel) -> &FrameBus {
        &self.bus
    }
    fn control(&self, msg: ControlMsg) -> Result<(), String> {
        match msg {
            ControlMsg::ShareAttach(t) => Err(format!("L1 {}x{} pitch {}", t.width, t.height, t.row_pitch)),
            ControlMsg::ShareDetach | ControlMsg::InvalidateAssets { .. } | ControlMsg::Shutdown => Ok(()),
        }
    }
    fn ready(&self) -> bool {
        false
    }
}

fn probe_backend() -> ProbeBackend {
    ProbeBackend {
        info: BackendInfo {
            kind: BackendKind::Godot, method: None, driver: None, source: ConfigSource::Default, godot_version: None,
        },
        caps: Capabilities {
            legs: LegSet { sprite_mesh: true, model: true, sentinels_v6: false },
            pipelined: true, preview: true, particles: false, cpu_rgba8: true, shared_d3d12: true, zero_copy: false,
            stats: StatsCaps { nonzero: true, triangles: true, truncated: false, mesh_fallbacks: false, mesh_classes: false },
            max_draws: MaxDraws { sprite_mesh: None, model: None, sentinels_v6: None },
        },
        bus: FrameBus::default(),
    }
}

fn rpc(s: &mut TcpStream, method: &str, params: Value) -> Value {
    let body = serde_json::to_vec(&json!({ "jsonrpc": "2.0", "id": 1, "method": method, "params": params })).unwrap();
    s.write_all(&(body.len() as u32).to_le_bytes()).unwrap();
    s.write_all(&body).unwrap();
    let mut len = [0u8; 4];
    s.read_exact(&mut len).unwrap();
    let mut buf = vec![0u8; u32::from_le_bytes(len) as usize];
    s.read_exact(&mut buf).unwrap();
    let v: Value = serde_json::from_slice(&buf).unwrap();
    assert!(v.get("error").is_none(), "{method} 不应报错:{v}");
    v["result"].clone()
}


#[test]
fn start_core_thread_mode_serves_rpc_and_refuses_a_second_start() {
    let root = std::env::temp_dir().join(format!("f3_start_core_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let handle = start_core(CoreConfig {
        port: 0,
        game_scene: None,
        project_root: Some(root.clone()),
        backend: Box::new(RurixBackend::new()),
        accept: AcceptMode::Thread,
    })
    .unwrap_or_else(|e| panic!("start_core: {e}"));
    assert_ne!(handle.port, 0, "port 0 由系统分配,句柄里是实际端口");
    assert!(handle.stream_port.is_some_and(|p| p != 0 && p != handle.port), "{:?}", handle.stream_port);

    let mut s = TcpStream::connect(("127.0.0.1", handle.port)).expect("连 CoreHandle.port");
    s.set_read_timeout(Some(Duration::from_secs(30))).unwrap();
    let ping = rpc(&mut s, "host.ping", json!({}));
    assert_eq!(ping["pong"], true);
    assert_eq!(ping["pid"], std::process::id(), "应答来自本进程内的核心");
    assert_eq!(ping["version"], env!("CARGO_PKG_VERSION"));
    let info = rpc(&mut s, "render.backendInfo", json!({}));
    assert_eq!((info["renderBackend"].as_str(), info["ready"].as_bool()), (Some("rurix"), Some(true)), "{info}");
    assert_eq!(info["versions"]["engineHost"], env!("CARGO_PKG_VERSION"));

    // CoreHandle.state 就是在服的状态:非只读 RPC 成功后 scene_rev + 1。
    let rev = handle.state.lock().unwrap().scene_rev;
    rpc(&mut s, "scene.new", json!({ "name": "f3", "mode": "3d" }));
    assert_eq!(handle.state.lock().unwrap().scene_rev, rev + 1);
    // CoreConfig.project_root 覆盖生效:scene.save 的缺省路径落在覆盖根下。
    let saved = rpc(&mut s, "scene.save", json!({}));
    let expected = root.join("data").join("scene.rxscene");
    assert_eq!(saved["path"].as_str().map(std::path::PathBuf::from), Some(expected.clone()));
    assert!(expected.is_file());

    // 第二次 start_core:install 是第一步 → 重复安装;不建状态、不绑端口。
    let probe_port = TcpListener::bind(("127.0.0.1", 0)).unwrap().local_addr().unwrap().port();
    let second = start_core(CoreConfig {
        port: probe_port,
        game_scene: None,
        project_root: None,
        backend: Box::new(RurixBackend::new()),
        accept: AcceptMode::Thread,
    });
    let err = second.err().expect("第二次 start_core 必须失败");
    assert_eq!((err.kind, err.exit_code()), (CoreErrorKind::BackendInstalled, 1), "{err}");
    TcpListener::bind(("127.0.0.1", probe_port)).expect("失败的 start_core 不得绑端口");
    assert_eq!(rpc(&mut s, "host.ping", json!({}))["pid"], std::process::id(), "原核心不受影响");
    let _ = std::fs::remove_dir_all(&root);
}

/// 外部 crate 实现的后端能被当作 RenderBackend 使用(不安装:进程内的后端单例归上面的测试)。
#[test]
fn an_external_crate_can_implement_the_backend_traits() {
    let b: Box<dyn RenderBackend> = Box::new(probe_backend());
    assert_eq!(b.info().kind, BackendKind::Godot);
    assert!(b.capabilities().legs.contains(Leg::Model) && !b.capabilities().legs.contains(Leg::SentinelsV6));
    let FramePath::Pipelined(p) = b.path() else { panic!("probe 走 Pipelined") };
    assert_eq!((p.next_seq(), p.ready()), (1, false));
    let _bus: &FrameBus = p.bus(Channel::Preview);
    assert!(p.control(ControlMsg::InvalidateAssets { generation: 1 }).is_ok());
    let cam = engine_host::EditorCamera::default();
    let scene = forge_scene::Scene::new("probe");
    let input = FrameInput {
        scene: &scene, cam: &cam, selected: None, width: 4, height: 2, want_readback: true, want_stats: false,
        vp_override: Some(cam.view_proj(2.0)),
    };
    let err = probe_backend().render(input).err().unwrap();
    assert!(err.starts_with("RENDER_UNSUPPORTED: probe 4x2"), "{err}");
    let _m4: engine_host::M4 = cam.view_proj(1.0);
}
