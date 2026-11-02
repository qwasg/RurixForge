//! Artifact-only selected Bistro owner. Old authored1x1 Pending is untouched.
//! Consumes a freshly initialized REAL DeviceFrameSession and returns only the
//! selected diagnostic payload after one suffix. No legacy owner escapes.
use super::*;
use super::agent_persistent_transaction as tx;
use std::io::Write;
use std::ops::Range;
#[derive(Clone)]
pub struct Config {
    pub capacity:u32,
    /// Disposable HQ batch logs may lose the current unfinished frame on a crash.
    /// Default false retains durable per-region logging.
    pub journal_sync_per_frame:bool,
    pub stages:Vec<Vec<Range<usize>>>,pub pixels:u64,pub command:usize,pub header:usize,
    pub identity:usize,pub selector:usize,pub width:u32,pub height:u32,
    pub interface_budget:u32,
    pub sealed_bindings:std::collections::BTreeMap<usize,[u8;32]>,
    pub frame:usize,pub sample:u64,pub seed:u64,pub source_digest:[u8;32],
    pub cloud_inputs:Option<[usize;5]>,

}
pub struct Complete<V>{pub readbacks:Vec<Vec<u8>>,pub validated:V,pub report:String,pub telemetry:DeviceFrameTelemetry}
impl Config{pub fn schedule_json(&self)->String{let stages=self.stages.iter().map(|parts|format!("[{}]",parts.iter().map(|r|format!("[{},{}]",r.start,r.end)).collect::<Vec<_>>().join(","))).collect::<Vec<_>>().join(",");format!("{{\"schema\":\"rurix.hq.optical_schedule.v1\",\"width\":{},\"height\":{},\"pixels\":{},\"capacity\":{},\"budget\":{},\"frame_resource\":{},\"seed_u64\":{},\"finite_pass_threshold\":{},\"r_prefix_max_as\":1,\"stages\":[{stages}]}}",self.width,self.height,self.pixels,self.capacity,self.interface_budget,self.frame,self.seed,super::agent_persistent_factory::FINITE_PASS_THRESHOLD)}}
pub struct PersistentOwner<'a>{device:std::mem::ManuallyDrop<DeviceFrameSession<'a>>,cfg:Config,transaction:Option<tx::Session>,journal:std::fs::File,descriptor_hashes:Vec<[u8;32]>,poisoned:bool,total_metadata_budget:u64,failure_probe:Option<(usize,usize)>}
impl Drop for PersistentOwner<'_>{fn drop(&mut self){
    if hq_setup_failure::poisoned(){return;}
    // SAFETY: confirmed/never-submitted owner follows the original native Drop.
    // Unconfirmed GPU work retains the ENTIRE ManuallyDrop ownership graph,
    // including AS/setup objects, callback Box, buffers and descriptor storage.
    unsafe{std::mem::ManuallyDrop::drop(&mut self.device);}
}}

// RFC-0054/U32 safe admission: only the explicit HQ constructor supplies this
// phase. Ownership stays separate from the access/layout transaction tracker.
fn admit_private_bistro_buffers(resources:&[ResourceDesc<'_>],exports:&[(u32,u64,u32)],imports:&[u32],
    phase:Option<HqExternalPhase>,committed_generation:u64)->Result<(),String>{
    if !imports.is_empty() || resources.iter().any(|r|!matches!(r,ResourceDesc::Buffer(_))){
        return Err("private Bistro owner requires buffer-only resources and no imports".into());
    }
    let ids=exports.iter().map(|row|row.0).collect::<Vec<_>>();
    validate_hq_exportable_buffers(resources,&ids)?;
    for &(id,allocation_bytes,_) in exports {
        let Some(ResourceDesc::Buffer(buffer))=resources.get(id as usize) else{return Err("private Bistro exporter is not a linear buffer".into());};
        if buffer.size==0 || allocation_bytes<buffer.size{return Err("private Bistro external allocation does not cover its buffer".into());}
    }
    match (exports.is_empty(),phase,committed_generation){
        (true,None,_)=>Ok(()),
        (false,Some(HqExternalPhase::Producer),0)=>Ok(()),
        (false,Some(HqExternalPhase::ConsumerComplete),generation) if generation>0=>Ok(()),
        _=>Err("private Bistro exports require fresh HQ Producer or a completed external consumer before the next frame".into()),
    }
}
fn private_bistro_final_release(exports:bool,phase:Option<HqExternalPhase>)->Result<(),String>{
    if matches!((exports,phase),(false,None)|(true,Some(HqExternalPhase::Released))){Ok(())}
    else{Err("private Bistro final suffix did not complete its external release".into())}
}
impl<'a> PersistentOwner<'a>{
    /// RFC-0054: completed blocking RR consumption returns linear exporters to
    /// the next logical frame. No region may invent an external completion.
    pub fn mark_external_consumer_complete(&mut self)->Result<(),String>{self.device.mark_external_consumer_complete()}
    pub fn new(device:DeviceFrameSession<'a>,cfg:Config,directory:&std::path::Path,total_metadata_budget:u64)->Result<Self,String>{
        if !matches!(cfg.capacity,4096|65536)||device.frame_generation!=0||total_metadata_budget==0||total_metadata_budget>4*1024*1024{return Err("new persistent owner or aggregate metadata admission invalid".into())}
        admit_private_bistro_buffers(device.resources,&device.native.frame.exportable_meta,&device.native.frame.imported_indices,
            device.native.external_roundtrip,device.frame_generation)?;
        std::fs::create_dir_all(directory).map_err(|e|e.to_string())?;
        let journal=std::fs::OpenOptions::new().write(true).create_new(true).open(directory.join("agent_persistent_regions.txt")).map_err(|e|e.to_string())?;
        let descriptor_hashes=device.passes.iter().map(|p|rurix_pkg::sha256::digest(format!("{:?}",pass_bindings(p)).as_bytes())).collect();
        Ok(Self{device:std::mem::ManuallyDrop::new(device),cfg,transaction:None,journal,descriptor_hashes,poisoned:false,total_metadata_budget,failure_probe:None})
    }
    /// No legacy execute/submit or raw device mutation entry is exposed. The
    /// graph and allocation owner remain alive across all successful frames.
    pub fn frame<V>(&mut self,update:FrameUpdate,sample:u64,seed:u64,clouds:bool,other_metadata_before:u64,out:&std::path::Path,validate:impl FnOnce(&[Vec<u8>],&DeviceFrameTelemetry,u64)->Result<(V,u64),String>)->Result<Complete<V>,String>{
        if self.poisoned{return Err("persistent optical owner poisoned; discard required".into())}
        if sample>(1u64<<24){return Err("sample exceeds the exact legacy frame0 identity domain; no u64/f32 alias admission".into())}
        if seed!=self.cfg.seed||self.cfg.interface_budget==0||self.cfg.interface_budget>32{return Err("sampling seed/budget requires source-bound normal-table graph admission".into())}
        if clouds!=self.cfg.cloud_inputs.is_some()||(!clouds&&self.device.passes.iter().any(|p|pass_name(p).contains("canonical_air")||pass_name(p).contains("solar_cloud")||pass_name(p).contains("finite_cloud"))){return Err("optical cloud mode differs from the source-admitted segment operator family".into())}
        let mut cfg=self.cfg.clone();cfg.sample=sample;cfg.seed=seed;
        let native_generation_before=self.device.frame_generation;
        let result=run_borrowed(&mut self.device,&mut self.transaction,&mut self.journal,&mut self.descriptor_hashes,cfg,update,self.total_metadata_budget,other_metadata_before,self.failure_probe,out,validate);
        if let Err(reason)=&result{
            self.poisoned=true;
            // Observation only: this does not manufacture a VkResult or infer
            // whether the application publisher started. Its own failure receipt
            // supplies that fact. Record the actual owner state before Drop.
            let audit=format!("{{\"schema\":\"rurix.hq.optical_owner_failure.v1\",\"native_generation_before\":{native_generation_before},\"native_generation_after\":{},\"poisoned\":{},\"native_completion_unconfirmed_quarantined\":{},\"reason\":{reason:?}}}",self.device.frame_generation,self.poisoned,hq_setup_failure::poisoned());
            if let Err(error)=std::fs::write(out.join("agent_owner_failure.json"),audit){return Err(format!("{reason}; optical owner failure audit: {error}"));}
        }result
    }
    /// An existing read-only probe may survive a rejected shader-error header.
    /// This neither publishes a frame nor changes any transport/error decision.
    pub fn observe_failed_frame(&mut self,resource:usize,bytes:usize)->Result<(),String>{
        if !matches!(self.device.resources.get(resource),Some(ResourceDesc::Buffer(b))
            if !b.device_local && bytes>0 && bytes as u64<=b.size) {
            return Err("failed-frame probe requires an existing host-visible buffer range".into());
        }
        self.failure_probe=Some((resource,bytes));Ok(())
    }
    pub fn committed_generation(&self)->u64{self.device.frame_generation}
    pub fn is_poisoned(&self)->bool{self.poisoned}
    pub fn device_caps(&self)->&DeviceCaps{self.device.device_caps()}
    pub fn capacity_preflight(&self)->Option<&FrameCapacityReport>{self.device.capacity_preflight()}
    pub fn accel_backface_culling_enabled(&self)->bool{self.device.accel_backface_culling_enabled()}
}
fn run_borrowed<V>(device:&mut DeviceFrameSession<'_>,saved:&mut Option<tx::Session>,journal:&mut std::fs::File,descriptor_hashes:&mut Vec<[u8;32]>,cfg:Config,frozen:FrameUpdate,total_metadata_budget:u64,other_metadata_before:u64,failure_probe:Option<(usize,usize)>,out:&std::path::Path,validate:impl FnOnce(&[Vec<u8>],&DeviceFrameTelemetry,u64)->Result<(V,u64),String>)->Result<Complete<V>,String>{
    let frame_clock=std::time::Instant::now();
    let committed_before=device.frame_generation;
    let frame_validation_before=device.native.validation_errors.load(std::sync::atomic::Ordering::Relaxed);
    let mut measure=super::agent_persistent_telemetry::Measure::new(device.passes.len());
    if saved.as_ref().map_or(committed_before!=0,|t|t.committed().0!=committed_before){return Err("native/logical committed generation mismatch".into())}
    if device.native.slot_busy.iter().any(|x|*x) || device.native.capacity_preflight.is_none()
        ||device.native.hq_submission_chunks.is_some() ||device.native.hq_readback_diagnostics.is_some()
        {return Err("fresh private Bistro buffer session required".into())}
    admit_private_bistro_buffers(device.resources,&device.native.frame.exportable_meta,&device.native.frame.imported_indices,
        device.native.external_roundtrip,committed_before)?;
    if frozen.tlas_update.is_some()||frozen.blas_refit.is_some()||frozen.readback_subset.is_none() {return Err("fixed baked binding/static AS/readback profile".into())}
    // Original validator runs against the ACTUAL session. Uploads are retained
    // exactly once; frame/camera and material mip updates are never reconstructed.
    let (bindings,_)=device.frame_update_state(&frozen,None,None)?;
    for (&i,expected)in &cfg.sealed_bindings{let b=bindings.get(i).ok_or("sealed actual source pass absent")?;if rurix_pkg::sha256::digest(format!("{b:?}").as_bytes())!=*expected{return Err("actual FrameUpdate changed a tail-dependent source role".into())}}
    let frame=frozen.buffer_uploads.iter().find(|(r,offset,b)|r.0==cfg.frame as u64+1&&*offset==0&&b.len()==512).ok_or("actual complete frame update absent")?;
    let words=frame.2.chunks_exact(4).map(|b|f32::from_le_bytes(b.try_into().unwrap())).collect::<Vec<_>>();
    if words.len()!=128||cfg.sample>(1u64<<24)||words[0].to_bits()!=(cfg.sample as f32).to_bits()||words[1]!=(cfg.width as f32)||words[2]!=(cfg.height as f32)||cfg.pixels!=u64::from(cfg.width)*u64::from(cfg.height){return Err("actual frame viewport/sample does not match exact u64 host identity".into())}
    if let Some(inputs)=cfg.cloud_inputs{
        let cloud=frozen.buffer_uploads.iter().filter(|(r,_,_)|r.0==inputs[0]as u64+1).collect::<Vec<_>>();
        if cloud.len()!=1||cloud[0].1!=0||cloud[0].2.len()!=284{return Err("pending cloud profile requires exactly one actual complete params71 upload".into())}
        let values=cloud[0].2.chunks_exact(4).map(|v|f32::from_le_bytes(v.try_into().unwrap())).collect::<Vec<_>>();
        if values.iter().any(|v|!v.is_finite())||values[0]!=cfg.width as f32||values[1]!=cfg.height as f32{return Err("actual cloud snapshot extent/nonfinite data differs".into())}
    }
    let rb_sources=frozen.readback_subset.as_ref().unwrap().iter().map(|&i|i as usize).collect::<Vec<_>>();
    let selected_readbacks=rb_sources.iter().map(|&i|device.readbacks[i]).collect::<Vec<_>>();
    if selected_readbacks.is_empty(){return Err("persistent frame requires selected original readbacks".into())}

    if device.readbacks.len()<6 || device.readbacks.iter().any(|r|!matches!(r,Readback::Buffer{..})){return Err("real selected HDR buffer readbacks required".into())}
    if other_metadata_before>total_metadata_budget{return Err("aggregate metadata budget already exceeded before optical frame".into())}
    let as_count=device.native.as_state.as_ref().map_or(0,|s|s.managers.len());
    if as_count!=1||!device.accel_backface_culling_enabled(){return Err("original frozen AS identity/generation/culling differs".into())}
    let as_identity=|d:&DeviceFrameSession<'_>|{let mut v=vec![];if let Some(s)=d.native.as_state.as_ref(){for(i,m)in s.managers.iter().enumerate(){v.push(m.tlas());for&(mem,bytes,heap)in m.allocation_handles(){v.extend([mem,bytes,u64::from(heap)]);}v.push(d.resource_generations[d.resources.len()+i]);}}v};
    let as_before=as_identity(&device);let allocations=&device.native.resource_allocations;
    if allocations.len()!=device.resources.len()+as_count{return Err("actual allocation census differs".into())}
    let mut resources=vec![];
    for(i,r)in device.resources.iter().enumerate(){let ResourceDesc::Buffer(b)=r else{unreachable!()};
        let initialized=b.data.is_some_and(|x|x.len()as u64==b.size);
        if b.data.is_some()&&!initialized{return Err("partial source initialization is outside selected profile".into())}
        // NONE below is only the synchronized tracker state. A full data payload
        // and successful NativePersistentFrame creation establish initialization:
        // DL uses the runtime's source-bound one-shot copy + bounded fence; host
        // uses coherent map upload. Scratch remains uninitialized until its writer.
        let tracked=if let Some(t)=saved.as_ref(){t.committed_tracker()[i]}else if initialized&&!b.device_local{tx::HOST_WRITE}else{(0,STAGE2_NONE,0)};
        if committed_before==0{writeln!(journal,"initial resource {i} bytes {} device_local {} initialized {} tracker {:?} source_sha {} native_allocation {}",b.size,b.device_local,initialized,tracked,b.data.map(rurix_pkg::sha256::hex_digest).unwrap_or_else(||"none_scratch".into()),allocations[i].0).map_err(|e|e.to_string())?;}
        resources.push(tx::Resource{allocation:allocations[i].0,bytes:b.size,host_visible:!b.device_local,generation:device.resource_generations[i],tracked});
    }
    if let Some(state)=device.native.as_state.as_ref(){for(i,mgr)in state.managers.iter().enumerate(){let a=mgr.allocation_handles();let bytes=a.get(mgr.tlas_storage_allocation_index()).ok_or("AS allocation missing")?.1;resources.push(tx::Resource{allocation:allocations[device.resources.len()+i].0,bytes,host_visible:false,generation:device.resource_generations[device.resources.len()+i],tracked:if let Some(t)=saved.as_ref(){t.committed_tracker()[device.resources.len()+i]}else{(0,STAGE2_COMPUTE_SHADER,ACCESS2_ACCEL_STRUCTURE_READ)}});}}
    let passes=device.passes.iter().zip(&bindings).enumerate().map(|(pi,(p,b))|{
        let req=pass_requirements_with(p,b);tx::Pass{name:pass_name(p).to_owned(),barriers:device.barriers[pi].iter().map(|&(id,target)|(id as usize,state_fields(target))).collect(),accesses:pass_runtime_accesses_with(p,b,device.resources.len()as u32).into_iter().map(|(id,a)|tx::Access{resource:id as usize,writes:a!=RuntimeAccessKind::Read,target:if id as usize>=device.resources.len(){(0,STAGE2_COMPUTE_SHADER,ACCESS2_ACCEL_STRUCTURE_READ)}else{state_fields(req.iter().find(|(r,_)|*r==id).expect("source binding target").1)}}).collect()}
    }).collect();
    let mut uploads=vec![];let mut controls=vec![tx::ControlRange{resource:cfg.command,bytes:0..64}];let mut seen=std::collections::BTreeSet::new();
    controls.push(tx::ControlRange{resource:cfg.identity,bytes:0..40});controls.push(tx::ControlRange{resource:cfg.selector,bytes:0..64});
    for(id,offset,bytes)in frozen.buffer_uploads {let index=(id.0-1)as usize;if index==cfg.command||!seen.insert(index){return Err("duplicate/frozen command upload".into())}controls.push(tx::ControlRange{resource:index,bytes:offset..offset+bytes.len()as u64});uploads.push(tx::Upload{resource:index,offset,bytes});}
    // Derive the terminal state intent from the ACTUAL selected readback table
    // and actual native staging allocation. NONE is not used as initialization.
    let mut readback_map=std::collections::BTreeMap::new();
    if device.native.frame.rb_buffers.len()!=device.readbacks.len(){return Err("native HDR staging/readback count mismatch".into())}
    for (&source_index,rb) in rb_sources.iter().zip(&selected_readbacks){
        let Readback::Buffer{res,..}=*rb else{return Err("closed HDR buffer readback profile".into())};
        let ResourceDesc::Buffer(desc)=&device.resources[res as usize]else{unreachable!()};
        let staged=device.native.frame.rb_buffers[source_index].is_some();
        if staged!=desc.device_local{return Err("actual native staging disagrees with readback source residency".into())}
        if staged{readback_map.insert(res as usize,state_fields(TargetState::TransferSrc));}
    }
    let final_readback_states=readback_map.into_iter().collect::<Vec<_>>();
    let plan=tx::Plan{resources,passes,stages:cfg.stages,pixels:cfg.pixels,capacity:cfg.capacity,final_readback_states,header_resource:cfg.header,controls,histories:std::collections::BTreeSet::new()};

    let mut transaction=if let Some(mut t)=saved.take(){t.reconfigure_idle(plan)?;t}else{tx::Session::new(plan,committed_before)?};let snapshot=tx::Snapshot{sample:cfg.sample,sampling_seed:cfg.seed,immutable_inputs_sha256:cfg.source_digest};let mut token=transaction.begin(snapshot)?;
    if committed_before==0{
    let mut actual_graph_chain=[0u8;32];
    for(i,p)in device.passes.iter().enumerate(){let Pass::Compute(c)=p else{return Err("compute-only source graph".into())};
        let row=format!("{i}|{}|{}|{:?}\n",c.name,rurix_pkg::sha256::hex_digest(c.spirv),bindings[i]);
        let mut bytes=actual_graph_chain.to_vec();bytes.extend_from_slice(row.as_bytes());actual_graph_chain=rurix_pkg::sha256::digest(&bytes);
    }
    writeln!(journal,"actual_graph_source_bound_passes {} chained_sha256 {} row_format index_pipe_name_pipe_spvsha_pipe_BindingsDebug_newline",device.passes.len(),actual_graph_chain.iter().map(|v|format!("{v:02x}")).collect::<String>()).map_err(|e|e.to_string())?;
    if let Some(chain)=super::agent_persistent_graph::dispatch_source_chain(device.resources,device.passes,device.barriers)?{writeln!(journal,"actual_graph_dispatch_bound_passes {} chained_sha256 {} row_format resource_usage_then_index_pipe_dispatch_pipe_barriers_newline",device.passes.len(),chain.iter().map(|v|format!("{v:02x}")).collect::<String>()).map_err(|e|e.to_string())?;}
    journal.sync_data().map_err(|e|e.to_string())?;
    }
    let frame_log_start=journal.metadata().map_err(|e|e.to_string())?.len();
    writeln!(journal,"committed tracker entry count {:?}",transaction.committed_tracker().len()).map_err(|e|e.to_string())?;
    writeln!(journal,"begin frame_generation {} sample {} seed {} viewport {} {}",committed_before+1,cfg.sample,cfg.seed,cfg.width,cfg.height).map_err(|e|e.to_string())?;
    // Descriptor sets are not command-state: each changed actual FrameUpdate
    // binding must be rewritten after the previous frame fence completed.
    let native=&mut device.native;let fence=native.fences[0];
    // SAFETY: private persistent owner has no FIF/legacy users; bounded prior fence.
    let wait_started=std::time::Instant::now();let done=unsafe{(native.frame.dev.wait_fences)(native.device,1,&fence,1,PERSISTENT_WAIT_TIMEOUT_NS)};
    if done!=VK_SUCCESS{return Err(hq_setup_failure::quarantine(hq_setup_failure::Phase::OpticalRegionWait,done,elapsed_ns(wait_started),fence,native.frame.cmd as usize));}
    let handles=native.as_state.as_ref().map_or_else(Vec::new,|s|s.managers.iter().map(|m|m.tlas()).collect::<Vec<_>>());
    if descriptor_hashes.len()!=bindings.len(){return Err("persistent descriptor census changed".into())}
    for(i,b)in bindings.iter().enumerate(){let hash=rurix_pkg::sha256::digest(format!("{b:?}").as_bytes());if descriptor_hashes[i]!=hash{
        let set=native.frame.setups[i].set.ok_or("changed descriptor has no set")?;
        // SAFETY: original validator checked layout/bounds; preceding fence is
        // complete; this owner exclusively holds every affected descriptor set.
        unsafe{write_pass_descriptor_set(&native.frame.dev,native.device,set,b,&native.frame.rt,native.frame.cleanup.sampler,&handles,i)}?;
        descriptor_hashes[i]=hash;
    }}
    let mut count=0u64;let mut tiles=[0u64;2];let mut total_physical_active=0u64;
    loop {
        let header=transaction.next_header(&token)?;let(stage,base)=transaction.tile(&token)?;
        // Bounded evidence storage is an admission constraint, never a completed
        // optical sample. Exceeding it poisons/discards this pending frame.
        if journal.metadata().map_err(|e|e.to_string())?.len().saturating_sub(frame_log_start)+other_metadata_before>total_metadata_budget {
            transaction.poison(&token)?;
            std::fs::write(out.join("agent_failure_state.json"),format!("{{\"status\":\"JOURNAL_BUDGET_PENDING_REJECT\",\"poisoned\":true,\"native_generation\":{},\"transaction_generation\":{},\"published\":false}}",device.frame_generation,transaction.committed().0)).map_err(|e|e.to_string())?;
            return Err("4MiB journal admission exceeded; pending frame not committed".into());
        }
        let arm=if stage<4{0u32}else{1u32};
        let identity=[64u32,cfg.capacity/64,cfg.interface_budget,arm,cfg.sample as u32,(cfg.sample>>32)as u32,cfg.seed as u32,(cfg.seed>>32)as u32,base as u32,(base>>32)as u32];
        let selector=[1112756529u32,1,cfg.width,cfg.height,0,0,cfg.sample as u32,(cfg.sample>>32)as u32,cfg.seed as u32,(cfg.seed>>32)as u32,base as u32,(base>>32)as u32,cfg.interface_budget,arm,1,cfg.capacity/64];
        let mut once=std::mem::take(&mut uploads);
        once.push(tx::Upload{resource:cfg.command,offset:0,bytes:header.into_iter().flat_map(u32::to_le_bytes).collect()});
        once.push(tx::Upload{resource:cfg.identity,offset:0,bytes:identity.into_iter().flat_map(u32::to_le_bytes).collect()});
        once.push(tx::Upload{resource:cfg.selector,offset:0,bytes:selector.into_iter().flat_map(u32::to_le_bytes).collect()});
        let mut backend=Backend{device,journal,journal_sync_per_frame:cfg.journal_sync_per_frame,measure:&mut measure,selected_readbacks:&selected_readbacks,rb_sources:&rb_sources,bindings:&bindings,header:cfg.header,queue_identity:cfg.identity,queue_selector:cfg.selector,missing_timestamp_ordinal:None,observation:None,observation_dir:out,failure_probe};
        let progress=match transaction.advance(&mut token,once,&mut backend){Ok(p)=>p,Err(e)=>{
            let report=format!("{{\"status\":\"REJECTED_PENDING\",\"transaction_generation\":{},\"native_generation\":{},\"published\":false,\"stage\":{stage},\"tile_base\":{base},\"error\":{:?}}}",transaction.committed().0,device.frame_generation,e);
            std::fs::write(out.join("agent_failure_state.json"),report).map_err(|x|x.to_string())?;return Err(e)
        }};count+=1;
        if device.frame_generation!=committed_before||transaction.committed().0!=committed_before||as_identity(&device)!=as_before{return Err("owner/AS/generation changed before complete two-arm suffix".into())}
        if progress.stage==3||progress.stage==7{tiles[arm as usize]+=1;}
        if progress.stage==2||progress.stage==6{total_physical_active=total_physical_active.checked_add(u64::from(progress.active)).ok_or("active statistics overflow")?;}
        transaction.pause(&token)?;
        if progress.ready{break}
    }
    let expected_tiles=cfg.pixels.div_ceil(u64::from(cfg.capacity));if tiles!=[expected_tiles,expected_tiles]{return Err("not every original pixel tile completed in both arms".into())}
    // The completed suffix has already released these exporters. RR runs in the
    // following validation/publication callback; the caller marks its actual
    // blocking completion afterwards. Do not mark Producer here on the CPU.
    private_bistro_final_release(!device.native.frame.exportable_meta.is_empty(),device.native.external_roundtrip)?;
    if journal.metadata().map_err(|e|e.to_string())?.len().saturating_sub(frame_log_start)+other_metadata_before>total_metadata_budget{transaction.poison(&token)?;return Err("final journal exceeds4MiB; no commit".into())}
    // All completed region rows must be durable before the callback can publish
    // this frame. Keep initialization and separate observation-file syncs intact.
    if cfg.journal_sync_per_frame {journal.sync_data().map_err(|e|e.to_string())?;}
    // SAFETY: every same-owner region fence completed; physical memory and
    // allocation telemetry uses the source-native collector, never guessed values.
    let telemetry=unsafe{super::agent_persistent_telemetry::collect(&device.native,device.passes,&measure,frame_validation_before)}?;
    let report=format!("{{\"status\":\"PASS_GPU_ALL_PIXELS_TWO_ARMS_FULL_HDR\",\"GPU_executed\":true,\"quality_status\":\"NOT_EVALUATED\",\"regions\":{count},\"commits\":1,\"viewport\":[{},{}],\"sample\":{},\"tiles_T\":{},\"tiles_R\":{},\"remaining_after_physical_steps_sum\":{total_physical_active},\"expected_boundary_uploaded\":false}}",cfg.width,cfg.height,cfg.sample,tiles[0],tiles[1]);
    let mut report=report;report.pop();
    report.push_str(&format!(",\"postfence_heap_budget\":{}",postfence_heap_budget_json(&telemetry.heaps)?));
    report.push_str(&format!(",\"timing\":{{\"schema\":\"rurix.hq.optical_owner_timing.v1\",\"gpu_timing_status\":\"{}\",\"scope\":\"owner_entry_through_before_publication\",\"owner_before_publication_ns\":{},\"cpu_record_ns\":{},\"cpu_submit_ns\":{},\"cpu_fence_wait_ns\":{}}}}}",if measure.gpu_timestamps_complete {"available"} else {"unavailable"},elapsed_ns(frame_clock),measure.record_ns,measure.submit_ns,measure.wait_ns));
    let report_bytes=report.len()as u64+1;
    let mut validated=None;
    let done=transaction.finish(&mut token,|readbacks|{let delta_before=journal.metadata().map_err(|e|e.to_string())?.len().saturating_sub(frame_log_start);
        let allowance=total_metadata_budget.checked_sub(delta_before).and_then(|v|v.checked_sub(report_bytes)).ok_or("metadata budget exhausted before publication")?;
        let(value,all_other_metadata)=validate(readbacks,&telemetry,allowance)?;
        let delta=journal.metadata().map_err(|e|e.to_string())?.len().saturating_sub(frame_log_start);
        if delta.checked_add(all_other_metadata).and_then(|v|v.checked_add(report_bytes)).is_none_or(|n|n>total_metadata_budget){return Err("aggregate actual frame metadata budget exceeded; no generation commit".into())}validated=Some(value);Ok(())})?;
    device.frame_generation=done.frame_generation;device.resource_generations.clone_from(&done.resource_generations);device.native.frame.recorded_rb_sources=None;
    if device.frame_generation!=committed_before+1||as_identity(&device)!=as_before{return Err("final AS/commit mismatch".into())}


    *saved=Some(transaction);
    Ok(Complete{readbacks:done.payload,validated:validated.ok_or("HDR validator result absent")?,report,telemetry})

}

struct Backend<'s,'a>{device:&'s mut DeviceFrameSession<'a>,journal:&'s mut std::fs::File,journal_sync_per_frame:bool,measure:&'s mut super::agent_persistent_telemetry::Measure,selected_readbacks:&'s [Readback],rb_sources:&'s [usize],bindings:&'s [Bindings],header:usize,queue_identity:usize,queue_selector:usize,missing_timestamp_ordinal:Option<u64>,observation:Option<(usize,usize)>,observation_dir:&'s std::path::Path,failure_probe:Option<(usize,usize)>}
impl tx::RegionBackend for Backend<'_,'_>{
    fn execute_region(&mut self,_plan:&tx::Plan,call:&tx::Invocation)->Result<tx::RegionOutput,String>{
        hq_setup_failure::check()?;
        let resources=self.device.resources;let passes=self.device.passes;let barriers=self.device.barriers;
        let native=&mut self.device.native;let slot=0;let fence=native.fences[slot];
        let acquire_external=native.external_acquire_needed()?;
        let before=native.validation_errors.load(std::sync::atomic::Ordering::Relaxed);
        let mut scope=hq_region_scope::Scope{range:call.range.clone(),final_suffix:call.region==tx::Region::Suffix&&call.range.end==passes.len(),
            header:self.header,tracker:call.tracker_before[..resources.len()].to_vec(),query_epoch:call.query_epoch,skip_last_timestamp:self.missing_timestamp_ordinal==Some(call.ordinal)};
        let queries=scope.validate(passes.len(),resources.len(),0)?;
        // SAFETY: proposed U32 repeat ownership; prior invocation collected its
        // fence before returning, and this freshly owned session has no FIF user.
        let wait_started=std::time::Instant::now();
        let idle=unsafe{(native.frame.dev.wait_fences)(native.device,1,&fence,1,PERSISTENT_WAIT_TIMEOUT_NS)};
        self.measure.wait_ns+=elapsed_ns(wait_started);
        if idle!=VK_SUCCESS{return Err(hq_setup_failure::quarantine(hq_setup_failure::Phase::OpticalRegionWait,idle,elapsed_ns(wait_started),fence,native.frame.cmd as usize));}
        let record_started=std::time::Instant::now();
        for upload in &call.uploads {
            let RtRes::Buf(buffer)=&native.frame.rt[upload.resource] else{return Err("control resource changed type".into());};
            // SAFETY: primitive admitted exact closed host-visible ranges; this
            // completed fence means no device user overlaps the map/write.
            unsafe{map_write(native,buffer.mem,upload.offset,&upload.bytes)}?;
        }
        // Hash the actual successfully mapped upload bytes. The following
        // submitted row records it; selected journal policy controls durability. A write
        // failure here returns before reset/submit while the fence is complete.
        let identity=call.uploads.iter().find(|u|u.resource==self.queue_identity&&u.offset==0&&u.bytes.len()==40).ok_or("actual queue identity upload absent")?;
        let selector=call.uploads.iter().find(|u|u.resource==self.queue_selector&&u.offset==0&&u.bytes.len()==64).ok_or("actual queue selector upload absent")?;
        writeln!(self.journal,"queue_upload ordinal {} identity_sha256 {} selector_sha256 {}",call.ordinal,rurix_pkg::sha256::hex_digest(&identity.bytes),rurix_pkg::sha256::hex_digest(&selector.bytes)).map_err(|e|e.to_string())?;
        // SAFETY: completed nonpending same-device fence, uniquely owned.
        checked("region reset fence",unsafe{(native.frame.dev.reset_fences)(native.device,1,&fence)})?;
        // SAFETY: same completed fence proves cmd no longer pending; pool supports reset.
        checked("region reset command",unsafe{(native.frame.dev.reset_cmd)(native.frame.cmd,0)})?;
        let begin=CommandBufferBeginInfo{s_type:ST_COMMAND_BUFFER_BEGIN_INFO,p_next:std::ptr::null(),flags:0,p_inheritance_info:std::ptr::null()};
        // SAFETY: reset same-device primary command, stack begin-info live.
        checked("region begin command",unsafe{(native.frame.dev.begin_cmd)(native.frame.cmd,&begin)})?;
        // SAFETY: recording command, bounded original query range allocated at session creation.
        unsafe{(native.frame.dev.cmd_reset_query_pool)(native.frame.cmd,native.frame.cleanup.query_pool,queries.start,queries.end-queries.start)};
        // SAFETY: explicit same-queue device dependency retained across submissions.
        unsafe{cmd_global_barrier2(&native.frame.dev,native.frame.cmd,STAGE2_ALL_COMMANDS,ACCESS2_MEMORY_WRITE,
            STAGE2_ALL_COMMANDS,ACCESS2_MEMORY_READ|ACCESS2_MEMORY_WRITE)};
        let readbacks=if scope.final_suffix{self.selected_readbacks}else{&[]};
        let mut effective_rb=self.rb_sources.iter().map(|&i|native.frame.rb_buffers[i]).collect::<Vec<_>>();
        let exports=if native.external_roundtrip.is_some(){native.frame.exportable_meta.iter().map(|x|x.0).collect::<Vec<_>>()}else{Vec::new()};
        let p=FrameBodyParams{dev:&native.frame.dev,device:native.device,memprops:&native.memprops,cmd:native.frame.cmd,
            resources,rt:&native.frame.rt,passes,barriers,effective_bindings:self.bindings,setups:&native.frame.setups,
            query_pool:native.frame.cleanup.query_pool,query_base:0,inline_vbs:&native.frame.inline_vbs,readbacks,
            record_upload_segment:false,exportable:&exports,external_roundtrip:native.external_roundtrip.is_some(),acquire_external,
            queue_family_index:native.frame.queue_family_index,slot_set_overrides:None};
        // SAFETY: proposed U32 extension; admitted fixed tables/host buffers;
        // recorder checks scope, uses original setups, and retains tracker.
        unsafe{record_frame_body_hq_scoped(&p,&mut effective_rb,None,None,None,None,Some(&mut scope))}?;
        // SAFETY: fully recorded command outside render pass.
        checked("region end command",unsafe{(native.frame.dev.end_cmd)(native.frame.cmd)})?;
        let submit=SubmitInfo{s_type:ST_SUBMIT_INFO,p_next:std::ptr::null(),wait_semaphore_count:0,p_wait_semaphores:std::ptr::null(),
            p_wait_dst_stage_mask:std::ptr::null(),command_buffer_count:1,p_command_buffers:&native.frame.cmd,
            signal_semaphore_count:0,p_signal_semaphores:std::ptr::null()};
        // SAFETY: executable owned command and reset nonpending fence; all handles alive.
        self.measure.record_ns+=elapsed_ns(record_started);
        let submit_started=std::time::Instant::now();
        {let permit=hq_setup_failure::permit()?;let result=unsafe{(native.frame.dev.queue_submit)(native.frame.queue,1,&submit,fence)};
            if result!=VK_SUCCESS{return Err(permit.quarantine(hq_setup_failure::Phase::OpticalRegionSubmit,result,elapsed_ns(submit_started),fence,native.frame.cmd as usize));}}
        self.measure.submit_ns+=elapsed_ns(submit_started);
        // SAFETY: this exact invocation just wrote its range/header; no reuse until collection.
        // A logging failure after a successful submit must not release native
        // resources before collecting this actual fence. Quota admission itself
        // is checked outside this backend, between completed regions.
        let submitted_log_error=writeln!(self.journal,"submitted ordinal {} region {:?}",call.ordinal,call.region)
            .and_then(|_|if self.journal_sync_per_frame {Ok(())} else {self.journal.sync_data()}).err();
        let collected=unsafe{super::agent_persistent_collect::collect(native,resources,&scope,passes.len(),0,slot,before)}?;
        native.complete_external_submission(acquire_external,scope.final_suffix);
        self.measure.wait_ns+=collected.cpu_fence_wait_ns;
        if let Some(timestamps)=&collected.timestamps {
            for &(pass,begin,end)in timestamps{self.measure.gpu_ns[pass]+=end.wrapping_sub(begin)as f64*native.timestamp_period_ns as f64;self.measure.invocations[pass]+=1;}
        }else{self.measure.gpu_timestamps_complete=false;}
        if let Some(error)=submitted_log_error{return Err(format!("journal failed after successful GPU submission and completed fence: {error}"))}
        // Retain the actual error/header words even when this region is rejected.
        if let Some(timestamps)=&collected.timestamps {
            let timestamp_bytes=timestamps.iter().flat_map(|(p,a,b)|[(*p as u64).to_le_bytes(),a.to_le_bytes(),b.to_le_bytes()].concat()).collect::<Vec<_>>();
            writeln!(self.journal,"collected ordinal {} header {:?} query_epoch {} timestamp_count {} timestamp_sha256 {}",
                call.ordinal,collected.header,collected.query_epoch,timestamps.len(),rurix_pkg::sha256::hex_digest(&timestamp_bytes)).map_err(|e|e.to_string())?;
            // Reuse completed stage4 query values; no additional GPU query or fence.
            // Persist beside the existing SHA before the same journal sync.
            if call.stage==4 {
                write!(self.journal,"ticks ordinal {} period_bits {}",call.ordinal,native.timestamp_period_ns.to_bits()).map_err(|e|e.to_string())?;
                for &(pass,begin,end) in timestamps {
                    write!(self.journal," {pass}:{begin}:{end}").map_err(|e|e.to_string())?;
                }
                writeln!(self.journal).map_err(|e|e.to_string())?;
            }
        }else{
            writeln!(self.journal,"collected ordinal {} header {:?} query_epoch {} timestamp_status unavailable",
                call.ordinal,collected.header,collected.query_epoch).map_err(|e|e.to_string())?;
        }
        if !self.journal_sync_per_frame {self.journal.sync_data().map_err(|e|e.to_string())?;}
        // Raw diagnostic capture follows the same completed-fence global HOST_READ
        // dependency. It is retained even if the actual completion header rejects
        // this region; it is never a DeviceFrameOutput or a history publication.
        if call.region==tx::Region::Repeat {if let Some((id,size))=self.observation{
            if !matches!(resources.get(id),Some(ResourceDesc::Buffer(b))if !b.device_local&&size>0&&(size as u64)<=b.size){return Err("direct observation requires in-range host-visible buffer".into())}
            let RtRes::Buf(buffer)=&native.frame.rt[id]else{return Err("observation type drift".into())};
            // SAFETY: artifact repeat ownership; admitted host coherent buffer,
            // exact size, completed fence, global HOST_READ dependency above.
            let bytes=unsafe{map_read(native,buffer.mem,0,size as u64)}?;
            let path=self.observation_dir.join(format!("region_{:06}_observation.bin",call.ordinal));
            let mut file=std::fs::OpenOptions::new().write(true).create_new(true).open(path).map_err(|e|e.to_string())?;
            file.write_all(&bytes).map_err(|e|e.to_string())?;file.sync_data().map_err(|e|e.to_string())?;
            writeln!(self.journal,"observation ordinal {} resource {} bytes {} sha256 {}",call.ordinal,id,size,rurix_pkg::sha256::hex_digest(&bytes)).map_err(|e|e.to_string())?;
            if !self.journal_sync_per_frame {self.journal.sync_data().map_err(|e|e.to_string())?;}
        }}
        // Validate the device error/header BEFORE reading/publishing final payload.
        let active=collected.header[11];
        if collected.header!=call.header(active,0) || active>call.capacity || (scope.final_suffix&&active!=0) {
            if collected.header[13]!=0 {if let Some((id,size))=self.failure_probe {
                let RtRes::Buf(buffer)=&native.frame.rt[id]else{return Err("actual region completion/error header invalid; failed probe type drift".into())};
                // SAFETY(U32): the existing diagnostic-only host-coherent buffer was
                // range-checked when attached; this region fence completed and
                // the recorder emitted its global shader-write/HOST_READ barrier.
                let bytes=unsafe{map_read(native,buffer.mem,0,size as u64)}
                    .map_err(|e|format!("actual region completion/error header invalid; failed probe map: {e}"))?;
                let path=self.observation_dir.join("failed_pixel_probe.bin");
                let mut file=std::fs::OpenOptions::new().write(true).create_new(true).open(path)
                    .map_err(|e|format!("actual region completion/error header invalid; failed probe file: {e}"))?;
                file.write_all(&bytes).and_then(|_|file.sync_data())
                    .map_err(|e|format!("actual region completion/error header invalid; failed probe write: {e}"))?;
                writeln!(self.journal,"failed_pixel_probe ordinal {} resource {} bytes {} header {:?}",call.ordinal,id,size,collected.header)
                    .and_then(|_|self.journal.sync_data()).map_err(|e|e.to_string())?;
            }}
            return Err("actual region completion/error header invalid".into());
        }
        let payload=if scope.final_suffix {
            let mut values=Vec::with_capacity(readbacks.len());
            for (index,r)in readbacks.iter().enumerate(){
                let Readback::Buffer{res,offset,size}=*r else{return Err("HDR buffer-only readback profile".into())};
                let ResourceDesc::Buffer(desc)=&resources[res as usize]else{unreachable!()};
                let(mem,at)=if desc.device_local{let Some((_buffer,mem))=native.frame.rb_buffers[self.rb_sources[index]]else{return Err("actual HDR staging missing".into())};(mem,0)}else{let RtRes::Buf(buffer)=&native.frame.rt[res as usize]else{unreachable!()};(buffer.mem,offset)};
                // SAFETY: source-owned final copies, completed actual fence and
                // HOST_READ dependency; exact selected staging/source range.
                values.push(unsafe{map_read(native,mem,at,size)}?);
            }Some(values)
        }else{None};
        Ok(tx::RegionOutput{header:collected.header,fence_complete:true,validation_errors:0,
            tracker_after:scope.tracker.into_iter().chain(call.tracker_before[resources.len()..].iter().copied()).collect(),timestamps:collected.timestamps.map(|timestamps|timestamps.into_iter().map(|(pass,begin,end)|tx::Timestamp{
                query_epoch:collected.query_epoch,pass,begin,end,written:true}).collect()),final_payload:payload})
    }
    fn journal_region(&mut self,call:&tx::Invocation,out:&tx::RegionOutput)->Result<(),String>{
        let state_bytes=out.tracker_after.iter().flat_map(|(a,b,c)|[u64::from(*a).to_le_bytes(),b.to_le_bytes(),c.to_le_bytes()].concat()).collect::<Vec<_>>();
        writeln!(self.journal,"accepted ordinal {} stage {} tile {} range {:?} tracker_sha256 {}",call.ordinal,call.stage,call.tile_base,call.range,rurix_pkg::sha256::hex_digest(&state_bytes)).map_err(|e|e.to_string())?;
        if self.journal_sync_per_frame {Ok(())} else {self.journal.sync_data().map_err(|e|e.to_string())}
    }
}
fn checked(phase:&str,value:i32)->Result<(),String>{if value==VK_SUCCESS{Ok(())}else{Err(queue_result_error(phase,value))}}

// Proposed U32 extension, staged and not enabled. Callers establish completed
// fence + coherent host visibility + declared range + exclusive ownership.
unsafe fn map_write(n:&NativePersistentFrame,mem:VkDeviceMemory,offset:u64,bytes:&[u8])->Result<(),String>{
    let mut ptr=std::ptr::null_mut();
    // SAFETY: caller-established live host allocation and in-bounds range.
    checked("control map",unsafe{(n.frame.dev.map_mem)(n.device,mem,offset,bytes.len() as u64,0,&mut ptr)})?;
    if ptr.is_null(){
        // SAFETY: balance successful same-allocation map.
        unsafe{(n.frame.dev.unmap_mem)(n.device,mem)};return Err("control map null".into());
    }
    // SAFETY: caller-established disjoint immutable bytes and mapped output extent.
    unsafe{std::ptr::copy_nonoverlapping(bytes.as_ptr(),ptr.cast::<u8>(),bytes.len())};
    // SAFETY: one completed write and one successful map.
    unsafe{(n.frame.dev.unmap_mem)(n.device,mem)};Ok(())
}
unsafe fn map_read(n:&NativePersistentFrame,mem:VkDeviceMemory,offset:u64,size:u64)->Result<Vec<u8>,String>{
    let mut bytes=Vec::new();let len=usize::try_from(size).map_err(|_|"payload too large")?;
    bytes.try_reserve_exact(len).map_err(|e|e.to_string())?;bytes.resize(len,0);
    let mut ptr=std::ptr::null_mut();
    // SAFETY: caller-established synchronized live allocation and declared extent.
    checked("payload map",unsafe{(n.frame.dev.map_mem)(n.device,mem,offset,size,0,&mut ptr)})?;
    if ptr.is_null(){
        // SAFETY: balance successful same-allocation map.
        unsafe{(n.frame.dev.unmap_mem)(n.device,mem)};return Err("payload map null".into());
    }
    // SAFETY: completed suffix, mapped input extent and disjoint allocated host destination.
    unsafe{std::ptr::copy_nonoverlapping(ptr.cast::<u8>(),bytes.as_mut_ptr(),len)};
    // SAFETY: one successful map and completed copy.
    unsafe{(n.frame.dev.unmap_mem)(n.device,mem)};Ok(bytes)
}


// RFC-0052: serialize the existing collect() result, never query the device here.
// This text is formed before report_bytes and the existing aggregate4MiB gate.
fn postfence_heap_budget_json(heaps:&[HeapBudgetTelemetry])->Result<String,String>{
 if heaps.is_empty()||heaps.len()>16||heaps.iter().enumerate().any(|(i,h)|h.heap_index as usize!=i||h.budget_bytes==0||h.driver_usage_bytes>h.budget_bytes){return Err("invalid collected postfence heap budget".into())}
 let rows=heaps.iter().map(|h|format!("{{\"heap_index\":{},\"budget_bytes\":{},\"driver_usage_bytes\":{},\"ledger_bytes\":{}}}",h.heap_index,h.budget_bytes,h.driver_usage_bytes,h.ledger_bytes)).collect::<Vec<_>>().join(",");
 Ok(format!("{{\"schema\":\"rurix.hq.postfence_heap_budget.v1\",\"source\":\"VK_EXT_memory_budget_and_native_allocation_ledger\",\"query_point\":\"after_completed_region_fences_and_host_readback_before_publication\",\"is_peak_measurement\":false,\"heaps\":[{rows}]}}"))
}
#[cfg(test)]mod postfence_heap_budget_tests{
 use super::*;
 #[test]fn actual_collected_values_and_observation_scope_are_preserved(){
  let heaps=[HeapBudgetTelemetry{heap_index:0,budget_bytes:7477395456,driver_usage_bytes:7234567890,ledger_bytes:6997867568},HeapBudgetTelemetry{heap_index:1,budget_bytes:16876498944,driver_usage_bytes:898233296,ledger_bytes:796262400}];
  let json=postfence_heap_budget_json(&heaps).unwrap();
  assert!(json.contains("\"budget_bytes\":7477395456,\"driver_usage_bytes\":7234567890,\"ledger_bytes\":6997867568"));
  assert!(json.contains("\"heap_index\":1,\"budget_bytes\":16876498944,\"driver_usage_bytes\":898233296,\"ledger_bytes\":796262400"));
  assert!(json.contains("after_completed_region_fences_and_host_readback_before_publication")&&json.contains("\"is_peak_measurement\":false"));
 }
 #[test]fn invalid_collected_budget_or_census_rejects(){
  assert!(postfence_heap_budget_json(&[]).is_err());
  for (index,budget,usage) in [(1,100,1),(0,0,0),(0,100,101)]{assert!(postfence_heap_budget_json(&[HeapBudgetTelemetry{heap_index:index,budget_bytes:budget,driver_usage_bytes:usage,ledger_bytes:1}]).is_err());}
 }
}

#[cfg(test)]
mod private_bistro_export_tests {
    use super::*;
    fn resources()->Vec<ResourceDesc<'static>>{
        vec![ResourceDesc::Buffer(BufferDesc{size:64,usage:BufferUsage{storage:true,..Default::default()},data:None,device_local:false}),
            ResourceDesc::Buffer(BufferDesc{size:256,usage:BufferUsage{storage:true,..Default::default()},data:None,device_local:true})]
    }
    #[test]
    fn explicit_profile_requires_fresh_producer_and_real_consumer_return(){
        let resources=resources();let exports=[(1,512,0)];
        assert!(admit_private_bistro_buffers(&resources,&exports,&[],Some(HqExternalPhase::Producer),0).is_ok());
        assert!(admit_private_bistro_buffers(&resources,&exports,&[],None,0).is_err());
        assert!(admit_private_bistro_buffers(&resources,&exports,&[],Some(HqExternalPhase::Released),0).is_err());
        assert!(admit_private_bistro_buffers(&resources,&exports,&[],Some(HqExternalPhase::ConsumerComplete),0).is_err());
        assert!(admit_private_bistro_buffers(&resources,&exports,&[],Some(HqExternalPhase::Producer),1).is_err());
        assert!(admit_private_bistro_buffers(&resources,&exports,&[],Some(HqExternalPhase::Released),1).is_err());
        assert!(admit_private_bistro_buffers(&resources,&exports,&[],Some(HqExternalPhase::ConsumerComplete),1).is_ok());
    }
    #[test]
    fn two_frame_region_cycle_consumes_acquire_once_and_releases_only_at_suffix(){
        let resources=resources();let exports=[(1,512,0)];let mut phase=HqExternalPhase::Producer;
        for generation in 0..2 {
            assert!(admit_private_bistro_buffers(&resources,&exports,&[],Some(phase),generation).is_ok());
            let acquire=phase.acquire_needed().unwrap();assert_eq!(acquire,generation!=0);
            // Mirror the backend's update only after the real collected prefix.
            phase=phase.completed(acquire,false);assert_eq!(phase,HqExternalPhase::Producer);
            for _ in 0..3 {assert!(!phase.acquire_needed().unwrap());phase=phase.completed(false,false);assert_eq!(phase,HqExternalPhase::Producer);}
            assert!(private_bistro_final_release(true,Some(phase)).is_err());
            phase=phase.completed(false,true);assert!(private_bistro_final_release(true,Some(phase)).is_ok());
            assert!(admit_private_bistro_buffers(&resources,&exports,&[],Some(phase),generation+1).is_err());
            phase=phase.consumer_complete().unwrap();assert_eq!(phase,HqExternalPhase::ConsumerComplete);
            assert!(phase.consumer_complete().is_err());
        }
    }
    #[test]
    fn native_none_profile_stays_valid_but_imports_and_inconsistent_exports_reject(){
        let resources=resources();
        for generation in [0,1,42] {assert!(admit_private_bistro_buffers(&resources,&[],&[],None,generation).is_ok());}
        assert!(private_bistro_final_release(false,None).is_ok());
        assert!(private_bistro_final_release(false,Some(HqExternalPhase::Released)).is_err());
        assert!(admit_private_bistro_buffers(&resources,&[],&[0],None,0).is_err());
        assert!(admit_private_bistro_buffers(&resources,&[],&[],Some(HqExternalPhase::Producer),0).is_err());
    }
    #[test]
    fn actual_export_metadata_requires_unique_uninitialized_device_local_buffers(){
        let mut resources=resources();let phase=Some(HqExternalPhase::Producer);
        for exports in [vec![(9,512,0)],vec![(1,512,0),(1,512,0)],vec![(1,255,0)],vec![(0,512,0)]]{
            assert!(admit_private_bistro_buffers(&resources,&exports,&[],phase,0).is_err());
        }
        const ZERO:[u8;256]=[0;256];
        let ResourceDesc::Buffer(buffer)=&mut resources[1] else{unreachable!()};buffer.data=Some(&ZERO);
        assert!(admit_private_bistro_buffers(&resources,&[(1,512,0)],&[],phase,0).is_err());
        resources[1]=ResourceDesc::Texture(TextureDesc{width:1,height:1,format:TexFormat::Rgba8Unorm,
            usage:TextureUsage{sampled:true,..Default::default()},data:None});
        assert!(admit_private_bistro_buffers(&resources,&[(1,512,0)],&[],phase,0).is_err());
        assert!(admit_private_bistro_buffers(&resources,&[],&[],None,0).is_err());
    }
}
