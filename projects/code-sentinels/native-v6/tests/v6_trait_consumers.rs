//! Isolated trait-consumer rule fixtures; these do not claim earned campaign progression.
use sentinels_v6::{catalog,Building,Command,Game,Link,NetworkStore,Order,Pos,Room,Shipment,Unit};
use serde_json::json;
fn game()->Game{let mut g=Game::new(21,false);g.state.terrain.fill(0);g.next_id=10000;g}
fn unit(id:u64,kind:&str,p:Pos)->Unit{let d=catalog::unit_ref(kind).unwrap();serde_json::from_value(json!({"id":id,"owner":1,"kind":kind,"pos":p,"x":p.x as f64+0.5,"y":p.y as f64+0.5,"z":p.level,"tier":d.tier,"hp":1000.,"maxHp":1000.,"battery":120.,"batteryMax":240.,"covered":false,"wired":false,"ammo":0.,"route":[],"target":null,"cooldown":999.,"skillCooldown":0.,"plugins":[],"statuses":{},"invested":d.cost,"moving":false,"attackCount":0,"fuel":120.,"fuelMax":120.,"branch":d.branch})).unwrap()}
fn room(id:u64,kind:&str,branch:Option<&str>)->Room{serde_json::from_value(json!({"id":id,"shell":900,"owner":1,"rect":{"x":20,"y":40,"z":0,"w":4,"h":2},"kind":kind,"branch":branch,"tier":2,"hp":280.,"maxHp":280.,"powered":true,"connected":true,"online":true,"capacity":0,"gpus":[],"inventory":0.,"progress":1.,"buildTime":1.,"cooldown":0.})).unwrap()}
fn bank(g:&mut Game,p:Pos,amount:f64){let anchor=Pos::new(p.x+1,p.y,p.level);let cells=(p.x..=p.x+8).flat_map(|x|(p.y..=p.y+2).map(move|y|Pos::new(x,y,p.level))).collect();g.state.networkStores.push(NetworkStore{owner:1,anchor,cells,compute:amount,capacity:1000.,production:0.});g.state.links.push(Link{unitEndpoints:g.state.units.iter().filter(|u|u.pos==p&&catalog::unit_ref(&u.kind).is_some_and(|d|d.category=="ai")).map(|u|u.id).collect(),id:990,owner:1,kind:"compute".into(),path:vec![p,anchor],hp:150.,active:true,invested:0.});g.summarize_compute();}
fn order(g:&mut Game,command:Command){let result=g.order(Order{owner:1,sequence:g.sequences[0]+1,command});assert!(result.accepted,"{}",result.reason);}
#[test]
fn formal_vision_passive_and_plugin_change_actual_visibility_without_rounding_up(){
    let mut g=game();g.state.units.push(unit(800,"kimi",Pos::new(50,48,0)));g.refresh_fog();assert!(g.visible_to(1,Pos::new(76,48,0)));assert!(!g.visible_to(1,Pos::new(77,48,0)));
    g.state.units[0]=unit(800,"deepseek",Pos::new(50,48,0));g.refresh_fog();assert!(!g.visible_to(1,Pos::new(70,48,0)));g.state.units[0].tier=4;g.state.units[0].plugins.push("algorithm-support".into());g.refresh_fog();assert!(g.visible_to(1,Pos::new(70,48,0)));assert!(!g.visible_to(1,Pos::new(71,48,0)));
}
#[test]
fn movement_plugin_changes_paid_units_real_step_distance(){
    let mut a=game();let mut u=unit(801,"kimi",Pos::new(50,40,0));u.goal=Some(Pos::new(51,40,0));u.route=vec![Pos::new(51,40,0)];a.state.units.push(u);let mut b=a.clone();b.state.units[0].plugins.push("speed-core".into());a.step();b.step();assert!(((b.state.units[0].x-50.5)/(a.state.units[0].x-50.5)-1.2).abs()<1e-8);
}
#[test]
fn security_core_mitigates_network_damage_only(){
    let mut g=game();let mut u=unit(802,"claude",Pos::new(50,40,0));u.plugins.push("security-core".into());g.state.units.push(u);g.damage(802,100.,"network",2);assert!((g.state.units[0].hp-917.5).abs()<1e-8);g.damage(802,100.,"kinetic",2);assert!((g.state.units[0].hp-817.5).abs()<1e-8);
}
#[test]
fn ally_armor_uses_one_maximum_bonus_for_units_and_cargo_never_buildings(){
    let mut g=game();for (id,y) in [(803,40),(804,42)]{let mut u=unit(id,"gpt",Pos::new(20,y,0));u.tier=4;u.plugins.push("security-support".into());g.state.units.push(u);}g.state.units.push(unit(805,"deepseek",Pos::new(22,40,0)));
    let shipment:Shipment=serde_json::from_value(json!({"id":806,"owner":1,"from":1,"to":2,"pos":{"x":22,"y":41,"z":0},"route":[],"amount":20.,"hp":1000.,"progress":0.,"cargo":"ammo"})).unwrap();g.state.shipments.push(shipment);
    let building:Building=serde_json::from_value(json!({"id":807,"owner":1,"kind":"core","rect":{"x":21,"y":38,"z":0,"w":2,"h":2},"tier":1,"hp":1000.,"maxHp":1000.,"progress":1.,"buildTime":1.,"powered":true,"connected":false,"power":0.,"demand":0.,"capacity":0,"branch":null,"inventory":0.,"invested":0.,"jam":0.,"shield":0.,"born":0})).unwrap();g.state.buildings.push(building);
    g.damage(805,100.,"kinetic",2);assert!((g.state.units.iter().find(|u|u.id==805).unwrap().hp-915.).abs()<1e-8);g.damage(806,100.,"kinetic",2);assert!((g.state.shipments[0].hp-915.).abs()<1e-8);g.damage(807,100.,"kinetic",2);assert!((g.state.buildings.iter().find(|b|b.id==807).unwrap().hp-900.).abs()<1e-8);
}
#[test]
fn science_core_increases_capacity_without_manufacturing_energy_or_compute(){
    let mut g=game();let d=catalog::units().into_iter().find(|d|d.branch=="science"&&d.category=="vehicle").unwrap();let mut u=unit(810,&d.id,Pos::new(20,40,0));u.tier=2;u.battery=0.;u.batteryMax=0.;u.energy=50.;u.energyMax=100.;g.state.units.push(u);g.player_mut(1).unwrap().branches.insert("science".into(),2);bank(&mut g,Pos::new(20,40,0),100.);order(&mut g,Command::Plugin{id:810,plugin:"science-core".into()});let u=&g.state.units[0];assert_eq!(u.batteryMax,120.);assert_eq!(u.battery,0.);assert_eq!(u.energyMax,120.);assert_eq!(u.energy,50.);
}
#[test]
fn modular_workshop_quote_is_read_only_and_only_credits_receive_the_discount(){
    let mut g=game();let mut u=unit(811,"glm",Pos::new(20,40,0));u.battery=80.;u.batteryMax=300.;g.state.units.push(u);g.player_mut(1).unwrap().branches.insert("lightweight".into(),2);g.state.rooms.push(room(812,"modular-workshop",None));bank(&mut g,Pos::new(20,40,0),0.);let cash=g.player(1).unwrap().credits;let meta=catalog::plugin_ref("lightweight-core").unwrap();let quote=g.snapshot(Some(1));assert_eq!(quote.units[0].pluginDiscount,0.1);assert_eq!(g.state.units[0].pluginDiscount,0.);order(&mut g,Command::Plugin{id:811,plugin:"lightweight-core".into()});assert!((cash-g.player(1).unwrap().credits-meta.cost*0.9).abs()<1e-8);assert!((g.state.units[0].battery-(80.-meta.compute_cost)).abs()<1e-8);
}
#[test]
fn glm_deploy_uses_light_cache_capacity_but_keeps_initial_compute_at_120(){
    let mut g=game();g.player_mut(1).unwrap().branches.insert("lightweight".into(),2);g.state.rooms.push(room(820,"research-lab",Some("lightweight")));bank(&mut g,Pos::new(20,40,0),200.);order(&mut g,Command::Deploy{room:820,kind:"glm".into(),pos:Pos::new(20,40,0)});let glm=g.state.units.iter().find(|u|u.kind=="glm").unwrap();assert_eq!(glm.batteryMax,300.);assert_eq!(glm.battery,120.);
}

#[test]
fn wire_order_binds_only_explicit_friendly_ai_at_a_real_endpoint(){
    let mut g=game();let pos=Pos::new(20,40,0);g.state.units.push(unit(850,"deepseek",pos));g.state.units.push(unit(851,"scout-buggy",pos));let mut enemy=unit(852,"claude",pos);enemy.owner=2;g.state.units.push(enemy);g.refresh_fog();
    let path=vec![pos,Pos::new(21,40,0)];let cash=g.player(1).unwrap().credits;
    for (kind,bindings) in [("compute",vec![851]),("compute",vec![852]),("compute",vec![850,850]),("power",vec![850])]{let result=g.order(Order{owner:1,sequence:g.sequences[0]+1,command:Command::Wire{kind:kind.into(),path:path.clone(),unit_endpoints:bindings}});assert!(!result.accepted);assert_eq!(g.player(1).unwrap().credits,cash);assert!(g.state.links.is_empty());}
    order(&mut g,Command::Wire{kind:"compute".into(),path:path.clone(),unit_endpoints:vec![]});assert!(g.state.links.last().unwrap().unitEndpoints.is_empty());assert!(!g.state.units.iter().find(|u|u.id==850).unwrap().wired);
    order(&mut g,Command::Wire{kind:"compute".into(),path,unit_endpoints:vec![850]});assert_eq!(g.state.links.last().unwrap().unitEndpoints,vec![850]);assert!(g.state.units.iter().find(|u|u.id==850).unwrap().wired);assert!(!g.state.units.iter().find(|u|u.id==851).unwrap().wired);
    let saved=g.save();assert!(Game::load(saved).is_ok());g.state.units.iter_mut().find(|u|u.id==850).unwrap().hp=0.;g.cleanup_dead();assert!(g.state.links.iter().all(|l|l.unitEndpoints.is_empty()));
}
