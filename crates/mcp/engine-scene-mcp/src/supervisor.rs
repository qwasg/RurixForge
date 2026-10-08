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
    if matches!(
        method,
        "viewport.frame" | "template.preview" | "asset.reload" | "play.enter"
    ) {
        Duration::from_secs(30)
    } else {
        CALL_TIMEOUT
    }
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
    workspace_root()
        .join("target")
        .join("debug")
        .join("engine-host.exe")
}

/// host 事件日志路径；允许独立运行实例指定文件，缺省保持 <root>/data/host-events.jsonl。
pub fn events_log_path() -> PathBuf {
    std::env::var_os("FORGE_HOST_EVENTS_LOG").filter(|p| !p.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| workspace_root().join("data").join("host-events.jsonl"))
}

/// Godot 宿主等就绪行的上限(02 §6.2:冷启动 2–5 s,首次运行还有着色器编译)。rurix 仍是 START_TIMEOUT(10 s)。
const GODOT_START_TIMEOUT: Duration = Duration::from_secs(30);
/// Godot 宿主连续启动失败时的重启退避上限(1 s、2 s、4 s…)。
const GODOT_BACKOFF_MAX: Duration = Duration::from_secs(30);

/// 读 forge.toml 的项目根:env FORGE_PROJECT_ROOT > <workspace>/projects/demo(与 engine-host 的回退一致)。
fn config_project_root() -> PathBuf {
    std::env::var("FORGE_PROJECT_ROOT")
        .ok()
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| workspace_root().join("projects").join("demo"))
}

/// 02 §7.3:forge.toml [render](assetd 是唯一解析器)叠加 env FORGE_RENDER_*;优先级 Env > ForgeToml > Default。
/// 返回 (配置, 来源)。解析失败 → Err(PARSE_ERR 原文),监督器不启动宿主。
pub fn resolve_render() -> Result<(assetd::project::RenderConfig, &'static str), String> {
    resolve_render_from(&config_project_root(), &|k: &str| std::env::var(k).ok())
}

/// resolve_render 的可测形态:显式给项目根与 env 查询。
fn resolve_render_from(
    root: &std::path::Path,
    env: &dyn Fn(&str) -> Option<String>,
) -> Result<(assetd::project::RenderConfig, &'static str), String> {
    use assetd::project::{ForgeProject, RenderConfig};
    let proj = ForgeProject::load(root).map_err(|e| format!("{}: {}", e.code, e.message))?;
    let toml = proj.render;
    let env = |k: &str| env(k).filter(|v| !v.is_empty());
    let (eb, em, ed) = (
        env("FORGE_RENDER_BACKEND"),
        env("FORGE_RENDER_METHOD"),
        env("FORGE_RENDER_DRIVER"),
    );
    if eb.is_none() && em.is_none() && ed.is_none() {
        let source = if toml == RenderConfig::default() {
            "default"
        } else {
            "forge.toml"
        };
        return Ok((toml, source));
    }
    let (tb, tm, td) = toml.as_strs();
    let (cfg, warnings) = RenderConfig::from_parts(
        Some(eb.as_deref().unwrap_or(tb)),
        em.as_deref().or(tm),
        ed.as_deref().or(td),
    )
    .map_err(|e| format!("{}: {}(env FORGE_RENDER_*)", e.code, e.message))?;
    for w in warnings {
        eprintln!("engine-scene-mcp: {w}");
    }
    Ok((cfg, "env"))
}

/// Godot 运行时目录:env FORGE_GODOT_RUNTIME_DIR > <workspace>/target/godot-runtime(scripts/godot-runtime.ps1 生成)。
pub fn godot_runtime_dir() -> PathBuf {
    std::env::var("FORGE_GODOT_RUNTIME_DIR")
        .ok()
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| workspace_root().join("target").join("godot-runtime"))
}

/// Validate the self-contained runtime before spawning it. Workspace-local runtimes also compare
/// the bundled DLL with the matching Cargo profile; external runtimes have no source-checkout dependency.
fn validate_godot_runtime(dir: &std::path::Path) -> Result<(), String> {
    if !dir.is_dir() {
        return Err(format!(
            "Godot 运行时目录不存在:{}(先运行 scripts\\godot-runtime.ps1 -Build)",
            dir.display()
        ));
    }
    let manifest = assetd::godot_runtime::validate_runtime(dir)
        .map_err(|e| format!("Godot 运行时校验失败 [{}]: {}", e.code, e.message))?;
    let target = workspace_root().join("target");
    let runtime_path = dir.canonicalize().unwrap_or_else(|_| dir.to_path_buf());
    let target_path = target.canonicalize().unwrap_or(target);
    if runtime_path.starts_with(&target_path) {
        let source_dll = target_path.join(&manifest.profile).join("godot_host.dll");
        assetd::godot_runtime::validate_development_runtime(dir, source_dll).map_err(|e| {
            if e.code == "GODOT_RUNTIME_STALE" {
                format!("Godot 开发运行时过期 [{}]: {}", e.code, e.message)
            } else {
                format!("Godot 运行时校验失败 [{}]: {}", e.code, e.message)
            }
        })?;
    }
    Ok(())
}

/// 一次启动用哪个宿主。
enum HostPlan {
    Rurix,
    Godot {
        method: &'static str,
        driver: &'static str,
        source: &'static str,
    },
}

/// 等就绪行的结果。
enum StartOutcome {
    Ready(Child),
    /// Godot 宿主报 LUID 不一致并给出对齐用的 --gpu-index(godot-host host.rs);监督器据此重启一次。
    GpuHint(u32),
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
    /// Godot 宿主报过的对齐 --gpu-index(FORGE_GODOT_GPU_HINT),之后的启动 / 看门狗重启都带上。
    godot_gpu_index: Option<u32>,
    /// 最近一次启动的是 Godot 宿主(看门狗对它退避,rurix 行为不变)。
    /// Most recent host plan. Kept separately so a failed start can retain
    /// the selected Godot mode for watchdog backoff.
    last_plan_godot: bool,
    fail_streak: u32,
    next_try: Option<Instant>,
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
            godot_gpu_index: None,
            last_plan_godot: false,
            fail_streak: 0,
            next_try: None,
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
                format!(
                    "启动失败:{}",
                    self.last_start_error.clone().unwrap_or_default()
                )
            };
            self.append_log(&json!({
                "ts": utc_now_iso8601(),
                "event": "host.crashed",
                "reason": reason,
            }));
            self.alive = false;
            self.last_start_error = None;
        }
        // 清理残留进程,重启。Godot 宿主连续失败时退避(1 s、2 s、4 s…上限 30 s,02 §6.2);rurix 照旧每拍重试。
        if self.last_plan_godot && self.next_try.is_some_and(|t| Instant::now() < t) {
            return;
        }
        self.teardown();
        match self.start_host() {
            Ok(()) => {
                self.alive = true;
                self.fail_streak = 0;
                self.next_try = None;
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
                if self.last_plan_godot {
                    self.fail_streak = self.fail_streak.saturating_add(1);
                    let wait =
                        Duration::from_secs(1u64 << self.fail_streak.saturating_sub(1).min(5))
                            .min(GODOT_BACKOFF_MAX);
                    self.next_try = Some(Instant::now() + wait);
                }
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

    /// 按 forge.toml [render] + env 选宿主再启动(02 §6.2、§7.3)。缺省 rurix,走 start_rurix(与之前逐字相同)。
    fn start_host(&mut self) -> Result<(), String> {
        let (render, source) = match resolve_render() {
            Ok(v) => v,
            // Preserve the rurix fallback for unrelated legacy forge.toml errors;
            // only an invalid [render] section blocks host selection.
            Err(e) if e.contains("forge.toml [render]") => return Err(e),
            Err(e) => {
                eprintln!("engine-scene-mcp: forge.toml 读取失败,按缺省 rurix 启动:{e}");
                (assetd::project::RenderConfig::default(), "default")
            }
        };
        let plan = match render.as_strs() {
            ("godot", Some(method), Some(driver)) => HostPlan::Godot {
                method,
                driver,
                source,
            },
            _ => HostPlan::Rurix,
        };
        self.last_plan_godot = matches!(plan, HostPlan::Godot { .. });
        let HostPlan::Godot {
            method,
            driver,
            source,
        } = plan
        else {
            return self.start_rurix(render, source);
        };
        // FORGE_GODOT_GPU_HINT:Godot 选的卡与 presenter 的缺省 adapter 不同 → 带 --gpu-index 重启一次(L1 需要同卡)。
        for _ in 0..2 {
            match self.start_godot(method, driver, source)? {
                StartOutcome::Ready(child) => {
                    self.child = Some(child);
                    return Ok(());
                }
                StartOutcome::GpuHint(i) => {
                    eprintln!("engine-scene-mcp: Godot 宿主提示 --gpu-index {i}(与缺省 adapter 对齐),重启一次");
                    self.godot_gpu_index = Some(i);
                }
            }
        }
        Err("Godot 宿主按 --gpu-index 重启后仍未就绪".to_string())
    }

    /// spawn engine-host:先 bind 127.0.0.1:0 取空闲端口 drop 后传 --port;
    /// 等 FORGE_HOST_LISTENING 行,10s 超时结构化错误。
    fn start_rurix(
        &mut self,
        render: assetd::project::RenderConfig,
        source: &'static str,
    ) -> Result<(), String> {
        if !self.host_bin.exists() {
            return Err(format!(
                "engine-host 二进制不存在:{}",
                self.host_bin.display()
            ));
        }
        let port = TcpListener::bind("127.0.0.1:0")
            .and_then(|l| l.local_addr())
            .map(|a| a.port())
            .map_err(|e| format!("取空闲端口失败:{e}"))?;
        // listener 已 drop,端口短暂空闲窗交给 host 绑定(同机独占使用,可接受)。
        let (backend, method, driver) = render.as_strs();
        let mut child = Command::new(&self.host_bin)
            .args(["--port", &port.to_string()])
            .env("FORGE_PROJECT_ROOT", config_project_root())
            .env("FORGE_RENDER_BACKEND", backend)
            .env("FORGE_RENDER_METHOD", method.unwrap_or(""))
            .env("FORGE_RENDER_DRIVER", driver.unwrap_or(""))
            .env("FORGE_RENDER_SOURCE", source)
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

    /// Godot 宿主(02 §6.2):运行时目录里的 console 版模板(杀它即经 Job 杀整棵树);核心参数一律走 env,
    /// FORGE_HOST_PORT 显式设置(与 Node 宿主的同名 env 冲突,§6.1)。就绪行按前缀逐行扫描(gdext / Godot 的横幅在前),
    /// 就绪后读行线程继续把 stdout 排空到 <workspace>/engine-host-out.log,否则管道写满 Godot 会卡住。
    fn start_godot(
        &mut self,
        method: &str,
        driver: &str,
        source: &str,
    ) -> Result<StartOutcome, String> {
        let dir = godot_runtime_dir();
        validate_godot_runtime(&dir)?;
        let exe = dir.join("forge-godot_console.exe");
        if !exe.exists() {
            return Err(format!(
                "Godot 运行时不存在:{}(先运行 scripts/godot-runtime.ps1 -Build)",
                exe.display()
            ));
        }
        let port = TcpListener::bind("127.0.0.1:0")
            .and_then(|l| l.local_addr())
            .map(|a| a.port())
            .map_err(|e| format!("取空闲端口失败:{e}"))?;
        let user_gpu = std::env::var("FORGE_GODOT_GPU_INDEX")
            .ok()
            .and_then(|v| v.parse::<u32>().ok());
        let gpu = user_gpu.or(self.godot_gpu_index);
        let mut cmd = Command::new(&exe);
        cmd.current_dir(&dir).args([
            "--rendering-method",
            method,
            "--rendering-driver",
            driver,
            "--audio-driver",
            "Dummy",
        ]);
        if let Some(i) = gpu {
            cmd.args(["--gpu-index", &i.to_string()]);
        }
        if let Ok(extra) = std::env::var("FORGE_GODOT_ARGS") {
            cmd.args(extra.split_whitespace()); // 排障用(如 --gpu-validation / --verbose)
        }
        cmd.env("FORGE_HOST_PORT", port.to_string())
            .env("FORGE_PROJECT_ROOT", config_project_root())
            .env("FORGE_GAME_SCENE", "")
            .env("FORGE_GODOT_RUNTIME_DIR", &dir)
            .env("FORGE_RENDER_BACKEND", "godot")
            .env("FORGE_RENDER_METHOD", method)
            .env("FORGE_RENDER_DRIVER", driver)
            .env("FORGE_RENDER_SOURCE", source)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(
                std::fs::OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(workspace_root().join("engine-host-err.log"))
                    .map(Stdio::from)
                    .unwrap_or(Stdio::null()),
            );
        let mut child = cmd
            .spawn()
            .map_err(|e| format!("spawn Godot 宿主失败:{e}"))?;
        let stdout = child.stdout.take().ok_or("无 stdout 管道")?;
        let drain = workspace_root().join("engine-host-out.log");
        let (tx, rx) = mpsc::channel::<Option<String>>();
        std::thread::spawn(move || {
            let mut reader = BufReader::new(stdout);
            let mut line = String::new();
            let mut out: Option<std::fs::File> = None;
            loop {
                line.clear();
                match reader.read_line(&mut line) {
                    Ok(0) | Err(_) => {
                        let _ = tx.send(None);
                        return;
                    }
                    Ok(_) => {
                        if out.is_none() && tx.send(Some(line.trim().to_string())).is_ok() {
                            continue;
                        }
                        // 接收方已走(就绪之后):改为排空到日志文件。
                        if out.is_none() {
                            out = std::fs::OpenOptions::new()
                                .create(true)
                                .append(true)
                                .open(&drain)
                                .ok();
                        }
                        if let Some(f) = out.as_mut() {
                            let _ = f.write_all(line.as_bytes());
                        }
                    }
                }
            }
        });
        let deadline = Instant::now() + GODOT_START_TIMEOUT;
        loop {
            let remain = deadline.saturating_duration_since(Instant::now());
            let line = match rx.recv_timeout(remain) {
                Ok(Some(l)) => l,
                Ok(None) => {
                    let _ = child.kill();
                    return Err("Godot 宿主就绪前退出(见 engine-host-err.log)".to_string());
                }
                Err(_) => {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(format!(
                        "等 FORGE_HOST_LISTENING 超时({}s,Godot 宿主)",
                        GODOT_START_TIMEOUT.as_secs()
                    ));
                }
            };
            if line.starts_with("FORGE_HOST_LISTENING") {
                break;
            }
            if let Some(i) = line
                .strip_prefix("FORGE_GODOT_GPU_HINT index=")
                .and_then(|v| v.trim().parse::<u32>().ok())
            {
                if gpu.is_none() {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Ok(StartOutcome::GpuHint(i));
                }
            }
        }
        let stream = TcpStream::connect(("127.0.0.1", port))
            .map_err(|e| format!("连接 Godot 宿主失败:{e}"))?;
        let _ = stream.set_read_timeout(Some(CALL_TIMEOUT));
        let _ = stream.set_write_timeout(Some(CALL_TIMEOUT));
        self.stream = Some(stream);
        Ok(StartOutcome::Ready(child))
    }

    /// host 客户端:长度前缀帧一发一收;JSON-RPC error → Err(message)。
    fn call_host(&mut self, method: &str, params: Value) -> Result<Value, String> {
        let stream = self
            .stream
            .as_mut()
            .ok_or_else(|| "engine-host 未连接".to_string())?;
        stream
            .set_read_timeout(Some(response_timeout(method)))
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

#[cfg(test)]
mod tests {
    use super::*;

    fn project(tag: &str, toml: Option<&str>) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("esm-render-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        if let Some(t) = toml {
            std::fs::write(dir.join("forge.toml"), t).unwrap();
        }
        dir
    }

    fn strs(
        r: &(assetd::project::RenderConfig, &'static str),
    ) -> (
        &'static str,
        Option<&'static str>,
        Option<&'static str>,
        &'static str,
    ) {
        let (b, m, d) = r.0.as_strs();
        (b, m, d, r.1)
    }

    /// 02 §7.3:优先级 Env > ForgeToml > Default;[render] 非法 → PARSE_ERR(监督器不启动宿主)。
    #[test]
    fn render_resolution_follows_02_7_3_priority() {
        let none = |_: &str| None;
        let plain = project("plain", None);
        assert_eq!(
            strs(&resolve_render_from(&plain, &none).unwrap()),
            ("rurix", None, None, "default")
        );
        let godot = project(
            "godot",
            Some("[project]\nname = \"g\"\n\n[render]\nbackend = \"godot\"\nmethod = \"mobile\"\n"),
        );
        assert_eq!(
            strs(&resolve_render_from(&godot, &none).unwrap()),
            ("godot", Some("mobile"), Some("d3d12"), "forge.toml")
        );
        let env_vulkan = |k: &str| (k == "FORGE_RENDER_DRIVER").then(|| "vulkan".to_string());
        assert_eq!(
            strs(&resolve_render_from(&godot, &env_vulkan).unwrap()),
            ("godot", Some("mobile"), Some("vulkan"), "env")
        );
        let env_rurix = |k: &str| (k == "FORGE_RENDER_BACKEND").then(|| "rurix".to_string());
        assert_eq!(
            strs(&resolve_render_from(&godot, &env_rurix).unwrap()),
            ("rurix", None, None, "env")
        );
        let env_compat = |k: &str| match k {
            "FORGE_RENDER_BACKEND" => Some("godot".to_string()),
            "FORGE_RENDER_METHOD" => Some("gl_compatibility".to_string()),
            _ => None,
        };
        assert_eq!(
            strs(&resolve_render_from(&plain, &env_compat).unwrap()),
            ("godot", Some("gl_compatibility"), Some("opengl3"), "env")
        );
        let bad = project("bad", Some("[project]\n\n[render]\nbackend = \"godot\"\nmethod = \"gl_compatibility\"\ndriver = \"d3d12\"\n"));
        let e = resolve_render_from(&bad, &none).unwrap_err();
        assert!(
            e.starts_with("PARSE_ERR") && e.contains("forge.toml [render]"),
            "{e}"
        );
        let bad_env = |k: &str| (k == "FORGE_RENDER_BACKEND").then(|| "unity".to_string());
        assert!(resolve_render_from(&plain, &bad_env)
            .unwrap_err()
            .contains("forge.toml [render]"));
        for d in [plain, godot, bad] {
            let _ = std::fs::remove_dir_all(d);
        }
    }
}
