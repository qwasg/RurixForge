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

struct SharedTex {
    texture: ID3D12Resource,
    /// 加堆腿(F1 wave.3):vk req > committed alloc 时,纹理为堆内 placed resource,
    /// 堆对象须活过纹理(字段序:先 texture 后 heap,drop 同序)。
    heap: Option<ID3D12Heap>,
    /// local_tex_handle 指向堆(true,D3D12_HEAP import)还是纹理本身(false,D3D12_RESOURCE)。
    is_heap_handle: bool,
    upload: ID3D12Resource,
    upload_ptr: *mut u8,
    row_pitch: usize,
    width: u32,
    height: u32,
    fence: ID3D12Fence,
    fence_value: u64,
    fence_event: HANDLE,
    in_copy_dest: bool,
    /// 本进程持有的纹理 NT handle(F1 wave.3:VK import 用;Drop 关闭)。
    local_tex_handle: HANDLE,
    /// `GetResourceAllocationInfo` 实测分配字节数(VK import `allocationSize` 用)。
    alloc_size: u64,
    /// 零拷贝档首帧是否已把纹理一次性迁移到 COPY_SOURCE。
    zc_transitioned: bool,
}

impl Drop for SharedTex {
    fn drop(&mut self) {
        unsafe {
            // SAFETY: fence_event/local_tex_handle 为本进程持有的内核句柄;COM 引用随 drop。
            let _ = CloseHandle(self.fence_event);
            if !self.local_tex_handle.is_invalid() {
                let _ = CloseHandle(self.local_tex_handle);
            }
        }
    }
}

struct Producer {
    device: ID3D12Device,
    queue: ID3D12CommandQueue,
    allocator: ID3D12CommandAllocator,
    list: ID3D12GraphicsCommandList,
    shared: Option<SharedTex>,
}

// SAFETY: Producer 持有 D3D12 COM 指针(windows-rs 接口内为 *mut);仅经 SHARE 全局
// Mutex 互斥访问,满足 D3D12 自由线程模型(外部同步);无跨线程无同步使用。
unsafe impl Send for Producer {}

static SHARE: OnceLock<Mutex<Option<Producer>>> = OnceLock::new();

fn slot() -> &'static Mutex<Option<Producer>> {
    SHARE.get_or_init(|| Mutex::new(None))
}

fn barrier_transition(
    res: &ID3D12Resource,
    before: D3D12_RESOURCE_STATES,
    after: D3D12_RESOURCE_STATES,
) -> D3D12_RESOURCE_BARRIER {
    D3D12_RESOURCE_BARRIER {
        Type: D3D12_RESOURCE_BARRIER_TYPE_TRANSITION,
        Anonymous: D3D12_RESOURCE_BARRIER_0 {
            Transition: std::mem::ManuallyDrop::new(D3D12_RESOURCE_TRANSITION_BARRIER {
                pResource: std::mem::ManuallyDrop::new(Some(res.clone())),
                Subresource: D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES,
                StateBefore: before,
                StateAfter: after,
            }),
        },
        ..Default::default()
    }
}

impl Producer {
    fn new() -> Result<Self, String> {
        unsafe {
            // SAFETY: 标准 D3D12 初始化序列;错误经 HRESULT 传播,不unwrap。
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
            Ok(Producer {
                device,
                queue,
                allocator,
                list,
                shared: None,
            })
        }
    }

    /// 创建/重建共享纹理 + upload 堆 + 共享 fence;返回 (dup_tex, dup_fence, is_heap)。
    /// `min_alloc` = VK import 端图像内存需求(probe 实测,0 = 未知/无 vulkan);
    /// `min_alloc > committed alloc` 时走共享堆 + placed resource 腿(committed 无法超尺寸)。
    fn open(
        &mut self,
        width: u32,
        height: u32,
        target_pid: u32,
        min_alloc: u64,
    ) -> Result<(u64, u64, bool), String> {
        self.shared = None; // 释放旧资源(GPU 已闲:调用方保证帧间)
        unsafe {
            // SAFETY: desc/heap 常量字段合法;CreateCommittedResource 出参经 Option 校验。
            let tex_desc = D3D12_RESOURCE_DESC {
                Dimension: D3D12_RESOURCE_DIMENSION_TEXTURE2D,
                Width: width as u64,
                Height: height,
                DepthOrArraySize: 1,
                MipLevels: 1,
                Format: DXGI_FORMAT_R8G8B8A8_UNORM,
                SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
                Layout: D3D12_TEXTURE_LAYOUT_UNKNOWN,
                Flags: D3D12_RESOURCE_FLAG_NONE,
                ..Default::default()
            };
            let default_heap = D3D12_HEAP_PROPERTIES {
                Type: D3D12_HEAP_TYPE_DEFAULT,
                ..Default::default()
            };
            // VK import 需要 D3D12 分配字节数(RFC-0001 §4.2.2 同配方)。
            let committed_alloc = self.device.GetResourceAllocationInfo(0, &[tex_desc]).SizeInBytes;
            // 加堆腿判定:VK 需求(同 pitch 行补齐差异,960x540 实测 2,457,600 vs 2,228,224)
            // 超过 committed 分配 → 共享堆尺寸 = max 并 64KiB 对齐,placed resource 在偏移 0。
            let use_heap = min_alloc > committed_alloc;
            let (texture, heap, alloc_size): (ID3D12Resource, Option<ID3D12Heap>, u64) = if use_heap {
                let heap_size = min_alloc.next_multiple_of(65_536);
                let heap_desc = D3D12_HEAP_DESC {
                    SizeInBytes: heap_size,
                    Properties: default_heap.clone(),
                    Alignment: 0,
                    Flags: D3D12_HEAP_FLAG_SHARED,
                };
                let mut heap: Option<ID3D12Heap> = None;
                self.device
                    .CreateHeap(&heap_desc, &mut heap)
                    .map_err(|e| format!("CreateHeap({heap_size}): {e}"))?;
                let heap = heap.ok_or("heap 为空")?;
                let mut texture: Option<ID3D12Resource> = None;
                self.device
                    .CreatePlacedResource(
                        &heap,
                        0,
                        &tex_desc,
                        D3D12_RESOURCE_STATE_COPY_DEST,
                        None,
                        &mut texture,
                    )
                    .map_err(|e| format!("CreatePlacedResource: {e}"))?;
                let texture = texture.ok_or("placed texture 为空")?;
                (texture, Some(heap), heap_size)
            } else {
                let mut texture: Option<ID3D12Resource> = None;
                self.device
                    .CreateCommittedResource(
                        &default_heap,
                        D3D12_HEAP_FLAG_SHARED,
                        &tex_desc,
                        D3D12_RESOURCE_STATE_COPY_DEST,
                        None,
                        &mut texture,
                    )
                    .map_err(|e| format!("CreateCommittedResource(texture): {e}"))?;
                (texture.ok_or("texture 为空")?, None, committed_alloc)
            };

            let row_pitch = ((width as usize) * 4).div_ceil(256) * 256;
            let upload_size = (row_pitch * height as usize) as u64;
            let buf_desc = D3D12_RESOURCE_DESC {
                Dimension: D3D12_RESOURCE_DIMENSION_BUFFER,
                Width: upload_size,
                Height: 1,
                DepthOrArraySize: 1,
                MipLevels: 1,
                Format: DXGI_FORMAT_UNKNOWN,
                SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
                Layout: D3D12_TEXTURE_LAYOUT_ROW_MAJOR,
                Flags: D3D12_RESOURCE_FLAG_NONE,
                ..Default::default()
            };
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
            // 本进程副本保留在 SharedTex(F1 wave.3 VK import 用),Drop 关闭。
            // 加堆腿时句柄指向堆(D3D12_HEAP import;presenter 端自建 placed resource)。
            // SAFETY: texture/heap/fence 存活;GENERIC_ALL 访问;DuplicateHandle 参数合法。
            let tex_handle = if let Some(h) = &heap {
                self.device
                    .CreateSharedHandle(h, None, GENERIC_ALL_ACCESS, PCWSTR::null())
                    .map_err(|e| format!("CreateSharedHandle(heap): {e}"))?
            } else {
                self.device
                    .CreateSharedHandle(&texture, None, GENERIC_ALL_ACCESS, PCWSTR::null())
                    .map_err(|e| format!("CreateSharedHandle(texture): {e}"))?
            };
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
            ok1.map_err(|e| format!("DuplicateHandle(texture): {e}"))?;
            ok2.map_err(|e| format!("DuplicateHandle(fence): {e}"))?;

            self.shared = Some(SharedTex {
                texture,
                heap,
                is_heap_handle: use_heap,
                upload,
                upload_ptr: upload_ptr as *mut u8,
                row_pitch,
                width,
                height,
                fence,
                fence_value: 0,
                fence_event,
                in_copy_dest: true,
                local_tex_handle: tex_handle,
                alloc_size,
                zc_transitioned: false,
            });
            Ok((dup_tex.0 as u64, dup_fence.0 as u64, use_heap))
        }
    }

    /// 写一帧 rgba8 → 共享纹理;fence 递增并等待 GPU 完成(有界 2s)。
    fn write(&mut self, rgba8: &[u8], width: u32, height: u32) -> Result<u64, String> {
        let Some(shared) = &mut self.shared else {
            return Err("共享纹理未打开".into());
        };
        if shared.width != width || shared.height != height {
            return Err(format!(
                "尺寸不符:share {}x{} ≠ frame {}x{}",
                shared.width, shared.height, width, height
            ));
        }
        let w = width as usize;
        let h = height as usize;
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

            if !shared.in_copy_dest {
                self.list.ResourceBarrier(&[barrier_transition(
                    &shared.texture,
                    D3D12_RESOURCE_STATE_COPY_SOURCE,
                    D3D12_RESOURCE_STATE_COPY_DEST,
                )]);
                shared.in_copy_dest = true;
            }

            let dst = D3D12_TEXTURE_COPY_LOCATION {
                pResource: std::mem::ManuallyDrop::new(Some(shared.texture.clone())),
                Type: D3D12_TEXTURE_COPY_TYPE_SUBRESOURCE_INDEX,
                Anonymous: D3D12_TEXTURE_COPY_LOCATION_0 {
                    SubresourceIndex: 0,
                },
            };
            let src = D3D12_TEXTURE_COPY_LOCATION {
                pResource: std::mem::ManuallyDrop::new(Some(shared.upload.clone())),
                Type: D3D12_TEXTURE_COPY_TYPE_PLACED_FOOTPRINT,
                Anonymous: D3D12_TEXTURE_COPY_LOCATION_0 {
                    PlacedFootprint: D3D12_PLACED_SUBRESOURCE_FOOTPRINT {
                        Offset: 0,
                        Footprint: D3D12_SUBRESOURCE_FOOTPRINT {
                            Format: DXGI_FORMAT_R8G8B8A8_UNORM,
                            Width: width,
                            Height: height,
                            Depth: 1,
                            RowPitch: shared.row_pitch as u32,
                        },
                    },
                },
            };
            self.list.CopyTextureRegion(&dst, 0, 0, 0, &src, None);
            self.list.ResourceBarrier(&[barrier_transition(
                &shared.texture,
                D3D12_RESOURCE_STATE_COPY_DEST,
                D3D12_RESOURCE_STATE_COPY_SOURCE,
            )]);
            shared.in_copy_dest = false;
            self.list.Close().map_err(|e| format!("list.Close: {e}"))?;
            let cmd: ID3D12CommandList = self.list.cast().map_err(|e| format!("list cast: {e}"))?;
            self.queue.ExecuteCommandLists(&[Some(cmd)]);

            shared.fence_value += 1;
            let v = shared.fence_value;
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

    /// 零拷贝帧信号(F1 wave.3 方向 B):VK 已直渲进共享纹理(session 帧 fence 完成
    /// 由 CPU 侧知悉),此处仅推进共享 fence 通知消费者;首次调用把纹理一次性迁移
    /// 到 COPY_SOURCE(与 signal 同队列顺序提交,消费者见 fence ≥ v 时迁移已完成)。
    /// 返回本帧 fence 值。
    fn signal(&mut self) -> Result<u64, String> {
        let Some(shared) = &mut self.shared else {
            return Err("共享纹理未打开".into());
        };
        unsafe {
            // SAFETY: 全部对象存活于 self;命令对按序配对;同队列序保证迁移先于信号。
            if !shared.zc_transitioned {
                self.allocator.Reset().map_err(|e| format!("allocator.Reset: {e}"))?;
                self.list
                    .Reset(&self.allocator, None)
                    .map_err(|e| format!("list.Reset: {e}"))?;
                if shared.in_copy_dest {
                    self.list.ResourceBarrier(&[barrier_transition(
                        &shared.texture,
                        D3D12_RESOURCE_STATE_COPY_DEST,
                        D3D12_RESOURCE_STATE_COPY_SOURCE,
                    )]);
                    shared.in_copy_dest = false;
                }
                self.list.Close().map_err(|e| format!("list.Close: {e}"))?;
                let cmd: ID3D12CommandList =
                    self.list.cast().map_err(|e| format!("list cast: {e}"))?;
                self.queue.ExecuteCommandLists(&[Some(cmd)]);
                shared.zc_transitioned = true;
            }
            shared.fence_value += 1;
            let v = shared.fence_value;
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

/// 打开/重建共享纹理并把句柄移交 target_pid;`min_alloc` = VK import 端需求(probe 实测,
/// 0 = 未知)。返回 (texHandle, fenceHandle, width, height, isHeapHandle)。
pub fn open(
    width: u32,
    height: u32,
    target_pid: u32,
    min_alloc: u64,
) -> Result<(u64, u64, u32, u32, bool), String> {
    let mut g = slot().lock().unwrap_or_else(|e| e.into_inner());
    if g.is_none() {
        *g = Some(Producer::new()?);
    }
    let p = g.as_mut().expect("producer 刚初始化");
    let (t, f, heap) = p.open(width, height, target_pid, min_alloc)?;
    Ok((t, f, width, height, heap))
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

/// 是否已打开共享纹理(供 viewport.frame 决定是否随帧写入)。
pub fn is_open() -> bool {
    let g = slot().lock().unwrap_or_else(|e| e.into_inner());
    g.as_ref().is_some_and(|p| p.shared.is_some())
}

/// VK import 面(F1 wave.3 方向 B):(本进程 NT handle, allocation_size, width, height,
/// is_heap)。未打开 = None。handle 生命周期随 SharedTex(close/重建即失效),调用方须
/// 即取即用。is_heap=true 时句柄指向共享堆(D3D12_HEAP import,placed resource 偏移 0)。
pub fn vk_import_info() -> Option<(u64, u64, u32, u32, bool)> {
    let g = slot().lock().unwrap_or_else(|e| e.into_inner());
    let p = g.as_ref()?;
    let s = p.shared.as_ref()?;
    Some((
        s.local_tex_handle.0 as u64,
        s.alloc_size,
        s.width,
        s.height,
        s.is_heap_handle,
    ))
}

/// 零拷贝帧信号:仅推进共享 fence(首调用一次性迁移纹理到 COPY_SOURCE)。
/// 共享未打开 = Ok(None)。
pub fn signal_frame() -> Result<Option<u64>, String> {
    let mut g = slot().lock().unwrap_or_else(|e| e.into_inner());
    let Some(p) = g.as_mut() else { return Ok(None) };
    if p.shared.is_none() {
        return Ok(None);
    }
    p.signal().map(Some)
}

/// 关闭共享纹理(幂等)。
pub fn close() {
    let mut g = slot().lock().unwrap_or_else(|e| e.into_inner());
    if let Some(p) = g.as_mut() {
        p.close();
    }
}
