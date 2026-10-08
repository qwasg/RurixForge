//! Isolated motion/visibility/save fixtures. Paid cross-floor replay is also
//! covered by gameplay::paid_floor_transit_refuses_wire_without_losing_motion_or_save_validity.
use sentinels_v6::{catalog,Command,DashState,Game,Order,Pos,Unit};
use serde_json::json;
fn actor(id:u64,owner:u32,kind:&str,pos:Pos)->Unit{
    let d=catalog::unit_ref(kind).unwrap();
    serde_json::from_value(json!({"id":id,"owner":owner,"kind":kind,"pos":pos,"x":pos.x as f64+0.5,"y":pos.y as f64+0.5,"z":pos.level,"tier":d.tier,"hp":d.hp,"maxHp":d.hp,"battery":120.,"batteryMax":240.,"covered":false,"wired":false,"ammo":d.ammo_capacity,"ammoMax":d.ammo_capacity,"energy":d.energy_capacity,"energyMax":d.energy_capacity,"fuel":d.fuel_capacity,"fuelMax":d.fuel_capacity,"route":[],"target":null,"cooldown":999.,"skillCooldown":0.,"plugins":[],"statuses":{},"invested":d.cost,"moving":false,"attackCount":0,"branch":d.branch,"facing":4,"altitude":0.,"flightState":"ground","sourceFacility":0})).unwrap()
}
fn fixture()->Game{
    let mut g=Game::new(73,false);g.state.terrain.fill(0);g.state.units.push(actor(100000,1,"algorithm-scout",Pos::new(20,40,0)));g.next_id=100010;g.refresh_fog();g
}
fn order(g:&mut Game,owner:u32,command:Command){let r=g.order(Order{owner,sequence:g.sequences[(owner-1)as usize]+1,command});assert!(r.accepted,"{}",r.reason);}
fn measured_step(g:&mut Game,id:u64)->(f64,f64){
    let u=g.state.units.iter().find(|u|u.id==id).unwrap();let before=(u.x,u.y,u.level);g.step();let u=g.state.units.iter().find(|u|u.id==id).unwrap();
    let expected=if u.level==before.2{((u.x-before.0)*60.,(u.y-before.1)*60.)}else{(0.,0.)};
    assert!((u.velocityX-expected.0).abs()<1e-9);assert!((u.velocityY-expected.1).abs()<1e-9);(u.velocityX,u.velocityY)
}
#[test]
fn velocity_tracks_actual_turning_and_stopping_not_the_requested_goal(){
    let mut g=fixture();order(&mut g,1,Command::Move{ids:vec![100000],pos:Pos::new(25,40,0)});
    assert_eq!((g.state.units[0].velocityX,g.state.units[0].velocityY),(0.,0.),"issuing future intent is not movement");
    let v=measured_step(&mut g,100000);assert!(v.0>0.);assert_eq!(v.1,0.);
    let p=g.state.units[0].pos;order(&mut g,1,Command::Move{ids:vec![100000],pos:Pos::new(p.x,p.y-2,0)});let v=measured_step(&mut g,100000);assert!(v.1<0.);
    order(&mut g,1,Command::Stop{ids:vec![100000]});assert_eq!(measured_step(&mut g,100000),(0.,0.));
}
#[test]
fn dash_observation_uses_real_accelerated_displacement(){
    let mut g=fixture();g.state.units[0]=actor(100000,1,"kimi",Pos::new(20,40,0));g.refresh_fog();
    order(&mut g,1,Command::Move{ids:vec![100000],pos:Pos::new(24,40,0)});
    g.state.units[0].dash=Some(DashState{previous:[20.5,40.5],hitTargets:vec![],remaining:1.,damage:1.,width:1.});
    let v=measured_step(&mut g,100000);assert!((v.0-catalog::unit_ref("kimi").unwrap().speed*3.).abs()<1e-8);assert_eq!(v.1,0.);
}
#[test]
fn visible_enemy_velocity_is_public_but_future_routes_and_hidden_enemies_are_not(){
    let mut g=fixture();g.state.units.push(actor(100001,2,"algorithm-scout",Pos::new(24,40,0)));g.state.units.push(actor(100002,2,"algorithm-scout",Pos::new(110,70,0)));g.refresh_fog();
    order(&mut g,2,Command::Move{ids:vec![100001],pos:Pos::new(28,40,0)});measured_step(&mut g,100001);g.refresh_fog();
    let actual=g.state.units.iter().find(|u|u.id==100001).unwrap();let view=g.snapshot(Some(1));let seen=view.units.iter().find(|u|u.id==100001).unwrap();
    assert_eq!((seen.velocityX,seen.velocityY),(actual.velocityX,actual.velocityY));assert!(seen.velocityX>0.);assert!(seen.route.is_empty());assert!(seen.queuedGoals.is_empty());assert!(seen.goal.is_none());assert!(!view.units.iter().any(|u|u.id==100002));
}
#[test]
fn signed_motion_round_trips_and_nonfinite_values_are_rejected(){
    let mut g=fixture();assert_eq!(g.state.units[0].velocityX,0.);assert_eq!(g.state.units[0].velocityY,0.);
    g.state.units[0].velocityX=-3.;g.state.units[0].velocityY=2.;let save=g.save();let loaded=Game::load(save.clone()).unwrap();assert_eq!((loaded.state.units[0].velocityX,loaded.state.units[0].velocityY),(-3.,2.));
    for bad in [f64::NAN,f64::INFINITY,f64::NEG_INFINITY]{let mut broken=save.clone();broken.snapshot.units[0].velocityX=bad;assert!(Game::load(broken).is_err());}
    let mut old=serde_json::to_value(&g.state.units[0]).unwrap();old.as_object_mut().unwrap().remove("velocityX");old.as_object_mut().unwrap().remove("velocityY");let decoded:Unit=serde_json::from_value(old).unwrap();assert_eq!((decoded.velocityX,decoded.velocityY),(0.,0.));
}
