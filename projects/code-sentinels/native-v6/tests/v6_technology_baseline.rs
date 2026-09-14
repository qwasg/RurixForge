//! Planned ordinary-command baselines against an idle opponent, not a balanced
//! match oracle. Every elapsed tick, mineral delivery and research payment is real.
use sentinels_v6::{catalog,Command,Game,Order,Pos,Rect};
use serde_json::{json,Value};
use std::{collections::BTreeMap,path::PathBuf};
#[path="support/paid_opening.rs"]mod support;
struct Plan{g:Game,nodes:usize,label:String,milestones:Vec<Value>,payments:Vec<Value>,waits:BTreeMap<String,u64>,samples:Vec<Value>,seen_nodes:Vec<u64>,tiers:BTreeMap<String,u32>,stop:String}
impl Plan{
 fn new(nodes:usize)->Self{let g=support::opening();let tiers=g.player(1).unwrap().branches.clone();Self{g,nodes,label:format!("{nodes}-node-planned"),milestones:vec![],payments:vec![],waits:BTreeMap::new(),samples:vec![],seen_nodes:vec![],tiers,stop:String::new()}}
 fn tick(&mut self)->Result<(),String>{
  if self.g.state.winner.is_some(){return Err(format!("measurement ended by real victory: {}",self.g.state.winReason));}
  if self.g.state.tick>=55*60*60{return Err("measurement limit55min reached".into());}
  self.g.step();
  if self.g.state.tick%60==0{
   let p=self.g.player(1).unwrap();
   for(branch,tier)in &p.branches{if self.tiers.get(branch)!=Some(tier){self.milestones.push(json!({"kind":"research-complete","branch":branch,"tier":tier,"seconds":self.g.state.tick as f64/60.}));}}
   self.tiers=p.branches.clone();
   for n in self.g.state.resources.iter().filter(|r|r.kind=="node"&&r.owner==1){if !self.seen_nodes.contains(&n.id){self.seen_nodes.push(n.id);self.milestones.push(json!({"kind":"node-controlled","id":n.id,"pos":n.pos,"seconds":self.g.state.tick as f64/60.}));}}
   assert!(self.seen_nodes.len()<=self.nodes,"no third-node shortcut");assert!(p.credits>=0.&&p.compute>=0.&&p.science>=0.);
   if self.g.state.tick%3600==0{self.samples.push(json!({"seconds":self.g.state.tick/60,"credits":p.credits,"compute":p.compute,"capacity":p.computeCapacity,"science":p.science,"production":p.production,"power":p.power,"demand":p.demand,"branches":p.branches,"researches":p.researches,"oreDelivered":p.totals.get("ore-delivered"),"extractors":self.g.state.buildings.iter().filter(|b|b.owner==1&&b.kind=="extractor").count()}));}
  }Ok(())
 }
 fn wait(&mut self,s:u64)->Result<(),String>{for _ in 0..s*60{self.tick()?;}Ok(())}
 fn order(&mut self,c:Command)->Result<(),String>{let before=self.g.player(1).unwrap().clone();let r=self.g.order(Order{owner:1,sequence:self.g.sequences[0]+1,command:c.clone()});let after=self.g.player(1).unwrap();self.payments.push(json!({"seconds":self.g.state.tick as f64/60.,"command":c,"receipt":r,"credits":before.credits-after.credits,"compute":before.compute-after.compute,"science":before.science-after.science}));if r.accepted{Ok(())}else{Err(format!("ordinary command {c:?}: {}",r.reason))}}
 fn paid(&mut self,c:Command)->Result<(),String>{loop{let mut preview=self.g.clone();match preview.execute(1,c.clone()){Ok(())=>return self.order(c),Err(reason)if reason.contains("金币不足")||reason.contains("算力不足")||reason.contains("科研数据不足")=>{*self.waits.entry(reason).or_default()+=1;self.wait(1)?;},Err(reason)=>return Err(format!("preview {c:?}: {reason}"))}}}
 fn room(&self,kind:&str)->u64{self.g.state.rooms.iter().find(|r|r.owner==1&&r.kind==kind).unwrap().id}
 fn route_goal(&self,id:u64,desired:Pos)->Result<Pos,String>{let u=self.g.state.units.iter().find(|u|u.id==id).unwrap();for radius in 0_i32..=6{for y in desired.y-radius..=desired.y+radius{for x in desired.x-radius..=desired.x+radius{let p=Pos::new(x,y,0);if self.g.route(u.pos,p,"vehicle").is_some(){return Ok(p);}}}}Err(format!("no physical scout route near {desired:?}"))}
 fn scout(&mut self,factory:u64,desired:Pos)->Result<u64,String>{let kind=catalog::units().into_iter().find(|d|d.branch=="algorithm"&&d.chassis=="scout").unwrap().id;self.paid(Command::Deploy{room:factory,kind:kind.clone(),pos:Pos::new(6,45,0)})?;let id=self.g.state.units.iter().filter(|u|u.owner==1&&u.kind==kind).max_by_key(|u|u.id).unwrap().id;let goal=self.route_goal(id,desired)?;self.order(Command::Move{ids:vec![id],pos:goal})?;self.wait(3)?;Ok(id)}
 fn wire(&mut self,kind:&str,a:Pos,b:Pos)->Result<(),String>{self.paid(Command::Wire{kind:kind.into(),path:support::line(a,b),unit_endpoints:vec![]})}
 fn extractor_near(&mut self,node:Pos)->Result<(),String>{
  loop{let mut candidates=Vec::new();for y in node.y-4..=node.y+3{for x in node.x-4..=node.x+3{let p=Pos::new(x,y,0);let c=Command::Build{pos:p,kind:"extractor".into()};let mut preview=self.g.clone();if preview.execute(1,c.clone()).is_ok(){candidates.push((p.distance(node),c));}}}candidates.sort_by(|a,b|a.0.total_cmp(&b.0));if let Some((_,c))=candidates.into_iter().next(){return self.order(c);}*self.waits.entry("visible/reachable extractor site and actual credits".into()).or_default()+=1;self.wait(1)?;}
 }
 fn branch_wait(&mut self,branch:&str,tier:u32)->Result<(),String>{while self.g.tech(1,branch)<tier{self.wait(1)?;}Ok(())}
 fn save_report(&self)->Value{json!({"label":self.label,"rulesVersion":sentinels_v6::RULES_VERSION,"rulesFingerprint":sentinels_v6::RULES_FINGERPRINT,"scope":"Planned ordinary-command expansion against an idle human slot; standard2000 initial credits, no injected resources, altered terrain/ownership, winner suppression or tick skipping; not a normal match balance result","permittedNodes":self.nodes,"seconds":self.g.state.tick as f64/60.,"stopReason":self.stop,"winner":self.g.state.winner,"winReason":self.g.state.winReason,"milestones":self.milestones,"payments":self.payments,"blockedSeconds":self.waits,"minuteSamples":self.samples,"players":self.g.state.players,"resources":self.g.state.resources})}
}
impl Drop for Plan{fn drop(&mut self){let dir=PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../game/v6/technology-baseline-20260911").join(&self.label);let _=std::fs::create_dir_all(&dir);let _=std::fs::write(dir.join("report.json"),serde_json::to_vec_pretty(&self.save_report()).unwrap());let _=std::fs::write(dir.join("save.json"),serde_json::to_vec(&self.g.save()).unwrap());}}
fn develop(p:&mut Plan)->Result<(),String>{
 let lab=p.room("research-lab");p.paid(Command::Research{room:lab,branch:"algorithm".into()})?;
 let rect=Rect{x:2,y:38,width:6,height:4,level:0};p.paid(Command::Shell{rect})?;p.paid(Command::Build{pos:Pos::new(8,38,0),kind:"wind-power".into()})?;p.wait(20)?;
 let shell=p.g.state.buildings.iter().find(|b|b.owner==1&&b.kind=="shell"&&b.rect==rect).unwrap().id;
 p.paid(Command::Entrance{pos:Pos::new(3,41,0),to_level:0,kind:"door".into(),width:3})?;
 p.paid(Command::Room{shell,rect:Rect{x:3,y:38,width:4,height:2,level:0},kind:"factory".into(),branch:None})?;p.wait(25)?;
 p.wire("power",Pos::new(8,38,0),Pos::new(3,38,0))?;p.wire("power",Pos::new(8,38,0),Pos::new(13,42,0))?;
 let factory=p.room("factory");p.scout(factory,Pos::new(64,48,0))?;
 let explorer=p.scout(factory,Pos::new(40,38,0))?;
 p.extractor_near(Pos::new(44,34,0))?;
 if p.nodes==2{let goal=p.route_goal(explorer,Pos::new(64,24,0))?;p.order(Command::Move{ids:vec![explorer],pos:goal})?;}
 p.branch_wait("algorithm",2)?;
 let dc=p.room("data-center");p.paid(Command::RemoveGpu{room:dc,bay:0})?;p.paid(Command::InstallGpu{room:dc,model:"rtx-5070".into()})?;
 let primary_shell=p.g.state.rooms.iter().find(|r|r.id==dc).unwrap().shell;
 p.paid(Command::Room{shell:primary_shell,rect:Rect{x:15,y:48,width:2,height:2,level:0},kind:"data-center".into(),branch:None})?;
 if p.nodes==1{p.paid(Command::Room{shell:primary_shell,rect:Rect{x:17,y:48,width:2,height:2,level:0},kind:"data-synthesis".into(),branch:None})?;}
 p.wait(25)?;
 for kind in ["power","compute"]{p.wire(kind,Pos::new(17,46,0),Pos::new(17,49,0))?;p.wire(kind,Pos::new(17,49,0),Pos::new(15,49,0))?;}
 p.paid(Command::Research{room:lab,branch:"algorithm".into()})?;p.branch_wait("algorithm",3)?;
 p.paid(Command::Build{pos:Pos::new(17,52,0),kind:"wind-power".into()})?;p.wait(10)?;p.wire("power",Pos::new(17,52,0),Pos::new(17,49,0))?;
 let spare=p.g.state.rooms.iter().find(|r|r.owner==1&&r.kind=="data-center"&&r.id!=dc).unwrap().id;p.paid(Command::InstallGpu{room:spare,model:"a100".into()})?;
 for tier in [4,5]{p.paid(Command::Research{room:lab,branch:"algorithm".into()})?;p.branch_wait("algorithm",tier)?;}
 if p.nodes==1{
  // The new lab adds20 real load: existing450 power/440 load has only10 spare.
  // Pay for the fourth turbine and its physical wire before the second lab.
  p.paid(Command::Build{pos:Pos::new(13,54,0),kind:"wind-power".into()})?;p.wait(10)?;p.wire("power",Pos::new(13,54,0),Pos::new(17,52,0))?;
  p.paid(Command::Room{shell:primary_shell,rect:Rect{x:15,y:46,width:2,height:2,level:0},kind:"research-lab".into(),branch:Some("security".into())})?;p.wait(25)?;
  let second=p.g.state.rooms.iter().find(|r|r.owner==1&&r.branch.as_deref()==Some("security")).unwrap().id;
  for tier in [2,3,4,5]{p.paid(Command::Research{room:second,branch:"security".into()})?;p.branch_wait("security",tier)?;}
 }
 Ok(())
}
fn measure(nodes:usize){let mut p=Plan::new(nodes);let result=develop(&mut p);p.stop=result.as_ref().err().cloned().unwrap_or_else(||"planned target tiers completed".into());if let Err(e)=result{assert!(e.starts_with("measurement"),"{e}");}let save=p.g.save();assert_eq!(serde_json::to_value(Game::load(save.clone()).unwrap().state).unwrap(),serde_json::to_value(&p.g.state).unwrap());assert!(Game::replay(&save).unwrap());println!("{}",p.save_report());}
#[test]fn one_node_with_paid_synthesis_measures_first_and_second_t5(){measure(1);}
#[test]fn two_ordinary_nodes_measure_first_t5_without_suppressing_victory(){measure(2);}
