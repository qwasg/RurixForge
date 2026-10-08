//! L1 帧通道(01 §3.5 X1 的 a2 变体、02 §4.2):只在 D3D12、且 Godot adapter 的 LUID 等于缺省 adapter
//! (presenter 用 `D3D12CreateDevice(None)` 取的那块卡)时启用。每帧:
//! 1. frame_post_draw(N):RD `texture_copy` 视口纹理 → 导入 RD 的中间纹理 `export_tex`(录进下一帧的 RD 命令缓冲);
//! 2. frame_post_draw(N+1):我方命令列表在 Godot 主队列上把 `export_tex` `CopyTextureRegion` 进共享 buffer
//!    (RGBA8、行距 256 对齐),再 `Signal(共享 fence, ++v)`。同一队列按提交顺序执行,读到的一定是 RD 拷贝之后的内容。
//!    所以 L1 比渲染晚 1 帧,交付的 seq 是上一帧的。
//!
//! 屏障:export_tex 在 RD 拷贝后处于 COPY_DEST;我方 COPY_DEST → COPY_SOURCE → 拷贝 → COPY_DEST,还原成 RD 认为的状态;
//! 屏障 API 与 Godot 驱动的选择一致(OPTIONS12.EnhancedBarriersSupported,01 §3.3)。
//! 全部对象只在 Godot 主线程使用。

use std::ffi::c_void;
use std::mem::transmute_copy;

use godot::classes::rendering_device::{DataFormat, DriverResource, TextureSamples, TextureType, TextureUsageBits};
use godot::classes::RenderingDevice;
use godot::prelude::*;
use windows::core::Interface;
use windows::Win32::Foundation::{CloseHandle, HANDLE, LUID};
use windows::Win32::Graphics::Direct3D12::*;
use windows::Win32::Graphics::Dxgi::Common::*;
use windows::Win32::Graphics::Dxgi::{CreateDXGIFactory1, IDXGIAdapter1};
use windows::Win32::System::Threading::{CreateEventW, WaitForSingleObject};

use engine_host::{DebugLayerStats, SharedTarget};

use crate::export::FrameMeta;

const SLOTS: usize = 3;

struct CmdSlot {
    alloc: ID3D12CommandAllocator,
    list: ID3D12GraphicsCommandList,
    /// 这个槽上一次提交后 signal 的私有 fence 值;复用前等它完成。
    used_at: u64,
}

struct Attached {
    width: u32,
    height: u32,
    row_pitch: u32,
    fence_value: &'static std::sync::atomic::AtomicU64,
    buffer: ID3D12Resource,
    shared_fence: ID3D12Fence,
    export_tex: ID3D12Resource,
    export_rd: Rid,
    /// 已录进 RD 的 texture_copy、还没拷进共享 buffer 的那一帧。
    pending: Option<FrameMeta>,
}

pub struct L1 {
    device: ID3D12Device,
    queue: ID3D12CommandQueue,
    enhanced: bool,
    slots: Vec<CmdSlot>,
    next: usize,
    fence: ID3D12Fence,
    fence_value: u64,
    event: HANDLE,
    target: Option<Attached>,
    info_queue: Option<ID3D12InfoQueue>,
}

fn luid_bytes(l: LUID) -> [u8; 8] {
    let mut out = [0u8; 8];
    out[..4].copy_from_slice(&l.LowPart.to_ne_bytes());
    out[4..].copy_from_slice(&l.HighPart.to_ne_bytes());
    out
}

/// 消费端(presenter / 其他 D3D12 读者)眼里的缺省 adapter:它们用 `D3D12CreateDevice(None)`。本进程不能直接取:
/// Godot 的 exe 导出了 NvOptimusEnablement / AmdPowerXpressRequestHighPerformance(GD/platform/windows/os_windows.cpp:101-102),
/// 系统因此把独显排到 Godot 进程的 0 号(实测:Godot 进程 0 号 = RTX 5060,普通进程 0 号 = Intel)。
/// 做法:用 rundll32(没有这些导出的普通进程)加载本 dll、调 `ForgeProbeDefaultAdapter`,让它报 `D3D12CreateDevice(None)` 的 LUID;
/// 探测失败才退回 `EnumAdapterByGpuPreference(0, MINIMUM_POWER)`(无偏好应用的系统缺省)。
pub fn consumer_adapter() -> Result<(IDXGIAdapter1, LUID, String), String> {
    use windows::Win32::Graphics::Dxgi::{IDXGIFactory4, IDXGIFactory6, DXGI_GPU_PREFERENCE_MINIMUM_POWER};
    static PROBED: std::sync::OnceLock<Option<LUID>> = std::sync::OnceLock::new();
    let probed = *PROBED.get_or_init(probe_plain_process_luid);
    unsafe {
        let name = |a: &IDXGIAdapter1| -> Result<(LUID, String), String> {
            let d = a.GetDesc1().map_err(|e| format!("GetDesc1: {e}"))?;
            let n = d.Description.iter().position(|c| *c == 0).unwrap_or(d.Description.len());
            Ok((d.AdapterLuid, String::from_utf16_lossy(&d.Description[..n])))
        };
        if let Some(l) = probed {
            let f: IDXGIFactory4 = CreateDXGIFactory1().map_err(|e| format!("CreateDXGIFactory1: {e}"))?;
            if let Ok(a) = f.EnumAdapterByLuid::<IDXGIAdapter1>(l) {
                let (l, n) = name(&a)?;
                return Ok((a, l, n));
            }
        }
        let f: IDXGIFactory6 = CreateDXGIFactory1().map_err(|e| format!("CreateDXGIFactory1: {e}"))?;
        let a: IDXGIAdapter1 = f
            .EnumAdapterByGpuPreference(0, DXGI_GPU_PREFERENCE_MINIMUM_POWER)
            .map_err(|e| format!("EnumAdapterByGpuPreference(MINIMUM_POWER): {e}"))?;
        let (l, n) = name(&a)?;
        Ok((a, l, n))
    }
}

/// 跑 `rundll32 <bin\godot_host.dll>,ForgeProbeDefaultAdapter <临时文件>`,读回普通进程的缺省 adapter LUID。
fn probe_plain_process_luid() -> Option<LUID> {
    let dll = std::env::current_exe().ok()?.parent()?.join("bin").join("godot_host.dll");
    let out = std::env::temp_dir().join(format!("forge-adapter-probe-{}.txt", std::process::id()));
    let _ = std::fs::remove_file(&out);
    let mut child = std::process::Command::new("rundll32.exe")
        .arg(format!("{},ForgeProbeDefaultAdapter", dll.display()))
        .arg(&out)
        .spawn()
        .ok()?;
    let t0 = std::time::Instant::now();
    while t0.elapsed() < std::time::Duration::from_secs(5) {
        if let Ok(Some(_)) = child.try_wait() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    let _ = child.kill();
    let text = std::fs::read_to_string(&out).ok()?;
    let _ = std::fs::remove_file(&out);
    let v = u64::from_str_radix(text.trim(), 16).ok()?;
    Some(LUID { LowPart: v as u32, HighPart: (v >> 32) as i32 })
}

/// rundll32 入口(签名 = RUNDLL32 约定):在调用它的普通进程里取 `D3D12CreateDevice(None)` 的 adapter LUID,
/// 以 16 位十六进制(高 32 位在前)写到命令行给的文件。只给 consumer_adapter 用。
#[no_mangle]
pub extern "system" fn ForgeProbeDefaultAdapter(_hwnd: isize, _hinst: isize, cmdline: *const std::ffi::c_char, _show: i32) {
    if cmdline.is_null() {
        return;
    }
    // SAFETY: rundll32 传入以 NUL 结尾的 ANSI 命令行。
    let path = unsafe { std::ffi::CStr::from_ptr(cmdline) }.to_string_lossy().trim().trim_matches('"').to_string();
    let mut dev: Option<ID3D12Device> = None;
    // SAFETY: 标准 D3D12 初始化;失败就什么都不写。
    if unsafe { D3D12CreateDevice(None, windows::Win32::Graphics::Direct3D::D3D_FEATURE_LEVEL_11_0, &mut dev) }.is_err() {
        return;
    }
    if let Some(d) = dev {
        let l = unsafe { d.GetAdapterLuid() };
        let v = (u64::from(l.HighPart as u32) << 32) | u64::from(l.LowPart);
        let _ = std::fs::write(path, format!("{v:016x}"));
    }
}

/// 给 engine-host 的共享 buffer 模块装 device(02 §4.2 第 1-2 步):Godot 的 device 与消费端同卡 → 装它(L1 可写,返回 true);
/// 否则在消费端的 adapter 上另建一个 device 装进去(只走 L2 的 CPU 上传档,presenter 仍打得开共享对象,返回 false)。
pub fn install_share_device(godot_device: Option<&ID3D12Device>) -> Result<bool, String> {
    let (adapter, luid, name) = consumer_adapter()?;
    unsafe {
        if let Some(d) = godot_device {
            if luid_bytes(d.GetAdapterLuid()) == luid_bytes(luid) {
                engine_host::install_d3d12_device(d.clone().into_raw() as usize, true)?;
                return Ok(true);
            }
        }
        let mut dev: Option<ID3D12Device> = None;
        windows::Win32::Graphics::Direct3D12::D3D12CreateDevice(&adapter, windows::Win32::Graphics::Direct3D::D3D_FEATURE_LEVEL_11_0, &mut dev)
            .map_err(|e| format!("D3D12CreateDevice({name}): {e}"))?;
        let dev = dev.ok_or("D3D12CreateDevice 返回空")?;
        engine_host::install_d3d12_device(dev.into_raw() as usize, false)?;
        Ok(false)
    }
}

/// Godot D3D12 驱动按 `EnumAdapterByGpuPreference(i, HIGH_PERFORMANCE)` 编号(rendering_context_driver_d3d12.cpp:365),
/// 与 DXGI 的 EnumAdapters1 顺序不同。返回让 Godot 选中缺省 adapter 的 `--gpu-index`(按运行时实际枚举结果)。
pub fn gpu_index_for_default_adapter() -> Option<u32> {
    use windows::Win32::Graphics::Dxgi::{IDXGIFactory6, DXGI_GPU_PREFERENCE_HIGH_PERFORMANCE};
    let (_, want, _) = consumer_adapter().ok()?;
    unsafe {
        let f: IDXGIFactory6 = CreateDXGIFactory1().ok()?;
        for i in 0..16u32 {
            let a: IDXGIAdapter1 = f.EnumAdapterByGpuPreference(i, DXGI_GPU_PREFERENCE_HIGH_PERFORMANCE).ok()?;
            let d = a.GetDesc1().ok()?;
            if luid_bytes(d.AdapterLuid) == luid_bytes(want) {
                return Some(i);
            }
        }
    }
    None
}

/// 借用 RD 返回的原生指针(不接管引用),clone 出一份自己的 COM 引用。
unsafe fn borrow<T: Interface + Clone>(raw: u64, what: &str) -> Result<T, String> {
    let p = raw as *mut c_void;
    T::from_raw_borrowed(&p).cloned().ok_or_else(|| format!("get_driver_resource({what}) 返回空"))
}

impl L1 {
    /// 取 Godot 的 device / 主队列并比较 LUID;相同 → 把 device 装进 engine-host 的共享 buffer 模块(02 §4.2 第 1 步)。
    /// Err = L1 不可用的原因(调用方只走 L2)。
    pub fn probe(rd: &Gd<RenderingDevice>) -> Result<L1, String> {
        unsafe {
            let device: ID3D12Device = borrow(rd.get_driver_resource(DriverResource::LOGICAL_DEVICE, Rid::Invalid, 0), "LOGICAL_DEVICE")?;
            let queue: ID3D12CommandQueue = borrow(rd.get_driver_resource(DriverResource::COMMAND_QUEUE, Rid::Invalid, 0), "COMMAND_QUEUE")?;
            let godot_luid = device.GetAdapterLuid();
            let (_, default_luid, default_name) = consumer_adapter()?;
            if luid_bytes(godot_luid) != luid_bytes(default_luid) {
                let hint = gpu_index_for_default_adapter()
                    .map(|i| format!(";用 --gpu-index {i} 启动可让 Godot 与缺省 adapter 对齐"))
                    .unwrap_or_default();
                return Err(format!(
                    "LUID 不一致:Godot adapter {:?} ≠ 缺省 adapter {:?}({default_name}),presenter 打不开 Godot device 上的共享对象,只走 L2{hint}",
                    luid_bytes(godot_luid), luid_bytes(default_luid)
                ));
            }
            let mut o12 = D3D12_FEATURE_DATA_D3D12_OPTIONS12::default();
            let enhanced = device
                .CheckFeatureSupport(D3D12_FEATURE_D3D12_OPTIONS12, &mut o12 as *mut _ as *mut c_void, std::mem::size_of_val(&o12) as u32)
                .is_ok()
                && o12.EnhancedBarriersSupported.as_bool();
            let mut slots = Vec::with_capacity(SLOTS);
            for _ in 0..SLOTS {
                let alloc: ID3D12CommandAllocator =
                    device.CreateCommandAllocator(D3D12_COMMAND_LIST_TYPE_DIRECT).map_err(|e| format!("CreateCommandAllocator: {e}"))?;
                let list: ID3D12GraphicsCommandList =
                    device.CreateCommandList(0, D3D12_COMMAND_LIST_TYPE_DIRECT, &alloc, None).map_err(|e| format!("CreateCommandList: {e}"))?;
                list.Close().map_err(|e| format!("list.Close: {e}"))?;
                slots.push(CmdSlot { alloc, list, used_at: 0 });
            }
            let fence: ID3D12Fence = device.CreateFence(0, D3D12_FENCE_FLAG_NONE).map_err(|e| format!("CreateFence: {e}"))?;
            let event = CreateEventW(None, false, false, None).map_err(|e| format!("CreateEventW: {e}"))?;
            // 只有开了 debug layer(--gpu-validation)的 device 才能 QI 出 InfoQueue。
            let info_queue = device.cast::<ID3D12InfoQueue>().ok();
            Ok(L1 { device, queue, enhanced, slots, next: 0, fence, fence_value: 0, event, target: None, info_queue })
        }
    }

    pub fn device(&self) -> &ID3D12Device {
        &self.device
    }

    pub fn enhanced_barriers(&self) -> bool {
        self.enhanced
    }

    /// 等我方已提交的命令全部执行完(有界 2 s)。
    fn wait_idle(&self, value: u64) {
        unsafe {
            if self.fence.GetCompletedValue() >= value {
                return;
            }
            if self.fence.SetEventOnCompletion(value, self.event).is_ok() {
                let _ = WaitForSingleObject(self.event, 2000);
            }
        }
    }
}


impl L1 {
    /// ShareAttach:接管 SharedTarget 里 AddRef 过的 buffer / fence,建同尺寸的 export_tex 并导入 RD。
    pub fn attach(&mut self, rd: &mut Gd<RenderingDevice>, t: SharedTarget) -> Result<(), String> {
        self.detach(rd);
        unsafe {
            // SAFETY: 两个指针是 share::gpu_target 给的已 AddRef 引用,所有权随 from_raw 移交。
            let buffer = ID3D12Resource::from_raw(t.buffer as *mut c_void);
            let shared_fence = ID3D12Fence::from_raw(t.fence as *mut c_void);
            let desc = D3D12_RESOURCE_DESC {
                Dimension: D3D12_RESOURCE_DIMENSION_TEXTURE2D,
                Width: u64::from(t.width),
                Height: t.height,
                DepthOrArraySize: 1,
                MipLevels: 1,
                Format: DXGI_FORMAT_R8G8B8A8_UNORM,
                SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
                Layout: D3D12_TEXTURE_LAYOUT_UNKNOWN,
                Flags: D3D12_RESOURCE_FLAG_ALLOW_RENDER_TARGET,
                ..Default::default()
            };
            let heap = D3D12_HEAP_PROPERTIES { Type: D3D12_HEAP_TYPE_DEFAULT, ..Default::default() };
            // 初始状态随 Godot 驱动的屏障模型:
            // - legacy:texture_create_from_extension 假定导入纹理处于 RENDER_TARGET(rendering_device_driver_d3d12.cpp),按它创建;
            // - enhanced:legacy 创建的纹理要与 enhanced Barrier 互操作,必须处于 COMMON(debug layer id=1350,Stage 3 实测),
            //   RD 的 graph 首次使用时从 UNDEFINED 转换,不依赖这个初始状态。
            let initial = if self.enhanced { D3D12_RESOURCE_STATE_COMMON } else { D3D12_RESOURCE_STATE_RENDER_TARGET };
            let mut tex: Option<ID3D12Resource> = None;
            self.device
                .CreateCommittedResource(&heap, D3D12_HEAP_FLAG_NONE, &desc, initial, None, &mut tex)
                .map_err(|e| format!("CreateCommittedResource(export_tex): {e}"))?;
            let export_tex = tex.ok_or("export_tex 为空")?;
            let export_rd = rd.texture_create_from_extension(
                TextureType::TYPE_2D,
                DataFormat::R8G8B8A8_UNORM,
                TextureSamples::SAMPLES_1,
                TextureUsageBits::CAN_COPY_TO_BIT | TextureUsageBits::CAN_COPY_FROM_BIT,
                export_tex.as_raw() as u64,
                u64::from(t.width),
                u64::from(t.height),
                1,
                1,
            );
            if !export_rd.is_valid() {
                return Err("texture_create_from_extension(export_tex) 返回无效 RID".into());
            }
            self.target = Some(Attached {
                width: t.width,
                height: t.height,
                row_pitch: t.row_pitch,
                fence_value: t.fence_value,
                buffer,
                shared_fence,
                export_tex,
                export_rd,
                pending: None,
            });
        }
        Ok(())
    }

    /// ShareDetach / 重开 / 降级:等我方命令执行完,释放 RD 导入与 COM 引用。
    pub fn detach(&mut self, rd: &mut Gd<RenderingDevice>) {
        if let Some(a) = self.target.take() {
            self.wait_idle(self.fence_value);
            rd.free_rid(a.export_rd);
            drop(a); // Release buffer / 共享 fence / export_tex
        }
    }

    /// 第 1 段(frame_post_draw(N)):RD 把视口纹理拷进 export_tex。尺寸不等于共享 buffer 时不走 L1(Ok(false))。
    pub fn stage1(&mut self, rd: &mut Gd<RenderingDevice>, vp_rd: Rid, meta: &FrameMeta) -> Result<bool, String> {
        let Some(a) = self.target.as_mut() else { return Ok(false) };
        if (meta.width, meta.height) != (a.width, a.height) {
            return Ok(false);
        }
        let size = Vector3::new(a.width as f32, a.height as f32, 1.0);
        let err = rd.texture_copy(vp_rd, a.export_rd, Vector3::ZERO, Vector3::ZERO, size, 0, 0, 0, 0);
        if err != godot::global::Error::OK {
            return Err(format!("RD texture_copy(视口 → export_tex): {err:?}"));
        }
        a.pending = Some(meta.clone());
        Ok(true)
    }

    fn barrier(&self, list: &ID3D12GraphicsCommandList, tex: &ID3D12Resource, to_source: bool) -> Result<(), String> {
        unsafe {
            if self.enhanced {
                let list7: ID3D12GraphicsCommandList7 = list.cast().map_err(|e| format!("CommandList7: {e}"))?;
                let (sb, ab, lb, sa, aa, la) = if to_source {
                    (D3D12_BARRIER_SYNC_NONE, D3D12_BARRIER_ACCESS_NO_ACCESS, D3D12_BARRIER_LAYOUT_COPY_DEST,
                     D3D12_BARRIER_SYNC_COPY, D3D12_BARRIER_ACCESS_COPY_SOURCE, D3D12_BARRIER_LAYOUT_COPY_SOURCE)
                } else {
                    (D3D12_BARRIER_SYNC_COPY, D3D12_BARRIER_ACCESS_COPY_SOURCE, D3D12_BARRIER_LAYOUT_COPY_SOURCE,
                     D3D12_BARRIER_SYNC_NONE, D3D12_BARRIER_ACCESS_NO_ACCESS, D3D12_BARRIER_LAYOUT_COPY_DEST)
                };
                let tb = D3D12_TEXTURE_BARRIER {
                    SyncBefore: sb, SyncAfter: sa, AccessBefore: ab, AccessAfter: aa, LayoutBefore: lb, LayoutAfter: la,
                    pResource: transmute_copy(tex),
                    Subresources: D3D12_BARRIER_SUBRESOURCE_RANGE { IndexOrFirstMipLevel: 0xffff_ffff, ..Default::default() },
                    Flags: D3D12_TEXTURE_BARRIER_FLAG_NONE,
                };
                let group = D3D12_BARRIER_GROUP {
                    Type: D3D12_BARRIER_TYPE_TEXTURE,
                    NumBarriers: 1,
                    Anonymous: D3D12_BARRIER_GROUP_0 { pTextureBarriers: &tb },
                };
                list7.Barrier(&[group]);
            } else {
                let (before, after) = if to_source {
                    (D3D12_RESOURCE_STATE_COPY_DEST, D3D12_RESOURCE_STATE_COPY_SOURCE)
                } else {
                    (D3D12_RESOURCE_STATE_COPY_SOURCE, D3D12_RESOURCE_STATE_COPY_DEST)
                };
                let b = D3D12_RESOURCE_BARRIER {
                    Type: D3D12_RESOURCE_BARRIER_TYPE_TRANSITION,
                    Anonymous: D3D12_RESOURCE_BARRIER_0 {
                        Transition: std::mem::ManuallyDrop::new(D3D12_RESOURCE_TRANSITION_BARRIER {
                            pResource: transmute_copy(tex),
                            Subresource: D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES,
                            StateBefore: before,
                            StateAfter: after,
                        }),
                    },
                    ..Default::default()
                };
                list.ResourceBarrier(&[b]);
            }
        }
        Ok(())
    }
}


impl L1 {
    /// 第 2 段(frame_post_draw(N+1)):上一帧的 RD 拷贝已随 frame N+1 的命令缓冲提交;在同一主队列上
    /// export_tex → 共享 buffer(PLACED_FOOTPRINT),再推进共享 fence。返回写进共享 buffer 的那一帧。
    pub fn stage2(&mut self) -> Result<Option<FrameMeta>, String> {
        let Some(meta) = self.target.as_mut().and_then(|a| a.pending.take()) else { return Ok(None) };
        let i = self.next;
        self.next = (self.next + 1) % SLOTS;
        self.wait_idle(self.slots[i].used_at);
        let a = self.target.as_ref().expect("pending 存在则 target 存在");
        let slot = &self.slots[i];
        unsafe {
            slot.alloc.Reset().map_err(|e| format!("allocator.Reset: {e}"))?;
            slot.list.Reset(&slot.alloc, None).map_err(|e| format!("list.Reset: {e}"))?;
            self.barrier(&slot.list, &a.export_tex, true)?;
            let dst = D3D12_TEXTURE_COPY_LOCATION {
                pResource: transmute_copy(&a.buffer),
                Type: D3D12_TEXTURE_COPY_TYPE_PLACED_FOOTPRINT,
                Anonymous: D3D12_TEXTURE_COPY_LOCATION_0 {
                    PlacedFootprint: D3D12_PLACED_SUBRESOURCE_FOOTPRINT {
                        Offset: 0,
                        Footprint: D3D12_SUBRESOURCE_FOOTPRINT {
                            Format: DXGI_FORMAT_R8G8B8A8_UNORM,
                            Width: a.width,
                            Height: a.height,
                            Depth: 1,
                            RowPitch: a.row_pitch,
                        },
                    },
                },
            };
            let src = D3D12_TEXTURE_COPY_LOCATION {
                pResource: transmute_copy(&a.export_tex),
                Type: D3D12_TEXTURE_COPY_TYPE_SUBRESOURCE_INDEX,
                Anonymous: D3D12_TEXTURE_COPY_LOCATION_0 { SubresourceIndex: 0 },
            };
            slot.list.CopyTextureRegion(&dst, 0, 0, 0, &src, None);
            self.barrier(&slot.list, &a.export_tex, false)?;
            slot.list.Close().map_err(|e| format!("list.Close: {e}"))?;
            let cmd: ID3D12CommandList = slot.list.cast().map_err(|e| format!("list cast: {e}"))?;
            self.queue.ExecuteCommandLists(&[Some(cmd)]);
            // 共享 fence 与 Producer 共用一个值序列(share.rs FENCE_VALUE):L1 期间只有这里推进,每帧 +1。
            let v = a.fence_value.fetch_add(1, std::sync::atomic::Ordering::SeqCst) + 1;
            self.queue.Signal(&a.shared_fence, v).map_err(|e| format!("Signal(共享 fence): {e}"))?;
            self.fence_value += 1;
            self.queue.Signal(&self.fence, self.fence_value).map_err(|e| format!("Signal(私有 fence): {e}"))?;
        }
        self.slots[i].used_at = self.fence_value;
        Ok(Some(meta))
    }

    /// debug layer 存量消息统计(只有 --gpu-validation 时有 InfoQueue);Godot 的存储过滤器屏蔽了 INFO。
    pub fn debug_stats(&self) -> Option<DebugLayerStats> {
        let q = self.info_queue.as_ref()?;
        let mut s = DebugLayerStats::default();
        unsafe {
            let n = q.GetNumStoredMessages();
            for i in 0..n {
                let mut len = 0usize;
                if q.GetMessage(i, None, &mut len).is_err() || len == 0 {
                    continue;
                }
                let mut buf = vec![0u64; len.div_ceil(8)];
                let msg = buf.as_mut_ptr() as *mut D3D12_MESSAGE;
                if q.GetMessage(i, Some(msg), &mut len).is_err() {
                    continue;
                }
                match (*msg).Severity {
                    D3D12_MESSAGE_SEVERITY_CORRUPTION => s.corruption += 1,
                    D3D12_MESSAGE_SEVERITY_ERROR => s.errors += 1,
                    D3D12_MESSAGE_SEVERITY_WARNING => s.warnings += 1,
                    _ => {}
                }
            }
        }
        Some(s)
    }

    /// error / corruption(以及 warn = true 时的 warning)级消息的正文(供汇报与测试断言;最多 n 条)。
    pub fn debug_messages(&self, n: usize, warn: bool) -> Vec<String> {
        let Some(q) = self.info_queue.as_ref() else { return Vec::new() };
        let mut out = Vec::new();
        unsafe {
            for i in 0..q.GetNumStoredMessages() {
                let mut len = 0usize;
                if q.GetMessage(i, None, &mut len).is_err() || len == 0 {
                    continue;
                }
                let mut buf = vec![0u64; len.div_ceil(8)];
                let msg = buf.as_mut_ptr() as *mut D3D12_MESSAGE;
                let sev = if q.GetMessage(i, Some(msg), &mut len).is_ok() { (*msg).Severity } else { continue };
                let wanted = matches!(sev, D3D12_MESSAGE_SEVERITY_ERROR | D3D12_MESSAGE_SEVERITY_CORRUPTION)
                    || (warn && sev == D3D12_MESSAGE_SEVERITY_WARNING);
                if wanted {
                    let d = std::slice::from_raw_parts((*msg).pDescription, (*msg).DescriptionByteLength.saturating_sub(1));
                    let tag = match sev {
                        D3D12_MESSAGE_SEVERITY_CORRUPTION => "CORRUPTION",
                        D3D12_MESSAGE_SEVERITY_ERROR => "ERROR",
                        _ => "WARNING",
                    };
                    out.push(format!("{tag} id={} {}", (*msg).ID.0, String::from_utf8_lossy(d)));
                    if out.len() >= n {
                        break;
                    }
                }
            }
        }
        out
    }
}

impl Drop for L1 {
    fn drop(&mut self) {
        self.wait_idle(self.fence_value);
        unsafe {
            let _ = CloseHandle(self.event);
        }
    }
}

/// L1 不可用时收到的 SharedTarget:只释放它带来的两份 COM 引用,不泄漏。
pub fn release_target(t: SharedTarget) {
    unsafe {
        // SAFETY: 与 attach 相同的所有权约定(share::gpu_target 已 AddRef)。
        drop(ID3D12Resource::from_raw(t.buffer as *mut c_void));
        drop(ID3D12Fence::from_raw(t.fence as *mut c_void));
    }
}
