//! Private sequential HQ command segmentation. RFC-0052, existing U32.
//! Disabled by default; no Vulkan object allocation or timeout policy change.
//! Assisted-by: Codex:GPT-6.
use super::*;

struct Chunk {
    begin: u32, end: u32, is_final: bool,
    submitted: bool, fence_complete: bool,
    submit_result: Option<i32>, wait_result: Option<i32>,
    submit_ns: u64, wait_ns: u64, resume_barrier: bool, reused_after_fence: bool,
}
struct Frame {
    ordinal: u64, pass_count: u32, status: &'static str,
    uploads: usize, descriptors: usize, query_resets: u32, completed: u32,
    final_fence: bool, timestamps: bool, returned: bool, returned_count: usize,
    failure: Option<&'static str>, current: usize, chunks: Vec<Chunk>,
}
pub(super) struct State {
    pub(super) max_passes: u32,
    poisoned: bool, started: u64, last: Option<Frame>,
}

impl State {
    pub(super) fn new(max_passes: u32) -> Result<Self,String> {
        if !(16..=256).contains(&max_passes) || !max_passes.is_power_of_two() {
            return Err("HQ submission chunk limit must be a power of two in 16..256".into());
        }
        Ok(Self {max_passes,poisoned:false,started:0,last:None})
    }
    pub(super) fn require_clean(&self) -> Result<(),String> {
        if self.poisoned {Err("HQ chunked session must be discarded after failure".into())} else {Ok(())}
    }
    pub(super) fn begin(&mut self,passes: usize) -> Result<(),String> {
        self.require_clean()?;
        if self.last.as_ref().is_some_and(|f| !f.returned) {return Err("HQ chunk frame already active".into());}
        let count=u32::try_from(passes).map_err(|_| "HQ chunk pass count overflow")?;
        if count==0 || count>u32::MAX/2 {return Err("HQ chunk timestamp range invalid".into());}
        let mut chunks=Vec::new();
        chunks.try_reserve_exact(count.div_ceil(self.max_passes) as usize).map_err(|_| "HQ chunk evidence allocation failed")?;
        for begin in (0..count).step_by(self.max_passes as usize) {
            let end=begin.saturating_add(self.max_passes).min(count);
            chunks.push(Chunk {begin,end,is_final:end==count,submitted:false,fence_complete:false,
                submit_result:None,wait_result:None,submit_ns:0,wait_ns:0,resume_barrier:false,reused_after_fence:false});
        }
        let ordinal=self.started;self.started=self.started.checked_add(1).ok_or("HQ chunk ordinal overflow")?;
        self.last=Some(Frame {ordinal,pass_count:count,status:"RECORDING",uploads:0,descriptors:0,query_resets:0,
            completed:0,final_fence:false,timestamps:false,returned:false,returned_count:0,failure:None,current:0,chunks});
        Ok(())
    }
    pub(super) fn fail(&mut self,phase: &'static str) {
        self.poisoned=true;
        if let Some(f)=self.last.as_mut() {f.status="FAILED";f.failure.get_or_insert(phase);f.returned=false;f.returned_count=0;}
    }
    pub(super) fn uploaded(&mut self) {if let Some(f)=self.last.as_mut(){f.uploads+=1;}}
    pub(super) fn descriptor_updated(&mut self) {if let Some(f)=self.last.as_mut(){f.descriptors+=1;}}
    pub(super) fn query_reset(&mut self) {if let Some(f)=self.last.as_mut(){f.query_resets+=1;}}
    pub(super) fn is_boundary(&self,end: usize) -> bool {
        self.last.as_ref().is_some_and(|f| end==f.chunks[f.current].end as usize && end<f.pass_count as usize)
    }
    pub(super) fn before_submit(&mut self,is_final: bool) -> Result<(),String> {
        self.require_clean()?;let f=self.last.as_mut().ok_or("HQ chunk frame absent")?;
        let c=&f.chunks[f.current];
        if f.status!="RECORDING" || c.is_final!=is_final || c.submit_result.is_some() || f.query_resets!=1 {
            return Err("HQ chunk submit lifecycle/range mismatch".into());
        }
        f.status="SUBMITTING";Ok(())
    }
    pub(super) fn submitted(&mut self,result:i32,ns:u64) {
        if let Some(f)=self.last.as_mut() {let c=&mut f.chunks[f.current];c.submit_result=Some(result);c.submit_ns=ns;
            c.submitted=result==VK_SUCCESS;f.status="WAITING";}
        if result!=VK_SUCCESS {self.fail("queue_submit");}
    }
    pub(super) fn waited(&mut self,result:i32,ns:u64) -> Result<(),String> {
        let f=self.last.as_mut().ok_or("HQ chunk frame absent")?;let c=&mut f.chunks[f.current];
        if f.status!="WAITING" || !c.submitted || c.wait_result.is_some() {return Err("HQ chunk wait lifecycle mismatch".into());}
        c.wait_result=Some(result);c.wait_ns=ns;c.fence_complete=result==VK_SUCCESS;
        if result==VK_SUCCESS {f.completed=c.end;if c.is_final {f.final_fence=true;f.status="COLLECTING";}}
        else {self.fail("fence_wait");}
        Ok(())
    }
    pub(super) fn resumed(&mut self) -> Result<(),String> {
        self.require_clean()?;let f=self.last.as_mut().ok_or("HQ chunk frame absent")?;
        if !f.chunks[f.current].fence_complete || f.chunks[f.current].is_final {return Err("HQ chunk resume before fence/after final".into());}
        f.current+=1;let c=&mut f.chunks[f.current];c.resume_barrier=true;c.reused_after_fence=true;f.status="RECORDING";Ok(())
    }
    pub(super) fn timestamps(&mut self) -> Result<(),String> {
        let f=self.last.as_mut().ok_or("HQ chunk frame absent")?;
        if !f.final_fence || f.completed!=f.pass_count {return Err("HQ chunk timestamp collection before final fence".into());}
        f.timestamps=true;Ok(())
    }
    pub(super) fn returned(&mut self,count:usize) -> Result<(),String> {
        self.require_clean()?;let f=self.last.as_mut().ok_or("HQ chunk frame absent")?;
        if !f.final_fence || !f.timestamps || f.query_resets!=1 || f.completed!=f.pass_count {
            return Err("HQ chunk incomplete frame cannot return readbacks".into());
        }
        f.returned=true;f.returned_count=count;f.status="RETURNED";Ok(())
    }
    pub(super) fn intermediate_submit_ns(&self) -> u64 {
        self.last.as_ref().map_or(0,|f| f.chunks.iter().filter(|c|!c.is_final).map(|c|c.submit_ns).sum())
    }
    pub(super) fn intermediate_wait_ns(&self) -> u64 {
        self.last.as_ref().map_or(0,|f| f.chunks.iter().filter(|c|!c.is_final).map(|c|c.wait_ns).sum())
    }
    pub(super) fn evidence_json(&self) -> String {
        let latest=self.last.as_ref().map_or_else(||"null".into(),|f| {
            let chunks=f.chunks.iter().enumerate().map(|(i,c)|format!(
                "{{\"index\":{i},\"pass_begin\":{},\"pass_end\":{},\"query_begin\":{},\"query_end\":{},\"is_final\":{},\"submitted\":{},\"fence_complete\":{},\"submit_result\":{},\"wait_result\":{},\"submit_ns\":{},\"wait_ns\":{},\"resume_barrier\":{},\"command_buffer_reused_after_fence\":{}}}",
                c.begin,c.end,c.begin*2,c.end*2,c.is_final,c.submitted,c.fence_complete,
                c.submit_result.map_or("null".into(),|v|v.to_string()),c.wait_result.map_or("null".into(),|v|v.to_string()),c.submit_ns,c.wait_ns,c.resume_barrier,c.reused_after_fence)).collect::<Vec<_>>().join(",");
            format!("{{\"ordinal\":{},\"pass_count\":{},\"status\":\"{}\",\"uploads\":{},\"descriptor_updates\":{},\"query_reset_count\":{},\"completed_passes\":{},\"final_fence_complete\":{},\"timestamps_collected\":{},\"readbacks_returned\":{},\"failure_phase\":{},\"chunks\":[{}]}}",
                f.ordinal,f.pass_count,f.status,f.uploads,f.descriptors,f.query_resets,f.completed,f.final_fence,f.timestamps,f.returned_count,
                f.failure.map_or("null".into(),|v|format!("\"{v}\"")),chunks)
        });
        format!("{{\"schema\":\"rurix.hq.submission_chunks.v1\",\"max_passes\":{},\"wait_timeout_ns\":{},\"poisoned\":{},\"frames_started\":{},\"latest_frame\":{latest}}}",self.max_passes,PERSISTENT_WAIT_TIMEOUT_NS,self.poisoned,self.started)
    }
}

pub(super) struct Recorder<'a> {
    pub(super) state: &'a mut State,
    pub(super) queue: VkQueue,
    pub(super) fence: VkFence,
    pub(super) validation_errors: &'a std::sync::atomic::AtomicU64,
    pub(super) validation_before: u64,
}
impl Recorder<'_> {
    /// Borrowed U32 handles; no pending command buffer is reset. The caller
    /// retains tracked resource state and resets timestamps only in chunk zero.
    pub(super) unsafe fn submit_and_resume(&mut self,p:&FrameBodyParams<'_>) -> Result<(),String> {
        self.state.before_submit(false)?;
        // SAFETY: U32: this same-device command buffer is recording, outside render passes.
        let end=unsafe {(p.dev.end_cmd)(p.cmd)};
        if end!=VK_SUCCESS {self.state.fail("end_command");return Err(queue_result_error("HQ chunk vkEndCommandBuffer",end));}
        let submit=SubmitInfo {s_type:ST_SUBMIT_INFO,p_next:std::ptr::null(),wait_semaphore_count:0,
            p_wait_semaphores:std::ptr::null(),p_wait_dst_stage_mask:std::ptr::null(),command_buffer_count:1,
            p_command_buffers:&p.cmd,signal_semaphore_count:0,p_signal_semaphores:std::ptr::null()};
        let started=std::time::Instant::now();
        // SAFETY: U32: ended cmd, unsignaled nonpending fence; submit/handle stack lives through call.
        let result={let _permit=super::hq_setup_failure::permit()?;
            // SAFETY: U32: same already-ended command and nonpending fence;
            // worker quarantine was checked under the submission gate.
            unsafe {(p.dev.queue_submit)(self.queue,1,&submit,self.fence)}};
        self.state.submitted(result,elapsed_ns(started));
        if result!=VK_SUCCESS {return Err(queue_result_error("HQ chunk vkQueueSubmit",result));}
        let started=std::time::Instant::now();
        // SAFETY: U32: valid same-device fence from the just submitted chunk; existing finite bound.
        let result=unsafe {(p.dev.wait_fences)(p.device,1,&self.fence,1,PERSISTENT_WAIT_TIMEOUT_NS)};
        let result=g31_fault_fence_timeout(result);self.state.waited(result,elapsed_ns(started))?;
        if result!=VK_SUCCESS {return Err(queue_result_error("HQ chunk bounded fence wait",result));}
        if self.validation_errors.load(std::sync::atomic::Ordering::Relaxed)>self.validation_before {
            self.state.fail("validation");return Err("HQ intermediate chunk reported Vulkan validation ERROR; discard session".into());
        }
        // SAFETY: U32: the completed signal has no pending queue use; reset only after successful wait.
        let reset=unsafe {(p.dev.reset_fences)(p.device,1,&self.fence)};
        if reset!=VK_SUCCESS {self.state.fail("reset_fence");return Err(queue_result_error("HQ chunk reset fence",reset));}
        // SAFETY: U32: fence proves cmd no longer pending; its pool permits individual resets.
        let reset=unsafe {(p.dev.reset_cmd)(p.cmd,0)};
        if reset!=VK_SUCCESS {self.state.fail("reset_command");return Err(queue_result_error("HQ chunk reset command",reset));}
        let begin=CommandBufferBeginInfo {s_type:ST_COMMAND_BUFFER_BEGIN_INFO,p_next:std::ptr::null(),flags:0,p_inheritance_info:std::ptr::null()};
        // SAFETY: U32: same valid primary cmd is reset; initialized stack begin-info lives through call.
        let result=unsafe {(p.dev.begin_cmd)(p.cmd,&begin)};
        if result!=VK_SUCCESS {self.state.fail("begin_command");return Err(queue_result_error("HQ chunk begin command",result));}
        // SAFETY: U32: resumed same-queue cmd records global availability/visibility before consumers.
        unsafe {cmd_global_barrier2(p.dev,p.cmd,STAGE2_ALL_COMMANDS,ACCESS2_MEMORY_WRITE,STAGE2_ALL_COMMANDS,ACCESS2_MEMORY_READ|ACCESS2_MEMORY_WRITE)};
        self.state.resumed()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test] fn closed_limits_and_complete_query_ranges() {
        for limit in [0,1,15,17,63,257,512] {assert!(State::new(limit).is_err());}
        for limit in [16,32,64,128,256] {
            let mut s=State::new(limit).unwrap();s.begin(1540).unwrap();let f=s.last.as_ref().unwrap();
            assert_eq!(f.chunks.first().unwrap().begin,0);assert_eq!(f.chunks.last().unwrap().end,1540);
            for pair in f.chunks.windows(2) {assert_eq!(pair[0].end,pair[1].begin);}
            assert!(f.chunks.iter().all(|c|c.end>c.begin && c.end-c.begin<=limit));
            assert!(s.before_submit(false).is_err()); // Missing query reset.
        }
    }
    #[test] fn only_last_complete_chunk_can_return_payload() {
        let mut s=State::new(64).unwrap();s.begin(129).unwrap();s.query_reset();s.uploaded();s.descriptor_updated();
        for final_chunk in [false,false,true] {
            assert!(s.returned(2).is_err());s.before_submit(final_chunk).unwrap();s.submitted(VK_SUCCESS,11);
            assert!(s.resumed().is_err());assert!(s.timestamps().is_err());s.waited(VK_SUCCESS,13).unwrap();
            if !final_chunk {s.resumed().unwrap();}
        }
        assert!(s.resumed().is_err());s.timestamps().unwrap();s.returned(2).unwrap();
        assert!(s.evidence_json().contains("\"completed_passes\":129"));assert_eq!(s.intermediate_wait_ns(),26);
        s.begin(1).unwrap();assert_eq!(s.last.as_ref().unwrap().ordinal,1);
    }
    #[test] fn middle_failure_poison_never_returns_or_restarts() {
        let mut s=State::new(16).unwrap();s.begin(33).unwrap();s.query_reset();s.before_submit(false).unwrap();
        s.submitted(VK_SUCCESS,1);s.waited(VK_SUCCESS,2).unwrap();s.resumed().unwrap();s.before_submit(false).unwrap();
        s.submitted(VK_SUCCESS,1);s.waited(VK_TIMEOUT,5_000_000_000).unwrap();
        assert!(s.returned(2).is_err());assert!(s.resumed().is_err());assert!(s.begin(33).is_err());
        assert_eq!(s.last.as_ref().unwrap().completed,16);assert!(!s.last.as_ref().unwrap().chunks[2].submitted);
    }
    #[test] fn duplicates_and_premature_final_are_rejected() {
        let mut s=State::new(16).unwrap();s.begin(17).unwrap();s.query_reset();assert!(s.before_submit(true).is_err());
        s.before_submit(false).unwrap();assert!(s.before_submit(false).is_err());s.submitted(VK_SUCCESS,0);
        s.waited(VK_SUCCESS,1).unwrap();assert!(s.waited(VK_SUCCESS,1).is_err());s.resumed().unwrap();s.query_reset();
        assert!(s.before_submit(true).is_err());
    }
}
