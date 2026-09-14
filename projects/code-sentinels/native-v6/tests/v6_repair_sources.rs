//! Physical routing/inventory fixtures; these do not claim an earned economy.
use sentinels_v6::{catalog,Command,Game,Order,Pos,Rect};
use serde_json::json;
fn fixture()->Game{
    let mut g=Game::new(91,false);g.state.terrain.fill(0);g.state.rooms.clear();g.state.units.clear();g.state.entrances.clear();g.state.links.clear();g.state.walls.clear();g.state.jobs.clear();
    let core=g.state.buildings.iter_mut().find(|b|b.owner==1&&b.kind=="core").unwrap();core.stock.insert("repair".into(),10.);
    let mut shell=core.clone();shell.id=10000;shell.kind="shell".into();shell.rect=Rect{x:13,y:45,level:0,width:6,height:6};shell.power=0.;shell.demand=0.;shell.powered=false;shell.stock.clear();shell.inventory=0.;g.state.buildings.push(shell);
    for(id,x,y)in[(10001,16,48),(10002,14,46)]{g.state.rooms.push(serde_json::from_value(json!({"id":id,"shell":10000,"owner":1,"rect":{"x":x,"y":y,"z":0,"w":2,"h":2},"kind":"depot","branch":null,"tier":1,"hp":280.,"maxHp":280.,"antiHeal":0.,"equipmentShare":1.,"powered":false,"connected":false,"online":false,"maintenance":0.,"capacity":1,"gpus":[],"inventory":10.,"stock":{"repair":10.},"progress":1.,"buildTime":1.,"cooldown":0.,"invested":0.})).unwrap());}
    let d=catalog::unit_ref("vscode").unwrap();g.state.units.push(serde_json::from_value(json!({"id":10003,"owner":1,"kind":"vscode","pos":Pos::new(21,48,0),"x":21.5,"y":48.5,"z":0,"tier":d.tier,"hp":20.8,"maxHp":d.hp,"battery":0.,"batteryMax":0.,"covered":false,"wired":false,"ammo":d.ammo_capacity,"ammoMax":d.ammo_capacity,"fuel":d.fuel_capacity,"fuelMax":d.fuel_capacity,"energy":d.energy_capacity,"energyMax":d.energy_capacity,"route":[],"target":null,"cooldown":999.,"skillCooldown":0.,"plugins":[],"statuses":{},"invested":d.cost,"moving":false,"attackCount":0,"branch":d.branch,"facing":0,"altitude":0.,"flightState":"ground","sourceFacility":0})).unwrap());g.next_id=10004;g.refresh_fog();g
}
fn repair(sequence:u64)->Order{Order{owner:1,sequence,command:Command::Repair{id:10003}}}
#[test]
fn repair_skips_two_closed_near_sources_and_charges_the_reachable_far_source_once(){
    let mut g=fixture();let rect=Rect{x:21,y:48,level:0,width:1,height:1};
    for r in &g.state.rooms{assert!(g.construction_route(r.rect.center(),rect,"repair").is_none());}
    let core=g.state.buildings.iter().find(|b|b.owner==1&&b.kind=="core").unwrap().clone();assert!(g.construction_route(core.rect.center(),rect,"repair").is_some());
    let cash=g.player(1).unwrap().credits;let result=g.order(repair(1));assert!(result.accepted,"{}",result.reason);
    assert_eq!(g.player(1).unwrap().credits,cash-20.);assert_eq!(g.state.buildings.iter().find(|b|b.id==core.id).unwrap().stock["repair"],0.);assert!(g.state.rooms.iter().all(|r|r.stock["repair"]==10.));
    assert_eq!(g.state.jobs.len(),1);assert_eq!(g.state.jobs[0].worker,core.rect.center());assert!(!g.state.jobs[0].route.is_empty());
    let after=serde_json::to_value(&g.state).unwrap();assert!(g.order(repair(1)).accepted);assert_eq!(serde_json::to_value(&g.state).unwrap(),after);
    assert!(!g.order(repair(2)).accepted);assert_eq!(g.player(1).unwrap().credits,cash-20.);assert_eq!(g.state.jobs.len(),1);
}
#[test]
fn all_inaccessible_sources_or_no_materials_leave_money_and_stock_unchanged(){
    let mut g=fixture();g.state.buildings.iter_mut().find(|b|b.owner==1&&b.kind=="core").unwrap().stock.insert("repair".into(),0.);
    let cash=g.player(1).unwrap().credits;let r=g.order(repair(1));assert!(!r.accepted&&r.reason.contains("无可达路线"));assert_eq!(g.player(1).unwrap().credits,cash);assert!(g.state.jobs.is_empty());assert!(g.state.rooms.iter().all(|r|r.stock["repair"]==10.));
    for r in &mut g.state.rooms{r.stock.insert("repair".into(),0.);r.inventory=0.;}
    let r=g.order(repair(2));assert!(!r.accepted&&r.reason.contains("没有维修物资"));assert_eq!(g.player(1).unwrap().credits,cash);assert!(g.state.jobs.is_empty());
}
