# F1 wave.3 上游补丁 round 6:新增 probe_image_mem_req 诊断探针(RD-F1-003 对账面)。
# 同设备同扩展路径查询图像 (size, alignment),external=true 挂 D3D12_RESOURCE 外部句柄
# 类型并启用 VK_KHR_external_memory_win32 —— 数字与 import 会话创建面直接对账。
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

Patch 'R6 probe_image_mem_req 探针' @'
    // SAFETY: 见模块头 U32 契约;句柄线性配对 create/destroy,末尾逆序销毁。
    unsafe { probe_caps_inner(gipa) }
}
'@ @'
    // SAFETY: 见模块头 U32 契约;句柄线性配对 create/destroy,末尾逆序销毁。
    unsafe { probe_caps_inner(gipa) }
}

/// F1 wave.3 诊断探针(RD-F1-003 对账面):同设备同扩展路径查询图像内存需求
/// `(size, alignment)`。`external=true` 挂 `VkExternalMemoryImageCreateInfo`
/// (D3D12_RESOURCE)且 device 启用 `VK_KHR_external_memory_win32` —— 与 import
/// 会话创建面同构,数字可直接对账。
pub fn probe_image_mem_req(
    width: u32,
    height: u32,
    format: TexFormat,
    usage: TextureUsage,
    external: bool,
) -> Result<(u64, u64), String> {
    let gipa = load_vulkan_loader().ok_or("vulkan loader 不可用")?;
    // SAFETY: 句柄线性配对 create/destroy(image→device→instance 逆序)。
    unsafe {
        let (instance, _validation) = create_instance(gipa, c"rurix-img-req-probe")?;
        let vk_destroy_instance: FnDestroyInstance =
            cast_fn(gipa(instance, c"vkDestroyInstance".as_ptr())).ok_or("缺 vkDestroyInstance")?;
        let out = (|| {
            let pd = pick_physical_device(gipa, instance)?;
            let caps = read_physical_caps(gipa, instance, pd)?;
            if external && !caps.external_memory_win32 {
                return Err("设备无 VK_KHR_external_memory_win32".to_owned());
            }
            let vk_get_qf: FnGetPhysicalDeviceQueueFamilyProperties = cast_fn(gipa(
                instance,
                c"vkGetPhysicalDeviceQueueFamilyProperties".as_ptr(),
            ))
            .ok_or("缺 vkGetPhysicalDeviceQueueFamilyProperties")?;
            let vk_create_device: FnCreateDevice =
                cast_fn(gipa(instance, c"vkCreateDevice".as_ptr())).ok_or("缺 vkCreateDevice")?;
            let vk_get_device_proc: FnGetDeviceProcAddr =
                cast_fn(gipa(instance, c"vkGetDeviceProcAddr".as_ptr()))
                    .ok_or("缺 vkGetDeviceProcAddr")?;
            let mut qf_count = 0u32;
            vk_get_qf(pd, &mut qf_count, std::ptr::null_mut());
            let mut qfs: Vec<QueueFamilyProperties> = (0..qf_count)
                .map(|_| QueueFamilyProperties {
                    queue_flags: 0,
                    queue_count: 0,
                    timestamp_valid_bits: 0,
                    min_image_transfer_granularity: VkExtent3D {
                        width: 0,
                        height: 0,
                        depth: 0,
                    },
                })
                .collect();
            vk_get_qf(pd, &mut qf_count, qfs.as_mut_ptr());
            let qfi = qfs
                .iter()
                .position(|q| q.queue_flags & QUEUE_GRAPHICS_BIT != 0)
                .ok_or("无 graphics queue family")? as u32;
            let mut exts: Vec<*const c_char> = vec![c"VK_KHR_synchronization2".as_ptr()];
            if external {
                exts.push(c"VK_KHR_external_memory_win32".as_ptr());
            }
            let mut sync2_feat = PhysicalDeviceSynchronization2Features {
                s_type: ST_PHYSICAL_DEVICE_SYNCHRONIZATION_2_FEATURES,
                p_next: std::ptr::null_mut(),
                synchronization2: 1,
            };
            let prio = [1.0f32];
            let dqci = DeviceQueueCreateInfo {
                s_type: ST_DEVICE_QUEUE_CREATE_INFO,
                p_next: std::ptr::null(),
                flags: 0,
                queue_family_index: qfi,
                queue_count: 1,
                p_queue_priorities: prio.as_ptr(),
            };
            let dci = DeviceCreateInfo {
                s_type: ST_DEVICE_CREATE_INFO,
                p_next: (&mut sync2_feat as *mut PhysicalDeviceSynchronization2Features)
                    .cast::<c_void>(),
                flags: 0,
                queue_create_info_count: 1,
                p_queue_create_infos: &dqci,
                enabled_layer_count: 0,
                pp_enabled_layer_names: std::ptr::null(),
                enabled_extension_count: exts.len() as u32,
                pp_enabled_extension_names: exts.as_ptr(),
                p_enabled_features: std::ptr::null(),
            };
            let mut device: VkDevice = std::ptr::null_mut();
            if vk_create_device(pd, &dci, std::ptr::null(), &mut device) != VK_SUCCESS {
                return Err("vkCreateDevice 失败".to_owned());
            }
            let out2 = (|| {
                let dev = Dev::load(vk_get_device_proc, device)?;
                let emi = ExternalMemoryImageCreateInfo {
                    s_type: ST_EXTERNAL_MEMORY_IMAGE_CREATE_INFO,
                    p_next: std::ptr::null(),
                    handle_types: EXTERNAL_MEMORY_HANDLE_TYPE_D3D12_RESOURCE,
                };
                let ici = ImageCreateInfo {
                    s_type: ST_IMAGE_CREATE_INFO,
                    p_next: if external {
                        (&emi as *const ExternalMemoryImageCreateInfo).cast()
                    } else {
                        std::ptr::null()
                    },
                    flags: 0,
                    image_type: IMAGE_TYPE_2D,
                    format: format.vk_format(),
                    extent: VkExtent3D {
                        width,
                        height,
                        depth: 1,
                    },
                    mip_levels: 1,
                    array_layers: 1,
                    samples: SAMPLE_COUNT_1,
                    tiling: IMAGE_TILING_OPTIMAL,
                    usage: texture_usage_flags(usage),
                    sharing_mode: SHARING_MODE_EXCLUSIVE,
                    queue_family_index_count: 0,
                    p_queue_family_indices: std::ptr::null(),
                    initial_layout: LAYOUT_UNDEFINED,
                };
                let mut image: VkImage = VK_NULL_HANDLE;
                if (dev.create_image)(device, &ici, std::ptr::null(), &mut image) != VK_SUCCESS {
                    return Err("probe: vkCreateImage 失败".to_owned());
                }
                let mut req = std::mem::zeroed::<MemoryRequirements>();
                (dev.img_mem_req)(device, image, &mut req);
                (dev.destroy_image)(device, image, std::ptr::null());
                Ok((req.size, req.alignment))
            })();
            let dev_destroy: Option<FnDestroyDevice> =
                cast_fn(vk_get_device_proc(device, c"vkDestroyDevice".as_ptr()));
            if let Some(dd) = dev_destroy {
                dd(device, std::ptr::null());
            }
            out2
        })();
        vk_destroy_instance(instance, std::ptr::null());
        out
    }
}
'@

[System.IO.File]::WriteAllText($f, $t, (New-Object System.Text.UTF8Encoding $false))
Write-Host "ROUND6 OK"
