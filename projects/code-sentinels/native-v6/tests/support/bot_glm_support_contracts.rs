//! Unearned tactical fixtures, separate from real case921 and full match evidence.
//! Eligible engagement starts with an actual native paid weapon attack below.
use super::*;
use serde_json::json;

fn actor(id:u64,owner:u32,kind:&str,pos:Pos)->Unit {
    let d=catalog::unit_ref(kind).unwrap();
    serde_json::from_value(json!({"id":id,"owner":owner,"kind":kind,"pos":pos,
        "x":pos.x as f64+0.5,"y":pos.y as f64+0.5,"z":pos.level,"tier":d.tier,
        "hp":d.hp,"maxHp":d.hp,"battery":240.,"batteryMax":300.,"covered":false,"wired":false,
        "ammo":d.ammo_capacity,"ammoMax":d.ammo_capacity,"energy":d.energy_capacity,"energyMax":d.energy_capacity,
        "fuel":d.fuel_capacity,"fuelMax":d.fuel_capacity,"route":[],"target":null,"cooldown":999.,
        "skillCooldown":0.,"plugins":[],"statuses":{},"invested":d.cost,"moving":false,
        "attackCount":0,"branch":d.branch})).unwrap()
}

fn arena(recipient:&str)->Game {
    let mut g=Game::new(75,false);
    g.state.terrain.fill(0);g.state.rooms.clear();g.state.units.clear();
    g.state.links.clear();g.state.walls.clear();g.state.projectiles.clear();g.state.networkStores.clear();
    g.state.visible[0]=(0..96).flat_map(|y|(0..128).map(move|x|Pos::new(x,y,0))).collect();
    g.state.explored[0]=g.state.visible[0].clone();g.state.tick=600;g.state.revision+=1;g.next_id=10_000;
    let home=g.state.buildings.iter_mut().find(|b|b.owner==1&&b.kind=="core").unwrap();
    home.rect.x=36;home.rect.y=44;
    g.state.units.push(actor(201,1,"glm",Pos::new(40,40,0)));
    let mut engaged=actor(202,1,recipient,Pos::new(45,40,0));
    engaged.cooldown=0.;engaged.target=Some(301);g.state.units.push(engaged);
    g.state.units.push(actor(301,2,"light-tank",Pos::new(50,40,0)));
    let mut recovering=actor(204,1,"gpt",Pos::new(42,42,0));recovering.hp=recovering.maxHp*0.6;
    g.state.units.push(recovering);g.invalidate_navigation();
    assert!(g.bot_ai_recovering(g.state.units.iter().find(|u|u.id==204).unwrap()));
    g.advance_combat();
    let ally=g.state.units.iter().find(|u|u.id==202).unwrap();
    assert_eq!(ally.lastAttackTick,Some(600));assert_eq!(ally.attackCount,1);
    assert!(g.state.events.iter().any(|e|e.kind=="fire"&&e.subject==202));
    g
}

fn decision(g:&Game)->Option<Command> {
    let unit=g.state.units.iter().find(|u|u.id==201).unwrap();
    let own=g.state.units.iter().filter(|u|u.owner==1&&u.hp>0.).collect::<Vec<_>>();
    let enemy=g.state.units.iter().filter(|u|u.owner==2&&u.hp>0.&&g.visible_to(1,u.pos)).collect::<Vec<_>>();
    g.bot_ai_action(1,unit,&own,&enemy)
}

fn unchanged_decision(g:&Game)->Option<Command> {
    let before=serde_json::to_value(g.save()).unwrap();let command=decision(g);
    assert_eq!(before,serde_json::to_value(g.save()).unwrap(),"selection must not spend or mutate state");command
}

fn skill_at(command:Option<Command>,expected:Pos)->Command {
    let command=command.expect("valid support action");
    assert!(matches!(&command,Command::Skill{id:201,pos,..} if *pos==expected),"unexpected {command:?}");command
}

fn remove_recovering(g:&mut Game) {g.state.units.retain(|u|u.id!=204);}

fn barrier(g:&mut Game,x:i32,owner:u32) {
    for y in 0..96 {g.state.walls.push(Wall{id:20_000+y as u64,owner,pos:Pos::new(x,y,0),kind:"physical".into(),hp:1000.,maxHp:1000.,shield:0.,invested:0.,antiHeal:0.});}
    g.state.revision+=1;g.invalidate_navigation();
}

#[test]
fn glm_support_prefers_actual_engagement_over_injured_recovery_and_pays_native_cost() {
    println!("test-source rules {} {}",crate::RULES_VERSION,crate::RULES_FINGERPRINT);
    let mut g=arena("deepseek");
    let command=skill_at(unchanged_decision(&g),Pos::new(45,40,0));
    let before=g.state.units[0].battery;let cost=g.active_skill_cost(&g.state.units[0]);
    let receipt=g.order(Order{owner:1,sequence:1,command});assert!(receipt.accepted,"{}",receipt.reason);
    assert!((g.state.units[0].battery-(before-cost)).abs()<1e-9);
    let ally=g.state.units.iter().find(|u|u.id==202).unwrap();assert_eq!(ally.statuses.get("support-boost"),Some(&12.));
    assert!(!g.state.units.iter().find(|u|u.id==204).unwrap().statuses.contains_key("support-boost"));
}

#[test]
fn glm_support_skips_recovering_only_idle_stale_and_future_attack_records() {
    let mut recovering=arena("deepseek");recovering.state.units.retain(|u|u.id!=202);
    assert!(unchanged_decision(&recovering).is_none(),"recovery HP is not support value");
    for last in [None,Some(0),Some(601)] {
        let mut g=arena("deepseek");remove_recovering(&mut g);
        g.state.units.iter_mut().find(|u|u.id==202).unwrap().lastAttackTick=last;
        assert!(unchanged_decision(&g).is_none(),"stale target and nonrecent/future attack {last:?} cannot establish engagement");
    }
}

#[test]
fn glm_support_does_not_refresh_active_boost_or_spend_without_an_unbuffed_recipient() {
    let mut g=arena("deepseek");remove_recovering(&mut g);
    let ally=g.state.units.iter_mut().find(|u|u.id==202).unwrap();ally.hp*=0.8;ally.statuses.insert("support-boost".into(),6.);
    let mut second=actor(205,1,"deepseek",Pos::new(45,42,0));second.target=Some(301);second.cooldown=0.;
    g.state.units.push(second);g.advance_combat();
    assert_eq!(g.state.units.iter().find(|u|u.id==205).unwrap().lastAttackTick,Some(600));
    skill_at(unchanged_decision(&g),Pos::new(45,42,0));
    g.state.units.retain(|u|u.id!=205);
    assert!(unchanged_decision(&g).is_none(),"active buff must not be refreshed merely because its owner is injured");
}

#[test]
fn glm_support_zero_or_expired_boost_keys_do_not_block_a_paid_new_cast() {
    for remaining in [0.,-0.1] {
        let mut g=arena("deepseek");remove_recovering(&mut g);
        g.state.units.iter_mut().find(|u|u.id==202).unwrap().statuses.insert("support-boost".into(),remaining);
        skill_at(unchanged_decision(&g),Pos::new(45,40,0));
    }
}

#[test]
fn glm_support_requires_recipient_local_ammo_energy_or_compute_and_caster_funds() {
    for kind in ["deepseek","light-tank","laser-tank"] {
        let mut g=arena(kind);remove_recovering(&mut g);
        let ally=g.state.units.iter_mut().find(|u|u.id==202).unwrap();ally.battery=0.;ally.ammo=0.;ally.energy=0.;
        g.player_mut(1).unwrap().compute=1_000_000.;
        assert!(unchanged_decision(&g).is_none(),"unconnected global resources cannot pay {kind}'s current attack");
    }
    let mut g=arena("deepseek");remove_recovering(&mut g);
    g.state.units[0].battery=g.active_skill_cost(&g.state.units[0])-1.;
    assert!(unchanged_decision(&g).is_none());
}

#[test]
fn glm_support_needs_present_visible_reachable_threat_and_preserves_hidden_future_privacy() {
    let mut g=arena("deepseek");remove_recovering(&mut g);
    let chosen=serde_json::to_value(unchanged_decision(&g).unwrap()).unwrap();
    let enemy=g.state.units.iter_mut().find(|u|u.id==301).unwrap();enemy.route=vec![Pos::new(50,70,0)];enemy.goal=Some(Pos::new(70,70,0));enemy.target=Some(201);enemy.ammo=0.;enemy.battery=0.;
    g.player_mut(2).unwrap().compute=99_999.;g.player_mut(2).unwrap().credits=1.;
    assert_eq!(chosen,serde_json::to_value(unchanged_decision(&g).unwrap()).unwrap());
    g.state.visible[0].retain(|p|p.x<48);
    assert!(unchanged_decision(&g).is_none(),"old target pointer cannot reveal an unseen threat");
    let mut blocked=arena("deepseek");remove_recovering(&mut blocked);barrier(&mut blocked,48,1);
    assert!(unchanged_decision(&blocked).is_none(),"friendly cover blocks every current hostile weapon line");
    let mut cast_blocked=arena("deepseek");remove_recovering(&mut cast_blocked);barrier(&mut cast_blocked,43,2);
    assert!(unchanged_decision(&cast_blocked).is_none(),"native support cast line still applies");
    let mut floor=arena("deepseek");remove_recovering(&mut floor);
    let ally=floor.state.units.iter_mut().find(|u|u.id==202).unwrap();ally.level=1;ally.pos.level=1;
    assert!(unchanged_decision(&floor).is_none());
}

#[test]
fn glm_support_recent_window_tracks_two_weapon_cycles_without_accepting_old_idle_fire() {
    let mut fast=arena("deepseek");remove_recovering(&mut fast);fast.state.tick+=4*60;
    assert!(unchanged_decision(&fast).is_none());
    let mut slow=arena("missile-truck");remove_recovering(&mut slow);slow.state.tick+=7*60;
    skill_at(unchanged_decision(&slow),Pos::new(45,40,0));
    slow.state.tick+=2*60;assert!(unchanged_decision(&slow).is_none());
}

#[test]
fn glm_support_change_does_not_replace_gpt_injured_ally_healing() {
    let mut g=arena("deepseek");g.state.units[0]=actor(201,1,"gpt",Pos::new(40,40,0));
    let command=skill_at(unchanged_decision(&g),Pos::new(42,42,0));
    let old_hp=g.state.units.iter().find(|u|u.id==204).unwrap().hp;
    let battery=g.state.units[0].battery;let cost=g.active_skill_cost(&g.state.units[0]);
    let receipt=g.order(Order{owner:1,sequence:1,command});assert!(receipt.accepted,"{}",receipt.reason);
    assert!(g.state.units.iter().find(|u|u.id==204).unwrap().hp>old_hp);
    assert!((g.state.units[0].battery-(battery-cost)).abs()<1e-9);
    assert!(g.state.units.iter().all(|u|!u.statuses.contains_key("support-boost")));
}

#[test]
fn glm_support_includes_visible_enemy_core_siege_with_no_enemy_unit_list() {
    let mut g=arena("deepseek");remove_recovering(&mut g);g.state.units.retain(|u|u.id!=301);
    let core=g.state.buildings.iter_mut().find(|b|b.owner==2&&b.kind=="core").unwrap();
    core.rect.x=50;core.rect.y=40;let target=core.id;
    let ally=g.state.units.iter_mut().find(|u|u.id==202).unwrap();ally.target=Some(target);ally.cooldown=0.;
    g.invalidate_navigation();g.advance_combat();
    assert_eq!(g.state.units.iter().find(|u|u.id==202).unwrap().attackCount,2);
    // One real exposed edge remains visible while the building centre is fogged.
    g.state.visible[0].retain(|p|p.x<49||*p==Pos::new(50,40,0));
    skill_at(unchanged_decision(&g),Pos::new(45,40,0));
}

#[test]
fn glm_support_keeps_native_legal_self_support_for_an_engaged_nonrecovering_caster() {
    let mut g=arena("deepseek");g.state.units.retain(|u|u.id==201||u.id==301);
    let caster=g.state.units.iter_mut().find(|u|u.id==201).unwrap();
    caster.hp*=0.8;caster.cooldown=0.;caster.target=Some(301);
    g.advance_combat();assert_eq!(g.state.units[0].lastAttackTick,Some(600));
    assert!(!g.bot_ai_recovering(&g.state.units[0]));
    let command=skill_at(unchanged_decision(&g),Pos::new(40,40,0));
    let before=g.state.units[0].battery;let cost=g.active_skill_cost(&g.state.units[0]);
    let receipt=g.order(Order{owner:1,sequence:1,command});assert!(receipt.accepted,"{}",receipt.reason);
    assert_eq!(g.state.units[0].statuses.get("support-boost"),Some(&12.));
    assert!((g.state.units[0].battery-(before-cost)).abs()<1e-9);
}
