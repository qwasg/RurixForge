//! stream — 视口直连推流通道(帧推送 + 实时输入)。
//!
//! 动机:既有帧通路是「浏览器 100ms 轮询 → host 代理 → agentd → MCP stdio → TCP →
//! 持全局锁同步渲染 + base64 JSON 原路返回」,实测 5-8 fps;渲染回读把 60Hz 物理/
//! 逻辑线程一并阻塞,且 play 态浏览器键盘没有到 logic.inject_input 的任何通路。
//! 本模块让浏览器与 engine-host 直连 WebSocket(127.0.0.1 临时端口):
//! - 推流线程按节拍主动渲染并推送**二进制 RGBA8 帧**(零 base64、零 JSON 转发拷贝);
//! - 键盘/相机/选中/尺寸消息经同一连接低延迟回传(input 直入 input_queue);
//! - 渲染在场景快照上**锁外**执行,物理线程不再被帧通道阻塞;
//! - 编辑态按 scene_rev 空闲跳帧(场景/相机没变不渲染,空闲零 GPU 开销)。
//! 既有 MCP 轮询腿(viewport.frame)完整保留:agent 截图与浏览器降级回退仍走旧路。
//!
//! 安全面:仅绑定 127.0.0.1;握手 URL 须带 `viewport.streamInfo` 下发的随机 token
//! (防本机任意网页盲连);token 不落盘,进程重启即换。
//!
//! 帧消息布局(小端,20B 头 + 紧凑 RGBA8):
//! `magic "FGF1" | frameId u32 | width u16 | height u16 | flags u32 | draws u32`
//! flags:bit0 = play_running,bit1 = truncated,bit2 = imported(零拷贝档)。

use std::collections::VecDeque;
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, OnceLock};
use std::thread;
use std::time::{Duration, Instant};

use serde_json::{json, Value};
use tungstenite::handshake::server::{ErrorResponse, Request, Response};
use tungstenite::{Message, WebSocket};

use crate::render::{self, FramePath, FrameRequester, SnapshotParams};
use crate::rpc::{self, HostState, PlayState};

/// 帧消息头长度(magic4 + frameId4 + w2 + h2 + flags4 + draws4)。
pub const FRAME_HEADER_LEN: usize = 20;
/// 推流尺寸上限:与浏览器侧 1280 封顶一致;同时保证遗留 MCP 腿按流尺寸出帧时
/// base64 JSON 响应(≈4.9MB)仍在 8MB TCP 帧上限内。
pub(crate) const MAX_W: u32 = 1280;
pub(crate) const MAX_H: u32 = 720;
/// 文本消息队列上限(status/error 小消息;溢出丢最旧,绝不堆积内存)。
const TEXT_QUEUE_CAP: usize = 8;
/// 空闲轮询粒度:无订阅者 50ms;编辑态 rev 未变 15ms(≈66Hz 检查,开销可忽略)。
const IDLE_NO_SUBS_MS: u64 = 50;
const IDLE_UNCHANGED_MS: u64 = 15;

/// 推流服务器信息(viewport.streamInfo RPC 下发面)。
pub struct StreamInfo {
    pub port: u16,
    pub token: String,
}

static INFO: OnceLock<StreamInfo> = OnceLock::new();
static REGISTRY: OnceLock<Arc<Registry>> = OnceLock::new();

/// 服务器已启动则给 (port, token);未启动(端口绑定失败)返回 None。
pub fn info() -> Option<&'static StreamInfo> {
    INFO.get()
}

/// 遗留 viewport.frame 腿的尺寸对齐:有订阅者时返回主订阅者尺寸。
/// 多消费者尺寸不一致会触发 1-5s 会话重建拉锯(agent 截图 960×540 vs 流面板尺寸
/// 交替重建);推流期间以流为尺寸权威,遗留腿响应 width/height 如实为准。
pub fn primary_size() -> Option<(u32, u32)> {
    REGISTRY
        .get()
        .and_then(|r| r.primary_cfg())
        .map(|c| (c.width, c.height))
}

/// 订阅配置(连接线程写、推流线程读;整体在 Registry 锁内更新)。
#[derive(Debug, Clone, Copy)]
struct SubCfg {
    width: u32,
    height: u32,
    max_fps: u32,
    selected: Option<u64>,
}

/// 单订阅者:推流线程写信箱,连接线程取走发送。
struct Subscriber {
    id: u64,
    cfg: SubCfg,
    /// latest-wins 帧信箱(Arc 共享编码后的完整 WS 负载;慢消费者只丢帧不堆积)。
    frame: Arc<Mutex<Option<Arc<Vec<u8>>>>>,
    /// 文本消息队列(status/error;上限 TEXT_QUEUE_CAP,溢出丢最旧)。
    texts: Arc<Mutex<VecDeque<String>>>,
}

/// 订阅注册表 + 配置代次(subscribe/resize/select 自增,推流线程据此即刻重渲)。
struct Registry {
    subs: Mutex<Vec<Subscriber>>,
    cfg_rev: AtomicU64,
    next_id: AtomicU64,
}

/// 毒化容忍加锁(与 rpc::lock 同纪律:取回内部值,不 panic)。
fn plock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

impl Registry {
    fn new() -> Self {
        Registry {
            subs: Mutex::new(Vec::new()),
            cfg_rev: AtomicU64::new(0),
            next_id: AtomicU64::new(1),
        }
    }

    fn bump(&self) {
        self.cfg_rev.fetch_add(1, Ordering::Relaxed);
    }

    fn cfg_rev(&self) -> u64 {
        self.cfg_rev.load(Ordering::Relaxed)
    }

    /// 主订阅者 = 最新注册者(重连产生的新订阅压过僵尸连接)。
    fn primary_cfg(&self) -> Option<SubCfg> {
        plock(&self.subs).last().map(|s| s.cfg)
    }

    fn register(
        &self,
        cfg: SubCfg,
        frame: Arc<Mutex<Option<Arc<Vec<u8>>>>>,
        texts: Arc<Mutex<VecDeque<String>>>,
    ) -> u64 {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        plock(&self.subs).push(Subscriber { id, cfg, frame, texts });
        self.bump();
        id
    }

    fn unregister(&self, id: u64) {
        plock(&self.subs).retain(|s| s.id != id);
        self.bump();
    }

    fn set_selected(&self, id: u64, selected: Option<u64>) {
        let mut subs = plock(&self.subs);
        if let Some(s) = subs.iter_mut().find(|s| s.id == id) {
            if s.cfg.selected != selected {
                s.cfg.selected = selected;
                drop(subs);
                self.bump();
            }
        }
    }

    /// subscribe(重复)/resize 共用:更新尺寸与 maxFps(钳制在协议上限内)。
    fn update_cfg(&self, id: u64, v: &Value) {
        let mut subs = plock(&self.subs);
        if let Some(s) = subs.iter_mut().find(|x| x.id == id) {
            let next = parse_cfg(v, Some(s.cfg));
            if !cfg_eq(s.cfg, next) {
                s.cfg = next;
                drop(subs);
                self.bump();
            }
        }
    }

    /// 广播一帧(latest-wins:未取走的旧帧直接覆盖)。
    fn broadcast_frame(&self, buf: Arc<Vec<u8>>) {
        for s in plock(&self.subs).iter() {
            *plock(&s.frame) = Some(Arc::clone(&buf));
        }
    }

    fn broadcast_text(&self, text: String) {
        for s in plock(&self.subs).iter() {
            let mut q = plock(&s.texts);
            if q.len() >= TEXT_QUEUE_CAP {
                q.pop_front();
            }
            q.push_back(text.clone());
        }
    }
}

fn cfg_eq(a: SubCfg, b: SubCfg) -> bool {
    a.width == b.width && a.height == b.height && a.max_fps == b.max_fps && a.selected == b.selected
}

/// 解析订阅配置(subscribe/resize 消息;prev 给了则未带字段沿用)。
fn parse_cfg(v: &Value, prev: Option<SubCfg>) -> SubCfg {
    let base = prev.unwrap_or(SubCfg {
        width: 960,
        height: 540,
        max_fps: 60,
        selected: None,
    });
    let gi = |k: &str, d: u32| v.get(k).and_then(Value::as_u64).map(|x| x as u32).unwrap_or(d);
    SubCfg {
        width: gi("width", base.width).clamp(16, MAX_W),
        height: gi("height", base.height).clamp(16, MAX_H),
        max_fps: gi("maxFps", base.max_fps).clamp(1, 60),
        selected: base.selected,
    }
}

/// 随机 token(非加密用途:本机回环端口防误连;熵源 = 单调/系统时钟 + pid + 栈地址,
/// fnv 双轮混合;进程重启即换)。
fn gen_token() -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    let ticks = Instant::now();
    let mut src = Vec::with_capacity(40);
    src.extend_from_slice(&now.as_nanos().to_le_bytes());
    src.extend_from_slice(&(std::process::id() as u64).to_le_bytes());
    src.extend_from_slice(&((&ticks as *const _ as usize) as u64).to_le_bytes());
    let h1 = crate::meshres::fnv1a64(&src);
    src.extend_from_slice(&h1.to_le_bytes());
    let h2 = crate::meshres::fnv1a64(&src);
    format!("{h1:016x}{h2:016x}")
}

/// 启动推流服务器:绑定 127.0.0.1 临时端口,spawn accept 线程 + 推流线程。
/// 返回端口;绑定失败如实 Err(main 打日志,视口自然回退轮询腿)。
pub fn spawn(state: Arc<Mutex<HostState>>) -> std::io::Result<u16> {
    let listener = TcpListener::bind(("127.0.0.1", 0))?;
    let port = listener.local_addr()?.port();
    let reg = Arc::new(Registry::new());
    let _ = INFO.set(StreamInfo { port, token: gen_token() });
    let _ = REGISTRY.set(Arc::clone(&reg));
    {
        let state = Arc::clone(&state);
        let reg = Arc::clone(&reg);
        thread::spawn(move || render_loop(&state, &reg));
    }
    thread::spawn(move || {
        for conn in listener.incoming() {
            if crate::core_stopping() {
                return; // 关停:wake() 的那次连接把 accept 唤醒到这里
            }
            let Ok(sock) = conn else { continue };
            let state = Arc::clone(&state);
            let reg = Arc::clone(&reg);
            thread::spawn(move || serve_ws(sock, &state, &reg));
        }
    });
    Ok(port)
}

/// 关停时唤醒阻塞在 accept 上的推流线程(连一下自己的端口即可;未启动则什么都不做)。
pub(crate) fn wake() {
    if let Some(i) = INFO.get() {
        let _ = TcpStream::connect_timeout(&std::net::SocketAddr::from(([127, 0, 0, 1], i.port)), Duration::from_millis(200));
    }
}

/// IO 空转错误(读超时;非连接终结)。
fn is_idle(e: &tungstenite::Error) -> bool {
    matches!(
        e,
        tungstenite::Error::Io(io)
            if matches!(io.kind(), std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut)
    )
}

/// 单连接服务:握手(token 校验)→ 首条 subscribe → 注册 → 收发主循环。
/// 主循环单线程交替:发帧信箱/文本队列 → 5ms 超时读(输入消息)。写不拆线程,
/// 回环 3.7MB 写 <1ms,5ms 读粒度给帧推送的额外延迟上限即 5ms。
fn serve_ws(sock: TcpStream, state: &Mutex<HostState>, reg: &Registry) {
    let _ = sock.set_nodelay(true);
    // 握手与首条 subscribe 限 5s(半开连接不长占线程)。
    let _ = sock.set_read_timeout(Some(Duration::from_secs(5)));
    let Some(info) = INFO.get() else { return };
    let expected = info.token.as_str();
    let auth = |req: &Request, resp: Response| -> Result<Response, ErrorResponse> {
        let q = req.uri().query().unwrap_or("");
        if q.split('&').any(|kv| kv.strip_prefix("token=") == Some(expected)) {
            Ok(resp)
        } else {
            let mut r = ErrorResponse::new(Some("token 校验失败".to_string()));
            *r.status_mut() = tungstenite::http::StatusCode::FORBIDDEN;
            Err(r)
        }
    };
    let mut ws: WebSocket<TcpStream> = match tungstenite::accept_hdr(sock, auth) {
        Ok(w) => w,
        Err(_) => return, // 坏握手/坏 token:静默断(对端已收 403)
    };

    // 首条消息须为 subscribe(5s 内)。
    let cfg = loop {
        match ws.read() {
            Ok(Message::Text(t)) => {
                let Ok(v) = serde_json::from_str::<Value>(&t) else { return };
                if v.get("type").and_then(Value::as_str) == Some("subscribe") {
                    break parse_cfg(&v, None);
                }
            }
            Ok(Message::Ping(_) | Message::Pong(_)) => continue,
            Ok(Message::Close(_)) => { let _ = ws.flush(); return; }
            Ok(_) => return,
            Err(_) => return, // 超时未订阅/断连:结束
        }
    };

    let frame_slot: Arc<Mutex<Option<Arc<Vec<u8>>>>> = Arc::new(Mutex::new(None));
    let texts: Arc<Mutex<VecDeque<String>>> = Arc::new(Mutex::new(VecDeque::new()));
    let sub_id = reg.register(cfg, Arc::clone(&frame_slot), Arc::clone(&texts));
    let hello = json!({
        "type": "hello",
        "proto": 1,
        "width": cfg.width,
        "height": cfg.height,
        "maxFps": cfg.max_fps,
    });
    if ws.send(Message::Text(hello.to_string())).is_err() {
        reg.unregister(sub_id);
        return;
    }
    let _ = ws.get_ref().set_read_timeout(Some(Duration::from_millis(5)));

    'conn: loop {
        // 1) 帧信箱(latest-wins,取走即发)。
        let pending = plock(&frame_slot).take();
        if let Some(buf) = pending {
            // tungstenite 0.24 Binary 收 Vec<u8>:单订阅者一次拷贝,回环带宽充裕。
            if ws.send(Message::Binary(buf.as_ref().clone())).is_err() {
                break 'conn;
            }
        }
        // 2) 文本队列(status/error)。
        loop {
            let next = plock(&texts).pop_front();
            match next {
                Some(t) => {
                    if ws.send(Message::Text(t)).is_err() {
                        break 'conn;
                    }
                }
                None => break,
            }
        }
        // 3) 读(5ms 超时;超时即空转回到发送步)。
        match ws.read() {
            Ok(Message::Text(t)) => handle_client_msg(&t, state, reg, sub_id),
            Ok(Message::Close(_)) => {
                // read() queues tungstenite's close reply. Flush it before
                // dropping TCP so intentional reconnects complete normally.
                let _ = ws.flush();
                break 'conn;
            }
            Ok(_) => {}
            Err(e) if is_idle(&e) => {}
            Err(_) => break 'conn,
        }
    }
    reg.unregister(sub_id);
}

/// 客户端消息分派(input/camera/select/resize/subscribe)。
/// 坏消息静默丢弃(实时输入通道,逐条报错只会刷屏)。
fn handle_client_msg(text: &str, state: &Mutex<HostState>, reg: &Registry, sub_id: u64) {
    let Ok(v) = serde_json::from_str::<Value>(text) else { return };
    match v.get("type").and_then(Value::as_str) {
        Some("input") => {
            let Some(action) = v.get("action").and_then(Value::as_str) else { return };
            let Some(value) = v.get("value").and_then(Value::as_f64) else { return };
            let mut st = rpc::lock(state);
            // edit 态静默丢弃:play.exit 后飞行中的 keyup 属正常时序,不算错。
            // 不逐条 push_event(60Hz 键盘会刷爆 1024 事件 ring,淹没 agent 诊断面)。
            let _ = rpc::queue_input(&mut st, action, value);
        }
        Some("pointer") => {
            // play 态指针点击:归一化坐标按本订阅者流尺寸的 aspect 反投影到游戏平面,
            // 入队 <action>_x/_y/_z + <action>(见 rpc::queue_pointer);edit 态静默丢弃。
            let action = v.get("action").and_then(Value::as_str).unwrap_or("click");
            let (Some(x), Some(y)) = (
                v.get("x").and_then(Value::as_f64),
                v.get("y").and_then(Value::as_f64),
            ) else {
                return;
            };
            let size = plock(&reg.subs)
                .iter()
                .find(|s| s.id == sub_id)
                .map(|s| (s.cfg.width, s.cfg.height));
            let mut st = rpc::lock(state);
            let _ = rpc::queue_pointer(&mut st, action, x, y, size);
        }
        Some("camera") => {
            let mut st = rpc::lock(state);
            // 与 viewport.setCamera 同一套子集更新+钳制;成功即 bump rev(编辑态跟手)。
            if rpc::viewport_set_camera(&mut st, &v).is_ok() {
                st.scene_rev = st.scene_rev.wrapping_add(1);
            }
        }
        Some("select") => {
            reg.set_selected(sub_id, v.get("id").and_then(Value::as_u64));
        }
        Some("resize") | Some("subscribe") => {
            reg.update_cfg(sub_id, &v);
        }
        _ => {}
    }
}

/// 编码一帧为 WS 二进制负载(20B 头 + 紧凑 RGBA8)。
fn encode_frame(frame_id: u32, f: &crate::viewport::FramePixels, playing: bool) -> Vec<u8> {
    let mut out = Vec::with_capacity(FRAME_HEADER_LEN + f.rgba8.len());
    out.extend_from_slice(b"FGF1");
    out.extend_from_slice(&frame_id.to_le_bytes());
    out.extend_from_slice(&(f.width as u16).to_le_bytes());
    out.extend_from_slice(&(f.height as u16).to_le_bytes());
    let mut flags = 0u32;
    if playing {
        flags |= 1;
    }
    if f.truncated {
        flags |= 2;
    }
    if f.imported {
        flags |= 4;
    }
    out.extend_from_slice(&flags.to_le_bytes());
    out.extend_from_slice(&(f.draws.min(u32::MAX as usize) as u32).to_le_bytes());
    out.extend_from_slice(&f.rgba8);
    out
}

/// 推流腿的一帧:rurix 同步出帧(Owned);Pipelined 从 FrameBus 取(Shared,02 §4.5 第 3/4/6 项)。
enum StreamFrame {
    Owned(crate::viewport::FramePixels),
    Shared(Arc<crate::render::bus::FrameOut>),
}

impl StreamFrame {
    fn pixels(&self) -> &crate::viewport::FramePixels {
        match self {
            StreamFrame::Owned(f) => f,
            StreamFrame::Shared(o) => &o.pixels,
        }
    }

    /// 第 4 项:rurix = 原 feed_share_frame;Pipelined = render::sink::feed_share(L1 帧不再推 fence)。
    fn feed_share(&self) -> Result<(&'static str, bool), String> {
        match self {
            StreamFrame::Owned(f) => rpc::feed_share_frame(f),
            StreamFrame::Shared(o) => crate::render::sink::feed_share(o),
        }
    }
}

/// 第 3 项 Pipelined 分支:extract → submit(Main, list, None) → 等 seq ≥ 本快照、尺寸等于本订阅且带像素的帧(上限 1 s);
/// 其他请求者的帧(尺寸不同 / format=none)跳过。超时:首帧前 `RENDER_NOT_READY:`,之后 `RENDER_TIMEOUT:`。
fn pipelined_frame(
    p: &dyn crate::render::backend::PipelinedRender,
    snap: &crate::render::snapshot::RenderSnapshot,
) -> Result<Arc<crate::render::bus::FrameOut>, String> {
    use crate::render::bus::Channel;
    let list = render::pipelined_list(render::backend(), snap)?;
    p.submit(Channel::Main, list, None)?;
    let want = (snap.params.width, snap.params.height);
    let deadline = Instant::now() + Duration::from_secs(1);
    let mut after = snap.seq.saturating_sub(1);
    loop {
        let remain = deadline.saturating_duration_since(Instant::now());
        if remain.is_zero() {
            break;
        }
        match p.bus(Channel::Main).wait_newer(after, remain) {
            Some(out) if (out.pixels.width, out.pixels.height) == want && !out.pixels.rgba8.is_empty() => return Ok(out),
            Some(out) => after = out.seq,
            None => break,
        }
    }
    Err(if p.ready() {
        format!("RENDER_TIMEOUT: 推流等帧超时(seq={} {}x{})", snap.seq, want.0, want.1)
    } else {
        format!("RENDER_NOT_READY: 渲染后端尚未出首帧(seq={})", snap.seq)
    })
}

/// 推流主循环:快照(短锁)→ 锁外渲染 → 喂共享纹理 → 广播;
/// Running 全速(≤maxFps≤60),编辑态按 scene_rev/cfg_rev 空闲跳帧;
/// 渲染失败(DEV_ENV_DEGRADE 等)同因去重广播 error 并 1s 限速重试——绝不伪造帧。
fn render_loop(state: &Mutex<HostState>, reg: &Registry) {
    let mut frame_id: u32 = 0;
    // (scene_rev, cfg_rev) 已渲快照;None = 需渲。
    let mut rendered: Option<(u64, u64)> = None;
    let mut last_err: Option<String> = None;
    let mut sec_t0 = Instant::now();
    let mut sec_frames = 0u32;
    let mut fps = 0u32;
    let mut last_status = Instant::now();
    // 后端是进程级单例(I10):出帧形态在循环外取一次。
    let path = render::backend().path();
    loop {
        // CoreHandle::shutdown(AcceptMode::Thread)才会置位;rurix bin 从不置位。
        if crate::core_stopping() {
            return;
        }
        let Some(cfg) = reg.primary_cfg() else {
            rendered = None;
            last_err = None;
            thread::sleep(Duration::from_millis(IDLE_NO_SUBS_MS));
            continue;
        };
        let cfg_rev = reg.cfg_rev();
        let t0 = Instant::now();

        // ── 快照(短锁):场景克隆 + 相机 + play 态;渲染在锁外执行,
        // 物理线程与输入注入不再被帧通道阻塞(旧轮询腿的头号卡顿源)。──
        // render::snapshot 与原 (scene, cam, play, rev, vp_override) 元组同字段、同时机(02 §4.5 第 2 项)。
        let snap = {
            let st = rpc::lock(state);
            let rev = st.scene_rev;
            let play = st.play;
            if play != PlayState::Running && rendered == Some((rev, cfg_rev)) {
                drop(st);
                thread::sleep(Duration::from_millis(IDLE_UNCHANGED_MS));
                continue;
            }
            let params = SnapshotParams { scene_camera: false,
                width: cfg.width,
                height: cfg.height,
                selected: cfg.selected,
                want_readback: true,
                want_stats: false, // 推流路径跳过 nonzero 全帧扫描(诊断统计留给遗留腿)
                requester: FrameRequester::Stream,
            };
            let seq = match path {
                FramePath::Pipelined(p) => p.next_seq(),
                FramePath::Immediate(_) => 0,
            };
            render::snapshot(&st, params, seq)
        };
        let (play, rev) = (snap.play, snap.scene_rev);

        // 第 3 项:Immediate 分支的八个实参与原 render_scene_frame 调用一一相同(snap.input());
        // Pipelined 分支:extract → submit(Main) → 等 FrameBus 上 seq ≥ 本快照、尺寸等于本订阅的帧。
        let frame = match path {
            FramePath::Immediate(r) => r.render(snap.input()).map(StreamFrame::Owned),
            FramePath::Pipelined(p) => pipelined_frame(p, &snap).map(StreamFrame::Shared),
        };
        match frame {
            Ok(frame) => {
                let f = frame.pixels();
                // 共享纹理喂帧(presenter 腿自动获得推流节奏;失败随 status 如实带出)。
                let share = frame.feed_share();
                {
                    let mut st = rpc::lock(state);
                    st.frames += 1;
                    st.last_tris = f.triangles;
                    rpc::flush_text_issues(&mut st);
                    if matches!(share, Ok((_, true))) {
                        st.cpu_uploads += 1;
                    }
                }
                let playing = play == PlayState::Running;
                reg.broadcast_frame(Arc::new(encode_frame(frame_id, f, playing)));
                frame_id = frame_id.wrapping_add(1);
                rendered = Some((rev, cfg_rev));
                last_err = None;
                sec_frames += 1;
                if last_status.elapsed() >= Duration::from_secs(1) {
                    last_status = Instant::now();
                    let mut status = json!({
                        "type": "status",
                        "playState": play.as_str(),
                        "deviceName": f.device_name,
                        "draws": f.draws,
                        "truncated": f.truncated,
                        "fps": fps,
                    });
                    if let Err(e) = &share {
                        status["shareError"] = json!(e);
                    }
                    reg.broadcast_text(status.to_string());
                }
            }
            Err(e) => {
                if last_err.as_deref() != Some(e.as_str()) {
                    reg.broadcast_text(json!({ "type": "error", "message": e }).to_string());
                    last_err = Some(e);
                }
                thread::sleep(Duration::from_secs(1));
                continue;
            }
        }
        if sec_t0.elapsed() >= Duration::from_secs(1) {
            fps = sec_frames;
            sec_frames = 0;
            sec_t0 = Instant::now();
        }
        // 节拍:Running 全速;编辑/暂停态封顶 30fps(渲染本就 rev 驱动,
        // 此上限只约束连续相机拖拽这类高频 rev 场景)。
        let target = if play == PlayState::Running {
            cfg.max_fps.clamp(1, 60)
        } else {
            cfg.max_fps.clamp(1, 30)
        };
        let interval = Duration::from_micros(1_000_000 / u64::from(target));
        let spent = t0.elapsed();
        if spent < interval {
            thread::sleep(interval - spent);
        }
    }
}

// ─────────────────────────── 测试(纯 host 腿,无网络/设备依赖) ───────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn fake_frame(w: u32, h: u32) -> crate::viewport::FramePixels {
        crate::viewport::FramePixels {
            width: w,
            height: h,
            rgba8: vec![7u8; (w * h * 4) as usize],
            device_name: "test".into(),
            draws: 3,
            truncated: true,
            nonzero: 0,
            triangles: 6,
            mesh_fallbacks: 0,
            mesh_classes: 0,
            imported: true,
        }
    }

    #[test]
    fn frame_header_layout_roundtrip() {
        let f = fake_frame(320, 180);
        let buf = encode_frame(42, &f, true);
        assert_eq!(buf.len(), FRAME_HEADER_LEN + 320 * 180 * 4);
        assert_eq!(&buf[0..4], b"FGF1");
        assert_eq!(u32::from_le_bytes(buf[4..8].try_into().unwrap()), 42);
        assert_eq!(u16::from_le_bytes(buf[8..10].try_into().unwrap()), 320);
        assert_eq!(u16::from_le_bytes(buf[10..12].try_into().unwrap()), 180);
        let flags = u32::from_le_bytes(buf[12..16].try_into().unwrap());
        assert_eq!(flags, 1 | 2 | 4, "playing+truncated+imported 全置位");
        assert_eq!(u32::from_le_bytes(buf[16..20].try_into().unwrap()), 3);
        assert!(buf[FRAME_HEADER_LEN..].iter().all(|b| *b == 7));
    }

    #[test]
    fn parse_cfg_clamps_and_inherits() {
        let c = parse_cfg(
            &json!({ "type": "subscribe", "width": 4000, "height": 8, "maxFps": 999 }),
            None,
        );
        assert_eq!((c.width, c.height, c.max_fps), (MAX_W, 16, 60));
        // resize 未带 maxFps → 沿用;selected 由 select 消息独立管理不被覆盖。
        let prev = SubCfg { width: 640, height: 360, max_fps: 24, selected: Some(9) };
        let c2 = parse_cfg(&json!({ "type": "resize", "width": 800, "height": 600 }), Some(prev));
        assert_eq!((c2.width, c2.height, c2.max_fps, c2.selected), (800, 600, 24, Some(9)));
    }

    #[test]
    fn registry_latest_subscriber_is_primary_and_latest_wins_mailbox() {
        let reg = Registry::new();
        let f1 = Arc::new(Mutex::new(None));
        let t1 = Arc::new(Mutex::new(VecDeque::new()));
        let a = reg.register(
            SubCfg { width: 100, height: 100, max_fps: 30, selected: None },
            Arc::clone(&f1),
            Arc::clone(&t1),
        );
        let f2 = Arc::new(Mutex::new(None));
        let t2 = Arc::new(Mutex::new(VecDeque::new()));
        let _b = reg.register(
            SubCfg { width: 640, height: 360, max_fps: 60, selected: None },
            Arc::clone(&f2),
            Arc::clone(&t2),
        );
        assert_eq!(reg.primary_cfg().map(|c| c.width), Some(640), "最新订阅者为主");
        reg.broadcast_frame(Arc::new(vec![1]));
        reg.broadcast_frame(Arc::new(vec![2]));
        // latest-wins:未取走的旧帧被覆盖。
        assert_eq!(plock(&f1).take().map(|b| b[0]), Some(2));
        reg.unregister(a);
        assert_eq!(reg.primary_cfg().map(|c| c.width), Some(640));
        // 文本队列有界。
        for i in 0..20 {
            reg.broadcast_text(format!("m{i}"));
        }
        assert_eq!(plock(&t2).len(), TEXT_QUEUE_CAP);
    }

    #[test]
    fn token_is_nontrivial() {
        let t = gen_token();
        assert_eq!(t.len(), 32);
        assert_ne!(t, "0".repeat(32));
    }
}
