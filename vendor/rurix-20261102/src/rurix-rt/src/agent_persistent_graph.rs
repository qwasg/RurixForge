//! Concrete lifetime bridge for the next production integration. Compile-only
//! in the first frozen v32 diagnostic; it does not silently adopt a new epoch.
//! The recipe lives outside the frame loop. Bound buffers/passes borrow it, and
//! DeviceFrameSession borrows BoundGraph plus barrier_refs for the whole run.
use super::*;
use std::collections::BTreeMap;
pub struct Extra {pub bytes:u64,pub device_local:bool,pub initial:Option<Vec<u8>>}
pub struct Kernel {pub name:String,pub spirv:std::sync::Arc<[u8]>,pub bindings:Bindings,pub groups:[u32;3]}
pub enum Source {Original(usize),Tail(usize)}
pub struct Recipe {pub device_local_queue:bool,pub queue_capacity:u32,pub extras:Vec<Extra>,pub kernels:Vec<Kernel>,pub schedule:Vec<Source>,pub source_epoch:[u8;32],pub sealed_original_bindings:BTreeMap<usize,[u8;32]>}
pub struct BoundGraph<'a>{pub resources:Vec<ResourceDesc<'a>>,pub passes:Vec<Pass<'a>>,pub barrier_storage:Vec<Vec<(u32,TargetState)>>,pub original_to_augmented:Vec<u32>,pub original_resource_count:usize,pub source_epoch:[u8;32],pub sealed_original_bindings:BTreeMap<usize,[u8;32]>}
// RFC-0052 / RXS-0346: preserve actual GPU-generated rain dispatch only.
fn has_indirect(passes:&[Pass<'_>])->bool{passes.iter().any(|p|matches!(p,Pass::Compute(ComputePass{dispatch:DispatchSpec::Indirect{..},..})))}
fn dispatch_text(d:DispatchSpec)->String{match d{DispatchSpec::Direct(g)=>format!("direct:{},{},{}",g[0],g[1],g[2]),DispatchSpec::Indirect{res,offset}=>format!("indirect:{res}:{offset}")}}
pub(super) fn dispatch_source_chain(resources:&[ResourceDesc<'_>],passes:&[Pass<'_>],barriers:&[&[(u32,TargetState)]])->Result<Option<[u8;32]>,String>{
 if !has_indirect(passes){return Ok(None)}
 if passes.len()!=barriers.len(){return Err("dispatch barrier census".into())}
 let mut chain=[0u8;32];let mut add=|row:String|{let mut bytes=chain.to_vec();bytes.extend_from_slice(row.as_bytes());chain=rurix_pkg::sha256::digest(&bytes);};
 for(i,r)in resources.iter().enumerate(){let ResourceDesc::Buffer(b)=r else{return Err("dispatch source buffer profile".into())};if !b.usage.storage||b.usage.uniform||b.usage.vertex{return Err("dispatch source closed SSBO usage".into())}add(format!("resource:{i}|{}|{}|{}|{}|{}\n",b.size,b.usage.storage,b.usage.uniform,b.usage.vertex,b.usage.indirect));}
 for(i,p)in passes.iter().enumerate(){let Pass::Compute(c)=p else{return Err("dispatch source compute profile".into())};let plan=barriers[i].iter().map(|(r,t)|format!("{r}:{t:?}")).collect::<Vec<_>>().join(",");add(format!("{i}|{}|{plan}\n",dispatch_text(c.dispatch)));}Ok(Some(chain))
}
fn validate_source_dispatch(resources:&[ResourceDesc<'_>],passes:&[Pass<'_>],barriers:&[&[(u32,TargetState)]])->Result<(),String>{
 let mut count=0usize;
 for(i,p)in passes.iter().enumerate(){let Pass::Compute(c)=p else{continue};if let DispatchSpec::Indirect{res,offset}=c.dispatch{
  count+=1;let b=&c.bindings;
  if c.name!="hq_rain_pack"||offset!=0||b.storage_buffers.len()!=3||b.storage_buffers[0]!=res||!b.accel_structs.is_empty(){return Err("unregistered original indirect rain dispatch".into())}
  if b.storage_buffers[0]==b.storage_buffers[1]||b.storage_buffers[0]==b.storage_buffers[2]||b.storage_buffers[1]==b.storage_buffers[2]{return Err("rain pack binding alias".into())}
  let Some(ResourceDesc::Buffer(args))=resources.get(res as usize)else{return Err("rain args buffer absent".into())};
  if args.size!=32||args.device_local||!args.usage.storage||!args.usage.indirect{return Err("rain indirect args shape/usage".into())}
  if !matches!(barriers[i].last(),Some((r,TargetState::IndirectRead))if *r==res)||!barriers[i].iter().any(|(r,t)|*r==res&&matches!(t,TargetState::ShaderRead)){return Err("rain indirect/shader barrier closure".into())}
  let producers=passes[..i].iter().filter(|p|matches!(p,Pass::Compute(c)if c.name=="hq_rain_indirect_args"&&c.bindings.storage_buffers.len()==4&&c.bindings.storage_buffers[2]==res&&matches!(c.dispatch,DispatchSpec::Direct(_)))).count();if producers!=1{return Err("rain args producer absent/duplicate".into())}
 }}if count>1{return Err("duplicate original rain pack".into())}Ok(())
}

// Closed role/stride map, independent of memory allocation length.
pub(super) fn queue_internal_stride(i:usize)->Option<u64>{
    const S:[u64;54]=[64,40,128,64,512,64,256,128,128,256,4,4,12,16,256,4,256,4,4,4,64,128,48,48,4,16,4,4,256,4,4,4,12,16,256,4,4,4,4,4,4,16,4,64,64,4,16,4,4,4,4,64,4,4];
    if matches!(i,7..=38|40..=47|52..=53){Some(S[i])}else{None}
}
fn validate_queue_extra(i:usize,e:&Extra,device_local_queue:bool,capacity:u32)->Result<(),String>{
    if e.bytes==0||e.initial.as_ref().is_some_and(|b|b.len()as u64!=e.bytes){return Err("invalid extra initialization extent".into())}
    if device_local_queue {
        if !matches!(capacity,4096|65536){return Err("unknown closed queue residency capacity".into())}
        if let Some(stride)=queue_internal_stride(i){
            if !e.device_local||e.bytes!=stride*u64::from(capacity)||!e.initial.as_ref().is_some_and(|b|b.iter().all(|&v|v==0)){
                return Err("closed device queue requires exact full source zero upload".into())
            }
            return Ok(())
        }
    }
    if e.device_local&&e.initial.is_some(){return Err("unauthorized initialized device-local extra".into())}
    Ok(())
}

impl Recipe {
    pub fn bind<'a>(&'a self,resources:&[ResourceDesc<'a>],passes:&[Pass<'a>],barriers:&[&[(u32,TargetState)]],expected_epoch:[u8;32])->Result<BoundGraph<'a>,String>{
        if expected_epoch!=self.source_epoch||passes.len()!=barriers.len(){return Err("graph epoch/barrier census mismatch".into())}
        validate_source_dispatch(resources,passes,barriers)?;
        let mut rs=resources.to_vec();
        if self.device_local_queue&&self.extras.len()<54{return Err("closed queue residency role census".into())}
        for (i,e) in self.extras.iter().enumerate() {validate_queue_extra(i,e,self.device_local_queue,self.queue_capacity)?;rs.push(ResourceDesc::Buffer(BufferDesc{size:e.bytes,usage:BufferUsage{storage:true,..Default::default()},data:e.initial.as_deref(),device_local:e.device_local}));}
        let mut ps=vec![];let mut bs=vec![];let mut map=vec![];
        for source in &self.schedule{match *source{
            Source::Original(i)=>{if i!=map.len(){return Err("original pass duplicate/gap/order".into())}map.push(u32::try_from(ps.len()).map_err(|_|"physical pass overflow")?);ps.push(passes.get(i).ok_or("source pass absent")?.clone());bs.push(barriers[i].to_vec());}
            Source::Tail(i)=>{let k=self.kernels.get(i).ok_or("tail absent")?;if k.groups.contains(&0)||k.name.is_empty(){return Err("tail ABI".into())}ps.push(Pass::Compute(ComputePass{name:&k.name,spirv:k.spirv.as_ref(),entry:None,dispatch:DispatchSpec::Direct(k.groups),bindings:k.bindings.clone()}));bs.push(vec![]);}
        }}
        if map.len()!=passes.len(){return Err("incomplete full source graph".into())}
        Ok(BoundGraph{resources:rs,passes:ps,barrier_storage:bs,original_to_augmented:map,original_resource_count:resources.len(),source_epoch:self.source_epoch,sealed_original_bindings:self.sealed_original_bindings.clone()})
    }
}
impl BoundGraph<'_>{
    pub fn barrier_refs(&self)->Vec<&[(u32,TargetState)]>{self.barrier_storage.iter().map(Vec::as_slice).collect()}
    pub fn evidence_json(&self)->Result<String,String>{
        let typed=has_indirect(&self.passes);
        let schema=if typed{"rurix.hq.optical_augmented_graph.v2"}else{"rurix.hq.optical_augmented_graph.v1"};
        let mut resources=vec![];for(i,r)in self.resources.iter().enumerate(){let ResourceDesc::Buffer(b)=r else{return Err("optical graph resource must be buffer".into())};resources.push(format!("{{\"index\":{i},\"bytes\":{},\"device_local\":{},\"initial_sha256\":{}{}}}",b.size,b.device_local,b.data.map(|x|format!("\"{}\"",rurix_pkg::sha256::hex_digest(x))).unwrap_or("null".into()),if typed{format!(",\"indirect_usage\":{}",b.usage.indirect)}else{String::new()}));}
        let source=self.original_to_augmented.iter().enumerate().map(|(old,&new)|(new as usize,old)).collect::<BTreeMap<_,_>>();let mut passes=vec![];
        for(i,p)in self.passes.iter().enumerate(){let Pass::Compute(c)=p else{return Err("optical graph compute-only".into())};let command=match c.dispatch{DispatchSpec::Direct(groups)=>if typed{format!("\"dispatch\":{{\"kind\":\"direct\",\"groups\":{groups:?}}}")}else{format!("\"groups\":{groups:?}")},DispatchSpec::Indirect{res,offset}=>format!("\"dispatch\":{{\"kind\":\"indirect\",\"resource\":{res},\"offset\":{offset}}}")};
            let barriers=self.barrier_storage[i].iter().map(|(id,target)|format!("[{},\"{:?}\"]",id,target)).collect::<Vec<_>>().join(",");
            passes.push(format!("{{\"index\":{i},\"original\":{},\"name\":\"{}\",\"spv_sha256\":\"{}\",\"storage\":{:?},\"AS\":{:?},\"push\":{:?},{command},\"barriers\":[{barriers}]}}",source.get(&i).map(|v|v.to_string()).unwrap_or("null".into()),c.name,rurix_pkg::sha256::hex_digest(c.spirv),c.bindings.storage_buffers,c.bindings.accel_structs,c.bindings.push_constants));}
        Ok(format!("{{\"schema\":\"{schema}\",\"source_epoch\":\"{}\",\"original_resource_count\":{},\"old_to_new\":{:?},\"resources\":[{}],\"passes\":[{}]}}",self.source_epoch.iter().map(|x|format!("{x:02x}")).collect::<String>(),self.original_resource_count,self.original_to_augmented,resources.join(","),passes.join(",")))
    }
    pub fn mapped_update_json(&self,update:&FrameUpdate,original:&FrameUpdate,sample:u64,frame:u64)->Result<String,String>{
        let mut effective=self.passes.iter().map(|p|pass_bindings(p).clone()).collect::<Vec<_>>();for &(i,ref b)in &update.binding_overrides{*effective.get_mut(i as usize).ok_or("mapped binding index")?=b.clone();}for &(i,ref bytes)in &update.push_constant_overrides{effective.get_mut(i as usize).ok_or("mapped push index")?.push_constants.clone_from(bytes);}
        let canonical=effective.iter().enumerate().map(|(i,b)|format!("{i}|{:?}|{:?}|{:?}\n",b.storage_buffers,b.accel_structs,b.push_constants)).collect::<String>();
        let rows=update.binding_overrides.iter().map(|(i,b)|format!("{{\"pass\":{i},\"storage\":{:?},\"AS\":{:?},\"push\":{:?}}}",b.storage_buffers,b.accel_structs,b.push_constants)).collect::<Vec<_>>().join(",");
        let pushes=update.push_constant_overrides.iter().map(|(i,p)|format!("[{i},{p:?}]")).collect::<Vec<_>>().join(",");
        let upload_json=|u:&[(StableResourceId,u64,Vec<u8>)]|u.iter().map(|(id,offset,bytes)|{let payload=mapped_upload_payload_json(bytes);format!("{{\"resource\":{},\"offset\":{offset},\"bytes\":{},\"sha256\":\"{}\",\"payload\":{payload}}}",id.0-1,bytes.len(),rurix_pkg::sha256::hex_digest(bytes))}).collect::<Vec<_>>().join(",");
        let uploads=upload_json(&update.buffer_uploads);let original_uploads=upload_json(&original.buffer_uploads);
        let originals=original.binding_overrides.iter().map(|(i,b)|format!("{{\"pass\":{i},\"storage\":{:?},\"AS\":{:?},\"push\":{:?}}}",b.storage_buffers,b.accel_structs,b.push_constants)).collect::<Vec<_>>().join(",");
        let original_push=original.push_constant_overrides.iter().map(|(i,p)|format!("[{i},{p:?}]")).collect::<Vec<_>>().join(",");
        Ok(format!("{{\"schema\":\"rurix.hq.optical_mapped_update.v2\",\"frame\":{frame},\"sample_u64\":{sample},\"binding_overrides\":[{rows}],\"push_overrides\":[{pushes}],\"uploads\":[{uploads}],\"original_uploads\":[{original_uploads}],\"original_binding_overrides\":[{originals}],\"original_push_overrides\":[{original_push}],\"original_readback_subset\":{:?},\"readback_subset\":{:?},\"effective_canonical_sha256\":\"{}\"}}",original.readback_subset.as_ref().ok_or("explicit original readback subset absent")?,update.readback_subset.as_ref().ok_or("explicit optical readback subset absent")?,rurix_pkg::sha256::hex_digest(canonical.as_bytes())))
    }
    /// The original FrameUpdate remains the sole per-frame source. History
    /// parity changes target the mapped ORIGINAL physical passes. No asset,
    /// camera, mip, sample or tail state is synthesized from source hashes.
    pub fn map_update(&self,original:&FrameUpdate)->Result<FrameUpdate,String>{
        if original.tlas_update.is_some()||original.blas_refit.is_some(){return Err("AS regeneration requires a separate before-pending owner adapter".into())}
        for &(i,ref b)in &original.binding_overrides{if self.sealed_original_bindings.get(&(i as usize)).is_some_and(|expected|*expected!=rurix_pkg::sha256::digest(format!("{b:?}").as_bytes())){return Err("tail-dependent source role binding changed without tail rebind certificate".into())}}
        for &(i,ref bytes)in &original.push_constant_overrides{if self.sealed_original_bindings.contains_key(&(i as usize)){let mapped=*self.original_to_augmented.get(i as usize).ok_or("source push index")?;if &pass_bindings(&self.passes[mapped as usize]).push_constants!=bytes{return Err("tail-dependent source role push constants changed".into())}}}
        let mut update=original.clone();let mut seen=BTreeMap::new();
        for (index,_)in &mut update.binding_overrides{let mapped=*self.original_to_augmented.get(*index as usize).ok_or("source binding override index")?;if seen.insert(mapped,()).is_some(){return Err("duplicate mapped override".into())}*index=mapped;}
        for (index,_)in &mut update.push_constant_overrides{*index=*self.original_to_augmented.get(*index as usize).ok_or("source push override index")?;}
        if update.buffer_uploads.iter().any(|(id,_,_)|id.0==0||id.0>self.original_resource_count as u64){return Err("original FrameUpdate cannot write tail controls/state".into())}
        Ok(update)
    }
}

// Append inside agent_persistent_graph.rs for the root-owned native test build.
// CPU-only tests of actual Recipe/BoundGraph, no Vulkan calls or shader execution.
// RFC-0052 / RXS-0346. Assisted-by: Codex:GPT-6.
#[cfg(test)]
mod original_rain_dispatch_tests {
    use super::*;
    fn resources()->Vec<ResourceDesc<'static>> {
        static ARGS:[u8;32]=[0;32];
        (0..4).map(|i|ResourceDesc::Buffer(BufferDesc{size:if i==0{32}else{64},
            usage:BufferUsage{storage:true,indirect:i==0,..Default::default()},
            data:if i==0{Some(&ARGS)}else{None},device_local:false})).collect()
    }
    fn passes()->Vec<Pass<'static>> {
        vec![Pass::Compute(ComputePass{name:"hq_rain_indirect_args",spirv:&[],entry:None,
            dispatch:DispatchSpec::Direct([1,1,1]),bindings:Bindings{storage_buffers:vec![1,2,0,3],..Default::default()}}),
            Pass::Compute(ComputePass{name:"hq_rain_pack",spirv:&[],entry:None,
            dispatch:DispatchSpec::Indirect{res:0,offset:0},bindings:Bindings{storage_buffers:vec![0,1,2],..Default::default()}})]
    }
    fn recipe()->Recipe {
        Recipe{device_local_queue:false,queue_capacity:4096,extras:vec![],kernels:vec![Kernel{name:"cpu_test_tail".into(),spirv:std::sync::Arc::from([]),bindings:Bindings::default(),groups:[1,1,1]}],
            schedule:vec![Source::Original(0),Source::Tail(0),Source::Original(1)],source_epoch:[9;32],sealed_original_bindings:BTreeMap::new()}
    }
    fn plans()->Vec<Vec<(u32,TargetState)>> {vec![vec![(0,TargetState::StorageWrite)],vec![(0,TargetState::ShaderRead),(1,TargetState::ShaderRead),(2,TargetState::StorageWrite),(0,TargetState::IndirectRead)]]}
    #[test]
    fn original_rain_indirect_command_and_barriers_survive_exactly() {
        let r=resources();let p=passes();let b=plans();let br=b.iter().map(Vec::as_slice).collect::<Vec<_>>();let owner=recipe();let graph=owner.bind(&r,&p,&br,[9;32]).unwrap();
        assert_eq!(graph.original_to_augmented,vec![0,2]);assert_eq!(graph.barrier_storage[2],b[1]);
        let Pass::Compute(c)=&graph.passes[2]else{panic!("compute")};assert!(matches!(c.dispatch,DispatchSpec::Indirect{res:0,offset:0}));
        let text=graph.evidence_json().unwrap();assert!(text.contains("rurix.hq.optical_augmented_graph.v2"));assert!(text.contains("\"dispatch\":{\"kind\":\"indirect\",\"resource\":0,\"offset\":0}"));
    }
    #[test]
    fn both_parities_and_reset_upload_keep_fixed_actual_indirect_command() {
        let r=resources();let p=passes();let b=plans();let br=b.iter().map(Vec::as_slice).collect::<Vec<_>>();let owner=recipe();let graph=owner.bind(&r,&p,&br,[9;32]).unwrap();
        for state in [1,3,1] {
            let reset=vec![7u8;32];let original=FrameUpdate{binding_overrides:vec![(1,Bindings{storage_buffers:vec![0,state,2],..Default::default()})],buffer_uploads:vec![(StableResourceId(1),0,reset.clone())],..Default::default()};
            let mapped=graph.map_update(&original).unwrap();assert_eq!(mapped.binding_overrides[0].0,2);assert_eq!(mapped.binding_overrides[0].1.storage_buffers,vec![0,state,2]);assert_eq!(mapped.buffer_uploads[0].2,reset);
            let Pass::Compute(c)=&graph.passes[2]else{panic!("compute")};assert!(matches!(c.dispatch,DispatchSpec::Indirect{res:0,offset:0}));
        }
    }
    #[test]
    fn wrong_command_offset_usage_and_missing_barrier_fail_before_device() {
        let r=resources();let p=passes();let b=plans();let br=b.iter().map(Vec::as_slice).collect::<Vec<_>>();let owner=recipe();
        let mut wrong=p.clone();let Pass::Compute(c)=&mut wrong[1]else{panic!("compute")};c.dispatch=DispatchSpec::Indirect{res:0,offset:4};assert!(owner.bind(&r,&wrong,&br,[9;32]).is_err());
        let mut wrong=r.clone();let ResourceDesc::Buffer(args)=&mut wrong[0]else{panic!("buffer")};args.usage.indirect=false;assert!(owner.bind(&wrong,&p,&br,[9;32]).is_err());
        let mut wrong=b.clone();wrong[1].pop();let refs=wrong.iter().map(Vec::as_slice).collect::<Vec<_>>();assert!(owner.bind(&r,&p,&refs,[9;32]).is_err());
    }
    #[test]
    fn native_dispatch_digest_binds_command_usage_and_barrier_program() {
        let r=resources();let p=passes();let b=plans();let br=b.iter().map(Vec::as_slice).collect::<Vec<_>>();let first=dispatch_source_chain(&r,&p,&br).unwrap().unwrap();
        let mut commands=p.clone();let Pass::Compute(c)=&mut commands[1]else{panic!("compute")};c.dispatch=DispatchSpec::Indirect{res:0,offset:4};assert_ne!(dispatch_source_chain(&r,&commands,&br).unwrap().unwrap(),first);
        let mut usage=r.clone();let ResourceDesc::Buffer(a)=&mut usage[0]else{panic!("buffer")};a.usage.indirect=false;assert_ne!(dispatch_source_chain(&usage,&p,&br).unwrap().unwrap(),first);
        let mut plans=b.clone();plans[1].pop();let refs=plans.iter().map(Vec::as_slice).collect::<Vec<_>>();assert_ne!(dispatch_source_chain(&r,&p,&refs).unwrap().unwrap(),first);
    }
}

// Lossless v2 upload evidence. The codec is chosen by scanning the ACTUAL
// requested bytes; SHA256 below always covers those full uncompressed bytes.
fn mapped_upload_payload_json(bytes:&[u8])->String {
    if bytes.iter().all(|&v|v==0){"{\"encoding\":\"zero_u8_v1\"}".into()}
    else{format!("{{\"encoding\":\"literal_u8_v1\",\"values\":{bytes:?}}}")}
}
#[cfg(test)]mod mapped_upload_codec_tests {
 use super::mapped_upload_payload_json as encode;
 #[test]fn actual_zero_scan_selects_lossless_zero_codec(){assert_eq!(encode(&vec![0;3_686_400]),"{\"encoding\":\"zero_u8_v1\"}");}
 #[test]fn one_nonzero_byte_must_be_literal(){assert_eq!(encode(&[0,0,1,0]),"{\"encoding\":\"literal_u8_v1\",\"values\":[0, 0, 1, 0]}");}
 #[test]fn tail_nonzero_is_not_a_zero_claim(){let mut x=vec![0;8192];x[8191]=255;let s=encode(&x);assert!(s.contains("literal_u8_v1")&&!s.contains("zero_u8_v1")&&s.ends_with("255]}"));}
}

#[cfg(test)]
mod queue_residency_tests {
 use super::*;
 #[test]fn closed_roles_preserve_real_full_zero_payloads(){
  let mut count=0;let mut sum=0;
  for i in 0..60 {if let Some(stride)=queue_internal_stride(i){count+=1;sum+=stride*4096;
   let e=Extra{bytes:stride*4096,device_local:true,initial:Some(vec![0;(stride*4096)as usize])};
   assert!(validate_queue_extra(i,&e,true,4096).is_ok());assert!(validate_queue_extra(i,&e,false,4096).is_err());
  }}assert_eq!(count,42);assert_eq!(sum,8781824);
 }
 #[test]fn missing_partial_nonzero_or_host_payload_reject(){
  let size=4096*4;let mut e=Extra{bytes:size,device_local:true,initial:None};
  assert!(validate_queue_extra(10,&e,true,4096).is_err());e.initial=Some(vec![0;size as usize-1]);assert!(validate_queue_extra(10,&e,true,4096).is_err());
  e.initial=Some(vec![0;size as usize]);e.initial.as_mut().unwrap()[17]=1;assert!(validate_queue_extra(10,&e,true,4096).is_err());
  e.initial.as_mut().unwrap()[17]=0;e.device_local=false;assert!(validate_queue_extra(10,&e,true,4096).is_err());
 }
 #[test]fn wrong_capacity_and_nonqueue_initialized_device_reject(){
  let e=Extra{bytes:4096*4,device_local:true,initial:Some(vec![0;4096*4])};
  for cap in [0,1,4095,65535,65537] {assert!(validate_queue_extra(10,&e,true,cap).is_err());}
  assert!(validate_queue_extra(10,&e,true,65536).is_err());
  for i in [0,1,2,3,4,5,6,39,48,49,50,51,54,55,56,57,58,59] {assert!(validate_queue_extra(i,&e,true,4096).is_err());}
 }
}
