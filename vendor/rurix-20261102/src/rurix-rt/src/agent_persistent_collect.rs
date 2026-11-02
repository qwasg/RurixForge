//! Private native region collector, staged only. No public entry point.
//! Proposed repeat ownership is NOT approved by existing U32 automatically.
//! Full RFC and unsafe-audit admission remain required before production use.
//! Assisted-by: Codex:GPT-6.
use super::*;

pub(super) struct Collected {
    pub header: [u32;16],
    pub timestamps: Option<Vec<(usize,u64,u64)>>,
    pub query_epoch: u64,
    pub cpu_fence_wait_ns:u64,
}

/// The caller owns native exclusively, has just submitted this region using
/// fences[slot], and has not reset/reused queries. The recorder emitted the
/// shader-write to HOST_READ dependency. Only source-bound registered headers
/// are accepted by the higher pending transaction; this function does not
/// validate epoch/active/error fields or publish anything.
///
/// SAFETY admission: proposed extension of U32, NOT production-enabled.
pub(super) unsafe fn collect(
    native: &mut NativePersistentFrame,
    resources: &[ResourceDesc<'_>],
    scope: &hq_region_scope::Scope,
    total_passes: usize,
    query_base: u32,
    slot: usize,
    validation_before: u64,
) -> Result<Collected,String> {
    let queries=scope.validate(total_passes,resources.len(),query_base)?;
    let fence=*native.fences.get(slot).ok_or("region fence slot missing")?;
    let Some(ResourceDesc::Buffer(header_desc))=resources.get(scope.header) else {
        return Err("region header is not a buffer".into());
    };
    if header_desc.device_local || header_desc.size<64 {return Err("region header is not host-visible64".into());}
    let Some(RtRes::Buf(header))=native.frame.rt.get(scope.header) else {
        return Err("native header resource absent".into());
    };
    // SAFETY: proposed U32 extension: valid same-device submitted fence;
    // the existing finite timeout remains unchanged.
    let wait_started=std::time::Instant::now();
    let done=unsafe {(native.frame.dev.wait_fences)(native.device,1,&fence,1,PERSISTENT_WAIT_TIMEOUT_NS)};
    let cpu_fence_wait_ns=elapsed_ns(wait_started);
    if done!=VK_SUCCESS {return Err(hq_setup_failure::quarantine(hq_setup_failure::Phase::OpticalRegionWait,done,cpu_fence_wait_ns,fence,native.frame.cmd as usize));}
    if native.validation_errors.load(std::sync::atomic::Ordering::Relaxed)>validation_before {
        return Err("HQ region validation error; discard session".into());
    }
    let mut ticks=vec![[0u64;2];(queries.end-queries.start) as usize];
    // VK_QUERY_RESULT_WITH_AVAILABILITY_BIT=0x4, standard Vulkan core flag.
    // Read only the written original slots, without QUERY_RESULT_WAIT_BIT.
    // SAFETY: proposed U32 extension: fence completed this exact written range;
    // output contains two u64 per query and is live until synchronous return.
    let got=unsafe {(native.frame.dev.get_query_pool_results)(native.device,
        native.frame.cleanup.query_pool,queries.start,queries.end-queries.start,
        ticks.len()*16,ticks.as_mut_ptr().cast(),16,QUERY_RESULT_64_BIT|0x4)};
    // Optional performance data never substitutes for the completed fence,
    // validation check, actual header, or final image readback below.
    const REGION_VK_NOT_READY:i32=1;
    if got!=VK_SUCCESS && got!=REGION_VK_NOT_READY {
        return Err(queue_result_error("HQ region timestamp collection",got));
    }
    // Deliberately omitted queries remain a rejecting fault-injection arm.
    if scope.skip_last_timestamp {return Err("injected missing region timestamp".into());}
    let timestamps=if got==REGION_VK_NOT_READY || ticks.iter().any(|v|v[1]==0) {
        let missing=ticks.chunks_exact(2).enumerate()
            .filter(|(_,v)|v[0][1]==0||v[1][1]==0)
            .map(|(i,v)|format!("pass={} begin_query={} begin_availability={} end_query={} end_availability={}",
                scope.range.start+i,queries.start+(i as u32)*2,v[0][1],queries.start+(i as u32)*2+1,v[1][1]))
            .collect::<Vec<_>>().join("; ");
        eprintln!("HQ_REGION_TIMESTAMP_UNAVAILABLE epoch={} passes={}..{} result={} unavailable=[{}]",
            scope.query_epoch,scope.range.start,scope.range.end,got,missing);
        None
    }else{Some(hq_region_scope::decode_timestamps(scope,&ticks)?)};
    let mut bytes=[0u8;64];
    let mut ptr=std::ptr::null_mut();
    // SAFETY: proposed U32 extension: admitted host-visible coherent allocation,
    // completed fence plus recorder's host-domain dependency, declared64 bytes.
    let mapped=unsafe {(native.frame.dev.map_mem)(native.device,header.mem,0,64,0,&mut ptr)};
    if mapped!=VK_SUCCESS {return Err(queue_result_error("HQ region header map",mapped));}
    if ptr.is_null() {
        // SAFETY: balance successful same-device map even for unusable pointer.
        unsafe {(native.frame.dev.unmap_mem)(native.device,header.mem)};
        return Err("HQ region header map returned null".into());
    }
    // SAFETY: valid mapping of64 bytes and disjoint fixed-size host destination.
    unsafe {std::ptr::copy_nonoverlapping(ptr.cast::<u8>(),bytes.as_mut_ptr(),64)};
    // SAFETY: same allocation's one successful map, copied before unmapping.
    unsafe {(native.frame.dev.unmap_mem)(native.device,header.mem)};
    let words=std::array::from_fn(|i|u32::from_le_bytes(bytes[i*4..i*4+4].try_into().unwrap()));
    Ok(Collected{header:words,timestamps,query_epoch:scope.query_epoch,cpu_fence_wait_ns})
}
