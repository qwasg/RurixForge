//! Isolated flat-layout fixtures using actual paid room commands, not balance evidence.
use sentinels_v6::{catalog, Command, Game, Order, Pos, Rect, Room};

fn order(g: &mut Game, command: Command) {
    let receipt=g.order(Order{owner:1,sequence:g.sequences[0]+1,command});
    assert!(receipt.accepted,"{}",receipt.reason);
}

fn complete_jobs(g: &mut Game) {
    for _ in 0..3600 {
        if g.state.jobs.is_empty() { return; }
        g.step();
    }
    panic!("ordinary construction did not complete: {:?}",g.state.jobs);
}

fn layout() -> (Game,u64) {
    let mut g=Game::new(603201,false);
    g.state.terrain.fill(0);
    let rect=Rect{x:2,y:38,level:0,width:6,height:4};
    order(&mut g,Command::Shell{rect});complete_jobs(&mut g);
    let shell=g.state.buildings.iter().find(|b|b.owner==1&&b.kind=="shell").unwrap().id;
    order(&mut g,Command::Entrance{pos:Pos::new(2,41,0),to_level:0,kind:"door".into(),width:1});
    (g,shell)
}

fn room(g: &mut Game,shell:u64,x:i32,width:i32,height:i32) -> u64 {
    let rect=Rect{x,y:38,level:0,width,height};
    order(g,Command::Room{shell,rect,kind:"data-center".into(),branch:None});
    g.state.rooms.iter().find(|r|r.owner==1&&r.rect==rect).unwrap().id
}

fn get(g: &Game,id:u64) -> &Room {
    g.state.rooms.iter().find(|r|r.id==id).unwrap()
}

fn attempt(g: &mut Game,command:Command) -> bool {
    g.order(Order{owner:1,sequence:g.sequences[0]+1,command}).accepted
}

fn window(g: &mut Game,x:i32,y:i32) -> u64 {
    order(g,Command::Entrance{pos:Pos::new(x,y,0),to_level:0,kind:"window".into(),width:1});
    g.state.entrances.iter().find(|e|e.pos==Pos::new(x,y,0)&&e.kind=="window").unwrap().id
}

fn bpc(g: &Game,id:u64) -> (u32,u32,u32) {
    let r=get(g,id);(r.capacityBudget.unwrap(),r.potentialCapacity.unwrap(),r.capacity)
}

fn split(g: &mut Game,id:u64) -> Vec<u64> {
    order(g,Command::SplitRoom{id,axis:"x".into(),offset:2});
    g.state.rooms.iter().filter(|r|r.owner==1).map(|r|r.id).collect()
}

fn separate_six_cell_rooms() -> (Game,u64,u64) {
    let (mut g,shell)=layout();let a=room(&mut g,shell,2,2,3);let b=room(&mut g,shell,4,2,3);
    complete_jobs(&mut g);(g,a,b)
}

#[test]
fn newly_built_room_hp_grows_sublinearly_from_four_cell_baseline() {
    println!("compiled rules {} {}",sentinels_v6::RULES_VERSION,sentinels_v6::RULES_FINGERPRINT);
    let (mut g,shell)=layout();let id=room(&mut g,shell,2,4,2);
    let r=g.state.rooms.iter().find(|r|r.id==id).unwrap();
    let expected=140.*2_f64.powf(0.85);
    assert!((r.maxHp-expected).abs()<1e-8,"8-cell room actual maxHp={}, expected sublinear{}",r.maxHp,expected);
    assert!((r.hp-r.maxHp*0.1).abs()<1e-8);
    g.damage(id,expected*0.05,"kinetic",2);
    complete_jobs(&mut g);
    assert!((get(&g,id).hp-expected*0.95).abs()<1e-7,"construction must not erase damage received while building");
    assert!((get(&g,id).maxHp-expected).abs()<1e-8);
}

#[test]
fn two_separately_paid_six_cell_rooms_cannot_gain_a_third_slot_by_merging() {
    let (mut g,shell)=layout();let a=room(&mut g,shell,2,2,3);let b=room(&mut g,shell,4,2,3);
    complete_jobs(&mut g);
    let old=g.state.rooms.iter().filter(|r|r.id==a||r.id==b).map(|r|r.capacity).sum::<u32>();
    assert_eq!(old,2);
    order(&mut g,Command::MergeRooms{ids:vec![a,b]});
    let merged=g.state.rooms.iter().find(|r|r.id==a).unwrap();
    assert_eq!(merged.capacity,old,"merging net6+6 must not fabricate a newly rounded-up slot");
    assert_eq!(bpc(&g,a),(2,3,2));
}

#[test]
fn hp_baseline_and_marginal_area_growth_are_sublinear() {
    let rect=Rect{x:2,y:38,level:0,width:2,height:2};
    assert_eq!(Game::room_hp(rect),140.);
    let mut previous_hp=140.;let mut previous_density=35.;
    for (width,height) in [(4,2),(4,4),(8,4),(8,8),(24,24)] {
        let r=Rect{width,height,..rect};let hp=Game::room_hp(r);
        assert!(hp>previous_hp && hp/(r.area() as f64)<previous_density);
        previous_hp=hp;previous_density=hp/r.area() as f64;
    }
}

#[test]
fn split_remainders_preserve_purchased_budget_hp_and_investment() {
    let (mut g,shell)=layout();let id=room(&mut g,shell,2,4,3);complete_jobs(&mut g);
    g.damage(id,40.,"kinetic",2);
    let original=get(&g,id).clone();assert_eq!(bpc(&g,id),(3,3,3));
    for _ in 0..3 {
        let ids=split(&mut g,id);
        let parts:Vec<_>=ids.iter().map(|id|get(&g,*id)).collect();
        assert_eq!(parts.iter().map(|r|r.capacityBudget.unwrap()).sum::<u32>(),3);
        assert_eq!(parts.iter().map(|r|r.capacity).sum::<u32>(),2);
        assert_eq!(parts.iter().map(|r|r.capacityBudget.unwrap()).collect::<Vec<_>>(),vec![1,2]);
        for (actual,expected) in [(parts.iter().map(|r|r.hp).sum::<f64>(),original.hp),
            (parts.iter().map(|r|r.maxHp).sum::<f64>(),original.maxHp),
            (parts.iter().map(|r|r.invested).sum::<f64>(),original.invested)] {
            assert!((actual-expected).abs()<1e-8);
        }
        order(&mut g,Command::MergeRooms{ids});
        assert_eq!(bpc(&g,id),(3,3,3));
        assert!((get(&g,id).hp-original.hp).abs()<1e-8);
        assert!((get(&g,id).maxHp-original.maxHp).abs()<1e-8);
    }
}

#[test]
fn entrance_refresh_cannot_round_merged_budget_up_again() {
    let (mut g,a,b)=separate_six_cell_rooms();
    order(&mut g,Command::MergeRooms{ids:vec![a,b]});
    assert_eq!(bpc(&g,a),(2,3,2));
    window(&mut g,7,41); // Same shell, outside the merged room.
    assert_eq!(bpc(&g,a),(2,3,2));
    for _ in 0..2 { order(&mut g,Command::InstallGpu{room:a,model:"rtx-5060".into()}); }
    let credits=g.player(1).unwrap().credits;
    assert!(!attempt(&mut g,Command::InstallGpu{room:a,model:"rtx-5060".into()}));
    assert_eq!(g.player(1).unwrap().credits,credits);
}

#[test]
fn temporary_entry_occupancy_restores_budget_only_after_real_removal() {
    let (mut g,shell)=layout();let id=room(&mut g,shell,2,4,2);complete_jobs(&mut g);
    assert_eq!(bpc(&g,id),(2,2,2));
    let entry=window(&mut g,3,38);assert_eq!(bpc(&g,id),(2,1,1));
    order(&mut g,Command::Recycle{id:entry});
    assert!(!g.state.entrances.iter().any(|e|e.id==entry));
    assert_eq!(bpc(&g,id),(2,2,2));
    order(&mut g,Command::Entrance{pos:Pos::new(3,38,0),to_level:0,kind:"door".into(),width:1});
    let door=g.state.entrances.iter().find(|e|e.pos==Pos::new(3,38,0)&&e.kind=="door").unwrap().id;
    order(&mut g,Command::Recycle{id:door});
    assert!(g.state.entrances.iter().any(|e|e.id==door&&e.hp==0.&&e.reserves_space()));
    assert_eq!(bpc(&g,id),(2,1,1),"destroyed door opening still occupies space");
}

#[test]
fn entrance_and_partition_cannot_hide_installed_gpu_overflow() {
    let (mut g,shell)=layout();let id=room(&mut g,shell,2,4,3);complete_jobs(&mut g);
    for _ in 0..3 { order(&mut g,Command::InstallGpu{room:id,model:"rtx-5060".into()}); }
    let saved=get(&g,id).clone();let credits=g.player(1).unwrap().credits;
    assert!(!attempt(&mut g,Command::SplitRoom{id,axis:"x".into(),offset:2}));
    assert!(!attempt(&mut g,Command::Entrance{pos:Pos::new(3,38,0),to_level:0,kind:"window".into(),width:1}));
    assert_eq!(bpc(&g,id),(3,3,3));assert_eq!(get(&g,id).gpus,saved.gpus);
    assert_eq!(g.player(1).unwrap().credits,credits);
}

#[test]
fn same_kind_paid_refit_expands_budget_without_healing() {
    let (mut g,a,b)=separate_six_cell_rooms();order(&mut g,Command::MergeRooms{ids:vec![a,b]});
    g.damage(a,30.,"kinetic",2);let original=get(&g,a).clone();let credits=g.player(1).unwrap().credits;
    order(&mut g,Command::ConvertRoom{id:a,kind:"data-center".into(),branch:None});
    assert!((credits-g.player(1).unwrap().credits-78.).abs()<1e-8);
    assert_eq!(bpc(&g,a),(3,3,3));assert_eq!(get(&g,a).hp,original.hp);
    assert_eq!(get(&g,a).maxHp,original.maxHp);
    assert!((get(&g,a).invested-original.invested-78.).abs()<1e-8);
    complete_jobs(&mut g);
    assert_eq!(get(&g,a).hp,original.hp);assert_eq!(get(&g,a).maxHp,original.maxHp);
}

#[test]
fn same_kind_refit_keeps_already_paid_but_obstructed_budget() {
    let (mut g,shell)=layout();let id=room(&mut g,shell,2,4,3);complete_jobs(&mut g);
    let entry=window(&mut g,3,38);assert_eq!(bpc(&g,id),(3,2,2));
    order(&mut g,Command::ConvertRoom{id,kind:"data-center".into(),branch:None});
    assert_eq!(bpc(&g,id),(3,2,2));complete_jobs(&mut g);
    order(&mut g,Command::Recycle{id:entry});assert_eq!(bpc(&g,id),(3,3,3));
}

#[test]
fn convert_cancel_restores_history_but_respects_new_entry_occupancy_and_damage() {
    let (mut g,shell)=layout();let id=room(&mut g,shell,2,4,2);complete_jobs(&mut g);
    let original=get(&g,id).clone();let credits=g.player(1).unwrap().credits;
    order(&mut g,Command::ConvertRoom{id,kind:"depot".into(),branch:None});
    assert_eq!(bpc(&g,id),(160,160,160));
    window(&mut g,3,38);g.damage(id,15.,"kinetic",2);
    order(&mut g,Command::Cancel{id});
    let restored=get(&g,id);assert_eq!(restored.kind,"data-center");
    assert_eq!(bpc(&g,id),(2,1,1));assert_eq!(restored.invested,original.invested);
    assert_eq!(restored.maxHp,original.maxHp);assert!((restored.hp-(original.hp-15.)).abs()<1e-8);
    assert!(g.player(1).unwrap().credits<credits);
    let cash=g.player(1).unwrap().credits;assert!(!attempt(&mut g,Command::Cancel{id}));assert_eq!(g.player(1).unwrap().credits,cash);
}

#[test]
fn cancelled_new_room_has_no_remaining_hp_budget_or_duplicate_refund() {
    let (mut g,shell)=layout();let credits=g.player(1).unwrap().credits;
    let id=room(&mut g,shell,2,4,2);order(&mut g,Command::Cancel{id});
    assert!(!g.state.rooms.iter().any(|r|r.id==id));assert!(!g.state.jobs.iter().any(|j|j.target==id));
    let cash=g.player(1).unwrap().credits;assert!(cash<credits);
    assert!(!attempt(&mut g,Command::Cancel{id}));assert_eq!(g.player(1).unwrap().credits,cash);
}

#[test]
fn repair_uses_purchased_historical_max_hp_without_recomputing_area_curve() {
    let (mut g,a,b)=separate_six_cell_rooms();order(&mut g,Command::MergeRooms{ids:vec![a,b]});
    let maximum=get(&g,a).maxHp;
    assert!(maximum>Game::room_hp(get(&g,a).rect),"independently purchased HP remains after merge");
    g.damage(a,maximum*0.8,"kinetic",2);
    let investment=get(&g,a).invested;let credits=g.player(1).unwrap().credits;
    let core=g.state.buildings.iter().find(|b|b.owner==1&&b.kind=="core").unwrap().id;
    let stock=g.state.buildings.iter().find(|b|b.id==core).unwrap().stock["repair"];
    order(&mut g,Command::Repair{id:a});assert!((credits-g.player(1).unwrap().credits-20.).abs()<1e-8);
    assert_eq!(g.state.buildings.iter().find(|b|b.id==core).unwrap().stock["repair"],stock-10.);
    complete_jobs(&mut g);assert!((get(&g,a).hp-maximum*0.55).abs()<1e-7);
    assert_eq!(get(&g,a).maxHp,maximum);assert_eq!(get(&g,a).invested,investment);
}

#[test]
fn full_save_load_preserves_split_remainder_and_rejects_invalid_capacity() {
    let (mut g,shell)=layout();let id=room(&mut g,shell,2,4,3);complete_jobs(&mut g);let ids=split(&mut g,id);
    let saved=g.save();let restored=Game::load(saved.clone()).unwrap();
    assert_eq!(serde_json::to_value(&g.state).unwrap(),serde_json::to_value(&restored.state).unwrap());
    assert_eq!(restored.state.rooms.iter().map(|r|r.capacityBudget.unwrap()).sum::<u32>(),3);
    let mut legacy=serde_json::to_value(&saved).unwrap();
    for value in legacy["snapshot"]["rooms"].as_array_mut().unwrap(){value.as_object_mut().unwrap().remove("capacityBudget");value.as_object_mut().unwrap().remove("potentialCapacity");}
    let conservative=Game::load(serde_json::from_value(legacy).unwrap()).unwrap();
    assert_eq!(conservative.state.rooms.iter().map(|r|r.capacityBudget.unwrap()).sum::<u32>(),2,"missing metadata means stored usable capacity, not a geometric or unlimited new budget");
    for mutation in 0..4 {
        let mut bad=saved.clone();let r=&mut bad.snapshot.rooms[0];
        match mutation{0=>r.capacity=2,1=>r.capacityBudget=Some(0),2=>r.potentialCapacity=Some(99),_=>r.gpus=vec!["rtx-5060".into();2]}
        assert!(Game::load(bad).is_err(),"mutation{mutation}");
    }
    let mut merged=restored;order(&mut merged,Command::MergeRooms{ids});assert_eq!(bpc(&merged,id),(3,3,3));
}

#[test]
fn partial_replica_keeps_native_capacity_when_hidden_entries_are_omitted() {
    let (mut g,shell)=layout();let id=room(&mut g,shell,2,4,2);complete_jobs(&mut g);window(&mut g,3,38);
    let mut snapshot=g.state.clone();snapshot.entrances.clear();
    assert_eq!(bpc(&g,id),(2,1,1));assert!(snapshot.validate(true).is_ok());
    assert!(snapshot.validate(false).is_err(),"full authority must include actual occupancy and matching metadata");
}

#[test]
fn pending_conversion_snapshot_validates_its_before_room_budget() {
    let (mut g,shell)=layout();let id=room(&mut g,shell,2,4,2);complete_jobs(&mut g);
    order(&mut g,Command::ConvertRoom{id,kind:"depot".into(),branch:None});
    let saved=g.save();assert!(Game::load(saved.clone()).is_ok());
    let mut bad=saved;bad.snapshot.jobs.iter_mut().find(|j|j.target==id).unwrap().beforeRoom.as_mut().unwrap().capacityBudget=Some(0);
    assert!(Game::load(bad).is_err());
}

#[test]
fn ordinary_paid_refit_replays_its_exact_budget_and_hp_history() {
    // No terrain/resource/time mutations: this separate scenario can use real replay.
    let mut g=Game::new(63,false);let shell_rect=Rect{x:13,y:46,level:0,width:6,height:4};
    order(&mut g,Command::Shell{rect:shell_rect});complete_jobs(&mut g);
    let shell=g.state.buildings.iter().find(|b|b.owner==1&&b.kind=="shell").unwrap().id;
    order(&mut g,Command::Entrance{pos:Pos::new(13,49,0),to_level:0,kind:"door".into(),width:1});
    for x in [13,15]{order(&mut g,Command::Room{shell,rect:Rect{x,y:46,level:0,width:2,height:3},kind:"data-center".into(),branch:None});}
    complete_jobs(&mut g);let ids=g.state.rooms.iter().filter(|r|r.owner==1).map(|r|r.id).collect::<Vec<_>>();let id=ids[0];
    order(&mut g,Command::MergeRooms{ids});assert_eq!(bpc(&g,id),(2,3,2));
    order(&mut g,Command::ConvertRoom{id,kind:"data-center".into(),branch:None});
    let pending=g.save();assert!(Game::replay(&pending).unwrap());
    complete_jobs(&mut g);assert_eq!(bpc(&g,id),(3,3,3));
    let saved=g.save();assert!(Game::replay(&saved).unwrap());
    let restored=Game::load(saved).unwrap();assert_eq!(serde_json::to_value(&g.state).unwrap(),serde_json::to_value(&restored.state).unwrap());
    assert_eq!(catalog::facility_ref("data-center").unwrap().cost,120.);
}
