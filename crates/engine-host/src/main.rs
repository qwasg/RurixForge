//! engine-host — Forge 引擎宿主进程。
//!
//! 控制通道:JSON-RPC 2.0 over TCP(4 字节小端长度前缀帧),绑定 127.0.0.1;
//! 后台线程以真实时间 accumulator 驱动 rurix-physics 固定步(dt=1/60)空跑。

mod frame;
mod rpc;
mod share;
mod timeutil;
mod viewport;

use std::io::Write;
use std::net::{TcpListener, TcpStream};
use std::process;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use rpc::{HostState, DT_FIXED};

fn main() {
    let port = parse_port();
    let game_scene = parse_game();
    let state = Arc::new(Mutex::new(HostState::new()));
    {
        let st = rpc::lock(&state);
        if st.physics.is_none() {
            eprintln!("engine-host: 无可用物理后端(Jolt/Rapier 均未编译),退出");
            process::exit(1);
        }
    }
    spawn_physics_thread(Arc::clone(&state));

    let listener = match TcpListener::bind(("127.0.0.1", port)) {
        Ok(l) => l,
        Err(e) => {
            eprintln!("engine-host: 绑定 127.0.0.1:{port} 失败:{e}");
            process::exit(1);
        }
    };
    let actual_port = listener.local_addr().map(|a| a.port()).unwrap_or(port);
    // 就绪行必须 stdout + flush(MCP autoStart 据此探测)。
    println!("FORGE_HOST_LISTENING port={actual_port}");
    let _ = std::io::stdout().flush();

    // F6 wave.4:--game <场景(项目根相对)> → 绑定后即 scene_load + play_enter;
    // 失败如实退出(出不了帧就是失败,不静默退化)。
    if let Some(scene) = &game_scene {
        let mut st = rpc::lock(&state);
        if let Err(e) = rpc::game_boot(&mut st, scene) {
            eprintln!("engine-host: --game 启动失败: {e}");
            process::exit(1);
        }
        println!("FORGE_HOST_GAME_BOOTED scene={scene}");
        let _ = std::io::stdout().flush();
    }

    for conn in listener.incoming() {
        match conn {
            Ok(stream) => {
                let st = Arc::clone(&state);
                thread::spawn(move || serve_conn(stream, st));
            }
            Err(_) => continue, // 单连接失败不影响主循环
        }
    }
}

/// 端口解析:CLI `--port N` / `--port=N` 优先,其次 env FORGE_HOST_PORT,缺省 17810。
fn parse_port() -> u16 {
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        if a == "--port" {
            if let Some(v) = args.next() {
                return v.parse().unwrap_or_else(|_| {
                    eprintln!("engine-host: 非法 --port 值:{v}");
                    process::exit(2);
                });
            }
            eprintln!("engine-host: --port 缺参数");
            process::exit(2);
        }
        if let Some(v) = a.strip_prefix("--port=") {
            return v.parse().unwrap_or_else(|_| {
                eprintln!("engine-host: 非法 --port 值:{v}");
                process::exit(2);
            });
        }
    }
    if let Ok(v) = std::env::var("FORGE_HOST_PORT") {
        if let Ok(p) = v.parse() {
            return p;
        }
        eprintln!("engine-host: 忽略非法 FORGE_HOST_PORT:{v}");
    }
    17810
}

/// --game 场景解析(F6 wave.4):`--game <项目根相对路径>` / `--game=<路径>`;env FORGE_GAME_SCENE 兜底。
fn parse_game() -> Option<String> {
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        if a == "--game" {
            if let Some(v) = args.next() {
                return Some(v);
            }
            eprintln!("engine-host: --game 缺参数");
            process::exit(2);
        }
        if let Some(v) = a.strip_prefix("--game=") {
            return Some(v.to_string());
        }
    }
    std::env::var("FORGE_GAME_SCENE").ok().filter(|v| !v.is_empty())
}

/// 物理后台线程:真实时间 accumulator 驱动固定步。
/// F4 wave.3:仅 play_running 态推进完整逻辑帧(advance_frame = 物理 step + 接触翻译 +
/// 变换回写 + 图解释);edit/play_paused 态不步进并重置 accumulator(edit 态步进会空耗
/// 且污染 steps 语义;Paused 态由 play.step 单帧驱动,墙钟步进会破坏确定性)。
fn spawn_physics_thread(state: Arc<Mutex<HostState>>) {
    thread::spawn(move || {
        let dt_secs = f64::from(DT_FIXED);
        let mut last = Instant::now();
        let mut acc = 0.0f64;
        loop {
            thread::sleep(Duration::from_millis(2));
            let now = Instant::now();
            acc += (now - last).as_secs_f64();
            last = now;
            if acc > 0.25 {
                acc = 0.25; // 防死亡螺旋
            }
            if acc < dt_secs {
                continue;
            }
            let mut st = rpc::lock(&state);
            if st.play != rpc::PlayState::Running {
                acc = 0.0;
                continue;
            }
            while acc >= dt_secs {
                rpc::advance_frame(&mut st);
                acc -= dt_secs;
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
