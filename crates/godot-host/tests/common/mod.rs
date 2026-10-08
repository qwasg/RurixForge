//! godot-host 集成测试的公共部分:生成测试专用运行时目录、拉起 Godot 宿主、JSON-RPC / WS 客户端。
//! 测试之间用一把全局锁串行(每个用例一个 Godot 进程,避免并发抢 GPU / 端口)。
#![allow(dead_code)]

use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc;
use std::sync::{Arc, Mutex, MutexGuard, OnceLock};
use std::time::{Duration, Instant};

use serde_json::{json, Value};

pub fn build_target() -> PathBuf {
    std::env::var_os("CARGO_TARGET_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| repo_root().join("target"))
}

pub fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .unwrap()
        .to_path_buf()
}

/// 串行锁(poison 也继续,前一个用例失败不连累后面的)。
pub fn serial() -> MutexGuard<'static, ()> {
    static L: Mutex<()> = Mutex::new(());
    L.lock().unwrap_or_else(|e| e.into_inner())
}

/// 用 scripts/godot-runtime.ps1 生成测试专用运行时目录(target/godot-runtime-test)。
/// cdylib 不会被集成测试链接,`cargo test` 不保证重编 dll:这里先确认 target/debug/godot_host.dll 不比源码旧,
/// 旧了直接失败并提示先 `cargo build -p godot-host`(避免拿旧 dll 测出假结果)。
pub fn runtime() -> &'static Path {
    static DIR: OnceLock<PathBuf> = OnceLock::new();
    DIR.get_or_init(|| {
        let root = repo_root();
        let dll = build_target().join("debug").join("godot_host.dll");
        let built = std::fs::metadata(&dll)
            .and_then(|m| m.modified())
            .expect("缺 target/debug/godot_host.dll:先 cargo build -p godot-host");
        let newest_src = ["crates/godot-host/src", "crates/engine-host/src"]
            .iter()
            .flat_map(|d| walk(&root.join(d)))
            .filter_map(|p| std::fs::metadata(p).and_then(|m| m.modified()).ok())
            .max()
            .unwrap();
        assert!(
            built >= newest_src,
            "godot_host.dll 比源码旧:先 cargo build -p godot-host 再跑测试"
        );
        let out = std::env::var_os("FORGE_TEST_GODOT_RUNTIME")
            .map(PathBuf::from)
            .unwrap_or_else(|| build_target().join("godot-runtime-test"));
        let shell = if Command::new("pwsh")
            .args(["-NoProfile", "-Command", "exit 0"])
            .status()
            .is_ok()
        {
            "pwsh"
        } else {
            "powershell"
        };
        let st = Command::new(shell)
            .args(["-NoProfile", "-ExecutionPolicy", "Bypass", "-File"])
            .arg(root.join("scripts").join("godot-runtime.ps1"))
            .arg("-Out")
            .arg(&out)
            .status()
            .expect("跑 godot-runtime.ps1");
        assert!(st.success(), "godot-runtime.ps1 失败");
        out
    })
}

fn walk(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    for e in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        let p = e.path();
        if p.is_dir() {
            out.extend(walk(&p));
        } else {
            out.push(p);
        }
    }
    out
}

pub struct Godot {
    pub child: Child,
    pub port: u16,
    /// 就绪行之前与之后的全部 stdout 行(读行线程持续排空,02 §6.2)。
    pub stdout: Arc<Mutex<Vec<String>>>,
    pub ready_ms: u128,
}

impl Godot {
    /// 拉起 forge-godot_console.exe;等 FORGE_HOST_LISTENING(逐行扫前缀,40 s 上限)。
    pub fn start(method: &str, driver: &str, extra: &[&str], env: &[(&str, &str)]) -> Godot {
        let dir = runtime();
        let mut cmd = Command::new(dir.join("forge-godot_console.exe"));
        cmd.current_dir(dir)
            .args([
                "--rendering-method",
                method,
                "--rendering-driver",
                driver,
                "--audio-driver",
                "Dummy",
            ])
            .args(extra)
            .env("FORGE_HOST_PORT", "0")
            .env(
                "FORGE_PROJECT_ROOT",
                repo_root().join("projects").join("demo"),
            )
            .env("FORGE_RENDER_BACKEND", "godot")
            .env("FORGE_RENDER_METHOD", method)
            .env("FORGE_RENDER_DRIVER", driver)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        for (k, v) in env {
            cmd.env(k, v);
        }
        let t0 = Instant::now();
        let mut child = cmd.spawn().expect("spawn forge-godot_console.exe");
        let stdout = Arc::new(Mutex::new(Vec::new()));
        let (tx, rx) = mpsc::channel();
        let out = child.stdout.take().unwrap();
        let sink = Arc::clone(&stdout);
        std::thread::spawn(move || {
            for line in BufReader::new(out).lines().map_while(Result::ok) {
                if let Some(p) = line.strip_prefix("FORGE_HOST_LISTENING port=") {
                    let _ = tx.send(p.trim().parse::<u16>().ok());
                }
                sink.lock().unwrap().push(line);
            }
            let _ = tx.send(None);
        });
        let err = child.stderr.take().unwrap();
        let esink = Arc::clone(&stdout);
        std::thread::spawn(move || {
            for line in BufReader::new(err).lines().map_while(Result::ok) {
                esink.lock().unwrap().push(format!("[stderr] {line}"));
            }
        });
        let port = rx.recv_timeout(Duration::from_secs(40)).ok().flatten();
        let Some(port) = port else {
            let _ = child.kill();
            panic!("40 s 内没有就绪行:{:?}", stdout.lock().unwrap());
        };
        Godot {
            child,
            port,
            stdout,
            ready_ms: t0.elapsed().as_millis(),
        }
    }

    pub fn rpc(&self) -> Rpc {
        Rpc::connect(self.port)
    }

    /// 与监督器同一策略:Godot 选的卡与消费端缺省 adapter 不同时,它在就绪行之前打印
    /// `FORGE_GODOT_GPU_HINT index=i`;这里照提示带 --gpu-index 重启一次(L1 需要同卡)。
    pub fn start_aligned(method: &str, driver: &str, extra: &[&str]) -> Godot {
        let g = Godot::start(method, driver, extra, &[]);
        let hint = g.log().iter().find_map(|l| {
            l.strip_prefix("FORGE_GODOT_GPU_HINT index=")
                .map(|v| v.trim().to_string())
        });
        let Some(i) = hint else { return g };
        drop(g);
        let mut args: Vec<&str> = extra.to_vec();
        args.extend(["--gpu-index", i.as_str()]);
        Godot::start(method, driver, &args, &[])
    }

    pub fn log(&self) -> Vec<String> {
        self.stdout.lock().unwrap().clone()
    }
}

impl Drop for Godot {
    fn drop(&mut self) {
        // 杀 console 版 = 关它的 Job(KILL_ON_JOB_CLOSE),主 exe 一起退出。
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

pub struct Rpc(TcpStream);

impl Rpc {
    pub fn connect(port: u16) -> Rpc {
        let s = TcpStream::connect(("127.0.0.1", port)).expect("连 RPC");
        s.set_read_timeout(Some(Duration::from_secs(30))).unwrap();
        Rpc(s)
    }

    pub fn raw(&mut self, method: &str, params: Value) -> Value {
        let body = serde_json::to_vec(
            &json!({ "jsonrpc": "2.0", "id": 1, "method": method, "params": params }),
        )
        .unwrap();
        self.0
            .write_all(&(body.len() as u32).to_le_bytes())
            .unwrap();
        self.0.write_all(&body).unwrap();
        let mut len = [0u8; 4];
        self.0.read_exact(&mut len).unwrap();
        let mut buf = vec![0u8; u32::from_le_bytes(len) as usize];
        self.0.read_exact(&mut buf).unwrap();
        serde_json::from_slice(&buf).unwrap()
    }

    pub fn call(&mut self, method: &str, params: Value) -> Value {
        let v = self.raw(method, params);
        assert!(v.get("error").is_none(), "{method} 不应报错:{v}");
        v["result"].clone()
    }

    /// viewport.frame rgba8 → (结果 JSON, 像素)。
    pub fn frame(&mut self, w: u32, h: u32) -> (Value, Vec<u8>) {
        let r = self.call("viewport.frame", json!({ "width": w, "height": h }));
        let px = base64::Engine::decode(
            &base64::engine::general_purpose::STANDARD,
            r["pixelsB64"].as_str().unwrap(),
        )
        .unwrap();
        assert_eq!(px.len(), (w * h * 4) as usize);
        (r, px)
    }
}

pub fn sha256(b: &[u8]) -> String {
    use sha2::Digest;
    sha2::Sha256::digest(b)
        .iter()
        .map(|x| format!("{x:02x}"))
        .collect()
}

/// rurix 参照宿主(target/debug/engine-host.exe,同一份 demo 项目)。
pub struct Rurix {
    pub child: Child,
    pub port: u16,
}

impl Rurix {
    pub fn start() -> Rurix {
        let exe = build_target().join("debug").join("engine-host.exe");
        assert!(
            exe.exists(),
            "缺 {}:先 cargo build -p engine-host",
            exe.display()
        );
        let mut child = Command::new(exe)
            .args(["--port", "0"])
            .env(
                "FORGE_PROJECT_ROOT",
                repo_root().join("projects").join("demo"),
            )
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let mut line = String::new();
        BufReader::new(child.stdout.take().unwrap())
            .read_line(&mut line)
            .unwrap();
        let port = line
            .trim()
            .strip_prefix("FORGE_HOST_LISTENING port=")
            .expect("rurix 就绪行")
            .parse()
            .unwrap();
        Rurix { child, port }
    }
}

impl Drop for Rurix {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// 逐像素比较:(不同的像素数, 最大通道差)。
pub fn diff(a: &[u8], b: &[u8]) -> (usize, u8) {
    let mut n = 0;
    let mut max = 0u8;
    for (p, q) in a.chunks_exact(4).zip(b.chunks_exact(4)) {
        let d = (0..3).map(|i| p[i].abs_diff(q[i])).max().unwrap();
        if d > 0 {
            n += 1;
        }
        max = max.max(d);
    }
    (n, max)
}

/// 消费端读共享 buffer(与 presenter 同一路:缺省 adapter 的 device、OpenSharedHandle、等共享 fence)。
#[cfg(windows)]
pub mod share {
    use windows::core::Interface;
    use windows::Win32::Foundation::HANDLE;
    use windows::Win32::Graphics::Direct3D::D3D_FEATURE_LEVEL_11_0;
    use windows::Win32::Graphics::Direct3D12::*;
    use windows::Win32::Graphics::Dxgi::Common::*;

    pub struct Reader {
        device: ID3D12Device,
        queue: ID3D12CommandQueue,
        alloc: ID3D12CommandAllocator,
        list: ID3D12GraphicsCommandList,
        pub buffer: ID3D12Resource,
        pub fence: ID3D12Fence,
        own: ID3D12Fence,
        own_v: u64,
        size: u64,
    }

    impl Reader {
        pub fn open(buf_handle: u64, fence_handle: u64, size: u64) -> Reader {
            unsafe {
                let mut device: Option<ID3D12Device> = None;
                D3D12CreateDevice(None, D3D_FEATURE_LEVEL_11_0, &mut device).unwrap();
                let device = device.unwrap();
                let luid = device.GetAdapterLuid();
                let who = {
                    use windows::Win32::Graphics::Dxgi::{CreateDXGIFactory1, IDXGIFactory4};
                    let f: IDXGIFactory4 = CreateDXGIFactory1().unwrap();
                    let a: windows::Win32::Graphics::Dxgi::IDXGIAdapter1 =
                        f.EnumAdapterByLuid(luid).unwrap();
                    let d = a.GetDesc1().unwrap();
                    let n = d.Description.iter().position(|c| *c == 0).unwrap_or(128);
                    format!(
                        "{} luid={:08x}:{:08x}",
                        String::from_utf16_lossy(&d.Description[..n]),
                        luid.HighPart,
                        luid.LowPart
                    )
                };
                eprintln!("test consumer device(D3D12CreateDevice(None))= {who}");
                let mut buffer: Option<ID3D12Resource> = None;
                device
                    .OpenSharedHandle(HANDLE(buf_handle as *mut _), &mut buffer)
                    .unwrap_or_else(|e| panic!("OpenSharedHandle(buffer) on {who}: {e}"));
                let mut fence: Option<ID3D12Fence> = None;
                device
                    .OpenSharedHandle(HANDLE(fence_handle as *mut _), &mut fence)
                    .expect("OpenSharedHandle(fence)");
                let queue: ID3D12CommandQueue = device
                    .CreateCommandQueue(&D3D12_COMMAND_QUEUE_DESC {
                        Type: D3D12_COMMAND_LIST_TYPE_DIRECT,
                        ..Default::default()
                    })
                    .unwrap();
                let alloc: ID3D12CommandAllocator = device
                    .CreateCommandAllocator(D3D12_COMMAND_LIST_TYPE_DIRECT)
                    .unwrap();
                let list: ID3D12GraphicsCommandList = device
                    .CreateCommandList(0, D3D12_COMMAND_LIST_TYPE_DIRECT, &alloc, None)
                    .unwrap();
                list.Close().unwrap();
                let own: ID3D12Fence = device.CreateFence(0, D3D12_FENCE_FLAG_NONE).unwrap();
                Reader {
                    device,
                    queue,
                    alloc,
                    list,
                    buffer: buffer.unwrap(),
                    fence: fence.unwrap(),
                    own,
                    own_v: 0,
                    size,
                }
            }
        }

        pub fn fence_value(&self) -> u64 {
            unsafe { self.fence.GetCompletedValue() }
        }

        /// 等共享 fence ≥ v 之后把整个共享 buffer 拷回 CPU。
        pub fn read(&mut self, v: u64) -> Vec<u8> {
            unsafe {
                let heap = D3D12_HEAP_PROPERTIES {
                    Type: D3D12_HEAP_TYPE_READBACK,
                    ..Default::default()
                };
                let desc = D3D12_RESOURCE_DESC {
                    Dimension: D3D12_RESOURCE_DIMENSION_BUFFER,
                    Width: self.size,
                    Height: 1,
                    DepthOrArraySize: 1,
                    MipLevels: 1,
                    Format: DXGI_FORMAT_UNKNOWN,
                    SampleDesc: DXGI_SAMPLE_DESC {
                        Count: 1,
                        Quality: 0,
                    },
                    Layout: D3D12_TEXTURE_LAYOUT_ROW_MAJOR,
                    ..Default::default()
                };
                let mut rb: Option<ID3D12Resource> = None;
                self.device
                    .CreateCommittedResource(
                        &heap,
                        D3D12_HEAP_FLAG_NONE,
                        &desc,
                        D3D12_RESOURCE_STATE_COPY_DEST,
                        None,
                        &mut rb,
                    )
                    .unwrap();
                let rb = rb.unwrap();
                self.alloc.Reset().unwrap();
                self.list.Reset(&self.alloc, None).unwrap();
                self.list
                    .CopyBufferRegion(&rb, 0, &self.buffer, 0, self.size);
                self.list.Close().unwrap();
                self.queue.Wait(&self.fence, v).unwrap();
                self.queue
                    .ExecuteCommandLists(&[Some(self.list.cast().unwrap())]);
                self.own_v += 1;
                self.queue.Signal(&self.own, self.own_v).unwrap();
                let t0 = std::time::Instant::now();
                while self.own.GetCompletedValue() < self.own_v {
                    assert!(t0.elapsed().as_secs() < 5, "等共享 fence {v} 超时");
                    std::thread::sleep(std::time::Duration::from_millis(2));
                }
                let mut p = std::ptr::null_mut();
                rb.Map(0, None, Some(&mut p)).unwrap();
                let out = std::slice::from_raw_parts(p as *const u8, self.size as usize).to_vec();
                rb.Unmap(0, None);
                out
            }
        }
    }
}
