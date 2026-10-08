//! Stage 3:Pipelined 后端在 engine-host 核心里的全链路(不依赖 Godot)。
//! 用只依赖 lib 导出的假后端 + 假 [gmain] 线程(取 SubmitBox → 造帧 → FrameSink 交付),验证:
//! 首帧前 RENDER_NOT_READY、viewport.frame 三段式(rgba8 / none / h264 与 rurix 同键)、template.preview 错误同文、
//! render.backendInfo / render.capabilities 的 Godot 形态、WS 推流出帧、CoreHandle::shutdown 关停 accept。

use std::io::{Read, Write};
use std::net::TcpStream;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use engine_host::{
    clear_rgb8, start_core, AcceptMode, BackendInfo, BackendKind, Capabilities, Channel, ConfigSource, ControlMsg,
    CoreConfig, Coverage, FrameBus, FrameChannels, FrameOrigin, FrameOut, FramePath, FramePixels, FrameRequest,
    FrameSink, HostFrameSink, LegSet, MaxDraws, PipelinedRender, RenderBackend, RenderList, StatsCaps,
};
use serde_json::{json, Value};

struct Fake {
    info: BackendInfo,
    caps: Capabilities,
    sink: Arc<HostFrameSink>,
    ready: Arc<AtomicBool>,
}

impl RenderBackend for Fake {
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
        Some("fake-gpu".into())
    }
    fn frame_channels(&self) -> Option<FrameChannels> {
        Some(FrameChannels { l2: "fake", l1_reason: Some("no d3d12".into()), ..FrameChannels::default() })
    }
    fn coverage(&self) -> Option<Coverage> {
        Some(Coverage { skipped: vec!["Sprite"] })
    }
}

impl PipelinedRender for Fake {
    fn next_seq(&self) -> u64 {
        self.sink.main.next_seq()
    }
    fn submit(&self, ch: Channel, list: Arc<RenderList>, req: Option<FrameRequest>) -> Result<(), String> {
        self.sink.submit_box(ch).put(list, req);
        Ok(())
    }
    fn bus(&self, ch: Channel) -> &FrameBus {
        self.sink.bus(ch)
    }
    fn control(&self, msg: ControlMsg) -> Result<(), String> {
        self.sink.main_box.control(msg);
        Ok(())
    }
    fn ready(&self) -> bool {
        self.ready.load(Ordering::SeqCst)
    }
}

/// 假 [gmain]:paused 期间不出帧(模拟冷启动);之后每份清单出一帧:清屏色底 + 左上 2x2 白块。
fn fake_gmain(sink: Arc<HostFrameSink>, ready: Arc<AtomicBool>, paused: Arc<AtomicBool>) {
    std::thread::spawn(move || loop {
        std::thread::sleep(Duration::from_millis(5));
        sink.main_box.expire(Instant::now());
        sink.preview_box.expire(Instant::now());
        if paused.load(Ordering::SeqCst) {
            continue;
        }
        for ch in [Channel::Main, Channel::Preview] {
            let (_ctrl, list) = sink.submit_box(ch).take();
            let Some(l) = list else { continue };
            let bg = clear_rgb8(l.clear_rgba);
            let mut rgba8 = Vec::with_capacity((l.width * l.height * 4) as usize);
            for y in 0..l.height {
                for x in 0..l.width {
                    let white = x < 2 && y < 2;
                    rgba8.extend_from_slice(&if white { [255, 255, 255, 255] } else { [bg[0], bg[1], bg[2], 255] });
                }
            }
            let pixels = FramePixels {
                width: l.width, height: l.height, rgba8: if l.want_pixels { rgba8 } else { Vec::new() },
                device_name: "fake-gpu".into(), draws: l.items.len(), truncated: false, nonzero: 0,
                triangles: l.stats.triangles, mesh_fallbacks: l.stats.mesh_fallbacks, mesh_classes: 0, imported: false,
            };
            let origin = if l.want_pixels { FrameOrigin::CpuReadback } else { FrameOrigin::NoPixels };
            sink.deliver(ch, FrameOut { seq: l.seq, scene_rev: l.scene_rev, pixels, origin });
            ready.store(true, Ordering::SeqCst);
        }
    });
}

fn rpc_raw(s: &mut TcpStream, method: &str, params: Value) -> Value {
    let body = serde_json::to_vec(&json!({ "jsonrpc": "2.0", "id": 1, "method": method, "params": params })).unwrap();
    s.write_all(&(body.len() as u32).to_le_bytes()).unwrap();
    s.write_all(&body).unwrap();
    let mut len = [0u8; 4];
    s.read_exact(&mut len).unwrap();
    let mut buf = vec![0u8; u32::from_le_bytes(len) as usize];
    s.read_exact(&mut buf).unwrap();
    serde_json::from_slice(&buf).unwrap()
}

fn rpc(s: &mut TcpStream, method: &str, params: Value) -> Value {
    let v = rpc_raw(s, method, params);
    assert!(v.get("error").is_none(), "{method} 不应报错:{v}");
    v["result"].clone()
}


fn fake_backend(sink: &Arc<HostFrameSink>, ready: &Arc<AtomicBool>) -> Fake {
    Fake {
        info: BackendInfo {
            kind: BackendKind::Godot, method: Some(engine_host::RenderMethod::ForwardPlus),
            driver: Some(engine_host::RenderDriver::D3d12), source: ConfigSource::ForgeToml, godot_version: Some("4.7.2-fake".into()),
        },
        caps: Capabilities {
            legs: LegSet { sprite_mesh: true, model: false, sentinels_v6: false },
            pipelined: true, preview: true, particles: false, cpu_rgba8: true, shared_d3d12: true, zero_copy: false,
            stats: StatsCaps { nonzero: true, triangles: true, truncated: false, mesh_fallbacks: true, mesh_classes: false },
            max_draws: MaxDraws { sprite_mesh: None, model: None, sentinels_v6: None },
        },
        sink: Arc::clone(sink),
        ready: Arc::clone(ready),
    }
}

fn keys(v: &Value) -> Vec<String> {
    let mut k: Vec<String> = v.as_object().unwrap().keys().cloned().collect();
    k.sort();
    k
}

#[test]
fn pipelined_backend_serves_rpc_and_ws_frames_through_the_core() {
    let root = std::env::temp_dir().join(format!("f3_pipelined_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    let sink = Arc::new(HostFrameSink::new());
    let (ready, paused) = (Arc::new(AtomicBool::new(false)), Arc::new(AtomicBool::new(true)));
    fake_gmain(Arc::clone(&sink), Arc::clone(&ready), Arc::clone(&paused));
    let h = start_core(CoreConfig {
        port: 0, game_scene: None, project_root: Some(root.clone()),
        backend: Box::new(fake_backend(&sink, &ready)), accept: AcceptMode::Thread,
    })
    .expect("Thread 模式启动");
    let mut s = TcpStream::connect(("127.0.0.1", h.port)).unwrap();
    s.set_read_timeout(Some(Duration::from_secs(15))).unwrap();

    let info = rpc(&mut s, "render.backendInfo", json!({}));
    assert_eq!((info["renderBackend"].clone(), info["ready"].clone(), info["source"].clone()), (json!("godot"), json!(false), json!("forge.toml")));
    assert_eq!((info["method"].clone(), info["driver"].clone(), info["versions"]["gdext"].clone()), (json!("forward_plus"), json!("d3d12"), json!("0.5.5")));
    assert_eq!(info["frameChannels"]["l2"], "fake");
    assert_eq!(info["frameChannels"]["l1Reason"], "no d3d12");
    let caps = rpc(&mut s, "render.capabilities", json!({}));
    assert_eq!((caps["legs"].clone(), caps["pipelined"].clone()), (json!(["sprite_mesh"]), json!(true)));
    assert_eq!(caps["coverage"], json!({ "skipped": ["Sprite"] }));
    assert_eq!(caps["maxDraws"], json!({ "spriteMesh": null, "model": null, "sentinelsV6": null }));

    // 冷启动:[gmain] 还没出过帧 → 3 s 后 RENDER_NOT_READY(-32000)。
    let t0 = Instant::now();
    let e = rpc_raw(&mut s, "viewport.frame", json!({ "width": 64, "height": 32 }));
    assert_eq!(e["error"]["code"], -32000, "{e}");
    assert!(e["error"]["message"].as_str().unwrap().starts_with("RENDER_NOT_READY:"), "{e}");
    assert!(t0.elapsed() >= Duration::from_millis(2900));
    paused.store(false, Ordering::SeqCst);

    rpc(&mut s, "entity.create", json!({ "name": "cube", "components": [{ "type": "MeshRenderer", "props": { "mesh": "cube", "material": "m" } }] }));
    let f = rpc(&mut s, "viewport.frame", json!({ "width": 64, "height": 32 }));
    let rurix_keys = ["cpuUploads", "deviceName", "draws", "format", "framePath", "frames", "height", "meshClasses", "meshFallbacks", "nonZeroPixels", "pixelsB64", "triangles", "truncated", "width"];
    assert_eq!(keys(&f), rurix_keys, "与 rurix 同键");
    let px = base64::Engine::decode(&base64::engine::general_purpose::STANDARD, f["pixelsB64"].as_str().unwrap()).unwrap();
    assert_eq!(px.len(), 64 * 32 * 4);
    assert_eq!((f["nonZeroPixels"].clone(), f["draws"].clone(), f["triangles"].clone()), (json!(4), json!(1), json!(12)));
    assert_eq!((f["framePath"].clone(), f["cpuUploads"].clone(), f["deviceName"].clone()), (json!("no_share"), json!(0), json!("fake-gpu")));
    let frames = f["frames"].as_u64().unwrap();
    let n = rpc(&mut s, "viewport.frame", json!({ "width": 64, "height": 32, "format": "none" }));
    assert!(n.get("pixelsB64").is_none() && n["format"] == "none" && n["nonZeroPixels"] == 0);
    assert_eq!(n["frames"].as_u64().unwrap(), frames + 1, "短锁记账");
    let v = rpc(&mut s, "viewport.frame", json!({ "width": 64, "height": 32, "format": "h264" }));
    assert!(v["format"] == "h264" && v["nalB64"].as_str().is_some_and(|b| !b.is_empty()) && v["keyframe"] == true, "{v}");

    // template.preview:预制体参数错误与 rurix 同码同文。
    let e = rpc_raw(&mut s, "template.preview", json!({}));
    assert_eq!((e["error"]["code"].clone(), e["error"]["message"].clone()), (json!(-32000), json!("prefabRef required")));

    // WS 推流:subscribe → hello → 二进制帧(20 B 头 + 紧凑 RGBA8)。
    let si = rpc(&mut s, "viewport.streamInfo", json!({}));
    let (mut ws, _) = tungstenite::connect(si["wsUrl"].as_str().unwrap()).expect("WS 直连");
    if let tungstenite::stream::MaybeTlsStream::Plain(t) = ws.get_ref() {
        t.set_read_timeout(Some(Duration::from_millis(200))).unwrap();
    }
    ws.send(tungstenite::Message::Text(json!({ "type": "subscribe", "width": 48, "height": 24, "maxFps": 30 }).to_string())).unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    let bin = loop {
        assert!(Instant::now() < deadline, "10 s 内没有二进制帧");
        match ws.read() {
            Ok(tungstenite::Message::Binary(b)) => break b,
            Ok(tungstenite::Message::Text(t)) => assert!(!t.contains("\"error\""), "推流报错:{t}"),
            Ok(_) => {}
            Err(tungstenite::Error::Io(e)) if matches!(e.kind(), std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut) => {}
            Err(e) => panic!("WS 读失败:{e}"),
        }
    };
    assert_eq!(&bin[..4], b"FGF1");
    assert_eq!((u16::from_le_bytes([bin[8], bin[9]]), u16::from_le_bytes([bin[10], bin[11]])), (48, 24));
    assert_eq!(bin.len(), 20 + 48 * 24 * 4);
    drop(ws);

    // CoreHandle::shutdown:accept 线程退出,之后连不上。
    h.shutdown();
    let deadline = Instant::now() + Duration::from_secs(3);
    while TcpStream::connect(("127.0.0.1", h.port)).is_ok() {
        assert!(Instant::now() < deadline, "shutdown 后 3 s 内 RPC 端口仍可连");
        std::thread::sleep(Duration::from_millis(50));
    }
    let _ = std::fs::remove_dir_all(&root);
}
