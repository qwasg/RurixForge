# F1 wave.3 上游补丁 round 4:import 档内存守卫 + vkBindImageMemory 返回值检查。
# 背景:960x540 首帧 import 渲染 VK_ERROR_DEVICE_LOST(128x96 正常)——bind 返回值被丢弃,
# 未绑定图像参与渲染导致设备丢失。本补丁把静默失败转为诚实错误并暴露 req.size vs alloc 数字。
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

Patch 'R4 import 内存守卫 + bind 检查' @'
                    let mut mem: VkDeviceMemory = VK_NULL_HANDLE;
                    let alloc_r = (dev.alloc_mem)(device, &mai, std::ptr::null(), &mut mem);
                    if alloc_r != VK_SUCCESS {
                        (dev.destroy_image)(device, image, std::ptr::null());
                        return Err(format!(
                            "resources[{i}]: vkAllocateMemory 失败(image): {alloc_r}"
                        ));
                    }
                    (dev.bind_img)(device, image, mem, 0);
'@ @'
                    let mut mem: VkDeviceMemory = VK_NULL_HANDLE;
                    let alloc_r = (dev.alloc_mem)(device, &mai, std::ptr::null(), &mut mem);
                    if alloc_r != VK_SUCCESS {
                        (dev.destroy_image)(device, image, std::ptr::null());
                        return Err(format!(
                            "resources[{i}]: vkAllocateMemory 失败(image): {alloc_r}"
                        ));
                    }
                    // F1 wave.3 守卫:import 档校验 vk 内存需求 ≤ D3D12 分配尺寸,且
                    // bind 返回值必须检查——否则未绑定图像参与渲染,后续同步点
                    // VK_ERROR_DEVICE_LOST(2026-08-17 960x540 首帧设备丢失实测)。
                    if let Some(imp) = t.external_import {
                        if req.size > imp.allocation_size {
                            (dev.free_mem)(device, mem, std::ptr::null());
                            (dev.destroy_image)(device, image, std::ptr::null());
                            return Err(format!(
                                "resources[{i}]: import 内存需求越界: vk req.size={} > d3d12 alloc={}",
                                req.size, imp.allocation_size
                            ));
                        }
                    }
                    let bind_r = (dev.bind_img)(device, image, mem, 0);
                    if bind_r != VK_SUCCESS {
                        (dev.free_mem)(device, mem, std::ptr::null());
                        (dev.destroy_image)(device, image, std::ptr::null());
                        return Err(format!(
                            "resources[{i}]: vkBindImageMemory 失败: {bind_r}(req.size={}, alloc={:?})",
                            req.size,
                            t.external_import.map(|imp| imp.allocation_size)
                        ));
                    }
'@

[System.IO.File]::WriteAllText($f, $t, (New-Object System.Text.UTF8Encoding $false))
Write-Host "ROUND4 OK"
