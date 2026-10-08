//! 主循环类 ForgeHost(01 §4.3:`#[class(base=SceneTree)]`;02 §5.3:`initialize` 里 start_core)。
//! 一帧(01 T3):`process()` 取交接箱 → RenderDelta → RS 调用 → 引擎自己 sync + draw → `frame_post_draw` 发起导出。
//! 宿主不自己调 RS::draw,也不在锁里等帧;所有 RS / RD 调用都在这里(Godot 主线程)。

use std::sync::Arc;
use std::time::{Duration, Instant};

use godot::classes::window::Mode;
use godot::classes::{Engine, ISceneTree, RenderingDevice, RenderingServer, SceneTree, Window};
use godot::prelude::*;

use engine_host::{
    AcceptMode, BackendInfo, BackendKind, Channel, ConfigSource, ControlMsg, CoreConfig, CoreHandle, FrameOrigin,
    RenderDriver, RenderList,
};

use crate::backend::{GodotBackend, Shared};
use crate::config;
use crate::export::{self, FrameMeta, L1Log};
use crate::scene::{Assets, Slot};

#[derive(GodotClass)]
#[class(base=SceneTree, init)]
pub struct ForgeHost {
    base: Base<SceneTree>,
    rt: Option<Runtime>,
}

#[godot_api]
impl ISceneTree for ForgeHost {
    fn initialize(&mut self) {
        match Runtime::start() {
            Ok(rt) => {
                self.rt = Some(rt);
                let cb = Callable::from_object_method(&self.to_gd(), "forge_post_draw");
                let err = RenderingServer::singleton().connect("frame_post_draw", &cb);
                if err != godot::global::Error::OK {
                    eprintln!("godot-host: 连接 frame_post_draw 失败:{err:?}");
                }
            }
            Err(e) => {
                // 自己打印结构化错误再退出,不走引擎的 OS::alert 模态框(00 §2 第 3 条)。
                eprintln!("{e}");
                self.base_mut().quit_ex().exit_code(1).done();
            }
        }
    }

    fn process(&mut self, _delta: f64) -> bool {
        let root = self.base().get_root();
        let quit = match self.rt.as_mut() {
            Some(rt) => {
                rt.keep_window_drawable(root);
                rt.process();
                rt.quit_requested
            }
            None => false,
        };
        if quit {
            self.base_mut().quit();
        }
        false
    }

    fn finalize(&mut self) {
        if let Some(rt) = self.rt.take() {
            rt.shutdown();
        }
    }
}

#[godot_api]
impl ForgeHost {
    /// RS 的 frame_post_draw(Safe 模式下在主线程、同一次 RS::draw 之内触发,01 §1.3)。
    #[func]
    fn forge_post_draw(&mut self) {
        if let Some(rt) = self.rt.as_mut() {
            rt.post_draw();
        }
    }
}

pub struct Runtime {
    rs: Gd<RenderingServer>,
    rd: Option<Gd<RenderingDevice>>,
    shared: Arc<Shared>,
    assets: Assets,
    main: Slot,
    preview: Option<Slot>,
    /// 本帧 process() 应用了清单、等 frame_post_draw 发起导出的帧(每条通道至多一份)。
    rendered: Vec<FrameMeta>,
    #[cfg(windows)]
    l1: Option<crate::l1::L1>,
    l1log: L1Log,
    frame_no: u64,
    device: String,
    core: CoreHandle,
    last_minimized_warn: Option<Instant>,
    debug_errors_printed: bool,
    /// 实际渲染方式是 Mobile(预热帧只在这里需要,见 apply)。
    mobile: bool,
    pub quit_requested: bool,
}

impl Runtime {
    fn start() -> Result<Runtime, String> {
        let cfg = config::from_env()?;
        let mut rs = RenderingServer::singleton();
        // 以 Godot 实际生效的方式 / 驱动为准(模板可能因为不支持而回退,01 §4.3)。
        let method_s = rs.get_current_rendering_method().to_string();
        let driver_s = rs.get_current_rendering_driver_name().to_string();
        let (method, driver) = (config::parse_method(&method_s), config::parse_driver(&driver_s));
        let mut source = cfg.source;
        let method_off = cfg.requested_method.is_some() && cfg.requested_method != method;
        let driver_off = cfg.requested_driver.is_some() && cfg.requested_driver != driver;
        if method_off || driver_off {
            eprintln!(
                "godot-host: 请求 {:?}/{:?},Godot 实际生效 {method_s}/{driver_s};以实际值为准(source=cli)",
                cfg.requested_method.map(config::method_str),
                cfg.requested_driver.map(config::driver_str)
            );
            source = ConfigSource::Cli;
        }
        let rd = rs.get_rendering_device();
        let device = rs.get_video_adapter_name().to_string();
        let godot_version = Engine::singleton().get_version_info().get(&"string".to_variant()).map(|v| v.to_string());
        let shared = Arc::new(Shared::default());
        #[cfg(windows)]
        let (l1, l1_reason) = match (&rd, driver) {
            (Some(rd), Some(RenderDriver::D3d12)) => match crate::l1::L1::probe(rd) {
                Ok(l) => {
                    eprintln!("godot-host: L1 可用(同一 adapter;enhanced barriers = {})", l.enhanced_barriers());
                    (Some(l), "L1 可用,等 viewport.shareOpen".to_string())
                }
                Err(e) => {
                    eprintln!("godot-host: L1 不可用:{e}");
                    // 监督器据此用 --gpu-index 重启一次,让 Godot 与 presenter 的缺省 adapter 对齐(02 §6.2 补充)。
                    if e.starts_with("LUID 不一致") {
                        if let Some(i) = crate::l1::gpu_index_for_default_adapter() {
                            use std::io::Write;
                            println!("FORGE_GODOT_GPU_HINT index={i}");
                            let _ = std::io::stdout().flush();
                        }
                    }
                    (None, e)
                }
            },
            (None, _) => (None, "Compatibility(GLES3)没有 RenderingDevice,只走 L2".to_string()),
            _ => (None, format!("驱动 {driver_s} 不是 D3D12(Godot 的 Vulkan 驱动没开 Win32 外部内存,01 X2),只走 L2")),
        };
        #[cfg(windows)]
        let l1_available = l1.is_some();
        // 共享 buffer 建在消费端(presenter)的卡上:同卡用 Godot 的 device(L1),否则另建一个(只走 L2 的 CPU 上传档)。
        #[cfg(windows)]
        match crate::l1::install_share_device(l1.as_ref().map(|l| l.device())) {
            Ok(gpu_writer) => eprintln!("godot-host: 共享 buffer device 已安装(L1 直写 = {gpu_writer})"),
            Err(e) => eprintln!("godot-host: 共享 buffer device 安装失败,presenter 腿不可用:{e}"),
        }
        #[cfg(not(windows))]
        let (l1_available, l1_reason) = (false, "非 Windows,只走 L2".to_string());
        let l2 = if rd.is_some() { "rd_async" } else { "texture_2d_get" };
        let adapter = device.clone();
        shared.update_channels(|c| {
            c.l2 = l2;
            c.l1_available = l1_available;
            c.l1_reason = Some(l1_reason);
            c.adapter = Some(adapter);
        });
        let info = BackendInfo { kind: BackendKind::Godot, method, driver, source, godot_version };
        let backend = GodotBackend::new(info, l1_available, Arc::clone(&shared));
        let assets = Assets::new(&mut rs, method == Some(engine_host::RenderMethod::GlCompatibility));
        let mobile = method == Some(engine_host::RenderMethod::Mobile);
        let main = Slot::new(&mut rs, mobile, true);
        // 就绪行由 start_core 打印(与 rurix 逐字相同);accept 在 [accept] 线程,主线程还给 Godot。
        let core = engine_host::start_core(CoreConfig {
            port: cfg.port,
            game_scene: cfg.game_scene,
            project_root: cfg.project_root,
            backend: Box::new(backend),
            accept: AcceptMode::Thread,
        })
        .map_err(|e| e.message)?;
        Ok(Runtime {
            rs,
            rd,
            shared,
            assets,
            main,
            preview: None,
            rendered: Vec::new(),
            #[cfg(windows)]
            l1,
            l1log: L1Log::default(),
            frame_no: 0,
            device,
            core,
            last_minimized_warn: None,
            debug_errors_printed: false,
            mobile,
            quit_requested: false,
        })
    }
}


impl Runtime {
    /// `process()`:先控制消息、后清单(ControlMsg 约定);超时的等帧请求在这里失败(RENDER_TIMEOUT:)。
    fn process(&mut self) {
        self.assets.graphs.tick(&mut self.rs);
        self.assets.sprites.graphs.tick(&mut self.rs);
        let now = Instant::now();
        let (ctrl, list) = self.shared.sink.main_box.take();
        for m in ctrl {
            self.control(m);
        }
        self.shared.sink.main_box.expire(now);
        self.shared.sink.preview_box.expire(now);
        if let Some(l) = list {
            self.apply(Channel::Main, l);
        }
        let (_, plist) = self.shared.sink.preview_box.take();
        if let Some(l) = plist {
            self.apply(Channel::Preview, l);
        }
    }

    fn apply(&mut self, ch: Channel, list: Arc<RenderList>) {
        let slot = match ch {
            Channel::Main => &mut self.main,
            Channel::Preview => {
                let mobile = self.mobile;
                self.preview.get_or_insert_with(|| Slot::new(&mut self.rs, mobile, false))
            }
        };
        let (draws, fresh) = slot.apply(&mut self.rs, &mut self.assets, Arc::clone(&list));
        self.rendered.retain(|m| m.ch != ch);
        self.rendered.push(FrameMeta {
            ch,
            seq: list.seq,
            scene_rev: list.scene_rev,
            width: list.width,
            height: list.height,
            want_pixels: list.want_pixels,
            draws,
            triangles: list.stats.triangles,
            mesh_fallbacks: list.stats.mesh_fallbacks,
            frame_no: self.frame_no + 1, // 本次 process() 之后的那次 RS::draw
            // Mobile:新材质第一次绘制走 ubershader、专用管线编好后切换,两者差 1 LSB(g4_stability 实测,
            // F+ / Compatibility 没有);内容刚变的帧先不交付,再画一帧,保证"同一场景连续取帧逐字节相同"。
            warm: fresh && self.mobile,
        });
    }

    fn control(&mut self, m: ControlMsg) {
        match m {
            ControlMsg::ShareAttach(t) => {
                #[cfg(windows)]
                {
                    let r = match (self.l1.as_mut(), self.rd.as_mut()) {
                        (Some(l1), Some(rd)) => Some(l1.attach(rd, t)),
                        _ => None,
                    };
                    match r {
                        Some(Ok(())) => self.shared.update_channels(|c| {
                            c.l1_active = true;
                            c.l1_reason = None;
                        }),
                        Some(Err(e)) => self.degrade_l1(e),
                        None => crate::l1::release_target(t),
                    }
                }
                #[cfg(not(windows))]
                let _ = t;
            }
            ControlMsg::ShareDetach => {
                #[cfg(windows)]
                if let (Some(l1), Some(rd)) = (self.l1.as_mut(), self.rd.as_mut()) {
                    l1.detach(rd);
                }
                self.shared.update_channels(|c| {
                    c.l1_active = false;
                    c.l1_reason = Some("共享 buffer 已关闭(shareClose),等下一次 shareOpen".into());
                });
                self.shared.detach_done();
            }
            ControlMsg::InvalidateAssets { .. } => {
                self.main.reset();
                if let Some(p) = self.preview.as_mut() {
                    p.reset();
                }
                self.assets.invalidate();
                // 全部实例 / 灯 / 按内容缓存的资源都已释放;只剩常驻的着色器与 overlay(g4_mesh 泄漏测试读这一行)。
                eprintln!("godot-host: live RID handles = {}", crate::rid::live());
            }
            ControlMsg::Shutdown => self.quit_requested = true,
        }
    }

    /// L1 任一步失败:放开共享 buffer、只走 L2(X3:只降级、不停帧)。
    #[cfg(windows)]
    fn degrade_l1(&mut self, e: String) {
        eprintln!("godot-host: L1 降级为 L2:{e}");
        if let (Some(l1), Some(rd)) = (self.l1.as_mut(), self.rd.as_mut()) {
            l1.detach(rd);
        }
        self.shared.update_channels(|c| {
            c.l1_active = false;
            c.l1_reason = Some(format!("降级:{e}"));
        });
    }

    fn post_draw(&mut self) {
        self.assets.graphs.post_draw();
        self.assets.sprites.graphs.post_draw();
        self.frame_no += 1;
        self.shared.set_frame_no(self.frame_no);
        // ① L1 第 2 段:上一帧录进 RD 的拷贝已随本帧命令缓冲提交 → 拷进共享 buffer + Signal。
        #[cfg(windows)]
        {
            let r = self.l1.as_mut().map(|l| l.stage2());
            match r {
                Some(Ok(Some(meta))) => {
                    self.l1log.push(meta.seq);
                    let lag = self.frame_no.saturating_sub(meta.frame_no);
                    self.shared.update_channels(|c| {
                        c.l1_frames += 1;
                        c.l1_lag = Some(lag);
                    });
                    if !meta.want_pixels {
                        export::deliver(&self.shared, &meta, Vec::new(), FrameOrigin::SharedGpuCopy, &self.device);
                    }
                }
                Some(Err(e)) => self.degrade_l1(e),
                _ => {}
            }
        }
        // ② 本帧新画的视口:L1 第 1 段 + L2。预热帧(Mobile、内容刚变)不导出,让视口下一帧再画一次。
        let mut again = Vec::new();
        for meta in std::mem::take(&mut self.rendered) {
            if meta.warm {
                match meta.ch {
                    Channel::Main => self.main.redraw(&mut self.rs),
                    Channel::Preview => {
                        if let Some(p) = self.preview.as_ref() {
                            p.redraw(&mut self.rs);
                        }
                    }
                }
                again.push(FrameMeta { warm: false, frame_no: self.frame_no + 1, ..meta });
                continue;
            }
            let vp = match meta.ch {
                Channel::Main => self.main.viewport,
                Channel::Preview => match self.preview.as_ref() {
                    Some(p) => p.viewport,
                    None => continue,
                },
            };
            let vp_tex = self.rs.viewport_get_texture(vp);
            #[allow(unused_mut)]
            let mut l1_taken = false;
            #[cfg(windows)]
            if meta.ch == Channel::Main {
                let vp_rd = self.rs.texture_get_rd_texture(vp_tex);
                let r = match (self.l1.as_mut(), self.rd.as_mut()) {
                    (Some(l1), Some(rd)) => Some(l1.stage1(rd, vp_rd, &meta)),
                    _ => None,
                };
                match r {
                    Some(Ok(t)) => l1_taken = t,
                    Some(Err(e)) => self.degrade_l1(e),
                    None => {}
                }
            }
            if meta.want_pixels {
                match self.rd.as_mut() {
                    Some(rd) => {
                        let vp_rd = self.rs.texture_get_rd_texture(vp_tex);
                        let (sh, log, dev) = (Arc::clone(&self.shared), self.l1log.clone(), self.device.clone());
                        if let Err(e) = export::start_rd_readback(rd, vp_rd, meta.clone(), sh, log, dev) {
                            eprintln!("godot-host: L2 发起失败 seq={}:{e}", meta.seq);
                        }
                    }
                    None => match export::gles_readback(&self.rs, vp_tex, &meta) {
                        Ok(px) => export::deliver(&self.shared, &meta, px, FrameOrigin::CpuReadback, &self.device),
                        Err(e) => eprintln!("godot-host: GLES3 回读失败 seq={}:{e}", meta.seq),
                    },
                }
            } else if !l1_taken {
                export::deliver(&self.shared, &meta, Vec::new(), FrameOrigin::NoPixels, &self.device);
            }
        }
        self.rendered.extend(again);
        #[cfg(windows)]
        if self.frame_no.is_multiple_of(30) {
            if let Some(l1) = self.l1.as_ref() {
                let s = l1.debug_stats();
                if !self.debug_errors_printed && s.is_some_and(|s| s.errors + s.corruption + s.warnings > 0) {
                    self.debug_errors_printed = true;
                    for m in l1.debug_messages(8, true) {
                        eprintln!("godot-host: D3D12 debug layer {m}");
                    }
                }
                self.shared.update_channels(|c| c.debug_layer = s);
            }
        }
    }

    /// 主窗口一最小化就停绘(00 §2 第 1 条,main.cpp:5080-5095):发现即恢复,1 s 限速告警。
    fn keep_window_drawable(&mut self, mut root: Gd<Window>) {
        if root.get_mode() == Mode::MINIMIZED {
            root.set_mode(Mode::WINDOWED);
            if self.last_minimized_warn.is_none_or(|t| t.elapsed() >= Duration::from_secs(1)) {
                self.last_minimized_warn = Some(Instant::now());
                eprintln!("godot-host: 宿主窗口被最小化(会停帧),已恢复");
            }
        }
    }

    fn shutdown(mut self) {
        self.core.shutdown();
        for b in [&self.shared.sink.main_box, &self.shared.sink.preview_box] {
            b.fail_all("RENDER_NOT_READY: 渲染后端正在关停");
        }
        #[cfg(windows)]
        if let (Some(mut l1), Some(rd)) = (self.l1.take(), self.rd.as_mut()) {
            l1.detach(rd);
        }
        let mut rs = self.rs.clone();
        self.main.free(&mut rs);
        if let Some(p) = self.preview.take() {
            p.free(&mut rs);
        }
        self.assets.free();
    }
}
