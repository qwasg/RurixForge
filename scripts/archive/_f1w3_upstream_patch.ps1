# F1 wave.3 上游补丁(H:\rurix rurix-rt render_exec.rs + 下游 TextureDesc 字面量站)。
# 方向 B:D3D12 建共享纹理 → VK import 直渲(纯 pNext 注入,零新 FFI 函数)。
# 行尾符随文件原状归一化;每处替换断言唯一命中;任一失败即整体退出非零。
$ErrorActionPreference = 'Stop'
$f = 'H:\rurix\src\rurix-rt\src\render_exec.rs'
$bytes = [System.IO.File]::ReadAllBytes($f)
$hasBom = $bytes.Length -ge 3 -and $bytes[0] -eq 0xEF -and $bytes[1] -eq 0xBB -and $bytes[2] -eq 0xBF
$t = [System.IO.File]::ReadAllText($f)
$eol = if ($t.Contains("`r`n")) { "`r`n" } else { "`n" }
Write-Host "[patch] EOL=$([System.Text.Encoding]::ASCII.GetBytes($eol) -join ',') BOM=$hasBom"

function Norm($s) { ($s -split "`r`n|`n") -join $script:eol }
function Patch($name, $old, $new, [int]$expect = 1) {
  $o = Norm $old; $n = Norm $new
  $hit = ([regex]::Matches($script:t, [regex]::Escape($o))).Count
  if ($hit -ne $expect) { throw "$name 命中 $hit ≠ $expect" }
  $script:t = $script:t.Replace($o, $n)
  Write-Host "[patch] $name"
}

# P1: FFI 结构体与常量(MemoryAllocateInfo 之后)
Patch 'P1 FFI 结构体' @'
#[repr(C)]
struct MemoryAllocateInfo {
    s_type: u32,
    p_next: *const c_void,
    allocation_size: VkDeviceSize,
    memory_type_index: u32,
}
'@ @'
#[repr(C)]
struct MemoryAllocateInfo {
    s_type: u32,
    p_next: *const c_void,
    allocation_size: VkDeviceSize,
    memory_type_index: u32,
}

/// `VK_STRUCTURE_TYPE_EXTERNAL_MEMORY_IMAGE_CREATE_INFO`(SDK 1.3.296 `vulkan_core.h` 核对)。
const ST_EXTERNAL_MEMORY_IMAGE_CREATE_INFO: u32 = 1_000_056_000;
/// `VK_STRUCTURE_TYPE_IMPORT_MEMORY_WIN32_HANDLE_INFO_KHR`(同核对)。
const ST_IMPORT_MEMORY_WIN32_HANDLE_INFO_KHR: u32 = 1_000_057_000;
/// `VK_EXTERNAL_MEMORY_HANDLE_TYPE_OPAQUE_WIN32_BIT`(NT handle;D3D12 `CreateSharedHandle` 产物)。
const EXTERNAL_MEMORY_HANDLE_TYPE_OPAQUE_WIN32: u32 = 0x2;

/// `VkExternalMemoryImageCreateInfo`:external 图像创建(handleTypes 声明;F1 wave.3 方向 B)。
#[repr(C)]
struct ExternalMemoryImageCreateInfo {
    s_type: u32,
    p_next: *const c_void,
    handle_types: u32,
}

/// `VkImportMemoryWin32HandleInfoKHR`:D3D12 committed resource NT handle import 为
/// `VkDeviceMemory`(纯 pNext 注入,零新函数指针;`name=None` 按句柄引用)。
#[repr(C)]
struct ImportMemoryWin32HandleInfo {
    s_type: u32,
    p_next: *const c_void,
    handle_type: u32,
    handle: *mut c_void,
    name: *const u16,
}
'@

# P2: ExternalTextureImport + TextureDesc 字段
Patch 'P2 TextureDesc 字段' @'
/// texture2d 资源描述(device-local optimal tiling;初始数据经 staging 上传)。
#[derive(Debug, Clone)]
pub struct TextureDesc<'a> {
    /// 宽(≥1)。
    pub width: u32,
    /// 高(≥1)。
    pub height: u32,
    /// 像素格式。
    pub format: TexFormat,
    /// 用途位。
    pub usage: TextureUsage,
    /// 可选初始数据(逐纹素紧凑字节,长度须 = `width*height*bytes_per_texel`)。
    pub data: Option<&'a [u8]>,
}
'@ @'
/// 外部纹理 import 描述(F1 wave.3 零拷贝腿,方向 B:D3D12 建 → VK import 直渲)。
///
/// `nt_handle` = D3D12 `CreateSharedHandle` 产出的 NT handle(进程内直接用;跨进程须先
/// `DuplicateHandle`);`allocation_size` = D3D12 `GetResourceAllocationInfo().SizeInBytes`
/// (RFC-0001 §4.2.2 同配方)。import 端 `vkAllocateMemory` 仅挂
/// `VkImportMemoryWin32HandleInfoKHR`,内存对象由 VK 持有引用,`vkFreeMemory` 释放引用、
/// 不释放 D3D12 底层分配(所有权留 D3D12 COM 侧)。
#[derive(Debug, Clone, Copy)]
pub struct ExternalTextureImport {
    /// D3D12 shared NT handle 值。
    pub nt_handle: u64,
    /// D3D12 分配字节数(与 import 端 `allocationSize` 一致)。
    pub allocation_size: u64,
}

/// texture2d 资源描述(device-local optimal tiling;初始数据经 staging 上传)。
#[derive(Debug, Clone)]
pub struct TextureDesc<'a> {
    /// 宽(≥1)。
    pub width: u32,
    /// 高(≥1)。
    pub height: u32,
    /// 像素格式。
    pub format: TexFormat,
    /// 用途位。
    pub usage: TextureUsage,
    /// 可选初始数据(逐纹素紧凑字节,长度须 = `width*height*bytes_per_texel`)。
    pub data: Option<&'a [u8]>,
    /// 外部 import(F1 wave.3):`Some` 时内存不经 VK 分配,改为 import D3D12 共享
    /// committed resource 并直渲;`None` = 既有行为 0-byte 不变。
    pub external_import: Option<ExternalTextureImport>,
}
'@

# P3: DeviceCaps 字段
Patch 'P3 DeviceCaps 字段' @'
    /// `VK_EXT_memory_budget` 驱动 heap budget/usage 查询面。
    pub memory_budget: bool,
'@ @'
    /// `VK_EXT_memory_budget` 驱动 heap budget/usage 查询面。
    pub memory_budget: bool,
    /// `VK_KHR_external_memory_win32` 扩展存在(F1 wave.3 零拷贝 import 前提;
    /// 仅扩展存在性,无 feature 结构)。
    pub external_memory_win32: bool,
'@

# P4: 探测行
Patch 'P4 探测行' @'
    let conservative_raster_ext = has_ext(c"VK_EXT_conservative_rasterization");
'@ @'
    let conservative_raster_ext = has_ext(c"VK_EXT_conservative_rasterization");
    let external_memory_win32_ext = has_ext(c"VK_KHR_external_memory_win32");
'@

# P5a: 真构造
Patch 'P5a DeviceCaps 构造' @'
        memory_budget: memory_budget_ext,
'@ @'
        memory_budget: memory_budget_ext,
        external_memory_win32: external_memory_win32_ext,
'@

# P5b: 测试构造
Patch 'P5b test_caps 构造' @'
            memory_budget: false,
'@ @'
            memory_budget: false,
            external_memory_win32: false,
'@

# P6a: execute_on_device 纹理创建分支(唯一)
Patch 'P6a 纹理创建分支' @'
                ResourceDesc::Texture(t) => {
                    let ici = ImageCreateInfo {
                        s_type: ST_IMAGE_CREATE_INFO,
                        p_next: std::ptr::null(),
'@ @'
                ResourceDesc::Texture(t) => {
                    // F1 wave.3 方向 B:外部 import(D3D12 committed resource NT handle)→
                    // image 创建挂 ExternalMemoryImageCreateInfo(OPAQUE_WIN32),内存分配挂
                    // ImportMemoryWin32HandleInfo;allocationSize 以 D3D12
                    // GetResourceAllocationInfo 为准,req.size 仅作 ledger 登记。
                    let emi = ExternalMemoryImageCreateInfo {
                        s_type: ST_EXTERNAL_MEMORY_IMAGE_CREATE_INFO,
                        p_next: std::ptr::null(),
                        handle_types: EXTERNAL_MEMORY_HANDLE_TYPE_OPAQUE_WIN32,
                    };
                    let imi = t.external_import.map(|imp| ImportMemoryWin32HandleInfo {
                        s_type: ST_IMPORT_MEMORY_WIN32_HANDLE_INFO_KHR,
                        p_next: std::ptr::null(),
                        handle_type: EXTERNAL_MEMORY_HANDLE_TYPE_OPAQUE_WIN32,
                        handle: imp.nt_handle as *mut c_void,
                        name: std::ptr::null(),
                    });
                    let ici = ImageCreateInfo {
                        s_type: ST_IMAGE_CREATE_INFO,
                        p_next: if t.external_import.is_some() {
                            (&emi as *const ExternalMemoryImageCreateInfo).cast()
                        } else {
                            std::ptr::null()
                        },
'@

# P6b: 内存分配分支(唯一)
Patch 'P6b 内存分配分支' @'
                    let mai = MemoryAllocateInfo {
                        s_type: ST_MEMORY_ALLOCATE_INFO,
                        p_next: std::ptr::null(),
                        allocation_size: req.size,
                        memory_type_index: mt,
                    };
'@ @'
                    let mai = MemoryAllocateInfo {
                        s_type: ST_MEMORY_ALLOCATE_INFO,
                        p_next: imi.as_ref().map_or(std::ptr::null(), |p| {
                            (p as *const ImportMemoryWin32HandleInfo).cast()
                        }),
                        allocation_size: t
                            .external_import
                            .map_or(req.size, |imp| imp.allocation_size),
                        memory_type_index: mt,
                    };
'@

# P7: 瞬时路径 device 扩展启用
Patch 'P7 瞬时路径扩展' @'
        // device 创建:sync2(硬)+ atomic int64(机会性)扩展与 feature 链。
        let mut exts: Vec<*const c_char> = vec![c"VK_KHR_synchronization2".as_ptr()];
'@ @'
        // device 创建:sync2(硬)+ atomic int64(机会性)扩展与 feature 链。
        let mut exts: Vec<*const c_char> = vec![c"VK_KHR_synchronization2".as_ptr()];
        // F1 wave.3:外部纹理 import 须启用 VK_KHR_external_memory_win32;
        // 扩展不在位 → 确定性 Err(fail-closed,不隐式降级)。
        let needs_external_import = resources
            .iter()
            .any(|r| matches!(r, ResourceDesc::Texture(t) if t.external_import.is_some()));
        if needs_external_import {
            if !caps.external_memory_win32 {
                return Err(
                    "外部纹理 import 需 VK_KHR_external_memory_win32,设备不在位(fail-closed)"
                        .into(),
                );
            }
            exts.push(c"VK_KHR_external_memory_win32".as_ptr());
        }
'@

# P8: 持久路径 device 扩展启用
Patch 'P8 持久路径扩展' @'
        let mut exts = vec![
            c"VK_KHR_synchronization2".as_ptr(),
            c"VK_EXT_memory_budget".as_ptr(),
        ];
'@ @'
        let mut exts = vec![
            c"VK_KHR_synchronization2".as_ptr(),
            c"VK_EXT_memory_budget".as_ptr(),
        ];
        // F1 wave.3:外部纹理 import 须启用 VK_KHR_external_memory_win32(同瞬时路径 fail-closed)。
        let needs_external_import = resources
            .iter()
            .any(|r| matches!(r, ResourceDesc::Texture(t) if t.external_import.is_some()));
        if needs_external_import {
            if !caps.external_memory_win32 {
                return Err(
                    "外部纹理 import 需 VK_KHR_external_memory_win32,设备不在位(fail-closed)"
                        .into(),
                );
            }
            exts.push(c"VK_KHR_external_memory_win32".as_ptr());
        }
'@

# 写回(保留原 BOM 状态)
[System.IO.File]::WriteAllText($f, $t, (New-Object System.Text.UTF8Encoding $hasBom))
Write-Host "[patch] render_exec.rs 写回完成"

# P9: 下游 TextureDesc 字面量站补 external_import: None(行状态机,保缩进)
$targets = @(
  'H:\rurix\apps\uc06-renderer\src\pipeline.rs',
  'H:\rurix\apps\uc06-renderer\src\device_g75_hw.rs',
  'H:\rurix\apps\uc08-physics\src\device.rs',
  'H:\rurix\src\rurix-render\src\bin\g9_m95_visbuffer_swhw.rs',
  'H:\rurix\src\rurix-render\src\bin\g10_m134_frame_capture.rs',
  'H:\rurix\src\rurix-rt\src\render_exec.rs'
)
foreach ($tf in $targets) {
  $lines = [System.IO.File]::ReadAllLines($tf)
  $out = New-Object System.Collections.Generic.List[string]
  $inDesc = $false
  $count = 0
  foreach ($line in $lines) {
    $out.Add($line)
    if ($line -match 'TextureDesc\s*\{\s*$') { $inDesc = $true; continue }
    if ($inDesc -and $line -match '^(\s*)data:') {
      $out.Add($Matches[1] + 'external_import: None,')
      $inDesc = $false
      $count++
    }
  }
  if ($count -eq 0) { throw "$tf 未补到任何 TextureDesc 站点" }
  $b = [System.IO.File]::ReadAllBytes($tf)
  $bom = $b.Length -ge 3 -and $b[0] -eq 0xEF -and $b[1] -eq 0xBB -and $b[2] -eq 0xBF
  [System.IO.File]::WriteAllLines($tf, [string[]]$out, (New-Object System.Text.UTF8Encoding $bom))
  Write-Host "[patch] $tf 补 $count 站 (BOM=$bom)"
}
Write-Host "ALL PATCHES OK"
