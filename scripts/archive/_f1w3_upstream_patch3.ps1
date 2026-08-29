# F1 wave.3 上游补丁 round 3:handle type 修正为 D3D12_RESOURCE_BIT(0x40)。
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

Patch 'R3 handle type 常量' @'
/// `VK_EXTERNAL_MEMORY_HANDLE_TYPE_OPAQUE_WIN32_BIT`(NT handle;D3D12 `CreateSharedHandle` 产物)。
const EXTERNAL_MEMORY_HANDLE_TYPE_OPAQUE_WIN32: u32 = 0x2;
'@ @'
/// `VK_EXTERNAL_MEMORY_HANDLE_TYPE_D3D12_RESOURCE_BIT`(= 0x40;SDK `vulkan_core.h`
/// 核对)。import **D3D12 committed resource** 的 NT handle 须用此类型(OPAQUE_WIN32
/// 仅适配 VK 自身导出句柄;与 RFC-0001 CUDA 侧 `..._D3D12_RESOURCE` 旗标同源)。
const EXTERNAL_MEMORY_HANDLE_TYPE_D3D12_RESOURCE: u32 = 0x40;
'@

Patch 'R3 image handleTypes' @'
                    let emi = ExternalMemoryImageCreateInfo {
                        s_type: ST_EXTERNAL_MEMORY_IMAGE_CREATE_INFO,
                        p_next: std::ptr::null(),
                        handle_types: EXTERNAL_MEMORY_HANDLE_TYPE_OPAQUE_WIN32,
                    };
'@ @'
                    let emi = ExternalMemoryImageCreateInfo {
                        s_type: ST_EXTERNAL_MEMORY_IMAGE_CREATE_INFO,
                        p_next: std::ptr::null(),
                        handle_types: EXTERNAL_MEMORY_HANDLE_TYPE_D3D12_RESOURCE,
                    };
'@

Patch 'R3 import handle_type' @'
                        handle_type: EXTERNAL_MEMORY_HANDLE_TYPE_OPAQUE_WIN32,
'@ @'
                        handle_type: EXTERNAL_MEMORY_HANDLE_TYPE_D3D12_RESOURCE,
'@

[System.IO.File]::WriteAllText($f, $t, (New-Object System.Text.UTF8Encoding $false))
Write-Host "ROUND3 OK"
