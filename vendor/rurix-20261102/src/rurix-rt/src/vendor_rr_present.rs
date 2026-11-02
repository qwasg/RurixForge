//! RFC-0054/U63 staged amendment: real private WSI on the selected RR device.
//! No internal Streamline entry point and no other presentation device is used.
use super::*;
use crate::render_exec::RrPresentFailureStage as Stage;
use std::{marker::PhantomData, rc::Rc, thread::ThreadId};

const WAIT_NS: u64 = 2_000_000_000;
const PRESENT_LAYOUT: i32 = 1_000_001_002;
const UNWRITTEN: i32 = i32::MAX;
pub(crate) const INSTANCE_EXTENSIONS: [&std::ffi::CStr; 4] = [
    c"VK_KHR_surface", c"VK_KHR_win32_surface", c"VK_KHR_get_surface_capabilities2", c"VK_EXT_surface_maintenance1",
];
pub(crate) const DEVICE_EXTENSIONS: [&std::ffi::CStr; 2] = [c"VK_KHR_swapchain", c"VK_EXT_swapchain_maintenance1"];
#[repr(C)]
#[derive(Clone,Copy)]
struct Extension {name:[u8;256],version:u32}
fn check_extension_list(list:&[Extension],required:&[&std::ffi::CStr])->Result<(),VendorError>{
    for name in required {if !list.iter().any(|e|{let end=e.name.iter().position(|b|*b==0).unwrap_or(256);&e.name[..end]==name.to_bytes()}){return Err(bad(format!("RR WSI required extension absent: {}",name.to_string_lossy())));}}Ok(())
}
pub(crate) fn verify_instance_extensions(fns:SlVkFns)->Result<(),VendorError>{
    type Enum=unsafe extern "system" fn(*const c_char,*mut u32,*mut Extension)->i32;
    // SAFETY: U63, null-instance lookup of the official global enumeration API through SL.
    let p=unsafe{(fns.vk_gipa)(std::ptr::null_mut(),c"vkEnumerateInstanceExtensionProperties".as_ptr())};
    // SAFETY: U63, exact Vulkan PFN signature and null check.
    let enumerate=unsafe{cast_sym::<Enum>(p)}.ok_or_else(||bad("RR WSI instance-extension query missing"))?;
    let mut count=0;
    // SAFETY: U63, count-only enumeration, no layer-name pointer.
    let r=unsafe{enumerate(std::ptr::null(),&mut count,std::ptr::null_mut())};vk_check("RR WSI instance extension count",r)?;
    let mut list=vec![Extension{name:[0;256],version:0};count as usize];
    // SAFETY: U63, exact capacity and initialized output records; incomplete result is rejected.
    let r=unsafe{enumerate(std::ptr::null(),&mut count,list.as_mut_ptr())};vk_check("RR WSI instance extensions",r)?;list.truncate(count as usize);
    check_extension_list(&list,&INSTANCE_EXTENSIONS)
}
pub(crate) fn verify_device_extensions(fns:SlVkFns,instance:VkInstance,physical:VkPhysicalDevice)->Result<(),VendorError>{
    type Enum=unsafe extern "system" fn(VkPhysicalDevice,*const c_char,*mut u32,*mut Extension)->i32;
    // SAFETY: U63, official instance-level enumeration on the selected RR instance.
    let p=unsafe{(fns.vk_gipa)(instance,c"vkEnumerateDeviceExtensionProperties".as_ptr())};
    // SAFETY: U63, exact Vulkan PFN signature and null check.
    let enumerate=unsafe{cast_sym::<Enum>(p)}.ok_or_else(||bad("RR WSI device-extension query missing"))?;
    let mut count=0;
    // SAFETY: U63, valid selected physical device; count-only query.
    let r=unsafe{enumerate(physical,std::ptr::null(),&mut count,std::ptr::null_mut())};vk_check("RR WSI device extension count",r)?;
    let mut list=vec![Extension{name:[0;256],version:0};count as usize];
    // SAFETY: U63, vector capacity equals the queried count; incomplete results are rejected.
    let r=unsafe{enumerate(physical,std::ptr::null(),&mut count,list.as_mut_ptr())};vk_check("RR WSI device extensions",r)?;list.truncate(count as usize);
    check_extension_list(&list,&DEVICE_EXTENSIONS)
}

#[repr(C)]
pub(crate) struct MaintenanceFeatures { pub s_type:u32, pub p_next:*mut c_void, pub enabled:u32 }
impl MaintenanceFeatures { fn query()->Self{Self{s_type:1_000_275_000,p_next:std::ptr::null_mut(),enabled:0}} }
#[repr(C)]
struct SurfaceCreate { s_type:u32,p_next:*const c_void,flags:u32,hinstance:*mut c_void,hwnd:*mut c_void }
#[repr(C)]
#[derive(Clone,Copy,Default)]
struct Extent { width:u32,height:u32 }
#[repr(C)]
#[derive(Clone,Copy,Default)]
struct Caps { min_images:u32,max_images:u32,current:Extent,min:Extent,max:Extent,layers:u32,transforms:u32,transform:u32,alpha:u32,usage:u32 }
#[repr(C)]
#[derive(Clone,Copy,Default)]
struct Format { format:u32,color_space:u32 }
#[repr(C)]
struct SwapCreate { s_type:u32,p_next:*const c_void,flags:u32,surface:u64,min_images:u32,format:u32,color_space:u32,extent:Extent,layers:u32,usage:u32,sharing:u32,family_count:u32,families:*const u32,transform:u32,alpha:u32,mode:u32,clipped:u32,old:u64 }
#[repr(C)]
struct FenceCreate { s_type:u32,p_next:*const c_void,flags:u32 }
#[repr(C)]
struct PresentFence { s_type:u32,p_next:*const c_void,count:u32,fences:*const u64 }
#[repr(C)]
struct PresentInfo { s_type:u32,p_next:*const c_void,wait_count:u32,waits:*const u64,count:u32,swapchains:*const u64,indices:*const u32,results:*mut i32 }
type CreateSurface=unsafe extern "system" fn(VkInstance,*const SurfaceCreate,*const c_void,*mut u64)->i32;
type DestroySurface=unsafe extern "system" fn(VkInstance,u64,*const c_void);
type Support=unsafe extern "system" fn(VkPhysicalDevice,u32,u64,*mut u32)->i32;
type GetCaps=unsafe extern "system" fn(VkPhysicalDevice,u64,*mut Caps)->i32;
type GetFormats=unsafe extern "system" fn(VkPhysicalDevice,u64,*mut u32,*mut Format)->i32;
type GetModes=unsafe extern "system" fn(VkPhysicalDevice,u64,*mut u32,*mut u32)->i32;
type CreateSwap=unsafe extern "system" fn(VkDevice,*const SwapCreate,*const c_void,*mut u64)->i32;
type DestroySwap=unsafe extern "system" fn(VkDevice,u64,*const c_void);
type GetImages=unsafe extern "system" fn(VkDevice,u64,*mut u32,*mut u64)->i32;
type Acquire=unsafe extern "system" fn(VkDevice,u64,u64,u64,u64,*mut u32)->i32;
type QueuePresent=unsafe extern "system" fn(VkQueue,*const PresentInfo)->i32;
type CreateSync=unsafe extern "system" fn(VkDevice,*const FenceCreate,*const c_void,*mut u64)->i32;
type DestroySync=unsafe extern "system" fn(VkDevice,u64,*const c_void);
type ResetFences=unsafe extern "system" fn(VkDevice,u32,*const u64)->i32;
type WaitFences=unsafe extern "system" fn(VkDevice,u32,*const u64,u32,u64)->i32;
type Clear=unsafe extern "system" fn(VkCommandBuffer,u64,i32,*const [f32;4],u32,*const VkImageSubresourceRange);
#[derive(Clone,Copy)]
struct InstanceFns {create:CreateSurface,destroy:DestroySurface,support:Support,caps:GetCaps,formats:GetFormats,modes:GetModes}
#[derive(Clone,Copy)]
struct WsiFns {create:CreateSwap,destroy:DestroySwap,images:GetImages,acquire:Acquire,present:QueuePresent,create_fence:CreateSync,destroy_fence:DestroySync,create_sem:CreateSync,destroy_sem:DestroySync,reset:ResetFences,wait:WaitFences,clear:Clear}

#[derive(Debug,Clone,Default)]
pub struct PresentReport {
    pub hidden:bool,pub extent:(u32,u32),pub image_count:u32,pub queue_family:u32,
    pub device_uuid:[u8;16],pub maintenance1:bool,pub attempts:u64,pub completed:u64,
    pub last_frame:Option<u32>,pub failed_stage:Option<&'static str>,pub driver_vk_result:Option<i32>,
    pub pending_acquire:bool,pub pending_submit:bool,pub pending_present:bool,
}

#[derive(Default)]
struct Progress { failed:bool,acquire:bool,submit:bool,present:bool,acquired:bool }
impl Progress {
    fn retired(&self)->bool{!self.failed&&!self.acquire&&!self.submit&&!self.present&&!self.acquired}
    fn accept_present(&mut self,top:i32,per:i32)->bool{
        // Before-hook may run even when the driver fails. Never retry this frame.
        if top!=0||per!=0 {self.failed=true;return false;}
        self.acquired=false;self.present=true;true
    }
}

fn retire_pair(progress:&mut Progress,wait:WaitFences,device:VkDevice,submit:u64,present:u64)->Result<(),(Stage,Option<i32>,u64)>{
    if progress.failed{return Err((Stage::FinalPresentProof,None,0));}
    for (stage,fence) in [(Stage::SubmitWait,submit),(Stage::PresentWait,present)] {
        // SAFETY: U63, caller owns both tracked pending fences; bounded wait on the matching actual FFI entry.
        let result=unsafe{wait(device,1,&fence,1,WAIT_NS)};
        if result!=0{progress.failed=true;return Err((stage,Some(result),fence));}
        match stage {Stage::SubmitWait=>progress.submit=false,Stage::PresentWait=>progress.present=false,_=>unreachable!()}
    }
    Ok(())
}

fn invoke_present(function:QueuePresent,queue:VkQueue,swapchain:u64,index:u32,done:u64,fence:u64)->(i32,i32){
    let fence_info=PresentFence{s_type:1_000_275_001,p_next:std::ptr::null(),count:1,fences:&fence};
    let mut per=UNWRITTEN;
    let info=PresentInfo{s_type:1_000_001_001,p_next:(&fence_info as *const PresentFence).cast(),wait_count:1,waits:&done,count:1,swapchains:&swapchain,indices:&index,results:&mut per};
    // SAFETY: U63, caller owns the acquired image, same-device queue, retired binary semaphore and unsignaled maintenance fence.
    let top=unsafe{function(queue,&info)};
    (top,per)
}

#[cfg(windows)]
mod win {
    use super::*;
    type Proc=unsafe extern "system" fn(*mut c_void,u32,usize,isize)->isize;
    #[repr(C)]
    pub(super) struct Class {pub style:u32,pub proc:Option<Proc>,pub class_extra:i32,pub window_extra:i32,pub instance:*mut c_void,pub icon:*mut c_void,pub cursor:*mut c_void,pub background:*mut c_void,pub menu:*const u16,pub name:*const u16}
    #[repr(C)]
    pub(super) struct Message {pub hwnd:*mut c_void,pub message:u32,pub wparam:usize,pub lparam:isize,pub time:u32,pub x:i32,pub y:i32,pub private:u32}
    type Peek=unsafe extern "system" fn(*mut Message,*mut c_void,u32,u32,u32)->i32;
    type Translate=unsafe extern "system" fn(*const Message)->i32;
    type Dispatch=unsafe extern "system" fn(*const Message)->isize;
    #[derive(Clone,Copy)]
    pub(super) struct MessageFns {pub peek:Peek,pub translate:Translate,pub dispatch:Dispatch}
    #[link(name="user32")]
    unsafe extern "system" {
        pub(super) fn RegisterClassW(c:*const Class)->u16;
        pub(super) fn UnregisterClassW(name:*const u16,instance:*mut c_void)->i32;
        pub(super) fn CreateWindowExW(ex:u32,class:*const u16,title:*const u16,style:u32,x:i32,y:i32,w:i32,h:i32,parent:*mut c_void,menu:*mut c_void,instance:*mut c_void,param:*mut c_void)->*mut c_void;
        pub(super) fn DestroyWindow(hwnd:*mut c_void)->i32;
        pub(super) fn IsWindow(hwnd:*mut c_void)->i32;
        pub(super) fn IsWindowVisible(hwnd:*mut c_void)->i32;
        fn DefWindowProcW(hwnd:*mut c_void,msg:u32,w:usize,l:isize)->isize;
    }
    #[link(name="kernel32")]
    unsafe extern "system" {pub(super) fn GetModuleHandleW(name:*const u16)->*mut c_void;}
    impl MessageFns {
        pub(super) fn resolve()->Result<Self,VendorError>{
            let name:Vec<u16>="user32.dll".encode_utf16().chain(Some(0)).collect();
            // SAFETY: U63, user32 is already loaded by the statically linked window APIs; this borrows its existing module.
            let module=unsafe{GetModuleHandleW(name.as_ptr())};
            if module.is_null(){return Err(bad("RR WSI user32 module unavailable"));}
            macro_rules! symbol {($name:literal,$ty:ty)=>{{
                // SAFETY: U63, borrowed loaded user32 module and exact NUL-terminated Windows export name.
                let address=unsafe{loader::sym(module,concat!($name,"\0").as_ptr().cast())};
                // SAFETY: U63, official WinSDK x64 signature using the complete MSG layout; null checked before storing.
                unsafe{cast_sym::<$ty>(address)}.ok_or_else(||VendorError::SymbolMissing($name.into()))?
            }}}
            Ok(Self{peek:symbol!("PeekMessageW",Peek),translate:symbol!("TranslateMessage",Translate),dispatch:symbol!("DispatchMessageW",Dispatch)})
        }
    }
    pub(super) unsafe extern "system" fn proc(hwnd:*mut c_void,msg:u32,w:usize,l:isize)->isize{
        if msg==0x10||msg==2{return 0;} // private window: no global quit/input messages
        // SAFETY: U63, Windows supplied this callback's HWND and message values.
        unsafe{DefWindowProcW(hwnd,msg,w,l)}
    }
}

/// Native ownership has explicit destruction only after slShutdown. Drop never
/// calls a native API, so quarantining/taking this owner cannot destroy pending WSI.
pub(crate) struct Present {
    thread:ThreadId,_not_send:PhantomData<Rc<()>>,fns:SlVkFns,instance:VkInstance,
    inst:InstanceFns,device:VkDevice,queue:VkQueue,dev:Option<(VkDevFns,WsiFns)>,
    module:*mut c_void,hwnd:*mut c_void,class:Vec<u16>,registered:bool,surface:u64,
    #[cfg(windows)] message_fns:Option<win::MessageFns>,
    swapchain:u64,images:Vec<u64>,pool:u64,cmd:VkCommandBuffer,acquire_fence:u64,submit_fence:u64,present_fence:u64,done:u64,
    progress:Progress,report:PresentReport,sdk_failure:Option<RrSdkShutdownFailure>,
}

impl Present {
    pub(crate) fn empty(fns:SlVkFns,instance:VkInstance)->Result<Self,VendorError>{
        macro_rules! sym {($name:literal,$ty:ty)=>{{
            // SAFETY: U63, live RR instance; official SL gipa resolves the exact named public Vulkan API.
            let p=unsafe{(fns.vk_gipa)(instance,concat!($name,"\0").as_ptr().cast())};
            // SAFETY: U63, matching Vulkan SDK function signature, checked for null before use.
            unsafe{cast_sym::<$ty>(p)}.ok_or_else(||VendorError::SymbolMissing($name.into()))?
        }}}
        let inst=InstanceFns{create:sym!("vkCreateWin32SurfaceKHR",CreateSurface),destroy:sym!("vkDestroySurfaceKHR",DestroySurface),support:sym!("vkGetPhysicalDeviceSurfaceSupportKHR",Support),caps:sym!("vkGetPhysicalDeviceSurfaceCapabilitiesKHR",GetCaps),formats:sym!("vkGetPhysicalDeviceSurfaceFormatsKHR",GetFormats),modes:sym!("vkGetPhysicalDeviceSurfacePresentModesKHR",GetModes)};
        Ok(Self{thread:std::thread::current().id(),_not_send:PhantomData,fns,instance,inst,device:std::ptr::null_mut(),queue:std::ptr::null_mut(),dev:None,module:std::ptr::null_mut(),hwnd:std::ptr::null_mut(),class:Vec::new(),registered:false,surface:0,#[cfg(windows)] message_fns:None,swapchain:0,images:Vec::new(),pool:0,cmd:std::ptr::null_mut(),acquire_fence:0,submit_fence:0,present_fence:0,done:0,progress:Progress::default(),report:PresentReport::default(),sdk_failure:None})
    }
    pub(crate) fn report(&self)->PresentReport{
        let mut r=self.report.clone();r.pending_acquire=self.progress.acquire;r.pending_submit=self.progress.submit;r.pending_present=self.progress.present;r
    }
    fn failure(&mut self,stage:Stage,result:Option<i32>,fence:u64,started:Instant)->VendorError{
        self.progress.failed=true;self.report.failed_stage=Some(stage.name());self.report.driver_vk_result=result;
        let message=crate::render_exec::quarantine_hq_vendor_rr_present(stage,result,started.elapsed().as_nanos().min(u64::MAX as u128) as u64,self.device as usize,fence,self.cmd as usize);
        eprintln!("HQ_RR_PRESENT_FAILURE stage={} vk_result={result:?} attempts={} completed={} {message}",stage.name(),self.report.attempts,self.report.completed);
        bad(message)
    }
    pub(crate) fn check_thread(&mut self)->Result<(),VendorError>{
        if self.thread!=std::thread::current().id(){return Err(self.failure(Stage::ThreadCheck,None,0,Instant::now()));}
        if self.progress.failed||crate::render_exec::hq_vendor_worker_quarantined(){return Err(bad("RR WSI owner is quarantined"));} Ok(())
    }
    pub(crate) fn check_readonly(&self)->Result<(),VendorError>{
        if self.thread!=std::thread::current().id(){
            let message=crate::render_exec::quarantine_hq_vendor_rr_present(Stage::ThreadCheck,None,0,self.device as usize,0,self.cmd as usize);
            return Err(bad(message));
        }
        if self.progress.failed||crate::render_exec::hq_vendor_worker_quarantined(){return Err(bad("RR WSI owner is quarantined"));}Ok(())
    }
    pub(crate) fn proof(&mut self)->Result<(),VendorError>{
        self.check_thread()?;
        if !self.progress.retired(){return Err(self.failure(Stage::FinalPresentProof,None,self.present_fence,Instant::now()));}Ok(())
    }
    pub(crate) fn initialize_surface(&mut self)->Result<(),VendorError>{
        self.check_thread()?;
        #[cfg(not(windows))] {return Err(bad("RR hidden WSI requires Windows"));}
        #[cfg(windows)] {
            self.message_fns=Some(win::MessageFns::resolve()?);
            // SAFETY: U63, query this process's executable module; no borrowed handle is closed.
            self.module=unsafe{win::GetModuleHandleW(std::ptr::null())};
            if self.module.is_null(){return Err(bad("RR WSI GetModuleHandleW failed"));}
            static SEQ:AtomicU64=AtomicU64::new(0);
            self.class=format!("RurixRrPrivateWsi_{}_{}",std::process::id(),SEQ.fetch_add(1,Ordering::Relaxed)).encode_utf16().chain(Some(0)).collect();
            let wc=win::Class{style:0,proc:Some(win::proc),class_extra:0,window_extra:0,instance:self.module,icon:std::ptr::null_mut(),cursor:std::ptr::null_mut(),background:std::ptr::null_mut(),menu:std::ptr::null(),name:self.class.as_ptr()};
            // SAFETY: U63, class/proc/name remain valid through private window lifetime; guard already owns self.
            let atom=unsafe{win::RegisterClassW(&wc)};
            if atom==0{return Err(bad("RR WSI RegisterClassW failed"));}self.registered=true;
            // SAFETY: U63, non-visible top-level HWND on its owner thread; no product WndProc or input state.
            self.hwnd=unsafe{win::CreateWindowExW(0x0800_0080,self.class.as_ptr(),self.class.as_ptr(),0x8000_0000,0,0,64,64,std::ptr::null_mut(),std::ptr::null_mut(),self.module,std::ptr::null_mut())};
            if self.hwnd.is_null(){return Err(bad("RR WSI CreateWindowExW failed"));}
            let info=SurfaceCreate{s_type:1_000_009_000,p_next:std::ptr::null(),flags:0,hinstance:self.module,hwnd:self.hwnd};
            let mut surface=0;
            // SAFETY: U63, private HWND and SL-created instance are alive; exact public proxy signature.
            let r=unsafe{(self.inst.create)(self.instance,&info,std::ptr::null(),&mut surface)};
            vk_check("RR WSI create surface",r)?;self.surface=surface;
            if surface==0{return Err(bad("RR WSI null surface"));} Ok(())
        }
    }
    pub(crate) fn supports(&self,physical:VkPhysicalDevice,family:u32)->Result<bool,VendorError>{
        let mut supported=0;
        // SAFETY: U63, family enumerated from the selected physical device and actual live private surface.
        let r=unsafe{(self.inst.support)(physical,family,self.surface,&mut supported)};
        vk_check("RR WSI surface support",r)?;Ok(supported==1)
    }
    pub(crate) fn query_feature(&mut self,physical:VkPhysicalDevice)->Result<MaintenanceFeatures,VendorError>{
        type Get=unsafe extern "system" fn(VkPhysicalDevice,*mut Features2);
        // SAFETY: U63, same SL instance, exact public core Vulkan feature query.
        let address=unsafe{(self.fns.vk_gipa)(self.instance,c"vkGetPhysicalDeviceFeatures2".as_ptr())};
        // SAFETY: U63, official feature2 signature and null check.
        let get=unsafe{cast_sym::<Get>(address)}.ok_or_else(||bad("missing WSI features2"))?;
        let mut maintenance=MaintenanceFeatures::query();
        let mut query=Features2{s_type:1_000_059_000,p_next:(&mut maintenance as *mut MaintenanceFeatures).cast(),features:[0;55]};
        // SAFETY: U63, writable header-matched feature chain remains alive through synchronous query.
        unsafe{get(physical,&mut query)};
        if maintenance.enabled!=1{return Err(bad("RR WSI maintenance1 unavailable"));}
        self.report.maintenance1=true;Ok(maintenance)
    }
    pub(crate) fn record_uuid(&mut self,uuid:[u8;16]){self.report.device_uuid=uuid;}
    pub(crate) fn initialize_device(&mut self,physical:VkPhysicalDevice,device:VkDevice,queue:VkQueue,family:u32,core:VkDevFns)->Result<(),VendorError>{
        self.check_thread()?;self.device=device;self.queue=queue;self.report.queue_family=family;
        macro_rules! sym {($name:literal,$ty:ty)=>{{
            // SAFETY: U63, only the existing RR device is passed to official SL gdpa.
            let p=unsafe{(self.fns.vk_gdpa)(device,concat!($name,"\0").as_ptr().cast())};
            // SAFETY: U63, exact Vulkan signature checked before storing dispatch.
            unsafe{cast_sym::<$ty>(p)}.ok_or_else(||VendorError::SymbolMissing($name.into()))?
        }}}
        let wsi=WsiFns{create:sym!("vkCreateSwapchainKHR",CreateSwap),destroy:sym!("vkDestroySwapchainKHR",DestroySwap),images:sym!("vkGetSwapchainImagesKHR",GetImages),acquire:sym!("vkAcquireNextImageKHR",Acquire),present:sym!("vkQueuePresentKHR",QueuePresent),create_fence:sym!("vkCreateFence",CreateSync),destroy_fence:sym!("vkDestroyFence",DestroySync),create_sem:sym!("vkCreateSemaphore",CreateSync),destroy_sem:sym!("vkDestroySemaphore",DestroySync),reset:sym!("vkResetFences",ResetFences),wait:sym!("vkWaitForFences",WaitFences),clear:sym!("vkCmdClearColorImage",Clear)};
        self.dev=Some((core,wsi));
        let mut caps=Caps::default();
        // SAFETY: U63, live selected physical device/private surface and writable exact capabilities struct.
        let r=unsafe{(self.inst.caps)(physical,self.surface,&mut caps)};vk_check("RR WSI caps",r)?;
        if caps.current.width==0||caps.current.height==0||caps.current.width==u32::MAX||caps.current.height==u32::MAX||caps.usage&2==0||caps.alpha==0{return Err(bad("RR WSI invalid extent/transfer-dst/composite-alpha capability"));}
        let mut n=0;
        // SAFETY: U63, null output requests format count.
        let r=unsafe{(self.inst.formats)(physical,self.surface,&mut n,std::ptr::null_mut())};vk_check("RR WSI format count",r)?;
        let mut formats=vec![Format::default();n as usize];
        // SAFETY: U63, vector capacity matches queried format count; incomplete enumeration is rejected.
        let r=unsafe{(self.inst.formats)(physical,self.surface,&mut n,formats.as_mut_ptr())};vk_check("RR WSI formats",r)?;formats.truncate(n as usize);
        let format=formats.into_iter().find_map(|mut f|{if f.format==0{f.format=44;}[37,43,44,50].contains(&f.format).then_some(f)}).ok_or_else(||bad("RR WSI needs supported RGBA8/BGRA8 surface format"))?;
        n=0;
        // SAFETY: U63, null output requests supported presentation-mode count.
        let r=unsafe{(self.inst.modes)(physical,self.surface,&mut n,std::ptr::null_mut())};vk_check("RR WSI mode count",r)?;
        let mut modes=vec![0u32;n as usize];
        // SAFETY: U63, vector capacity matches queried mode count.
        let r=unsafe{(self.inst.modes)(physical,self.surface,&mut n,modes.as_mut_ptr())};vk_check("RR WSI modes",r)?;modes.truncate(n as usize);
        if !modes.contains(&2){return Err(bad("RR WSI FIFO unavailable"));}
        let mut count=caps.min_images.checked_add(1).ok_or_else(||bad("RR WSI image count overflow"))?;
        if caps.max_images!=0{count=count.min(caps.max_images);}if count<caps.min_images||count==0{return Err(bad("RR WSI invalid image count"));}
        let info=SwapCreate{s_type:1_000_001_000,p_next:std::ptr::null(),flags:0,surface:self.surface,min_images:count,format:format.format,color_space:format.color_space,extent:caps.current,layers:1,usage:2,sharing:0,family_count:0,families:std::ptr::null(),transform:caps.transform,alpha:caps.alpha&caps.alpha.wrapping_neg(),mode:2,clipped:1,old:0};
        let mut sc=0;
        // SAFETY: U63, capability-derived swapchain config on the same present-capable RR device.
        let r=unsafe{(wsi.create)(device,&info,std::ptr::null(),&mut sc)};vk_check("RR WSI create swapchain",r)?;self.swapchain=sc;
        if sc==0{return Err(bad("RR WSI null swapchain"));}
        n=0;
        // SAFETY: U63, live swapchain; only borrow its image handles.
        let r=unsafe{(wsi.images)(device,sc,&mut n,std::ptr::null_mut())};vk_check("RR WSI image count",r)?;self.images.resize(n as usize,0);
        // SAFETY: U63, image output vector matches count and images remain swapchain-owned.
        let r=unsafe{(wsi.images)(device,sc,&mut n,self.images.as_mut_ptr())};vk_check("RR WSI images",r)?;self.images.truncate(n as usize);
        if self.images.is_empty(){return Err(bad("RR WSI no images"));}
        let pool_info=VkCommandPoolCreateInfo{s_type:39,p_next:std::ptr::null(),flags:2,queue_family_index:family};
        let mut pool=0;
        // SAFETY: U63, private pool uses the one graphics/compute/present family.
        let r=unsafe{(core.create_command_pool)(device,&pool_info,std::ptr::null(),&mut pool)};vk_check("RR WSI create pool",r)?;self.pool=pool;
        let alloc=VkCommandBufferAllocateInfo{s_type:40,p_next:std::ptr::null(),command_pool:pool,level:0,command_buffer_count:1};
        let mut cmd=std::ptr::null_mut();
        // SAFETY: U63, one command buffer owned by this private pool.
        let r=unsafe{(core.allocate_command_buffers)(device,&alloc,&mut cmd)};vk_check("RR WSI allocate command",r)?;self.cmd=cmd;
        if cmd.is_null(){return Err(bad("RR WSI null command buffer"));}
        for which in 0..4 {
            let info=FenceCreate{s_type:if which==3{9}else{8},p_next:std::ptr::null(),flags:0};let mut handle=0;
            let create=if which==3{wsi.create_sem}else{wsi.create_fence};
            // SAFETY: U63, exact fence/semaphore create layout; every successful handle is immediately owned below.
            let r=unsafe{create(device,&info,std::ptr::null(),&mut handle)};vk_check("RR WSI create sync",r)?;
            match which{0=>self.acquire_fence=handle,1=>self.submit_fence=handle,2=>self.present_fence=handle,_=>self.done=handle}
        }
        self.report.hidden=true;self.report.extent=(caps.current.width,caps.current.height);self.report.image_count=n;
        eprintln!("HQ_RR_PRESENT_CAPABILITY hidden=true extent={}x{} images={} family={} maintenance1=true uuid={:02x?} dispatch=streamline_public_proxy",caps.current.width,caps.current.height,n,family,self.report.device_uuid);Ok(())
    }
    fn pump(&mut self)->Result<(),VendorError>{
        self.check_thread()?;
        #[cfg(windows)] {
            let fns=self.message_fns.ok_or_else(||bad("RR WSI message dispatch missing"))?;
            for _ in 0..128 {
                let mut msg=win::Message{hwnd:std::ptr::null_mut(),message:0,wparam:0,lparam:0,time:0,x:0,y:0,private:0};
                // SAFETY: U63, retrieve only this private HWND's messages on its creator thread.
                let found=unsafe{(fns.peek)(&mut msg,self.hwnd,0,0,1)};if found==0{break;}
                // SAFETY: U63, message returned by PeekMessageW and no product state touched.
                unsafe{(fns.translate)(&msg)};
                // SAFETY: U63, private live window proc handles this thread's message.
                unsafe{(fns.dispatch)(&msg)};
            }
            // SAFETY: U63, retained private HWND queried without mutation.
            let exists=unsafe{win::IsWindow(self.hwnd)};
            // SAFETY: U63, visibility query; this module never makes the HWND visible.
            let visible=unsafe{win::IsWindowVisible(self.hwnd)};
            if exists==0||visible!=0{return Err(self.failure(Stage::WindowState,None,0,Instant::now()));}
        }
        Ok(())
    }
    fn check(&mut self,stage:Stage,result:i32,fence:u64,started:Instant)->Result<(),VendorError>{
        if result==0{Ok(())}else{Err(self.failure(stage,Some(result),fence,started))}
    }
    fn wait(&mut self,stage:Stage,fence:u64,wait:WaitFences)->Result<(),VendorError>{
        let started=Instant::now();
        // SAFETY: U63, this exact pending operation owns the fence; bounded wait and no premature reuse.
        let result=unsafe{wait(self.device,1,&fence,1,WAIT_NS)};
        self.check(stage,result,fence,started)
    }
    pub(crate) fn frame_end(&mut self,frame:u32)->Result<(),VendorError>{
        self.proof()?;self.pump()?;
        let (core,wsi)=self.dev.ok_or_else(||bad("RR WSI device not initialized"))?;
        let started=Instant::now();let fences=[self.acquire_fence,self.submit_fence,self.present_fence];
        // SAFETY: U63, prior acquire/submit/present are retired; all three fences have no pending users.
        let r=unsafe{(wsi.reset)(self.device,3,fences.as_ptr())};self.check(Stage::Record,r,0,started)?;
        let mut index=u32::MAX;
        // SAFETY: U63, bounded acquisition with unsignaled fence on the genuine non-retired swapchain.
        let r=unsafe{(wsi.acquire)(self.device,self.swapchain,WAIT_NS,0,self.acquire_fence,&mut index)};
        if r==0||r==1_000_001_003{self.progress.acquire=true;self.progress.acquired=true;}
        self.check(Stage::Acquire,r,self.acquire_fence,started)?;
        let image=match self.images.get(index as usize){Some(image)=>*image,None=>return Err(self.failure(Stage::Acquire,None,self.acquire_fence,started))};
        self.wait(Stage::AcquireWait,self.acquire_fence,wsi.wait)?;self.progress.acquire=false;
        // SAFETY: U63, command is never reused before its prior submission fence completed.
        let r=unsafe{(core.reset_command_buffer)(self.cmd,0)};self.check(Stage::Record,r,0,started)?;
        let begin=VkCommandBufferBeginInfo{s_type:42,p_next:std::ptr::null(),flags:1,p_inheritance_info:std::ptr::null()};
        // SAFETY: U63, idle private command buffer begins a new single-use recording.
        let r=unsafe{(core.begin_command_buffer)(self.cmd,&begin)};self.check(Stage::Record,r,0,started)?;
        let mut barrier=VkImageMemoryBarrier{s_type:45,p_next:std::ptr::null(),src_access_mask:0,dst_access_mask:0x1000,old_layout:0,new_layout:7,src_queue_family_index:u32::MAX,dst_queue_family_index:u32::MAX,image,subresource_range:VkImageSubresourceRange{aspect_mask:1,base_mip_level:0,level_count:1,base_array_layer:0,layer_count:1}};
        // SAFETY: U63, acquire fence completed; same queue family owns image and prior content is discarded.
        unsafe{(core.cmd_pipeline_barrier)(self.cmd,1,0x1000,0,0,std::ptr::null(),0,std::ptr::null(),1,&barrier)};
        let clear=[0.,0.,0.,1.];
        // SAFETY: U63, supported transfer-destination image and float-compatible RGBA format, one color subresource.
        unsafe{(wsi.clear)(self.cmd,image,7,&clear,1,&barrier.subresource_range)};
        barrier.src_access_mask=0x1000;barrier.dst_access_mask=0;barrier.old_layout=7;barrier.new_layout=PRESENT_LAYOUT;
        // SAFETY: U63, clear precedes transition; presentation obtains visibility after waiting on the binary semaphore.
        unsafe{(core.cmd_pipeline_barrier)(self.cmd,0x1000,0x2000,0,0,std::ptr::null(),0,std::ptr::null(),1,&barrier)};
        // SAFETY: U63, all recording inputs are consumed before command buffer completion.
        let r=unsafe{(core.end_command_buffer)(self.cmd)};self.check(Stage::Record,r,0,started)?;
        let submit=VkSubmitInfo{s_type:4,p_next:std::ptr::null(),wait_semaphore_count:0,p_wait_semaphores:std::ptr::null(),p_wait_dst_stage_mask:std::ptr::null(),command_buffer_count:1,p_command_buffers:&self.cmd,signal_semaphore_count:1,p_signal_semaphores:&self.done};
        self.progress.submit=true; // Fail-stop retains ownership even if submission result is uncertain.
        // SAFETY: U63, image ownership acquired on host; executable command; semaphore retired by previous present fence.
        let r=unsafe{(core.queue_submit)(self.queue,1,&submit,self.submit_fence)};self.check(Stage::Submit,r,self.submit_fence,started)?;
        self.report.attempts+=1;self.progress.present=true;
        let (top,per)=invoke_present(wsi.present,self.queue,self.swapchain,index,self.done,self.present_fence);
        if !self.progress.accept_present(top,per){
            let (stage,result)=if top!=0{(Stage::Present,Some(top))}else{(Stage::PresentResult,(per!=UNWRITTEN).then_some(per))};
            return Err(self.failure(stage,result,self.present_fence,started));
        }
        if let Err((stage,result,fence))=retire_pair(&mut self.progress,wsi.wait,self.device,self.submit_fence,self.present_fence){return Err(self.failure(stage,result,fence,started));}
        self.pump()?;self.report.completed+=1;self.report.last_frame=Some(frame);
        eprintln!("HQ_RR_PRESENT_COMPLETE frame={frame} attempts={} completed={} image={index} present_fence_complete=true",self.report.attempts,self.report.completed);Ok(())
    }
    /// Only called after confirmed SDK shutdown. No pending WSI is permitted.
    pub(crate) fn destroy_after_shutdown(&mut self)->Result<(),VendorError>{
        self.proof()?;
        if let Some((core,wsi))=self.dev {
            if self.swapchain!=0 {
                // SAFETY: U63, SDK shut down and every presentation fence completed; swapchain owns its images.
                unsafe{(wsi.destroy)(self.device,self.swapchain,std::ptr::null())};self.swapchain=0;
            }
            for fence in [&mut self.acquire_fence,&mut self.submit_fence,&mut self.present_fence] {
                if *fence!=0 {
                    // SAFETY: U63, explicit retirement proof covers all uses of this owned fence.
                    unsafe{(wsi.destroy_fence)(self.device,*fence,std::ptr::null())};*fence=0;
                }
            }
            if self.done!=0 {
                // SAFETY: U63, present-fence proof covers semaphore payload use by the presentation engine.
                unsafe{(wsi.destroy_sem)(self.device,self.done,std::ptr::null())};self.done=0;
            }
            if self.pool!=0 {
                // SAFETY: U63, private command is retired; pool owns and releases it.
                unsafe{(core.destroy_command_pool)(self.device,self.pool,std::ptr::null())};self.pool=0;
            }
        }
        if self.surface!=0 {
            // SAFETY: U63, all child swapchains destroyed, instance and private HWND still live.
            unsafe{(self.inst.destroy)(self.instance,self.surface,std::ptr::null())};self.surface=0;
        }
        #[cfg(windows)] {
            if !self.hwnd.is_null() {
                // SAFETY: U63, creator thread verified and surface no longer references this HWND.
                let r=unsafe{win::DestroyWindow(self.hwnd)};
                if r==0{return Err(self.failure(Stage::WindowState,None,0,Instant::now()));}self.hwnd=std::ptr::null_mut();
            }
            if self.registered {
                // SAFETY: U63, no HWND remains for this private class; class string/module remain live.
                let r=unsafe{win::UnregisterClassW(self.class.as_ptr(),self.module)};
                if r==0{return Err(self.failure(Stage::WindowState,None,0,Instant::now()));}self.registered=false;
            }
        }
        Ok(())
    }
    pub(crate) fn sdk_failure(&self)->Option<RrSdkShutdownFailure>{self.sdk_failure}
    pub(crate) fn shutdown_failed(&mut self,code:i32)->VendorError{
        let failure=RrSdkShutdownFailure{operation:"slShutdown",result:code};
        self.sdk_failure=Some(failure);
        eprintln!("HQ_RR_SDK_FAILURE operation={} sl_result={}",failure.operation,failure.result);
        let retained=self.failure(Stage::Shutdown,None,0,Instant::now());
        bad(format!("RR SDK {} returned Streamline result {}; {retained}",failure.operation,failure.result))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    #[derive(Default)]
    struct Calls {top:i32,per:Option<i32>,present_calls:usize,wait_results:Vec<i32>,wait_fences:Vec<u64>}
    thread_local!{static CALLS:RefCell<Calls>=RefCell::new(Calls::default());}
    unsafe extern "system" fn fake_present(_queue:VkQueue,raw:*const PresentInfo)->i32{
        // SAFETY: U63 CPU fixture, invoke_present supplies a live complete PresentInfo for this synchronous call.
        let info=unsafe{&*raw};
        assert_eq!(info.count,1);assert_eq!(info.wait_count,1);assert_eq!(info.s_type,1_000_001_001);
        // SAFETY: U63 CPU fixture, the helper chains a live header-matched PresentFence.
        let fence=unsafe{&*info.p_next.cast::<PresentFence>()};
        assert_eq!(fence.count,1);assert_eq!(fence.s_type,1_000_275_001);
        // SAFETY: U63 CPU fixture, helper supplies exactly one live borrowed swapchain handle.
        assert_eq!(unsafe{*info.swapchains},11);
        // SAFETY: U63 CPU fixture, helper supplies one live acquired-image index.
        assert_eq!(unsafe{*info.indices},2);
        // SAFETY: U63 CPU fixture, helper supplies one live binary semaphore handle.
        assert_eq!(unsafe{*info.waits},22);
        // SAFETY: U63 CPU fixture, helper supplies one live present-fence handle.
        assert_eq!(unsafe{*fence.fences},33);
        CALLS.with(|calls|{let mut calls=calls.borrow_mut();calls.present_calls+=1;
            if let Some(value)=calls.per{
                // SAFETY: U63 CPU fixture, writable pResults points to helper-owned stack storage.
                unsafe{*info.results=value};
            }calls.top})
    }
    unsafe extern "system" fn fake_wait(_device:VkDevice,count:u32,fences:*const u64,all:u32,limit:u64)->i32{
        assert_eq!(count,1);assert_eq!(all,1);assert_eq!(limit,WAIT_NS);
        // SAFETY: U63 CPU fixture, retire_pair supplies one live fence value.
        let fence=unsafe{*fences};
        CALLS.with(|calls|{let mut calls=calls.borrow_mut();let n=calls.wait_fences.len();calls.wait_fences.push(fence);calls.wait_results[n]})
    }
    #[test] fn header_layouts_x64(){
        assert_eq!(size_of::<MaintenanceFeatures>(),24);assert_eq!(std::mem::offset_of!(MaintenanceFeatures,enabled),16);
        assert_eq!(size_of::<SurfaceCreate>(),40);assert_eq!(std::mem::offset_of!(SurfaceCreate,hwnd),32);
        assert_eq!(size_of::<Caps>(),52);assert_eq!(size_of::<Format>(),8);
        assert_eq!(size_of::<SwapCreate>(),104);assert_eq!(std::mem::offset_of!(SwapCreate,old),96);
        assert_eq!(size_of::<FenceCreate>(),24);assert_eq!(size_of::<PresentFence>(),32);
        assert_eq!(size_of::<PresentInfo>(),64);assert_eq!(std::mem::offset_of!(PresentInfo,results),56);
        #[cfg(windows)]{assert_eq!(size_of::<win::Class>(),72);assert_eq!(size_of::<win::Message>(),48);}
    }
    #[test] fn present_result_cannot_hide_failed_driver_or_unwritten_result(){
        for (top,per) in [(0,UNWRITTEN),(0,-1),(-4,0),(0,1_000_001_003),(-1_000_001_004,0)]{
            let mut p=Progress{acquired:true,..Default::default()};assert!(!p.accept_present(top,per));assert!(!p.retired());assert!(p.failed);
        }
    }
    #[test] fn successful_present_is_not_retired_until_fences_finish(){
        let mut p=Progress{acquired:true,submit:true,..Default::default()};assert!(p.accept_present(0,0));assert!(!p.retired());
        p.submit=false;assert!(!p.retired());p.present=false;assert!(p.retired());
    }
    #[test] fn fake_ffi_receives_real_present_shape_and_unwritten_result_is_rejected(){
        for (top,per) in [(0,Some(0)),(0,None),(0,Some(-1)),(-4,Some(0)),(0,Some(1_000_001_003))]{
            CALLS.with(|c|*c.borrow_mut()=Calls{top,per,..Default::default()});
            let results=invoke_present(fake_present,std::ptr::null_mut(),11,2,22,33);
            assert_eq!(results,(top,per.unwrap_or(UNWRITTEN)));
            CALLS.with(|c|assert_eq!(c.borrow().present_calls,1));
            let mut progress=Progress{acquired:true,submit:true,..Default::default()};
            assert_eq!(progress.accept_present(results.0,results.1),top==0&&per==Some(0));
            assert!(!progress.retired());
        }
    }
    #[test] fn fake_ffi_fence_failure_is_terminal_and_never_waits_or_presents_again(){
        for results in [vec![2,0],vec![-4,0],vec![0,2],vec![0,-4],vec![0,-13]]{
            CALLS.with(|c|*c.borrow_mut()=Calls{wait_results:results.clone(),..Default::default()});
            let mut progress=Progress{submit:true,present:true,..Default::default()};
            let result=retire_pair(&mut progress,fake_wait,std::ptr::null_mut(),44,55);
            assert!(result.is_err());assert!(progress.failed);assert!(!progress.retired());
            let expected=if results[0]==0{vec![44,55]}else{vec![44]};
            CALLS.with(|c|assert_eq!(c.borrow().wait_fences,expected));
            assert!(retire_pair(&mut progress,fake_wait,std::ptr::null_mut(),44,55).is_err());
            CALLS.with(|c|{assert_eq!(c.borrow().wait_fences,expected);assert_eq!(c.borrow().present_calls,0);});
        }
    }
    #[test] fn fake_ffi_two_fences_are_both_required_for_retirement(){
        CALLS.with(|c|*c.borrow_mut()=Calls{wait_results:vec![0,0],..Default::default()});
        let mut progress=Progress{submit:true,present:true,..Default::default()};
        assert!(retire_pair(&mut progress,fake_wait,std::ptr::null_mut(),44,55).is_ok());
        assert!(progress.retired());CALLS.with(|c|assert_eq!(c.borrow().wait_fences,vec![44,55]));
    }
}
