#[path="agent_persistent_transaction.rs"]mod tx;
use tx::*;
fn plan(pixels:u64)->Plan{
 let resources=vec![Resource{allocation:1,bytes:64,host_visible:true,generation:0,tracked:(0,0,0)},Resource{allocation:2,bytes:64,host_visible:false,generation:0,tracked:(0,0,0)}];
 let passes=(0..9).map(|i|Pass{name:format!("p{i}"),barriers:vec![(1,(0,0x800,0x20))],accesses:vec![Access{resource:0,writes:true,target:HEADER_HOST_READ}]}).collect();
 Plan{resources,passes,stages:(0..9).map(|i|vec![i..i+1]).collect(),pixels,capacity:4096,final_readback_states:vec![(1,(0,0x1000,0x800))],header_resource:0,controls:vec![],histories:Default::default()}
}
struct Backend{active:bool,fail:Option<usize>,calls:Vec<(usize,u64)>}
impl RegionBackend for Backend{
 fn execute_region(&mut self,_:&Plan,c:&Invocation)->Result<RegionOutput>{self.calls.push((c.stage,c.tile_base));let active=if self.active&&(c.stage==1||c.stage==5){3}else{0};let mut tracker=c.expected_tracker_after.clone();if self.fail==Some(c.stage){tracker[1]=(9,9,9);}
 Ok(RegionOutput{header:c.header(active,0),fence_complete:true,validation_errors:0,tracker_after:tracker,timestamps:Some(vec![Timestamp{query_epoch:c.query_epoch,pass:c.range.start,begin:1,end:2,written:true}]),final_payload:if c.stage==8{Some(vec![vec![1]])}else{None}})}
 fn journal_region(&mut self,_:&Invocation,_:&RegionOutput)->Result<()>{Ok(())}
}
fn start(pixels:u64)->(Session,PendingToken){let mut s=Session::new(plan(pixels),0).unwrap();let t=s.begin(Snapshot{sample:2105,sampling_seed:7312026601,immutable_inputs_sha256:[0;32]}).unwrap();(s,t)}
#[test]fn two_active_arms_and_partial_last_tile_exact_coverage(){let(mut s,mut t)=start(4097);let mut b=Backend{active:true,fail:None,calls:vec![]};loop{let p=s.advance(&mut t,vec![],&mut b).unwrap();assert_eq!(s.committed().0,0);if p.ready{break}}let c=s.finish(&mut t,|_|Ok(())).unwrap();assert_eq!(c.frame_generation,1);assert_eq!(b.calls,vec![(0,0),(1,0),(2,0),(3,0),(1,4096),(2,4096),(3,4096),(4,0),(5,0),(6,0),(7,0),(5,4096),(6,4096),(7,4096),(8,0)]);assert!(s.finish(&mut t,|_|Ok(())).is_err());}
#[test]fn zero_active_still_covers_every_tile_without_repeat(){let(mut s,mut t)=start(8193);let mut b=Backend{active:false,fail:None,calls:vec![]};loop{if s.advance(&mut t,vec![],&mut b).unwrap().ready{break}}assert!(!b.calls.iter().any(|(stage,_)|*stage==2||*stage==6));assert_eq!(b.calls.iter().filter(|(x,_)|*x==3).count(),3);assert_eq!(b.calls.iter().filter(|(x,_)|*x==7).count(),3);assert_eq!(s.finish(&mut t,|_|Ok(())).unwrap().frame_generation,1);}
#[test]fn failure_at_each_stage_keeps_generation_zero_and_poisoned(){for stage in 0..9{let(mut s,mut t)=start(1);let mut b=Backend{active:true,fail:Some(stage),calls:vec![]};loop{if s.advance(&mut t,vec![],&mut b).is_err(){break}}assert_eq!(s.committed().0,0);assert!(s.next_header(&t).unwrap_err().contains("poison"));assert!(s.finish(&mut t,|_|Ok(())).is_err());}}
#[test]fn final_cpu_readback_validation_failure_no_commit(){let(mut s,mut t)=start(1);let mut b=Backend{active:false,fail:None,calls:vec![]};loop{if s.advance(&mut t,vec![],&mut b).unwrap().ready{break}}assert!(s.finish(&mut t,|_|Err("injected payload failure".into())).is_err());assert_eq!(s.committed().0,0);}
#[test]fn bad_stage_order_overlap_and_capacity_reject(){for kind in 0..3{let mut p=plan(1);match kind{0=>p.stages[3][0]=2..4,1=>p.capacity=4095,_=>p.pixels=0};assert!(Session::new(p,0).is_err());}}
#[test]fn convergence_on_last_admitted_repeat_can_write_and_commit(){let(mut s,mut t)=start(1);struct Last{counts:[u32;2]};impl RegionBackend for Last{fn execute_region(&mut self,_:&Plan,c:&Invocation)->Result<RegionOutput>{let active=if c.stage==1||c.stage==5{1}else if c.stage==2||c.stage==6{let arm=usize::from(c.stage==6);self.counts[arm]+=1;u32::from(self.counts[arm]<2048)}else{0};Ok(RegionOutput{header:c.header(active,0),fence_complete:true,validation_errors:0,tracker_after:c.expected_tracker_after.clone(),timestamps:Some(vec![Timestamp{query_epoch:c.query_epoch,pass:c.range.start,begin:1,end:2,written:true}]),final_payload:if c.stage==8{Some(vec![vec![1]])}else{None}})}fn journal_region(&mut self,_:&Invocation,_:&RegionOutput)->Result<()>{Ok(())}}let mut b=Last{counts:[0;2]};loop{if s.advance(&mut t,vec![],&mut b).unwrap().ready{break}}assert_eq!(b.counts,[2048,2048]);assert_eq!(s.finish(&mut t,|_|Ok(())).unwrap().frame_generation,1);}

#[test]fn actual_committed_tracker_and_generation_reused_across_two_frames(){let(mut s,mut t)=start(4097);let mut b=Backend{active:true,fail:None,calls:vec![]};loop{if s.advance(&mut t,vec![],&mut b).unwrap().ready{break}}let c=s.finish(&mut t,|_|Ok(())).unwrap();assert_eq!(c.frame_generation,1);
let mut p=plan(4097);for(i,r)in p.resources.iter_mut().enumerate(){r.generation=s.committed().1[i];r.tracked=s.committed_tracker()[i];}s.reconfigure_idle(p).unwrap();let mut t=s.begin(Snapshot{sample:2106,sampling_seed:7312026601,immutable_inputs_sha256:[1;32]}).unwrap();loop{if s.advance(&mut t,vec![],&mut b).unwrap().ready{break}}assert_eq!(s.finish(&mut t,|_|Ok(())).unwrap().frame_generation,2);}
#[test]fn stale_tracker_or_generation_on_next_frame_rejected(){let(mut s,mut t)=start(1);let mut b=Backend{active:false,fail:None,calls:vec![]};loop{if s.advance(&mut t,vec![],&mut b).unwrap().ready{break}}s.finish(&mut t,|_|Ok(())).unwrap();assert!(s.reconfigure_idle(plan(1)).is_err());}
#[test]fn refresh_while_pending_rejected(){let(mut s,_t)=start(1);assert!(s.reconfigure_idle(plan(1)).is_err());}

#[test]fn closed_dual_capacity_and_partial_last_tile_coverage(){
 for cap in [4096u32,65536]{for pixels in [1,u64::from(cap)-1,u64::from(cap),u64::from(cap)+1]{
  let mut p=plan(pixels);p.capacity=cap;let mut s=Session::new(p,0).unwrap();let mut t=s.begin(Snapshot{sample:7,sampling_seed:11,immutable_inputs_sha256:[0;32]}).unwrap();let mut b=Backend{active:true,fail:None,calls:vec![]};
  loop{if s.advance(&mut t,vec![],&mut b).unwrap().ready{break}}
  let expected=(0..pixels.div_ceil(u64::from(cap))).map(|i|i*u64::from(cap)).collect::<Vec<_>>();
  for stage in [1usize,3,5,7]{assert_eq!(b.calls.iter().filter_map(|(s,b)|(*s==stage).then_some(*b)).collect::<Vec<_>>(),expected);}
  assert_eq!(s.finish(&mut t,|_|Ok(())).unwrap().frame_generation,1);
 }}
 for cap in [0,1,4095,4097,16384,65535,65537,u32::MAX]{let mut p=plan(1);p.capacity=cap;assert!(Session::new(p,0).is_err());}
}
#[test]fn capacity_cannot_change_between_successful_frames(){
 let(mut s,mut t)=start(1);let mut b=Backend{active:false,fail:None,calls:vec![]};loop{if s.advance(&mut t,vec![],&mut b).unwrap().ready{break}}s.finish(&mut t,|_|Ok(())).unwrap();
 let mut p=plan(1);p.capacity=65536;for(i,r)in p.resources.iter_mut().enumerate(){r.generation=s.committed().1[i];r.tracked=s.committed_tracker()[i];}assert!(s.reconfigure_idle(p).is_err());
}

// Optional timing cannot authorize completion; all real execution gates remain.
struct TimingBackend{inner:Backend,mode:u8}
impl RegionBackend for TimingBackend{
 fn execute_region(&mut self,p:&Plan,c:&Invocation)->Result<RegionOutput>{
  let mut o=self.inner.execute_region(p,c)?;
  if self.mode<=6{o.timestamps=None;}
  match self.mode{
   1=>o.fence_complete=false,
   2=>o.validation_errors=1,
   3=>o.header[13]=1,
   4=>o.tracker_after[0]=(99,0,0),
   5=>o.final_payload=Some(vec![vec![1]]),
   6=>{if c.stage==8{o.final_payload=None;}},
   7=>{o.timestamps.as_mut().unwrap().clear();},
   8=>o.timestamps.as_mut().unwrap()[0].written=false,
   9=>o.timestamps.as_mut().unwrap()[0].query_epoch+=1,
   10=>o.timestamps.as_mut().unwrap()[0].pass+=1,
   _=>{}
  }Ok(o)
 }
 fn journal_region(&mut self,c:&Invocation,o:&RegionOutput)->Result<()>{self.inner.journal_region(c,o)}
}
fn timing_backend(mode:u8)->TimingBackend{TimingBackend{inner:Backend{active:false,fail:None,calls:vec![]},mode}}
#[test]fn unavailable_timing_can_commit_only_complete_real_payload(){
 let(mut s,mut t)=start(1);let mut b=timing_backend(0);
 loop{if s.advance(&mut t,vec![],&mut b).unwrap().ready{break}}
 let done=s.finish(&mut t,|data|{assert_eq!(data,&[vec![1]]);Ok(())}).unwrap();
 assert_eq!(done.frame_generation,1);assert!(done.timestamps.iter().all(Option::is_none));
}
#[test]fn unavailable_timing_preserves_fence_validation_header_tracker_and_payload_rejection(){
 for mode in 1..=6{let(mut s,mut t)=start(1);let mut b=timing_backend(mode);
  loop{match s.advance(&mut t,vec![],&mut b){Err(_)=>break,Ok(p)=>assert!(!p.ready,"invalid completion mode {mode}")}}
  assert_eq!(s.committed().0,0);assert!(s.finish(&mut t,|_|Ok(())).is_err());
 }
}
#[test]fn present_timing_still_rejects_missing_unwritten_stale_or_reordered_records(){
 for mode in 7..=10{let(mut s,mut t)=start(1);let mut b=timing_backend(mode);
  assert!(s.advance(&mut t,vec![],&mut b).is_err());assert_eq!(s.committed().0,0);
 }
}
