//! Artifact-only private HQ pending logical-frame primitive. No Vulkan adapter.
//! Full RFC identifier is deliberately unallocated until production admission.
//! Safe Rust; default runtime APIs are not modified by this file.
//! Assisted-by: Codex:GPT-6.
#![forbid(unsafe_code)]

use std::collections::BTreeSet;
use std::ops::Range;
use std::sync::atomic::{AtomicU64, Ordering};

pub type Result<T> = std::result::Result<T, String>;
pub type TrackedState = (u32, u64, u64);
// Same masks as render_exec.rs; scope is buffer/compute plus host header.
pub const HOST_WRITE: TrackedState = (0, 0x4000, 0x4000);
pub const HEADER_HOST_READ: TrackedState = (0, 0x4000, 0x2000);
pub const MAGIC: u32 = 0x484c4631;
pub const RECENT_REGIONS: usize = 16;
pub const MAX_REGION_PASSES: usize = 256;
static OWNER: AtomicU64 = AtomicU64::new(1);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Region { Prefix, Repeat, Suffix }
impl Region { fn tag(self) -> u32 { match self {Self::Prefix=>0,Self::Repeat=>1,Self::Suffix=>2} } }

#[derive(Clone, Debug)]
pub struct Resource {
    pub allocation: u64, pub bytes: u64, pub host_visible: bool,
    pub generation: u64, pub tracked: TrackedState,
}
#[derive(Clone, Debug)]
pub struct Access {pub resource: usize, pub writes: bool, pub target: TrackedState}
#[derive(Clone, Debug)]
pub struct Pass {pub name: String, pub barriers: Vec<(usize,TrackedState)>, pub accesses: Vec<Access>}
#[derive(Clone, Debug)]
pub struct ControlRange {pub resource: usize, pub bytes: Range<u64>}
#[derive(Clone, Debug)]
pub struct Plan {
    pub passes:Vec<Pass>,pub resources:Vec<Resource>,pub stages:Vec<Vec<Range<usize>>>,pub pixels:u64,pub capacity:u32,
    pub final_readback_states: Vec<(usize,TrackedState)>,
    pub header_resource: usize, pub controls: Vec<ControlRange>,
    pub histories: BTreeSet<usize>,
}
impl Plan {
    pub fn validate(&self) -> Result<()> {
        if !matches!(self.capacity,4096|65536) || self.pixels==0 || self.stages.len()!=9 || self.resources.is_empty(){return Err("closed two-arm tiled plan".into())}
        let mut cursor=0usize;
        for (stage,parts)in self.stages.iter().enumerate(){
            if parts.is_empty() || ((stage==2||stage==6)&&parts.len()!=1){return Err("stage parts absent/repeated stage split".into())}
            for part in parts{if part.start!=cursor||part.start>=part.end||part.len()>MAX_REGION_PASSES{return Err("stage pass gap/overlap/oversize".into())}cursor=part.end;}
        }
        if cursor!=self.passes.len(){return Err("full physical schedule not covered".into())}
        let mut readback_ids=BTreeSet::new();
        for (id,_) in &self.final_readback_states {
            if *id>=self.resources.len()||self.resources[*id].host_visible||!readback_ids.insert(*id){return Err("invalid or duplicated staged readback source state".into())}
        }
        if self.header_resource>=self.resources.len() || self.resources[self.header_resource].bytes<64 || !self.resources[self.header_resource].host_visible {
            return Err("internal64-byte host-visible header absent or too small".into());
        }
        if self.resources.iter().any(|r|r.bytes==0 || r.allocation==0) {return Err("invalid resource declaration".into());}
        for (pi,p) in self.passes.iter().enumerate() {
            if p.name.is_empty() || p.accesses.is_empty() {return Err("unnamed/empty pass".into());}
            if p.barriers.iter().any(|(id,_)|*id>=self.resources.len()){return Err("explicit barrier resource outside actual table".into());}
            let mut seen=BTreeSet::new();
            for a in &p.accesses {
                if a.resource>=self.resources.len() || !seen.insert(a.resource) {return Err("invalid/duplicate pass resource".into());}
                if a.writes && self.histories.contains(&a.resource) && !self.stages[8].iter().any(|r|r.contains(&pi)) {
                    return Err("history write outside final suffix".into());
                }
            }
        }
        // Conservative initial admission: every repeated-region resource has
        // one allocation identity in the entire fixed resource table.
        let repeated=[2usize,6usize].into_iter().flat_map(|stage|self.passes[self.stages[stage][0].clone()].iter()).flat_map(|p|p.accesses.iter().map(|a|a.resource)).collect::<BTreeSet<_>>();
        for id in repeated {
            if self.resources.iter().filter(|r|r.allocation==self.resources[id].allocation).count()!=1 {
                return Err("repeated-region resource aliases another logical resource".into());
            }
        }
        for &id in &self.histories {if id>=self.resources.len(){return Err("invalid history resource".into());}}
        for c in &self.controls {
            if c.resource>=self.resources.len() || !self.resources[c.resource].host_visible
                || c.bytes.start>=c.bytes.end || c.bytes.end>self.resources[c.resource].bytes
                || self.histories.contains(&c.resource) || c.resource==self.header_resource {
                return Err("invalid mutable control range".into());
            }
            if self.resources.iter().filter(|r|r.allocation==self.resources[c.resource].allocation).count()!=1 {
                return Err("control allocation is aliased".into());
            }
        }
        for r in self.stages.iter().flat_map(|p|p.iter()) {
            if !self.passes[r.end-1].accesses.iter().any(|a|a.resource==self.header_resource && a.writes) {
                return Err("every region must end with actual completion-header producer".into());
            }
        }
        Ok(())
    }

}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Snapshot {pub sample: u64, pub sampling_seed: u64, pub immutable_inputs_sha256: [u8;32]}
#[derive(Debug)]
pub struct PendingToken {owner:u64, epoch:u64}
#[derive(Clone, Debug)]
pub struct Upload {pub resource:usize,pub offset:u64,pub bytes:Vec<u8>}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ResourceVersion {pub resource:usize,pub consumed:u64,pub produced:Option<u64>}
#[derive(Clone, Debug)]
pub struct Invocation {
    pub owner:u64,pub epoch:u64,pub ordinal:u64,pub region:Region,pub range:Range<usize>,pub stage:usize,pub tile_base:u64,
    pub snapshot:Snapshot,pub capacity:u32,pub previous_active:Option<u32>,
    pub query_epoch:u64,pub uploads:Vec<Upload>,pub pass_versions:Vec<Vec<ResourceVersion>>,
    pub tracker_before:Vec<TrackedState>,pub expected_tracker_after:Vec<TrackedState>,
}
impl Invocation {
    pub fn header(&self,active:u32,error:u32)->[u32;16]{
        [MAGIC,1,self.owner as u32,(self.owner>>32) as u32,self.epoch as u32,(self.epoch>>32) as u32,
         self.snapshot.sample as u32,(self.snapshot.sample>>32) as u32,self.ordinal as u32,(self.ordinal>>32) as u32,
         self.region.tag(),active,self.capacity,error,(self.range.end-self.range.start) as u32,self.stage as u32]
    }
}
#[derive(Clone, Debug)]
pub struct Timestamp {pub query_epoch:u64,pub pass:usize,pub begin:u64,pub end:u64,pub written:bool}
#[derive(Clone, Debug)]
pub struct RegionOutput {
    pub header:[u32;16],pub fence_complete:bool,pub validation_errors:u64,
    pub tracker_after:Vec<TrackedState>,pub timestamps:Option<Vec<Timestamp>>,
    pub final_payload:Option<Vec<Vec<u8>>>,
}
/// A native adapter must record only invocation.range against the original
/// full pass/pipeline/descriptor tables; collect these queries BEFORE reset;
/// perform internal header transfer/host visibility; and never call public
/// execute/collect or commit runtime/history generations.
pub trait RegionBackend {
    fn execute_region(&mut self,plan:&Plan,call:&Invocation)->Result<RegionOutput>;
    /// Persist a complete region witness before allowing the next region or
    /// publishing. A bounded stream/hash chain replaces unbounded RAM history.
    fn journal_region(&mut self,call:&Invocation,output:&RegionOutput)->Result<()>;
}

#[derive(Clone, Debug)]
pub struct Progress {pub ordinal:u64,pub region:Region,pub active:u32,pub ready:bool,pub stage:usize,pub tile_base:u64}
#[derive(Debug)]
pub struct Completed {
    pub snapshot:Snapshot,pub frame_generation:u64,pub resource_generations:Vec<u64>,
    pub payload:Vec<Vec<u8>>,pub journaled_regions:u64,pub invocations:Vec<Invocation>,pub timestamps:Vec<Option<Vec<Timestamp>>>,
}
#[derive(Debug)]
struct Pending {
    snapshot:Snapshot,epoch:u64,next:Option<Region>,ordinal:u64,active:Option<u32>,stage:usize,part:usize,tile_base:u64,repeat_round:u32,
    generations:Vec<u64>,tracker:Vec<TrackedState>,payload:Option<Vec<Vec<u8>>>,
    invocations:Vec<Invocation>,timestamps:Vec<Option<Vec<Timestamp>>>,
}
#[derive(Debug)]
pub struct Session {
    owner:u64,plan:Plan,committed_frame:u64,committed_generations:Vec<u64>,
    committed_tracker:Vec<TrackedState>,pending:Option<Pending>,poisoned:bool,next_query_epoch:u64,
}
impl Session {
    pub fn new(plan:Plan,committed_frame:u64)->Result<Self>{
        plan.validate()?;
        let owner=OWNER.fetch_update(Ordering::Relaxed,Ordering::Relaxed,|v|v.checked_add(1)).map_err(|_|"owner id overflow")?;
        let committed_generations=plan.resources.iter().map(|r|r.generation).collect();
        let committed_tracker=plan.resources.iter().map(|r|r.tracked).collect();
        Ok(Self{owner,plan,committed_frame,committed_generations,committed_tracker,pending:None,poisoned:false,next_query_epoch:1})
    }
    /// Every legacy execute, submit/FIF, collection, AS mutation, and policy
    /// mutation entry point must call this guard in a future native adapter.
    pub fn ensure_legacy_idle(&self)->Result<()>{
        if self.poisoned {Err("pending HQ session poisoned; discard required".into())}
        else if self.pending.is_some(){Err("logical HQ frame is pending even if no fence slot is busy".into())}else{Ok(())}
    }
    pub fn begin(&mut self,snapshot:Snapshot)->Result<PendingToken>{
        self.ensure_legacy_idle()?;
        let epoch=self.committed_frame.checked_add(1).ok_or("frame generation overflow")?;
        self.pending=Some(Pending{snapshot,epoch,next:Some(Region::Prefix),ordinal:0,active:None,stage:0,part:0,tile_base:0,repeat_round:0,
            generations:self.committed_generations.clone(),tracker:self.committed_tracker.clone(),payload:None,
            invocations:vec![],timestamps:vec![]});
        Ok(PendingToken{owner:self.owner,epoch})
    }
    fn check(&self,t:&PendingToken)->Result<()>{
        if self.poisoned{return Err("poisoned logical frame".into());}
        match &self.pending {Some(p) if t.owner==self.owner&&t.epoch==p.epoch=>Ok(()),_=>Err("foreign/stale/missing pending token".into())}
    }
    pub fn pause(&self,t:&PendingToken)->Result<()>{self.check(t)}
    /// Pure preview for the source-bound command upload; no invocation is run.
    pub fn next_header(&self,t:&PendingToken)->Result<[u32;16]>{
        let (call,_)=self.prepare(t,vec![])?;Ok(call.header(0,0))
    }
    pub fn tile(&self,t:&PendingToken)->Result<(usize,u64)>{self.check(t)?;let p=self.pending.as_ref().unwrap();Ok((p.stage,p.tile_base))}
    pub fn committed(&self)->(u64,&[u64]){(self.committed_frame,&self.committed_generations)}
    pub fn committed_tracker(&self)->&[TrackedState]{&self.committed_tracker}
    /// Rebind only while idle. Histories/parity and selected readbacks can vary
    /// per frame, but allocation identity, committed state and generations cannot.
    pub fn reconfigure_idle(&mut self,plan:Plan)->Result<()>{
        self.ensure_legacy_idle()?;plan.validate()?;
        if plan.resources.len()!=self.plan.resources.len()||plan.capacity!=self.plan.capacity||plan.pixels!=self.plan.pixels||plan.stages!=self.plan.stages||plan.header_resource!=self.plan.header_resource{return Err("persistent graph shape changed".into())}
        for(i,(old,new))in self.plan.resources.iter().zip(&plan.resources).enumerate(){
            if old.allocation!=new.allocation||old.bytes!=new.bytes||old.host_visible!=new.host_visible||new.generation!=self.committed_generations[i]||new.tracked!=self.committed_tracker[i]{return Err("persistent allocation/actual committed state drift".into())}
        }
        if self.plan.passes.len()!=plan.passes.len()||self.plan.passes.iter().zip(&plan.passes).any(|(a,b)|a.name!=b.name){return Err("persistent pass identity changed".into())}
        self.plan=plan;Ok(())
    }

    pub fn poison(&mut self,t:&PendingToken)->Result<()>{self.check(t)?;self.poisoned=true;Ok(())}

    fn prepare(&self,t:&PendingToken,uploads:Vec<Upload>)->Result<(Invocation,Vec<u64>)>{
        self.check(t)?;let p=self.pending.as_ref().unwrap();
        p.ordinal.checked_add(1).ok_or("region ordinal overflow")?;
        self.next_query_epoch.checked_add(1).ok_or("query epoch overflow")?;
        let region=p.next.ok_or("logical frame ready; finalize instead of advancing")?;
        let range=self.plan.stages[p.stage][p.part].clone();
        if region==Region::Repeat&&p.repeat_round>=2048{return Err("per-tile diagnostic watchdog; active rays remain unresolved".into())}

        let mut generations=p.generations.clone();let mut tracker=p.tracker.clone();let mut uploaded=BTreeSet::new();
        for u in &uploads {
            let end=u.offset.checked_add(u.bytes.len() as u64).ok_or("upload size overflow")?;
            if u.bytes.is_empty() || !uploaded.insert(u.resource) || !self.plan.controls.iter().any(|c|
                c.resource==u.resource && u.offset>=c.bytes.start && end<=c.bytes.end) {
                return Err("upload outside closed unaliased host-control ranges".into());
            }
            generations[u.resource]=generations[u.resource].checked_add(1).ok_or("upload generation overflow")?;
            tracker[u.resource]=HOST_WRITE;
        }
        let tracker_before=tracker.clone();let mut pass_versions=vec![];
        for pass in &self.plan.passes[range.clone()] {
            for &(id,target) in &pass.barriers {tracker[id]=target;}
            let mut versions=vec![];
            for a in &pass.accesses {
                let consumed=generations[a.resource];
                let produced=if a.writes {let next=consumed.checked_add(1).ok_or("resource generation overflow")?;generations[a.resource]=next;Some(next)}else{None};
                versions.push(ResourceVersion{resource:a.resource,consumed,produced});tracker[a.resource]=a.target;
            }
            pass_versions.push(versions);
        }
        if p.stage==8&&range.end==self.plan.passes.len() {
            for &(id,state) in &self.plan.final_readback_states {tracker[id]=state;}
        }
        tracker[self.plan.header_resource]=HEADER_HOST_READ;
        Ok((Invocation{owner:self.owner,epoch:p.epoch,ordinal:p.ordinal,region,range,stage:p.stage,tile_base:p.tile_base,snapshot:p.snapshot.clone(),
            capacity:self.plan.capacity,previous_active:p.active,query_epoch:self.next_query_epoch,
            uploads,pass_versions,tracker_before,expected_tracker_after:tracker},generations))
    }
    fn validate_output(&self,c:&Invocation,o:&RegionOutput)->Result<u32>{
        if !o.fence_complete || o.validation_errors!=0{return Err("region fence/validation failure".into());}
        let active=o.header[11];
        if o.header!=c.header(active,0) || active>c.capacity{return Err("stale/malformed/error completion header".into());}
        if c.region==Region::Repeat && active>c.previous_active.unwrap_or(0){return Err("active path count increased".into());}
        if (c.stage==3||c.stage==7||c.stage==8) && active!=0{return Err("suffix has unresolved paths".into());}
        if o.tracker_after!=c.expected_tracker_after{return Err("resource state tracker not preserved".into());}
        // None explicitly means unavailable optional timing, never completion.
        // Present timing must retain the complete original epoch/order evidence.
        if let Some(timestamps)=&o.timestamps {
            if timestamps.len()!=c.range.len() || timestamps.iter().zip(c.range.clone()).any(|(t,pass)|
                !t.written || t.query_epoch!=c.query_epoch || t.pass!=pass) {return Err("missing/stale/reordered region timestamps".into());}
        }
        if (c.stage==8&&c.range.end==self.plan.passes.len())!=o.final_payload.is_some(){return Err("final payload before suffix or absent at suffix".into());}
        Ok(active)
    }
    pub fn advance<B:RegionBackend>(&mut self,t:&mut PendingToken,uploads:Vec<Upload>,backend:&mut B)->Result<Progress>{
        // Validation before any native operation leaves the same pending token
        // resumable. Errors after execute begins poison the device session.
        let (call,generations)=self.prepare(t,uploads)?;
        let output=match backend.execute_region(&self.plan,&call){Ok(v)=>v,Err(e)=>{self.poisoned=true;return Err(e)}};
        let active=match self.validate_output(&call,&output){Ok(v)=>v,Err(e)=>{self.poisoned=true;return Err(e)}};
        if let Err(e)=backend.journal_region(&call,&output){self.poisoned=true;return Err(e)}
        let p=self.pending.as_mut().unwrap();
        p.generations=generations;p.tracker=output.tracker_after;p.active=Some(active);
        if p.part+1<self.plan.stages[p.stage].len(){p.part+=1;}
        else {p.part=0;match p.stage {
            0=>{p.stage=1;p.tile_base=0;},
            1|5=>{p.repeat_round=0;p.stage+=if active>0{1}else{2};},
            2|6=>{p.repeat_round+=1;if active==0{p.stage+=1;}},
            3|7=>{let next=p.tile_base.checked_add(u64::from(self.plan.capacity)).ok_or("tile overflow")?;
                if next<self.plan.pixels{p.tile_base=next;p.stage-=2;}else{p.tile_base=0;p.stage+=1;}},
            4=>{p.stage=5;p.tile_base=0;},
            8=>{p.next=None;},_=>unreachable!()
        }}
        if p.next.is_some(){p.next=Some(if p.stage==2||p.stage==6{Region::Repeat}else if p.stage==8{Region::Suffix}else{Region::Prefix});}

        p.payload=output.final_payload;p.timestamps.push(output.timestamps);p.invocations.push(call.clone());
        if p.invocations.len()>RECENT_REGIONS{p.invocations.remove(0);p.timestamps.remove(0);}
        p.ordinal=match p.ordinal.checked_add(1){Some(v)=>v,None=>{self.poisoned=true;return Err("region ordinal overflow".into())}};
        self.next_query_epoch+=1;
        Ok(Progress{ordinal:call.ordinal,region:call.region,active,ready:p.next.is_none(),stage:call.stage,tile_base:call.tile_base})
    }
    /// CPU/device attachment validation completes before the only public
    /// generation transition. A validation failure leaves no Completed value.
    pub fn finish<F>(&mut self,t:&mut PendingToken,validate:F)->Result<Completed>
    where F:FnOnce(&[Vec<u8>])->Result<()> {
        self.check(t)?;
        let p=self.pending.as_ref().unwrap();
        if p.next.is_some() || p.active!=Some(0){return Err("cannot finish unresolved logical frame".into());}
        if let Err(e)=validate(p.payload.as_deref().ok_or("missing suffix payload")?){self.poisoned=true;return Err(e)}
        let p=self.pending.take().unwrap();self.committed_frame=p.epoch;
        self.committed_generations=p.generations.clone();self.committed_tracker=p.tracker;
        Ok(Completed{snapshot:p.snapshot,frame_generation:p.epoch,resource_generations:p.generations,
            payload:p.payload.unwrap(),journaled_regions:p.ordinal,invocations:p.invocations,timestamps:p.timestamps})
    }
}
