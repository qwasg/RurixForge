//! RFC-0054 official Streamline 2.14.16 Vulkan RR boundary (U63).
//! Input is one current noisy scene-linear frame; output is linear HDR.
//! Host and external-buffer modes use the same owned images and resource tags.
//! Assisted-by: Codex:GPT-6.
use super::*;
use std::time::Instant;
#[path = "vendor_rr_shutdown.rs"]
mod rr_shutdown;
#[path = "vendor_rr_present.rs"]
pub(super) mod rr_present;
pub use rr_present::PresentReport as RrPresentReport;

const FEATURE: u32 = 1001;
// Session-owned textures remain allocated throughout all vendor use. Refresh
// actual content/layout tags after uploads; do not request volatile cloning.
const RR_LIFECYCLE_SESSION: i32 = 1; // sl::eValidUntilPresent
const GUID_OPTIONS: SlStructType = sl_guid(
    0x0ad87504,
    0x774e,
    0x4bf3,
    [0x96, 0x33, 0xa4, 0x4d, 0x1f, 0x7f, 0x9c, 0xb8],
);
const GUID_OPTIMAL: SlStructType = sl_guid(
    0xfbd0c637,
    0xa28f,
    0x41f2,
    [0xbc, 0x91, 0xb4, 0x21, 0xfa, 0xee, 0x8e, 0x1e],
);
const GUID_STATE: SlStructType = sl_guid(
    0x71873c14,
    0xf8ca,
    0x4767,
    [0x9e, 0xaf, 0x3b, 0x43, 0x93, 0xea, 0x98, 0xfa],
);
const GUID_ADAPTER: SlStructType = sl_guid(
    0x0677315f,
    0xa746,
    0x4492,
    [0x9f, 0x42, 0xcb, 0x61, 0x42, 0xc9, 0xc3, 0xd4],
);
const FORMAT_R32: u32 = 100;
const FORMAT_RGBA32: u32 = 109;
// Official Vulkan VkStructureType value, independent of the older SR helper.
const RR_QUERY_POOL_CREATE_INFO: u32 = 11;
struct ProcessState {
    legacy_sessions: usize,
    rr_active: bool,
}
impl ProcessState {
    fn acquire(&mut self,rr:bool)->bool {
        if self.rr_active||(rr&&self.legacy_sessions!=0){return false;}
        if rr{self.rr_active=true;}else{self.legacy_sessions+=1;}true
    }
    fn release(&mut self,rr:bool,retained:bool){
        if retained{return;}
        if rr{self.rr_active=false;}else{self.legacy_sessions=self.legacy_sessions.saturating_sub(1);}
    }
}
static PROCESS_STATE: std::sync::Mutex<ProcessState> = std::sync::Mutex::new(ProcessState {
    legacy_sessions: 0,
    rr_active: false,
});
pub(super) struct SessionLease {
    rr: bool,
    retained: bool,
}
impl SessionLease {
    pub(super) fn acquire(rr: bool) -> Result<Self, VendorError> {
        let mut state = PROCESS_STATE
            .lock()
            .map_err(|_| bad("SL session latch is poisoned"))?;
        if !state.acquire(rr) {
            return Err(bad(
                "RR and SR Streamline sessions cannot coexist in one process",
            ));
        }
        Ok(Self { rr, retained: false })
    }
}
impl SessionLease { pub(super) fn retain(&mut self) {self.retained=true;} }
impl Drop for SessionLease {
    fn drop(&mut self) {
        if let Ok(mut state) = PROCESS_STATE.lock() {state.release(self.rr,self.retained);}
    }
}
// Production binaries from the verified official v2.14.16 release archive.
// Provenance: artifacts/day_1020_dlss_rr/SDK.json and sdk_abi_probe_all.txt.
const PRODUCTION_DLLS: [(&str, &str); 6] = [
    (
        "sl.interposer.dll",
        "b1d7395aebd7b9722841d64dcf7433680020b6a8e421f851c16d5ba0c2182edf",
    ),
    (
        "sl.common.dll",
        "38eea3b4209afc455636ea372b5933d65fcc13becec6c84d101dcfc4d8648f75",
    ),
    (
        "sl.dlss.dll",
        "18871eea87c90345da97d8109faa81ea7d5840e8d99b51b55b0e46cfa177f84e",
    ),
    (
        "sl.dlss_d.dll",
        "8dff36fd063094eb1f659e8d4dcd52c7423859a9c4a3df3796084c48412d60ac",
    ),
    (
        "nvngx_dlss.dll",
        "d00b5b0ab29c3ca101b70c3a5ecb3a01b7ef33766a3383a28a18623083e6123e",
    ),
    (
        "nvngx_dlssd.dll",
        "b49435ae346f227feafe7bbe937d88277ea0ee7323975ec06488f377fdb46cee",
    ),
];

#[repr(C)]
struct SlRrOptions {
    base: SlBaseStructure,
    mode: u32,
    output_width: u32,
    output_height: u32,
    sharpness: f32,
    pre_exposure: f32,
    exposure_scale: f32,
    color_buffers_hdr: i8,
    indicator_x: i8,
    indicator_y: i8,
    pad0: u8,
    normal_roughness_mode: u32,
    world_to_view: [[f32; 4]; 4],
    view_to_world: [[f32; 4]; 4],
    alpha_upscaling: i8,
    pad1: [u8; 3],
    presets: [u32; 6],
}
#[repr(C)]
struct SlRrOptimal {
    base: SlBaseStructure,
    width: u32,
    height: u32,
    sharpness: f32,
    min_w: u32,
    min_h: u32,
    max_w: u32,
    max_h: u32,
}
#[repr(C)]
struct SlRrState {
    base: SlBaseStructure,
    vram: u64,
}
#[repr(C)]
struct SlAdapter {
    base: SlBaseStructure,
    luid: *mut u8,
    luid_size: u32,
    pad: u32,
    physical: *mut c_void,
}
const _: () = assert!(size_of::<SlRrOptions>() == 224);
const _: () = assert!(std::mem::offset_of!(SlRrOptions, normal_roughness_mode) == 60);
const _: () = assert!(std::mem::offset_of!(SlRrOptions, world_to_view) == 64);
const _: () = assert!(std::mem::offset_of!(SlRrOptions, view_to_world) == 128);
const _: () = assert!(std::mem::offset_of!(SlRrOptions, alpha_upscaling) == 192);
const _: () = assert!(std::mem::offset_of!(SlRrOptions, presets) == 196);
const _: () = assert!(size_of::<SlRrOptimal>() == 64);
const _: () = assert!(size_of::<SlRrState>() == 40);
const _: () = assert!(size_of::<SlAdapter>() == 56);
const _: () = assert!(std::mem::offset_of!(SlAdapter, physical) == 48);
type SetOptions =
    unsafe extern "system" fn(*const SlViewportHandle, *const SlRrOptions) -> SlResult;
type GetOptimal = unsafe extern "system" fn(*const SlRrOptions, *mut SlRrOptimal) -> SlResult;
type GetState = unsafe extern "system" fn(*const SlViewportHandle, *mut SlRrState) -> SlResult;
type Supported = unsafe extern "system" fn(u32, *const SlAdapter) -> SlResult;
type SetTagsForFrame = unsafe extern "system" fn(
    *const c_void,
    *const SlViewportHandle,
    *const SlResourceTag,
    u32,
    *mut c_void,
) -> SlResult;
type AllocateResources =
    unsafe extern "system" fn(*mut c_void, u32, *const SlViewportHandle) -> SlResult;

#[repr(C)]
struct MemoryBudget {
    s_type: u32,
    p_next: *mut c_void,
    budget: [u64; 16],
    usage: [u64; 16],
}
#[repr(C)]
struct MemoryProperties2 {
    s_type: u32,
    p_next: *mut c_void,
    properties: [u64; 65],
}
const _: () = assert!(size_of::<MemoryBudget>() == 272);
const _: () = assert!(std::mem::offset_of!(MemoryBudget, budget) == 16);
const _: () = assert!(std::mem::offset_of!(MemoryBudget, usage) == 144);
const _: () = assert!(size_of::<MemoryProperties2>() == 536);

#[repr(C)]
pub(super) struct PrivateDataFeatures {
    s_type: u32,
    pub(super) p_next: *mut c_void,
    private_data: u32,
}
#[repr(C)]
struct Features2 {
    s_type: u32,
    p_next: *mut c_void,
    features: [u32; 55],
}
const _: () = assert!(size_of::<PrivateDataFeatures>() == 24);
const _: () = assert!(std::mem::offset_of!(PrivateDataFeatures, private_data) == 16);
const _: () = assert!(size_of::<Features2>() == 240);
pub(super) fn query_private_data_features(
    fns: SlVkFns,
    instance: VkInstance,
    physical: VkPhysicalDevice,
) -> Result<PrivateDataFeatures, VendorError> {
    type Get = unsafe extern "system" fn(VkPhysicalDevice, *mut Features2);
    // SAFETY: U63, live SL Vulkan instance and exact Vulkan1.1 core symbol name.
    let address = unsafe { (fns.vk_gipa)(instance, c"vkGetPhysicalDeviceFeatures2".as_ptr()) };
    // SAFETY: U63, exact signature and null-checked dispatch resolution.
    let get = unsafe { cast_sym::<Get>(address) }
        .ok_or_else(|| VendorError::SymbolMissing("vkGetPhysicalDeviceFeatures2(RR)".into()))?;
    let mut feature = PrivateDataFeatures {
        s_type: 1_000_295_000,
        p_next: std::ptr::null_mut(),
        private_data: 0,
    };
    let mut query = Features2 {
        s_type: 1_000_059_000,
        p_next: (&mut feature as *mut PrivateDataFeatures).cast(),
        features: [0; 55],
    };
    // SAFETY: U63, verified feature2+privateData structure chain is writable and alive through the query.
    unsafe { get(physical, &mut query) };
    if feature.private_data != 1 {
        return Err(VendorError::DeviceUnavailable(
            "RR Streamline state tracking requires probed privateData=true".into(),
        ));
    }
    eprintln!("HQ_RR_CAPABILITY privateData=1 source=vkGetPhysicalDeviceFeatures2");
    Ok(feature)
}

// RR-only transaction around the existing SL/Vulkan bootstrap. Successful
// construction transfers every tracked object into DlssVkSession and disarms it.
pub(super) struct BootstrapGuard {
    fns: SlVkFns,
    armed: bool,
    pub(super) instance: VkInstance,
    pub(super) device: VkDevice,
    pub(super) dev: Option<VkDevFns>,
    pub(super) messenger: VkDebugUtilsMessenger,
    pub(super) validation_counter: *mut ValidationCounts,
    pub(super) cmd_pool: VkCommandPool,
    pub(super) images: Vec<(VkImage, VkDeviceMemory, VkImageView)>,
    pub(super) buffers: Vec<(VkBuffer, VkDeviceMemory)>,
    pub(super) present: Option<rr_present::Present>,
    pub(super) lease: Option<SessionLease>,
}
// U63: this production helper owns the only RefMut, validates both slots before
// either take, and returns values rather than a borrow. Its RefMut ends before
// a caller can start constructing the compatible core struct literal.
fn take_bootstrap_owner_pair<G,A,B>(
    owner:&std::cell::RefCell<Option<G>>,
    fields:impl for<'a> FnOnce(&'a mut G)->(&'a mut Option<A>,&'a mut Option<B>),
)->Result<(A,B),VendorError>{
    let mut borrowed=owner.try_borrow_mut().map_err(|_|bad("RR bootstrap owner transfer already borrowed"))?;
    let guard=borrowed.as_mut().ok_or_else(||bad("RR bootstrap guard absent during owner transfer"))?;
    let (present,lease)=fields(guard);
    if present.is_none()||lease.is_none(){return Err(bad("RR bootstrap owner transfer requires both WSI owner and SDK lease"));}
    // Both slots remain exclusively borrowed and no fallible/native operation
    // occurs between this check and the two moves.
    let present=present.take().expect("checked WSI owner");
    let lease=lease.take().expect("checked SDK lease");
    drop(borrowed);
    Ok((present,lease))
}
impl BootstrapGuard {
    pub(super) fn transfer_owners(owner:&std::cell::RefCell<Option<Self>>)->Result<(rr_present::Present,SessionLease),VendorError>{
        take_bootstrap_owner_pair(owner,|guard|(&mut guard.present,&mut guard.lease))
    }
    pub(super) fn new(fns: SlVkFns) -> Self {
        Self {
            fns,
            armed: true,
            instance: std::ptr::null_mut(),
            device: std::ptr::null_mut(),
            dev: None,
            messenger: 0,
            validation_counter: std::ptr::null_mut(),
            cmd_pool: 0,
            images: Vec::new(),
            buffers: Vec::new(),
            present: None,
            lease: None,
        }
    }
    pub(super) fn disarm(&mut self) {
        self.armed = false;
    }
}
impl BootstrapGuard {
    fn retain_native_owner(&mut self) {
        if let Some(lease)=self.lease.as_mut(){lease.retain();}
        std::mem::forget(self.present.take());
        std::mem::forget(std::mem::take(&mut self.images));
        std::mem::forget(std::mem::take(&mut self.buffers));
    }
}
impl Drop for BootstrapGuard {
    fn drop(&mut self) {
        if !self.armed{return;}
        if crate::render_exec::hq_vendor_worker_quarantined(){self.retain_native_owner();return;}
        if let Some(present)=self.present.as_mut(){if present.proof().is_err(){self.retain_native_owner();return;}}
        if !self.device.is_null() {
            // SAFETY: U63, live partially constructed RR device; exact public idle API.
            let address=unsafe{(self.fns.vk_gdpa)(self.device,c"vkDeviceWaitIdle".as_ptr())};
            // SAFETY: U63, header-matched signature, checked before native invocation.
            let wait=unsafe{cast_sym::<FnVkDeviceWaitIdle>(address)};
            let Some(wait)=wait else {
                let error=crate::render_exec::quarantine_hq_vendor_rr_present(crate::render_exec::RrPresentFailureStage::FinalPresentProof,None,0,self.device as usize,0,0);
                eprintln!("{error}");self.retain_native_owner();return;
            };
            let started=Instant::now();
            // SAFETY: U63, no concurrent user exists during bootstrap rollback.
            let result=unsafe{wait(self.device)};
            if result!=0&&result!=-4 {
                let error=crate::render_exec::quarantine_hq_vendor_rr_final_wait(result,started.elapsed().as_nanos().min(u64::MAX as u128) as u64,self.device as usize,0);
                eprintln!("{error}");self.retain_native_owner();return;
            }
        }
        // SAFETY: U63, exactly one successful slInit belongs to this armed guard; GPU and WSI proofs passed.
        let result=unsafe{(self.fns.sl_shutdown)()};
        if result!=SL_OK {
            if let Some(present)=self.present.as_mut(){eprintln!("{}",present.shutdown_failed(result));}
            else {let error=crate::render_exec::quarantine_hq_vendor_rr_present(crate::render_exec::RrPresentFailureStage::Shutdown,None,0,self.device as usize,0,0);eprintln!("{error} operation=slShutdown sl_result={result}");}
            self.retain_native_owner();return;
        }
        if let Some(present)=self.present.as_mut(){if present.destroy_after_shutdown().is_err(){self.retain_native_owner();return;}}
        if let Some(dev) = self.dev.as_ref() {
            if self.cmd_pool != 0 {
                // SAFETY: U63, construction-owned pool, no in-flight commands.
                unsafe { (dev.destroy_command_pool)(self.device, self.cmd_pool, std::ptr::null()) };
            }
            for &(image, memory, view) in self.images.iter().rev() {
                release_image(dev, self.device, image, memory, view);
            }
            for &(buffer, memory) in self.buffers.iter().rev() {
                release_buffer(dev, self.device, buffer, memory);
            }
        }
        if !self.device.is_null() {
            // SAFETY: U63, live device dispatch lookup; device is destroyed once below.
            let address = unsafe { (self.fns.vk_gdpa)(self.device, c"vkDestroyDevice".as_ptr()) };
            // SAFETY: U63, header-verified signature and null checked optional symbol.
            if let Some(destroy) = unsafe { cast_sym::<FnVkDestroyDevice>(address) } {
                // SAFETY: U63, tracked children released, startup work finished.
                unsafe { destroy(self.device, std::ptr::null()) };
            }
        }
        if !self.instance.is_null() {
            if self.messenger != 0 {
                // SAFETY: U63, instance is live, exact debug-utils export name.
                let address = unsafe {
                    (self.fns.vk_gipa)(self.instance, c"vkDestroyDebugUtilsMessengerEXT".as_ptr())
                };
                // SAFETY: U63, exact signature checked against the Vulkan SDK.
                if let Some(destroy) =
                    unsafe { cast_sym::<FnVkDestroyDebugUtilsMessenger>(address) }
                {
                    // SAFETY: U63, messenger belongs to this instance, no future callback users.
                    unsafe { destroy(self.instance, self.messenger, std::ptr::null()) };
                }
            }
            // SAFETY: U63, instance is live, exact Vulkan export.
            let address =
                unsafe { (self.fns.vk_gipa)(self.instance, c"vkDestroyInstance".as_ptr()) };
            // SAFETY: U63, exact signature, optional symbol null checked.
            if let Some(destroy) = unsafe { cast_sym::<FnVkDestroyInstance>(address) } {
                // SAFETY: U63, all child resources and callback messenger have been destroyed.
                unsafe { destroy(self.instance, std::ptr::null()) };
            }
        }
        if !self.validation_counter.is_null() {
            // SAFETY: U63, unique Box::into_raw pointer, messenger lifetime has ended.
            unsafe { drop(Box::from_raw(self.validation_counter)) };
        }
    }
}
pub(super) fn release_image(
    dev: &VkDevFns,
    device: VkDevice,
    image: VkImage,
    memory: VkDeviceMemory,
    view: VkImageView,
) {
    if view != 0 {
        // SAFETY: U63, uniquely owned view, no pending references.
        unsafe { (dev.destroy_image_view)(device, view, std::ptr::null()) };
    }
    if image != 0 {
        // SAFETY: U63, all views and GPU users have been released.
        unsafe { (dev.destroy_image)(device, image, std::ptr::null()) };
    }
    if memory != 0 {
        // SAFETY: U63, owned image was destroyed first, free is a single ownership release.
        unsafe { (dev.free_memory)(device, memory, std::ptr::null()) };
    }
}
pub(super) fn release_buffer(
    dev: &VkDevFns,
    device: VkDevice,
    buffer: VkBuffer,
    memory: VkDeviceMemory,
) {
    if buffer != 0 {
        // SAFETY: U63, buffer has no live command users.
        unsafe { (dev.destroy_buffer)(device, buffer, std::ptr::null()) };
    }
    if memory != 0 {
        // SAFETY: U63, corresponding buffer was destroyed; never closes external NT handles.
        unsafe { (dev.free_memory)(device, memory, std::ptr::null()) };
    }
}
pub(super) fn record_validation_error(counts: &ValidationCounts, data: *const c_void, id: &str) {
    if data.is_null() {
        return;
    }
    // SAFETY: U63, VkDebugUtilsMessengerCallbackDataEXT callback lifetime;
    // pMessage is at byte40 in the verified Win64 Vulkan structure.
    let pointer = unsafe { (data.cast::<u8>().add(40).cast::<*const c_char>()).read_unaligned() };
    if pointer.is_null() {
        return;
    }
    // SAFETY: U63, Vulkan supplies a NUL-terminated message valid during this callback.
    let message = unsafe { std::ffi::CStr::from_ptr(pointer) }.to_string_lossy();
    if let Ok(mut errors) = counts.rr_observed_errors.lock() {
        if errors.len() < 32 {
            errors.push(format!("{id}: {message}"));
        }
    }
}

/// Fixed dimensions. Native is the SDK's DLAA mode, not a claimed support result.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RrMode {
    Native,
    Quality,
}
impl RrMode {
    fn sl(self) -> u32 {
        match self {
            Self::Native => 6,
            Self::Quality => 3,
        }
    }
}
#[derive(Debug, Clone)]
pub struct RrConfig {
    pub input_size: (u32, u32),
    pub output_size: (u32, u32),
    pub mode: RrMode,
    pub validation: bool,
    pub preset: u32,
}
/// Explicit depth domain; view depth is positive distance along the view axis.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RrDepthDomain {
    DeviceZForward,
    DeviceZReversed,
    LinearView,
}
/// SDK row-major row-vector matrices, without jitter. Normals are world-space.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RrCamera {
    pub view_to_clip: [[f32; 4]; 4],
    pub clip_to_view: [[f32; 4]; 4],
    pub clip_to_prev_clip: [[f32; 4]; 4],
    pub prev_clip_to_clip: [[f32; 4]; 4],
    pub world_to_view: [[f32; 4]; 4],
    pub view_to_world: [[f32; 4]; 4],
    pub position: [f32; 3],
    pub up: [f32; 3],
    pub right: [f32; 3],
    pub forward: [f32; 3],
    pub near: f32,
    pub far: f32,
    pub fov_y_radians: f32,
}
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RrFrameParams {
    pub camera: RrCamera,
    /// Actual SDK image displacement in input pixels, already converted by the
    /// caller. For rays at pixel center + j this is -j on both image axes.
    pub jitter_pixels: [f32; 2],
    pub pre_exposure: f32,
    pub exposure_scale: f32,
    pub frame_index: u32,
    pub reset: bool,
    pub depth_domain: RrDepthDomain,
}
/// RGB inputs have three f32 channels; normal/roughness has four. Roughness is r,
/// not GGX alpha=r*r. Motion is unjittered prev-minus-current normalized UV.
/// Masks are independent current-frame finite [0,1] fields. No generic reactive tag.
#[derive(Debug, Clone, Copy)]
pub struct RrFrameInput<'a> {
    pub color: &'a [f32],
    pub normals_roughness: &'a [f32],
    pub diffuse_albedo: &'a [f32],
    pub specular_albedo: &'a [f32],
    pub depth: &'a [f32],
    pub mv: &'a [f32],
    pub specular_mv: Option<&'a [f32]>,
    pub specular_hit_distance: Option<&'a [f32]>,
    pub bias_current_color: Option<&'a [f32]>,
    pub transparency: Option<&'a [f32]>,
    pub params: RrFrameParams,
}
/// Tight linear input buffers: RGBA32F for color/normal/albedos, R32F for
/// depth/distance/masks, RG32F for motion. Output target is RGBA16F (8B/pixel).
/// External contents already follow the typed host conventions; no tiled image import.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(usize)]
pub enum RrInputSlot {
    Color = 0,
    Depth = 1,
    Motion = 2,
    NormalRoughness = 3,
    DiffuseAlbedo = 4,
    SpecularAlbedo = 5,
    SpecularHitDistance = 6,
    BiasCurrentColor = 7,
    Transparency = 8,
    SpecularMotion = 9,
}
const SLOT_COUNT: usize = 10;
const INPUT_BPP: [u64; SLOT_COUNT] = [16, 4, 8, 16, 16, 16, 4, 4, 4, 8];
const INPUT_FORMAT: [u32; SLOT_COUNT] = [
    FORMAT_RGBA32,
    FORMAT_R32,
    VK_FORMAT_R32G32_SFLOAT,
    FORMAT_RGBA32,
    FORMAT_RGBA32,
    FORMAT_RGBA32,
    FORMAT_R32,
    FORMAT_R32,
    FORMAT_R32,
    VK_FORMAT_R32G32_SFLOAT,
];
const SLOT_TAG: [u32; SLOT_COUNT] = [3, 0, 1, 14, 7, 8, 42, 29, 27, 10];
#[derive(Debug, Clone, Copy, Default)]
pub struct RrFrameTimings {
    pub host_pack_ms: f64,
    pub bookkeeping_ms: f64,
    pub submit_wait_ms: f64,
    pub readback_ms: f64,
    pub copy_in_gpu_ms: Option<f64>,
    pub rr_gpu_ms: Option<f64>,
    pub copy_out_gpu_ms: Option<f64>,
    pub total_wall_ms: f64,
}
#[derive(Debug, Clone)]
pub struct RrSessionReport {
    pub vendor: VendorSessionReport,
    pub sdk_version: &'static str,
    pub feature: u32,
    pub preset: u32,
    pub mode: RrMode,
    pub input_size: (u32, u32),
    pub output_size: (u32, u32),
    pub optimal_size: (u32, u32),
    pub supported_min: (u32, u32),
    pub supported_max: (u32, u32),
    pub adapter_luid: Option<[u8; 8]>,
    pub owned_allocation_bytes: u64,
    pub imported_allocation_reference_bytes: u64,
    pub estimated_rr_vram_bytes: u64,
    pub last_evaluated: Option<u32>,
    pub last_published: Option<u32>,
    pub reset_pending: bool,
    pub evaluations: u64,
    pub resets: u64,
    pub last_timings: RrFrameTimings,
    pub verified_loaded_dll_paths: Vec<String>,
    pub ota_enabled: bool,
    pub downloaded_plugins_enabled: bool,
    pub owned_image_handles: Vec<(String, u64, i32)>,
    pub existing_ngx_excluded_count: u64,
    pub existing_ngx_excluded_names: Vec<String>,
    pub observed_validation_errors: Vec<String>,
    pub output_valid: bool,
    pub validation_enabled: bool,
    pub streamline_preference_flags: u64,
    pub estimated_rr_vram_known: bool,
    pub preallocation_completed: bool,
    pub preallocation_wall_ms: f64,
    pub memory_before_preallocation: Option<RrMemorySnapshot>,
    pub memory_after_preallocation: Option<RrMemorySnapshot>,
    pub hidden_present: Option<RrPresentReport>,
}
/// VK_EXT_memory_budget observations for DEVICE_LOCAL heaps. Usage is already
/// live process usage: do not add it to the same heap budget a second time.
#[derive(Debug, Clone, Copy)]
pub struct RrMemorySnapshot {
    pub budget_bytes: u64,
    pub usage_bytes: u64,
    pub available_bytes: u64,
    pub heap_count: u32,
}
struct Buffer {
    raw: VkBuffer,
    memory: VkDeviceMemory,
    size: u64,
    allocation: u64,
}
struct Image {
    res: VkImageRes,
    allocation: u64,
}
pub struct DlssRrVkSession {
    core: std::mem::ManuallyDrop<DlssVkSession>,
    teardown: Option<RrShutdownOutcome>,
    config: RrConfig,
    set_options: SetOptions,
    get_state: GetState,
    set_tags_for_frame: SetTagsForFrame,
    inputs: Vec<Image>,
    staging: Option<Buffer>,
    imports: Vec<Option<Buffer>>,
    external_output: Option<Buffer>,
    offsets: [u64; SLOT_COUNT],
    query_pool: VkQueryPool,
    optimal: (u32, u32),
    min: (u32, u32),
    max: (u32, u32),
    rr_vram: u64,
    reset_pending: bool,
    recreate_pending: bool,
    poisoned: bool,
    evaluated: Option<u32>,
    published: Option<u32>,
    evaluations: u64,
    resets: u64,
    timings: RrFrameTimings,
    loaded_dll_paths: Vec<String>,
    rr_version: String,
    exposure_offset: u64,
    output_valid: bool,
    preallocated: bool,
    preallocation_wall_ms: f64,
    memory_before_preallocation: Option<RrMemorySnapshot>,
    memory_after_preallocation: Option<RrMemorySnapshot>,
    allocation_constants: Option<RrFrameParams>,
}
fn bad(message: impl Into<String>) -> VendorError {
    VendorError::ApiError(message.into())
}
fn sl_check(operation: &str, result: SlResult) -> Result<(), VendorError> {
    if result == SL_OK {
        Ok(())
    } else {
        Err(VendorError::VendorCall(format!(
            "{operation} → {}",
            sl_result_name(result)
        )))
    }
}
fn vk_check(operation: &str, result: i32) -> Result<(), VendorError> {
    if result == VK_SUCCESS {
        Ok(())
    } else {
        Err(bad(format!("{operation} → {result}")))
    }
}
impl DlssRrVkSession {
    fn admit_native(&mut self)->Result<(),VendorError>{
        if self.teardown.is_some(){return Err(bad("RR session already shut down"));}
        self.core.rr_present.as_mut().ok_or_else(||bad("RR WSI owner missing"))?.check_thread()?;
        if self.poisoned{return Err(bad("RR session is poisoned"));}Ok(())
    }
    pub fn create(sdk_dir: &Path, config: &RrConfig) -> Result<Self, VendorError> {
        validate_config(config)?;
        verify_package(sdk_dir)?;
        let core = DlssVkSession::create_profile(
            sdk_dir,
            config.input_size,
            config.output_size,
            config.validation,
            true,
        )?;
        let loaded_dll_paths = verify_loaded_modules()?;
        // SAFETY: U63; these exact function signatures/GUID versions match official
        // sl_dlss_d.h 2.14.16. Returned addresses are checked before use.
        let get = |name: &std::ffi::CStr| -> Result<*mut c_void, VendorError> {
            let mut p = std::ptr::null_mut();
            // SAFETY: U63, SL is live; name is NUL terminated; p is a valid out pointer.
            sl_check("slGetFeatureFunction(RR)", unsafe {
                (core.fns.sl_get_feature_function)(FEATURE, name.as_ptr(), &mut p)
            })?;
            if p.is_null() {
                return Err(VendorError::SymbolMissing(
                    name.to_string_lossy().into_owned(),
                ));
            }
            Ok(p)
        };
        // SAFETY: U63; function names and signatures are verified against SDK headers.
        let set_options = unsafe { cast_sym::<SetOptions>(get(c"slDLSSDSetOptions")?) }
            .ok_or_else(|| bad("RR SetOptions null"))?;
        // SAFETY: U63; same verified ABI; no caller pointer crosses this lifetime.
        let get_optimal = unsafe { cast_sym::<GetOptimal>(get(c"slDLSSDGetOptimalSettings")?) }
            .ok_or_else(|| bad("RR GetOptimal null"))?;
        // SAFETY: U63; same verified ABI.
        let get_state = unsafe { cast_sym::<GetState>(get(c"slDLSSDGetState")?) }
            .ok_or_else(|| bad("RR GetState null"))?;
        // SAFETY: U63, live pinned interposer and exact SDK frame-tag export.
        let tag_address = unsafe { loader::sym(core.library, c"slSetTagForFrame".as_ptr()) };
        // SAFETY: U63, header-verified signature, null checked before use.
        let set_tags_for_frame = unsafe { cast_sym::<SetTagsForFrame>(tag_address) }
            .ok_or_else(|| VendorError::SymbolMissing("slSetTagForFrame".into()))?;
        // SAFETY: U63; global SL export is resolved from the live interposer.
        // SAFETY: U63, live interposer handle and NUL-terminated SL export name.
        let supported_address =
            unsafe { loader::sym(core.library, c"slIsFeatureSupported".as_ptr()) };
        // SAFETY: U63, verified SL export from the already loaded interposer and exact SDK signature.
        let supported: Supported = unsafe { cast_sym(supported_address) }
            .ok_or_else(|| VendorError::SymbolMissing("slIsFeatureSupported".into()))?;
        let adapter = SlAdapter {
            base: sl_base(GUID_ADAPTER, 1),
            luid: std::ptr::null_mut(),
            luid_size: 0,
            pad: 0,
            physical: core.physical_device,
        };
        // SAFETY: U63, physical device belongs to the live SL instance, adapter is initialized.
        sl_check("slIsFeatureSupported(RR)", unsafe {
            supported(FEATURE, &adapter)
        })?;
        let mut feature_version = SlFeatureVersion {
            base: sl_base(SL_GUID_FEATURE_VERSION, 1),
            version_sl: [0; 3],
            version_ngx: [0; 3],
        };
        // SAFETY: U63, exact header-verified FeatureVersion output and live RR feature.
        sl_check("slGetFeatureVersion(RR)", unsafe {
            (core.fns.sl_get_feature_version)(FEATURE, &mut feature_version)
        })?;
        let rr_version = format!(
            "SL {}.{}.{} / RR NGX {}.{}.{}",
            feature_version.version_sl[0],
            feature_version.version_sl[1],
            feature_version.version_sl[2],
            feature_version.version_ngx[0],
            feature_version.version_ngx[1],
            feature_version.version_ngx[2]
        );
        let identity = identity();
        let options = options(config, 1., 1., identity, identity);
        let mut optimal = SlRrOptimal {
            base: sl_base(GUID_OPTIMAL, 1),
            width: 0,
            height: 0,
            sharpness: 0.,
            min_w: 0,
            min_h: 0,
            max_w: 0,
            max_h: 0,
        };
        // SAFETY: U63, complete versioned structures are alive for the synchronous query.
        sl_check("slDLSSDGetOptimalSettings", unsafe {
            get_optimal(&options, &mut optimal)
        })?;
        let (w, h) = config.input_size;
        if w < optimal.min_w || h < optimal.min_h || w > optimal.max_w || h > optimal.max_h {
            return Err(VendorError::DeviceUnavailable(format!(
                "RR input {w}x{h} outside returned [{},{}]×[{},{}]",
                optimal.min_w, optimal.max_w, optimal.min_h, optimal.max_h
            )));
        }
        let mut s = Self {
            core: std::mem::ManuallyDrop::new(core),
            teardown: None,
            config: config.clone(),
            set_options,
            get_state,
            set_tags_for_frame,
            inputs: Vec::new(),
            staging: None,
            imports: (0..SLOT_COUNT).map(|_| None).collect(),
            external_output: None,
            offsets: [0; SLOT_COUNT],
            query_pool: 0,
            optimal: (optimal.width, optimal.height),
            min: (optimal.min_w, optimal.min_h),
            max: (optimal.max_w, optimal.max_h),
            rr_vram: 0,
            reset_pending: true,
            recreate_pending: false,
            poisoned: false,
            evaluated: None,
            published: None,
            evaluations: 0,
            resets: 0,
            timings: RrFrameTimings::default(),
            loaded_dll_paths,
            rr_version,
            exposure_offset: 0,
            output_valid: false,
            preallocated: false,
            preallocation_wall_ms: 0.,
            memory_before_preallocation: None,
            memory_after_preallocation: None,
            allocation_constants: None,
        };
        let pixels = u64::from(w) * u64::from(h);
        let mut total = 0;
        for slot in 0..SLOT_COUNT {
            s.offsets[slot] = total;
            total = total
                .checked_add(pixels * INPUT_BPP[slot])
                .ok_or_else(|| bad("RR staging overflow"))?;
            let image = s.make_image(INPUT_FORMAT[slot], w, h)?;
            s.inputs.push(image);
        }
        // Official optional Exposure tag: an explicit constant 1x1 value,
        // avoiding an undeclared auto-exposure source in the RR boundary.
        s.exposure_offset = total;
        total = total
            .checked_add(4)
            .ok_or_else(|| bad("RR exposure staging overflow"))?;
        let exposure = s.make_image(FORMAT_R32, 1, 1)?;
        s.inputs.push(exposure);
        s.staging = Some(s.make_buffer(total, VK_BUFFER_USAGE_TRANSFER_SRC, s.core.host_type)?);
        let qi = VkQueryPoolCreateInfo {
            s_type: RR_QUERY_POOL_CREATE_INFO,
            p_next: std::ptr::null(),
            flags: 0,
            query_type: VK_QUERY_TYPE_TIMESTAMP,
            query_count: 4,
            pipeline_statistics: 0,
        };
        // SAFETY: U63, qi and output pool belong to this live device; Drop frees the pool.
        vk_check("vkCreateQueryPool(RR)", unsafe {
            (s.core.dev.create_query_pool)(s.core.device, &qi, std::ptr::null(), &mut s.query_pool)
        })?;
        // SAFETY: U63, matching viewport and complete RR options; allocation remains lazy until evaluate.
        sl_check("slDLSSDSetOptions(create)", unsafe {
            (s.set_options)(&s.core.viewport, &options)
        })?;
        s.query_vram()?;
        Ok(s)
    }
    pub fn physical_device_luid(&self) -> Option<[u8; 8]> {
        self.core.device_luid
    }
    pub fn external_memory_enabled(&self) -> bool {
        self.core.external_memory_enabled
    }
    /// Resets temporal history on the next frame; no physical renderer state is changed.
    pub fn reset_history(&mut self) {
        self.reset_pending = true;
    }
    /// Call after a successful evaluate whose downstream display/publication failed.
    /// The next evaluate frees/recreates vendor history. Published identity is retained.
    pub fn discard_frame(&mut self) {
        self.reset_pending = true;
        self.recreate_pending = true;
        self.output_valid = false;
    }
    pub fn publish_frame(&mut self, frame_index: u32) -> Result<(), VendorError> {
        if self.teardown.is_some(){return Err(bad("RR session already shut down"));}
        let present=self.core.rr_present.as_mut().ok_or_else(||bad("RR WSI owner missing"))?;
        present.proof()?;
        if present.report().last_frame!=Some(frame_index){return Err(bad("RR publish lacks matching completed hidden presentation"));}
        if self.evaluated != Some(frame_index) || self.poisoned || !self.output_valid {
            return Err(bad(
                "RR publish does not match the last successful evaluation",
            ));
        }
        self.published = Some(frame_index);
        Ok(())
    }
    pub fn last_timings(&self) -> RrFrameTimings {
        self.timings
    }
    pub fn report(&self) -> RrSessionReport {
        let base_images = [
            &self.core.color_in,
            &self.core.depth_in,
            &self.core.mv_in,
            &self.core.reactive_in,
            &self.core.color_out,
        ];
        let mut base_bytes = 0u64;
        for image in base_images {
            let mut req = VkMemoryRequirements::default();
            // SAFETY: U63, all base images belong to the live session; query only writes req.
            unsafe {
                (self.core.dev.get_image_memory_requirements)(
                    self.core.device,
                    image.image,
                    &mut req,
                )
            };
            base_bytes = base_bytes.saturating_add(req.size);
        }
        for (raw, _) in [
            (self.core.staging, self.core.staging_size),
            (self.core.readback, self.core.readback_size),
        ] {
            let mut req = VkMemoryRequirements::default();
            // SAFETY: U63, owned live buffers and local requirements output.
            unsafe {
                (self.core.dev.get_buffer_memory_requirements)(self.core.device, raw, &mut req)
            };
            base_bytes = base_bytes.saturating_add(req.size);
        }
        let mut vendor = self.core.report();
        vendor.backend = "dlss_ray_reconstruction_streamline_2.14.16_vulkan".into();
        vendor.engine_version = self.rr_version.clone();
        let (existing_ngx_excluded_count, existing_ngx_excluded_names) =
            self.core.validation_excluded();
        let observed_validation_errors = if self.core.validation_counter.is_null() {
            Vec::new()
        } else {
            // SAFETY: U63, counter Box is owned by live core and messenger outlives report access.
            unsafe { &*self.core.validation_counter }
                .rr_observed_errors
                .lock()
                .map(|v| v.clone())
                .unwrap_or_default()
        };
        RrSessionReport {
            vendor,
            sdk_version: "2.14.16",
            feature: FEATURE,
            preset: self.config.preset,
            mode: self.config.mode,
            input_size: self.config.input_size,
            output_size: self.config.output_size,
            optimal_size: self.optimal,
            supported_min: self.min,
            supported_max: self.max,
            adapter_luid: self.core.device_luid,
            owned_allocation_bytes: base_bytes
                + self.inputs.iter().map(|x| x.allocation).sum::<u64>()
                + self.staging.as_ref().map_or(0, |x| x.allocation),
            imported_allocation_reference_bytes: self
                .imports
                .iter()
                .flatten()
                .map(|x| x.allocation)
                .sum::<u64>()
                + self.external_output.as_ref().map_or(0, |x| x.allocation),
            estimated_rr_vram_bytes: self.rr_vram,
            last_evaluated: self.evaluated,
            last_published: self.published,
            reset_pending: self.reset_pending,
            evaluations: self.evaluations,
            resets: self.resets,
            last_timings: self.timings,
            verified_loaded_dll_paths: self.loaded_dll_paths.clone(),
            ota_enabled: false,
            downloaded_plugins_enabled: false,
            owned_image_handles: self
                .inputs
                .iter()
                .enumerate()
                .map(|(i, x)| (format!("rr_input_{i}"), x.res.image, x.res.layout))
                .chain([
                    (
                        "rr_output".into(),
                        self.core.color_out.image,
                        self.core.color_out.layout,
                    ),
                    (
                        "bootstrap_color".into(),
                        self.core.color_in.image,
                        self.core.color_in.layout,
                    ),
                    (
                        "bootstrap_depth".into(),
                        self.core.depth_in.image,
                        self.core.depth_in.layout,
                    ),
                    (
                        "bootstrap_motion".into(),
                        self.core.mv_in.image,
                        self.core.mv_in.layout,
                    ),
                    (
                        "bootstrap_reactive".into(),
                        self.core.reactive_in.image,
                        self.core.reactive_in.layout,
                    ),
                ])
                .collect(),
            existing_ngx_excluded_count,
            existing_ngx_excluded_names,
            observed_validation_errors,
            output_valid: self.output_valid,
            validation_enabled: self.config.validation,
            streamline_preference_flags: 133,
            estimated_rr_vram_known: self.rr_vram > 0
                && (self.preallocated || self.evaluated.is_some()),
            preallocation_completed: self.preallocated,
            preallocation_wall_ms: self.preallocation_wall_ms,
            memory_before_preallocation: self.memory_before_preallocation,
            memory_after_preallocation: self.memory_after_preallocation,
            hidden_present: self.core.rr_present.as_ref().map(|p|p.report()),
        }
    }
    fn query_vram(&mut self) -> Result<(), VendorError> {
        let mut state = SlRrState {
            base: sl_base(GUID_STATE, 1),
            vram: 0,
        };
        // SAFETY: U63, initialized versioned output and matching live viewport.
        sl_check("slDLSSDGetState", unsafe {
            (self.get_state)(&self.core.viewport, &mut state)
        })?;
        self.rr_vram = state.vram;
        Ok(())
    }

    /// Allocate the official RR feature with the actual first-frame camera and
    /// existing owned image metadata, in a separate completed command buffer.
    /// No noisy frame is evaluated or published. The SDK allocation entry uses
    /// frame-token index zero internally; this is an allocation epoch, not a
    /// fabricated physical renderer sample. First evaluate still resets history.
    pub fn preallocate(&mut self, p: &RrFrameParams) -> Result<(), VendorError> {
        if self.teardown.is_some(){return Err(bad("RR session already shut down"));}
        self.core.rr_present.as_mut().ok_or_else(||bad("RR WSI owner missing"))?.check_thread()?;
        validate_params(p)?;
        if self.poisoned || self.evaluated.is_some() || self.preallocated {
            return Err(bad("RR preallocate requires a fresh unevaluated session"));
        }
        let start = Instant::now();
        self.output_valid = false;
        self.memory_before_preallocation = self.memory_snapshot()?;
        // SAFETY: U63, verified SL exports from this already pinned/live interposer.
        let tags_address = unsafe { loader::sym(self.core.library, c"slSetTagForFrame".as_ptr()) };
        // SAFETY: U63, official 2.14.16 sl_core_api.h function signature.
        let set_tags = unsafe { cast_sym::<SetTagsForFrame>(tags_address) }
            .ok_or_else(|| VendorError::SymbolMissing("slSetTagForFrame".into()))?;
        // SAFETY: U63, same live interposer, NUL-terminated exact export name.
        let allocate_address =
            unsafe { loader::sym(self.core.library, c"slAllocateResources".as_ptr()) };
        // SAFETY: U63, official CommandBuffer*,Feature,Viewport signature, null checked.
        let allocate = unsafe { cast_sym::<AllocateResources>(allocate_address) }
            .ok_or_else(|| VendorError::SymbolMissing("slAllocateResources".into()))?;
        let allocation_index = 0u32;
        let mut token = std::ptr::null_mut();
        // SAFETY: U63, output token pointer and allocation epoch are alive for this call.
        sl_check("slGetNewFrameToken(RR allocation)", unsafe {
            (self.core.fns.sl_get_new_frame_token)(&mut token, &allocation_index)
        })?;
        if token.is_null() {
            return Err(bad("RR allocation token is null"));
        }
        let constants = constants(p, self.config.input_size, true);
        // SAFETY: U63, actual first camera/unjittered matrices, initialized 456-byte header-verified constants.
        sl_check("slSetConstants(RR allocation)", unsafe {
            (self.core.fns.sl_set_constants)(&constants, token, &self.core.viewport)
        })?;
        self.set_frame_options(p)?;
        let begin = VkCommandBufferBeginInfo {
            s_type: VK_STRUCTURE_TYPE_COMMAND_BUFFER_BEGIN_INFO,
            p_next: std::ptr::null(),
            flags: 1,
            p_inheritance_info: std::ptr::null(),
        };
        // SAFETY: U63, fresh nonpending command buffer; all allocation operations stay in this device domain.
        vk_check("vkBeginCommandBuffer(RR allocation)", unsafe {
            (self.core.dev.begin_command_buffer)(self.core.cmd, &begin)
        })?;
        let images = [
            sl_resource(&self.inputs[0].res, false),
            sl_resource(&self.inputs[1].res, false),
            sl_resource(&self.inputs[2].res, false),
            sl_resource(&self.core.color_out, true),
        ];
        let types = [
            3,
            if p.depth_domain == RrDepthDomain::LinearView {
                49
            } else {
                0
            },
            1,
            4,
        ];
        let tags: Vec<_> = images
            .iter()
            .zip(types)
            .map(|(resource, tag_type)| SlResourceTag {
                base: sl_base(SL_GUID_RESOURCE_TAG, 1),
                resource,
                tag_type,
                lifecycle: RR_LIFECYCLE_SESSION,
                extent: SlExtent {
                    top: 0,
                    left: 0,
                    width: resource.width,
                    height: resource.height,
                },
            })
            .collect();
        // SDK BeginEvent only consumes the four actual format/extent descriptors
        // to create the feature. It does not run EndEvent or sample image pixels.
        // SAFETY: U63, frame-based tagging flag is enabled; resources/tags survive through allocation below.
        let tagged = unsafe {
            set_tags(
                token,
                &self.core.viewport,
                tags.as_ptr(),
                tags.len() as u32,
                self.core.cmd,
            )
        };
        let allocated = if tagged == SL_OK {
            // SAFETY: U63, matching real viewport/options/tags, recording command buffer; no evaluate call.
            unsafe { allocate(self.core.cmd, FEATURE, &self.core.viewport) }
        } else {
            tagged
        };
        if allocated != SL_OK {
            // SAFETY: U63, abort this unsubmitted initialization recording.
            unsafe {
                let _ = (self.core.dev.end_command_buffer)(self.core.cmd);
            }
            // SAFETY: U63, no GPU user references the aborted command.
            unsafe {
                let _ = (self.core.dev.reset_command_buffer)(self.core.cmd, 0);
            }
            self.discard_frame();
            return sl_check("RR allocation tags/resources", allocated);
        }
        // SAFETY: U63, initialization recording is complete.
        vk_check("vkEndCommandBuffer(RR allocation)", unsafe {
            (self.core.dev.end_command_buffer)(self.core.cmd)
        })?;
        let submit = VkSubmitInfo {
            s_type: VK_STRUCTURE_TYPE_SUBMIT_INFO,
            p_next: std::ptr::null(),
            wait_semaphore_count: 0,
            p_wait_semaphores: std::ptr::null(),
            p_wait_dst_stage_mask: std::ptr::null(),
            command_buffer_count: 1,
            p_command_buffers: &self.core.cmd,
            signal_semaphore_count: 0,
            p_signal_semaphores: std::ptr::null(),
        };
        // SAFETY: U63, initialization command is executable and owned exclusively by this session.
        let r = unsafe { (self.core.dev.queue_submit)(self.core.queue, 1, &submit, 0) };
        if r != VK_SUCCESS {
            self.poisoned = true;
            return Err(bad(format!("RR allocation submit → {r}")));
        }
        // SAFETY: U63, completes allocation/init before budget observation or any renderer dispatch.
        let r = unsafe { (self.core.dev.queue_wait_idle)(self.core.queue) };
        if r != VK_SUCCESS {
            self.poisoned = true;
            return Err(bad(format!("RR allocation wait → {r}")));
        }
        // SAFETY: U63, queue idle proves the allocation command is reusable.
        vk_check("vkResetCommandBuffer(RR allocation)", unsafe {
            (self.core.dev.reset_command_buffer)(self.core.cmd, 0)
        })?;
        self.query_vram()?;
        self.memory_after_preallocation = self.memory_snapshot()?;
        self.preallocation_wall_ms = start.elapsed().as_secs_f64() * 1000.;
        if self.core.validation_errors() != 0 {
            self.discard_frame();
            return Err(bad("RR allocation validation error; capacity not admitted"));
        }
        self.loaded_dll_paths = verify_loaded_modules()?;
        self.preallocated = true;
        self.reset_pending = true;
        let mut cached = *p;
        cached.frame_index = 0;
        cached.reset = true;
        self.allocation_constants = Some(cached);
        if self.rr_vram == 0 {
            return Err(bad(
                "RR allocated but official GetState VRAM remains unknown; do not admit a complete capacity budget",
            ));
        }
        Ok(())
    }

    fn set_frame_options(&self, p: &RrFrameParams) -> Result<(), VendorError> {
        let sr = compatible_sr_options(&self.config, p);
        // SAFETY: U63, compatible SR options are required by official RR, matching viewport.
        sl_check("slDLSSSetOptions(RR compatible)", unsafe {
            (self.core.dlss.dlss_set_options)(&self.core.viewport, &sr)
        })?;
        let rr = options(
            &self.config,
            p.pre_exposure,
            p.exposure_scale,
            p.camera.world_to_view,
            p.camera.view_to_world,
        );
        // SAFETY: U63, RR uses its own official v3 options and function pointer.
        sl_check("slDLSSDSetOptions", unsafe {
            (self.set_options)(&self.core.viewport, &rr)
        })
    }

    /// Observe currently live DEVICE_LOCAL heap budgets/usages. None means the
    /// extension/query is unavailable, never a zero-usage assumption.
    pub fn memory_snapshot(&self) -> Result<Option<RrMemorySnapshot>, VendorError> {
        if self.teardown.is_some()||self.poisoned{return Err(bad("RR session not available for memory query"));}
        self.core.rr_present.as_ref().ok_or_else(||bad("RR WSI owner missing"))?.check_readonly()?;
        // SAFETY: U63, live instance, exact Vulkan instance-dispatch export.
        let enum_address = unsafe {
            (self.core.fns.vk_gipa)(
                self.core.instance,
                c"vkEnumerateDeviceExtensionProperties".as_ptr(),
            )
        };
        // SAFETY: U63, exact header signature, checked optional address.
        let Some(enumerate) =
            (unsafe { cast_sym::<FnVkEnumerateDeviceExtensionProperties>(enum_address) })
        else {
            return Ok(None);
        };
        let mut count = 0u32;
        // SAFETY: U63, physical device is live; count is a valid output.
        vk_check("RR memory extensions count", unsafe {
            enumerate(
                self.core.physical_device,
                std::ptr::null(),
                &mut count,
                std::ptr::null_mut(),
            )
        })?;
        if count == 0 || count > 1024 {
            return Ok(None);
        }
        let mut extensions = vec![[0u32; 65]; count as usize];
        // SAFETY: U63, each VkExtensionProperties is 260 bytes aligned to four.
        vk_check("RR memory extensions", unsafe {
            enumerate(
                self.core.physical_device,
                std::ptr::null(),
                &mut count,
                extensions.as_mut_ptr().cast(),
            )
        })?;
        let present = extensions.iter().any(|row| {
            let bytes: Vec<u8> = row[..64].iter().flat_map(|w| w.to_le_bytes()).collect();
            bytes.starts_with(b"VK_EXT_memory_budget\0")
        });
        if !present {
            return Ok(None);
        }
        type Get = unsafe extern "system" fn(VkPhysicalDevice, *mut MemoryProperties2);
        // SAFETY: U63, live instance, exact Vulkan1.1 core query.
        let address = unsafe {
            (self.core.fns.vk_gipa)(
                self.core.instance,
                c"vkGetPhysicalDeviceMemoryProperties2".as_ptr(),
            )
        };
        // SAFETY: U63, exact verified function signature and null checked.
        let Some(get) = (unsafe { cast_sym::<Get>(address) }) else {
            return Ok(None);
        };
        let mut budget = MemoryBudget {
            s_type: 1_000_237_000,
            p_next: std::ptr::null_mut(),
            budget: [0; 16],
            usage: [0; 16],
        };
        let mut properties = MemoryProperties2 {
            s_type: 1_000_059_006,
            p_next: (&mut budget as *mut MemoryBudget).cast(),
            properties: [0; 65],
        };
        // SAFETY: U63, advertised extension, exact 536+272 byte writable chain with actual physical device.
        unsafe { get(self.core.physical_device, &mut properties) };
        let bytes: [u8; 520] =
            std::array::from_fn(|i| (properties.properties[i / 8] >> ((i % 8) * 8)) as u8);
        let heap_count = u32::from_le_bytes(bytes[260..264].try_into().unwrap()).min(16);
        let mut result = RrMemorySnapshot {
            budget_bytes: 0,
            usage_bytes: 0,
            available_bytes: 0,
            heap_count: 0,
        };
        for i in 0..heap_count as usize {
            let offset = 264 + i * 16;
            let flags = u32::from_le_bytes(bytes[offset + 8..offset + 12].try_into().unwrap());
            if flags & 1 != 0 {
                if budget.budget[i] == 0 {
                    return Ok(None);
                }
                result.heap_count += 1;
                result.budget_bytes = result.budget_bytes.saturating_add(budget.budget[i]);
                result.usage_bytes = result.usage_bytes.saturating_add(budget.usage[i]);
            }
        }
        if result.heap_count == 0 {
            return Ok(None);
        }
        result.available_bytes = result.budget_bytes.saturating_sub(result.usage_bytes);
        Ok(Some(result))
    }
    fn make_image(&self, format: u32, w: u32, h: u32) -> Result<Image, VendorError> {
        let dev = &self.core.dev;
        let device = self.core.device;
        let ci = VkImageCreateInfo {
            s_type: VK_STRUCTURE_TYPE_IMAGE_CREATE_INFO,
            p_next: std::ptr::null(),
            flags: 0,
            image_type: VK_IMAGE_TYPE_2D,
            format,
            extent: VkExtent3D {
                width: w,
                height: h,
                depth: 1,
            },
            mip_levels: 1,
            array_layers: 1,
            samples: 1,
            tiling: 0,
            usage: VK_IMAGE_USAGE_SAMPLED | VK_IMAGE_USAGE_TRANSFER_DST,
            sharing_mode: 0,
            queue_family_index_count: 0,
            p_queue_family_indices: std::ptr::null(),
            initial_layout: VK_IMAGE_LAYOUT_UNDEFINED,
        };
        let mut image = 0;
        // SAFETY: U63, complete Vulkan create descriptor; image is stored or released below.
        vk_check("vkCreateImage(RR)", unsafe {
            (dev.create_image)(device, &ci, std::ptr::null(), &mut image)
        })?;
        let mut req = VkMemoryRequirements::default();
        // SAFETY: U63, image was successfully created, req is valid output storage.
        unsafe { (dev.get_image_memory_requirements)(device, image, &mut req) };
        if req.memory_type_bits & (1 << self.core.device_local_type) == 0 {
            // SAFETY: U63, unbound image created here has no users.
            unsafe { (dev.destroy_image)(device, image, std::ptr::null()) };
            return Err(bad("RR image memory type is incompatible"));
        }
        let ai = VkMemoryAllocateInfo {
            s_type: VK_STRUCTURE_TYPE_MEMORY_ALLOCATE_INFO,
            p_next: std::ptr::null(),
            allocation_size: req.size,
            memory_type_index: self.core.device_local_type,
        };
        let mut memory = 0;
        // SAFETY: U63, requirements-compatible allocation descriptor and valid output.
        let r = unsafe { (dev.allocate_memory)(device, &ai, std::ptr::null(), &mut memory) };
        if r != VK_SUCCESS {
            // SAFETY: U63, no memory was bound and no work uses image.
            unsafe { (dev.destroy_image)(device, image, std::ptr::null()) };
            return Err(bad(format!("vkAllocateMemory(RR image) → {r}")));
        }
        // SAFETY: U63, this allocation satisfies this image's requirements, offset is aligned zero.
        let bind = unsafe { (dev.bind_image_memory)(device, image, memory, 0) };
        let mut view = 0;
        let vi = VkImageViewCreateInfo {
            s_type: VK_STRUCTURE_TYPE_IMAGE_VIEW_CREATE_INFO,
            p_next: std::ptr::null(),
            flags: 0,
            image,
            view_type: 1,
            format,
            components: [0; 4],
            subresource_range: VkImageSubresourceRange {
                aspect_mask: VK_IMAGE_ASPECT_COLOR,
                base_mip_level: 0,
                level_count: 1,
                base_array_layer: 0,
                layer_count: 1,
            },
        };
        let r = if bind == VK_SUCCESS {
            // SAFETY: U63, bound image and matching format/range; output is local.
            unsafe { (dev.create_image_view)(device, &vi, std::ptr::null(), &mut view) }
        } else {
            bind
        };
        if r != VK_SUCCESS {
            release_image(dev, device, image, memory, view);
            return Err(bad(format!("RR bind/image view → {r}")));
        }
        Ok(Image {
            res: VkImageRes {
                image,
                memory,
                view,
                layout: VK_IMAGE_LAYOUT_UNDEFINED,
                format,
                w,
                h,
                aspect: VK_IMAGE_ASPECT_COLOR,
            },
            allocation: req.size,
        })
    }
    fn make_buffer(&self, size: u64, usage: u32, memory_type: u32) -> Result<Buffer, VendorError> {
        let dev = &self.core.dev;
        let device = self.core.device;
        let ci = VkBufferCreateInfo {
            s_type: VK_STRUCTURE_TYPE_BUFFER_CREATE_INFO,
            p_next: std::ptr::null(),
            flags: 0,
            size,
            usage,
            sharing_mode: 0,
            queue_family_index_count: 0,
            p_queue_family_indices: std::ptr::null(),
        };
        let mut raw = 0;
        // SAFETY: U63, initialized buffer descriptor and live device.
        vk_check("vkCreateBuffer(RR)", unsafe {
            (dev.create_buffer)(device, &ci, std::ptr::null(), &mut raw)
        })?;
        let mut req = VkMemoryRequirements::default();
        // SAFETY: U63, raw was created here; requirements output is valid.
        unsafe { (dev.get_buffer_memory_requirements)(device, raw, &mut req) };
        if req.memory_type_bits & (1 << memory_type) == 0 {
            // SAFETY: U63, no bound memory or users.
            unsafe { (dev.destroy_buffer)(device, raw, std::ptr::null()) };
            return Err(bad("RR buffer memory type incompatible"));
        }
        let ai = VkMemoryAllocateInfo {
            s_type: VK_STRUCTURE_TYPE_MEMORY_ALLOCATE_INFO,
            p_next: std::ptr::null(),
            allocation_size: req.size,
            memory_type_index: memory_type,
        };
        let mut memory = 0;
        // SAFETY: U63, compatible allocation and valid output.
        let r = unsafe { (dev.allocate_memory)(device, &ai, std::ptr::null(), &mut memory) };
        if r != VK_SUCCESS {
            // SAFETY: U63, unbound buffer has no users.
            unsafe { (dev.destroy_buffer)(device, raw, std::ptr::null()) };
            return Err(bad(format!("RR buffer allocation → {r}")));
        }
        // SAFETY: U63, matching live buffer/allocation at aligned zero offset.
        let r = unsafe { (dev.bind_buffer_memory)(device, raw, memory, 0) };
        if r != VK_SUCCESS {
            release_buffer(dev, device, raw, memory);
            return Err(bad(format!("RR buffer bind → {r}")));
        }
        Ok(Buffer {
            raw,
            memory,
            size,
            allocation: req.size,
        })
    }

    /// Evaluate a complete current-frame host packet. Raw renderer inputs are read-only.
    pub fn evaluate(&mut self, input: &RrFrameInput<'_>) -> Result<Vec<f32>, VendorError> {
        let mut output = Vec::new();
        self.evaluate_into(input, &mut output)?;
        Ok(output)
    }
    pub fn evaluate_into(
        &mut self,
        input: &RrFrameInput<'_>,
        output: &mut Vec<f32>,
    ) -> Result<(), VendorError> {
        self.admit_native()?;
        validate_frame(input, self.config.input_size)?;
        let start = Instant::now();
        self.pack_host(input)?;
        let pack = start.elapsed().as_secs_f64() * 1000.;
        let active = [
            true,
            true,
            true,
            true,
            true,
            true,
            input.specular_hit_distance.is_some(),
            true,
            true,
            input.specular_mv.is_some(),
        ];
        output.resize(
            self.config.output_size.0 as usize * self.config.output_size.1 as usize * 3,
            0.,
        );
        let result = self.frame(&input.params, active, false, Some(output.as_mut_slice()));
        self.timings.host_pack_ms = pack;
        self.timings.total_wall_ms = start.elapsed().as_secs_f64() * 1000.;
        if result.is_err() {
            self.discard_frame();
        }
        result
    }
    fn pack_host(&mut self, input: &RrFrameInput<'_>) -> Result<(), VendorError> {
        let buffer = self
            .staging
            .as_ref()
            .ok_or_else(|| bad("RR staging absent"))?;
        let mut ptr = std::ptr::null_mut();
        // SAFETY: U63, coherent staging is session-owned, idle and not aliased by GPU work.
        vk_check("vkMapMemory(RR staging)", unsafe {
            (self.core.dev.map_memory)(self.core.device, buffer.memory, 0, buffer.size, 0, &mut ptr)
        })?;
        if ptr.is_null() {
            return Err(bad("RR staging map returned null"));
        }
        // SAFETY: U63, successful map covers exactly buffer.size initialized writable bytes.
        let destination =
            unsafe { std::slice::from_raw_parts_mut(ptr.cast::<u8>(), buffer.size as usize) };
        destination.fill(0);
        destination[self.exposure_offset as usize..self.exposure_offset as usize + 4]
            .copy_from_slice(&1.0f32.to_le_bytes());
        let pixels = self.config.input_size.0 as usize * self.config.input_size.1 as usize;
        let data: [Option<&[f32]>; SLOT_COUNT] = [
            Some(input.color),
            Some(input.depth),
            Some(input.mv),
            Some(input.normals_roughness),
            Some(input.diffuse_albedo),
            Some(input.specular_albedo),
            input.specular_hit_distance,
            input.bias_current_color,
            input.transparency,
            input.specular_mv,
        ];
        for (slot, source) in data.into_iter().enumerate() {
            let Some(source) = source else {
                continue;
            };
            let offset = self.offsets[slot] as usize;
            if matches!(slot, 0 | 4 | 5) {
                for p in 0..pixels {
                    for channel in 0..4 {
                        let v = if channel < 3 {
                            source[p * 3 + channel]
                        } else {
                            1.
                        };
                        destination
                            [offset + p * 16 + channel * 4..offset + p * 16 + channel * 4 + 4]
                            .copy_from_slice(&v.to_le_bytes());
                    }
                }
            } else {
                for (n, value) in source.iter().enumerate() {
                    destination[offset + n * 4..offset + n * 4 + 4]
                        .copy_from_slice(&value.to_le_bytes());
                }
            }
        }
        // SAFETY: U63, all CPU staging accesses have ended; coherent memory needs no flush.
        unsafe { (self.core.dev.unmap_memory)(self.core.device, buffer.memory) };
        Ok(())
    }

    /// Import one exporter-owned NT handle as a linear buffer. The exporter must
    /// release to EXTERNAL and finish its producer frame before resident evaluate.
    pub fn import_external_input(
        &mut self,
        slot: RrInputSlot,
        desc: &ExternalBufferImportDesc,
        exporter_luid: [u8; 8],
    ) -> Result<(), VendorError> {
        self.admit_native()?;
        let i = slot as usize;
        if self.imports[i].is_some() {
            return Err(bad("RR external input slot already imported"));
        }
        let need = u64::from(self.config.input_size.0)
            * u64::from(self.config.input_size.1)
            * INPUT_BPP[i];
        if desc.size != need {
            return Err(bad(format!(
                "RR external {slot:?} expects exactly {need} bytes, got {}",
                desc.size
            )));
        }
        let buffer = self.import_buffer(desc, exporter_luid, VK_BUFFER_USAGE_TRANSFER_SRC)?;
        self.imports[i] = Some(buffer);
        Ok(())
    }
    /// Import a caller-owned linear output target. It must be idle/released before
    /// RR writes; the consumer acquires only after evaluate completes. RGBA16F tight.
    pub fn import_external_output(
        &mut self,
        desc: &ExternalBufferImportDesc,
        exporter_luid: [u8; 8],
    ) -> Result<(), VendorError> {
        self.admit_native()?;
        if self.external_output.is_some() {
            return Err(bad("RR external output already imported"));
        }
        let need = u64::from(self.config.output_size.0) * u64::from(self.config.output_size.1) * 8;
        if desc.size != need {
            return Err(bad(format!("RR external output expects {need} bytes")));
        }
        self.external_output =
            Some(self.import_buffer(desc, exporter_luid, VK_BUFFER_USAGE_TRANSFER_DST)?);
        Ok(())
    }
    fn import_buffer(
        &self,
        desc: &ExternalBufferImportDesc,
        luid: [u8; 8],
        usage: u32,
    ) -> Result<Buffer, VendorError> {
        if !self.core.external_memory_enabled || self.core.device_luid != Some(luid) {
            return Err(VendorError::DeviceUnavailable(
                "RR buffer import needs enabled Win32 external memory and identical adapter LUID"
                    .into(),
            ));
        }
        if desc.handle == 0
            || desc.size == 0
            || desc.allocation_size < desc.size
            || desc.memory_type_index >= 32
        {
            return Err(bad("invalid RR external buffer descriptor"));
        }
        let ex = VkExternalMemoryBufferCreateInfo {
            s_type: VK_STRUCTURE_TYPE_EXTERNAL_MEMORY_BUFFER_CREATE_INFO,
            p_next: std::ptr::null(),
            handle_types: VK_EXTERNAL_MEMORY_HANDLE_TYPE_OPAQUE_WIN32,
        };
        let ci = VkBufferCreateInfo {
            s_type: VK_STRUCTURE_TYPE_BUFFER_CREATE_INFO,
            p_next: (&ex as *const VkExternalMemoryBufferCreateInfo).cast(),
            flags: 0,
            size: desc.size,
            usage,
            sharing_mode: 0,
            queue_family_index_count: 0,
            p_queue_family_indices: std::ptr::null(),
        };
        let mut raw = 0;
        // SAFETY: U63, imported object is linear and created in this RR device.
        vk_check("vkCreateBuffer(RR import)", unsafe {
            (self.core.dev.create_buffer)(self.core.device, &ci, std::ptr::null(), &mut raw)
        })?;
        let mut req = VkMemoryRequirements::default();
        // SAFETY: U63, requirements output references the created buffer only.
        unsafe { (self.core.dev.get_buffer_memory_requirements)(self.core.device, raw, &mut req) };
        if req.size > desc.allocation_size
            || req.memory_type_bits & (1 << desc.memory_type_index) == 0
        {
            // SAFETY: U63, unbound buffer is not used by GPU work.
            unsafe { (self.core.dev.destroy_buffer)(self.core.device, raw, std::ptr::null()) };
            return Err(bad(
                "RR imported allocation fails actual memory requirements",
            ));
        }
        let dedicated = VkMemoryDedicatedAllocateInfo {
            s_type: VK_STRUCTURE_TYPE_MEMORY_DEDICATED_ALLOCATE_INFO,
            p_next: std::ptr::null(),
            image: 0,
            buffer: raw,
        };
        let imp = VkImportMemoryWin32HandleInfoKHR {
            s_type: VK_STRUCTURE_TYPE_IMPORT_MEMORY_WIN32_HANDLE_INFO_KHR,
            p_next: (&dedicated as *const VkMemoryDedicatedAllocateInfo).cast(),
            handle_type: VK_EXTERNAL_MEMORY_HANDLE_TYPE_OPAQUE_WIN32,
            handle: desc.handle as *mut c_void,
            name: std::ptr::null(),
        };
        let ai = VkMemoryAllocateInfo {
            s_type: VK_STRUCTURE_TYPE_MEMORY_ALLOCATE_INFO,
            p_next: (&imp as *const VkImportMemoryWin32HandleInfoKHR).cast(),
            allocation_size: desc.allocation_size,
            memory_type_index: desc.memory_type_index,
        };
        let mut memory = 0;
        // SAFETY: U63, LUID/requirements checked; NT handle remains exporter-owned.
        let r = unsafe {
            (self.core.dev.allocate_memory)(self.core.device, &ai, std::ptr::null(), &mut memory)
        };
        if r != VK_SUCCESS {
            // SAFETY: U63, no imported memory reference was acquired on failure.
            unsafe { (self.core.dev.destroy_buffer)(self.core.device, raw, std::ptr::null()) };
            return Err(bad(format!("RR import allocation → {r}")));
        }
        // SAFETY: U63, allocation and dedicated buffer refer to the same shared linear bytes.
        let r = unsafe { (self.core.dev.bind_buffer_memory)(self.core.device, raw, memory, 0) };
        if r != VK_SUCCESS {
            release_buffer(&self.core.dev, self.core.device, raw, memory);
            return Err(bad(format!("RR import bind → {r}")));
        }
        Ok(Buffer {
            raw,
            memory,
            size: desc.size,
            allocation: desc.allocation_size,
        })
    }
    /// Uses all currently imported slots. Required six guides and either specular
    /// motion or hit distance must exist. Optional masks absent at import are zero.
    /// Output remains in the owned image and, if imported, the external output buffer.
    pub fn evaluate_resident(&mut self, params: &RrFrameParams) -> Result<(), VendorError> {
        self.admit_native()?;
        validate_params(params)?;
        let mut active = std::array::from_fn(|i| self.imports[i].is_some());
        if !active[..6].iter().all(|x| *x) || (!active[6] && !active[9]) {
            return Err(bad(
                "RR resident requires all surface guides and specular MV or hit distance",
            ));
        }
        // The mask images are independent and are explicitly zero-filled if absent.
        active[7] = true;
        active[8] = true;
        let result = self.frame(params, active, true, None);
        if result.is_err() {
            self.discard_frame();
        }
        result
    }
    /// Explicit finite-run cleanup. Previously published frame facts remain
    /// valid; an unconfirmed teardown is a separate run failure, not a rollback.
    pub fn shutdown(mut self) -> Result<(), RrShutdownFailure> {
        let outcome=self.finalize_teardown();
        if outcome.released && outcome.vk_result==Some(VK_SUCCESS) {Ok(())}
        else {Err(RrShutdownFailure{vk_result:outcome.vk_result,resources_retained:!outcome.released,sdk_failure:outcome.sdk_failure})}
    }
    pub fn readback_output_into(&mut self, out: &mut Vec<f32>) -> Result<(), VendorError> {
        self.admit_native()?;
        if self.evaluated.is_none() || self.poisoned || !self.output_valid {
            return Err(bad("RR has no completed output"));
        }
        out.resize(
            self.config.output_size.0 as usize * self.config.output_size.1 as usize * 3,
            0.,
        );
        self.core.readback_output_into(out.as_mut_slice())?;
        validate_output(out)
    }

    fn frame(
        &mut self,
        p: &RrFrameParams,
        active: [bool; SLOT_COUNT],
        external: bool,
        output: Option<&mut [f32]>,
    ) -> Result<(), VendorError> {
        if self.teardown.is_some(){return Err(bad("RR session already shut down"));}
        self.core.rr_present.as_mut().ok_or_else(||bad("RR WSI owner missing"))?.check_thread()?;
        if self.poisoned {
            return Err(bad("RR device/session is poisoned; recreate it"));
        }
        validate_params(p)?;
        // The owned output can be overwritten by this attempt. Retain historical
        // identities for audit, but never expose a previous id for new failed bytes.
        self.output_valid = false;
        let start = Instant::now();
        self.timings = RrFrameTimings::default();
        if self.recreate_pending {
            // SAFETY: U63, previous evaluate completed or recording was aborted; no pending GPU writes.
            sl_check("slFreeResources(RR discard)", unsafe {
                (self.core.fns.sl_free_resources)(FEATURE, &self.core.viewport)
            })?;
            self.recreate_pending = false;
        }
        let reset = p.reset || self.reset_pending || self.published.is_none();
        if let Some(last) = self.evaluated {
            if self.published != Some(last) && !reset {
                return Err(bad(
                    "RR previous evaluation was not published; publish or discard it first",
                ));
            }
            if p.frame_index <= last && !reset {
                return Err(bad("RR frame index rollback requires reset"));
            }
        }
        let mut token = std::ptr::null_mut();
        // SAFETY: U63, frame token output and immutable application frame index are valid.
        sl_check("slGetNewFrameToken(RR)", unsafe {
            (self.core.fns.sl_get_new_frame_token)(&mut token, &p.frame_index)
        })?;
        if token.is_null() {
            return Err(bad("RR frame token is null"));
        }
        let mut already_set = false;
        if let Some(cached) = self.allocation_constants.take() {
            if p.frame_index == 0 {
                let mut actual = *p;
                actual.reset = reset;
                if cached != actual {
                    return Err(bad(
                        "RR frame-zero camera/constants changed after allocation; recreate the session",
                    ));
                }
                already_set = true;
            }
        }
        if !already_set {
            let constants = constants(p, self.config.input_size, reset);
            // SAFETY: U63, complete 456-byte constants; token and viewport match.
            sl_check("slSetConstants(RR)", unsafe {
                (self.core.fns.sl_set_constants)(&constants, token, &self.core.viewport)
            })?;
        }
        self.set_frame_options(p)?;
        self.timings.bookkeeping_ms = start.elapsed().as_secs_f64() * 1000.;
        if external {
            // Missing optional masks are independently initialized to zero in staging.
            let buffer = self
                .staging
                .as_ref()
                .ok_or_else(|| bad("RR staging missing"))?;
            let mut pointer = std::ptr::null_mut();
            // SAFETY: U63, idle coherent staging is exclusively mapped here.
            vk_check("vkMapMemory(RR optional masks)", unsafe {
                (self.core.dev.map_memory)(
                    self.core.device,
                    buffer.memory,
                    0,
                    buffer.size,
                    0,
                    &mut pointer,
                )
            })?;
            if pointer.is_null() {
                return Err(bad("RR optional mask staging map null"));
            }
            for slot in [7usize, 8] {
                if self.imports[slot].is_none() {
                    let n = (u64::from(self.config.input_size.0)
                        * u64::from(self.config.input_size.1)
                        * 4) as usize;
                    // SAFETY: U63, offset/length are bounded by persistent staging layout.
                    unsafe {
                        std::ptr::write_bytes(
                            pointer.cast::<u8>().add(self.offsets[slot] as usize),
                            0,
                            n,
                        )
                    };
                }
            }
            // SAFETY: U63, four-byte exposure region is inside the mapped staging allocation.
            unsafe {
                std::ptr::copy_nonoverlapping(
                    1.0f32.to_le_bytes().as_ptr(),
                    pointer.cast::<u8>().add(self.exposure_offset as usize),
                    4,
                )
            };
            // SAFETY: U63, all pointer accesses ended and staging is coherent.
            unsafe { (self.core.dev.unmap_memory)(self.core.device, buffer.memory) };
        }
        let begin = VkCommandBufferBeginInfo {
            s_type: VK_STRUCTURE_TYPE_COMMAND_BUFFER_BEGIN_INFO,
            p_next: std::ptr::null(),
            flags: 1,
            p_inheritance_info: std::ptr::null(),
        };
        // SAFETY: U63, prior queue work is complete and command buffer was reset.
        vk_check("vkBeginCommandBuffer(RR)", unsafe {
            (self.core.dev.begin_command_buffer)(self.core.cmd, &begin)
        })?;
        let old_layouts: Vec<_> = self.inputs.iter().map(|x| x.res.layout).collect();
        let old_output_layout = self.core.color_out.layout;
        let recorded = self.record(p, active, external, output.is_some(), token);
        if let Err(error) = recorded {
            // SAFETY: U63, end/reset an unsubmitted recording; no invalid output is published.
            unsafe {
                let _ = (self.core.dev.end_command_buffer)(self.core.cmd);
            }
            // SAFETY: U63, recording was ended and never submitted.
            unsafe {
                let _ = (self.core.dev.reset_command_buffer)(self.core.cmd, 0);
            }
            for (image, layout) in self.inputs.iter_mut().zip(old_layouts) {
                image.res.layout = layout;
            }
            self.core.color_out.layout = old_output_layout;
            self.reset_pending = true;
            self.recreate_pending = true;
            return Err(error);
        }
        // SAFETY: U63, all record calls finished and no borrowed stack resources survive recording.
        vk_check("vkEndCommandBuffer(RR)", unsafe {
            (self.core.dev.end_command_buffer)(self.core.cmd)
        })?;
        let submission = VkSubmitInfo {
            s_type: VK_STRUCTURE_TYPE_SUBMIT_INFO,
            p_next: std::ptr::null(),
            wait_semaphore_count: 0,
            p_wait_semaphores: std::ptr::null(),
            p_wait_dst_stage_mask: std::ptr::null(),
            command_buffer_count: 1,
            p_command_buffers: &self.core.cmd,
            signal_semaphore_count: 0,
            p_signal_semaphores: std::ptr::null(),
        };
        let wait = Instant::now();
        // SAFETY: U63, executable command buffer, one queue, external producer completion guaranteed by caller.
        let r = unsafe { (self.core.dev.queue_submit)(self.core.queue, 1, &submission, 0) };
        if r != VK_SUCCESS {
            self.poisoned = true;
            return Err(bad(format!("vkQueueSubmit(RR) → {r}")));
        }
        // SAFETY: U63, wait establishes completion before caller reuses any shared bytes or publishes output.
        let r = unsafe { (self.core.dev.queue_wait_idle)(self.core.queue) };
        if r != VK_SUCCESS {
            self.poisoned = true;
            return Err(bad(format!("vkQueueWaitIdle(RR) → {r}")));
        }
        self.timings.submit_wait_ms = wait.elapsed().as_secs_f64() * 1000.;
        let present_result=self.core.rr_present.as_mut().ok_or_else(||bad("RR WSI owner missing"))?.frame_end(p.frame_index);
        if let Err(error)=present_result {
            self.poisoned=true;self.discard_frame();return Err(error);
        }
        self.read_timestamps();
        let readback = Instant::now();
        if let Some(out) = output {
            self.core.map_convert_readback(out)?;
            validate_output(out)?;
        }
        self.timings.readback_ms = readback.elapsed().as_secs_f64() * 1000.;
        // SAFETY: U63, queue is idle; the sole recording slot is reusable.
        vk_check("vkResetCommandBuffer(RR)", unsafe {
            (self.core.dev.reset_command_buffer)(self.core.cmd, 0)
        })?;
        if self.core.validation_errors() != 0 {
            return Err(bad("RR Vulkan validation error; output rejected"));
        }
        self.query_vram()?;
        self.loaded_dll_paths = verify_loaded_modules()?;
        self.evaluated = Some(p.frame_index);
        self.output_valid = true;
        self.evaluations += 1;
        if reset {
            self.resets += 1;
        }
        self.reset_pending = false;
        self.timings.total_wall_ms = start.elapsed().as_secs_f64() * 1000.;
        Ok(())
    }
    fn record(
        &mut self,
        p: &RrFrameParams,
        active: [bool; SLOT_COUNT],
        external: bool,
        readback: bool,
        token: *mut c_void,
    ) -> Result<(), VendorError> {
        // SAFETY: U63, pool is owned and command buffer is recording; all four queries are reset before writes.
        unsafe { (self.core.dev.cmd_reset_query_pool)(self.core.cmd, self.query_pool, 0, 4) };
        self.timestamp(0);
        for slot in 0..SLOT_COUNT {
            if active[slot] {
                let source = if external {
                    self.imports[slot].as_ref().map(|b| b.raw)
                } else {
                    None
                };
                if let Some(buffer) = source {
                    self.buffer_barrier(
                        buffer,
                        self.imports[slot].as_ref().unwrap().size,
                        true,
                        false,
                    );
                }
                let raw = source.unwrap_or_else(|| self.staging.as_ref().unwrap().raw);
                self.upload(
                    slot,
                    raw,
                    if source.is_some() {
                        0
                    } else {
                        self.offsets[slot]
                    },
                );
            }
        }
        self.upload(
            SLOT_COUNT,
            self.staging.as_ref().unwrap().raw,
            self.exposure_offset,
        );
        if self.core.color_out.layout == VK_IMAGE_LAYOUT_UNDEFINED {
            let image = self.core.color_out.clone_shallow();
            self.core.vk_barrier(
                &image,
                VK_IMAGE_LAYOUT_UNDEFINED,
                VK_IMAGE_LAYOUT_GENERAL,
                0,
                VK_ACCESS_SHADER_WRITE,
                VK_PIPELINE_STAGE_TOP_OF_PIPE,
                VK_PIPELINE_STAGE_COMPUTE_SHADER,
            );
            self.core.color_out.layout = VK_IMAGE_LAYOUT_GENERAL;
        }
        self.timestamp(1);
        let mut resources = Vec::new();
        let mut types = Vec::new();
        for slot in 0..SLOT_COUNT {
            if active[slot] {
                resources.push(sl_resource(&self.inputs[slot].res, false));
                types.push(
                    if slot == 1 && p.depth_domain == RrDepthDomain::LinearView {
                        49
                    } else {
                        SLOT_TAG[slot]
                    },
                );
            }
        }
        resources.push(sl_resource(&self.inputs[SLOT_COUNT].res, false));
        types.push(13);
        resources.push(sl_resource(&self.core.color_out, true));
        types.push(SL_BUFFER_SCALING_OUTPUT_COLOR);
        let tags: Vec<_> = resources
            .iter()
            .zip(types)
            .map(|(resource, tag_type)| SlResourceTag {
                base: sl_base(SL_GUID_RESOURCE_TAG, 1),
                resource,
                tag_type,
                lifecycle: RR_LIFECYCLE_SESSION,
                extent: SlExtent {
                    top: 0,
                    left: 0,
                    width: resource.width,
                    height: resource.height,
                },
            })
            .collect();
        // Upload commands and barriers above precede the vendor work. Refresh
        // the frame-based cache with their actual SHADER_READ/GENERAL states;
        // preallocation's undefined metadata must not survive into evaluation.
        // SAFETY: U63, session-owned resources/tags remain live, exact token and viewport.
        sl_check("slSetTagForFrame(RR current)", unsafe {
            (self.set_tags_for_frame)(
                token,
                &self.core.viewport,
                tags.as_ptr(),
                tags.len() as u32,
                self.core.cmd,
            )
        })?;
        let pointers: [*const SlBaseStructure; 1] = [&self.core.viewport.base];
        // SAFETY: U63, exact RR feature, token, viewport and complete initialized tags live through evaluate.
        sl_check("slEvaluateFeature(DLSS_RR)", unsafe {
            (self.core.fns.sl_evaluate_feature)(
                FEATURE,
                token,
                pointers.as_ptr(),
                pointers.len() as u32,
                self.core.cmd,
            )
        })?;
        self.timestamp(2);
        if readback {
            self.core.record_output_readback();
        }
        if let Some(output) = self.external_output.as_ref() {
            let (buffer, size) = (output.raw, output.size);
            self.buffer_barrier(buffer, size, true, true);
            let image = self.core.color_out.clone_shallow();
            self.core.vk_barrier(
                &image,
                VK_IMAGE_LAYOUT_GENERAL,
                VK_IMAGE_LAYOUT_TRANSFER_SRC_OPTIMAL,
                VK_ACCESS_SHADER_WRITE,
                VK_ACCESS_TRANSFER_READ,
                VK_PIPELINE_STAGE_COMPUTE_SHADER,
                VK_PIPELINE_STAGE_TRANSFER,
            );
            let region = copy_region(
                self.config.output_size.0,
                self.config.output_size.1,
                0,
                VK_IMAGE_ASPECT_COLOR,
            );
            // SAFETY: U63, output target has exact tight RGBA16F capacity, acquired exclusively from exporter.
            unsafe {
                (self.core.dev.cmd_copy_image_to_buffer)(
                    self.core.cmd,
                    image.image,
                    VK_IMAGE_LAYOUT_TRANSFER_SRC_OPTIMAL,
                    buffer,
                    1,
                    &region,
                )
            };
            self.core.vk_barrier(
                &image,
                VK_IMAGE_LAYOUT_TRANSFER_SRC_OPTIMAL,
                VK_IMAGE_LAYOUT_GENERAL,
                VK_ACCESS_TRANSFER_READ,
                VK_ACCESS_SHADER_WRITE,
                VK_PIPELINE_STAGE_TRANSFER,
                VK_PIPELINE_STAGE_COMPUTE_SHADER,
            );
            self.buffer_barrier(buffer, size, false, true);
        }
        if external {
            for slot in 0..SLOT_COUNT {
                if active[slot] {
                    if let Some(buffer) = self.imports[slot].as_ref() {
                        let (raw, size) = (buffer.raw, buffer.size);
                        self.buffer_barrier(raw, size, false, false);
                    }
                }
            }
        }
        self.timestamp(3);
        Ok(())
    }
    fn timestamp(&self, index: u32) {
        // SAFETY: U63, command buffer recording, owned four-slot pool, index in 0..4.
        unsafe {
            (self.core.dev.cmd_write_timestamp)(
                self.core.cmd,
                VK_PIPELINE_STAGE_BOTTOM_OF_PIPE,
                self.query_pool,
                index,
            )
        };
    }
    fn read_timestamps(&mut self) {
        let mut values = [0u64; 4];
        // SAFETY: U63, queue idle guarantees queries complete; four u64 outputs with stride8.
        let r = unsafe {
            (self.core.dev.get_query_pool_results)(
                self.core.device,
                self.query_pool,
                0,
                4,
                size_of::<[u64; 4]>(),
                values.as_mut_ptr().cast(),
                8,
                VK_QUERY_RESULT_64_BIT,
            )
        };
        if r == VK_SUCCESS && self.core.timestamp_period_ns > 0. {
            let scale = f64::from(self.core.timestamp_period_ns) / 1e6;
            self.timings.copy_in_gpu_ms = Some(values[1].wrapping_sub(values[0]) as f64 * scale);
            self.timings.rr_gpu_ms = Some(values[2].wrapping_sub(values[1]) as f64 * scale);
            self.timings.copy_out_gpu_ms = Some(values[3].wrapping_sub(values[2]) as f64 * scale);
        }
    }
    fn upload(&mut self, slot: usize, buffer: VkBuffer, offset: u64) {
        let image = self.inputs[slot].res.clone_shallow();
        let first = image.layout == VK_IMAGE_LAYOUT_UNDEFINED;
        self.core.vk_barrier(
            &image,
            image.layout,
            VK_IMAGE_LAYOUT_TRANSFER_DST_OPTIMAL,
            if first { 0 } else { VK_ACCESS_SHADER_READ },
            VK_ACCESS_TRANSFER_WRITE,
            if first {
                VK_PIPELINE_STAGE_TOP_OF_PIPE
            } else {
                VK_PIPELINE_STAGE_COMPUTE_SHADER
            },
            VK_PIPELINE_STAGE_TRANSFER,
        );
        let region = copy_region(image.w, image.h, offset, VK_IMAGE_ASPECT_COLOR);
        // SAFETY: U63, buffer has exact format/extent bytes, image belongs to RR and is transfer destination.
        unsafe {
            (self.core.dev.cmd_copy_buffer_to_image)(
                self.core.cmd,
                buffer,
                image.image,
                VK_IMAGE_LAYOUT_TRANSFER_DST_OPTIMAL,
                1,
                &region,
            )
        };
        self.core.vk_barrier(
            &image,
            VK_IMAGE_LAYOUT_TRANSFER_DST_OPTIMAL,
            VK_IMAGE_LAYOUT_SHADER_READ_ONLY_OPTIMAL,
            VK_ACCESS_TRANSFER_WRITE,
            VK_ACCESS_SHADER_READ,
            VK_PIPELINE_STAGE_TRANSFER,
            VK_PIPELINE_STAGE_COMPUTE_SHADER,
        );
        self.inputs[slot].res.layout = VK_IMAGE_LAYOUT_SHADER_READ_ONLY_OPTIMAL;
    }
    fn buffer_barrier(&self, buffer: VkBuffer, size: u64, acquire: bool, write: bool) {
        let access = if write {
            VK_ACCESS_TRANSFER_WRITE
        } else {
            VK_ACCESS_TRANSFER_READ
        };
        let barrier = VkBufferMemoryBarrier {
            s_type: 44,
            p_next: std::ptr::null(),
            src_access_mask: if acquire { 0 } else { access },
            dst_access_mask: if acquire { access } else { 0 },
            src_queue_family_index: if acquire {
                VK_QUEUE_FAMILY_EXTERNAL
            } else {
                self.core.queue_family
            },
            dst_queue_family_index: if acquire {
                self.core.queue_family
            } else {
                VK_QUEUE_FAMILY_EXTERNAL
            },
            buffer,
            offset: 0,
            size,
        };
        // SAFETY: U63, buffer is an imported linear resource; paired external ownership transfer,
        // caller serializes producer/RR completion and no other device may access it concurrently.
        unsafe {
            (self.core.dev.cmd_pipeline_barrier)(
                self.core.cmd,
                if acquire {
                    VK_PIPELINE_STAGE_TOP_OF_PIPE
                } else {
                    VK_PIPELINE_STAGE_TRANSFER
                },
                if acquire {
                    VK_PIPELINE_STAGE_TRANSFER
                } else {
                    VK_PIPELINE_STAGE_BOTTOM_OF_PIPE
                },
                0,
                0,
                std::ptr::null(),
                1,
                (&barrier as *const VkBufferMemoryBarrier).cast(),
                0,
                std::ptr::null(),
            )
        };
    }
}

// U63_FINAL_TEARDOWN_BEGIN
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub struct RrSdkShutdownFailure {pub operation:&'static str,pub result:i32}

/// Final SDK retirement delegates all existing feature viewports to the global
/// shutdown contract. No allocation-state guess or per-feature free is needed.
/// SAFETY: U63, caller owns the initialized SDK and has completed native/WSI use.
unsafe fn shutdown_sdk_once(
    done:&mut bool,shutdown:unsafe extern "system" fn()->i32,
)->Result<(),RrSdkShutdownFailure>{
    if *done{return Ok(());}
    // SAFETY: U63, caller's actual SDK function table and final retirement proof.
    let result=unsafe{shutdown()};
    if result!=0{return Err(RrSdkShutdownFailure{operation:"slShutdown",result});}
    *done=true;Ok(())
}


#[derive(Clone,Copy)]
struct RrShutdownOutcome {vk_result:Option<i32>,released:bool,sdk_failure:Option<RrSdkShutdownFailure>}
#[derive(Clone,Copy,Debug)]
pub struct RrShutdownFailure {
    pub vk_result:Option<i32>,
    pub resources_retained:bool,
    pub sdk_failure:Option<RrSdkShutdownFailure>,
}
impl std::fmt::Display for RrShutdownFailure {
    fn fmt(&self,f:&mut std::fmt::Formatter<'_>)->std::fmt::Result {
        if let Some(sdk)=self.sdk_failure {
            write!(f,"RR teardown failed during SDK {}: Streamline result {}; resources_retained={}; worker restart required; see setup_lifecycle journal",sdk.operation,sdk.result,self.resources_retained)
        } else if !self.resources_retained && self.vk_result==Some(-4) {
            write!(f,"RR final device wait returned VK_ERROR_DEVICE_LOST; resources safely destroyed; run cleanup failed")
        } else if let Some(result)=self.vk_result {
            write!(f,"RR teardown failed; actual final device wait {result}; resources_retained={}; see setup_lifecycle journal",self.resources_retained)
        } else {
            write!(f,"RR teardown failed before native/SDK retirement was established; resources_retained={}; see setup_lifecycle journal",self.resources_retained)
        }
    }
}
impl std::error::Error for RrShutdownFailure {}
/// U63: only RR calls this. Presentation proof is checked before the existing
/// device-lost destruction allowance; SDK shutdown precedes any WSI destruction.
pub(super) fn retire_core(core:&mut DlssVkSession)->Result<i32,VendorError>{
    core.rr_present.as_mut().ok_or_else(||bad("RR WSI owner missing at teardown"))?.proof()?;
    let started=Instant::now();
    // SAFETY: U63, exclusively owned RR device; WSI presentation payloads already retired.
    let result=unsafe{(core.dev.device_wait_idle)(core.device)};
    if result!=0&&result!=-4 {
        let error=crate::render_exec::quarantine_hq_vendor_rr_final_wait(result,started.elapsed().as_nanos().min(u64::MAX as u128) as u64,core.device as usize,core.cmd as usize);
        return Err(bad(error));
    }
    core.rr_teardown_idle_confirmed=true;
    if !core.shutdown_done {
        // The official global shutdown owns teardown of all existing SDK
        // viewports, including zero-evaluation sessions. Loaded SR is not an
        // allocated SR viewport; no speculative per-feature free belongs here.
        // SAFETY: U63, actual WSI and device completion passed; SDK context and function table remain live.
        let shutdown=unsafe{shutdown_sdk_once(&mut core.shutdown_done,core.fns.sl_shutdown)};
        if let Err(failure)=shutdown{return Err(core.rr_present.as_mut().expect("RR WSI owner").shutdown_failed(failure.result));}
    }
    core.rr_present.as_mut().expect("RR WSI owner").destroy_after_shutdown()?;
    Ok(result)
}
impl DlssRrVkSession {
    fn finalize_teardown(&mut self)->RrShutdownOutcome {
        if let Some(outcome)=self.teardown{return outcome;}
        self.teardown=Some(RrShutdownOutcome{vk_result:None,released:false,sdk_failure:None});
        let inputs=&self.inputs;let imports=&self.imports;let staging=&self.staging;
        let output=&self.external_output;let query_pool=self.query_pool;let mut wait_result=None;let mut sdk_failure=None;
        let admit=|core:&mut DlssVkSession|match retire_core(core){
            Ok(result)=>{wait_result=Some(result);true},Err(error)=>{sdk_failure=core.rr_present.as_ref().and_then(|p|p.sdk_failure());eprintln!("HQ_RR_TEARDOWN_RETAIN {error}");false}
        };
        let release=|core:&mut DlssVkSession|{
            if query_pool!=0 {
                // SAFETY: U63, SDK shut down and actual final idle admitted resource release.
                unsafe{(core.dev.destroy_query_pool)(core.device,query_pool,std::ptr::null())};
            }
            for image in inputs {release_image(&core.dev,core.device,image.res.image,image.res.memory,image.res.view);}
            for buffer in imports.iter().flatten().chain(staging.iter()).chain(output.iter()) {release_buffer(&core.dev,core.device,buffer.raw,buffer.memory);}
        };
        // SAFETY: U63/U32, the sole owner drops only after WSI, device and SDK proofs; otherwise its entire graph is retained.
        let released=unsafe{rr_shutdown::finish_owner_admitted(&mut self.core,crate::render_exec::hq_vendor_worker_quarantined(),admit,release)};
        if !released {
            self.poisoned=true;self.output_valid=false;
            std::mem::forget((std::mem::take(&mut self.inputs),std::mem::take(&mut self.imports),self.staging.take(),self.external_output.take()));
        }
        let outcome=RrShutdownOutcome{vk_result:wait_result,released,sdk_failure};self.teardown=Some(outcome);outcome
    }
}
impl Drop for DlssRrVkSession {
    fn drop(&mut self) {let _=self.finalize_teardown();}
}
fn sl_resource(image: &VkImageRes, output: bool) -> SlResource {
    SlResource {
        base: sl_base(SL_GUID_RESOURCE, 1),
        res_type: SL_RESOURCE_TEX2D,
        _pad0: [0; 7],
        native: image.image as usize as *mut c_void,
        memory: image.memory as usize as *mut c_void,
        view: image.view as usize as *mut c_void,
        state: image.layout as u32,
        width: image.w,
        height: image.h,
        native_format: image.format,
        mip_levels: 1,
        array_layers: 1,
        gpu_virtual_address: 0,
        flags: 0,
        usage: if output {
            VK_IMAGE_USAGE_STORAGE | VK_IMAGE_USAGE_TRANSFER_SRC
        } else {
            VK_IMAGE_USAGE_SAMPLED | VK_IMAGE_USAGE_TRANSFER_DST
        },
        reserved: 0,
    }
}
fn copy_region(w: u32, h: u32, offset: u64, aspect: u32) -> VkBufferImageCopy {
    VkBufferImageCopy {
        buffer_offset: offset,
        buffer_row_length: 0,
        buffer_image_height: 0,
        image_subresource: VkImageSubresourceLayers {
            aspect_mask: aspect,
            mip_level: 0,
            base_array_layer: 0,
            layer_count: 1,
        },
        image_offset: [0; 3],
        image_extent: VkExtent3D {
            width: w,
            height: h,
            depth: 1,
        },
    }
}

fn identity() -> [[f32; 4]; 4] {
    [
        [1., 0., 0., 0.],
        [0., 1., 0., 0.],
        [0., 0., 1., 0.],
        [0., 0., 0., 1.],
    ]
}
fn options(
    c: &RrConfig,
    pre: f32,
    scale: f32,
    wv: [[f32; 4]; 4],
    vw: [[f32; 4]; 4],
) -> SlRrOptions {
    SlRrOptions {
        base: sl_base(GUID_OPTIONS, 3),
        mode: c.mode.sl(),
        output_width: c.output_size.0,
        output_height: c.output_size.1,
        sharpness: 0.,
        pre_exposure: pre,
        exposure_scale: scale,
        color_buffers_hdr: 1,
        indicator_x: 0,
        indicator_y: 0,
        pad0: 0,
        normal_roughness_mode: 1,
        world_to_view: wv,
        view_to_world: vw,
        alpha_upscaling: 0,
        pad1: [0; 3],
        presets: [c.preset; 6],
    }
}
fn compatible_sr_options(c: &RrConfig, p: &RrFrameParams) -> SlDlssOptions {
    SlDlssOptions {
        base: sl_base(SL_GUID_DLSS_OPTIONS, 3),
        mode: c.mode.sl(),
        output_width: c.output_size.0,
        output_height: c.output_size.1,
        sharpness: 0.,
        pre_exposure: p.pre_exposure,
        exposure_scale: p.exposure_scale,
        color_buffers_hdr: 1,
        indicator_invert_axis_x: 0,
        indicator_invert_axis_y: 0,
        _pad0: 0,
        dlaa_preset: 0,
        quality_preset: 0,
        balanced_preset: 0,
        performance_preset: 0,
        ultra_performance_preset: 0,
        ultra_quality_preset: 0,
        use_auto_exposure: 0,
        alpha_upscaling_enabled: 0,
        _pad1: [0; 2],
    }
}
fn validate_config(c: &RrConfig) -> Result<(), VendorError> {
    for size in [c.input_size, c.output_size] {
        if size.0 == 0
            || size.1 == 0
            || u64::from(size.0) * u64::from(size.1) > usize::MAX as u64 / 256
        {
            return Err(bad("invalid RR extent"));
        }
    }
    if c.mode == RrMode::Native && c.input_size != c.output_size {
        return Err(bad("RR Native requires identical input/output extent"));
    }
    if !matches!(c.preset, 4 | 5 | 6) {
        return Err(bad(
            "RR preset must explicitly select D(4), E(5) or F(6); default/OTA presets are refused",
        ));
    }
    Ok(())
}
fn verify_package(root: &Path) -> Result<(), VendorError> {
    for (name, expected) in PRODUCTION_DLLS {
        let (actual, _) = sha256_file(&root.join("bin/x64").join(name))?;
        if actual != expected {
            return Err(bad(format!(
                "RR official production v2.14.16 pin mismatch for {name}: {actual}"
            )));
        }
    }
    Ok(())
}

#[cfg(windows)]
fn verify_loaded_modules() -> Result<Vec<String>, VendorError> {
    unsafe extern "system" {
        fn GetModuleHandleW(name: *const u16) -> *mut c_void;
        fn GetModuleFileNameW(module: *mut c_void, buffer: *mut u16, size: u32) -> u32;
    }
    let mut loaded = Vec::new();
    for (name, expected) in PRODUCTION_DLLS {
        let wide: Vec<u16> = name.encode_utf16().chain(std::iter::once(0)).collect();
        // SAFETY: U63, NUL-terminated module name; borrowed module handle is never closed.
        let module = unsafe { GetModuleHandleW(wide.as_ptr()) };
        // NGX can delay-load its implementation until evaluation; every loaded
        // member is checked again after evaluate, before publication is accepted.
        if module.is_null() {
            continue;
        }
        let mut path = vec![0u16; 32768];
        // SAFETY: U63, live borrowed module handle and writable UTF-16 buffer.
        let n =
            unsafe { GetModuleFileNameW(module, path.as_mut_ptr(), path.len() as u32) } as usize;
        if n == 0 || n >= path.len() {
            return Err(bad(format!("cannot identify loaded RR module {name}")));
        }
        let path = PathBuf::from(String::from_utf16_lossy(&path[..n]));
        let (actual, _) = sha256_file(&path)?;
        if actual != expected {
            return Err(bad(format!(
                "loaded RR module {name} is not pinned production v2.14.16: {}",
                path.display()
            )));
        }
        loaded.push(path.to_string_lossy().into_owned());
    }
    if loaded.len() < 2 {
        return Err(bad("loaded RR interposer/common identity unavailable"));
    }
    Ok(loaded)
}
#[cfg(not(windows))]
fn verify_loaded_modules() -> Result<Vec<String>, VendorError> {
    Err(VendorError::DeviceUnavailable(
        "this official RR package contains Win32 production DLLs".into(),
    ))
}

fn constants(p: &RrFrameParams, size: (u32, u32), reset: bool) -> SlConstants {
    let c = &p.camera;
    SlConstants {
        base: sl_base(SL_GUID_CONSTANTS, 2),
        camera_view_to_clip: c.view_to_clip,
        clip_to_camera_view: c.clip_to_view,
        clip_to_lens_clip: identity(),
        clip_to_prev_clip: c.clip_to_prev_clip,
        prev_clip_to_clip: c.prev_clip_to_clip,
        jitter_offset: p.jitter_pixels,
        mvec_scale: [1., 1.],
        camera_pinhole_offset: [0., 0.],
        camera_pos: c.position,
        camera_up: c.up,
        camera_right: c.right,
        camera_fwd: c.forward,
        camera_near: c.near,
        camera_far: c.far,
        camera_fov: c.fov_y_radians,
        camera_aspect_ratio: size.0 as f32 / size.1 as f32,
        motion_vectors_invalid_value: SL_INVALID_FLOAT,
        depth_inverted: i8::from(p.depth_domain == RrDepthDomain::DeviceZReversed),
        camera_motion_included: 1,
        motion_vectors_3d: 0,
        reset: i8::from(reset),
        orthographic_projection: 0,
        motion_vectors_dilated: 0,
        motion_vectors_jittered: 0,
        _pad0: 0,
        min_relative_linear_depth_object_separation: 40.,
    }
}
fn validate_params(p: &RrFrameParams) -> Result<(), VendorError> {
    let c = &p.camera;
    for matrix in [
        &c.view_to_clip,
        &c.clip_to_view,
        &c.clip_to_prev_clip,
        &c.prev_clip_to_clip,
        &c.world_to_view,
        &c.view_to_world,
    ] {
        if !matrix.iter().flatten().all(|x| x.is_finite()) {
            return Err(bad("RR matrix contains nonfinite values"));
        }
    }
    for vector in [c.position, c.up, c.right, c.forward] {
        if !vector.iter().all(|x| x.is_finite()) {
            return Err(bad("RR camera vector is nonfinite"));
        }
    }
    for vector in [c.up, c.right, c.forward] {
        let length = vector.iter().map(|x| x * x).sum::<f32>();
        if (length - 1.).abs() > 0.01 {
            return Err(bad("RR camera basis must contain unit vectors"));
        }
    }
    if !c.near.is_finite()
        || !c.far.is_finite()
        || c.near <= 0.
        || c.far <= c.near
        || !c.fov_y_radians.is_finite()
        || c.fov_y_radians <= 0.
        || c.fov_y_radians >= std::f32::consts::PI
    {
        return Err(bad("RR camera near/far/FOV are invalid"));
    }
    if !p.pre_exposure.is_finite()
        || p.pre_exposure <= 0.
        || !p.exposure_scale.is_finite()
        || p.exposure_scale <= 0.
        || !p.jitter_pixels.iter().all(|v| v.is_finite())
    {
        return Err(bad("RR exposure/jitter are invalid"));
    }
    Ok(())
}
fn validate_frame(input: &RrFrameInput<'_>, size: (u32, u32)) -> Result<(), VendorError> {
    validate_params(&input.params)?;
    let pixels = size.0 as usize * size.1 as usize;
    let exact = |name: &str, v: &[f32], channels: usize| -> Result<(), VendorError> {
        if v.len() != pixels * channels || !v.iter().all(|v| v.is_finite()) {
            return Err(bad(format!("RR {name}: wrong length or nonfinite values")));
        }
        Ok(())
    };
    for (name, v, channels) in [
        ("color", input.color, 3),
        ("normalRoughness", input.normals_roughness, 4),
        ("diffuseAlbedo", input.diffuse_albedo, 3),
        ("specularAlbedo", input.specular_albedo, 3),
        ("depth", input.depth, 1),
        ("motion", input.mv, 2),
    ] {
        exact(name, v, channels)?;
    }
    for (name, v, channels) in [
        ("specularMotion", input.specular_mv, 2),
        ("specularHitDistance", input.specular_hit_distance, 1),
        ("biasCurrentColor", input.bias_current_color, 1),
        ("transparency", input.transparency, 1),
    ] {
        if let Some(v) = v {
            exact(name, v, channels)?;
        }
    }
    if input.specular_mv.is_none() && input.specular_hit_distance.is_none() {
        return Err(bad(
            "RR requires coherent specular motion or primary-origin specular hit distance",
        ));
    }
    if [input.color, input.diffuse_albedo, input.specular_albedo]
        .iter()
        .any(|a| a.iter().any(|x| *x < 0.))
    {
        return Err(bad("RR radiance/reflectance contains negative values"));
    }
    for v in input.normals_roughness.chunks_exact(4) {
        let n = v[0] * v[0] + v[1] * v[1] + v[2] * v[2];
        if (n - 1.).abs() > 0.01 || !(0.0..=1.0).contains(&v[3]) {
            return Err(bad(
                "RR needs normalized world normals and roughness r in [0,1]",
            ));
        }
    }
    if input.depth.iter().any(|x| match input.params.depth_domain {
        RrDepthDomain::LinearView => *x <= 0.,
        _ => !(0.0..=1.0).contains(x),
    }) {
        return Err(bad("RR depth is outside the declared domain"));
    }
    if input
        .specular_hit_distance
        .is_some_and(|v| v.iter().any(|x| *x < 0.))
    {
        return Err(bad("RR specular hit distance is negative"));
    }
    for mask in [input.bias_current_color, input.transparency]
        .into_iter()
        .flatten()
    {
        if mask.iter().any(|v| !(0.0..=1.0).contains(v)) {
            return Err(bad("RR mask outside [0,1]"));
        }
    }
    Ok(())
}
fn validate_output(out: &[f32]) -> Result<(), VendorError> {
    if !out.iter().all(|v| v.is_finite() && *v >= 0.) {
        return Err(bad("RR HDR output contains negative or nonfinite values"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::mem::ManuallyDrop;
    use std::sync::{Arc,atomic::{AtomicUsize,Ordering}};
    use super::rr_shutdown::finish_owner_admitted;
    use std::cell::RefCell;
    #[derive(Default)]
    struct SdkMock {calls:usize,result:i32,live_features:usize,released_features:usize,events:Vec<&'static str>}
    thread_local!{static SDK:RefCell<SdkMock>=RefCell::new(SdkMock::default());}
    unsafe extern "system" fn fake_global_shutdown()->i32{
        SDK.with(|state|{let mut state=state.borrow_mut();state.calls+=1;state.events.push("slShutdown");
            if state.result==0{let live=state.live_features;state.released_features+=live;state.live_features=0;}state.result})
    }
    unsafe extern "system" fn must_not_repeat_shutdown()->i32{panic!("successful SDK shutdown was repeated")}
    #[test]
    fn global_shutdown_zero_eval_and_existing_rr_features_each_call_once(){
        for live_features in [0,1]{
            SDK.with(|s|*s.borrow_mut()=SdkMock{live_features,..Default::default()});
            let mut done=false;
            // SAFETY: U63 CPU fake FFI, helper uses the provided SDK stub; no native library is loaded.
            assert_eq!(unsafe{shutdown_sdk_once(&mut done,fake_global_shutdown)},Ok(()));
            assert!(done);
            // SAFETY: U63 CPU fixture, successful completion must skip even this deliberately failing stub.
            assert_eq!(unsafe{shutdown_sdk_once(&mut done,must_not_repeat_shutdown)},Ok(()));
            SDK.with(|s|{let s=s.borrow();assert_eq!(s.calls,1);assert_eq!(s.live_features,0);assert_eq!(s.released_features,live_features);assert_eq!(s.events,vec!["slShutdown"]);});
        }
    }
    #[test]
    fn sdk_rejection_including_invalid_parameter_never_admits_native_destruction(){
        struct Owner(Arc<AtomicUsize>);
        impl Drop for Owner{fn drop(&mut self){self.0.fetch_add(1,Ordering::SeqCst);}}
        for result in [25,21,24,39]{
            SDK.with(|s|*s.borrow_mut()=SdkMock{result,live_features:1,..Default::default()});
            let drops=Arc::new(AtomicUsize::new(0));let destroys=Arc::new(AtomicUsize::new(0));
            let mut owner=ManuallyDrop::new(Owner(drops.clone()));let mut done=false;let mut failure=None;
            let admit=|_:&mut Owner|{
                // SAFETY: U63 CPU fake FFI, actual global-shutdown helper is exercised through owner admission.
                let outcome=unsafe{shutdown_sdk_once(&mut done,fake_global_shutdown)};
                failure=outcome.err();outcome.is_ok()
            };
            let release=|_:&mut Owner|{destroys.fetch_add(1,Ordering::SeqCst);};
            // SAFETY: U63 CPU fixture, rejected admission deliberately retains the unique owner.
            let released=unsafe{finish_owner_admitted(&mut owner,false,admit,release)};
            assert!(!released);assert!(!done);assert_eq!(failure,Some(RrSdkShutdownFailure{operation:"slShutdown",result}));
            assert_eq!(drops.load(Ordering::SeqCst),0);assert_eq!(destroys.load(Ordering::SeqCst),0);
            // SAFETY: U63 CPU fixture, an already quarantined owner must not invoke any further SDK or release callback.
            assert!(!unsafe{finish_owner_admitted(&mut owner,true,|_|panic!("SDK retried"),|_|panic!("native destroy retried"))});
            SDK.with(|s|{let s=s.borrow();assert_eq!(s.calls,1);assert_eq!(s.live_features,1);assert_eq!(s.released_features,0);});
        }
    }
    #[test]
    fn successful_global_shutdown_precedes_native_destroy_and_owner_drop(){
        struct Owner;
        impl Drop for Owner{fn drop(&mut self){SDK.with(|s|s.borrow_mut().events.push("drop"));}}
        SDK.with(|s|*s.borrow_mut()=SdkMock{live_features:1,..Default::default()});
        let mut owner=ManuallyDrop::new(Owner);let mut done=false;
        let admit=|_:&mut Owner|{
            // SAFETY: U63 CPU fixture, this fake SDK follows the official global cleanup responsibility.
            unsafe{shutdown_sdk_once(&mut done,fake_global_shutdown)}.is_ok()
        };
        // SAFETY: U63 CPU fixture, native release and Drop are admitted only after actual helper success.
        let released=unsafe{finish_owner_admitted(&mut owner,false,admit,|_|SDK.with(|s|s.borrow_mut().events.push("native_destroy")))};
        assert!(released);assert!(done);SDK.with(|s|assert_eq!(s.borrow().events,vec!["slShutdown","native_destroy","drop"]));
    }

    #[test]
    fn sdk_shutdown_display_preserves_sdk_operation_and_never_fakes_vk_result(){
        let failure=RrShutdownFailure{vk_result:None,resources_retained:true,sdk_failure:Some(RrSdkShutdownFailure{operation:"slShutdown",result:25})};
        let message=failure.to_string();assert!(message.contains("SDK slShutdown"));assert!(message.contains("Streamline result 25"));
        assert!(!message.contains("final device wait"));assert_eq!(failure.vk_result,None);
        let missing=RrShutdownFailure{vk_result:None,resources_retained:true,sdk_failure:None};
        assert!(missing.to_string().contains("RR teardown failed"));assert!(!missing.to_string().contains("final device wait"));
    }

    #[test]
    fn bootstrap_owner_pair_transfer_ends_borrow_before_struct_construction(){
        use std::{cell::{Cell,RefCell},rc::Rc};
        struct Tracked(Rc<Cell<usize>>);
        impl Drop for Tracked{fn drop(&mut self){self.0.set(self.0.get()+1);}}
        struct Guard{present:Option<Tracked>,lease:Option<Tracked>,armed:bool}
        struct Built{_present:Tracked,_lease:Tracked,_post_transfer_borrow:bool}
        let drops=Rc::new(Cell::new(0));
        let owner=RefCell::new(Some(Guard{present:Some(Tracked(drops.clone())),lease:Some(Tracked(drops.clone())),armed:true}));
        let (present,lease)=take_bootstrap_owner_pair(&owner,|g|(&mut g.present,&mut g.lease)).unwrap();
        // This second borrow is deliberately in the caller's next struct literal;
        // the production helper must have ended its RefMut before returning.
        let built=Built{_present:present,_lease:lease,_post_transfer_borrow:owner.borrow_mut().as_ref().unwrap().armed};
        owner.borrow_mut().as_mut().unwrap().armed=false;
        assert!(owner.borrow().as_ref().unwrap().present.is_none());
        assert!(owner.borrow().as_ref().unwrap().lease.is_none());
        drop(owner);assert_eq!(drops.get(),0);drop(built);assert_eq!(drops.get(),2);
    }
    #[test]
    fn bootstrap_owner_pair_partial_failure_preserves_present_and_real_lease_state(){
        use std::{cell::{Cell,RefCell},rc::Rc};
        struct Present(Rc<Cell<usize>>);
        impl Drop for Present{fn drop(&mut self){self.0.set(self.0.get()+1);}}
        struct Lease(Rc<RefCell<ProcessState>>);
        impl Drop for Lease{fn drop(&mut self){self.0.borrow_mut().release(true,false);}}
        struct Guard{present:Option<Present>,lease:Option<Lease>}
        for (has_present,has_lease) in [(false,true),(true,false),(false,false)]{
            let drops=Rc::new(Cell::new(0));let state=Rc::new(RefCell::new(ProcessState{legacy_sessions:0,rr_active:false}));
            if has_lease{assert!(state.borrow_mut().acquire(true));}
            let owner=RefCell::new(Some(Guard{present:has_present.then(||Present(drops.clone())),lease:has_lease.then(||Lease(state.clone()))}));
            assert!(take_bootstrap_owner_pair(&owner,|g|(&mut g.present,&mut g.lease)).is_err());
            let borrowed=owner.borrow();let guard=borrowed.as_ref().unwrap();
            assert_eq!(guard.present.is_some(),has_present);assert_eq!(guard.lease.is_some(),has_lease);assert_eq!(drops.get(),0);
            if has_lease{assert!(!state.borrow_mut().acquire(true));assert!(!state.borrow_mut().acquire(false));}
            drop(borrowed);drop(owner);assert_eq!(drops.get(),usize::from(has_present));assert!(!state.borrow().rr_active);
        }
    }
    #[test]
    fn bootstrap_owner_pair_busy_or_missing_guard_never_takes_a_slot(){
        use std::cell::RefCell;
        struct Guard{present:Option<u32>,lease:Option<u32>}
        let owner=RefCell::new(Some(Guard{present:Some(7),lease:Some(9)}));
        let held=owner.borrow();
        assert!(take_bootstrap_owner_pair(&owner,|g|(&mut g.present,&mut g.lease)).is_err());
        assert_eq!(held.as_ref().unwrap().present,Some(7));assert_eq!(held.as_ref().unwrap().lease,Some(9));
        drop(held);assert_eq!(take_bootstrap_owner_pair(&owner,|g|(&mut g.present,&mut g.lease)).unwrap(),(7,9));
        let absent:RefCell<Option<Guard>>=RefCell::new(None);
        assert!(take_bootstrap_owner_pair(&absent,|g|(&mut g.present,&mut g.lease)).is_err());
    }

    #[test]
    fn retained_bootstrap_lease_keeps_sdk_domain_exclusive(){
        let mut state=ProcessState{legacy_sessions:0,rr_active:false};
        assert!(state.acquire(true));state.release(true,true);
        assert!(!state.acquire(true));assert!(!state.acquire(false));
        assert!(state.rr_active);
        let mut released=ProcessState{legacy_sessions:0,rr_active:false};
        assert!(released.acquire(true));released.release(true,false);
        assert!(released.acquire(false));released.release(false,false);
        assert!(released.acquire(true));
    }

    fn params() -> RrFrameParams {
        RrFrameParams {
            camera: RrCamera {
                view_to_clip: identity(),
                clip_to_view: identity(),
                clip_to_prev_clip: identity(),
                prev_clip_to_clip: identity(),
                world_to_view: identity(),
                view_to_world: identity(),
                position: [0.; 3],
                up: [0., 1., 0.],
                right: [1., 0., 0.],
                forward: [0., 0., 1.],
                near: 0.1,
                far: 100.,
                fov_y_radians: 1.,
            },
            jitter_pixels: [0.; 2],
            pre_exposure: 1.,
            exposure_scale: 1.,
            frame_index: 0,
            reset: true,
            depth_domain: RrDepthDomain::DeviceZForward,
        }
    }
    #[test]
    fn official_21416_abi_layouts() {
        assert_eq!(size_of::<SlRrOptions>(), 224);
        assert_eq!(std::mem::align_of::<SlRrOptions>(), 8);
        assert_eq!(std::mem::offset_of!(SlRrOptions, mode), 32);
        assert_eq!(std::mem::offset_of!(SlRrOptions, output_width), 36);
        assert_eq!(std::mem::offset_of!(SlRrOptions, normal_roughness_mode), 60);
        assert_eq!(std::mem::offset_of!(SlRrOptions, presets), 196);
        assert_eq!(size_of::<SlConstants>(), 456);
        assert_eq!(size_of::<SlResource>(), 112);
        assert_eq!(size_of::<SlResourceTag>(), 64);
        assert_eq!(size_of::<SlPreferencesLayout>(), 144);
        assert_eq!(size_of::<SlViewportHandle>(), 40);
        assert_eq!(FEATURE, 1001);
    }
    #[test]
    fn strict_surface_input_has_separate_masks_and_optional_reflection_choice() {
        let color = [0.1, 0.2, 0.3];
        let normal = [0., 0., 1., 0.3];
        let depth = [0.5];
        let mv = [0., 0.];
        let mask = [0.];
        let mut input = RrFrameInput {
            color: &color,
            normals_roughness: &normal,
            diffuse_albedo: &color,
            specular_albedo: &color,
            depth: &depth,
            mv: &mv,
            specular_mv: Some(&mv),
            specular_hit_distance: None,
            bias_current_color: Some(&mask),
            transparency: Some(&mask),
            params: params(),
        };
        assert!(validate_frame(&input, (1, 1)).is_ok());
        input.specular_mv = None;
        assert!(validate_frame(&input, (1, 1)).is_err());
        input.specular_hit_distance = Some(&depth);
        assert!(validate_frame(&input, (1, 1)).is_ok());
        let invalid = [f32::NAN];
        input.bias_current_color = Some(&invalid);
        assert!(validate_frame(&input, (1, 1)).is_err());
    }
    #[test]
    fn reversed_and_linear_depth_are_explicit_tags() {
        let mut p = params();
        p.depth_domain = RrDepthDomain::DeviceZReversed;
        assert_eq!(constants(&p, (8, 4), false).depth_inverted, 1);
        p.depth_domain = RrDepthDomain::LinearView;
        assert_eq!(constants(&p, (8, 4), true).reset, 1);
        let c = RrConfig {
            input_size: (8, 8),
            output_size: (16, 16),
            mode: RrMode::Native,
            validation: false,
            preset: 6,
        };
        assert!(validate_config(&c).is_err());
    }
}
