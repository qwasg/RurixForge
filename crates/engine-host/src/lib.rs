//! engine-host — Forge 引擎宿主进程。
//!
//! 控制通道:JSON-RPC 2.0 over TCP(4 字节小端长度前缀帧),绑定 127.0.0.1;
//! 后台线程以真实时间 accumulator 驱动 rurix-physics 固定步(dt=1/60)空跑。
//!
//! 02 §5.3:本 crate 拆成 lib(`engine_host`)+ bin(`engine-host`)。启动顺序集中在 [`start_core`],
//! rurix bin(src/main.rs)与 Stage 3 的 godot-host 共用。模块全部私有;lib 只导出启动接口
//! 与渲染后端接缝里出现的类型,外部 crate 据此实现 [`RenderBackend`]。

mod anim;
mod frame;
mod meshres;
mod modelrt;
mod material_override;
mod modelrender;
mod prefab;
mod character;
mod rpc;
mod share;
mod stream;
mod viewport;
mod render_core;
mod render;
#[cfg(feature = "backend-rurix")]
mod gpu_particles;
mod sentinels_v6;
mod sentinels_v6_render;
mod sentinels_v6_assets;
mod sentinels_v6_pressure;
mod sentinels_v6_clock;
mod sentinels_v6_pages;
mod sentinels_v6_backend_metrics;

use std::io::Write;
use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use rpc::DT_FIXED;

// ── 公共接口:RenderBackend 与其签名里出现的全部类型(02 §4.1-§4.2)──
pub use render::backend::{
    BackendInfo, BackendKind, Capabilities, ConfigSource, ControlMsg, Coverage, DebugLayerStats, FrameChannels, FrameInput,
    FramePath, ImmediateRender, LegSet, MaxDraws, PipelinedRender, RenderBackend, RenderDriver, RenderMethod, StatsCaps,
};
pub use render::bus::{Channel, FrameBus, FrameOut, FrameRequest, SubmitBox};
#[cfg(feature = "backend-rurix")]
pub use render::rurix::RurixBackend;
pub use render::sink::{normalize_rgba8, FrameOrigin, FrameSink, HostFrameSink, SharedTarget, SrcFormat};
pub use render_core::camera::{view_basis, EditorCamera, Projection, ViewSetup, ViewSource};
pub use render_core::delta::RenderDelta;
pub use render_core::list::{
    clear_rgb8, ExtractStats, ItemBody, ItemKey, Leg, LightItem, LightKind, MeshData, MeshRef, ModelData, ModelPrim,
    ParticleItem, RenderItem, RenderList, SpriteBlend, SpriteDraw, TexData, V6Frame, V6SpriteDraw, MODEL_CLEAR_RGBA, SPRITE_MESH_CLEAR_RGBA, V6_CLEAR_RGBA,
};
/// FORGE_GPU_PARTICLES 开关(与 rurix gpu_particles::enabled 同一判据;godot-host 据此填 Capabilities.particles)。
pub use render_core::particles::enabled as particles_enabled;
pub use render_core::math::M4;
pub use rpc::HostState;
pub use viewport::FramePixels;
/// Stage 4:ModelData.bundle 的类型(godot-host 按它建 RS 网格 / 材质;经这里再导出,godot-host 不另加依赖)。
pub use assetd::model::{ModelBundle, ModelMaterial, ModelPrimitive, ModelTexture};
/// 模型腿旧 MeshRenderer 的缺省 PBR 材质(rurix legacy_draw 同一份值)。
pub use render_core::model::default_material;

/// L1(02 §4.2 第 1 步):Godot 后端把共享对象用的 ID3D12Device 交给共享 buffer 模块(接管调用方已 AddRef 的一份引用)。
/// gpu_writer = true:这是 Godot 渲染用的 device(与消费端同卡,[gmain] 可直写,走 L1);false:消费端 adapter 上的
/// 另一个 device(只走 CPU 上传档)。之后 viewport.shareOpen 在这个 device 上建共享 buffer / fence。只能装一次。
#[cfg(windows)]
pub fn install_d3d12_device(device: usize, gpu_writer: bool) -> Result<(), String> {
    share::install_device(device, gpu_writer)
}

/// 02 §7.3 第 2 条:rurix bin 永远用 rurix。项目的 forge.toml 写了 `[render] backend = "godot"`(说明有人绕过监督器
/// 直接启动了 engine-host.exe)时只在 stderr 警告一行,不失败;没有 [render] 的项目什么都不打印(输出与之前逐字相同)。
pub fn warn_if_forge_toml_selects_godot() {
    if let Ok(p) = assetd::project::ForgeProject::load(rpc::project_root()) {
        if p.render.backend == assetd::project::RenderBackendKind::Godot {
            eprintln!(
                "engine-host: forge.toml [render] backend = \"godot\";本进程是 rurix 宿主,仍用 rurix 渲染(经 engine-scene-mcp 监督器启动才会拉起 Godot 宿主)"
            );
        }
    }
}

/// 进程级关停标志:只有 AcceptMode::Thread 的 [`CoreHandle::shutdown`] / 启动失败会置位,rurix bin 从不置位。
static CORE_STOP: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

pub(crate) fn core_stopping() -> bool {
    CORE_STOP.load(std::sync::atomic::Ordering::Relaxed)
}

/// 置位关停标志并唤醒阻塞在 accept 上的线程([phys] / [wsr] 在下一拍自行退出;已建立的连接随对端关闭结束)。
fn request_stop(rpc_port: Option<u16>) {
    CORE_STOP.store(true, std::sync::atomic::Ordering::Relaxed);
    stream::wake();
    if let Some(port) = rpc_port {
        let addr = std::net::SocketAddr::from(([127, 0, 0, 1], port));
        let _ = TcpStream::connect_timeout(&addr, Duration::from_millis(200));
    }
}

/// [`start_core`] / [`CoreConfig::from_args_env`] 失败的原因。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CoreErrorKind {
    /// 命令行参数非法(bin 退出码 2)。
    Usage,
    /// 进程内已装过渲染后端:start_core 只能成功进入一次(I10)。
    BackendInstalled,
    /// 无可用物理后端(Jolt/Rapier 均未编译)。
    NoPhysics,
    /// RPC 端口绑定失败。
    Bind,
    /// `--game` 引导失败;此时就绪行已经打印。
    GameBoot,
    /// AcceptMode::Thread 起 accept 线程失败。
    AcceptThread,
}

/// 启动错误。`message` 就是 bin 打到 stderr 的那一行(逐字,不含换行),[`CoreError::exit_code`] 是 bin 的退出码。
/// 核心从不调用 `process::exit`(在 godot-host 里那会杀掉 Godot 进程),退出与否由调用方决定。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CoreError {
    pub kind: CoreErrorKind,
    pub message: String,
}

impl CoreError {
    fn new(kind: CoreErrorKind, message: String) -> Self {
        CoreError { kind, message }
    }

    /// bin 的退出码:参数错 2,其余 1(与拆分前 main.rs 的 `process::exit` 一致)。
    pub fn exit_code(&self) -> i32 {
        match self.kind {
            CoreErrorKind::Usage => 2,
            _ => 1,
        }
    }
}

impl std::fmt::Display for CoreError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for CoreError {}

fn usage(message: String) -> CoreError {
    CoreError::new(CoreErrorKind::Usage, message)
}


/// accept 循环跑在哪里。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AcceptMode {
    /// rurix bin:start_core 在调用线程上进 accept 循环,永不返回(与拆分前的 main 一致)。
    Inline,
    /// godot-host:另起名为 `accept` 的线程,start_core 返回 [`CoreHandle`],调用线程还给宿主。
    Thread,
}

/// [`start_core`] 的输入。
pub struct CoreConfig {
    /// RPC 端口:`--port` > `FORGE_HOST_PORT` > 17810;0 = 由系统分配。
    pub port: u16,
    /// `--game` > `FORGE_GAME_SCENE`(项目根相对的场景路径)。
    pub game_scene: Option<String>,
    /// Some 时覆盖 rpc::project_root() 的 env / 编译期回退(cdylib 里编译期路径没有意义)。
    pub project_root: Option<PathBuf>,
    /// 进程内唯一的渲染后端(I10);start_core 的第一步就安装它。
    pub backend: Box<dyn RenderBackend>,
    pub accept: AcceptMode,
}

/// [`start_core`] 成功后的句柄(AcceptMode::Thread 时返回)。
pub struct CoreHandle {
    /// 实际绑定的 RPC 端口(配置为 0 时由系统分配)。
    pub port: u16,
    pub state: Arc<Mutex<HostState>>,
    /// 推流 WS 端口;stream::spawn 失败时为 None(只在 stderr 报一行,不致命)。
    pub stream_port: Option<u16>,
}

impl CoreHandle {
    /// 关停核心线程(step 6 遗留的 Thread 模式关停接口):置位关停标志,唤醒 RPC / 推流的 accept,
    /// [phys] 与 [wsr] 在下一拍退出。幂等;进程内不能再次 start_core(后端单例,I10)。
    pub fn shutdown(&self) {
        request_stop(Some(self.port));
    }
}

/// AcceptMode::Thread 下启动中途失败:已起的 [phys] / 推流线程一并关停,再把错误交还调用方(Godot 进程继续活着)。
/// Inline(rurix bin)不走这里:bin 随后就 exit,行为与拆分前逐字相同。
fn thread_mode_fail(accept: AcceptMode, e: CoreError) -> CoreError {
    if accept == AcceptMode::Thread {
        request_stop(None);
    }
    e
}

impl CoreConfig {
    /// 与拆分前 main 相同的解析顺序与文案:先 `--port` / `FORGE_HOST_PORT`,再 `--game` / `FORGE_GAME_SCENE`。
    /// project_root = None,accept = Inline(bin 的形态)。参数错返回 [`CoreErrorKind::Usage`](bin 退出码 2)。
    pub fn from_args_env(backend: Box<dyn RenderBackend>) -> Result<Self, CoreError> {
        let port = parse_port()?;
        let game_scene = parse_game()?;
        Ok(CoreConfig { port, game_scene, project_root: None, backend, accept: AcceptMode::Inline })
    }
}

/// 端口解析:CLI `--port N` / `--port=N` 优先,其次 env FORGE_HOST_PORT,缺省 17810。
fn parse_port() -> Result<u16, CoreError> {
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        if a == "--port" {
            if let Some(v) = args.next() {
                return v.parse().map_err(|_| usage(format!("engine-host: 非法 --port 值:{v}")));
            }
            return Err(usage("engine-host: --port 缺参数".to_string()));
        }
        if let Some(v) = a.strip_prefix("--port=") {
            return v.parse().map_err(|_| usage(format!("engine-host: 非法 --port 值:{v}")));
        }
    }
    if let Ok(v) = std::env::var("FORGE_HOST_PORT") {
        if let Ok(p) = v.parse() {
            return Ok(p);
        }
        eprintln!("engine-host: 忽略非法 FORGE_HOST_PORT:{v}");
    }
    Ok(17810)
}

/// --game 场景解析(F6 wave.4):`--game <项目根相对路径>` / `--game=<路径>`;env FORGE_GAME_SCENE 兜底。
fn parse_game() -> Result<Option<String>, CoreError> {
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        if a == "--game" {
            if let Some(v) = args.next() {
                return Ok(Some(v));
            }
            return Err(usage("engine-host: --game 缺参数".to_string()));
        }
        if let Some(v) = a.strip_prefix("--game=") {
            return Ok(Some(v.to_string()));
        }
    }
    Ok(std::env::var("FORGE_GAME_SCENE").ok().filter(|v| !v.is_empty()))
}


/// 启动宿主核心。固定顺序(与拆分前 main 一致):render::install → 项目根覆盖 → HostState::new → 无物理后端检查
/// → [phys] 线程 → stream::spawn → 绑 TCP → 就绪行 + flush → `--game` 引导 → accept。
///
/// install 是第一步:重复调用返回 [`CoreErrorKind::BackendInstalled`],此时不建状态、不起线程、不绑端口。
/// 失败一律返回错误(不调 `process::exit`);AcceptMode::Inline 成功时不返回。
pub fn start_core(cfg: CoreConfig) -> Result<CoreHandle, CoreError> {
    let CoreConfig { port, game_scene, project_root, backend, accept } = cfg;
    render::backend::install(backend)
        .map_err(|e| CoreError::new(CoreErrorKind::BackendInstalled, format!("engine-host: 渲染后端重复安装: {e}")))?;
    if let Some(root) = project_root {
        // 只有成功 install 的那一次调用走到这里,覆盖值因此至多设一次。
        let _ = rpc::set_project_root(root);
    }
    let state = Arc::new(Mutex::new(HostState::new()));
    {
        let st = rpc::lock(&state);
        if st.physics.is_none() {
            return Err(CoreError::new(
                CoreErrorKind::NoPhysics,
                "engine-host: 无可用物理后端(Jolt/Rapier 均未编译),退出".to_string(),
            ));
        }
    }
    spawn_physics_thread(Arc::clone(&state));

    // 视口直连推流通道(WS 帧推送 + 实时输入;地址经 viewport.streamInfo 下发)。
    // 失败不致命:视口自然回退 MCP 轮询腿。日志走 stderr——stdout 是 MCP autoStart
    // 的就绪行协议面,不得混入其他行。
    let stream_port = match stream::spawn(Arc::clone(&state)) {
        Ok(ws_port) => {
            eprintln!("engine-host: 视口推流 WS 就绪 127.0.0.1:{ws_port}");
            Some(ws_port)
        }
        Err(e) => {
            eprintln!("engine-host: 推流服务器启动失败(视口走轮询回退腿): {e}");
            None
        }
    };

    let listener = match TcpListener::bind(("127.0.0.1", port)) {
        Ok(l) => l,
        Err(e) => {
            return Err(thread_mode_fail(
                accept,
                CoreError::new(CoreErrorKind::Bind, format!("engine-host: 绑定 127.0.0.1:{port} 失败:{e}")),
            ));
        }
    };
    let actual_port = listener.local_addr().map(|a| a.port()).unwrap_or(port);
    // 就绪行必须 stdout + flush(MCP autoStart 据此探测)。
    println!("FORGE_HOST_LISTENING port={actual_port}");
    let _ = std::io::stdout().flush();

    // F6 wave.4:--game <场景(项目根相对)> → 绑定后即 scene_load + play_enter;
    // 失败如实返回错误(出不了帧就是失败,不静默退化)。
    if let Some(scene) = &game_scene {
        let mut st = rpc::lock(&state);
        if let Err(e) = rpc::game_boot(&mut st, scene) {
            drop(st);
            return Err(thread_mode_fail(
                accept,
                CoreError::new(CoreErrorKind::GameBoot, format!("engine-host: --game 启动失败: {e}")),
            ));
        }
        println!("FORGE_HOST_GAME_BOOTED scene={scene}");
        let _ = std::io::stdout().flush();
    }

    let handle = CoreHandle { port: actual_port, state: Arc::clone(&state), stream_port };
    match accept {
        AcceptMode::Inline => accept_loop(listener, state),
        AcceptMode::Thread => {
            thread::Builder::new()
                .name("accept".to_string())
                .spawn(move || accept_loop(listener, state))
                .map_err(|e| {
                    thread_mode_fail(
                        accept,
                        CoreError::new(CoreErrorKind::AcceptThread, format!("engine-host: accept 线程启动失败: {e}")),
                    )
                })?;
        }
    }
    Ok(handle)
}

/// accept 循环(拆分前 main 的末段):每个连接一个线程。`incoming()` 不会结束,所以 Inline 模式下不返回。
/// Thread 模式关停时 request_stop 连一下本端口,accept 醒来后在这里退出。
fn accept_loop(listener: TcpListener, state: Arc<Mutex<HostState>>) {
    for conn in listener.incoming() {
        if core_stopping() {
            return;
        }
        match conn {
            Ok(stream) => {
                let st = Arc::clone(&state);
                thread::spawn(move || serve_conn(stream, st));
            }
            Err(_) => continue, // 单连接失败不影响主循环
        }
    }
}

/// 物理后台线程:真实时间 accumulator 驱动固定步。
/// F4 wave.3:仅 play_running 态推进完整逻辑帧(advance_frame = 物理 step + 接触翻译 +
/// 变换回写 + 图解释);edit/play_paused 态不步进并重置 accumulator(edit 态步进会空耗
/// 且污染 steps 语义;Paused 态由 play.step 单帧驱动,墙钟步进会破坏确定性)。
fn spawn_physics_thread(state: Arc<Mutex<HostState>>) {
    thread::spawn(move || {
        let dt_secs = f64::from(DT_FIXED);
        let mut last = Instant::now();
        let mut clock = sentinels_v6_clock::Clock::default();
        loop {
            // CoreHandle::shutdown / Thread 模式启动失败才会置位;rurix bin 从不置位。
            if core_stopping() {
                return;
            }
            thread::sleep(Duration::from_millis(2));
            // Never hold HostState through an entire catch-up burst: one slow
            // script frame previously let 15 ticks monopolize the lock for ~0.5s,
            // starving viewport snapshots and input despite a fast GPU renderer.
            // Keep the remaining accumulator, release between ticks, and yield.
            let mut steps = 0;
            while steps < sentinels_v6_clock::MAX_STEPS_PER_ITERATION {
                let advanced = {
                    let wait_start=Instant::now();
                    let mut st = rpc::lock(&state);
                    let now=Instant::now();
                    let elapsed=(now-last).as_secs_f64();last=now;
                    clock.observe(elapsed,sentinels_v6_clock::domain(&st));
                    if !clock.due(dt_secs){break;}
                    sentinels_v6::record_clock(&mut st,wait_start.elapsed().as_secs_f64()*1000.,clock.debt(),elapsed);
                    let advanced=match st.play {
                        rpc::PlayState::Running => { rpc::advance_frame(&mut st); true }
                        rpc::PlayState::Edit => { rpc::advance_preview(&mut st, DT_FIXED); true }
                        rpc::PlayState::Paused => false,
                    };
                    if advanced{clock.consume(dt_secs);}
                    advanced
                };
                if !advanced { break; }
                steps += 1;
                thread::yield_now();
            }
        }
    });
}

/// 单连接服务循环:读帧 → 分派 → 写帧;坏 JSON 回 -32700 后继续服务。
fn serve_conn(mut stream: TcpStream, state: Arc<Mutex<HostState>>) {
    loop {
        let raw = match frame::read_frame_raw(&mut stream) {
            Ok(b) => b,
            Err(_) => return, // 对端关闭或帧损坏 → 结束本连接
        };
        let resp = match serde_json::from_slice::<serde_json::Value>(&raw) {
            Ok(req) => rpc::dispatch(&state, &req),
            Err(e) => rpc::err(
                serde_json::Value::Null,
                -32700,
                &format!("parse error: {e}"),
            ),
        };
        if frame::write_frame(&mut stream, &resp).is_err() {
            return;
        }
    }
}
pub mod shader;
