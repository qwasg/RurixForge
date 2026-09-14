//! engine-host 监督器:autoStart spawn + 长度前缀帧客户端 + 500ms 看门狗 +
//! host-events.jsonl 落盘(host.crashed / host.restarted)。

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use serde_json::{json, Value};

use forge_util::timeutil::utc_now_iso8601;

/// 单帧上限(与 engine-host 侧一致)。
const MAX_FRAME: usize = 8 * 1024 * 1024;
/// 等 engine-host 就绪行超时。
const START_TIMEOUT: Duration = Duration::from_secs(10);
/// host 单次调用读超时。
const CALL_TIMEOUT: Duration = Duration::from_secs(5);
/// Large sprite atlases and a cold graphics pipeline can legitimately take
/// longer than an ordinary RPC. Keep ping/input responsive without treating
/// that one-time asset upload as a crashed engine.
fn response_timeout(method: &str) -> Duration {
    if matches!(method, "viewport.frame" | "template.preview" | "asset.reload" | "play.enter") {
        Duration::from_secs(30)
    } else { CALL_TIMEOUT }
}

/// workspace 根:CARGO_MANIFEST_DIR(…/crates/mcp/engine-scene-mcp)上三级。
pub fn workspace_root() -> PathBuf {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    manifest
        .ancestors()
        .nth(3)
        .expect("CARGO_MANIFEST_DIR 上三级须存在")
        .to_path_buf()
}

/// engine-host 二进制路径:env FORGE_ENGINE_HOST_BIN > 默认 <root>/target/debug/engine-host.exe。
pub fn host_bin_path() -> PathBuf {
    if let Ok(p) = std::env::var("FORGE_ENGINE_HOST_BIN") {
        return PathBuf::from(p);
    }
    workspace_root().join("target").join("debug").join("engine-host.exe")
}

/// host 事件日志路径:<root>/data/host-events.jsonl。
pub fn events_log_path() -> PathBuf {
    workspace_root().join("data").join("host-events.jsonl")
}

/// 监督器状态(单 Mutex 守护;stdio 线程与看门狗线程共享)。
pub struct Supervisor {
    host_bin: PathBuf,
    log_path: PathBuf,
    child: Option<Child>,
    stream: Option<TcpStream>,
    /// host 进程存活标记(看门狗沿边记录 crashed/restarted)。
    alive: bool,
    /// 最近一次启动失败原因(供工具调用回报结构化错误)。
    pub last_start_error: Option<String>,
    next_id: u64,
}

impl Supervisor {
    /// 建监督器并立即 autoStart(失败不 panic,记录 last_start_error,看门狗续试)。
    pub fn new() -> Self {
        let mut sup = Supervisor {
            host_bin: host_bin_path(),
            log_path: events_log_path(),
            child: None,
            stream: None,
            alive: false,
            last_start_error: None,
            next_id: 1,
        };
        match sup.start_host() {
            Ok(()) => sup.alive = true,
            Err(e) => sup.last_start_error = Some(e),
        }
        sup
    }

    /// 看门狗单拍:ping 失败 → 记 host.crashed → 重启 → scene.new 恢复 → 记 host.restarted。
    pub fn watchdog_tick(&mut self) {
        let ping_ok = self.alive && self.call_host("host.ping", json!({})).is_ok();
        if ping_ok {
            return;
        }
        if self.alive || self.last_start_error.is_some() {
            // 沿边记录一次 crashed(存活→失败,或启动即失败首次 tick)。
            let reason = if self.alive {
                "host.ping 失败(连接中断或超时)".to_string()
            } else {
                format!("启动失败:{}", self.last_start_error.clone().unwrap_or_default())
            };
            self.append_log(&json!({
                "ts": utc_now_iso8601(),
                "event": "host.crashed",
                "reason": reason,
            }));
            self.alive = false;
            self.last_start_error = None;
        }
        // 清理残留进程,重启。
        self.teardown();
        match self.start_host() {
            Ok(()) => {
                self.alive = true;
                // 恢复场景(契约:重启后 scene.new 一次)。
                let _ = self.call_host("scene.new", json!({ "name": "restored" }));
                self.append_log(&json!({
                    "ts": utc_now_iso8601(),
                    "event": "host.restarted",
                }));
            }
            Err(e) => {
                // 本拍重启失败,下拍重试;不重复记 crashed(非沿边)。
                self.last_start_error = Some(e);
            }
        }
    }

    /// 调用 host 方法(监督器外接口;host 不在线 → 结构化 Err)。
    pub fn call(&mut self, method: &str, params: Value) -> Result<Value, Value> {
        self.call_host(method, params).map_err(|e| {
            json!({
                "error": e,
                "hint": "engine-host 不在线;看门狗每 500ms 探测并自动重启",
                "lastStartError": self.last_start_error,
            })
        })
    }

    /// 读 host-events.jsonl,返回行数组(每行一个 JSON 值;坏行跳过)。
    pub fn read_events_log(&self) -> Vec<Value> {
        let Ok(text) = std::fs::read_to_string(&self.log_path) else {
            return Vec::new();
        };
        text.lines()
            .filter_map(|l| serde_json::from_str(l).ok())
            .collect()
    }

    /// 追加一行 JSONL(必要时创建 data 目录)。
    fn append_log(&self, v: &Value) {
        if let Some(dir) = self.log_path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let line = serde_json::to_string(v).unwrap_or_else(|_| "{}".to_string());
        if let Ok(mut f) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.log_path)
        {
            let _ = writeln!(f, "{line}");
        }
    }

    /// 关闭旧连接/进程(尽力而为)。
    fn teardown(&mut self) {
        self.stream = None;
        if let Some(mut c) = self.child.take() {
            let _ = c.kill();
            let _ = c.wait();
        }
    }

    /// 对外暴露的关停口:stdin 关闭后由 main 显式调用,确保 host 不成为孤儿。
    pub fn shutdown(&mut self) {
        self.teardown();
    }

    /// spawn engine-host:先 bind 127.0.0.1:0 取空闲端口 drop 后传 --port;
    /// 等 FORGE_HOST_LISTENING 行,10s 超时结构化错误。
    fn start_host(&mut self) -> Result<(), String> {
        if !self.host_bin.exists() {
            return Err(format!("engine-host 二进制不存在:{}", self.host_bin.display()));
        }
        let port = TcpListener::bind("127.0.0.1:0")
            .and_then(|l| l.local_addr())
            .map(|a| a.port())
            .map_err(|e| format!("取空闲端口失败:{e}"))?;
        // listener 已 drop,端口短暂空闲窗交给 host 绑定(同机独占使用,可接受)。
        let mut child = Command::new(&self.host_bin)
            .args(["--port", &port.to_string()])
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            // stderr 落文件(性能/故障定位用;F-GAME-2 期间 viewport 分段计时依赖)。
            .stderr(
                std::fs::OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(workspace_root().join("engine-host-err.log"))
                    .map(Stdio::from)
                    .unwrap_or(Stdio::null()),
            )
            .spawn()
            .map_err(|e| format!("spawn engine-host 失败:{e}"))?;
        let stdout = child.stdout.take().ok_or("无 stdout 管道")?;

        // 读行线程 + channel,主线程带超时收就绪行。
        let (tx, rx) = mpsc::channel::<Option<String>>();
        std::thread::spawn(move || {
            let mut reader = BufReader::new(stdout);
            let mut line = String::new();
            loop {
                line.clear();
                match reader.read_line(&mut line) {
                    Ok(0) => {
                        let _ = tx.send(None); // EOF:host 提前退出
                        return;
                    }
                    Ok(_) => {
                        if tx.send(Some(line.trim().to_string())).is_err() {
                            return; // 接收方已走,退出读线程
                        }
                    }
                    Err(_) => {
                        let _ = tx.send(None);
                        return;
                    }
                }
            }
        });

        let deadline = Instant::now() + START_TIMEOUT;
        loop {
            let remain = deadline.saturating_duration_since(Instant::now());
            if remain.is_zero() {
                let _ = child.kill();
                return Err("等 FORGE_HOST_LISTENING 超时(10s)".to_string());
            }
            match rx.recv_timeout(remain) {
                Ok(Some(line)) => {
                    if line.starts_with("FORGE_HOST_LISTENING") {
                        break;
                    }
                    // 非就绪行(理论上没有),继续等
                }
                Ok(None) => {
                    let _ = child.kill();
                    return Err("engine-host 就绪前退出".to_string());
                }
                Err(_) => {
                    let _ = child.kill();
                    return Err("等 FORGE_HOST_LISTENING 超时(10s)".to_string());
                }
            }
        }

        let stream = TcpStream::connect(("127.0.0.1", port))
            .map_err(|e| format!("连接 engine-host 失败:{e}"))?;
        let _ = stream.set_read_timeout(Some(CALL_TIMEOUT));
        let _ = stream.set_write_timeout(Some(CALL_TIMEOUT));
        self.child = Some(child);
        self.stream = Some(stream);
        Ok(())
    }

    /// host 客户端:长度前缀帧一发一收;JSON-RPC error → Err(message)。
    fn call_host(&mut self, method: &str, params: Value) -> Result<Value, String> {
        let stream = self.stream.as_mut().ok_or_else(|| "engine-host 未连接".to_string())?;
        stream.set_read_timeout(Some(response_timeout(method)))
            .map_err(|e| format!("设置响应超时失败:{e}"))?;
        let id = self.next_id;
        self.next_id += 1;
        let req = json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": method,
            "params": params,
        });
        let payload = serde_json::to_vec(&req).map_err(|e| e.to_string())?;
        let len = u32::try_from(payload.len()).map_err(|_| "请求帧超长")?;
        stream
            .write_all(&len.to_le_bytes())
            .and_then(|()| stream.write_all(&payload))
            .and_then(|()| stream.flush())
            .map_err(|e| format!("写请求失败:{e}"))?;

        let mut len_buf = [0u8; 4];
        stream
            .read_exact(&mut len_buf)
            .map_err(|e| format!("读响应头失败:{e}"))?;
        let resp_len = u32::from_le_bytes(len_buf) as usize;
        if resp_len > MAX_FRAME {
            return Err("响应帧长超上限".to_string());
        }
        let mut buf = vec![0u8; resp_len];
        stream
            .read_exact(&mut buf)
            .map_err(|e| format!("读响应体失败:{e}"))?;
        let resp: Value = serde_json::from_slice(&buf).map_err(|e| format!("响应非 JSON:{e}"))?;
        if let Some(err) = resp.get("error") {
            return Err(format!("host 返回错误:{}", err));
        }
        Ok(resp["result"].clone())
    }
}
