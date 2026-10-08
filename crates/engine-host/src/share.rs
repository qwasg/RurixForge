//! D3D12 共享纹理生产者(F1 wave.2 G-F1-9,editor glue——非渲染内核)。
//!
//! engine-host 把 `viewport.frame` 的 Vulkan readback 帧写入一张 **跨进程共享** 的
//! D3D12 纹理(legacy KMT handle,`DuplicateHandle` 移交 viewport-presenter);
//! 共享 fence 逐帧递增,消费者按 fence 序呈现。帧源唯一 = `viewport::render_scene_frame`,
//! 与 canvas 回退腿同帧,杜绝双真相源。
//!
//! 状态机:纹理在 COPY_DEST(生产者写)与 COPY_SOURCE(消费者读)间迁移,
//! 跨进程时序由共享 fence 保证(消费者 GPU 侧 queue wait + CPU 侧 completed 观察)。
//!
//! 全模块仅 Windows 参与编译;unsafe 逐块 SAFETY 标注。

#![cfg(windows)]

use std::ffi::c_void;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};

use windows::core::{Interface, PCWSTR};
use windows::Win32::Foundation::{
    CloseHandle, DuplicateHandle, HANDLE, DUPLICATE_SAME_ACCESS,
};
use windows::Win32::Graphics::Direct3D::D3D_FEATURE_LEVEL_11_0;
use windows::Win32::Graphics::Direct3D12::*;
use windows::Win32::Graphics::Dxgi::Common::*;
use windows::Win32::System::Threading::*;

/// GENERIC_ALL(0x10000000):CreateSharedHandle 访问掩码。
const GENERIC_ALL_ACCESS: u32 = 0x1000_0000;

/// 共享 fence 的值序列(每次 open 新建 fence 时归零;每帧 +1,presenter 把它当帧计数)。
/// Producer(CPU 上传档 / rurix 零拷贝档)与 Godot L1 的 [gmain](经 SharedTarget.fence_value)共用这一个计数器,
/// 保证任何时候 fence 值单调、不跳号。rurix 下取值序列与原先的 `SharedBuf.fence_value += 1` 逐一相同。
static FENCE_VALUE: AtomicU64 = AtomicU64::new(0);

/// Godot 后端装进来的 ID3D12Device(02 §4.2 L1 第 1-2 步)。共享 buffer / fence 必须建在消费端(presenter)的
/// adapter 上:presenter 用 `D3D12CreateDevice(None)`,而 Godot 进程因 NvOptimusEnablement 导出、本进程的缺省卡可能不同。
/// gpu_writer = 这就是 Godot 渲染用的 device(与消费端同卡)→ [gmain] 可在它的主队列上直写(L1);
/// 否则是 godot-host 在消费端 adapter 上另建的 device,只给 CPU 上传档用。
struct GodotDevice {
    device: ID3D12Device,
    gpu_writer: bool,
}
// SAFETY: ID3D12Device 是自由线程对象(D3D12 规定 device 方法可多线程并发调用)。
unsafe impl Send for GodotDevice {}
unsafe impl Sync for GodotDevice {}
static GODOT_DEVICE: OnceLock<GodotDevice> = OnceLock::new();

/// 安装共享对象用的 ID3D12Device(接管调用方已 AddRef 的一份引用)。必须早于第一次 open;重复安装返回 Err 并释放这份引用。
pub fn install_device(raw: usize, gpu_writer: bool) -> Result<(), String> {
    if raw == 0 {
        return Err("install_device: 空指针".into());
    }
    // SAFETY: raw 是调用方 AddRef 过的 ID3D12Device*,所有权随 from_raw 移交;失败路径随 drop Release。
    let device = unsafe { ID3D12Device::from_raw(raw as *mut c_void) };
    if slot().lock().unwrap_or_else(|e| e.into_inner()).is_some() {
        return Err("install_device: Producer 已在缺省 device 上创建".into());
    }
    GODOT_DEVICE.set(GodotDevice { device, gpu_writer }).map_err(|_| "install_device: 已安装".to_string())
}

/// L1 目标(02 §4.2):共享对象建在 Godot device 上时,给出 AddRef 过的 buffer / fence 指针与布局;否则 None。
pub fn gpu_target() -> Option<crate::render::sink::SharedTarget> {
    let g = slot().lock().unwrap_or_else(|e| e.into_inner());
    let p = g.as_ref()?;
    if !p.on_godot_device {
        return None;
    }
    let s = p.shared.as_ref()?;
    Some(crate::render::sink::SharedTarget {
        buffer: s.buffer.clone().into_raw() as usize,
        fence: s.fence.clone().into_raw() as usize,
        width: s.width,
        height: s.height,
        row_pitch: s.row_pitch as u32,
        size: s.size,
        fence_value: &FENCE_VALUE,
    })
}

/// 共享体线性布局:行距按 D3D12 `CopyTextureRegion` 的 PLACED_FOOTPRINT 契约
/// 256B 对齐(`D3D12_TEXTURE_DATA_PITCH_ALIGNMENT`),总字节 = 行距 × 高。
/// 生产者(VK pack pass 写)与消费者(D3D12 拷进后台缓冲)按同一公式推导,
/// 任一侧漂移即接线硬错。
pub fn shared_layout(width: u32, height: u32) -> (usize, u64) {
    let row_pitch = ((width as usize) * 4).div_ceil(256) * 256;
    (row_pitch, (row_pitch * height as usize) as u64)
}

struct SharedBuf {
    /// 共享线性 buffer(DEFAULT 堆 + `D3D12_HEAP_FLAG_SHARED`;VK 侧导入为 SSBO 直写)。
    buffer: ID3D12Resource,
    upload: ID3D12Resource,
    upload_ptr: *mut u8,
    row_pitch: usize,
    size: u64,
    width: u32,
    height: u32,
    fence: ID3D12Fence,
    fence_event: HANDLE,
    /// 本进程持有的 buffer NT handle(VK import 用;Drop 关闭)。
    local_buf_handle: HANDLE,
}

impl Drop for SharedBuf {
    fn drop(&mut self) {
        unsafe {
            // SAFETY: fence_event/local_buf_handle 为本进程持有的内核句柄;COM 引用随 drop。
            let _ = CloseHandle(self.fence_event);
            if !self.local_buf_handle.is_invalid() {
                let _ = CloseHandle(self.local_buf_handle);
            }
        }
    }
}

/// VK import 面(buffer 形态)。
#[cfg_attr(not(feature = "backend-rurix"), allow(dead_code))]
pub struct ShareInfo {
    pub handle: u64,
    pub size: u64,
    pub width: u32,
    pub height: u32,
    pub row_pitch: u32,
}

struct Producer {
    device: ID3D12Device,
    queue: ID3D12CommandQueue,
    allocator: ID3D12CommandAllocator,
    list: ID3D12GraphicsCommandList,
    shared: Option<SharedBuf>,
    /// device 是 install_device 装进来的 Godot device(L1 可用)。
    on_godot_device: bool,
}

// SAFETY: Producer 持有 D3D12 COM 指针(windows-rs 接口内为 *mut);仅经 SHARE 全局
// Mutex 互斥访问,满足 D3D12 自由线程模型(外部同步);无跨线程无同步使用。
unsafe impl Send for Producer {}

static SHARE: OnceLock<Mutex<Option<Producer>>> = OnceLock::new();

fn slot() -> &'static Mutex<Option<Producer>> {
    SHARE.get_or_init(|| Mutex::new(None))
}

// 共享体改为线性 buffer 后不再需要资源状态迁移(D3D12 buffer 无 layout,恒等价
// COMMON),原 barrier_transition helper 随之退役。

impl Producer {
    fn new() -> Result<Self, String> {
        unsafe {
            // SAFETY: 标准 D3D12 初始化序列;错误经 HRESULT 传播,不unwrap。
            // Godot 后端装过 device 时用它(消费端同卡);否则与原先一样取缺省 adapter。
            let on_godot_device = GODOT_DEVICE.get().is_some_and(|d| d.gpu_writer);
            let device = match GODOT_DEVICE.get() {
                Some(d) => d.device.clone(),
                None => {
                    let mut device: Option<ID3D12Device> = None;
                    D3D12CreateDevice(None, D3D_FEATURE_LEVEL_11_0, &mut device)
                        .map_err(|e| format!("D3D12CreateDevice: {e}"))?;
                    device.ok_or("D3D12CreateDevice 返回空")?
                }
            };
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
            Ok(Producer {
                device,
                queue,
                allocator,
                list,
                shared: None,
                on_godot_device,
            })
        }
    }

    /// 创建/重建共享 buffer + upload 堆 + 共享 fence;返回 (dup_buf, dup_fence)。
    ///
    /// 共享体是**线性 buffer** 而非纹理:两侧对同一张纹理的补齐规则不同
    /// (960×540 实测 VK 需 2,457,600B vs D3D12 committed 2,228,224B,曾致
    /// `VK_ERROR_DEVICE_LOST`),而线性 buffer 两侧字节数逐字一致,无需堆腿兜底。
    fn open(&mut self, width: u32, height: u32, target_pid: u32) -> Result<(u64, u64), String> {
        self.shared = None; // 释放旧资源(GPU 已闲:调用方保证帧间)
        unsafe {
            // SAFETY: desc/heap 常量字段合法;CreateCommittedResource 出参经 Option 校验。
            let (row_pitch, size) = shared_layout(width, height);
            let buf_desc = D3D12_RESOURCE_DESC {
                Dimension: D3D12_RESOURCE_DIMENSION_BUFFER,
                Width: size,
                Height: 1,
                DepthOrArraySize: 1,
                MipLevels: 1,
                Format: DXGI_FORMAT_UNKNOWN,
                SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
                Layout: D3D12_TEXTURE_LAYOUT_ROW_MAJOR,
                Flags: D3D12_RESOURCE_FLAG_NONE,
                ..Default::default()
            };
            let default_heap = D3D12_HEAP_PROPERTIES {
                Type: D3D12_HEAP_TYPE_DEFAULT,
                ..Default::default()
            };
            // D3D12 buffer 恒等价 COMMON 态(无 layout),跨 API 消费无需状态迁移。
            let mut buffer: Option<ID3D12Resource> = None;
            self.device
                .CreateCommittedResource(
                    &default_heap,
                    D3D12_HEAP_FLAG_SHARED,
                    &buf_desc,
                    D3D12_RESOURCE_STATE_COMMON,
                    None,
                    &mut buffer,
                )
                .map_err(|e| format!("CreateCommittedResource(shared buffer): {e}"))?;
            let buffer = buffer.ok_or("shared buffer 为空")?;

            let upload_heap = D3D12_HEAP_PROPERTIES {
                Type: D3D12_HEAP_TYPE_UPLOAD,
                ..Default::default()
            };
            let mut upload: Option<ID3D12Resource> = None;
            self.device
                .CreateCommittedResource(
                    &upload_heap,
                    D3D12_HEAP_FLAG_NONE,
                    &buf_desc,
                    D3D12_RESOURCE_STATE_GENERIC_READ,
                    None,
                    &mut upload,
                )
                .map_err(|e| format!("CreateCommittedResource(upload): {e}"))?;
            let upload = upload.ok_or("upload 为空")?;
            let mut upload_ptr: *mut c_void = std::ptr::null_mut();
            upload
                .Map(0, None, Some(&mut upload_ptr))
                .map_err(|e| format!("upload.Map: {e}"))?;
            if upload_ptr.is_null() {
                return Err("upload.Map 返回空指针".into());
            }

            let fence: ID3D12Fence = self
                .device
                .CreateFence(0, D3D12_FENCE_FLAG_SHARED)
                .map_err(|e| format!("CreateFence: {e}"))?;
            let fence_event =
                CreateEventW(None, false, false, None).map_err(|e| format!("CreateEventW: {e}"))?;

            // 句柄移交:CreateSharedHandle 产 NT handle(可 DuplicateHandle);
            // 本进程副本保留在 SharedBuf(VK import 用),Drop 关闭。
            // SAFETY: buffer/fence 存活;GENERIC_ALL 访问;DuplicateHandle 参数合法。
            let tex_handle = self
                .device
                .CreateSharedHandle(&buffer, None, GENERIC_ALL_ACCESS, PCWSTR::null())
                .map_err(|e| format!("CreateSharedHandle(buffer): {e}"))?;
            let fence_handle = self
                .device
                .CreateSharedHandle(&fence, None, GENERIC_ALL_ACCESS, PCWSTR::null())
                .map_err(|e| format!("CreateSharedHandle(fence): {e}"))?;

            let target = OpenProcess(PROCESS_DUP_HANDLE, false, target_pid)
                .map_err(|e| format!("OpenProcess({target_pid}): {e}"))?;
            let cur = GetCurrentProcess();
            let mut dup_tex = HANDLE::default();
            let mut dup_fence = HANDLE::default();
            let ok1 = DuplicateHandle(
                cur,
                tex_handle,
                target,
                &mut dup_tex,
                0,
                false,
                DUPLICATE_SAME_ACCESS,
            );
            let ok2 = DuplicateHandle(
                cur,
                fence_handle,
                target,
                &mut dup_fence,
                0,
                false,
                DUPLICATE_SAME_ACCESS,
            );
            // fence 本进程句柄即刻关闭(COM 引用持有本体);tex_handle 保留(VK import)。
            let _ = CloseHandle(fence_handle);
            let _ = CloseHandle(target);
            ok1.map_err(|e| format!("DuplicateHandle(buffer): {e}"))?;
            ok2.map_err(|e| format!("DuplicateHandle(fence): {e}"))?;

            // 新 fence 从 0 起:计数器同步归零(持 SH 锁,[gmain] 此时已 ShareDetach,不会并发取值)。
            FENCE_VALUE.store(0, Ordering::SeqCst);
            self.shared = Some(SharedBuf {
                buffer,
                upload,
                upload_ptr: upload_ptr as *mut u8,
                row_pitch,
                size,
                width,
                height,
                fence,
                fence_event,
                local_buf_handle: tex_handle,
            });
            Ok((dup_tex.0 as u64, dup_fence.0 as u64))
        }
    }

    /// 写一帧 rgba8 → 共享 buffer;fence 递增并等待 GPU 完成(有界 2s)。
    fn write(&mut self, rgba8: &[u8], width: u32, height: u32) -> Result<u64, String> {
        let Some(shared) = &mut self.shared else {
            return Err("共享 buffer 未打开".into());
        };
        if shared.width != width || shared.height != height {
            return Err(format!(
                "尺寸不符:share {}x{} ≠ frame {}x{}",
                shared.width, shared.height, width, height
            ));
        }
        let w = width as usize;
        let h = height as usize;
        // 紧凑 rgba8 长度校验(format=none 档 rgba8 为空,逐行拷贝会越界读)。
        let need = w * h * 4;
        if rgba8.len() < need {
            return Err(format!(
                "帧字节不足:{}B < {need}B({width}x{height} rgba8)",
                rgba8.len()
            ));
        }
        unsafe {
            // SAFETY: upload_ptr 为 Map 得到的有效映射,大小 row_pitch*h;逐行拷贝不越界。
            for y in 0..h {
                let src = rgba8.as_ptr().add(y * w * 4);
                let dst = shared.upload_ptr.add(y * shared.row_pitch);
                std::ptr::copy_nonoverlapping(src, dst, w * 4);
            }

            self.allocator.Reset().map_err(|e| format!("allocator.Reset: {e}"))?;
            self.list
                .Reset(&self.allocator, None)
                .map_err(|e| format!("list.Reset: {e}"))?;

            // 共享体是 buffer:整块线性拷贝即可(D3D12 buffer 无 layout,免状态迁移)。
            self.list
                .CopyBufferRegion(&shared.buffer, 0, &shared.upload, 0, shared.size);
            self.list.Close().map_err(|e| format!("list.Close: {e}"))?;
            let cmd: ID3D12CommandList = self.list.cast().map_err(|e| format!("list cast: {e}"))?;
            self.queue.ExecuteCommandLists(&[Some(cmd)]);

            let v = FENCE_VALUE.fetch_add(1, Ordering::SeqCst) + 1;
            self.queue
                .Signal(&shared.fence, v)
                .map_err(|e| format!("queue.Signal: {e}"))?;
            // 等 GPU 写完(生产者同步,保证 upload 复用安全;有界防死锁)。
            shared
                .fence
                .SetEventOnCompletion(v, shared.fence_event)
                .map_err(|e| format!("SetEventOnCompletion: {e}"))?;
            let r = WaitForSingleObject(shared.fence_event, 2000);
            if r.0 != 0 {
                return Err(format!("fence {v} 等待失败(wait={})", r.0));
            }
            Ok(v)
        }
    }

    /// 零拷贝帧信号(buffer 形态):VK 的 pack pass 已把本帧写进共享 buffer
    /// (session 帧 fence 完成由 CPU 侧知悉,且上游帧末已录 EXTERNAL release),
    /// 此处仅推进共享 fence 通知消费者。buffer 无 layout,无需任何状态迁移。
    /// 返回本帧 fence 值。
    fn signal(&mut self) -> Result<u64, String> {
        let Some(shared) = &mut self.shared else {
            return Err("共享 buffer 未打开".into());
        };
        unsafe {
            // SAFETY: fence 存活于 self;Signal 仅入队序号,无资源引用。
            let v = FENCE_VALUE.fetch_add(1, Ordering::SeqCst) + 1;
            self.queue
                .Signal(&shared.fence, v)
                .map_err(|e| format!("queue.Signal: {e}"))?;
            Ok(v)
        }
    }

    fn close(&mut self) {
        // SharedTex::Drop 统一关闭 fence_event/local_tex_handle;重建(open 内 `= None`)
        // 与显式 close 同路,杜绝 wave.2 遗留的 fence_event 重建泄漏。
        self.shared = None;
    }
}

impl Drop for Producer {
    fn drop(&mut self) {
        self.close();
    }
}

/// 打开/重建共享 buffer 并把句柄移交 target_pid。
/// 返回 (bufHandle, fenceHandle, width, height, rowPitch, size)。
pub fn open(
    width: u32,
    height: u32,
    target_pid: u32,
) -> Result<(u64, u64, u32, u32, u32, u64), String> {
    let mut g = slot().lock().unwrap_or_else(|e| e.into_inner());
    if g.is_none() {
        *g = Some(Producer::new()?);
    }
    let p = g.as_mut().expect("producer 刚初始化");
    let (t, f) = p.open(width, height, target_pid)?;
    let (row_pitch, size) = shared_layout(width, height);
    Ok((t, f, width, height, row_pitch as u32, size))
}

/// D3D12 adapter LUID(与 VK physical device LUID 对拍;producer 未初始化 = None)。
#[cfg_attr(not(feature = "backend-rurix"), allow(dead_code))]
pub fn adapter_luid() -> Option<[u8; 8]> {
    let g = slot().lock().unwrap_or_else(|e| e.into_inner());
    let p = g.as_ref()?;
    // SAFETY: device 存活于 Producer;GetAdapterLuid 无副作用。
    let luid = unsafe { p.device.GetAdapterLuid() };
    let mut out = [0u8; 8];
    out[..4].copy_from_slice(&luid.LowPart.to_ne_bytes());
    out[4..].copy_from_slice(&luid.HighPart.to_ne_bytes());
    Some(out)
}

/// 写一帧;返回 fence 值。共享未打开 = Ok(None)(不视为错误,canvas 腿照常)。
pub fn write_frame(rgba8: &[u8], width: u32, height: u32) -> Result<Option<u64>, String> {
    let mut g = slot().lock().unwrap_or_else(|e| e.into_inner());
    let Some(p) = g.as_mut() else { return Ok(None) };
    if p.shared.is_none() {
        return Ok(None);
    }
    p.write(rgba8, width, height).map(Some)
}

/// 是否已打开共享 buffer(供 viewport.frame 决定是否随帧写入)。
pub fn is_open() -> bool {
    let g = slot().lock().unwrap_or_else(|e| e.into_inner());
    g.as_ref().is_some_and(|p| p.shared.is_some())
}

/// VK import 面(buffer 形态)。未打开 = None。handle 生命周期随 SharedBuf
/// (close/重建即失效),调用方须即取即用。
#[cfg_attr(not(feature = "backend-rurix"), allow(dead_code))]
pub fn vk_import_info() -> Option<ShareInfo> {
    let g = slot().lock().unwrap_or_else(|e| e.into_inner());
    let p = g.as_ref()?;
    let s = p.shared.as_ref()?;
    Some(ShareInfo {
        handle: s.local_buf_handle.0 as u64,
        size: s.size,
        width: s.width,
        height: s.height,
        row_pitch: s.row_pitch as u32,
    })
}

/// 零拷贝帧信号:仅推进共享 fence。共享未打开 = Ok(None)。
pub fn signal_frame() -> Result<Option<u64>, String> {
    let mut g = slot().lock().unwrap_or_else(|e| e.into_inner());
    let Some(p) = g.as_mut() else { return Ok(None) };
    if p.shared.is_none() {
        return Ok(None);
    }
    p.signal().map(Some)
}

/// 关闭共享 buffer(幂等)。
pub fn close() {
    let mut g = slot().lock().unwrap_or_else(|e| e.into_inner());
    if let Some(p) = g.as_mut() {
        p.close();
    }
}

#[cfg(test)]
mod tests {
    use super::shared_layout;

    /// 共享体线性布局契约:行距 256B 对齐(D3D12 `CopyTextureRegion` 的
    /// PLACED_FOOTPRINT 要求),总字节 = 行距 × 高。生产者(VK pack pass 按
    /// row_words 写)与消费者(presenter 按 RowPitch 拷)共用本公式,任一侧漂移
    /// 即接线硬错,故此处看门。
    #[test]
    fn shared_layout_contract() {
        for (w, h) in [(128u32, 96u32), (64, 64), (512, 288), (960, 540), (1024, 540), (1920, 1080)]
        {
            let (row_pitch, size) = shared_layout(w, h);
            assert_eq!(row_pitch % 256, 0, "{w}x{h}: 行距 {row_pitch} 未 256B 对齐");
            assert!(
                row_pitch >= (w as usize) * 4,
                "{w}x{h}: 行距 {row_pitch} 容不下一行 rgba8"
            );
            assert!(
                row_pitch < (w as usize) * 4 + 256,
                "{w}x{h}: 行距 {row_pitch} 超出最小对齐冗余(公式漂移)"
            );
            assert_eq!(size, (row_pitch * h as usize) as u64, "{w}x{h}: 总字节 ≠ 行距×高");
            // pack 着色器按 u32 步进写,行距须 4B 整除(256 对齐已蕴含,显式钉死)。
            assert_eq!(row_pitch % 4, 0, "{w}x{h}: 行距非 4B 整除,pack row_words 会截断");
        }
    }
}
