# F1 wave.4 上游补丁 round 8:VK external_semaphore_win32 基础面。
# 类型/常量/结构体/函数指针/DeviceCaps 探测/Dev 装载;帧提交集成在 round 9。
$ErrorActionPreference = 'Stop'
$f = 'H:\rurix\src\rurix-rt\src\render_exec.rs'
$t = [System.IO.File]::ReadAllText($f)
$eol = if ($t.Contains("`r`n")) { "`r`n" } else { "`n" }
function Norm($s) { ($s -split "`r`n|`n") -join $script:eol }
function Patch($name, $old, $new) {
  $o = Norm $old; $n = Norm $new
  $hit = ([regex]::Matches($script:t, [regex]::Escape($o))).Count
  if ($hit -ne 1) { throw "$name 命中 $hit ≠ 1" }
  $script:t = $script:t.Replace($o, $n)
  Write-Host "[patch] $name"
}

Patch 'R8a VkSemaphore 类型' @'
type VkFence = u64;
'@ @'
type VkFence = u64;
/// `VkSemaphore` 句柄(F1 wave.4 external semaphore 导出)。
type VkSemaphore = u64;
'@

Patch 'R8b semaphore 常量' @'
const ST_FENCE_CREATE_INFO: u32 = 8;
'@ @'
const ST_FENCE_CREATE_INFO: u32 = 8;
/// `VK_STRUCTURE_TYPE_SEMAPHORE_CREATE_INFO`(SDK `vulkan_core.h` 核对)。
const ST_SEMAPHORE_CREATE_INFO: u32 = 9;
/// `VK_STRUCTURE_TYPE_EXPORT_SEMAPHORE_CREATE_INFO`(同核对)。
const ST_EXPORT_SEMAPHORE_CREATE_INFO: u32 = 1_000_077_000;
/// `VK_STRUCTURE_TYPE_SEMAPHORE_GET_WIN32_HANDLE_INFO_KHR`(同核对)。
const ST_SEMAPHORE_GET_WIN32_HANDLE_INFO_KHR: u32 = 1_000_078_003;
/// `VK_EXTERNAL_SEMAPHORE_HANDLE_TYPE_OPAQUE_WIN32_BIT`(= 0x2):导出 NT handle。
const EXTERNAL_SEMAPHORE_HANDLE_TYPE_OPAQUE_WIN32: u32 = 0x2;
'@

Patch 'R8c semaphore 结构体' @'
/// `VkExternalMemoryImageCreateInfo`:external 图像创建(handleTypes 声明;F1 wave.3 方向 B)。
#[repr(C)]
struct ExternalMemoryImageCreateInfo {
    s_type: u32,
    p_next: *const c_void,
    handle_types: u32,
}
'@ @'
/// `VkExternalMemoryImageCreateInfo`:external 图像创建(handleTypes 声明;F1 wave.3 方向 B)。
#[repr(C)]
struct ExternalMemoryImageCreateInfo {
    s_type: u32,
    p_next: *const c_void,
    handle_types: u32,
}

/// `VkSemaphoreCreateInfo`(F1 wave.4;`p_next` 挂 ExportSemaphoreCreateInfo 时导出)。
#[repr(C)]
struct SemaphoreCreateInfo {
    s_type: u32,
    p_next: *const c_void,
    flags: VkFlags,
}

/// `VkExportSemaphoreCreateInfo`:声明导出句柄类型(win32 NT handle)。
#[repr(C)]
struct ExportSemaphoreCreateInfo {
    s_type: u32,
    p_next: *const c_void,
    handle_types: u32,
}

/// `VkSemaphoreGetWin32HandleInfoKHR`:取导出 semaphore 的 NT handle。
#[repr(C)]
struct SemaphoreGetWin32HandleInfoKHR {
    s_type: u32,
    p_next: *const c_void,
    semaphore: VkSemaphore,
    handle_type: u32,
}
'@

Patch 'R8d semaphore 函数指针' @'
type FnDestroyFence = unsafe extern "system" fn(VkDevice, VkFence, *const c_void);
'@ @'
type FnDestroyFence = unsafe extern "system" fn(VkDevice, VkFence, *const c_void);
type FnCreateSemaphore = unsafe extern "system" fn(
    VkDevice,
    *const SemaphoreCreateInfo,
    *const c_void,
    *mut VkSemaphore,
) -> VkResult;
type FnDestroySemaphore = unsafe extern "system" fn(VkDevice, VkSemaphore, *const c_void);
/// `vkGetSemaphoreWin32HandleKHR`(VK_KHR_external_semaphore_win32 设备扩展函数)。
type FnGetSemaphoreWin32HandleKHR = unsafe extern "system" fn(
    VkDevice,
    *const SemaphoreGetWin32HandleInfoKHR,
    *mut *mut c_void,
) -> VkResult;
'@

Patch 'R8e Dev 结构体加字段' @'
    create_fence: FnCreateFence,
    destroy_fence: FnDestroyFence,
'@ @'
    create_fence: FnCreateFence,
    destroy_fence: FnDestroyFence,
    create_semaphore: FnCreateSemaphore,
    destroy_semaphore: FnDestroySemaphore,
    get_semaphore_win32_handle: Option<FnGetSemaphoreWin32HandleKHR>,
'@

Patch 'R8f Dev::load 装载' @'
            create_fence: dp!(c"vkCreateFence", FnCreateFence),
            destroy_fence: dp!(c"vkDestroyFence", FnDestroyFence),
'@ @'
            create_fence: dp!(c"vkCreateFence", FnCreateFence),
            destroy_fence: dp!(c"vkDestroyFence", FnDestroyFence),
            create_semaphore: dp!(c"vkCreateSemaphore", FnCreateSemaphore),
            destroy_semaphore: dp!(c"vkDestroySemaphore", FnDestroySemaphore),
            // 扩展函数:设备未启用 VK_KHR_external_semaphore_win32 时为 None(机会性装载,
            // 不用 dp! 硬拒——G-F1-13 要求无扩展设备如实回退 v1 CPU 块)。
            get_semaphore_win32_handle: cast_fn::<FnGetSemaphoreWin32HandleKHR>(
                gdpa(device, c"vkGetSemaphoreWin32HandleKHR".as_ptr()),
            ),
'@

Patch 'R8g DeviceCaps 探测字段' @'
    /// `VK_KHR_external_memory_win32` 扩展存在(F1 wave.3 零拷贝 import 前提;
    /// 仅扩展存在性,无 feature 结构)。
    pub external_memory_win32: bool,
'@ @'
    /// `VK_KHR_external_memory_win32` 扩展存在(F1 wave.3 零拷贝 import 前提;
    /// 仅扩展存在性,无 feature 结构)。
    pub external_memory_win32: bool,
    /// `VK_KHR_external_semaphore_win32` 扩展存在(F1 wave.4 GPU 侧帧同步前提;
    /// 仅扩展存在性,无 feature 结构)。
    pub external_semaphore_win32: bool,
'@

Patch 'R8h read_physical_caps 探测' @'
    let external_memory_win32_ext = has_ext(c"VK_KHR_external_memory_win32");
'@ @'
    let external_memory_win32_ext = has_ext(c"VK_KHR_external_memory_win32");
    let external_semaphore_win32_ext = has_ext(c"VK_KHR_external_semaphore_win32");
'@

Patch 'R8i DeviceCaps 构造赋值' @'
        external_memory_win32: external_memory_win32_ext,
        conservative_raster,
'@ @'
        external_memory_win32: external_memory_win32_ext,
        external_semaphore_win32: external_semaphore_win32_ext,
        conservative_raster,
'@

[System.IO.File]::WriteAllText($f, $t, (New-Object System.Text.UTF8Encoding $false))
Write-Host "ROUND8 OK"
