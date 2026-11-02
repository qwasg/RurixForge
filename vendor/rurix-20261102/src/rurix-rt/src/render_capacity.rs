// RFC-0052 HQ allocation preflight; U32/U30, opt-in and buffer/compute only.
// Assisted-by: Codex:gpt-6-astra.

/// One driver-sized allocation request; obtained without vkAllocateMemory.
#[derive(Debug, Clone)]
pub struct FrameCapacityAllocation {
    pub label: String,
    /// persistent, initial_upload, or readback. Upload buffers are freed before readback allocation.
    pub phase: &'static str,
    pub requested_bytes: u64,
    pub required_bytes: u64,
    pub alignment: u64,
    pub memory_type_bits: u32,
    pub memory_type_index: u32,
    pub memory_property_flags: u32,
    pub heap_index: u32,
}

/// Per-heap simultaneous allocation demand and a real budget snapshot.
#[derive(Debug, Clone)]
pub struct FrameCapacityHeap {
    pub heap_index: u32,
    pub heap_bytes: u64,
    pub budget_bytes: u64,
    pub usage_bytes: u64,
    pub available_bytes: u64,
    pub persistent_bytes: u64,
    pub initial_upload_bytes: u64,
    pub readback_bytes: u64,
    pub required_peak_bytes: u64,
    pub fits: bool,
}

/// Complete explicit VkDeviceMemory demand for an HQ sequential compute session.
/// This does not reserve memory; driver-private Vulkan object allocations and
/// CPU-owned assets are outside the VkDeviceMemory requirement API.
#[derive(Debug, Clone)]
pub struct FrameCapacityReport {
    pub device_json: String,
    pub allocations: Vec<FrameCapacityAllocation>,
    pub heaps: Vec<FrameCapacityHeap>,
    // Private queried property witness, not added to legacy JSON rows.
    memory_heap_flags: Vec<u32>,
    memory_type_properties: Vec<(u32,u32)>,
}

impl FrameCapacityReport {
    pub fn fits(&self) -> bool { self.heaps.iter().all(|h| h.fits) }

    pub fn require_fit(&self) -> Result<(), String> {
        match self.heaps.iter().find(|h| !h.fits) {
            Some(h) => Err(format!("HQ capacity exceeded before allocation: heap {} peak {} > available {} (heap {}, budget {}, usage {})",
                h.heap_index, h.required_peak_bytes, h.available_bytes, h.heap_bytes, h.budget_bytes, h.usage_bytes)),
            None => Ok(()),
        }
    }

    /// Independent post-creation coverage check, never a substitute for preflight.
    /// Upload staging has been freed before the session allocation ledger is returned.
    pub fn verify_resident_ledger(&self, ledger: &[AllocationLedgerEntry]) -> Result<(), String> {
        let mut expected = self.allocations.iter().filter(|a| a.phase != "initial_upload")
            .map(|a| (a.heap_index, a.required_bytes)).collect::<Vec<_>>();
        let mut actual = ledger.iter().map(|a| (a.heap_index, a.bytes)).collect::<Vec<_>>();
        expected.sort_unstable(); actual.sort_unstable();
        if expected != actual {
            return Err(format!("HQ capacity requirement coverage differs from resident ledger: predicted={expected:?}, actual={actual:?}"));
        }
        Ok(())
    }

    pub fn evidence_json(&self) -> String {
        let allocations = self.allocations.iter().map(|a| format!(
            "{{\"label\":\"{}\",\"phase\":\"{}\",\"requested_bytes\":{},\"required_bytes\":{},\"alignment\":{},\"memory_type_bits\":{},\"memory_type_index\":{},\"memory_property_flags\":{},\"heap_index\":{}}}",
            a.label, a.phase, a.requested_bytes, a.required_bytes, a.alignment, a.memory_type_bits, a.memory_type_index, a.memory_property_flags, a.heap_index)).collect::<Vec<_>>().join(",");
        let heaps = self.heaps.iter().map(|h| format!(
            "{{\"heap_index\":{},\"heap_bytes\":{},\"budget_bytes\":{},\"usage_bytes\":{},\"available_bytes\":{},\"persistent_bytes\":{},\"initial_upload_bytes\":{},\"readback_bytes\":{},\"required_peak_bytes\":{},\"fits\":{}}}",
            h.heap_index, h.heap_bytes, h.budget_bytes, h.usage_bytes, h.available_bytes,
            h.persistent_bytes, h.initial_upload_bytes, h.readback_bytes, h.required_peak_bytes, h.fits)).collect::<Vec<_>>().join(",");
        format!(concat!("{{\"schema\":\"rurix.hq.capacity_preflight.v2\",\"status\":\"{}\",\"device\":{},",
            "\"source\":\"unbound VkBuffer VkMemoryRequirements + VkAsManager build-size queries + actual memoryType heapIndex + VK_EXT_memory_budget before allocation\",",
            "\"submission_policy\":\"sequential\",\"device_memory_allocated_by_query\":false,",
            "\"compared\":\"per heap persistent + max(initial_upload, readback) <= min(heap_size, heapBudget) - heapUsage\",",
            "\"not_included\":[\"driver-private Vulkan object allocations not exposed by VkMemoryRequirements\",\"CPU-owned asset copies\"],",
            "\"allocations\":[{}],\"heaps\":[{}]}}"),
            if self.fits() { "pass" } else { "exceeded" }, self.device_json, allocations, heaps)
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum HqCapacityMode { Off, Query, Enforce }

enum PersistentPreparation {
    Session(NativePersistentFrame),
    Capacity(FrameCapacityReport),
}

/// Query HQ sequential-session requirements on the selected real device without
/// allocating device memory. Unsupported shapes and unavailable budgets fail closed.
pub fn query_hq_frame_capacity(
    resources: &[ResourceDesc<'_>], passes: &[Pass<'_>],
    barriers: &[&[(u32, TargetState)]], readbacks: &[Readback],
    accel_structs: &[AccelStructDesc<'_>],
) -> Result<FrameCapacityReport, String> {
    query_hq_frame_capacity_policy(resources,passes,barriers,readbacks,accel_structs,false,false,&[])
}

/// RFC-0053 private candidate policy: query the exact flags3 AS requirements,
/// including the same facing policy used by the actual sequential session.
pub fn query_hq_frame_capacity_unique_candidates(
    resources: &[ResourceDesc<'_>], passes: &[Pass<'_>],
    barriers: &[&[(u32, TargetState)]], readbacks: &[Readback],
    accel_structs: &[AccelStructDesc<'_>], cull_back_faces: bool,
) -> Result<FrameCapacityReport, String> {
    if accel_structs.is_empty() {return Err("private capacity query requires the actual AS table".into());}
    query_hq_frame_capacity_policy(resources,passes,barriers,readbacks,accel_structs,cull_back_faces,true,&[])
}

/// RFC-0054: exact external-buffer creation requirements plus ordinary HQ/AS
/// demand, queried before allocation on the same selected device.
pub fn query_hq_frame_capacity_exportable_buffers(
    resources:&[ResourceDesc<'_>],passes:&[Pass<'_>],barriers:&[&[(u32,TargetState)]],
    readbacks:&[Readback],accel_structs:&[AccelStructDesc<'_>],cull_back_faces:bool,exports:&[u32],
) -> Result<FrameCapacityReport,String> {
    validate_hq_exportable_buffers(resources,exports)?;
    query_hq_frame_capacity_policy(resources,passes,barriers,readbacks,accel_structs,cull_back_faces,false,exports)
}

fn validate_hq_exportable_buffers(resources:&[ResourceDesc<'_>],exports:&[u32])->Result<(),String> {
    for (position,&id) in exports.iter().enumerate() {
        match resources.get(id as usize) {
            Some(ResourceDesc::Buffer(b)) if b.device_local && b.data.is_none()=>{},
            _=>return Err(format!("HQ external buffer{id} must be a device-local buffer without initial data")),
        }
        if exports[..position].contains(&id){return Err(format!("HQ external buffer{id} is duplicated"));}
    }
    Ok(())
}

fn query_hq_frame_capacity_policy(
    resources: &[ResourceDesc<'_>], passes: &[Pass<'_>],
    barriers: &[&[(u32, TargetState)]], readbacks: &[Readback],
    accel_structs: &[AccelStructDesc<'_>], cull_back_faces: bool, unique_candidates: bool, exports:&[u32],
) -> Result<FrameCapacityReport, String> {
    validate_hq_capacity_shape(resources, passes)?;
    validate_frame_with_as(resources, passes, barriers, readbacks, accel_structs.len() as u32)?;
    let gipa = load_vulkan_loader().ok_or("Vulkan loader unavailable for HQ capacity query")?;
    // SAFETY: U32 capacity extension; validated descriptors; query mode creates
    // only device/unbound-buffer objects and destroys them before returning.
    let prepared = unsafe { prepare_persistent_frame(gipa, resources, passes, barriers, readbacks,
        2, accel_structs, exports, &[], cull_back_faces, unique_candidates, HqCapacityMode::Query) }?;
    match prepared {
        PersistentPreparation::Capacity(report) => Ok(report),
        PersistentPreparation::Session(_) => Err("capacity query unexpectedly created a session".into()),
    }
}

fn validate_hq_capacity_shape(resources: &[ResourceDesc<'_>], passes: &[Pass<'_>]) -> Result<(), String> {
    if resources.iter().any(|r| !matches!(r, ResourceDesc::Buffer(_)))
        || passes.iter().any(|p| !matches!(p, Pass::Compute(_)))
    {
        return Err("HQ capacity query supports sequential buffer/compute sessions only; images/raster are not omitted".into());
    }
    Ok(())
}

fn capacity_heap_totals(
    allocations: &[FrameCapacityAllocation], heap_sizes: &[u64], budgets: &[u64], usages: &[u64],
) -> Result<Vec<FrameCapacityHeap>, String> {
    if heap_sizes.is_empty() || heap_sizes.len() != budgets.len() || budgets.len() != usages.len() {
        return Err("HQ capacity: invalid heap/budget table".into());
    }
    let mut heaps: Vec<FrameCapacityHeap> = heap_sizes.iter().enumerate().map(|(i, &size)| FrameCapacityHeap {
        heap_index: i as u32, heap_bytes: size, budget_bytes: budgets[i], usage_bytes: usages[i],
        available_bytes: size.min(budgets[i]).saturating_sub(usages[i]),
        persistent_bytes: 0, initial_upload_bytes: 0, readback_bytes: 0, required_peak_bytes: 0, fits: true,
    }).collect();
    for a in allocations {
        let h = heaps.get_mut(a.heap_index as usize).ok_or("HQ capacity: allocation heap out of range")?;
        if a.required_bytes < a.requested_bytes || a.alignment == 0 || a.memory_type_index >= 32
            || a.memory_type_bits & (1u32 << a.memory_type_index) == 0 {
            return Err("HQ capacity: malformed driver memory requirements/type".into());
        }
        let field = match a.phase {
            "persistent" => &mut h.persistent_bytes,
            "initial_upload" => &mut h.initial_upload_bytes,
            "readback" => &mut h.readback_bytes,
            _ => return Err("HQ capacity: unknown allocation lifetime phase".into()),
        };
        *field = field.checked_add(a.required_bytes).ok_or("HQ capacity: allocation byte sum overflow")?;
    }
    for h in &mut heaps {
        h.required_peak_bytes = h.persistent_bytes.checked_add(h.initial_upload_bytes.max(h.readback_bytes))
            .ok_or("HQ capacity: phase peak overflow")?;
        if h.required_peak_bytes > 0 && (h.heap_bytes == 0 || h.budget_bytes == 0) {
            return Err(format!("HQ capacity: missing/zero budget for used heap {}", h.heap_index));
        }
        h.fits = h.required_peak_bytes <= h.available_bytes
            && (h.required_peak_bytes == 0 || h.usage_bytes <= h.heap_bytes.min(h.budget_bytes));
    }
    Ok(heaps)
}

/// # Safety
/// U32: dev/device/memprops describe the same live logical device. No memory is
/// allocated or bound; each unbound buffer is destroyed even on a validation error.
unsafe fn query_capacity_buffer(
    dev: &Dev, device: VkDevice, memprops: &PhysicalDeviceMemoryProperties,
    label: String, phase: &'static str, size: u64, usage: u32, class: BufferMemClass, external:bool,
) -> Result<FrameCapacityAllocation, String> {
    // Same linear external create chain as the existing U32 exporter; no memory
    // is allocated by this requirement query and no imported handle is consumed.
    let external_info=ExternalMemoryBufferCreateInfo{s_type:ST_EXTERNAL_MEMORY_BUFFER_CREATE_INFO,p_next:std::ptr::null(),handle_types:EXTERNAL_MEMORY_HANDLE_TYPE_OPAQUE_WIN32};
    let info = BufferCreateInfo { s_type: ST_BUFFER_CREATE_INFO, p_next: if external {(&external_info as *const ExternalMemoryBufferCreateInfo).cast()}else{std::ptr::null()},
        flags: 0, size, usage, sharing_mode: SHARING_MODE_EXCLUSIVE,
        queue_family_index_count: 0, p_queue_family_indices: std::ptr::null() };
    let mut buffer = VK_NULL_HANDLE;
    let result = (dev.create_buffer)(device, &info, std::ptr::null(), &mut buffer);
    if result != VK_SUCCESS { return Err(format!("HQ capacity: vkCreateBuffer({label}, {size}) failed: {result}")); }
    let mut req = std::mem::zeroed::<MemoryRequirements>();
    (dev.buf_mem_req)(device, buffer, &mut req);
    (dev.destroy_buffer)(device, buffer, std::ptr::null());
    let mt = pick_buffer_memory_type(memprops, req.memory_type_bits, class)
        .ok_or_else(|| format!("HQ capacity: no matching memory type for {label}"))?;
    Ok(FrameCapacityAllocation { label, phase, requested_bytes: size, required_bytes: req.size,
        alignment: req.alignment, memory_type_bits: req.memory_type_bits, memory_type_index: mt,
        memory_property_flags: memprops.memory_types[mt as usize].property_flags,
        heap_index: memprops.memory_types[mt as usize].heap_index })
}

/// # Safety
/// U32/U30: live selected device, enabled AS feature when needed, validated HQ
/// descriptors. Query mode never allocates/binds memory and records no commands.
#[allow(clippy::too_many_arguments)]
unsafe fn query_capacity_on_device(
    gdpa: FnGetDeviceProcAddr, device: VkDevice, pd: VkPhysicalDevice,
    get_mem2: FnGetPhysicalDeviceMemoryProperties2, memprops: &PhysicalDeviceMemoryProperties,
    caps: &DeviceCaps, resources: &[ResourceDesc<'_>], readbacks: &[Readback],
    accel_structs: &[AccelStructDesc<'_>],
    unique_candidates: bool, exports:&[u32],
) -> Result<FrameCapacityReport, String> {
    if memprops.memory_type_count>32 || memprops.memory_heap_count>16 || memprops.memory_heap_count==0
        || memprops.memory_types[..memprops.memory_type_count as usize].iter().any(|m|m.heap_index>=memprops.memory_heap_count) {
        return Err("HQ capacity: invalid actual memory type/heap property bounds".into());
    }
    let dev = Dev::load(gdpa, device)?;
    let mut allocations = Vec::new();
    for (i, resource) in resources.iter().enumerate() {
        let ResourceDesc::Buffer(b) = resource else { return Err("HQ capacity: non-buffer resource".into()); };
        allocations.push(query_capacity_buffer(&dev, device, memprops, format!("resource[{i}]"), "persistent",
            b.size, buffer_usage_flags(b.usage), if b.device_local { BufferMemClass::DeviceLocal } else { BufferMemClass::HostWc },exports.contains(&(i as u32)))?);
        if b.device_local && let Some(data) = b.data.filter(|d| !d.is_empty()) {
            allocations.push(query_capacity_buffer(&dev, device, memprops, format!("resource[{i}].upload"), "initial_upload",
                data.len().max(4) as u64, 0x1, BufferMemClass::HostWc,false)?);
        }
    }
    for (i, rb) in readbacks.iter().enumerate() {
        let Readback::Buffer { res, size, .. } = *rb else { return Err("HQ capacity: non-buffer readback".into()); };
        if matches!(resources.get(res as usize), Some(ResourceDesc::Buffer(b)) if b.device_local) {
            allocations.push(query_capacity_buffer(&dev, device, memprops, format!("readback[{i}]"), "readback",
                size.max(4), 0x2, BufferMemClass::HostCachedPreferred,false)?);
        }
    }
    if !accel_structs.is_empty() {
        let fns = VkAsFns::load(gdpa, device)?;
        for (i, desc) in accel_structs.iter().enumerate() {
            for request in VkAsManager::capacity_buffer_requests(&fns, device, &desc.scene, desc.updatable_blas, unique_candidates)? {
                allocations.push(query_capacity_buffer(&dev, device, memprops, format!("as[{i}].{}", request.label), "persistent",
                    request.size.max(4), request.usage,
                    if request.host_visible { BufferMemClass::HostWc } else { BufferMemClass::DeviceLocal },false)?);
            }
        }
    }
    let mut budget = PhysicalDeviceMemoryBudgetPropertiesExt { s_type: ST_PHYSICAL_DEVICE_MEMORY_BUDGET_PROPERTIES_EXT,
        p_next: std::ptr::null_mut(), heap_budget: [0; 16], heap_usage: [0; 16] };
    let mut memory = PhysicalDeviceMemoryProperties2 { s_type: ST_PHYSICAL_DEVICE_MEMORY_PROPERTIES_2,
        p_next: (&mut budget as *mut PhysicalDeviceMemoryBudgetPropertiesExt).cast(), memory_properties: std::mem::zeroed() };
    get_mem2(pd, &mut memory);
    let n = memprops.memory_heap_count as usize;
    if n > 16 || memory.memory_properties.memory_heap_count as usize != n {
        return Err("HQ capacity: memory heap mapping changed during query".into());
    }
    let sizes = memprops.memory_heaps[..n].iter().map(|h| h.size).collect::<Vec<_>>();
    let heaps = capacity_heap_totals(&allocations, &sizes, &budget.heap_budget[..n], &budget.heap_usage[..n])?;
    Ok(FrameCapacityReport { device_json: caps.evidence_json(), allocations, heaps, memory_heap_flags:memprops.memory_heaps[..n].iter().map(|h|h.flags).collect(), memory_type_properties:memprops.memory_types[..memprops.memory_type_count as usize].iter().map(|m|(m.property_flags,m.heap_index)).collect() })
}

#[cfg(test)]
mod capacity_tests {
    use super::*;
    fn allocation(phase: &'static str, bytes: u64, heap: u32) -> FrameCapacityAllocation {
        FrameCapacityAllocation { label: "fixture".into(), phase, requested_bytes: bytes,
            required_bytes: bytes, alignment: 256, memory_type_bits: 1,
            memory_type_index: 0, memory_property_flags: MEM_DEVICE_LOCAL, heap_index: heap }
    }
    #[test]
    fn uma_counts_host_device_and_nonoverlapping_staging_peaks() {
        let rows = [allocation("persistent", 300, 0), allocation("persistent", 200, 0),
            allocation("initial_upload", 400, 0), allocation("readback", 250, 0)];
        let h = capacity_heap_totals(&rows, &[2000], &[1200], &[300]).unwrap();
        assert_eq!((h[0].persistent_bytes, h[0].required_peak_bytes, h[0].available_bytes), (500, 900, 900));
        assert!(h[0].fits);
        assert!(!capacity_heap_totals(&rows, &[2000], &[1200], &[301]).unwrap()[0].fits);
    }
    #[test]
    fn heaps_cannot_borrow_each_others_budget() {
        let rows = [allocation("persistent", 100, 0), allocation("persistent", 700, 1)];
        let h = capacity_heap_totals(&rows, &[5000, 1000], &[5000, 800], &[0, 200]).unwrap();
        assert!(h[0].fits); assert!(!h[1].fits);
    }
    #[test]
    fn driver_padding_and_usage_are_not_ignored() {
        let mut row = allocation("persistent", 10, 0); row.required_bytes = 256;
        assert!(!capacity_heap_totals(&[row], &[1000], &[250], &[0]).unwrap()[0].fits);
        assert!(!capacity_heap_totals(&[allocation("persistent", 1, 0)], &[1000], &[1000], &[1001]).unwrap()[0].fits);
    }
    #[test]
    fn missing_budget_overflow_and_bad_mappings_are_errors() {
        assert!(capacity_heap_totals(&[allocation("persistent", 1, 0)], &[100], &[0], &[0]).is_err());
        assert!(capacity_heap_totals(&[allocation("persistent", 1, 1)], &[100], &[100], &[0]).is_err());
        assert!(capacity_heap_totals(&[allocation("persistent", u64::MAX, 0), allocation("initial_upload", 1, 0)], &[u64::MAX], &[u64::MAX], &[0]).is_err());
        let mut row = allocation("persistent", 1, 0); row.memory_type_bits = 0;
        assert!(capacity_heap_totals(&[row], &[100], &[100], &[0]).is_err());
    }

    #[test]
    fn resident_coverage_rejects_unplanned_allocation_or_changed_driver_size() {
        let report = FrameCapacityReport { device_json: "{}".into(), heaps: vec![], memory_heap_flags:vec![], memory_type_properties:vec![],
            allocations: vec![allocation("persistent", 256, 0), allocation("initial_upload", 512, 0),
                allocation("readback", 128, 1)] };
        let ledger = vec![
            AllocationLedgerEntry { allocation_id: StableAllocationId(2), resource_id: None, bytes: 128, heap_index: 1 },
            AllocationLedgerEntry { allocation_id: StableAllocationId(1), resource_id: None, bytes: 256, heap_index: 0 },
        ];
        assert!(report.verify_resident_ledger(&ledger).is_ok());
        let mut extra = ledger.clone(); extra.push(AllocationLedgerEntry {
            allocation_id: StableAllocationId(3), resource_id: None, bytes: 1, heap_index: 0 });
        assert!(report.verify_resident_ledger(&extra).is_err());
        extra = ledger.clone(); extra[0].bytes += 1;
        assert!(report.verify_resident_ledger(&extra).is_err());
    }
}
