# F1 wave.3 上游补丁 round 7:D3D12 共享堆 import 腿。
# 对账数字(2026-08-17 probe 实测,RTX 4070 Ti):960x540 RGBA8 vk req=2,457,600 >
# d3d12 committed alloc=2,228,224(同 pitch 4096,行补齐 600 vs 544)。committed resource
# 无法超尺寸分配 → 加「共享堆 + placed resource + D3D12_HEAP 句柄 import」腿。
# 同时回滚 round 5(usage 窄化实测对 req 零影响,保留只会与探针口径分叉)。
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

Patch 'R7a 回滚 R5 usage 窄化' @'
                        tiling: IMAGE_TILING_OPTIMAL,
                        // F1 wave.3:external import 图像收窄 usage —— 色目标只需
                        // COLOR_ATTACHMENT|TRANSFER_SRC(readback);恒附加的 TRANSFER_DST
                        // 等会使驱动选择大 padding 布局(960x540 实测 req 11x 于 d3d12 alloc)。
                        usage: if t.external_import.is_some() {
                            0x1 | 0x10 // TRANSFER_SRC | COLOR_ATTACHMENT
                        } else {
                            texture_usage_flags(t.usage)
                        },
                        sharing_mode: SHARING_MODE_EXCLUSIVE,
'@ @'
                        tiling: IMAGE_TILING_OPTIMAL,
                        usage: texture_usage_flags(t.usage),
                        sharing_mode: SHARING_MODE_EXCLUSIVE,
'@

Patch 'R7b ExternalTextureImport.heap 字段' @'
#[derive(Debug, Clone, Copy)]
pub struct ExternalTextureImport {
    /// D3D12 shared NT handle 值。
    pub nt_handle: u64,
    /// D3D12 分配字节数(与 import 端 `allocationSize` 一致)。
    pub allocation_size: u64,
}
'@ @'
#[derive(Debug, Clone, Copy)]
pub struct ExternalTextureImport {
    /// D3D12 shared NT handle 值。
    pub nt_handle: u64,
    /// D3D12 分配字节数(与 import 端 `allocationSize` 一致;heap 档 = 堆尺寸)。
    pub allocation_size: u64,
    /// `false` = committed resource 句柄(D3D12_RESOURCE);`true` = 共享堆句柄
    /// (D3D12_HEAP,vk 内存需求 > committed 分配时的加堆腿,placed resource 在偏移 0)。
    pub heap: bool,
}
'@

Patch 'R7c 堆句柄类型常量' @'
const EXTERNAL_MEMORY_HANDLE_TYPE_D3D12_RESOURCE: u32 = 0x40;
'@ @'
const EXTERNAL_MEMORY_HANDLE_TYPE_D3D12_RESOURCE: u32 = 0x40;
/// `VK_EXTERNAL_MEMORY_HANDLE_TYPE_D3D12_HEAP_BIT`(= 0x4;SDK `vulkan_core.h` 核对)。
/// import D3D12 **共享堆** 句柄用(placed resource 所在堆,绑偏移 0)。
const EXTERNAL_MEMORY_HANDLE_TYPE_D3D12_HEAP: u32 = 0x4;
'@

Patch 'R7d emi 句柄类型分流' @'
                    let emi = ExternalMemoryImageCreateInfo {
                        s_type: ST_EXTERNAL_MEMORY_IMAGE_CREATE_INFO,
                        p_next: std::ptr::null(),
                        handle_types: EXTERNAL_MEMORY_HANDLE_TYPE_D3D12_RESOURCE,
                    };
'@ @'
                    let import_htype = match t.external_import {
                        Some(imp) if imp.heap => EXTERNAL_MEMORY_HANDLE_TYPE_D3D12_HEAP,
                        _ => EXTERNAL_MEMORY_HANDLE_TYPE_D3D12_RESOURCE,
                    };
                    let emi = ExternalMemoryImageCreateInfo {
                        s_type: ST_EXTERNAL_MEMORY_IMAGE_CREATE_INFO,
                        p_next: std::ptr::null(),
                        handle_types: import_htype,
                    };
'@

Patch 'R7e dedicated 仅 committed 档 + imi 类型分流' @'
                    let ded = t.external_import.map(|_| MemoryDedicatedAllocateInfo {
'@ @'
                    // heap 档不挂 dedicated(整堆 import,非独占图像语义)。
                    let ded = t.external_import.filter(|imp| !imp.heap).map(|_| MemoryDedicatedAllocateInfo {
'@

Patch 'R7f imi handle_type 分流' @'
                        handle_type: EXTERNAL_MEMORY_HANDLE_TYPE_D3D12_RESOURCE,
'@ @'
                        handle_type: import_htype,
'@

[System.IO.File]::WriteAllText($f, $t, (New-Object System.Text.UTF8Encoding $false))
Write-Host "ROUND7 OK"
