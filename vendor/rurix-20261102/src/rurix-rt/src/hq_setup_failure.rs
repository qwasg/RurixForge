//! RFC-0016/RFC-0052, U32: AS creation journal and process-lifetime quarantine.
//! No Vulkan calls. Poison cannot be cleared in the same process.
use std::fs::{File,OpenOptions};
use std::io::Write;
use std::path::Path;
use std::sync::{Mutex,MutexGuard,OnceLock,atomic::{AtomicBool,Ordering}};
#[derive(Clone,Copy,PartialEq,Eq)]pub(super) enum Phase {AsSubmit,AsWait,OpticalRegionSubmit,OpticalRegionWait,RrFinalDeviceWait}
impl Phase {fn name(self)->&'static str{match self{Self::AsSubmit=>"as_initial_submit",Self::AsWait=>"as_initial_wait",Self::OpticalRegionSubmit=>"optical_region_submit",Self::OpticalRegionWait=>"optical_region_wait",Self::RrFinalDeviceWait=>"rr_final_device_wait"}}}

/// U63/U32 private closed failure taxonomy for RR's hidden frame-end present.
/// These are not final-device-wait results; absent VkResult stays absent.
/// Assisted-by: Codex:GPT-6.
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub(crate) enum RrPresentFailureStage {
 ThreadCheck,WindowState,Acquire,AcquireWait,Record,Submit,SubmitWait,
 Present,PresentResult,PresentWait,FinalPresentProof,Shutdown,
}
impl RrPresentFailureStage {
 pub(crate) fn name(self)->&'static str {match self {
  Self::ThreadCheck=>"thread_check",Self::WindowState=>"window_state",
  Self::Acquire=>"acquire",Self::AcquireWait=>"acquire_wait",Self::Record=>"record",
  Self::Submit=>"submit",Self::SubmitWait=>"submit_wait",Self::Present=>"present",
  Self::PresentResult=>"present_result",Self::PresentWait=>"present_wait",
  Self::FinalPresentProof=>"final_present_proof",Self::Shutdown=>"shutdown",
 }}
 fn wait_limit_ns(self)->Option<u64> {match self {
  Self::Acquire|Self::AcquireWait|Self::SubmitWait|Self::PresentWait=>Some(2_000_000_000),
  _=>None,
 }}
}
struct Service {poisoned:AtomicBool,gate:Mutex<()>,journal:OnceLock<Mutex<File>>}
static SERVICE:Service=Service::new();
pub(super) struct Permit<'a>{service:&'a Service,_gate:MutexGuard<'a,()>}
impl Service {
 const fn new()->Self{Self{poisoned:AtomicBool::new(false),gate:Mutex::new(()),journal:OnceLock::new()}}
 fn check(&self)->Result<(),String>{if self.poisoned.load(Ordering::Acquire){Err("render_exec worker quarantined after unconfirmed native GPU work; restart worker; submissions prohibited".into())}else{Ok(())}}
 fn permit(&self)->Result<Permit<'_>,String>{
  let gate=match self.gate.lock(){Ok(g)=>g,Err(_)=>{
   // Even a local admission failure must select the retained-owner branch;
   // an already-created setup fence must not leak through ordinary cleanup.
   self.poisoned.store(true,Ordering::Release);
   return Err("setup submission mutex poisoned; owners must be quarantined".into());
  }};
  self.check()?;Ok(Permit{service:self,_gate:gate})
 }
 fn configure(&self,path:&Path)->Result<(),String>{
  let _permit=self.permit()?;
  if self.journal.get().is_some(){return Err("setup journal already configured for this worker".into());}
  let mut file=OpenOptions::new().create_new(true).write(true).open(path).map_err(|e|format!("setup journal open {}: {e}",path.display()))?;
  writeln!(file,"{{\"schema\":\"rurix.hq.setup_lifecycle.v1\",\"event\":\"journal_open\",\"pid\":{}}}",std::process::id()).map_err(|e|format!("setup journal header: {e}"))?;
  file.sync_all().map_err(|e|format!("setup journal durable header: {e}"))?;
  self.journal.set(Mutex::new(file)).map_err(|_|"setup journal configured concurrently".to_owned())
 }
 fn append(&self,line:&str)->Result<(),String>{
  let mut file=self.journal.get().ok_or("setup journal unavailable; NOT claimed durable")?.lock().map_err(|_|"setup journal lock poisoned")?;
  file.write_all(line.as_bytes()).and_then(|_|file.write_all(b"\n")).and_then(|_|file.sync_all()).map_err(|e|format!("setup journal durable append: {e}"))
 }
 fn quarantine_locked(&self,phase:Phase,result:i32,elapsed_ns:u64,fence:u64,command:usize)->String{
  self.quarantine_record(phase,result,elapsed_ns,fence,command,None)
 }
 fn quarantine_record(&self,phase:Phase,result:i32,elapsed_ns:u64,fence:u64,command:usize,device:Option<usize>)->String{
  self.poisoned.store(true,Ordering::Release);
  let rr=phase==Phase::RrFinalDeviceWait;let phase=phase.name();
  let wait_limit=if rr{"null"}else{"5000000000"};
  let fence_json=if rr{"null".to_owned()}else{format!("\"0x{fence:x}\"")};
  let extra=if rr{format!(",\"wait_kind\":\"vkDeviceWaitIdle_indefinite\",\"device\":\"0x{:x}\"",device.unwrap_or(0))}else{String::new()};
  let line=format!("{{\"schema\":\"rurix.hq.setup_lifecycle.v1\",\"event\":\"quarantined\",\"phase\":\"{phase}\",\"driver_vk_result\":{result},\"elapsed_ns\":{elapsed_ns},\"wait_limit_ns\":{wait_limit},\"fence\":{fence_json},\"command\":\"0x{command:x}\",\"completion_confirmed\":false,\"reclamation\":\"not_performed_process_lifetime_quarantine\",\"worker_reusable\":false{extra}}}");
  let durable=self.append(&line);eprintln!("[rurix-rt-setup] {line}");
  match durable{Ok(())=>format!("native GPU {phase} VkResult {result}; original result durably recorded; owners quarantined; worker restart required"),
   Err(e)=>format!("native GPU {phase} VkResult {result}; owners quarantined; DURABLE_JOURNAL_FAILED: {e}")}
 }
 fn quarantine_rr_present(&self,stage:RrPresentFailureStage,result:Option<i32>,elapsed_ns:u64,
  device:usize,fence:u64,command:usize)->String {
  // Poison before attempting the submission gate or journal I/O. The caller
  // may still hold Permit on this thread: failure handling must never deadlock.
  self.poisoned.store(true,Ordering::Release);
  let gate=match self.gate.try_lock() {
   Ok(g)=>Ok(g),
   Err(std::sync::TryLockError::Poisoned(p))=>Ok(p.into_inner()),
   Err(std::sync::TryLockError::WouldBlock)=>Err("setup submission gate_busy; durable append not attempted"),
  };
  let wait_limit=stage.wait_limit_ns().map_or_else(||"null".to_owned(),|v|v.to_string());
  let stage=stage.name();
  let result_json=result.map_or_else(||"null".to_owned(),|v|v.to_string());
  let fence_json=if fence==0 {"null".to_owned()}else{format!("\"0x{fence:x}\"")};
  let line=format!("{{\"schema\":\"rurix.hq.setup_lifecycle.v1\",\"event\":\"quarantined\",\"phase\":\"vendor_rr_present\",\"stage\":\"{stage}\",\"driver_vk_result\":{result_json},\"elapsed_ns\":{elapsed_ns},\"wait_limit_ns\":{wait_limit},\"device\":\"0x{device:x}\",\"fence\":{fence_json},\"command\":\"0x{command:x}\",\"completion_confirmed\":false,\"reclamation\":\"not_performed_process_lifetime_quarantine\",\"worker_reusable\":false}}");
  let durable=match gate {Ok(_gate)=>self.append(&line),Err(error)=>Err(error.to_owned())};
  eprintln!("[rurix-rt-setup] {line}");
  let result_note=result.map_or_else(||"VkResult unavailable".to_owned(),|v|format!("VkResult {v}"));
  match durable {
   Ok(())=>format!("native GPU vendor_rr_present/{stage} {result_note}; original result availability durably recorded; owners quarantined; worker restart required"),
   Err(e)=>format!("native GPU vendor_rr_present/{stage} {result_note}; owners quarantined; DURABLE_JOURNAL_FAILED: {e}"),
  }
 }

}
impl Permit<'_>{pub(super) fn quarantine(&self,phase:Phase,result:i32,elapsed_ns:u64,fence:u64,command:usize)->String{self.service.quarantine_locked(phase,result,elapsed_ns,fence,command)}}
pub(super) fn poisoned()->bool{SERVICE.poisoned.load(Ordering::Acquire)}
pub(super) fn check()->Result<(),String>{SERVICE.check()}
pub(super) fn permit()->Result<Permit<'static>,String>{SERVICE.permit()}
pub(super) fn configure(path:&Path)->Result<(),String>{SERVICE.configure(path)}
pub(super) fn quarantine(phase:Phase,result:i32,elapsed_ns:u64,fence:u64,command:usize)->String{
 let _gate=SERVICE.gate.lock().unwrap_or_else(|p|p.into_inner());
 SERVICE.quarantine_locked(phase,result,elapsed_ns,fence,command)
}
pub(super) fn quarantine_rr_final_wait(result:i32,elapsed_ns:u64,device:usize,command:usize)->String{
 let _gate=SERVICE.gate.lock().unwrap_or_else(|p|p.into_inner());
 SERVICE.quarantine_record(Phase::RrFinalDeviceWait,result,elapsed_ns,0,command,Some(device))
}

/// Records/poisons only. Caller retains the complete RR/WSI/SDK/window graph and
/// may still hold Permit: a busy gate returns non-durable failure. No native wait.
pub(super) fn quarantine_rr_present(stage:RrPresentFailureStage,result:Option<i32>,elapsed_ns:u64,
 device:usize,fence:u64,command:usize)->String {
 SERVICE.quarantine_rr_present(stage,result,elapsed_ns,device,fence,command)
}
/// The full native owner graph and callback storage remain alive. This is NOT
/// reclamation. No forgotten handle can be reused through render_exec.
pub(super) fn retain_owners<A,B,C>(frame:A,as_state:B,callback:C){std::mem::forget((frame,as_state,callback));}
#[cfg(test)]mod tests{
 use super::*;use std::sync::{Arc,atomic::AtomicUsize};
 struct Owner(Arc<AtomicUsize>);impl Drop for Owner{fn drop(&mut self){self.0.fetch_add(1,Ordering::SeqCst);}}
 fn path(tag:&str)->std::path::PathBuf{std::env::temp_dir().join(format!("rurix-setup-{tag}-{}-{}.jsonl",std::process::id(),std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()))}
 #[test]fn durable_result_then_retained_owners_and_blocked_submit(){
  let s=Service::new();let path=path("wait");s.configure(&path).unwrap();let count=Arc::new(AtomicUsize::new(0));let owners=(Owner(count.clone()),Owner(count.clone()),Owner(count.clone()));
  {let permit=s.permit().unwrap();let error=permit.quarantine(Phase::AsWait,2,5_000_001_004,0x4b430000004b43,17);assert!(error.contains("durably recorded"));}
  let journal=std::fs::read_to_string(&path).unwrap();assert!(journal.contains("\"driver_vk_result\":2")&&journal.contains("\"phase\":\"as_initial_wait\""));
  retain_owners(owners.0,owners.1,owners.2);assert_eq!(count.load(Ordering::SeqCst),0);
  let calls=AtomicUsize::new(0);let attempted=(||->Result<(),String>{let _permit=s.permit()?;calls.fetch_add(1,Ordering::SeqCst);Ok(())})();assert!(attempted.is_err());assert_eq!(calls.load(Ordering::SeqCst),0);std::fs::remove_file(path).unwrap();
 }
 #[test]fn optical_region_phase_preserves_raw_result_and_blocks_every_next_submit(){for phase in [Phase::OpticalRegionSubmit,Phase::OpticalRegionWait]{let s=Service::new();let path=path("optical");s.configure(&path).unwrap();{let p=s.permit().unwrap();let e=p.quarantine(phase,2,5_000_000_021,0x1234,0x5678);assert!(e.contains("VkResult 2"));}let text=std::fs::read_to_string(&path).unwrap();assert!(text.contains(phase.name())&&text.contains("\"completion_confirmed\":false")&&!text.contains("as_initial_wait"));assert!(s.permit().is_err());std::fs::remove_file(path).unwrap();}}
 #[test]fn missing_journal_poisoned_and_never_claims_durable(){let s=Service::new();let permit=s.permit().unwrap();let msg=permit.quarantine(Phase::AsSubmit,-4,13,7,9);drop(permit);assert!(msg.contains("VkResult -4")&&msg.contains("DURABLE_JOURNAL_FAILED"));assert!(s.check().is_err());}
 #[test]fn confirmed_or_unsubmitted_owners_release_normally(){let s=Service::new();let count=Arc::new(AtomicUsize::new(0));{let _permit=s.permit().unwrap();let _owners=[Owner(count.clone()),Owner(count.clone()),Owner(count.clone())];}assert_eq!(count.load(Ordering::SeqCst),3);assert!(s.permit().is_ok());}
 #[test]fn journal_is_unique(){let s=Service::new();let a=path("one");let b=path("two");s.configure(&a).unwrap();assert!(s.configure(&b).is_err());assert!(!b.exists());std::fs::remove_file(a).unwrap();}
 #[test]fn mutex_denial_selects_quarantine_not_pre_submit_cleanup(){let s=Service::new();let _=std::panic::catch_unwind(||{let _g=s.gate.lock().unwrap();panic!("injected mutex owner panic")});assert!(s.permit().is_err());assert!(s.poisoned.load(Ordering::Acquire));}
}

#[cfg(test)]mod rr_present_bridge_tests {
 use super::*;use std::sync::{Arc,Barrier};
 fn path(tag:&str)->std::path::PathBuf {std::env::temp_dir().join(format!("rurix-rr-present-{tag}-{}-{}.jsonl",std::process::id(),std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()))}
 fn last(path:&Path)->String {std::fs::read_to_string(path).unwrap().lines().last().unwrap().to_owned()}
 #[test]fn closed_stages_keep_real_wait_limits_and_handles(){
  let stages=[(RrPresentFailureStage::ThreadCheck,"thread_check",false),(RrPresentFailureStage::WindowState,"window_state",false),
   (RrPresentFailureStage::Acquire,"acquire",true),(RrPresentFailureStage::AcquireWait,"acquire_wait",true),
   (RrPresentFailureStage::Record,"record",false),(RrPresentFailureStage::Submit,"submit",false),
   (RrPresentFailureStage::SubmitWait,"submit_wait",true),(RrPresentFailureStage::Present,"present",false),
   (RrPresentFailureStage::PresentResult,"present_result",false),(RrPresentFailureStage::PresentWait,"present_wait",true),
   (RrPresentFailureStage::FinalPresentProof,"final_present_proof",false),(RrPresentFailureStage::Shutdown,"shutdown",false)];
  let mut names=std::collections::BTreeSet::new();
  for(stage,name,bounded)in stages {
   assert_eq!(stage.name(),name);assert!(names.insert(name));let s=Service::new();let p=path(name);s.configure(&p).unwrap();
   let e=s.quarantine_rr_present(stage,None,123456789,0x123456,0xabcdef,0x789abc);assert!(e.contains("durably recorded"));
   let row=last(&p);assert!(row.contains("\"phase\":\"vendor_rr_present\"")&&row.contains(&format!("\"stage\":\"{name}\"")));
   assert!(row.contains(if bounded{"\"wait_limit_ns\":2000000000"}else{"\"wait_limit_ns\":null"}));
   assert!(row.contains("\"driver_vk_result\":null")&&row.contains("\"elapsed_ns\":123456789")&&row.contains("\"device\":\"0x123456\"")&&row.contains("\"fence\":\"0xabcdef\"")&&row.contains("\"command\":\"0x789abc\""));
   assert!(row.contains("\"completion_confirmed\":false")&&row.contains("\"worker_reusable\":false")&&row.contains("not_performed_process_lifetime_quarantine"));
   assert!(!row.contains("rr_final_device_wait")&&!row.contains("vkDeviceWaitIdle"));assert!(s.permit().is_err());std::fs::remove_file(p).unwrap();
  }
 }
 #[test]fn real_driver_results_are_not_mapped_to_final_wait_or_success(){
  for result in [2,-1,-2,-4,1000001003] {let s=Service::new();let p=path("actual");s.configure(&p).unwrap();
   s.quarantine_rr_present(RrPresentFailureStage::PresentResult,Some(result),19,0x51,0,0);
   let row=last(&p);assert!(row.contains(&format!("\"driver_vk_result\":{result}"))&&row.contains("\"fence\":null")&&row.contains("\"command\":\"0x0\""));
   assert!(s.check().is_err());std::fs::remove_file(p).unwrap();
  }
 }
 #[test]fn missing_driver_result_and_shutdown_remain_null(){
  for stage in [RrPresentFailureStage::ThreadCheck,RrPresentFailureStage::WindowState,RrPresentFailureStage::PresentResult,RrPresentFailureStage::FinalPresentProof,RrPresentFailureStage::Shutdown] {
   let s=Service::new();let p=path("none");s.configure(&p).unwrap();let e=s.quarantine_rr_present(stage,None,0,0,0,0);
   assert!(e.contains("VkResult unavailable"));let row=last(&p);assert!(row.contains("\"driver_vk_result\":null")&&!row.contains("\"driver_vk_result\":0"));std::fs::remove_file(p).unwrap();
  }
 }
 #[test]fn no_journal_still_poisons_and_never_claims_durability(){
  let s=Service::new();let e=s.quarantine_rr_present(RrPresentFailureStage::FinalPresentProof,None,7,1,0,0);
  assert!(e.contains("DURABLE_JOURNAL_FAILED")&&!e.contains("durably recorded"));assert!(s.check().is_err()&&s.permit().is_err());
 }
 #[test]fn journal_write_failure_still_poisons(){
  let p=path("readonly");std::fs::write(&p,b"original\n").unwrap();let s=Service::new();s.journal.set(Mutex::new(File::open(&p).unwrap())).unwrap();
  let e=s.quarantine_rr_present(RrPresentFailureStage::Submit,Some(-1),9,1,2,3);
  assert!(e.contains("DURABLE_JOURNAL_FAILED")&&s.check().is_err());assert_eq!(std::fs::read(&p).unwrap(),b"original\n");std::fs::remove_file(p).unwrap();
 }
 #[test]fn poisoned_gate_is_recovered_for_failure_recording_only(){
  let s=Service::new();let p=path("mutex");s.configure(&p).unwrap();let _=std::panic::catch_unwind(||{let _g=s.gate.lock().unwrap();panic!("injected gate panic")});
  let e=s.quarantine_rr_present(RrPresentFailureStage::Record,None,9,1,0,3);
  assert!(e.contains("durably recorded")&&s.check().is_err());assert!(last(&p).contains("\"stage\":\"record\""));std::fs::remove_file(p).unwrap();
 }
 #[test]fn poison_and_return_do_not_wait_for_submission_gate(){
  let s=Arc::new(Service::new());let gate=s.gate.lock().unwrap();let barrier=Arc::new(Barrier::new(2));let worker=s.clone();let start=barrier.clone();
  let (send,receive)=std::sync::mpsc::channel();
  let handle=std::thread::spawn(move||{start.wait();let error=worker.quarantine_rr_present(RrPresentFailureStage::PresentWait,Some(2),2_000_000_000,1,2,3);send.send(error).unwrap();});
  barrier.wait();let error=receive.recv_timeout(std::time::Duration::from_secs(2)).expect("present quarantine waited on held submission gate");
  assert!(s.check().is_err());assert!(error.contains("DURABLE_JOURNAL_FAILED")&&error.contains("gate_busy"));
  drop(gate);handle.join().unwrap();
 }
 #[test]fn same_thread_held_permit_returns_non_durable_without_deadlock(){
  let s=Service::new();let p=path("held-permit");s.configure(&p).unwrap();
  let permit=s.permit().unwrap();let started=std::time::Instant::now();
  let error=s.quarantine_rr_present(RrPresentFailureStage::Present,None,31,1,2,3);
  assert!(started.elapsed()<std::time::Duration::from_secs(1));
  assert!(error.contains("DURABLE_JOURNAL_FAILED")&&error.contains("gate_busy")&&!error.contains("durably recorded"));
  assert!(s.check().is_err());drop(permit);assert!(s.permit().is_err());
  assert!(last(&p).contains("journal_open"));std::fs::remove_file(p).unwrap();
 }
 #[test]fn original_final_wait_record_shape_remains_unchanged(){
  let s=Service::new();let p=path("legacy");s.configure(&p).unwrap();let e=s.quarantine_record(Phase::RrFinalDeviceWait,2,17,0,23,Some(29));
  assert!(e.contains("VkResult 2"));assert_eq!(last(&p),"{\"schema\":\"rurix.hq.setup_lifecycle.v1\",\"event\":\"quarantined\",\"phase\":\"rr_final_device_wait\",\"driver_vk_result\":2,\"elapsed_ns\":17,\"wait_limit_ns\":null,\"fence\":null,\"command\":\"0x17\",\"completion_confirmed\":false,\"reclamation\":\"not_performed_process_lifetime_quarantine\",\"worker_reusable\":false,\"wait_kind\":\"vkDeviceWaitIdle_indefinite\",\"device\":\"0x1d\"}");std::fs::remove_file(p).unwrap();
 }
}
