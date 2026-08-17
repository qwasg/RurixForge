//! engine-scene-mcp — Forge 引擎场景 MCP 服务器(stdio NDJSON JSON-RPC)。
//!
//! 启动即 autoStart engine-host(路径:env FORGE_ENGINE_HOST_BIN > 默认
//! <workspace>/target/debug/engine-host.exe);看门狗线程每 500ms host.ping,
//! 失败 → host-events.jsonl 记 host.crashed → 重启 → scene.new 恢复 → 记 host.restarted。

mod mcp;
mod supervisor;
mod timeutil;

use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use supervisor::Supervisor;

fn main() {
    // autoStart:Supervisor::new 内立即 spawn engine-host(失败转结构化错误,看门狗续试)。
    let sup = Arc::new(Mutex::new(Supervisor::new()));

    // 看门狗线程:500ms 一拍。
    {
        let sup = Arc::clone(&sup);
        thread::spawn(move || loop {
            thread::sleep(Duration::from_millis(500));
            let mut g = sup.lock().unwrap_or_else(|e| e.into_inner());
            g.watchdog_tick();
        });
    }

    // stdio MCP 主循环(阻塞至 stdin 关闭)。
    mcp::serve_stdio(Arc::clone(&sup));

    // stdin 关闭 = 调用方退出:显式关停 engine-host,防孤儿进程。
    if let Ok(mut g) = sup.lock() {
        g.shutdown();
    };
}
