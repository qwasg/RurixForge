//! Build the augmented graph ONCE from actual source role bindings. No v32
//! physical pass indices/resource IDs or sample2105 restrictions are inherited.
//! Clear-air only until the separate cloudy tail operator family is certified.
use super::*;
use super::agent_persistent_graph::{Recipe,Extra,Kernel,Source};
use super::agent_persistent_owner::Config;
use std::collections::BTreeMap;
use std::sync::Arc;
// RFC-0052: finite source-only submission threshold, shared by schedule evidence.
pub(super) const FINITE_PASS_THRESHOLD:usize=61;
pub struct Blob{pub bytes:Arc<[u8]>,pub sha256:[u8;32]}
pub struct Profile{pub device_local_queue:bool,pub capacity:u32,pub width:u32,pub height:u32,pub sample:u64,pub seed:u64,pub budget:u32,pub clouds:bool,pub source_epoch:[u8;32]}
pub struct Prepared{pub recipe:Recipe,pub config:Config,pub extra_declared_bytes:u64,pub T_finish:usize,pub R_finish:usize}
fn bindings(p:&Pass<'_>)->Result<Vec<u32>,String>{let Pass::Compute(c)=p else{return Err("compute-only optical source graph".into())};if c.bindings.accel_structs.len()>1{return Err("single original optical AS required".into())}Ok(c.bindings.storage_buffers.clone())}
fn name<'a>(p:&'a Pass<'_>)->&'a str{pass_name(p)}
#[derive(Clone)]
struct Cloud {inputs:[u32;5],control:u32,stride:u32}
struct Build<'a>{original:&'a [Pass<'a>],blobs:&'a BTreeMap<String,Blob>,recipe:Recipe,stages:Vec<Vec<std::ops::Range<usize>>>,base:usize,rows:u32,marker:u32,frame:u32,summary:u32,width:u32,height:u32,cloud:Option<Cloud>,capacity:u32}
impl Build<'_>{
 fn extra(&self,i:u32)->u32{self.base as u32+i}
 fn tail(&mut self,n:&str,ids:Vec<u32>,groups:[u32;3],as_query:bool)->Result<(),String>{let blob=self.blobs.get(n).ok_or_else(||format!("source-certified tail shader missing: {n}"))?;if rurix_pkg::sha256::digest(&blob.bytes)!=blob.sha256{return Err("tail SPIR-V pin drift".into())}let i=self.recipe.kernels.len();self.recipe.kernels.push(Kernel{name:n.into(),spirv:blob.bytes.clone(),bindings:Bindings{storage_buffers:ids,accel_structs:if as_query{vec![0]}else{vec![]},..Default::default()},groups});self.recipe.schedule.push(Source::Tail(i));Ok(())}
 fn local(&mut self,n:&str,ids:&[u32])->Result<(),String>{self.tail(n,ids.iter().map(|&i|self.extra(i)).collect(),[8,self.capacity/512,1],false)}
 fn local_error(&mut self,index:u32)->Result<(),String>{self.tail("agent_alltile_error_or",vec![self.extra(index),self.extra(39),self.extra(1)],[1,1,1],false)}
 fn end(&mut self,start:usize)->Result<std::ops::Range<usize>,String>{
  self.tail("agent_all_summary_rows",vec![self.summary,self.marker,self.frame,self.rows],[self.height.div_ceil(64),1,1],false)?;
  let header=[0,6,1,13,33,11,15,17,19,38,24,27,29,31,35,37,42,45,47,40,39,5];let mut ids=header.iter().map(|&i|self.extra(i)).collect::<Vec<_>>();ids.extend([self.rows,self.frame,self.extra(51)]);
  self.tail("agent_all_header",ids,[1,1,1],false)?;self.tail("hq_pending_header_copy_words",vec![self.extra(0),self.extra(5),self.extra(3)],[1,1,1],false)?;
  let end=self.recipe.schedule.len();if end-start>256{return Err("bounded scoped region overflow".into())}Ok(start..end)
 }
 fn finite(&mut self,from:usize,to:usize,capture_indices:&[usize])->Result<(),String>{
  // Stage4 packs whole error-summary scopes, with at most one AS dispatch
  // per region. Keep zero-AS scopes with adjacent work instead of flushing them.
  let r_prefix=self.stages.len()==4;let mut scopes=vec![];
  if r_prefix{
   let mut scope_from=from;let mut scope_as=0usize;
   for i in from..to{
    if let Pass::Compute(c)=&self.original[i]{scope_as+=usize::from(!c.bindings.accel_structs.is_empty());}
    if name(&self.original[i])=="hq_frame_error_summary_merge"{
     if scope_as>1{return Err(format!("R finite original scope {scope_from}..{} contains {scope_as} AS dispatches; cannot split inside an error-summary scope",i+1));}
     scopes.push((scope_from,scope_as));scope_from=i+1;scope_as=0;
    }
   }
   if scope_from<to{
    if scope_as>1{return Err(format!("R finite original scope {scope_from}..{to} contains {scope_as} AS dispatches; cannot split inside an error-summary scope"));}
    scopes.push((scope_from,scope_as));
   }
  }
  let mut parts=vec![];let mut start=self.recipe.schedule.len();let mut next_scope=0usize;let mut region_as=0usize;
  self.tail("agent_all_clear_marker",vec![self.frame,self.marker],[self.width.div_ceil(8),self.height.div_ceil(8),1],false)?;
  for i in from..to{
   if r_prefix&&next_scope<scopes.len()&&scopes[next_scope].0==i{
    let scope_as=scopes[next_scope].1;
    if region_as+scope_as>1{parts.push(self.end(start)?);start=self.recipe.schedule.len();region_as=0;}
    region_as+=scope_as;next_scope+=1;
   }
   if capture_indices.contains(&i){let state=bindings(&self.original[i])?[0];self.tail("agent_all_capture",vec![state,self.frame,self.extra(1),self.extra(54),self.marker],[self.width.div_ceil(8),self.height.div_ceil(8),1],false)?;}
   self.recipe.schedule.push(Source::Original(i));
   if self.recipe.schedule.len()-start>=FINITE_PASS_THRESHOLD&&name(&self.original[i])=="hq_frame_error_summary_merge"{parts.push(self.end(start)?);start=self.recipe.schedule.len();region_as=0;}
  }
  if start<self.recipe.schedule.len(){parts.push(self.end(start)?)}self.stages.push(parts);Ok(())
 }
 fn suffix(&mut self,from:usize,to:usize)->Result<(),String>{
  // Pack whole error-summary scopes as stage4 does, without marker clear/capture.
  let mut scopes=vec![];let mut scope_from=from;let mut scope_as=0usize;
  for i in from..to{
   if let Pass::Compute(c)=&self.original[i]{scope_as+=usize::from(!c.bindings.accel_structs.is_empty());}
   if name(&self.original[i])=="hq_frame_error_summary_merge"{
    if scope_as>1{return Err(format!("suffix original scope {scope_from}..{} contains {scope_as} AS dispatches; cannot split inside an error-summary scope",i+1));}
    scopes.push((scope_from,scope_as));scope_from=i+1;scope_as=0;
   }
  }
  if scope_from<to{
   if scope_as>1{return Err(format!("suffix original scope {scope_from}..{to} contains {scope_as} AS dispatches; cannot split inside an error-summary scope"));}
   scopes.push((scope_from,scope_as));
  }
  let mut parts=vec![];let mut at=self.recipe.schedule.len();let mut next_scope=0usize;let mut region_as=0usize;
  for i in from..to{
   if next_scope<scopes.len()&&scopes[next_scope].0==i{
    let scope_as=scopes[next_scope].1;
    if region_as+scope_as>1{parts.push(self.end(at)?);at=self.recipe.schedule.len();region_as=0;}
    region_as+=scope_as;next_scope+=1;
   }
   self.recipe.schedule.push(Source::Original(i));
   if self.recipe.schedule.len()-at>=FINITE_PASS_THRESHOLD&&name(&self.original[i])=="hq_frame_error_summary_merge"{parts.push(self.end(at)?);at=self.recipe.schedule.len();region_as=0;}
  }
  if at<self.recipe.schedule.len(){parts.push(self.end(at)?)}self.stages.push(parts);Ok(())
 }
 fn tiles(&mut self,step:&[u32],edge:&[u32],terminal:&[u32],environment:&[u32],target:u32,receiver:[u32;2])->Result<(),String>{
  let at=self.recipe.schedule.len();self.tail("agent_alltile_gather",vec![self.extra(54),self.marker,step[1],self.frame,self.extra(1),self.extra(7),self.extra(20),self.extra(52),self.extra(43),self.extra(44)],[8,self.capacity/512,1],false)?;
  self.tail("agent_alltile_error_or",vec![self.extra(52),self.extra(39),self.extra(1)],[1,1,1],false)?;
  for(n,ids)in [("agent_alltile_packetize",vec![7,7,1,8,9,10,11]),("hq_tail_boundary_rr_exact",vec![9,9,10,1,12,13]),("hq_tail_boundary_commit",vec![9,12,13,10,1,14,15]),("hq_tail_packet_promote",vec![14,1,16,17]),("hq_tail_packet_validate",vec![16,16,1,18,19]),("hq_tail_fixture_store",vec![16,1,6,2])]{self.local(n,&ids)?;}let part=self.end(at)?;self.stages.push(vec![part]);
  let at=self.recipe.schedule.len();self.local("hq_tail_fixture_unpack_guarded",&[6,1,39,2,38])?;
  let mut d=step[..34].to_vec();for(i,e)in [(0,2),(1,20),(29,21),(30,22),(31,23),(32,24),(33,25)]{d[i]=self.extra(e);}d.extend([self.extra(6),self.extra(26),self.extra(51)]);self.tail("agent_alltile_step_surface",d,[8,self.capacity/512,1],true)?;
  self.tail("agent_alltile_edge_repair",vec![self.extra(25),edge[1],edge[2],self.frame,self.extra(21),self.extra(27),self.extra(51)],[8,self.capacity/512,1],false)?;
  if let Some(cloud)=self.cloud.clone(){let mut ids=cloud.inputs.to_vec();ids.extend([self.extra(22),self.extra(57),self.extra(51)]);self.tail("agent_cloudtile_directional",ids,[8,self.capacity/512,1],false)?;}
  let mut d=terminal.to_vec();for(i,e)in [(0,23),(29,41),(30,42),(31,43),(32,44),(33,45),(34,21),(35,6)]{d[i]=self.extra(e);}if self.cloud.is_some(){d[41]=self.extra(58);}d.push(self.extra(51));self.tail(if self.cloud.is_some(){"agent_cloudtile_sun_terminal_surface"}else{"agent_alltile_sun_terminal_surface"},d,[8,self.capacity/512,1],true)?;
  if let Some(cloud)=self.cloud.clone(){self.tail("agent_cloudtile_finite_guard",vec![self.extra(58),self.frame,cloud.control,self.extra(59),self.extra(51)],[8,self.capacity/512,1],false)?;self.local_error(59)?;}
  let mut d=environment.to_vec();for(i,e)in [(2,23),(3,41),(4,46),(26,53)]{d[i]=self.extra(e);}d.push(self.extra(51));self.tail("agent_alltile_environment_surface",d,[8,self.capacity/512,1],true)?;
  self.tail("agent_alltile_error_or",vec![self.extra(53),self.extra(39),self.extra(1)],[1,1,1],false)?;
  if self.cloud.is_some(){self.local("agent_cloudtile_fold",&[2,46,57,1,21,47])?;}else{self.local("hq_tail_fixture_fold",&[2,46,46,1,21,47])?;}
  for(n,ids)in [("hq_tail_packet_advance",vec![6,21,26,1,28,29]),("hq_tail_packet_validate",vec![28,28,1,30,31]),("hq_tail_continue_rr_exact",vec![28,28,30,1,32,33]),("hq_tail_continue_rr_commit",vec![28,32,33,30,1,34,35]),("hq_tail_packet_validate",vec![34,34,1,36,37]),("hq_tail_fixture_store",vec![34,1,6,2])]{self.local(n,&ids)?;}let part=self.end(at)?;self.stages.push(vec![part]);
  let at=self.recipe.schedule.len();self.tail("agent_alltile_writeback",vec![self.extra(6),self.extra(10),self.extra(43),self.extra(44),self.extra(1),self.frame,self.marker,target,receiver[0],receiver[1],self.extra(40),self.extra(4)],[8,self.capacity/512,1],false)?;let part=self.end(at)?;self.stages.push(vec![part]);Ok(())
 }
}
pub fn prepare<'a>(resources:&[ResourceDesc<'a>],passes:&'a [Pass<'a>],blobs:&'a BTreeMap<String,Blob>,p:Profile)->Result<Prepared,String>{
 if !matches!(p.capacity,4096|65536){return Err("closed optical queue capacity must be4096 or65536".into())}
 for pass in passes{let b=pass_bindings(pass);if b.uniform.is_some()||!b.sampled_images.is_empty()||!b.storage_images.is_empty(){return Err("closed source graph supports SSBO/AS only".into())}}
 if p.width==0||p.height==0||p.width>(1u32<<24)||p.height>(1u32<<24)||p.sample>(1u64<<24)||p.budget==0||p.budget>32||(!p.clouds&&passes.iter().any(|v|name(v).contains("canonical_air")||name(v).contains("solar_cloud")||name(v).contains("finite_cloud"))){return Err("optical operator family/legacy exact identity domain mismatch".into())}
 // The opt-in T kernel retains the original first34 slots. Tail transport
 // still derives its packet from the unchanged R suffix and R terminal.
 let thin_nee=passes.iter().any(|v|name(v)=="hq_primary_partition_transmission_nee_step_surface");
 let t_step_name=if thin_nee{"hq_primary_partition_transmission_nee_step_surface"}else{"hq_primary_partition_transmission_step_surface"};
 let main_t=passes.iter().find(|v|name(v)==t_step_name).ok_or("main T source missing")?;let main_frame=bindings(main_t)?[24];
 let pixels=u64::from(p.width)*u64::from(p.height);let tsteps=passes.iter().enumerate().filter_map(|(i,v)|(name(v)==t_step_name && pass_bindings(v).storage_buffers.get(24)==Some(&main_frame)).then_some(i)).collect::<Vec<_>>();
 if tsteps.len()!=p.budget as usize+1{return Err("all-T source budget census differs".into())}let tlast=*tsteps.last().unwrap();
 let tf=(tlast+1..passes.len()).find(|&i|name(&passes[i])=="hq_glass_finish").ok_or("T finish missing")?;
 let rsteps=(tf+1..passes.len()).filter(|&i|name(&passes[i])=="hq_primary_one_r_suffix_step_surface" && pass_bindings(&passes[i]).storage_buffers.get(24)==Some(&main_frame)).collect::<Vec<_>>();if rsteps.len()!=p.budget as usize+1{return Err("actual R suffix budget census differs".into())}
 let rf=(*rsteps.last().unwrap()+1..passes.len()).find(|&i|name(&passes[i])=="hq_glass_finish").ok_or("R finish missing")?;
 let r_terminal_name=if p.clouds{"hq_primary_one_r_sun_terminal_mesh_full_brdf_capture_thin_shadow_ns_cloud_sun_finite_cloud_surface"}else{"hq_primary_one_r_sun_terminal_mesh_full_brdf_capture_thin_shadow_ns_surface"};
 let t_terminal_name=if p.clouds{"hq_glass_sun_primary_terminal_mesh_full_brdf_capture_thin_shadow_ns_cloud_sun_finite_cloud_surface"}else if thin_nee{"hq_glass_sun_primary_terminal_transmission_mis_surface"}else{"hq_glass_sun_primary_terminal_mesh_full_brdf_capture_thin_shadow_ns_surface"};
 let first=rsteps[0];let term=(first+1..rf).find(|&i|name(&passes[i])==r_terminal_name).ok_or("clear sun/full-BRDF R terminal missing")?;
 let edge=(first+1..term).find(|&i|name(&passes[i])=="hq_triangle_origin_edge_repair").ok_or("R edge repair missing")?;
 let env=(term+1..rf).find(|&i|name(&passes[i])=="hq_environment_fill_surface").ok_or("R environment source missing")?;
 let tt=(tlast+1..tf).find(|&i|name(&passes[i])==t_terminal_name).ok_or("T receiver source missing")?;
 let step=bindings(&passes[first])?;let terminal=bindings(&passes[term])?;let edges=bindings(&passes[edge])?;let environment=bindings(&passes[env])?;let tterminal=bindings(&passes[tt])?;
 if step.len()!=36||terminal.len()!=if p.clouds{43}else{36}||edges.len()!=6||environment.len()!=27||tterminal.len()!=if p.clouds{41}else if thin_nee{35}else{34}{return Err("actual source role descriptor ABI differs".into())}
 for i in [tlast,first,term,env,tt]{let Pass::Compute(c)=&passes[i]else{return Err("source query is not compute".into())};if c.bindings.accel_structs!=[0]{return Err("source query AS owner/index differs".into())}}
 let frame=step[24];if terminal[25]!=frame||edges[3]!=frame{return Err("tail source frame identity differs".into())}
 let cloud=if p.clouds{Some(cloud_source(resources,passes,&terminal,&tterminal,frame,p.width,p.height)?)}else{None};
 let summary=passes.iter().find(|v|name(v)=="hq_frame_error_summary_merge").ok_or_else(||"strict source summary absent".to_owned()).and_then(bindings)?;if summary.len()!=3{return Err("source summary ABI".into())}
 let ResourceDesc::Buffer(control)=&resources[step[25]as usize]else{return Err("source R control absent".into())};let cb=control.data.ok_or("immutable R control bytes absent")?;if cb.len()!=16||u32::from_le_bytes(cb[4..8].try_into().unwrap())!=p.budget{return Err("source R immutable budget differs".into())}
 if u64::from_le_bytes(cb[8..16].try_into().unwrap())!=p.seed^0x5243_4f4e_545f_5631u64{return Err("source finite-R normal/random table seed domain differs".into())}
 let sizes=[64u64,40,128,64,512,64,256,128,128,256,4,4,12,16,256,4,256,4,4,4,64,128,48,48,4,16,4,4,256,4,4,4,12,16,256,4,4,4,4,4,4,16,4,64,64,4,16,4,4,4,4,64,4,4];
 let scalar=[0usize,1,3,4,5,39,48,49,50,51];let mut extras=vec![];
 for(i,n)in sizes.into_iter().enumerate(){let bytes=n*if scalar.contains(&i){1}else{u64::from(p.capacity)};let uninitialized=i==2||i==6;let device_local=uninitialized||(p.device_local_queue&&super::agent_persistent_graph::queue_internal_stride(i).is_some());extras.push(Extra{bytes,device_local,initial:if uninitialized{None}else{Some(vec![0;usize::try_from(bytes).map_err(|_|"host extra overflow")?])}});}
 extras.push(Extra{bytes:pixels.checked_mul(128).ok_or("snapshot overflow")?,device_local:true,initial:None});extras.push(Extra{bytes:pixels*4,device_local:true,initial:None});extras.push(Extra{bytes:u64::from(p.height)*8,device_local:true,initial:None});if let Some(cloud)=cloud.as_ref(){for bytes in [u64::from(p.capacity)*32,u64::from(p.capacity)*u64::from(cloud.stride)*4,u64::from(p.capacity)*4]{extras.push(Extra{bytes,device_local:true,initial:None});}}let total=extras.iter().try_fold(0u64,|a,e|a.checked_add(e.bytes).ok_or("extra allocation overflow"))?;
 let mut sealed=BTreeMap::new();for i in rsteps.iter().copied().chain([tlast,tf,rf,edge,term,env,tt]){sealed.insert(i,rurix_pkg::sha256::digest(format!("{:?}",pass_bindings(&passes[i])).as_bytes()));}
 let base=resources.len();let mut b=Build{original:passes,blobs,recipe:Recipe{device_local_queue:p.device_local_queue,queue_capacity:p.capacity,extras,kernels:vec![],schedule:vec![],source_epoch:p.source_epoch,sealed_original_bindings:sealed},stages:vec![],base,rows:base as u32+56,marker:base as u32+55,frame,summary:summary[2],width:p.width,height:p.height,cloud:cloud.clone(),capacity:p.capacity};
 b.finite(0,tf,&[tlast])?;b.tiles(&step,&edges,&terminal,&environment,bindings(&passes[tf])?[0],[tterminal[31],tterminal[32]])?;b.finite(tf,rf,&rsteps)?;b.tiles(&step,&edges,&terminal,&environment,bindings(&passes[rf])?[0],[terminal[31],terminal[32]])?;
 b.suffix(rf,passes.len())?;
 let sealed_bindings=b.recipe.schedule.iter().enumerate().filter_map(|(new,s)|match s{Source::Original(old)=>b.recipe.sealed_original_bindings.get(old).map(|h|(new,*h)),Source::Tail(_)=>None}).collect();
 let config=Config{journal_sync_per_frame:false,capacity:p.capacity,stages:b.stages,pixels,command:base,header:base+3,identity:base+1,selector:base+51,width:p.width,height:p.height,interface_budget:p.budget,sealed_bindings,frame:frame as usize,sample:p.sample,seed:p.seed,source_digest:p.source_epoch,cloud_inputs:cloud.as_ref().map(|c|c.inputs.map(|v|v as usize))};
 Ok(Prepared{recipe:b.recipe,config,extra_declared_bytes:total,T_finish:tf,R_finish:rf})
}

// Actual immutable controls/resources, not hashes used as fabricated payloads.
fn cloud_source(resources:&[ResourceDesc<'_>],passes:&[Pass<'_>],terminal:&[u32],tterminal:&[u32],frame:u32,width:u32,height:u32)->Result<Cloud,String>{
 fn buffer<'a,'b>(r:&'b [ResourceDesc<'a>],i:u32)->Result<&'b BufferDesc<'a>,String>{match r.get(i as usize){Some(ResourceDesc::Buffer(b))=>Ok(b),_=>Err("cloud role buffer absent".into())}}
 fn words(b:&BufferDesc<'_>,bytes:usize)->Result<Vec<u32>,String>{let d=b.data.ok_or("immutable cloud control bytes absent")?;if b.size!=bytes as u64||d.len()!=bytes{return Err("cloud source control byte shape".into())}Ok(d.chunks_exact(4).map(|v|u32::from_le_bytes(v.try_into().unwrap())).collect())}
 let inputs:[u32;5]=terminal[36..41].try_into().unwrap();if tterminal[34..39]!=inputs||inputs.iter().copied().collect::<std::collections::BTreeSet<_>>().len()!=5||inputs.contains(&frame){return Err("T/R cloud source snapshot identity differs".into())}
 let cp=buffer(resources,inputs[0])?;if !cp.device_local&&cp.size==284&&cp.data.is_some_and(|v|v.len()==284){}else{return Err("cloud params71 source shape".into())}
 for &id in &inputs[1..]{let b=buffer(resources,id)?;if !b.device_local||b.data.is_none_or(|v|v.len()as u64!=b.size)||b.usage.indirect{return Err("cloud immutable asset role differs".into())}}
 let source_queries=passes.iter().filter(|p|name(p)=="hq_cloud_directional").map(bindings).collect::<Result<Vec<_>,_>>()?;
 if source_queries.is_empty()||source_queries.iter().any(|v|v.len()!=7||v[..5]!=inputs){return Err("original directional cloud input ownership differs".into())}
 let f=buffer(resources,frame)?;let fw=words(f,512)?;
 let number=|i:usize|->Result<u32,String>{let v=f32::from_bits(fw[i]);if !v.is_finite()||v<0.0||v>16_777_216.0||v.fract()!=0.0{return Err("source frame integer control domain".into())}Ok(v as u32)};
 if [number(1)?,number(2)?]!=[width,height]{return Err("cloud source viewport differs".into())}
 let qp=number(7)?.checked_mul(16).and_then(|v|v.checked_add(number(8).ok()?)).ok_or("cloud light count overflow")?;
 let control=terminal[42];let cb=buffer(resources,control)?;let cw=words(cb,16)?;let nee=buffer(resources,terminal[28])?;let nw=words(nee,20)?;
 if !cb.device_local||!nee.device_local||cw[0]!=qp||cw[1]!=nw[2]||!(1..=64).contains(&cw[1])||cw[2]<8||cw[2]%8!=0||cw[3]!=0||cw[0].checked_add(cw[1]).is_none_or(|n|n>cw[2]/8){return Err("source finite-cloud stride/count control mismatch".into())}
 let witness=buffer(resources,terminal[41])?;let expected=u64::from(width)*u64::from(height)*u64::from(cw[2])*4;
 if witness.size!=expected||!witness.device_local||witness.data.is_some(){return Err("original finite witness extent/control differs".into())}
 Ok(Cloud{inputs,control,stride:cw[2]})
}


#[cfg(test)]
mod finite_partition_tests {
 use super::*;
 fn blobs()->BTreeMap<String,Blob>{
  ["agent_all_clear_marker","agent_all_capture","agent_all_summary_rows","agent_all_header","hq_pending_header_copy_words"].into_iter().map(|n|{
   let bytes:Arc<[u8]>=Arc::from([1u8,2,3,4]);let sha256=rurix_pkg::sha256::digest(&bytes);(n.into(),Blob{bytes,sha256})
  }).collect()
 }
 fn source(summary:bool)->Vec<Pass<'static>>{
  (0..500).map(|i|Pass::Compute(ComputePass{name:if summary&&i%7==6{"hq_frame_error_summary_merge"}else{"source_transport"},spirv:&[1,2,3,4],entry:None,dispatch:DispatchSpec::Direct([i as u32+1,1,1]),bindings:Bindings{storage_buffers:vec![i as u32%13,1,2],..Default::default()}})).collect()
 }
 fn build<'a>(source:&'a[Pass<'a>],blobs:&'a BTreeMap<String,Blob>,cloud:bool)->Build<'a>{
  Build{original:source,blobs,recipe:Recipe{device_local_queue:false,queue_capacity:4096,extras:vec![],kernels:vec![],schedule:vec![],source_epoch:[0;32],sealed_original_bindings:BTreeMap::new()},stages:vec![],base:256,rows:312,marker:311,frame:24,summary:2,width:320,height:180,cloud:cloud.then_some(Cloud{inputs:[0;5],control:0,stride:128}),capacity:4096}
 }
 fn projection(b:&Build<'_>)->Vec<String>{b.recipe.schedule.iter().filter_map(|s|match s{
  Source::Original(i)=>Some(format!("original {i} {:?} {:?}",pass_bindings(&b.original[*i]),match &b.original[*i]{Pass::Compute(c)=>c.dispatch,_=>unreachable!()})),
  Source::Tail(i)=>{let k=&b.recipe.kernels[*i];(!matches!(k.name.as_str(),"agent_all_summary_rows"|"agent_all_header"|"hq_pending_header_copy_words")).then(||format!("tail {} {:?} {:?}",k.name,k.bindings,k.groups))}
 }).collect()}
 #[test]
 fn finite_prefix_and_suffix_keep_original_order_with_short_summary_boundaries(){
  let original=source(true);let blobs=blobs();let mut b=build(&original,&blobs,false);
  b.finite(0,400,&[12,64,130,249]).unwrap();b.suffix(400,500).unwrap();
  let originals=b.recipe.schedule.iter().filter_map(|s|if let Source::Original(i)=s{Some(*i)}else{None}).collect::<Vec<_>>();
  assert_eq!(originals,(0..500).collect::<Vec<_>>());
  let mut next=0;for parts in &b.stages{for (part,r) in parts.iter().enumerate(){assert_eq!(r.start,next);next=r.end;
   // 61 source/tail body entries, at most 6 more to reach the next summary,
   // then the three existing summary/header passes. This fails with old185.
   assert!(r.end-r.start<=70,"finite submission exceeded the smaller source bound");
   let names=b.recipe.schedule[r.end-3..r.end].iter().map(|s|match s{Source::Tail(i)=>b.recipe.kernels[*i].name.as_str(),_=>panic!("footer must be tail")}).collect::<Vec<_>>();
   assert_eq!(names,["agent_all_summary_rows","agent_all_header","hq_pending_header_copy_words"]);
   if part+1<parts.len(){let Source::Original(i)=b.recipe.schedule[r.end-4]else{panic!("split must follow original summary")};assert_eq!(name(&original[i]),"hq_frame_error_summary_merge");}
  }}assert_eq!(next,b.recipe.schedule.len());
  assert_eq!(b.recipe.kernels.iter().filter(|k|k.name=="agent_all_capture").count(),4);
 }
 #[test]
 fn clear_and_cloud_share_finite_bound_without_changing_body_or_dispatch(){
  let original=source(true);let blobs=blobs();let mut clear=build(&original,&blobs,false);let mut cloud=build(&original,&blobs,true);
  for b in [&mut clear,&mut cloud]{b.finite(0,400,&[12,64,130,249]).unwrap();b.suffix(400,500).unwrap();}
  assert_eq!(clear.stages,cloud.stages);assert_eq!(projection(&clear),projection(&cloud));
 }
  #[test]
  fn suffix_packs_zero_as_scopes_with_neighbors_and_keeps_original_footer(){
   let mut original=source(true);original.truncate(56);
   for i in [0usize,14,28,42]{let Pass::Compute(c)=&mut original[i]else{unreachable!()};c.bindings.accel_structs=vec![0];}
   let blobs=blobs();let mut b=build(&original,&blobs,false);b.suffix(0,56).unwrap();
   assert_eq!(b.stages.len(),1);assert_eq!(b.stages[0].len(),4);
   let mut next=0;
   for (part,r) in b.stages[0].iter().enumerate(){
    assert_eq!(r.start,next);next=r.end;assert!(r.end-r.start<=256);
    let ids=b.recipe.schedule[r.clone()].iter().filter_map(|s|if let Source::Original(i)=s{Some(*i)}else{None}).collect::<Vec<_>>();
    assert_eq!(ids,(part*14..(part+1)*14).collect::<Vec<_>>());
    assert_eq!(ids.iter().filter(|&&i|!pass_bindings(&original[i]).accel_structs.is_empty()).count(),1);
    let footer=b.recipe.schedule[r.end-3..r.end].iter().map(|s|match s{Source::Tail(i)=>b.recipe.kernels[*i].name.as_str(),_=>panic!("original in footer")}).collect::<Vec<_>>();
    assert_eq!(footer,["agent_all_summary_rows","agent_all_header","hq_pending_header_copy_words"]);
    let Source::Original(i)=b.recipe.schedule[r.end-4]else{panic!("split must follow original summary")};assert_eq!(name(&original[i]),"hq_frame_error_summary_merge");
   }
   assert_eq!(next,b.recipe.schedule.len());
   assert!(b.recipe.kernels.iter().all(|k|matches!(k.name.as_str(),"agent_all_summary_rows"|"agent_all_header"|"hq_pending_header_copy_words")));
  }
  #[test]
  fn suffix_rejects_two_as_inside_one_original_error_scope(){
   let mut original=source(true);original.truncate(7);
   for i in [0usize,1]{let Pass::Compute(c)=&mut original[i]else{unreachable!()};c.bindings.accel_structs=vec![0];}
   let blobs=blobs();let mut b=build(&original,&blobs,false);
   assert!(b.suffix(0,7).unwrap_err().contains("contains 2 AS dispatches; cannot split inside an error-summary scope"));
   assert!(b.recipe.schedule.is_empty());
  }
 #[test]
 fn missing_summary_still_rejects_over256_in_both_finite_paths(){
  let original=source(false);let blobs=blobs();for suffix in [false,true]{let mut b=build(&original,&blobs,false);
   let result=if suffix{b.suffix(0,500)}else{b.finite(0,500,&[])};assert_eq!(result.unwrap_err(),"bounded scoped region overflow");
  }
 }
}
