# F1 wave.3 上游补丁 round 2:VkMemoryDedicatedAllocateInfo 链 + alloc 错误码可见。
$ErrorActionPreference = 'Stop'
$f = 'H:\rurix\src\rurix-rt\src\render_exec.rs'
$bytes = [System.IO.File]::ReadAllBytes($f)
$hasBom = $bytes.Length -ge 3 -and $bytes[0] -eq 0xEF -and $bytes[1] -eq 0xBB -and $bytes[2] -eq 0xBF
$t = [System.IO.File]::ReadAllText($f)
$eol = if ($t.Contains("`r`n")) { "`r`n" } else { "`n" }
function Norm($s) { ($s -split "`r`n|`n") -join $script:eol }
function Patch($name, $old, $new, [int]$expect = 1) {
  $o = Norm $old; $n = Norm $new
  $hit = ([regex]::Matches($script:t, [regex]::Escape($o))).Count
  if ($hit -ne $expect) { throw "$name 命中 $hit ≠ $expect" }
  $script:t = $script:t.Replace($o, $n)
  Write-Host "[patch] $name"
}

# Q1: dedicated allocation 结构体(ImportMemoryWin32HandleInfo 之后)
Patch 'Q1 dedicated 结构体' @'
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
'@ @'
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

/// `VK_STRUCTURE_TYPE_MEMORY_DEDICATED_ALLOCATE_INFO`(Vulkan 1.1 core;SDK 核对)。
const ST_MEMORY_DEDICATED_ALLOCATE_INFO: u32 = 1_000_127_000;

/// `VkMemoryDedicatedAllocateInfo`:import D3D12 **committed** resource 时 VK 侧强制
/// dedicated 声明(与 RFC-0001 CUDA `CUDA_EXTERNAL_MEMORY_DEDICATED` 同义)。
#[repr(C)]
struct MemoryDedicatedAllocateInfo {
    s_type: u32,
    p_next: *const c_void,
    image: VkImage,
    buffer: VkBuffer,
}
'@

# Q2: imi 延后到 mai 点(需 image 已创建),从 ici 前移除
Patch 'Q2 imi 前移移除' @'
                    let imi = t.external_import.map(|imp| ImportMemoryWin32HandleInfo {
                        s_type: ST_IMPORT_MEMORY_WIN32_HANDLE_INFO_KHR,
                        p_next: std::ptr::null(),
                        handle_type: EXTERNAL_MEMORY_HANDLE_TYPE_OPAQUE_WIN32,
                        handle: imp.nt_handle as *mut c_void,
                        name: std::ptr::null(),
                    });
                    let ici = ImageCreateInfo {
'@ @'
                    let ici = ImageCreateInfo {
'@

# Q3: mai 挂 dedicated → import 链 + 错误码可见
Patch 'Q3 mai 链+错误码' @'
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
                    let mut mem: VkDeviceMemory = VK_NULL_HANDLE;
                    if (dev.alloc_mem)(device, &mai, std::ptr::null(), &mut mem) != VK_SUCCESS {
                        (dev.destroy_image)(device, image, std::ptr::null());
                        return Err(format!("resources[{i}]: vkAllocateMemory 失败(image)"));
                    }
'@ @'
                    // import 档:dedicated(image)→ import win32 handle 链入 pNext;
                    // 非 import 档 p_next 恒 null(0-byte 行为不变)。
                    let ded = t.external_import.map(|_| MemoryDedicatedAllocateInfo {
                        s_type: ST_MEMORY_DEDICATED_ALLOCATE_INFO,
                        p_next: std::ptr::null(),
                        image,
                        buffer: VK_NULL_HANDLE,
                    });
                    let imi = t.external_import.map(|imp| ImportMemoryWin32HandleInfo {
                        s_type: ST_IMPORT_MEMORY_WIN32_HANDLE_INFO_KHR,
                        p_next: ded.as_ref().map_or(std::ptr::null(), |d| {
                            (d as *const MemoryDedicatedAllocateInfo).cast()
                        }),
                        handle_type: EXTERNAL_MEMORY_HANDLE_TYPE_OPAQUE_WIN32,
                        handle: imp.nt_handle as *mut c_void,
                        name: std::ptr::null(),
                    });
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
                    let mut mem: VkDeviceMemory = VK_NULL_HANDLE;
                    let alloc_r = (dev.alloc_mem)(device, &mai, std::ptr::null(), &mut mem);
                    if alloc_r != VK_SUCCESS {
                        (dev.destroy_image)(device, image, std::ptr::null());
                        return Err(format!(
                            "resources[{i}]: vkAllocateMemory 失败(image): {alloc_r}"
                        ));
                    }
'@

[System.IO.File]::WriteAllText($f, $t, (New-Object System.Text.UTF8Encoding $hasBom))
Write-Host "ROUND2 OK"
