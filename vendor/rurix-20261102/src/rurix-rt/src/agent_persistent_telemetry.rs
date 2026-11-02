//! Exact native budget/ledger observation shared with the source collector.
use super::*;
pub(super) struct Measure{pub gpu_timestamps_complete:bool,pub gpu_ns:Vec<f64>,pub invocations:Vec<u64>,pub record_ns:u64,pub submit_ns:u64,pub wait_ns:u64}
impl Measure{pub fn new(n:usize)->Self{Self{gpu_timestamps_complete:true,gpu_ns:vec![0.;n],invocations:vec![0;n],record_ns:0,submit_ns:0,wait_ns:0}}}
// SAFETY: native is exclusively owned; all region fences completed before this
// source-derived physical-device memory query and ledger snapshot.
pub(super) unsafe fn collect(native:&NativePersistentFrame,passes:&[Pass<'_>],m:&Measure,validation_before:u64)->Result<DeviceFrameTelemetry,String>{
    let mut budget = PhysicalDeviceMemoryBudgetPropertiesExt {
        s_type: ST_PHYSICAL_DEVICE_MEMORY_BUDGET_PROPERTIES_EXT,
        p_next: std::ptr::null_mut(),
        heap_budget: [0; 16],
        heap_usage: [0; 16],
    };
    let mut memory = PhysicalDeviceMemoryProperties2 {
        s_type: ST_PHYSICAL_DEVICE_MEMORY_PROPERTIES_2,
        p_next: (&mut budget as *mut PhysicalDeviceMemoryBudgetPropertiesExt).cast(),
        // SAFETY: Vulkan memory-properties output POD is initialized by get_mem2.
        memory_properties: unsafe { std::mem::zeroed() },
    };
    // SAFETY: source-owned physical device and live correctly chained output structs.
    unsafe { (native.get_mem2)(native.pd, &mut memory); }
    let allocations: Vec<AllocationLedgerEntry> = native
        .frame
        .cleanup
        .allocations
        .iter()
        .map(|allocation| allocation.entry.clone())
        .collect();
    // C4 注入臂(默认关 = None 实值直通):budget 上报钳到注入上限演习 OOM 处置面。
    let budget_cap = g31_fault_budget_cap();
    let heaps = (0..memory.memory_properties.memory_heap_count as usize)
        .map(|heap_index| HeapBudgetTelemetry {
            heap_index: heap_index as u32,
            budget_bytes: match budget_cap {
                Some(cap) => budget.heap_budget[heap_index].min(cap),
                None => budget.heap_budget[heap_index],
            },
            driver_usage_bytes: budget.heap_usage[heap_index],
            ledger_bytes: allocations
                .iter()
                .filter(|entry| entry.heap_index as usize == heap_index)
                .map(|entry| entry.bytes)
                .sum(),
        })
        .collect::<Vec<_>>();
    if heaps.is_empty() || heaps.iter().any(|heap| heap.budget_bytes == 0) {
        return Err(budget_failure_diagnostic(
            "empty_or_zero_budget",
            &memory.memory_properties.memory_heaps.iter().take(memory.memory_properties.memory_heap_count as usize).map(|heap|heap.size).collect::<Vec<_>>(),
            &budget.heap_budget,&budget.heap_usage,&heaps));
    }
    if let Some(heap) = heaps
        .iter()
        .find(|heap| heap.driver_usage_bytes > heap.budget_bytes)
    {
        // C4 budget 违约处置面(注入臂下专项确定性 OOM 消息;默认关 = 既有
        // 非法实值消息逐字不变——驱动实报 usage>budget 本属非法遥测)。
        if budget_cap.is_some() {
            return Err(format!(
                "显存 budget 违约(OOM-suspected,fail-closed):heap {} driver_usage {} > \
                 budget {}(注入 budget 上限;分配请求超 budget 确定性失败,不降级不挂死)",
                heap.heap_index, heap.driver_usage_bytes, heap.budget_bytes
            ));
        }
        return Err(budget_failure_diagnostic(
            "driver_usage_exceeds_budget",
            &memory.memory_properties.memory_heaps.iter().take(memory.memory_properties.memory_heap_count as usize).map(|heap|heap.size).collect::<Vec<_>>(),
            &budget.heap_budget,&budget.heap_usage,&heaps));
    }
    let validation_error_count = native
        .validation_errors
        .load(std::sync::atomic::Ordering::Relaxed);
    if validation_error_count > validation_before {
        return Err(format!(
            "VK_LAYER_KHRONOS_validation 新增 {} 条 ERROR(fail-closed)",
            validation_error_count - validation_before
        ));
    }
    let as_objects: u64 = native.as_state.as_ref().map_or(0, |state| {
        state.managers.iter().map(VkAsManager::object_count).sum()
    });
    let outstanding_object_count = native.frame.cleanup.object_count()
        + as_objects
        + native.fences.len() as u64
        + 3
        + u64::from(native.pipelined_pool != VK_NULL_HANDLE);
    let outstanding_allocation_count = allocations.len() as u64;

    Ok(DeviceFrameTelemetry{timestamp_period_ns:native.timestamp_period_ns,
        passes:if m.gpu_timestamps_complete {passes.iter().enumerate().map(|(i,p)|PassGpuTiming{pass_id:i as u64+1,name:pass_name(p).into(),gpu_ns:m.gpu_ns[i]}).collect()}else{Vec::new()},cpu_record_ns:m.record_ns,cpu_submit_ns:m.submit_ns,cpu_fence_wait_ns:m.wait_ns,
        heaps,allocations,validation_error_count,device_lost:false,tdr_suspected:false,outstanding_object_count,outstanding_allocation_count,leaked_object_count:0,leaked_allocation_count:0,blas_bridge_copy_gpu_ms:None,blas_bridge_build_gpu_ms:None})
}
