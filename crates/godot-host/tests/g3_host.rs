//! Stage 3 验收:Godot 宿主经与 rurix 同一套 JSON-RPC / WS 出帧;四种配置都出帧;L1 零拷贝 + debug layer 关卡;
//! viewport.pick 与 rurix 相同。前提:vendor/godot 的官方模板、target/debug/engine-host.exe(rurix 参照)、GPU。
//! 用例之间串行(common::serial),每个用例一个 Godot 进程。

mod common;

use common::{diff, serial, sha256, Godot, Rurix};
use serde_json::{json, Value};

const MAZE: &str = "Content/Scenes/maze.rxscene";

/// 就绪行:gdext / Godot 横幅在前,监督器必须逐行扫前缀(01 R1);render.backendInfo / render.capabilities 的 Godot 形态。
#[test]
fn ready_line_backend_info_and_capabilities() {
    let _g = serial();
    let g = Godot::start("forward_plus", "d3d12", &[], &[]);
    let log = g.log();
    let ready = log.iter().position(|l| l.starts_with("FORGE_HOST_LISTENING")).unwrap();
    assert!(log[..ready].iter().any(|l| l.contains("Initialize godot-rust")), "横幅应在就绪行之前:{log:?}");
    let mut r = g.rpc();
    let info = r.call("render.backendInfo", json!({}));
    assert_eq!((info["renderBackend"].clone(), info["method"].clone(), info["driver"].clone()), (json!("godot"), json!("forward_plus"), json!("d3d12")));
    assert_eq!(info["versions"]["gdext"], "0.5.5");
    assert!(info["versions"]["godot"].as_str().unwrap().starts_with("4.7.2"), "{info}");
    assert_eq!(info["frameChannels"]["l2"], "rd_async");
    let caps = r.call("render.capabilities", json!({}));
    assert_eq!((caps["renderBackend"].clone(), caps["pipelined"].clone(), caps["legs"].clone()), (json!("godot"), json!(true), json!(["sprite_mesh", "model", "sentinels_v6"])));
    assert!(caps["coverage"]["skipped"].as_array().is_some_and(|a| !a.is_empty()));
    assert_eq!(caps["maxDraws"], json!({ "spriteMesh": null, "model": null, "sentinelsV6": null }));
    let ping = r.call("host.ping", json!({}));
    assert_eq!(ping["pong"], true, "核心(物理 / RPC)照常:{ping}");
    eprintln!("ready_ms={} info={info}", g.ready_ms);
}

/// 四种配置都出非空帧、同一场景连续两次取帧哈希相同、与 rurix 同帧逐像素最大差 ≤ 2(实测 1)。
#[test]
fn four_configs_render_stable_frames_close_to_rurix() {
    let _g = serial();
    let (w, h) = (320, 180);
    let reference = {
        let rx = Rurix::start();
        let mut r = common::Rpc::connect(rx.port);
        r.call("scene.load", json!({ "path": MAZE }));
        r.frame(w, h).1
    };
    for (method, driver, l2) in [
        ("forward_plus", "d3d12", "rd_async"),
        ("forward_plus", "vulkan", "rd_async"),
        ("mobile", "d3d12", "rd_async"),
        ("gl_compatibility", "opengl3", "texture_2d_get"),
    ] {
        let g = Godot::start(method, driver, &[], &[]);
        let mut r = g.rpc();
        r.call("scene.load", json!({ "path": MAZE }));
        let (f1, p1) = r.frame(w, h);
        let (_, p2) = r.frame(w, h);
        let info = r.call("render.backendInfo", json!({}));
        assert_eq!((info["method"].clone(), info["driver"].clone()), (json!(method), json!(driver)), "实际生效 = 请求");
        assert_eq!(info["frameChannels"]["l2"], l2);
        assert_eq!(info["ready"], true);
        assert_eq!((f1["draws"].clone(), f1["framePath"].clone()), (json!(36), json!("no_share")), "{method}/{driver}");
        assert!(f1["nonZeroPixels"].as_u64().unwrap() > 10_000, "{method}/{driver} 非空帧:{f1:?}");
        assert_eq!(sha256(&p1), sha256(&p2), "{method}/{driver} 同一场景两次取帧应逐字节相同");
        let (n, max) = diff(&p1, &reference);
        eprintln!("{method}/{driver}: nonZero={} diffPixels={n} maxDiff={max} sha={}", f1["nonZeroPixels"], &sha256(&p1)[..16]);
        assert!(max <= 2, "{method}/{driver} 与 rurix 最大通道差 {max} > 2");
    }
}


/// WS 推流(proto 1):subscribe → hello → 二进制帧(20 B 头 + 紧凑 RGBA8)→ 每秒 status。
#[test]
fn ws_stream_delivers_frames() {
    let _g = serial();
    let g = Godot::start("forward_plus", "d3d12", &[], &[]);
    let mut r = g.rpc();
    r.call("scene.load", json!({ "path": MAZE }));
    let url = r.call("viewport.streamInfo", json!({}))["wsUrl"].as_str().unwrap().to_string();
    let (mut ws, _) = tungstenite::connect(url.as_str()).expect("WS 直连");
    if let tungstenite::stream::MaybeTlsStream::Plain(s) = ws.get_ref() {
        s.set_read_timeout(Some(std::time::Duration::from_millis(200))).unwrap();
    }
    ws.send(tungstenite::Message::Text(json!({ "type": "subscribe", "width": 96, "height": 54, "maxFps": 30 }).to_string())).unwrap();
    let t0 = std::time::Instant::now();
    let (mut frames, mut status, mut texts) = (0, None::<Value>, Vec::new());
    let mut nudged = std::time::Instant::now();
    while t0.elapsed().as_secs() < 15 && (frames < 3 || status.is_none()) {
        // 编辑态按 scene_rev 空闲跳帧、status 随出帧每秒一条:隔 1.2 s 动一下相机让推流继续出帧。
        if nudged.elapsed().as_millis() > 1200 {
            nudged = std::time::Instant::now();
            r.call("viewport.setCamera", json!({ "yaw": 30.0 + t0.elapsed().as_secs_f64() }));
        }
        match ws.read() {
            Ok(tungstenite::Message::Binary(b)) => {
                assert_eq!(&b[..4], b"FGF1");
                assert_eq!((u16::from_le_bytes([b[8], b[9]]), u16::from_le_bytes([b[10], b[11]])), (96, 54));
                assert_eq!(b.len(), 20 + 96 * 54 * 4);
                frames += 1;
            }
            Ok(tungstenite::Message::Text(t)) => {
                let v: Value = serde_json::from_str(&t).unwrap();
                assert_ne!(v["type"], "error", "推流报错:{v}");
                if v["type"] == "status" {
                    status = Some(v.clone());
                }
                texts.push(v);
            }
            Ok(_) => {}
            Err(tungstenite::Error::Io(e)) if matches!(e.kind(), std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut) => {}
            Err(e) => panic!("WS:{e}"),
        }
    }
    assert!(frames >= 3, "15 s 内二进制帧不足 3:{frames},texts={texts:?}");
    let s = status.unwrap_or_else(|| panic!("15 s 内没有 status:frames={frames} texts={texts:?}"));
    assert_eq!(s["deviceName"], r.call("render.backendInfo", json!({}))["deviceName"]);
}

/// L1(01 X1):D3D12 + 同卡 → 共享 buffer 由 Godot 主队列直写,framePath = zero_copy、cpuUploads 不涨;
/// 共享 buffer 的内容与同一场景的 L2 像素逐字节相同(也确认了行序:首行在上);
/// 硬性关卡:--gpu-validation 打开 D3D12 debug layer,整轮没有 error / corruption 级消息。
#[cfg(windows)]
#[test]
fn l1_zero_copy_matches_l2_under_d3d12_debug_layer() {
    let _g = serial();
    let g = Godot::start_aligned("forward_plus", "d3d12", &["--gpu-validation"]);
    let mut r = g.rpc();
    let info = r.call("render.backendInfo", json!({}));
    assert_eq!(info["frameChannels"]["l1Available"], true, "本机 L1 应可用:{info}");
    r.call("scene.load", json!({ "path": MAZE }));
    let (w, h) = (320u32, 180u32);
    let (_, l2) = r.frame(w, h); // 首帧:L2 基准(同一静态场景,之后每帧逐字节相同)
    let share = r.call("viewport.shareOpen", json!({ "pid": std::process::id(), "width": w, "height": h }));
    let (pitch, size) = (share["rowPitch"].as_u64().unwrap() as usize, share["bufferSize"].as_u64().unwrap());
    assert_eq!((share["handleKind"].clone(), pitch), (json!("buffer"), 1280));
    let mut rd = common::share::Reader::open(share["texHandle"].as_u64().unwrap(), share["fenceHandle"].as_u64().unwrap(), size);
    let uploads0 = r.call("viewport.frame", json!({ "width": w, "height": h, "format": "none" }))["cpuUploads"].as_u64().unwrap();
    let mut paths = Vec::new();
    for _ in 0..6 {
        let (f, px) = r.frame(w, h);
        assert_eq!(sha256(&px), sha256(&l2), "L1 期间 L2 像素照常");
        paths.push(f["framePath"].as_str().unwrap().to_string());
        assert_eq!(f["cpuUploads"].as_u64().unwrap(), uploads0, "L1 帧不走 CPU 上传:{f}");
    }
    assert!(paths.iter().all(|p| p == "zero_copy"), "{paths:?}");
    let v = rd.fence_value();
    assert!(v >= 5, "共享 fence 应随 L1 帧推进:{v}");
    let buf = rd.read(v);
    let tight: Vec<u8> = (0..h as usize).flat_map(|y| buf[y * pitch..y * pitch + w as usize * 4].to_vec()).collect();
    let (n, max) = diff(&tight, &l2);
    assert_eq!((n, max), (0, 0), "共享 buffer 与 L2 像素应逐字节相同(含行序)");
    std::thread::sleep(std::time::Duration::from_millis(1500)); // 等 [gmain] 刷新 debug layer 统计(每 30 帧一次)
    let info = r.call("render.backendInfo", json!({}));
    let fc = &info["frameChannels"];
    assert_eq!(fc["l1Active"], true, "{fc}");
    assert!(fc["l1Frames"].as_u64().unwrap() >= 5, "{fc}");
    let dl = &fc["debugLayer"];
    assert!(dl.is_object(), "--gpu-validation 下应能读到 InfoQueue:{fc}");
    let errors: Vec<String> = g.log().into_iter().filter(|l| l.contains("Message Id Number") || l.contains("D3D12 debug layer")).collect();
    eprintln!("L1: fence={v} l1Frames={} l1Lag={} l2Lag={} debugLayer={dl} logged={}", fc["l1Frames"], fc["l1Lag"], fc["l2Lag"], errors.len());
    assert_eq!(fc["l1Lag"], 1, "L1 比渲染晚 1 帧(a2 变体):{fc}");
    assert_eq!((dl["errors"].as_u64(), dl["corruption"].as_u64()), (Some(0), Some(0)), "debug layer 报错:{errors:?}");
    assert!(errors.iter().all(|l| !l.contains("ERROR") && !l.contains("CORRUPTION")), "{errors:?}");
    for l in &errors {
        eprintln!("debug layer: {l}");
    }
    r.call("viewport.shareClose", json!({}));
    let fc = r.call("render.backendInfo", json!({}))["frameChannels"].clone();
    assert_eq!(fc["l1Active"], false, "shareClose 之后 L1 关闭:{fc}");
}

/// viewport.pick 走共用的 CPU 逻辑(02 §3.2 P4),与 rurix 结果逐字相同。
#[test]
fn pick_matches_rurix() {
    let _g = serial();
    let rx = Rurix::start();
    let mut a = common::Rpc::connect(rx.port);
    let g = Godot::start("forward_plus", "d3d12", &[], &[]);
    let mut b = g.rpc();
    for r in [&mut a, &mut b] {
        r.call("scene.load", json!({ "path": MAZE }));
    }
    let mut hits = 0;
    for (x, y) in (0..8).flat_map(|i| (0..5).map(move |j| (20 + i * 40, 18 + j * 36))) {
        let p = json!({ "x": x, "y": y, "width": 320, "height": 180 });
        let (ra, rb) = (a.call("viewport.pick", p.clone()), b.call("viewport.pick", p));
        assert_eq!(ra, rb, "pick({x},{y})");
        hits += usize::from(ra["hit"] == true);
    }
    assert!(hits > 5, "点选应命中若干墙体:{hits}");
}

/// PIE:play.enter → Running 下出帧 → play.exit 回到编辑态(物理 / 逻辑仍只走 rurix-physics + forge-logic)。
#[test]
fn pie_enter_frame_exit() {
    let _g = serial();
    let g = Godot::start("forward_plus", "d3d12", &[], &[]);
    let mut r = g.rpc();
    r.call("scene.load", json!({ "path": MAZE }));
    r.call("play.enter", json!({}));
    assert_eq!(r.call("play.state", json!({}))["state"], "play_running");
    let (f, _) = r.frame(160, 90);
    assert!(f["draws"].as_u64().unwrap() > 0);
    r.call("play.exit", json!({}));
    assert_eq!(r.call("play.state", json!({}))["state"], "edit");
    let e = r.raw("template.preview", json!({}));
    assert_eq!(e["error"]["message"], "prefabRef required", "template.preview 参数错误与 rurix 同文");
}
