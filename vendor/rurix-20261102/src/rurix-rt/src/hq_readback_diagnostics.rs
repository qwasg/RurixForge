//! Explicit private HQ host collection witness; no environment activation.
//! RFC-0052; continuation_1003_p1_v1/HQ_READBACK_DIAGNOSTICS_V1.md.
//! Assisted-by: Codex:gpt-6-astra.

/// Private HQ diagnostic selector. Indices are original registered readbacks.
#[doc(hidden)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HqReadbackFault {
    pub frame_ordinal: u64,
    pub readback_index: u32,
}

#[derive(Default)]
struct Request {
    original_index: usize,
    subset_position: usize,
    resource: u32,
    requested_bytes: u64,
    cumulative_bytes_before: u64,
    map_attempted: bool,
    map_succeeded: bool,
    unmapped: bool,
    process_memory_before_allocation: Option<[u64; 3]>,
}

#[cfg(windows)]
fn process_memory() -> Option<[u64; 3]> {
    #[repr(C)]
    #[derive(Default)]
    struct Counters {
        cb: u32, page_fault_count: u32, peak_working_set: usize, working_set: usize,
        quota_peak_paged: usize, quota_paged: usize, quota_peak_nonpaged: usize,
        quota_nonpaged: usize, pagefile: usize, peak_pagefile: usize, private_usage: usize,
    }
    #[link(name = "psapi")]
    unsafe extern "system" { fn GetProcessMemoryInfo(process: *mut std::ffi::c_void, counters: *mut Counters, size: u32) -> i32; }
    #[link(name = "kernel32")]
    unsafe extern "system" { fn GetCurrentProcess() -> *mut std::ffi::c_void; }
    let mut counters = Counters::default();
    counters.cb = std::mem::size_of::<Counters>() as u32;
    // SAFETY: U62 (unsafe-audit/rurix-rt.md): no arguments; borrowed current-process pseudo handle.
    let process = unsafe { GetCurrentProcess() };
    // SAFETY: U62: exact repr(C) COUNTERS_EX size/cb, initialized writable stack output alive across call.
    let okay = unsafe { GetProcessMemoryInfo(process, &mut counters, std::mem::size_of::<Counters>() as u32) };
    (okay != 0).then_some([counters.working_set as u64, counters.peak_working_set as u64, counters.private_usage as u64])
}
#[cfg(not(windows))]
fn process_memory() -> Option<[u64; 3]> { None }

struct Frame {
    ordinal: u64,
    selected_count: usize,
    completed_count: usize,
    completed_bytes: u64,
    live_bytes: u64,
    peak_bytes: u64,
    request: Option<Request>,
    status: &'static str,
}

pub(super) struct State {
    fault: Option<HqReadbackFault>,
    next_ordinal: u64,
    readback_count: usize,
    last: Option<Frame>,
    poisoned: bool,
}

impl State {
    pub(super) fn new(fault: Option<HqReadbackFault>, readback_count: usize) -> Result<Self, String> {
        if fault.is_some_and(|f| f.readback_index as usize >= readback_count) {
            return Err("HQ diagnostic original readback index out of range".into());
        }
        Ok(Self { fault, next_ordinal: 0, readback_count, last: None, poisoned: false })
    }
    pub(super) fn require_clean(&self) -> Result<(), String> {
        if self.poisoned { Err("HQ readback diagnostic session must be discarded after failure".into()) }
        else { Ok(()) }
    }
    pub(super) fn begin(&mut self, selected_indices: &[usize]) -> Result<(), String> {
        self.require_clean()?;
        if self.last.as_ref().is_some_and(|f| f.status != "ownership_transferred_to_device_frame_output") {
            return Err("HQ collection already active or not transferred".into());
        }
        let ordinal = self.next_ordinal;
        self.next_ordinal = ordinal.checked_add(1).ok_or("HQ collection ordinal overflow")?;
        self.last = Some(Frame { ordinal, selected_count: selected_indices.len(), completed_count: 0,
            completed_bytes: 0, live_bytes: 0, peak_bytes: 0, request: None, status: "collecting" });
        if selected_indices.iter().any(|&index| index >= self.readback_count)
            || selected_indices.iter().enumerate().any(|(i, index)| selected_indices[..i].contains(index)) {
            self.failed("invalid_registered_selection");
            return Err("HQ diagnostic original readback selection invalid".into());
        }
        if self.fault.is_some_and(|fault| fault.frame_ordinal == ordinal
            && !selected_indices.contains(&(fault.readback_index as usize))) {
            self.failed("fault_target_not_selected_before_dispatch");
            return Err("HQ diagnostic target readback not selected at target frame; no dispatch".into());
        }
        Ok(())
    }
    pub(super) fn before(&mut self, original_index: usize, subset_position: usize,
        resource: u32, requested_bytes: u64) -> bool {
        let f = self.last.as_mut().expect("collection has begun");
        f.request = Some(Request { original_index, subset_position, resource, requested_bytes,
            cumulative_bytes_before: f.completed_bytes, process_memory_before_allocation: process_memory(), ..Default::default() });
        self.fault.is_some_and(|fault| fault.frame_ordinal == f.ordinal
            && fault.readback_index as usize == original_index)
    }
    pub(super) fn allocated(&mut self, size: u64) {
        let f = self.last.as_mut().expect("collection has begun");
        // Actual live Vec lengths fit usize; capacity checks precede allocation.
        f.live_bytes += size;
        f.peak_bytes = f.peak_bytes.max(f.live_bytes);
    }
    pub(super) fn map_attempt(&mut self) {
        self.last.as_mut().unwrap().request.as_mut().unwrap().map_attempted = true;
    }
    pub(super) fn mapped(&mut self) {
        self.last.as_mut().unwrap().request.as_mut().unwrap().map_succeeded = true;
    }
    pub(super) fn unmapped(&mut self) {
        self.last.as_mut().unwrap().request.as_mut().unwrap().unmapped = true;
    }
    pub(super) fn completed(&mut self, size: u64) {
        let f = self.last.as_mut().unwrap();
        f.completed_count += 1;
        f.completed_bytes += size;
    }
    pub(super) fn failed(&mut self, status: &'static str) {
        self.poisoned = true;
        if let Some(f) = self.last.as_mut() {
            f.status = status;
            // Caller has released current payload and all collected outputs.
            f.live_bytes = 0;
        }
    }
    pub(super) fn fail_if_clean(&mut self, status: &'static str) {
        if !self.poisoned { self.failed(status); }
    }
    pub(super) fn returned(&mut self) -> Result<(), String> {
        self.require_clean()?;
        let f = self.last.as_mut().ok_or("HQ collection has not begun")?;
        if f.status != "collecting" || f.completed_count != f.selected_count {
            return Err("HQ collection incomplete or ownership already transferred".into());
        }
        f.status = "ownership_transferred_to_device_frame_output";
        Ok(())
    }
    pub(super) fn evidence_json(&self) -> String {
        let Some(f) = self.last.as_ref() else {
            return "{\"schema\":\"rurix.hq.host_readback_lifecycle.v1\",\"status\":\"enabled_before_first_collection\",\"session_poisoned\":false}".into();
        };
        let current = f.request.as_ref().map(|r| format!(
            "{{\"original_readback_index\":{},\"subset_position\":{},\"source_resource\":{},\"requested_bytes\":{},\"cumulative_bytes_before\":{},\"map_attempted\":{},\"map_succeeded\":{},\"unmapped\":{},\"process_memory_before_allocation\":{}}}",
            r.original_index, r.subset_position, r.resource, r.requested_bytes,
            r.cumulative_bytes_before, r.map_attempted, r.map_succeeded, r.unmapped,
            r.process_memory_before_allocation.map(|[working, peak, private]| format!("{{\"working_set_bytes\":{working},\"peak_working_set_bytes\":{peak},\"private_commit_bytes\":{private}}}")).unwrap_or_else(|| "null".into())))
            .unwrap_or_else(|| "null".into());
        format!("{{\"schema\":\"rurix.hq.host_readback_lifecycle.v1\",\"frame_ordinal\":{},\"status\":\"{}\",\"selected_count\":{},\"completed_count\":{},\"completed_bytes\":{},\"current_frame_live_payload_bytes\":{},\"current_frame_peak_payload_bytes\":{},\"current_request\":{},\"session_poisoned\":{},\"scope\":\"current collection payload ownership; separate point-in-time OS counters; no future capacity guarantee\"}}",
            f.ordinal, f.status, f.selected_count, f.completed_count, f.completed_bytes,
            f.live_bytes, f.peak_bytes, current, self.poisoned)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn exact_frame_and_original_subset_index_select_first_middle_last_failures() {
        for target in [1usize, 3, 7] {
            let mut state = State::new(Some(HqReadbackFault { frame_ordinal: 1,
                readback_index: target as u32 }), 8).unwrap();
            for frame in 0..2 {
                state.begin(&[1, 3, 7]).unwrap();
                for (position, index) in [1usize, 3, 7].into_iter().enumerate() {
                    let fault = state.before(index, position, 40 + index as u32, 12);
                    if fault {
                        assert_eq!(frame, 1); assert_eq!(index, target);
                        state.failed("injected_host_allocation_failure");
                        let f = state.last.as_ref().unwrap();
                        let r = f.request.as_ref().unwrap();
                        assert_eq!(r.cumulative_bytes_before, position as u64 * 12);
                        assert!(!r.map_attempted && !r.map_succeeded && !r.unmapped);
                        assert_eq!(f.live_bytes, 0);
                        assert_eq!(f.peak_bytes, position as u64 * 12);
                        assert!(state.begin(&[1, 3, 7]).is_err());
                        break;
                    }
                    state.allocated(12); state.map_attempt(); state.mapped();
                    state.unmapped(); state.completed(12);
                }
                if frame == 0 { state.returned().unwrap(); }
            }
        }
    }
    #[test]
    fn clean_collection_retains_payload_ownership_and_actual_map_state() {
        let mut state = State::new(None, 2).unwrap();
        state.begin(&[0, 1]).unwrap();
        for (index, size) in [(0, 12), (1, 24)] {
            assert!(!state.before(index, index, index as u32, size));
            state.allocated(size); state.map_attempt(); state.mapped(); state.unmapped();
            state.completed(size);
        }
        state.returned().unwrap();
        let f = state.last.as_ref().unwrap();
        assert_eq!((f.completed_count, f.completed_bytes, f.live_bytes, f.peak_bytes), (2, 36, 36, 36));
        assert!(state.evidence_json().contains("ownership_transferred"));
        assert!(state.evidence_json().contains("\"session_poisoned\":false"));
        #[cfg(windows)]
        { let memory = f.request.as_ref().unwrap().process_memory_before_allocation.unwrap();
          assert!(memory[0] > 0 && memory[1] >= memory[0] && memory[2] > 0); }
        assert!(state.returned().is_err());
        assert!(State::new(Some(HqReadbackFault { frame_ordinal: 0, readback_index: 2 }), 2).is_err());
    }
    #[test]
    fn original_index_survives_subset_permutation_and_missing_target_refuses_dispatch() {
        for selected in [[7usize, 1, 3], [3, 7, 1]] {
            let mut state = State::new(Some(HqReadbackFault { frame_ordinal: 0, readback_index: 7 }), 8).unwrap();
            state.begin(&selected).unwrap();
            for (position, index) in selected.into_iter().enumerate() {
                let hit = state.before(index, position, 20 + index as u32, 12);
                assert_eq!(hit, index == 7);
                if hit { state.failed("injected_host_allocation_failure"); break; }
                state.allocated(12); state.map_attempt(); state.mapped(); state.unmapped(); state.completed(12);
            }
        }
        let fault = Some(HqReadbackFault { frame_ordinal: 0, readback_index: 7 });
        let mut missing = State::new(fault, 8).unwrap();
        assert!(missing.begin(&[1, 3]).unwrap_err().contains("no dispatch"));
        assert!(missing.require_clean().is_err());
        for selected in [&[8usize][..], &[1usize, 1][..]] {
            assert!(State::new(None, 8).unwrap().begin(selected).is_err());
        }
        let mut fresh = State::new(None, 8).unwrap();
        fresh.begin(&[7]).unwrap();
        assert!(!fresh.before(7, 0, 47, 12));
        fresh.allocated(12); fresh.map_attempt(); fresh.mapped(); fresh.unmapped(); fresh.completed(12);
        fresh.returned().unwrap();
    }
    #[test]
    fn actual_allocation_and_map_errors_preserve_peak_and_clear_live_ownership() {
        for status in ["host_payload_allocation_failure", "map_failure", "map_success_null_pointer"] {
            let mut state = State::new(None, 2).unwrap();
            state.begin(&[0, 1]).unwrap();
            state.before(0, 0, 10, 12); state.allocated(12); state.map_attempt(); state.mapped(); state.unmapped(); state.completed(12);
            state.before(1, 1, 11, 24);
            if status != "host_payload_allocation_failure" { state.allocated(24); state.map_attempt(); }
            if status == "map_success_null_pointer" { state.unmapped(); }
            state.failed(status); state.fail_if_clean("generic_error");
            let f = state.last.as_ref().unwrap();
            assert_eq!(f.status, status); assert_eq!(f.live_bytes, 0); assert_eq!(f.completed_bytes, 12);
            assert_eq!(f.peak_bytes, if status == "host_payload_allocation_failure" { 12 } else { 36 });
            assert!(!f.request.as_ref().unwrap().map_succeeded);
            assert!(state.returned().is_err());
        }
    }
}
