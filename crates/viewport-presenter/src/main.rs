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
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Mutex,OnceLock};
use std::collections::VecDeque;
use std::time::Instant;
use std::sync::mpsc;
use std::thread;

use windows::core::{Interface, PCWSTR};
use windows::Win32::Foundation::{DXGI_STATUS_OCCLUDED, CloseHandle, HANDLE, HINSTANCE, HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows::Win32::Graphics::Direct3D::D3D_FEATURE_LEVEL_11_0;
use windows::Win32::Graphics::Direct3D12::*;
use windows::Win32::Graphics::Dxgi::Common::*;
use windows::Win32::Graphics::Dxgi::*;
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Threading::{CreateEventW, WaitForSingleObject};
use windows::Win32::UI::HiDpi::{SetProcessDpiAwarenessContext, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2};
use windows::Win32::UI::WindowsAndMessaging::*;

const WINDOW_CLASS: &str = "ForgeViewportPresenter";
/// 呈现帧率目标(就近 vsync 近似;消息泵空闲期 peek 间隔)。
const PRESENT_TICK_MS: u32 = 8;
static PRESENT_CALLS:AtomicU64=AtomicU64::new(0);
static RECORD_METRICS:AtomicBool=AtomicBool::new(false);
static SAMPLES:OnceLock<Mutex<VecDeque<String>>>=OnceLock::new();
fn record_sample(value:String){let mut samples=SAMPLES.get_or_init(||Mutex::new(VecDeque::new())).lock().unwrap_or_else(|e|e.into_inner());if samples.len()==512{samples.pop_front();}samples.push_back(value);}
fn print_sample_stats(){let samples=SAMPLES.get_or_init(||Mutex::new(VecDeque::new())).lock().unwrap_or_else(|e|e.into_inner());println!("PRESENT_STATS {{\"presentCalls\":{},\"visiblePresents\":{},\"samples\":[{}]}}",PRESENT_CALLS.load(Ordering::SeqCst),presented_counter().0.load(Ordering::SeqCst),samples.iter().cloned().collect::<Vec<_>>().join(","));std::io::stdout().flush().ok();}


// ─────────────────────────── D3D12 消费者状态 ───────────────────────────

fn diagnostic_epoch_micros()->Option<u64>{std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).ok().and_then(|t|u64::try_from(t.as_micros()).ok())}
struct ReceivedHandle(HANDLE);
impl Drop for ReceivedHandle { fn drop(&mut self) { // SAFETY: one owner for each duplicated process handle.
    unsafe { let _=CloseHandle(self.0); }
} }
struct NativeSlot { buffer: ID3D12Resource, produced: ID3D12Fence, consumed: ID3D12Fence, seen: u64 }

struct Dx {
    _factory: IDXGIFactory2,
    device: ID3D12Device,
    queue: ID3D12CommandQueue,
    allocator: ID3D12CommandAllocator,
    list: ID3D12GraphicsCommandList,
    swapchain: Option<IDXGISwapChain3>,
    rtv_heap: ID3D12DescriptorHeap,
    rtv_size: u32,
    /// 共享线性 buffer(生产者 VK pack pass 按 row_pitch 逐行写)。
    shared: Option<ID3D12Resource>,
    fence: Option<ID3D12Fence>,
    fence_event: HANDLE,
    seen_fence: u64,
    width: u32,
    height: u32,
    /// 共享 buffer 行距(256B 对齐;PLACED_FOOTPRINT 拷贝用)。
    row_pitch: u32,
    presented: u64,
    native_slots: Vec<NativeSlot>,
    generation: u64,
    completion: ID3D12Fence,
    completion_value: u64,
    last_present:Option<Instant>,
    last_visible:bool,
}

impl Dx {
    fn new() -> Result<Self, String> {
        unsafe {
            // SAFETY: 标准 DXGI/D3D12 初始化;全部参数有效,错误经 HRESULT 传播。
            let factory: IDXGIFactory2 = CreateDXGIFactory2(DXGI_CREATE_FACTORY_FLAGS(0))
                .map_err(|e| format!("CreateDXGIFactory2: {e}"))?;
            let mut device: Option<ID3D12Device> = None;
            if std::env::args().any(|arg|arg=="--high-performance") {
                let gpu_factory:IDXGIFactory6=factory.cast().map_err(|e|e.to_string())?;
                let adapter:IDXGIAdapter1=gpu_factory.EnumAdapterByGpuPreference(0,DXGI_GPU_PREFERENCE_HIGH_PERFORMANCE).map_err(|e|e.to_string())?;
                D3D12CreateDevice(&adapter,D3D_FEATURE_LEVEL_11_0,&mut device).map_err(|e|e.to_string())?;
            } else {D3D12CreateDevice(None,D3D_FEATURE_LEVEL_11_0,&mut device).map_err(|e|e.to_string())?;}
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
            let completion:ID3D12Fence=device.CreateFence(0,D3D12_FENCE_FLAG_NONE).map_err(|e|e.to_string())?;
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
                row_pitch: 0,
                presented: 0,
                native_slots:Vec::new(), generation:0, completion, completion_value:0, last_present:None,last_visible:false,
            })
        }
    }

    /// 绑定共享 buffer + fence(各仅一次;重复 bind 先释放旧引用)。
    ///
    /// 共享体是**线性 buffer** 而非纹理:两侧对同一张纹理的行/高补齐规则不同
    /// (960×540 实测 VK 需 2,457,600B vs D3D12 committed 2,228,224B),曾致
    /// `VK_ERROR_DEVICE_LOST`;线性 buffer 两侧字节数逐字一致。生产者(engine-host
    /// 的 VK pack pass)按 `row_pitch` 逐行写,本侧按同一 `row_pitch` 以
    /// PLACED_FOOTPRINT 拷进后台缓冲。
    fn bind(
        &mut self,
        tex_raw: usize,
        fence_raw: usize,
        width: u32,
        height: u32,
        row_pitch: u32,
    ) -> Result<(), String> {
        unsafe {
            // SAFETY: tex_raw/fence_raw 为 engine-host 经 DuplicateHandle 复制给本进程的
            // 有效句柄;OpenSharedHandle 成功即取得 COM 引用,失败原样返回 HRESULT。
            let tex_h = ReceivedHandle(HANDLE(tex_raw as *mut c_void));
            let fence_h = ReceivedHandle(HANDLE(fence_raw as *mut c_void));
            self.shared = None;
            self.native_slots.clear();
            self.seen_fence=0;
            // 布局契约对拍:行距须 256B 对齐且容得下一行 rgba8,不符即 fail-closed
            // (与 engine-host share.rs::shared_layout 同式;任一侧漂移即接线硬错)。
            let expect = ((width as usize) * 4).div_ceil(256) * 256;
            if row_pitch as usize != expect {
                return Err(format!(
                    "共享 buffer 行距不匹配:收到 {row_pitch},按 {width}x{height} 应为 {expect}"
                ));
            }
            let shared = {
                let mut sh: Option<ID3D12Resource> = None;
                self.device
                    .OpenSharedHandle(tex_h.0, &mut sh)
                    .map_err(|e| format!("OpenSharedHandle(buffer): {e}"))?;
                sh.ok_or("OpenSharedHandle(buffer) 返回空")?
            };
            let mut fence: Option<ID3D12Fence> = None;
            self.device
                .OpenSharedHandle(fence_h.0, &mut fence)
                .map_err(|e| format!("OpenSharedHandle(fence): {e}"))?;
            let fence = fence.ok_or("OpenSharedHandle(fence) 返回空")?;

            self.shared = Some(shared);
            self.fence = Some(fence);
            self.width = width;
            self.height = height;
            self.row_pitch = row_pitch;
            Ok(())
        }
    }

    fn bind_native(&mut self, generation:u64, handles:[[usize;3];2], w:u32,h:u32,pitch:u32)->Result<(),String>{
        let mut slots=Vec::with_capacity(2);
        for [buffer,produced,consumed] in handles {
            self.bind(buffer,produced,w,h,pitch)?;
            // SAFETY: engine-host duplicated this NT handle for this process. Close after opening COM reference.
            let fence=unsafe{
                let handle=HANDLE(consumed as *mut c_void);let mut fence:Option<ID3D12Fence>=None;
                let result=self.device.OpenSharedHandle(handle,&mut fence);let _=CloseHandle(handle);
                result.map_err(|e|e.to_string())?;fence.ok_or("consumer fence missing")?
            };
            slots.push(NativeSlot{buffer:self.shared.take().unwrap(),produced:self.fence.take().unwrap(),consumed:fence,seen:0});
        }
        self.native_slots=slots; self.generation=generation; self.last_present=None; Ok(())
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
                // 共享 buffer 按推流尺寸(CSS px)建,子窗口按设备像素铺满视口面板:dpr ≠ 1 时两者不同,
                // NONE 会 1:1 只占左上角(其余黑),必须拉伸。
                Scaling: DXGI_SCALING_STRETCH,
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
    fn present_latest(&mut self, window_visible:bool, polling_sleep_ms:f64, polling_iterations:u64) -> Result<Option<u64>, String> {
        let started=Instant::now();
        let Some(sc)=self.swapchain.as_ref() else {return Ok(None)};
        // FIFO across two slots prevents an older slot being starved by a newer frame.
        let candidate=self.native_slots.iter().enumerate().filter_map(|(i,s)|{
            // SAFETY: fence is owned by the bound slot.
            let value=unsafe{s.produced.GetCompletedValue()};
            (value>s.seen).then_some((i,value))
        }).min_by_key(|(_,value)|*value);
        let (shared,target,native_index)=if self.native_slots.is_empty(){
            let(Some(shared),Some(fence))=(&self.shared,&self.fence) else {return Ok(None)};
            // SAFETY: bound producer fence remains live.
            let value=unsafe{fence.GetCompletedValue()};
            if value<=self.seen_fence{return Ok(None)}
            (shared.clone(),value,None)
        }else{
            let Some((index,value))=candidate else {return Ok(None)};
            (self.native_slots[index].buffer.clone(),value,Some(index))
        };
        let observed_utc_micros=diagnostic_epoch_micros().map(|v|v.to_string()).unwrap_or_else(||"null".into());
        unsafe {
            // SAFETY: the previous submission was waited before returning; allocator is idle.
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
            // 共享体是线性 buffer:按 PLACED_FOOTPRINT(256B 对齐行距)拷进后台缓冲。
            let dst = D3D12_TEXTURE_COPY_LOCATION {
                pResource: std::mem::ManuallyDrop::new(Some(back.clone())),
                Type: D3D12_TEXTURE_COPY_TYPE_SUBRESOURCE_INDEX,
                Anonymous: D3D12_TEXTURE_COPY_LOCATION_0 {
                    SubresourceIndex: 0,
                },
            };
            let src = D3D12_TEXTURE_COPY_LOCATION {
                pResource: std::mem::ManuallyDrop::new(Some(shared.clone())),
                Type: D3D12_TEXTURE_COPY_TYPE_PLACED_FOOTPRINT,
                Anonymous: D3D12_TEXTURE_COPY_LOCATION_0 {
                    PlacedFootprint: D3D12_PLACED_SUBRESOURCE_FOOTPRINT {
                        Offset: 0,
                        Footprint: D3D12_SUBRESOURCE_FOOTPRINT {
                            Format: DXGI_FORMAT_R8G8B8A8_UNORM,
                            Width: self.width,
                            Height: self.height,
                            Depth: 1,
                            RowPitch: self.row_pitch,
                        },
                    },
                },
            };
            self.list.CopyTextureRegion(&dst, 0, 0, 0, &src, None);
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
            // GPU acknowledgements are ordered after the copy, never after CPU submission alone.
            if let Some(index)=native_index {
                self.queue.Signal(&self.native_slots[index].consumed,target).map_err(|e|e.to_string())?;
            }
            self.completion_value+=1;
            self.queue.Signal(&self.completion,self.completion_value).map_err(|e|e.to_string())?;
            self.completion.SetEventOnCompletion(self.completion_value,self.fence_event).map_err(|e|e.to_string())?;
            let wait=WaitForSingleObject(self.fence_event,2000);
            if wait.0!=0 || self.completion.GetCompletedValue()<self.completion_value || self.completion.GetCompletedValue()==u64::MAX{return Err(format!("PRESENTER_COPY_TIMEOUT: generation={} frame={} wait={}",self.generation,target,wait.0))}
            let copy_ms=started.elapsed().as_secs_f64()*1000.;
            let present_start=Instant::now();
            let present_result=sc.Present(1, DXGI_PRESENT(0));
            present_result.ok().map_err(|e| format!("Present: {e}"))?;
            let occluded=present_result==DXGI_STATUS_OCCLUDED;
            self.last_visible=window_visible && !occluded;
            PRESENT_CALLS.fetch_add(1,Ordering::SeqCst);
            let now=Instant::now();
            let presented_utc_micros=diagnostic_epoch_micros().map(|v|v.to_string()).unwrap_or_else(||"null".into());
            if RECORD_METRICS.load(Ordering::Relaxed) {
                let mut stats=DXGI_FRAME_STATISTICS::default();
                let (count,qpc)=if sc.GetFrameStatistics(&mut stats).is_ok(){(stats.PresentCount.to_string(),stats.SyncQPCTime.to_string())}else{("null".into(),"null".into())};
                let interval=self.last_present.map(|previous|now.duration_since(previous).as_secs_f64()*1000.).map(|v|v.to_string()).unwrap_or_else(||"null".into());
                let slot=native_index.map(|i|i.to_string()).unwrap_or_else(||"null".into());
                record_sample(format!("{{\"sample\":{},\"generation\":{},\"slot\":{},\"producerFrame\":{},\"width\":{},\"height\":{},\"copyCompleteMs\":{},\"presentCallMs\":{},\"intervalMs\":{},\"dxgiPresentCount\":{},\"dxgiSyncQpcTime\":{},\"windowVisible\":{},\"occluded\":{},\"observedUtcMicros\":{},\"pollingSleepMs\":{},\"pollingIterations\":{},\"presentedUtcMicros\":{}}}",self.presented+1,self.generation,slot,target,self.width,self.height,copy_ms,now.duration_since(present_start).as_secs_f64()*1000.,interval,count,qpc,window_visible,occluded,observed_utc_micros,polling_sleep_ms,polling_iterations,presented_utc_micros));
            }
            self.last_present=Some(now);
            if let Some(index)=native_index {self.native_slots[index].seen=target;}
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
    BindNative { generation:u64, handles:[[usize;3];2], w:u32,h:u32,row_pitch:u32 },
    Bind { tex: usize, fence: usize, w: u32, h: u32, row_pitch: u32 },
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
        // 子窗口嵌入 Electron 视口区,必须压在 Chromium 自己的子窗口之上才看得见(z 序见呈现循环)。
        // WS_DISABLED:禁用的子窗口不收鼠标,输入交给父窗(Electron 顶层 → Chromium 按坐标分给网页),
        // 点选/环绕/滚轮才不被原生层吞掉;WS_EX_TRANSPARENT 只影响兄弟窗口的绘制顺序,跨进程并不穿透命中测试。
        // 不带 WS_VISIBLE:首帧呈现之后再显示,免得 bind 后、首帧前在视口上盖一块黑(推流按需出帧,空闲时可能很久没帧)。
        // NOACTIVATE = 永不抢键盘焦点。
        (WS_CHILD | WS_DISABLED, WS_EX_TRANSPARENT | WS_EX_NOACTIVATE)
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
    if parent.0.is_null() {
        // The launcher hides the console with STARTUPINFO. Explicitly show the requested
        // standalone viewport; counting a hidden window as presentation is forbidden.
        // SAFETY: hwnd was created on this thread. Geometry and activation are preserved.
        unsafe {SetWindowPos(hwnd,Some(HWND_TOP),0,0,0,0,SWP_NOMOVE|SWP_NOSIZE|SWP_NOACTIVATE|SWP_SHOWWINDOW).map_err(|e|eprintln!("[presenter] show failed: {e}")).ok();}
    }
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

    // 子窗口模式:首帧之前隐藏,之后显示并维持在兄弟窗口最上面;独立窗口(--selftest)一律照旧。
    let is_child = !parent.0.is_null();
    let mut shown = !is_child;
    let mut tick: u32 = 0;
    let mut polling_sleep_ms=0.0;let mut polling_iterations=0u64;
    let mut msg = MSG::default();
    'outer: loop {
        // 命令队列优先
        while let Ok(cmd) = rx.try_recv() {
            match cmd {
                Cmd::Bind { tex, fence, w, h, row_pitch } => {
                    let recreate = dx.width != w || dx.height != h || dx.swapchain.is_none();
                    if let Err(e) = dx.bind(tex, fence, w, h, row_pitch) {
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
                        if is_child && shown {
                            // 新 swapchain 在首帧前是黑的:先藏起来,让下面的网页画布顶着,首帧到了再显示
                            unsafe {
                                // SAFETY: hwnd 为本线程窗口。
                                let _ = ShowWindow(hwnd, SW_HIDE);
                            }
                            shown = false;
                        }
                    }
                }
                Cmd::BindNative {generation,handles,w,h,row_pitch}=>{
                    let recreate=dx.width!=w||dx.height!=h||dx.swapchain.is_none();
                    if let Err(e)=dx.bind_native(generation,handles,w,h,row_pitch){eprintln!("[presenter] native bind failed: {e}");break 'outer;}
                    if recreate {dx.swapchain=None;if let Err(e)=dx.create_swapchain(hwnd){eprintln!("[presenter] resize failed: {e}");break 'outer;}}
                }
                Cmd::Move { x, y, w, h } => unsafe {
                    // SAFETY: hwnd 为本线程窗口。子窗口顺带提到兄弟窗口最上面(HWND_TOP,不激活)。
                    let (after, flags) = if is_child { (Some(HWND_TOP), SWP_NOACTIVATE) } else { (None, SWP_NOZORDER | SWP_NOACTIVATE) };
                    let _ = SetWindowPos(hwnd, after, x, y, w as i32, h as i32, flags);
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
        if dx.shared.is_some() || !dx.native_slots.is_empty() {
            // A hidden/minimized window is not evidence of visible presentation.
            // SAFETY: hwnd belongs to this live window thread.
            let visible=unsafe{IsWindowVisible(hwnd).as_bool() && !IsIconic(hwnd).as_bool()};
            match dx.present_latest(visible,polling_sleep_ms,polling_iterations) {
                Ok(Some(_)) => {
                    polling_sleep_ms=0.0;polling_iterations=0;
                    if dx.last_visible {presented_counter().0.fetch_add(1, Ordering::SeqCst);}
                    if !shown {
                        // 首帧已进 swapchain:显示并提到兄弟窗口最上面(都不激活)
                        unsafe {
                            // SAFETY: hwnd 为本线程窗口。
                            let _ = ShowWindow(hwnd, SW_SHOWNA);
                            let _ = SetWindowPos(hwnd, Some(HWND_TOP), 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE);
                        }
                        shown = true;
                    }
                }
                Ok(None) => {}
                Err(e) => {
                    eprintln!("[presenter] present 失败: {e}");
                    break 'outer;
                }
            }
        }
        // z 序维护:Chromium 调整自己的子窗口(改尺寸、重建 GPU 输出窗口)时会把它们重新压到 presenter 上面。
        // 约每 256 ms 查一次,上方还有兄弟窗口就把自己提回最上(不激活)。
        tick = tick.wrapping_add(1);
        if is_child && shown && tick.is_multiple_of(32) {
            unsafe {
                // SAFETY: hwnd 为本线程窗口;GW_HWNDPREV 取 z 序紧挨在上方的兄弟窗口,没有时返回 Err。
                if GetWindow(hwnd, GW_HWNDPREV).is_ok_and(|h| !h.0.is_null()) {
                    let _ = SetWindowPos(hwnd, Some(HWND_TOP), 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE);
                }
            }
        }
        let sleep_started=Instant::now();
        thread::sleep(std::time::Duration::from_millis(if dx.native_slots.is_empty(){PRESENT_TICK_MS as u64}else{1}));
        polling_sleep_ms+=sleep_started.elapsed().as_secs_f64()*1000.;polling_iterations+=1;
    }
    unsafe {
        // SAFETY: 退出前销毁窗口,父窗口无悬挂子引用。
        let _ = DestroyWindow(hwnd);
    }
    HWND_SLOT.store(0, Ordering::SeqCst);
}

// ─────────────────────────── stdin 命令协议 ───────────────────────────

/// 逐行读取:`bind buf <handle> <fence> <w> <h> <rowPitch>` / `move <x> <y> <w> <h>` / `close`。
/// 共享体为线性 buffer(见 `Dx::bind` 头注);旧的 `bind [tex|heap] ...` 五/六段形态
/// 随纹理共享一并退役——收到即返回 None,由调用方如实报错,不做静默兼容。
/// 容忍 BOM(PowerShell 5.1 重定向 stdin 默认 UTF-8 带 BOM)。
fn parse_cmd(line: &str) -> Option<Cmd> {
    let line = line.trim().trim_start_matches('\u{feff}');
    let parts: Vec<&str> = line.split_whitespace().collect();
    match parts.first().copied()? {
        "bind" if parts.len()==12 && parts[1]=="buf2" => Some(Cmd::BindNative {
            generation:parts[2].parse().ok()?,
            handles:[[parts[3].parse().ok()?,parts[4].parse().ok()?,parts[5].parse().ok()?],
                     [parts[6].parse().ok()?,parts[7].parse().ok()?,parts[8].parse().ok()?]],
            w:parts[9].parse().ok()?,h:parts[10].parse().ok()?,row_pitch:parts[11].parse().ok()?,
        }),
        "bind" if parts.len() == 7 && parts[1] == "buf" => Some(Cmd::Bind {
            tex: parts[2].parse().ok()?,
            fence: parts[3].parse().ok()?,
            w: parts[4].parse().ok()?,
            h: parts[5].parse().ok()?,
            row_pitch: parts[6].parse().ok()?,
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
    eprintln!("usage: viewport-presenter --standalone [--high-performance] [--record-metrics] | --hwnd <parent_dec> | --selftest --tex <h> --fence <h> --w <n> --h <n> [--expect-frames <n>]");
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
    RECORD_METRICS.store(args.iter().any(|arg|arg=="--record-metrics"),Ordering::Relaxed);
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
            Some(cmd @ (Cmd::Bind { .. } | Cmd::BindNative { .. })) => tx.send(cmd).expect("send bind"),
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

    if args[1] == "--hwnd" || args[1] == "--standalone" {
        let standalone=args[1]=="--standalone";
        let parent_raw: isize = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(0);
        if parent_raw == 0 && !standalone {
            usage();
        }
        let (x, y, w, h) = (
            args.get(3).and_then(|s| s.parse().ok()).unwrap_or(0),
            args.get(4).and_then(|s| s.parse().ok()).unwrap_or(0),
            args.get(5).and_then(|s| s.parse().ok()).unwrap_or(320u32),
            args.get(6).and_then(|s| s.parse().ok()).unwrap_or(240u32),
        );
        let (w,h)=if standalone{(1280,720)}else{(w,h)};
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
        // Window close must end the process even while the control pipe remains open.
        // The window thread owns and retires GPU resources before main joins it.
        thread::spawn(move || {
        let stdin = std::io::stdin();
        for line in stdin.lock().lines() {
            let Ok(line) = line else { break };
            let trimmed = line.trim().trim_start_matches('\u{feff}');
            if trimmed.is_empty() {
                continue;
            }
            if trimmed == "stat_json" {print_sample_stats();continue;}
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
        });
        ht.join().ok();
        exit(0);
    }

    usage();
}

#[cfg(test)]
mod protocol_tests {
    use super::*;
    #[test]
    fn native_bind_requires_two_complete_slots_and_generation(){
        match parse_cmd("bind buf2 9 11 12 13 21 22 23 2560 1440 10240") {
            Some(Cmd::BindNative{generation,handles,w,h,row_pitch})=>{assert_eq!(generation,9);assert_eq!(handles,[[11,12,13],[21,22,23]]);assert_eq!((w,h,row_pitch),(2560,1440,10240));},
            _=>panic!("valid protocol 2 bind rejected"),
        }
        assert!(parse_cmd("bind buf2 9 11 12 13 2560 1440 10240").is_none());
        assert!(matches!(parse_cmd("bind buf 11 12 640 480 2560"),Some(Cmd::Bind{..})));
    }
}
