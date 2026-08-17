//! viewport-presenter — D3D12 共享纹理消费者(F1 wave.2 G-F1-9)。
//!
//! 职责:打开 engine-host 生产的共享纹理/fence,在(父)窗口的 D3D12 swapchain 上呈现帧。
//! 模式:--hwnd <parent> 子窗口嵌入(Electron 视口区);--selftest 独立窗口 + 自动校验。
//!
//! 线程模型:专用窗口线程持有 swapchain 并阻塞跑消息泵;主线程读 stdin 命令
//! (bind/move/close),经 channel 递交给窗口线程做 D3D12 操作(同线程纪律)。
//!
//! 帧同步:fence 值即帧计数;--selftest 中引擎按序渲 n 帧,消费者逐值等待后呈现,
//! 计数器即为序列断言(fence≥i+1 ⇒ 第 i 帧已被呈现),无需文件回读。

#![cfg(windows)]

use std::ffi::c_void;
use std::io::{BufRead, Write};
use std::process::exit;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::mpsc;
use std::thread;

use windows::core::{Interface, PCWSTR};
use windows::Win32::Foundation::{CloseHandle, HANDLE, HINSTANCE, HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows::Win32::Graphics::Direct3D::D3D_FEATURE_LEVEL_11_0;
use windows::Win32::Graphics::Direct3D12::*;
use windows::Win32::Graphics::Dxgi::Common::*;
use windows::Win32::Graphics::Dxgi::*;
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Threading::CreateEventW;
use windows::Win32::UI::HiDpi::{SetProcessDpiAwarenessContext, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2};
use windows::Win32::UI::WindowsAndMessaging::*;

const WINDOW_CLASS: &str = "ForgeViewportPresenter";
/// 呈现帧率目标(就近 vsync 近似;消息泵空闲期 peek 间隔)。
const PRESENT_TICK_MS: u32 = 8;

// ─────────────────────────── D3D12 消费者状态 ───────────────────────────

struct Dx {
    _factory: IDXGIFactory2,
    device: ID3D12Device,
    queue: ID3D12CommandQueue,
    allocator: ID3D12CommandAllocator,
    list: ID3D12GraphicsCommandList,
    swapchain: Option<IDXGISwapChain3>,
    rtv_heap: ID3D12DescriptorHeap,
    rtv_size: u32,
    shared: Option<ID3D12Resource>,
    fence: Option<ID3D12Fence>,
    fence_event: HANDLE,
    seen_fence: u64,
    width: u32,
    height: u32,
    presented: u64,
}

impl Dx {
    fn new() -> Result<Self, String> {
        unsafe {
            // SAFETY: 标准 DXGI/D3D12 初始化;全部参数有效,错误经 HRESULT 传播。
            let factory: IDXGIFactory2 = CreateDXGIFactory2(DXGI_CREATE_FACTORY_FLAGS(0))
                .map_err(|e| format!("CreateDXGIFactory2: {e}"))?;
            let mut device: Option<ID3D12Device> = None;
            D3D12CreateDevice(None, D3D_FEATURE_LEVEL_11_0, &mut device)
                .map_err(|e| format!("D3D12CreateDevice: {e}"))?;
            let device = device.ok_or("D3D12CreateDevice 返回空")?;
            let queue = device
                .CreateCommandQueue(&D3D12_COMMAND_QUEUE_DESC {
                    Type: D3D12_COMMAND_LIST_TYPE_DIRECT,
                    ..Default::default()
                })
                .map_err(|e| format!("CreateCommandQueue: {e}"))?;
            let allocator = device
                .CreateCommandAllocator(D3D12_COMMAND_LIST_TYPE_DIRECT)
                .map_err(|e| format!("CreateCommandAllocator: {e}"))?;
            let list: ID3D12GraphicsCommandList = device
                .CreateCommandList(0, D3D12_COMMAND_LIST_TYPE_DIRECT, &allocator, None)
                .map_err(|e| format!("CreateCommandList: {e}"))?;
            let _ = list.Close();
            let rtv_heap = device
                .CreateDescriptorHeap(&D3D12_DESCRIPTOR_HEAP_DESC {
                    Type: D3D12_DESCRIPTOR_HEAP_TYPE_RTV,
                    NumDescriptors: 2,
                    ..Default::default()
                })
                .map_err(|e| format!("CreateDescriptorHeap(RTV): {e}"))?;
            let rtv_size = device.GetDescriptorHandleIncrementSize(D3D12_DESCRIPTOR_HEAP_TYPE_RTV);
            let fence_event = CreateEventW(None, false, false, None).map_err(|e| format!("CreateEventW: {e}"))?;
            Ok(Dx {
                _factory: factory,
                device,
                queue,
                allocator,
                list,
                swapchain: None,
                rtv_heap,
                rtv_size,
                shared: None,
                fence: None,
                fence_event,
                seen_fence: 0,
                width: 0,
                height: 0,
                presented: 0,
            })
        }
    }

    /// 绑定共享纹理 + fence(各仅一次;重复 bind 先释放旧引用)。
    fn bind(&mut self, tex_raw: usize, fence_raw: usize, width: u32, height: u32) -> Result<(), String> {
        unsafe {
            // SAFETY: tex_raw/fence_raw 为 engine-host 经 DuplicateHandle 复制给本进程的
            // 有效句柄;OpenSharedHandle 成功即取得 COM 引用,失败原样返回 HRESULT。
            let tex_h = HANDLE(tex_raw as *mut c_void);
            let fence_h = HANDLE(fence_raw as *mut c_void);
            let mut shared: Option<ID3D12Resource> = None;
            self.device
                .OpenSharedHandle(tex_h, &mut shared)
                .map_err(|e| format!("OpenSharedHandle(texture): {e}"))?;
            let shared = shared.ok_or("OpenSharedHandle(texture) 返回空")?;
            let mut fence: Option<ID3D12Fence> = None;
            self.device
                .OpenSharedHandle(fence_h, &mut fence)
                .map_err(|e| format!("OpenSharedHandle(fence): {e}"))?;
            let fence = fence.ok_or("OpenSharedHandle(fence) 返回空")?;
            let _ = CloseHandle(tex_h);
            let _ = CloseHandle(fence_h);
            self.shared = Some(shared);
            self.fence = Some(fence);
            self.width = width;
            self.height = height;
            Ok(())
        }
    }

    /// 建 swapchain 于指定 hwnd(尺寸跟随当前绑定纹理)。
    fn create_swapchain(&mut self, hwnd: HWND) -> Result<(), String> {
        unsafe {
            // SAFETY: hwnd 为窗口线程持有的有效窗口;queue 存活;desc 常量字段合法。
            let desc = DXGI_SWAP_CHAIN_DESC1 {
                Width: self.width.max(16),
                Height: self.height.max(16),
                Format: DXGI_FORMAT_R8G8B8A8_UNORM,
                SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
                BufferUsage: DXGI_USAGE_RENDER_TARGET_OUTPUT,
                BufferCount: 2,
                SwapEffect: DXGI_SWAP_EFFECT_FLIP_SEQUENTIAL,
                Scaling: DXGI_SCALING_NONE,
                ..Default::default()
            };
            let sc = self
                ._factory
                .CreateSwapChainForHwnd(&self.queue, hwnd, &desc, None, None)
                .map_err(|e| format!("CreateSwapChainForHwnd: {e}"))?;
            let sc3: IDXGISwapChain3 = sc.cast().map_err(|e| format!("swapchain cast: {e}"))?;
            // RTV 双缓冲
            let base = self.rtv_heap.GetCPUDescriptorHandleForHeapStart();
            for i in 0..2u32 {
                let buf: ID3D12Resource = sc3.GetBuffer(i).map_err(|e| format!("GetBuffer({i}): {e}"))?;
                let h = D3D12_CPU_DESCRIPTOR_HANDLE {
                    ptr: base.ptr + (i * self.rtv_size) as usize,
                };
                self.device.CreateRenderTargetView(&buf, None, h);
            }
            self.swapchain = Some(sc3);
            Ok(())
        }
    }

    /// 呈现一帧:fence 等序 → copy → Present;返回本帧 fence 值(无新帧返回 None)。
    fn present_latest(&mut self) -> Result<Option<u64>, String> {
        let (Some(shared), Some(fence), Some(sc)) = (&self.shared, &self.fence, &self.swapchain) else {
            return Ok(None);
        };
        unsafe {
            // SAFETY: 全部对象存活于 self;命令对(reset/record/close/execute)按序配对。
            let current = fence.GetCompletedValue();
            if current <= self.seen_fence {
                return Ok(None); // 无新帧不重复呈现:计数器 = 已呈现新帧数(selftest 判定面)
            }
            let target = current;
            self.allocator.Reset().map_err(|e| format!("allocator.Reset: {e}"))?;
            self.list.Reset(&self.allocator, None).map_err(|e| format!("list.Reset: {e}"))?;

            let idx = sc.GetCurrentBackBufferIndex();
            let back: ID3D12Resource = sc.GetBuffer(idx).map_err(|e| format!("GetBuffer(cur): {e}"))?;
            // PRESENT → COPY_DEST
            let to_copy = D3D12_RESOURCE_BARRIER {
                Type: D3D12_RESOURCE_BARRIER_TYPE_TRANSITION,
                Anonymous: D3D12_RESOURCE_BARRIER_0 {
                    Transition: std::mem::ManuallyDrop::new(D3D12_RESOURCE_TRANSITION_BARRIER {
                        pResource: std::mem::ManuallyDrop::new(Some(back.clone())),
                        Subresource: D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES,
                        StateBefore: D3D12_RESOURCE_STATE_PRESENT,
                        StateAfter: D3D12_RESOURCE_STATE_COPY_DEST,
                    }),
                },
                ..Default::default()
            };
            self.list.ResourceBarrier(&[to_copy]);
            self.list.CopyResource(&back, shared);
            let to_present = D3D12_RESOURCE_BARRIER {
                Type: D3D12_RESOURCE_BARRIER_TYPE_TRANSITION,
                Anonymous: D3D12_RESOURCE_BARRIER_0 {
                    Transition: std::mem::ManuallyDrop::new(D3D12_RESOURCE_TRANSITION_BARRIER {
                        pResource: std::mem::ManuallyDrop::new(Some(back.clone())),
                        Subresource: D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES,
                        StateBefore: D3D12_RESOURCE_STATE_COPY_DEST,
                        StateAfter: D3D12_RESOURCE_STATE_PRESENT,
                    }),
                },
                ..Default::default()
            };
            self.list.ResourceBarrier(&[to_present]);
            self.list.Close().map_err(|e| format!("list.Close: {e}"))?;
            let cmd: ID3D12CommandList = self.list.cast().map_err(|e| format!("list cast: {e}"))?;
            self.queue.ExecuteCommandLists(&[Some(cmd)]);
            sc.Present(1, DXGI_PRESENT(0)).ok().map_err(|e| format!("Present: {e}"))?;
            // 记录已见帧号;不强制等 GPU(flip 队列自身节流)。
            self.seen_fence = target;
            self.presented += 1;
            Ok(Some(target))
        }
    }
}

impl Drop for Dx {
    fn drop(&mut self) {
        unsafe {
            // SAFETY: 进程退出前关闭事件句柄;COM 对象由 RAII 释放。
            let _ = CloseHandle(self.fence_event);
        }
    }
}

// ─────────────────────────── 窗口线程 ───────────────────────────

enum Cmd {
    Bind { tex: usize, fence: usize, w: u32, h: u32 },
    Move { x: i32, y: i32, w: u32, h: u32 },
    Close,
}

struct SharedPresented(AtomicU64);

static mut PRESENTED_PTR: *const SharedPresented = std::ptr::null();

/// 窗口线程创建后回写 hwnd(物理屏幕矩形查询用);0 = 未就绪。
static HWND_SLOT: AtomicUsize = AtomicUsize::new(0);

fn presented_counter() -> &'static SharedPresented {
    // SAFETY: PRESENTED_PTR 在窗口线程创建前写入一次,之后只读(见 main)。
    unsafe { &*PRESENTED_PTR }
}

extern "system" fn wnd_proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    unsafe {
        // SAFETY: 标准 DefWindowProcW 回退;无用户数据。
        DefWindowProcW(hwnd, msg, wparam, lparam)
    }
}

/// 窗口线程入口:建窗口 + Dx + swapchain,跑消息泵 + 呈现循环。
fn window_thread(parent: HWND, x: i32, y: i32, w: u32, h: u32, rx: mpsc::Receiver<Cmd>, ready: mpsc::Sender<Result<(), String>>) {
    let mut class_name: Vec<u16> = WINDOW_CLASS.encode_utf16().chain(std::iter::once(0)).collect();
    let hinst: HINSTANCE = unsafe {
        // SAFETY: 当前进程模块句柄恒有效。
        match GetModuleHandleW(None) {
            Ok(m) => HINSTANCE(m.0),
            Err(_) => HINSTANCE(std::ptr::null_mut()),
        }
    };
    let wc = WNDCLASSW {
        lpfnWndProc: Some(wnd_proc),
        hInstance: hinst,
        lpszClassName: PCWSTR(class_name.as_ptr()),
        ..Default::default()
    };
    unsafe {
        // SAFETY: 重复注册同名类失败可忽略(幂等)。
        let _ = RegisterClassW(&wc);
    }
    let title: Vec<u16> = "Forge Viewport\0".encode_utf16().collect();
    let (style, ex) = if parent.0.is_null() {
        (WS_OVERLAPPEDWINDOW | WS_VISIBLE, WS_EX_APPWINDOW)
    } else {
        // 子窗口嵌入 Electron 视口区:TRANSPARENT = 命中测试穿透到父窗 web 内容
        // (点选/环绕/滚轮交互不被原生层吞掉);NOACTIVATE = 永不抢键盘焦点。
        (WS_CHILD | WS_VISIBLE, WS_EX_TRANSPARENT | WS_EX_NOACTIVATE)
    };
    let hwnd = unsafe {
        // SAFETY: class 已注册;parent 为调用方持有的有效顶层窗口(或 null 独立窗口)。
        match CreateWindowExW(
            ex,
            PCWSTR(class_name.as_mut_ptr()),
            PCWSTR(title.as_ptr()),
            style,
            x,
            y,
            w as i32,
            h as i32,
            if parent.0.is_null() { None } else { Some(parent) },
            None,
            Some(hinst),
            None,
        ) {
            Ok(h) if !h.0.is_null() => h,
            Ok(_) => {
                let _ = ready.send(Err("CreateWindowExW 返回空窗口".to_string()));
                return;
            }
            Err(e) => {
                let _ = ready.send(Err(format!("CreateWindowExW: {e}")));
                return;
            }
        }
    };
    HWND_SLOT.store(hwnd.0 as usize, Ordering::SeqCst);
    let mut dx = match Dx::new() {
        Ok(d) => d,
        Err(e) => {
            let _ = ready.send(Err(format!("Dx 初始化失败: {e}")));
            return;
        }
    };
    if let Err(e) = dx.create_swapchain(hwnd) {
        let _ = ready.send(Err(e));
        return;
    }
    let _ = ready.send(Ok(()));

    let mut msg = MSG::default();
    'outer: loop {
        // 命令队列优先
        while let Ok(cmd) = rx.try_recv() {
            match cmd {
                Cmd::Bind { tex, fence, w, h } => {
                    let recreate = dx.width != w || dx.height != h || dx.swapchain.is_none();
                    if let Err(e) = dx.bind(tex, fence, w, h) {
                        eprintln!("[presenter] bind 失败: {e}");
                        break 'outer;
                    }
                    if recreate {
                        // 尺寸变化:重建 swapchain(旧 RTV 引用随 swapchain 释放)
                        dx.swapchain = None;
                        if let Err(e) = dx.create_swapchain(hwnd) {
                            eprintln!("[presenter] swapchain 重建失败: {e}");
                            break 'outer;
                        }
                    }
                }
                Cmd::Move { x, y, w, h } => unsafe {
                    // SAFETY: hwnd 为本线程窗口。
                    let _ = SetWindowPos(hwnd, None, x, y, w as i32, h as i32, SWP_NOZORDER | SWP_NOACTIVATE);
                },
                Cmd::Close => break 'outer,
            }
        }
        // 消息泵(peek 不阻塞)
        unsafe {
            while PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE).as_bool() {
                if msg.message == WM_QUIT {
                    break 'outer;
                }
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        }
        if dx.shared.is_some() {
            match dx.present_latest() {
                Ok(Some(_)) => {
                    presented_counter().0.fetch_add(1, Ordering::SeqCst);
                }
                Ok(None) => {}
                Err(e) => {
                    eprintln!("[presenter] present 失败: {e}");
                    break 'outer;
                }
            }
        }
        thread::sleep(std::time::Duration::from_millis(PRESENT_TICK_MS as u64));
    }
    unsafe {
        // SAFETY: 退出前销毁窗口,父窗口无悬挂子引用。
        let _ = DestroyWindow(hwnd);
    }
    HWND_SLOT.store(0, Ordering::SeqCst);
}

// ─────────────────────────── stdin 命令协议 ───────────────────────────

/// 逐行读取:`bind <tex> <fence> <w> <h>` / `move <x> <y> <w> <h>` / `close`。
/// 容忍 BOM(PowerShell 5.1 重定向 stdin 默认 UTF-8 带 BOM)。
fn parse_cmd(line: &str) -> Option<Cmd> {
    let line = line.trim().trim_start_matches('\u{feff}');
    let parts: Vec<&str> = line.split_whitespace().collect();
    match parts.first().copied()? {
        "bind" if parts.len() == 5 => Some(Cmd::Bind {
            tex: parts[1].parse().ok()?,
            fence: parts[2].parse().ok()?,
            w: parts[3].parse().ok()?,
            h: parts[4].parse().ok()?,
        }),
        "move" if parts.len() == 5 => Some(Cmd::Move {
            x: parts[1].parse().ok()?,
            y: parts[2].parse().ok()?,
            w: parts[3].parse().ok()?,
            h: parts[4].parse().ok()?,
        }),
        "close" => Some(Cmd::Close),
        _ => None,
    }
}

/// `stat` 即时查询(主线程直答,不进窗口线程):呈现计数 + 窗口屏幕物理矩形。
fn print_stat() {
    let presented = presented_counter().0.load(Ordering::SeqCst);
    let raw = HWND_SLOT.load(Ordering::SeqCst);
    let mut rect = RECT::default();
    let ok = raw != 0 && unsafe {
        // SAFETY: hwnd 由窗口线程持有,存活期 GetWindowRect 合法。
        GetWindowRect(HWND(raw as *mut c_void), &mut rect).is_ok()
    };
    if ok {
        println!(
            "STAT presented={} rect={},{},{},{}",
            presented,
            rect.left,
            rect.top,
            rect.right - rect.left,
            rect.bottom - rect.top
        );
    } else {
        println!("STAT presented={presented} rect=none");
    }
    std::io::stdout().flush().ok();
}

fn usage() -> ! {
    eprintln!("usage: viewport-presenter --hwnd <parent_dec> | --selftest --tex <h> --fence <h> --w <n> --h <n> [--expect-frames <n>]");
    exit(2);
}

fn main() {
    // DPI 感知:宿主(Electron)为 PerMonitorV2,本进程坐标不得被系统虚拟化,
    // 否则子窗口位置/尺寸与 web 侧 devicePixelRatio 换算错位。
    unsafe {
        // SAFETY: 进程启动早期单次调用;失败(旧系统)回退默认,坐标由调用方 DPR=1 语义兜底。
        let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
    }
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 2 {
        usage();
    }
    let counter = Box::leak(Box::new(SharedPresented(AtomicU64::new(0))));
    // SAFETY: 单线程启动期一次性写入,其后只读。
    unsafe { PRESENTED_PTR = counter };

    if args[1] == "--selftest" {
        // --selftest --w <n> --h <n> [--expect-frames <n>]
        // 句柄经 stdin 首行 `bind <tex> <fence> <w> <h>` 到达(调用方先 shareOpen 拿句柄)。
        let (mut w, mut h) = (0u32, 0u32);
        let mut expect = 3u64;
        let mut i = 2;
        while i < args.len() {
            match args[i].as_str() {
                "--w" => { w = args[i + 1].parse().unwrap_or(0); i += 2; }
                "--h" => { h = args[i + 1].parse().unwrap_or(0); i += 2; }
                "--expect-frames" => { expect = args[i + 1].parse().unwrap_or(3); i += 2; }
                _ => usage(),
            }
        }
        if w == 0 || h == 0 {
            usage();
        }
        let (tx, rx) = mpsc::channel();
        let (ready_tx, ready_rx) = mpsc::channel();
        let ht = thread::spawn(move || window_thread(HWND(std::ptr::null_mut()), 64, 64, w, h, rx, ready_tx));
        match ready_rx.recv() {
            Ok(Ok(())) => {}
            Ok(Err(e)) => {
                eprintln!("[selftest] FAIL 初始化: {e}");
                exit(1);
            }
            Err(e) => {
                eprintln!("[selftest] FAIL 窗口线程早退: {e}");
                exit(1);
            }
        }
        println!("[selftest] window ready,等待 stdin bind");
        std::io::stdout().flush().ok();
        // stdin 首行必须 bind(有界 10s 由调用方超时兜底)。
        let stdin = std::io::stdin();
        let mut line = String::new();
        if stdin.lock().read_line(&mut line).is_err() || line.trim().is_empty() {
            eprintln!("[selftest] FAIL 未收到 bind 行");
            exit(1);
        }
        match parse_cmd(line.trim()) {
            Some(cmd @ Cmd::Bind { .. }) => tx.send(cmd).expect("send bind"),
            _ => {
                eprintln!("[selftest] FAIL 首行非 bind: {}", line.trim());
                exit(1);
            }
        }
        // 呈现循环每呈现一个新帧 +1;达 expect 即 PASS。
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(15);
        loop {
            let p = counter.0.load(Ordering::SeqCst);
            if p >= expect {
                println!("[selftest] PASS presented={p} expect={expect}");
                std::io::stdout().flush().ok();
                tx.send(Cmd::Close).ok();
                ht.join().ok();
                exit(0);
            }
            if std::time::Instant::now() > deadline {
                eprintln!("[selftest] FAIL presented={p} < expect={expect}(超时)");
                tx.send(Cmd::Close).ok();
                ht.join().ok();
                exit(1);
            }
            thread::sleep(std::time::Duration::from_millis(50));
        }
    }

    if args[1] == "--hwnd" {
        let parent_raw: isize = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(0);
        if parent_raw == 0 {
            usage();
        }
        let (x, y, w, h) = (
            args.get(3).and_then(|s| s.parse().ok()).unwrap_or(0),
            args.get(4).and_then(|s| s.parse().ok()).unwrap_or(0),
            args.get(5).and_then(|s| s.parse().ok()).unwrap_or(320u32),
            args.get(6).and_then(|s| s.parse().ok()).unwrap_or(240u32),
        );
        let (tx, rx) = mpsc::channel();
        let (ready_tx, ready_rx) = mpsc::channel();
        let ht = thread::spawn(move || window_thread(HWND(parent_raw as *mut c_void), x, y, w, h, rx, ready_tx));
        match ready_rx.recv() {
            Ok(Ok(())) => {}
            Ok(Err(e)) => {
                eprintln!("[presenter] FAIL 初始化: {e}");
                exit(1);
            }
            Err(e) => {
                eprintln!("[presenter] FAIL 窗口线程早退: {e}");
                exit(1);
            }
        }
        // 就绪信号(desktop 冒烟据此判活)
        println!("PRESENTER_READY");
        std::io::stdout().flush().ok();
        let stdin = std::io::stdin();
        for line in stdin.lock().lines() {
            let Ok(line) = line else { break };
            let trimmed = line.trim().trim_start_matches('\u{feff}');
            if trimmed.is_empty() {
                continue;
            }
            if trimmed == "stat" {
                print_stat();
                continue;
            }
            match parse_cmd(trimmed) {
                Some(Cmd::Close) => {
                    tx.send(Cmd::Close).ok();
                    break;
                }
                Some(cmd) => {
                    tx.send(cmd).ok();
                }
                None => eprintln!("[presenter] 无法解析命令: {line}"),
            }
        }
        // stdin 关闭 = 调用方退出 → 收窗口线程
        tx.send(Cmd::Close).ok();
        ht.join().ok();
        exit(0);
    }

    usage();
}
