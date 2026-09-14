//! Ordinary-command progression and explicit malformed-save/rule fixtures.
//! These are compiled now; execution remains gated by the user's system policy.
use sentinels_v6::{catalog,Command,Game,Order,Pos,Rect,Receipt};
#[path="support/paid_opening.rs"]mod support;
use support::{cmd,wait,opening};
fn attempt(g:&mut Game,command:Command)->Receipt{let sequence=g.sequences[0]+1;g.order(Order{owner:1,sequence,command})}
fn cash(g:&Game)->f64{g.player(1).unwrap().credits}
fn shell(g:&mut Game,rect:Rect)->u64{cmd(g,1,Command::Shell{rect});g.state.buildings.iter().find(|b|b.owner==1&&b.kind=="shell"&&b.rect==rect).unwrap().id}
fn fitted_room(g:&mut Game,kind:&str,rect:Rect)->u64{
 let footprint=Rect{width:6,height:4,..rect};let id=shell(g,footprint);wait(g,20);
 cmd(g,1,Command::Entrance{pos:Pos::new(rect.x,rect.y+2,rect.level),to_level:rect.level,kind:"door".into(),width:1});
 cmd(g,1,Command::Room{shell:id,rect,kind:kind.into(),branch:None});wait(g,25);
 g.state.rooms.iter().find(|r|r.shell==id&&r.kind==kind).unwrap().id
}
#[test]fn physical_wall_and_short_wire_recycle_cannot_print_money(){
 let mut g=Game::new(61,false);let before=cash(&g);
 cmd(&mut g,1,Command::Wall{kind:"physical".into(),path:vec![Pos::new(3,38,0)]});let wall=g.state.walls.last().unwrap().id;
 cmd(&mut g,1,Command::Recycle{id:wall});assert!(cash(&g)<=before,"wall refund exceeds price");
 let after=cash(&g);assert!(!attempt(&mut g,Command::Recycle{id:wall}).accepted);assert_eq!(cash(&g),after);
 let before=cash(&g);cmd(&mut g,1,Command::Wire{unit_endpoints:vec![],kind:"power".into(),path:vec![Pos::new(4,38,0),Pos::new(5,38,0)]});let wire=g.state.links.last().unwrap().id;
 cmd(&mut g,1,Command::Recycle{id:wire});assert!(cash(&g)<=before,"short wire refund exceeds construction cost");
}
#[test]fn cancelling_queued_building_has_one_bounded_refund(){
 let mut g=Game::new(62,false);let before=cash(&g);let id=shell(&mut g,Rect{x:2,y:38,level:0,width:4,height:4});
 assert!(cash(&g)<before);cmd(&mut g,1,Command::Cancel{id});let after=cash(&g);assert!(after<=before);
 assert!(!attempt(&mut g,Command::Cancel{id}).accepted);assert_eq!(cash(&g),after);
}
#[test]fn split_merge_preserves_gpu_hp_and_refund_value(){
 let mut g=opening();let id=fitted_room(&mut g,"data-center",Rect{x:2,y:38,level:0,width:4,height:2});
 cmd(&mut g,1,Command::InstallGpu{room:id,model:"rtx-5060".into()});cmd(&mut g,1,Command::InstallGpu{room:id,model:"rtx-5060".into()});
 let before=g.state.rooms.iter().find(|r|r.id==id).unwrap().clone();let before_cash=cash(&g);
 // Establish what the unsplit room would refund, using the identical state in a clone.
 let mut unsplit=g.clone();cmd(&mut unsplit,1,Command::Recycle{id});let direct_refund=cash(&unsplit)-before_cash;
 cmd(&mut g,1,Command::SplitRoom{id,axis:"x".into(),offset:2});
 let parts:Vec<_>=g.state.rooms.iter().filter(|r|r.shell==before.shell&&r.kind=="data-center").cloned().collect();assert_eq!(parts.len(),2);
 assert_eq!(parts.iter().map(|r|r.capacity).sum::<u32>(),before.capacity);assert_eq!(parts.iter().map(|r|r.gpus.len()).sum::<usize>(),before.gpus.len());
 assert!((parts.iter().map(|r|r.hp).sum::<f64>()-before.hp).abs()<0.0001);assert!(cash(&g)<=before_cash);
 let mut split_sale=g.clone();let start=cash(&split_sale);for part in &parts{cmd(&mut split_sale,1,Command::Recycle{id:part.id});}
 assert!(cash(&split_sale)-start<=direct_refund+0.001,"splitting duplicated the room's refund");
 cmd(&mut g,1,Command::MergeRooms{ids:parts.iter().map(|r|r.id).collect()});let merged=g.state.rooms.iter().find(|r|r.id==id).unwrap();assert_eq!(merged.gpus.len(),2);assert_eq!(merged.capacity,before.capacity);assert!((merged.hp-before.hp).abs()<0.0001);
}
#[test]fn incoming_different_cargo_shares_actual_depot_capacity(){
 let mut g=opening();let depot=fitted_room(&mut g,"depot",Rect{x:2,y:38,level:0,width:2,height:2});let core=g.state.buildings.iter().find(|b|b.owner==1&&b.kind=="core").unwrap().id;
 let r=g.state.rooms.iter().find(|r|r.id==depot).unwrap();let def=catalog::facility_ref("depot").unwrap();let limit=(r.capacity as f64/def.capacity_per_area*def.stock_per_area).floor();
 let occupied=r.stock.values().sum::<f64>();let amount=(limit-occupied).min(60.);assert!(amount>0.);
 let original=g.state.buildings.iter().find(|b|b.id==core).unwrap().stock.clone();
 cmd(&mut g,1,Command::Supply{mode:"ground".into(),from:core,to:depot,amount,cargo:"ammo".into()});
 let denied=attempt(&mut g,Command::Supply{mode:"ground".into(),from:core,to:depot,amount:limit,cargo:"fuel".into()});assert!(!denied.accepted);
 let source=&g.state.buildings.iter().find(|b|b.id==core).unwrap().stock;assert!((source.get("ammo").unwrap()-(original.get("ammo").unwrap()-amount)).abs()<0.0001);assert_eq!(source.get("fuel"),original.get("fuel"));
}
#[test]fn research_branches_are_parallel_without_unlocking_other_factions(){
 let mut g=opening();let base=g.state.buildings.iter().find(|b|b.owner==1&&b.kind=="shell").unwrap().id;
 cmd(&mut g,1,Command::Room{shell:base,rect:Rect{x:15,y:46,level:0,width:2,height:2},kind:"research-lab".into(),branch:Some("security".into())});wait(&mut g,30);
 let algorithm=g.state.rooms.iter().find(|r|r.kind=="research-lab"&&r.branch.as_deref()==Some("algorithm")).unwrap().id;
 let security=g.state.rooms.iter().find(|r|r.kind=="research-lab"&&r.branch.as_deref()==Some("security")).unwrap().id;
 // One 5060 has 300 capacity, but running labs spend upkeep after recharge.
 // Start the first real 150-C job, replenish the tiny remaining shortfall,
 // then start the other while the first is still genuinely researching.
 while cash(&g)<800.||g.player(1).unwrap().compute<150.{wait(&mut g,1);assert!(g.state.tick<600*60,"opening economy cannot fund the parallel research budget");}
 let first_started=g.state.tick;
 cmd(&mut g,1,Command::Research{room:algorithm,branch:"algorithm".into()});
 while g.player(1).unwrap().compute<150.{wait(&mut g,1);assert!(g.state.tick-first_started<60*60,"second research did not overlap the first");}
 cmd(&mut g,1,Command::Research{room:security,branch:"security".into()});assert_eq!(g.player(1).unwrap().researches.len(),2);wait(&mut g,70);
 assert_eq!(g.tech(1,"algorithm"),2);assert_eq!(g.tech(1,"security"),2);assert_eq!(g.tech(1,"science"),0);
 let money=cash(&g);assert!(!attempt(&mut g,Command::Deploy{room:algorithm,kind:"gemini".into(),pos:Pos::new(15,48,0)}).accepted);assert_eq!(cash(&g),money);
}
#[test]fn splitting_cannot_bypass_function_minimum_area_or_round_capacity_up(){
 let mut g=opening();let lab=g.state.rooms.iter().find(|r|r.kind=="research-lab").unwrap().id;wait(&mut g,20);
 cmd(&mut g,1,Command::Research{room:lab,branch:"algorithm".into()});wait(&mut g,70);
 while cash(&g)<900.{wait(&mut g,1);assert!(g.state.tick<600*60,"ordinary income cannot fund the test facility");}
 let id=fitted_room(&mut g,"airfield",Rect{x:2,y:38,level:0,width:6,height:2});
 let before=g.state.rooms.iter().find(|r|r.id==id).unwrap().clone();let money=cash(&g);
 assert!(!attempt(&mut g,Command::SplitRoom{id,axis:"x".into(),offset:2}).accepted,"split created under-size functional airfield");
 let after=g.state.rooms.iter().find(|r|r.id==id).unwrap();assert_eq!(after.rect,before.rect);assert_eq!(after.capacity,before.capacity);assert_eq!(cash(&g),money);
}
#[test]fn paid_t2_construction_connects_basement_and_upper_floor(){
 let mut g=opening();let lab=g.state.rooms.iter().find(|r|r.kind=="research-lab").unwrap().id;wait(&mut g,20);cmd(&mut g,1,Command::Research{room:lab,branch:"algorithm".into()});wait(&mut g,70);assert_eq!(g.tech(1,"algorithm"),2);
 let upper=Rect{x:13,y:46,level:1,width:6,height:4};shell(&mut g,upper);
 cmd(&mut g,1,Command::Entrance{pos:Pos::new(15,48,0),to_level:1,kind:"stairs".into(),width:1});wait(&mut g,30);
 let basement=Rect{level:-1,..upper};cmd(&mut g,1,Command::Excavate{rect:basement});wait(&mut g,30);shell(&mut g,basement);
 cmd(&mut g,1,Command::Entrance{pos:Pos::new(15,49,0),to_level:-1,kind:"stairs".into(),width:1});wait(&mut g,40);
 assert!(g.state.buildings.iter().filter(|b|b.rect==upper||b.rect==basement).all(|b|b.progress>=1.));
 assert!(g.route(Pos::new(12,48,0),Pos::new(15,49,-1),"courier").is_some());assert!(g.route(Pos::new(15,49,-1),Pos::new(15,48,1),"courier").is_some());
 while cash(&g)<catalog::unit_ref("deepseek").unwrap().cost+50.||g.player(1).unwrap().compute<120.{wait(&mut g,1);assert!(g.state.tick<900*60,"ordinary economy unable to fund cross-floor unit");}
 cmd(&mut g,1,Command::Deploy{room:lab,kind:"deepseek".into(),pos:Pos::new(14,48,0)});let unit=g.state.units.iter().find(|u|u.kind=="deepseek").unwrap().id;
 cmd(&mut g,1,Command::Move{ids:vec![unit],pos:Pos::new(15,49,-1)});wait(&mut g,10);assert_eq!(g.state.units.iter().find(|u|u.id==unit).unwrap().pos.level,-1);
 cmd(&mut g,1,Command::Move{ids:vec![unit],pos:Pos::new(15,48,1)});wait(&mut g,12);assert_eq!(g.state.units.iter().find(|u|u.id==unit).unwrap().pos.level,1);
}
#[test]fn malformed_save_rejected_without_loading_partial_world(){
 // These deliberately malformed values test the loader, not earned gameplay.
 let valid=opening().save();assert!(Game::load(valid.clone()).is_ok());
 let mut wrong=valid.clone();wrong.snapshot.width=127;assert!(Game::load(wrong).is_err());
 let mut wrong=valid.clone();wrong.snapshot.terrain.pop();assert!(Game::load(wrong).is_err());
 let mut wrong=valid.clone();wrong.snapshot.players[0].credits=f64::NAN;assert!(Game::load(wrong).is_err());
 let mut wrong=valid.clone();wrong.snapshot.units[0].owner=9;assert!(Game::load(wrong).is_err());
 let mut wrong=valid.clone();wrong.snapshot.units[0].pos.level=20;assert!(Game::load(wrong).is_err());
 let mut wrong=valid.clone();wrong.snapshot.rooms[0].id=wrong.snapshot.buildings[0].id;assert!(Game::load(wrong).is_err());
 let mut wrong=valid;wrong.nextId=0;assert!(Game::load(wrong).is_err());
}
