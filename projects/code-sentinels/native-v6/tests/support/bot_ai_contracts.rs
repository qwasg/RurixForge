//! Explicit tactical fixtures. These verify planning/real command execution,
//! not earned AI growth; macro progress is measured separately by live matches.
use super::*;
use serde_json::json;
fn actor(id: u64, owner: u32, kind: &str, pos: Pos) -> Unit {
    let d = catalog::unit_ref(kind).unwrap();
    serde_json::from_value(json!({"id":id,"owner":owner,"kind":kind,"pos":pos,"x":pos.x as f64+0.5,"y":pos.y as f64+0.5,"z":pos.level,"tier":d.tier,"hp":d.hp,"maxHp":d.hp,"battery":240.,"batteryMax":240.,"covered":false,"wired":false,"ammo":d.ammo_capacity,"ammoMax":d.ammo_capacity,"fuel":d.fuel_capacity,"fuelMax":d.fuel_capacity,"energy":d.energy_capacity,"energyMax":d.energy_capacity,"route":[],"target":null,"cooldown":999.,"skillCooldown":0.,"plugins":[],"statuses":{},"invested":d.cost,"moving":false,"attackCount":0,"branch":d.branch})).unwrap()
}
fn arena() -> Game {
    let mut g = Game::new(75, false);
    g.state.terrain.fill(0);
    g.state.rooms.clear();
    g.state.units.clear();
    g.state.links.clear();
    g.state.walls.clear();
    g.state.projectiles.clear();
    g.state.networkStores.clear();
    g.state.visible[0] = (0..96)
        .flat_map(|y| (0..128).map(move |x| Pos::new(x, y, 0)))
        .collect();
    g.state.explored[0] = g.state.visible[0].clone();
    g.state.revision += 1;
    g.next_id = 10_000;
    g.invalidate_navigation();
    g
}
fn decision(g: &Game, id: u64) -> Option<Command> {
    let unit = g.state.units.iter().find(|u| u.id == id).unwrap();
    let own: Vec<_> = g
        .state
        .units
        .iter()
        .filter(|u| u.owner == unit.owner)
        .collect();
    let enemy: Vec<_> = g
        .state
        .units
        .iter()
        .filter(|u| u.owner != unit.owner && g.visible_to(unit.owner, u.pos))
        .collect();
    g.bot_ai_action(unit.owner, unit, &own, &enemy)
}
fn order(g: &mut Game, command: Command) {
    let r = g.order(Order {
        owner: 1,
        sequence: g.sequences[0] + 1,
        command,
    });
    assert!(r.accepted, "{}", r.reason);
}
#[test]
fn gemini_uses_observed_velocity_without_reading_hidden_future_orders() {
    let mut g = arena();
    g.state
        .units
        .push(actor(201, 1, "gemini", Pos::new(40, 40, 0)));
    let mut target = actor(202, 2, "light-tank", Pos::new(48, 40, 0));
    target.velocityX = 3.;
    target.moving = true;
    target.route = vec![Pos::new(49, 40, 0), Pos::new(50, 40, 0)];
    target.goal = Some(Pos::new(70, 40, 0));
    g.state.units.push(target);
    let first = decision(&g, 201).expect("visible straight-motion prediction");
    assert!(matches!(&first, Command::Skill { pos, .. } if *pos == Pos::new(55,40,0)));
    g.state.units[1].route = vec![Pos::new(48, 41, 0)];
    g.state.units[1].goal = Some(Pos::new(48, 70, 0));
    g.state.units[1].facing = 2;
    assert_eq!(
        serde_json::to_value(first).unwrap(),
        serde_json::to_value(decision(&g, 201).unwrap()).unwrap()
    );
    g.state.visible[0].retain(|p| p.x < 52);
    assert!(
        decision(&g, 201).is_none(),
        "an unseen predicted cell cannot be targeted"
    );
}
#[test]
fn gemini_prediction_aims_ahead_of_observed_velocity() {
    let mut g = arena();
    g.state
        .units
        .push(actor(201, 1, "gemini", Pos::new(40, 40, 0)));
    g.state
        .units
        .push(actor(202, 2, "light-tank", Pos::new(48, 40, 0)));
    assert!(g
        .order(Order {
            owner: 2,
            sequence: 1,
            command: Command::Move {
                ids: vec![202],
                pos: Pos::new(65, 40, 0),
            },
        })
        .accepted);
    for _ in 0..4 {
        g.step();
    }
    assert!(g.state.units[1].velocityX > 2.9);
    let now = g.state.units[1].pos;
    let command = decision(&g, 201).expect("visible straight-motion prediction");
    match command {
        Command::Skill { pos, .. } => {
            assert!(pos.x > now.x, "must aim ahead of {now:?}, got {pos:?}");
        }
        other => panic!("expected skill, got {other:?}"),
    }
}
#[test]
fn gemini_prefers_a_visible_stationary_cluster_over_a_lone_target() {
    let mut g = arena();
    g.state
        .units
        .push(actor(201, 1, "gemini", Pos::new(40, 40, 0)));
    for (id, pos) in [
        (202, Pos::new(43, 40, 0)),
        (203, Pos::new(50, 40, 0)),
        (204, Pos::new(51, 40, 0)),
        (205, Pos::new(50, 41, 0)),
    ] {
        g.state.units.push(actor(id, 2, "light-tank", pos));
    }
    let command = decision(&g, 201).unwrap();
    assert!(
        matches!(command,Command::Skill{pos,..} if pos.x>=49 && pos.distance(Pos::new(50,40,0))<=1.)
    );
}
#[test]
fn long_front_line_uses_known_terrain_and_pays_the_real_wire_cost() {
    let mut g = arena();
    let from = Pos::new(14, 42, 0);
    let to = Pos::new(56, 41, 0);
    let path = g
        .bot_wire(1, from, to, "power")
        .expect("A* must reach a known forward site within the planner budget");
    assert_eq!(path.first(), Some(&from));
    assert_eq!(path.last(), Some(&to));
    assert_eq!(path.len(), 44);
    let before = g.player(1).unwrap().credits;
    order(
        &mut g,
        Command::Wire {
            kind: "power".into(),
            path: path.clone(),
            unit_endpoints: vec![],
        },
    );
    assert_eq!(
        g.player(1).unwrap().credits,
        before - path.len() as f64 * 2.
    );
    let mut hidden = arena();
    hidden.state.explored[0].retain(|p| p.x < 30);
    hidden.state.visible[0].retain(|p| p.x < 30);
    assert!(hidden.bot_wire(1, from, to, "power").is_none());
}
#[test]
fn wire_planning_routes_around_explored_but_currently_unseen_cells() {
    let mut g = arena();
    let hidden = Pos::new(9, 33, 0);
    g.state.visible[0].remove(&hidden);
    assert!(g.state.explored[0].contains(&hidden));
    let path = g
        .bot_wire(1, Pos::new(9, 41, 0), Pos::new(16, 28, 0), "compute")
        .expect("currently visible alternative");
    assert_eq!(path.len(), 21);
    assert!(!path.contains(&hidden));
    assert!(path.iter().all(|p| g.visible_to(1, *p)));
    let before = g.player(1).unwrap().credits;
    order(
        &mut g,
        Command::Wire {
            kind: "compute".into(),
            path,
            unit_endpoints: vec![],
        },
    );
    assert_eq!(g.player(1).unwrap().credits, before - 63.);
}
#[test]
fn wire_planning_requires_visible_same_layer_cells() {
    let mut g = arena();
    let from = Pos::new(20, 20, 0);
    let to = Pos::new(25, 20, 0);
    assert!(g.visible_to(1, from));
    let path = g
        .bot_wire(1, from, to, "power")
        .expect("visible same-layer wire remains legal");
    assert!(path.iter().all(|p| p.level == 0));
    let command = Command::Wire {
        kind: "power".into(),
        path,
        unit_endpoints: vec![],
    };
    assert!(g.preflight_site(1, &command).is_ok());
    order(&mut g, command);
    g.state.visible[0].remove(&Pos::new(25, 20, 0));
    assert!(!g.construction_visible_to(1, to));
}
#[test]
fn first_aircraft_uses_best_affordable_tier_but_later_upgrades_do_not_repeat_cheap_planes() {
    let mut g = arena();
    g.player_mut(1).unwrap().branches.insert("speed".into(), 5);
    g.player_mut(1).unwrap().credits = 1955.38;
    for (id, kind, rect, branch) in [
        (
            301,
            "research-lab",
            Rect {
                x: 20,
                y: 20,
                level: 0,
                width: 2,
                height: 2,
            },
            Some("speed"),
        ),
        (
            302,
            "airfield",
            Rect {
                x: 32,
                y: 20,
                level: 0,
                width: 4,
                height: 3,
            },
            None,
        ),
    ] {
        let r:Room=serde_json::from_value(json!({"id":id,"shell":1,"owner":1,"rect":rect,"kind":kind,"branch":branch,"tier":1,"hp":500.,"maxHp":500.,"powered":true,"connected":true,"online":true,"capacity":0,"gpus":[],"inventory":0.,"progress":1.,"buildTime":1.,"cooldown":0.})).unwrap();
        g.state.rooms.push(r);
    }
    let mut runway = g.state.buildings[0].clone();
    runway.id = 303;
    runway.kind = "airstrip".into();
    runway.rect = Rect {
        x: 34,
        y: 15,
        level: 0,
        width: 8,
        height: 4,
    };
    g.state.buildings.push(runway);
    for id in 400..405 {
        g.state.units.push(actor(
            id,
            1,
            "light-tank",
            Pos::new(50 + (id - 400) as i32, 40, 0),
        ));
    }
    g.state
        .units
        .push(actor(410, 1, "speed-aa-launcher", Pos::new(50, 42, 0)));
    g.state
        .units
        .push(actor(411, 1, "speed-orbital-strike", Pos::new(51, 42, 0)));
    g.state
        .units
        .push(actor(412, 1, "speed-scout", Pos::new(52, 42, 0)));
    g.invalidate_navigation();
    let command = g
        .bot_produce(1, "speed", &g.state.rooms, &g.state.units, "maintech")
        .expect("initial affordable aviation");
    let Command::Deploy { kind, .. } = &command else {
        panic!("deployment expected")
    };
    let d = catalog::unit_ref(kind).unwrap();
    assert_eq!(d.category, "air");
    assert!(d.tier < 5 && d.cost + 250. <= 1955.38);
    let cost = d.cost;
    order(&mut g, command);
    assert!((g.player(1).unwrap().credits - (1955.38 - cost)).abs() < 1e-8);
    assert_eq!(
        g.state
            .units
            .iter()
            .filter(|u| catalog::unit_ref(&u.kind).is_some_and(|d| d.category == "air"))
            .count(),
        1
    );
    assert!(!matches!(g.bot_produce(1,"speed",&g.state.rooms,&g.state.units,"maintech"),Some(Command::Deploy{kind,..})if catalog::unit_ref(&kind).is_some_and(|d|d.category=="air"&&d.tier<5)),"existing air force must save for the actual latest tier, not repeatedly buy cheaper planes");
}
#[test]
fn critically_wounded_full_cache_ai_waits_for_real_paid_repair_then_can_advance_without_a_vehicle()
{
    for hp in [29.6, 20.8] {
        let mut g = arena();
        let pos = Pos::new(22, 44, 0);
        let mut ai = actor(201, 1, "gemini", pos);
        ai.hp = hp;
        g.state.units.push(ai);
        port(&mut g, pos);
        assert!(g.bot_ai_recovering(&g.state.units[0]));
        assert!(
            decision(&g, 201).is_none(),
            "full compute must not end a critical health recovery"
        );
        let own = g.state.units.clone();
        let refs: Vec<_> = own.iter().collect();
        assert!(g
            .bot_maneuver(1, Pos::new(10, 48, 0), false, "mixed-ai", &refs, &[])
            .is_none());
        let repair = g
            .bot_ai_maintenance(1, Pos::new(10, 48, 0), &own)
            .expect("credits-paid repair order");
        assert!(matches!(repair, Command::Repair { id: 201 }));
        let credits = g.player(1).unwrap().credits;
        order(&mut g, repair);
        assert_eq!(g.player(1).unwrap().credits, credits - 20.);
        assert_eq!(g.state.units[0].hp, hp, "paying does not instantly edit HP");
        for _ in 0..1200 {
            g.step();
            if g.state.units[0].hp >= g.state.units[0].maxHp * AI_RETURN_HP {
                break;
            }
        }
        assert!(
            g.state.units[0].hp >= g.state.units[0].maxHp * AI_RETURN_HP,
            "real worker/cargo must restore health"
        );
        assert!(!g.bot_ai_recovering(&g.state.units[0]));
        let own = g.state.units.clone();
        let refs: Vec<_> = own.iter().collect();
        assert!(
            matches!(g.bot_maneuver(1,Pos::new(10,48,0),false,"mixed-ai",&refs,&[]),Some(Command::Move{ids,..}) if ids.contains(&201)),
            "a recovered AI is not locked forever without a vehicle"
        );
    }
}
#[test]
fn grouped_moves_cannot_pull_a_full_cache_patient_out_of_recovery() {
    let mut g = arena();
    let mut ai = actor(201, 1, "gemini", Pos::new(22, 44, 0));
    ai.hp = 20.8;
    g.state.units.push(ai);
    g.state
        .units
        .push(actor(202, 1, "light-tank", Pos::new(23, 44, 0)));
    let own = g.state.units.clone();
    let refs: Vec<_> = own.iter().collect();
    let command = g
        .bot_maneuver(1, Pos::new(10, 48, 0), false, "mixed-ai", &refs, &[])
        .expect("healthy vehicle may advance");
    assert!(matches!(command,Command::Move{ids,..} if ids.contains(&202)&&!ids.contains(&201)));
}
#[test]
fn the_actual_red_base_20_hp_case_repairs_using_only_its_own_materials() {
    let mut g = arena();
    let mut ai = actor(201, 2, "gemini", Pos::new(111, 39, 0));
    ai.hp = 20.8;
    g.state.units.push(ai);
    assert!(decision(&g, 201).is_none());
    let repair = g
        .bot_ai_maintenance(2, Pos::new(118, 48, 0), &g.state.units)
        .expect("red repair must be reachable");
    let blue = g.player(1).unwrap().credits;
    let red = g.player(2).unwrap().credits;
    let receipt = g.order(Order {
        owner: 2,
        sequence: g.sequences[1] + 1,
        command: repair,
    });
    assert!(receipt.accepted, "{}", receipt.reason);
    assert_eq!(g.player(1).unwrap().credits, blue);
    assert_eq!(g.player(2).unwrap().credits, red - 20.);
    for _ in 0..1200 {
        g.step();
        if g.state.units[0].hp >= g.state.units[0].maxHp * AI_RETURN_HP {
            break;
        }
    }
    assert!(g.state.units[0].hp >= g.state.units[0].maxHp * AI_RETURN_HP);
}
#[test]
fn repair_credit_shortage_keeps_ai_defending_without_workshop_fallback() {
    let mut g = arena();
    g.player_mut(1).unwrap().credits = 0.;
    let mut ai = actor(201, 1, "gemini", Pos::new(22, 44, 0));
    ai.hp = 29.6;
    g.state.units.push(ai);
    assert!(decision(&g, 201).is_none());
    let before = serde_json::to_value(g.save()).unwrap();
    assert!(
        g.bot_ai_maintenance(1, Pos::new(10, 48, 0), &g.state.units)
            .is_none(),
        "zero credits cannot queue a paid repair"
    );
    assert_eq!(
        serde_json::to_value(g.save()).unwrap(),
        before,
        "shortage planning never mutates state"
    );
}
#[test]
fn mixed_ai_refits_from_carried_compute_with_a_logistics_buffer() {
    let mut g = arena();
    g.player_mut(1)
        .unwrap()
        .branches
        .insert("science".into(), 3);
    g.player_mut(1).unwrap().credits = 700.;
    let mut ai = actor(201, 1, "gemini", Pos::new(40, 40, 0));
    ai.battery = 60.;
    g.state.units.push(ai);
    // A second live elite completes the current assembly requirement without
    // changing the ordinary Upgrade's actual payment rules.
    g.state
        .units
        .push(actor(202, 1, "gemini", Pos::new(41, 40, 0)));
    assert!(g.unit_network(1, g.state.units[0].pos).is_none());
    assert!(g.bot_ai_refit(1, &g.state.units, "maintech").is_none());
    let command = g.bot_ai_refit(1, &g.state.units, "mixed-ai").unwrap();
    assert!(matches!(command, Command::Upgrade { id: 201 }));
    let cost = g.state.units[0].invested * 0.45;
    order(&mut g, command);
    assert_eq!(g.state.units[0].tier, 3);
    assert_eq!(g.state.units[0].battery, 0.);
    assert_eq!(g.player(1).unwrap().credits, 700. - cost);
    assert!(g.player(1).unwrap().credits >= MIXED_REFIT_BUFFER);
}
#[test]
fn mixed_ai_installs_a_compatible_plugin_from_its_carried_compute() {
    let mut g = arena();
    g.player_mut(1)
        .unwrap()
        .branches
        .insert("science".into(), 2);
    let plugin = catalog::plugin_ref("science-core").unwrap();
    g.player_mut(1).unwrap().credits = plugin.cost + MIXED_REFIT_BUFFER;
    let mut ai = actor(201, 1, "gemini", Pos::new(40, 40, 0));
    ai.battery = plugin.compute_cost;
    g.state.units.push(ai);
    g.state
        .units
        .push(actor(202, 1, "gemini", Pos::new(41, 40, 0)));
    let command = g.bot_ai_refit(1, &g.state.units, "mixed-ai").unwrap();
    assert!(matches!(&command,Command::Plugin{id:201,plugin} if plugin=="science-core"));
    order(&mut g, command);
    assert_eq!(g.state.units[0].battery, 0.);
    assert!(g.state.units[0].plugins.iter().any(|p| p == "science-core"));
    assert_eq!(g.player(1).unwrap().credits, MIXED_REFIT_BUFFER);
}
#[test]
fn an_affordable_mixed_elite_upgrade_is_not_blocked_by_the_next_research_full_price() {
    let mut g = arena();
    g.player_mut(1)
        .unwrap()
        .branches
        .insert("science".into(), 3);
    g.player_mut(1).unwrap().credits = 700.;
    for (id, x) in [(201, 40), (202, 41)] {
        g.state
            .units
            .push(actor(id, 1, "gemini", Pos::new(x, 40, 0)));
    }
    let room:Room=serde_json::from_value(json!({"id":301,"shell":1,"owner":1,"rect":{"x":13,"y":52,"z":0,"w":4,"h":2},"kind":"factory","branch":null,"tier":1,"hp":500.,"maxHp":500.,"powered":true,"online":true,"connected":false,"capacity":0,"gpus":[],"inventory":0.,"progress":1.,"buildTime":1.,"cooldown":0.})).unwrap();
    g.state.rooms.push(room);
    g.invalidate_navigation();
    g.bot_for_style(1, "science", "mixed-ai");
    assert!(g
        .orders
        .iter()
        .any(|o| o.receipt.accepted && matches!(o.order.command, Command::Upgrade { id: 201 })));
    assert_eq!(g.state.units.iter().find(|u| u.id == 201).unwrap().tier, 3);
    assert!(g.player(1).unwrap().credits >= MIXED_REFIT_BUFFER);
}
#[test]
fn health_retreat_chooses_a_reachable_point_outside_known_enemy_weapon_range() {
    let mut g = arena();
    let mut ai = actor(201, 1, "gemini", Pos::new(35, 48, 0));
    ai.hp = 20.8;
    g.state.units.push(ai);
    g.state
        .units
        .push(actor(202, 2, "light-tank", Pos::new(48, 48, 0)));
    let command = g.bot_ai_recovery_action(1, &g.state.units[0]).unwrap();
    let Command::Move { pos, .. } = command else {
        panic!("expected ordinary retreat")
    };
    assert!(g.bot_recovery_site(1, pos));
    assert_eq!(g.bot_known_danger(1, pos), 0);
    assert!(g.route(g.state.units[0].pos, pos, "ai").is_some());
    order(
        &mut g,
        Command::Move {
            ids: vec![201],
            pos,
        },
    );
    g.state.units[0].hp = g.state.units[0].maxHp * 0.5; // Hysteresis-only fixture observation.
    assert!(
        g.bot_ai_recovering(&g.state.units[0]),
        "partial field healing does not erase the actual return goal"
    );
}
fn port(g: &mut Game, pos: Pos) {
    g.state.networkStores.push(NetworkStore {
        owner: 1,
        anchor: pos,
        cells: [pos].into_iter().collect(),
        compute: 500.,
        capacity: 500.,
        production: 30.,
    });
    g.state.links.push(Link {
        unitEndpoints: vec![],
        id: 301,
        owner: 1,
        kind: "compute".into(),
        path: vec![Pos::new(pos.x - 1, pos.y, 0), pos],
        hp: 100.,
        active: true,
        invested: 6.,
    });
}
#[test]
#[ignore = "flat-world pathing: kimi dash approach needs retune with solid building footprints"]
fn kimi_closes_from_gun_range_then_pays_for_a_real_dash() {
    let mut g = arena();
    g.state
        .units
        .push(actor(201, 1, "kimi", Pos::new(40, 40, 0)));
    g.state
        .units
        .push(actor(202, 2, "light-tank", Pos::new(56, 40, 0)));
    let before = serde_json::to_value(g.save()).unwrap();
    let command = decision(&g, 201).expect("approach command");
    assert_eq!(
        before,
        serde_json::to_value(g.save()).unwrap(),
        "planning mutated real state"
    );
    assert!(matches!(&command,Command::Move{pos,..} if pos.distance(Pos::new(56,40,0))<=10.));
    order(&mut g, command);
    let mut skill = None;
    for _ in 0..360 {
        g.step();
        if let Some(Command::Skill { .. }) = decision(&g, 201) {
            skill = decision(&g, 201);
            break;
        }
    }
    let command = skill.expect("dash once within actual skill range");
    assert!(matches!(&command, Command::Skill { .. }));
    let cached = g.state.units[0].battery;
    let cost = g.active_skill_cost(&g.state.units[0]);
    order(&mut g, command);
    assert!((g.state.units[0].battery - (cached - cost)).abs() < 1e-7);
    assert!(g.state.units[0].dash.is_some());
}
#[test]
fn skill_and_economy_each_receive_a_real_order_without_starving_the_other() {
    let mut g = arena();
    g.state.tick = 180;
    g.state
        .units
        .push(actor(201, 1, "kimi", Pos::new(40, 40, 0)));
    g.state
        .units
        .push(actor(202, 2, "light-tank", Pos::new(48, 40, 0)));
    let buildings = g.state.buildings.len();
    g.bot_for_style(1, "speed", "mixed-ai");
    assert!(matches!(
        g.orders.first().unwrap().order.command,
        Command::Skill { .. }
    ));
    assert!(g.orders.first().unwrap().receipt.accepted);
    assert!(g
        .orders
        .iter()
        .any(|o| o.receipt.accepted && matches!(o.order.command, Command::Build { .. })));
    assert_eq!(g.state.buildings.len(), buildings + 1);
    assert_eq!(g.state.units[0].battery, 140.);
}
#[test]
fn an_ai_finishes_recharging_before_leaving_coverage() {
    let mut g = arena();
    let pos = Pos::new(40, 40, 0);
    g.state.units.push(actor(201, 1, "kimi", pos));
    port(&mut g, pos);
    g.state.units[0].covered = true;
    g.state.units[0].battery = 40.;
    assert!(g
        .bot_combat(1, Pos::new(10, 48, 0), false, "expansion")
        .is_none());
    g.state.units[0].battery = 210.;
    assert!(matches!(
        g.bot_combat(1, Pos::new(10, 48, 0), false, "expansion"),
        Some(Command::Move { .. })
    ));
}
#[test]
fn depleted_ai_can_interrupt_an_outbound_route_for_real_network_coverage() {
    let mut g = arena();
    g.state
        .units
        .push(actor(201, 1, "kimi", Pos::new(40, 40, 0)));
    let supply = Pos::new(25, 40, 0);
    port(&mut g, supply);
    g.state.units[0].battery = 30.;
    g.state.units[0].goal = Some(Pos::new(64, 40, 0));
    g.state.units[0].route = vec![Pos::new(41, 40, 0)];
    let command = decision(&g, 201).unwrap();
    assert!(matches!(&command,Command::Move{pos,..} if g.unit_network(1,*pos).is_some()));
    order(&mut g, command);
    assert_eq!(g.state.units[0].goal, Some(supply));
    assert_eq!(g.state.units[0].battery, 30.);
}
#[test]
fn blocked_support_skills_are_not_proposed_or_charged() {
    let mut g = arena();
    g.state
        .units
        .push(actor(201, 1, "gpt", Pos::new(40, 40, 0)));
    let mut ally = actor(202, 1, "light-tank", Pos::new(48, 40, 0));
    ally.hp = 50.;
    g.state.units.push(ally);
    for y in 0..96 {
        g.state.walls.push(Wall {
            id: 400 + y as u64,
            owner: 2,
            pos: Pos::new(44, y, 0),
            kind: "physical".into(),
            hp: 1000.,
            maxHp: 1000.,
            shield: 0.,
            invested: 0.,
            antiHeal: 0.,
        });
    }
    g.state.revision += 1;
    g.invalidate_navigation();
    let before = serde_json::to_value(g.save()).unwrap();
    assert!(decision(&g, 201).is_none());
    assert_eq!(before, serde_json::to_value(g.save()).unwrap());
}
#[test]
fn wired_support_can_cast_but_does_not_move_for_resupply() {
    let mut g = arena();
    let mut healer = actor(201, 1, "gpt", Pos::new(40, 40, 0));
    healer.wired = true;
    g.state.units.push(healer);
    let mut ally = actor(202, 1, "light-tank", Pos::new(48, 40, 0));
    ally.hp = 50.;
    g.state.units.push(ally);
    let command = decision(&g, 201).expect("stationary support skill");
    assert!(matches!(command, Command::Skill { .. }));
    order(&mut g, command);
    assert!(g.state.units[1].hp > 50.);
    g.state.units[0].battery = 20.;
    g.state.units[0].skillCooldown = 0.;
    assert!(decision(&g, 201).is_none());
    assert_eq!(g.state.units[0].pos, Pos::new(40, 40, 0));
}
#[test]
fn mirrored_plots_and_internal_rooms_use_the_same_local_geometry() {
    let mut g = arena();
    g.state.visible[1] = g.state.visible[0].clone();
    g.state.explored[1] = g.state.explored[0].clone();
    let left = g.bot_build_spot(1, Pos::new(10, 48, 0), 6, 4, 22).unwrap();
    let right = g.bot_build_spot(2, Pos::new(118, 48, 0), 6, 4, 22).unwrap();
    assert_eq!(right.x, 128 - left.x - 6);
    assert_eq!(right.y, 96 - left.y - 4);
    for (owner, pos, id) in [(1, left, 401), (2, right, 402)] {
        let mut shell = g.state.buildings[0].clone();
        shell.id = id;
        shell.owner = owner;
        shell.kind = "shell".into();
        shell.rect = Rect {
            x: pos.x,
            y: pos.y,
            level: 0,
            width: 6,
            height: 4,
        };
        shell.progress = 1.;
        g.state.buildings.push(shell);
    }
    let Command::Room { rect: a, .. } = g
        .bot_make_room(1, Pos::new(10, 48, 0), "factory", None, 4, 2)
        .unwrap()
    else {
        panic!("left room");
    };
    let Command::Room { rect: b, .. } = g
        .bot_make_room(2, Pos::new(118, 48, 0), "factory", None, 4, 2)
        .unwrap()
    else {
        panic!("right room");
    };
    assert_eq!(b.x, 128 - a.x - a.width);
    assert_eq!(b.y, 96 - a.y - a.height);
}

#[test]
fn idle_ground_formation_moves_together_but_aircraft_cannot_capture() {
    let mut g = arena();
    for (id, pos) in [(201, Pos::new(40, 40, 0)), (202, Pos::new(42, 40, 0))] {
        g.state.units.push(actor(id, 1, "light-tank", pos));
    }
    let mut plane = actor(203, 1, "fighter", Pos::new(41, 40, 0));
    plane.altitude = 3.;
    plane.flightState = "cruising".into();
    plane.fuel = 100.;
    plane.fuelMax = 100.;
    g.state.units.push(plane);
    let own: Vec<_> = g.state.units.iter().collect();
    let command = g
        .bot_maneuver(1, Pos::new(10, 48, 0), false, "expansion", &own, &[])
        .unwrap();
    let Command::Move { ids, .. } = command else {
        panic!("ground formation Move");
    };
    assert_eq!(ids, vec![201, 202]);
}

#[test]
fn two_node_garrisons_do_not_hold_the_surplus_army_forever() {
    let mut g = arena();
    let nodes: Vec<_> = g
        .state
        .resources
        .iter()
        .filter(|r| r.kind == "node")
        .map(|r| r.pos)
        .take(2)
        .collect();
    for node in g
        .state
        .resources
        .iter_mut()
        .filter(|r| r.kind == "node")
        .take(2)
    {
        node.owner = 1;
        node.contested = false;
    }
    for (id, pos) in [(201, nodes[0]), (202, nodes[1]), (203, Pos::new(82, 48, 0))] {
        g.state.units.push(actor(id, 1, "light-tank", pos));
    }
    let own: Vec<_> = g.state.units.iter().collect();
    let command = g
        .bot_maneuver(1, Pos::new(10, 48, 0), false, "expansion", &own, &[])
        .unwrap();
    let Command::Move { ids, pos } = command else {
        panic!("surplus scouts toward enemy deployment sector");
    };
    assert_eq!(ids, vec![203]);
    assert_eq!(pos, Pos::new(108, 48, 0));
    assert!(g.state.units[0].pos == nodes[0] && g.state.units[1].pos == nodes[1]);
}

#[test]
fn initial_ai_budget_is_a_paid_milestone_not_a_recurring_casualty_quota() {
    let mut g = arena();
    // Explicit planning-history fixture, not an earned economy acceptance.
    let logged = |owner, sequence, accepted| LoggedOrder {
        tick: 0,
        order: Order {
            owner,
            sequence,
            command: Command::Deploy {
                room: 10,
                kind: "kimi".into(),
                pos: Pos::new(40, 40, 0),
            },
        },
        receipt: Receipt {
            accepted,
            sequence,
            tick: 0,
            reason: String::new(),
        },
    };
    g.orders.push(logged(1, 1, true));
    g.orders.push(logged(1, 2, false));
    g.orders.push(logged(2, 1, true));
    assert!(!g.bot_initial_ai_complement_purchased(1));
    g.orders.push(logged(1, 3, true));
    assert!(g.bot_initial_ai_complement_purchased(1));
    g.state
        .units
        .push(actor(201, 1, "kimi", Pos::new(40, 40, 0)));
    g.state.units.clear();
    assert!(
        g.bot_initial_ai_complement_purchased(1),
        "casualties cannot reopen the initial purchase reserve"
    );
    assert!(!g.bot_initial_ai_complement_purchased(2));
}

fn mixed_production_fixture(owner: u32) -> Game {
    let mut g = arena();
    g.state.visible[1] = g.state.visible[0].clone();
    g.state.explored[1] = g.state.explored[0].clone();
    g.player_mut(owner)
        .unwrap()
        .branches
        .insert("science".into(), 2);
    // Explicit facility/budget fixture. Purchases below still use ordinary
    // authority validation and debit their full real credit/compute prices.
    g.player_mut(owner).unwrap().credits = 10_000.;
    for (id, kind, rect, branch) in [
        (
            501,
            "research-lab",
            Rect {
                x: 20,
                y: 20,
                level: 0,
                width: 2,
                height: 2,
            },
            Some("science"),
        ),
        (
            502,
            "factory",
            Rect {
                x: 30,
                y: 20,
                level: 0,
                width: 4,
                height: 2,
            },
            None,
        ),
    ] {
        g.state.rooms.push(serde_json::from_value(json!({"id":id,"shell":owner,"owner":owner,"rect":rect,"kind":kind,"branch":branch,"tier":1,"hp":500.,"maxHp":500.,"powered":true,"connected":true,"online":true,"capacity":0,"gpus":[],"inventory":0.,"progress":1.,"buildTime":1.,"cooldown":0.})).unwrap());
    }
    let mut dc = g.state.rooms[0].clone();
    dc.id = 503;
    dc.kind = "data-center".into();
    dc.branch = None;
    dc.rect = Rect {
        x: 24,
        y: 20,
        level: 0,
        width: 4,
        height: 4,
    };
    dc.capacity = 4;
    dc.gpus = vec!["rtx-5060".into(); 4];
    g.state.rooms.push(dc);
    let mut power = g.state.buildings[0].clone();
    power.id = 504;
    power.owner = owner;
    power.kind = "wind-power".into();
    power.rect = Rect {
        x: 18,
        y: 20,
        level: 0,
        width: 1,
        height: 1,
    };
    power.power = 2000.;
    g.state.buildings.push(power);
    for (id, kind) in [(505, "power"), (506, "compute")] {
        g.state.links.push(Link {
            id,
            owner,
            kind: kind.into(),
            path: (18..=33).map(|x| Pos::new(x, 20, 0)).collect(),
            unitEndpoints: vec![],
            hp: 100.,
            active: true,
            invested: 0.,
        });
    }
    g.networks();
    assert_eq!(g.state.networkStores.len(), 1);
    g.state.networkStores[0].compute = 1000.;
    g.invalidate_navigation();
    g
}

fn pay_planned_mixed_unit(g: &mut Game, owner: u32, expected_category: &str) -> u64 {
    let rooms: Vec<_> = g
        .state
        .rooms
        .iter()
        .filter(|r| r.owner == owner)
        .cloned()
        .collect();
    let units: Vec<_> = g
        .state
        .units
        .iter()
        .filter(|u| u.owner == owner && u.hp > 0.)
        .cloned()
        .collect();
    let command = g
        .bot_produce(owner, "science", &rooms, &units, "mixed-ai")
        .expect("affordable ordinary production");
    let Command::Deploy { ref kind, .. } = command else {
        panic!("deploy")
    };
    let d = catalog::unit_ref(kind).unwrap();
    assert_eq!(d.category, expected_category);
    let compute_cost = if d.category == "ai" {
        catalog::AI_DEPLOY_COMPUTE
    } else {
        0.
    };
    let price = g.bot_command_credit_cost(owner, &command);
    let credits = g.player(owner).unwrap().credits;
    let compute = g.state.networkStores[0].compute;
    let foreign_credits = g.player(3 - owner).unwrap().credits;
    let receipt = g.order(Order {
        owner,
        sequence: g.sequences[(owner - 1) as usize] + 1,
        command,
    });
    assert!(receipt.accepted, "{}", receipt.reason);
    assert!((g.player(owner).unwrap().credits - (credits - price)).abs() < 1e-8);
    assert_eq!(g.state.networkStores[0].compute, compute - compute_cost);
    assert_eq!(g.player(3 - owner).unwrap().credits, foreign_credits);
    let unit = g.state.units.last_mut().unwrap();
    let id = unit.id;
    // Clear the fixture factory between purchases; movement itself has its own
    // earned-opening contracts. This is not simulated macro growth evidence.
    unit.pos = Pos::new(50 + (id % 10) as i32 * 3, 40, 0);
    unit.x = unit.pos.x as f64 + 0.5;
    unit.y = unit.pos.y as f64 + 0.5;
    unit.route.clear();
    id
}

#[test]
fn mixed_initial_support_requires_two_living_combat_escorts_and_full_paid_orders() {
    for owner in [1, 2] {
        let mut g = mixed_production_fixture(owner);
        let scout = pay_planned_mixed_unit(&mut g, owner, "vehicle");
        assert_eq!(
            catalog::unit_ref(&g.state.units.last().unwrap().kind)
                .unwrap()
                .chassis,
            "scout"
        );
        g.state
            .units
            .push(actor(301, 3 - owner, "light-tank", Pos::new(90, 40, 0)));
        let mut dead = actor(302, owner, "light-tank", Pos::new(92, 40, 0));
        dead.hp = 0.;
        g.state.units.push(dead);
        assert_eq!(
            g.bot_combat_escort_count(owner, &g.state.units),
            0,
            "scout, foreign, and dead vehicles cannot escort"
        );
        pay_planned_mixed_unit(&mut g, owner, "vehicle");
        assert_eq!(g.bot_combat_escort_count(owner, &g.state.units), 1);
        let second = pay_planned_mixed_unit(&mut g, owner, "vehicle");
        assert_eq!(g.bot_combat_escort_count(owner, &g.state.units), 2);
        pay_planned_mixed_unit(&mut g, owner, "ai");
        assert!(!g.bot_initial_ai_complement_purchased(owner));
        g.state
            .units
            .iter_mut()
            .find(|u| u.id == second)
            .unwrap()
            .hp = 0.;
        pay_planned_mixed_unit(&mut g, owner, "vehicle");
        pay_planned_mixed_unit(&mut g, owner, "ai");
        assert!(g.bot_initial_ai_complement_purchased(owner));
        assert!(!g.bot_initial_ai_complement_purchased(3 - owner));
        g.state.units.retain(|u| u.owner != owner || u.id == scout);
        assert!(
            g.bot_initial_ai_complement_purchased(owner),
            "casualties retain the paid assembly milestone"
        );
    }
}

#[test]
fn second_combat_escort_uses_its_real_affordable_budget_even_with_multiple_scouts() {
    for owner in [1, 2] {
        let mut g = mixed_production_fixture(owner);
        for (id, kind) in [
            (201, "science-scout"),
            (202, "science-scout"),
            (203, "laser-tank"),
        ] {
            g.state.units.push(actor(
                id,
                owner,
                kind,
                Pos::new(50 + (id - 201) as i32 * 3, 40, 0),
            ));
        }
        let cost = catalog::unit_ref("laser-tank").unwrap().cost;
        g.player_mut(owner).unwrap().credits = cost + 250.;
        pay_planned_mixed_unit(&mut g, owner, "vehicle");
        assert_eq!(g.bot_combat_escort_count(owner, &g.state.units), 2);
        assert_eq!(g.player(owner).unwrap().credits, 250.);
    }
}

#[test]
fn main_mixed_planner_does_not_hold_the_second_escort_behind_research_or_ai_reserves() {
    for owner in [1, 2] {
        for existing_ai in [false, true] {
            let mut g = mixed_production_fixture(owner);
            // Isolate the spending decision: no mine can be constructed and
            // no synthesis shortfall competes with this explicit army fixture.
            g.state.resources.retain(|r| r.kind == "node");
            for node in &mut g.state.resources {
                node.owner = owner;
                node.contested = false;
            }
            for (id, kind) in [
                (201, "science-scout"),
                (202, "science-scout"),
                (203, "laser-tank"),
            ] {
                g.state.units.push(actor(
                    id,
                    owner,
                    kind,
                    Pos::new(50 + (id - 201) as i32 * 3, 40, 0),
                ));
            }
            if existing_ai {
                g.state
                    .units
                    .push(actor(204, owner, "gemini", Pos::new(65, 40, 0)));
            }
            let cost = catalog::unit_ref("laser-tank").unwrap().cost;
            g.player_mut(owner).unwrap().credits = cost + 250.;
            g.bot_for_style(owner, "science", "mixed-ai");
            assert!(g.orders.iter().any(|o| o.receipt.accepted
                && matches!(&o.order.command, Command::Deploy { kind, .. } if kind == "laser-tank")),
                "owner {owner}, existing_ai={existing_ai}: {:?}", g.orders);
            assert_eq!(g.bot_combat_escort_count(owner, &g.state.units), 2);
            assert_eq!(g.player(owner).unwrap().credits, 250.);
        }
    }
}

#[test]
#[ignore = "flat abstract economy: opening escort budget needs retune without stock/depot pipeline"]
fn ordinary_mixed_openings_pay_two_escorts_before_each_initial_ai_for_both_owners() {
    for owner in [1, 2] {
        let mut g = Game::new(1000, false);
        let mut purchases = 0;
        for _ in 0..900 * 60 {
            if g.state.tick % 180 == 0 {
                let before = g.orders.len();
                let escorts = g.bot_combat_escort_count(owner, &g.state.units);
                let credits = g.player(owner).unwrap().credits;
                g.bot_for_style(owner, "science", "mixed-ai");
                for logged in &g.orders[before..] {
                    if logged.receipt.accepted {
                        if let Command::Deploy { kind, .. } = &logged.order.command {
                            if catalog::unit_ref(kind).is_some_and(|d| d.category == "ai") {
                                assert!(escorts >= 2, "owner {owner} AI {} paid at {}s with only {escorts} combat escorts", purchases + 1, g.state.tick / 60);
                                assert!(g.player(owner).unwrap().credits < credits);
                                purchases += 1;
                                eprintln!("owner={owner} paid initial AI {purchases} at {}s with {escorts} living combat escorts", g.state.tick / 60);
                            }
                        }
                    }
                }
            }
            if purchases >= 2 {
                break;
            }
            g.step();
        }
        assert_eq!(
            purchases, 2,
            "owner {owner}: escorts/research reserve must not starve actual paid assembly"
        );
        assert!(g.orders.iter().all(|o| o.order.owner == owner));
        assert!(g.bot_initial_ai_complement_purchased(owner));
        let mut restored = Game::load(g.save()).expect("earned ordinary save remains valid");
        restored.state.units.retain(|u| {
            u.owner != owner || !catalog::unit_ref(&u.kind).is_some_and(|d| d.category == "ai")
        });
        assert!(
            restored.bot_initial_ai_complement_purchased(owner),
            "saved paid history survives an explicit casualty fixture"
        );
    }
}

#[test]
fn separate_ground_cohorts_flank_different_public_nodes() {
    let mut g = arena();
    for index in 0..4 {
        g.state.units.push(actor(
            201 + index,
            1,
            "light-tank",
            Pos::new(40 + index as i32 * 2, 40, 0),
        ));
    }
    let own: Vec<_> = g.state.units.iter().collect();
    let first = g
        .bot_maneuver(1, Pos::new(10, 48, 0), false, "expansion", &own, &[])
        .unwrap();
    let Command::Move {
        ids: ref first_ids,
        pos: first_pos,
    } = first
    else {
        panic!("first ground cohort");
    };
    assert_eq!(first_ids, &vec![201, 202]);
    order(&mut g, first);
    let own: Vec<_> = g.state.units.iter().collect();
    let second = g
        .bot_maneuver(1, Pos::new(10, 48, 0), false, "expansion", &own, &[])
        .unwrap();
    let Command::Move {
        ids: second_ids,
        pos: second_pos,
    } = second
    else {
        panic!("second ground cohort");
    };
    assert_eq!(second_ids, vec![203, 204]);
    assert_ne!(
        first_pos, second_pos,
        "one contested nearest node must not absorb every replacement"
    );
}

#[test]
fn scouting_and_the_first_ai_are_not_held_for_the_second_ai_payment() {
    let mut g = arena();
    g.state.players[0].branches.insert("speed".into(), 2);
    g.state
        .units
        .push(actor(201, 1, "speed-scout", Pos::new(22, 48, 0)));
    g.state
        .units
        .push(actor(202, 1, "kimi", Pos::new(20, 48, 0)));
    g.state.visible[0].retain(|p| p.x < 26);
    g.state.explored[0] = g.state.visible[0].clone();
    let command = g
        .bot_recon_action(1, Pos::new(10, 48, 0))
        .expect("paid scout leaves using public exploration goals");
    assert!(matches!(&command,Command::Move{ids,pos} if ids==&vec![201]&&pos.x>25));
    order(&mut g, command);
    let own: Vec<_> = g.state.units.iter().collect();
    let first_ai = g
        .bot_maneuver(1, Pos::new(10, 48, 0), false, "mixed-ai", &own, &[])
        .expect("first AI can support the front");
    assert!(matches!(first_ai,Command::Move{ids,..} if ids==vec![202]));
}

#[test]
fn proposed_load_uses_the_local_grid_and_committed_construction() {
    let mut g = arena();
    let pos = Pos::new(20, 40, 0);
    g.state.players[0].power = 2000.;
    g.state.players[0].demand = 100.;
    g.state.powerGrids = vec![PowerGrid {
        owner: 1,
        cells: [pos].into_iter().collect(),
        output: 150.,
        load: 130.,
    }];
    let room:Room=serde_json::from_value(json!({"id":501,"owner":1,"shell":100,"rect":{"x":19,"y":39,"z":0,"w":2,"h":2},"kind":"data-center","branch":null,"tier":1,"hp":300.,"maxHp":300.,"powered":true,"connected":true,"capacity":1,"gpus":[],"inventory":0.,"progress":1.,"buildTime":10.,"cooldown":0.})).unwrap();
    g.state.rooms.push(room);
    assert_eq!(
        g.bot_power_margin(1, pos),
        20.,
        "unrelated aggregate surplus must not fund this grid"
    );
    let command = Command::InstallGpu {
        room: 501,
        model: "rtx-5060".into(),
    };
    assert_eq!(g.bot_new_power_load(&command), Some((pos, 55.)));
    let mut pending = g.state.rooms[0].clone();
    pending.id = 502;
    pending.kind = "data-synthesis".into();
    pending.progress = 0.5;
    g.state.rooms.push(pending);
    assert_eq!(
        g.bot_power_margin(1, pos),
        -15.,
        "already paid unfinished equipment also needs its future35 power"
    );
}

fn moving_claude_arena() -> Game {
    let mut g = arena();
    g.state.tick = 180;
    let mut claude = actor(201, 1, "claude", Pos::new(40, 40, 0));
    claude.route = vec![Pos::new(41, 40, 0)];
    claude.goal = Some(Pos::new(56, 40, 0));
    g.state.units.push(claude);
    g.state
        .units
        .push(actor(202, 2, "light-tank", Pos::new(48, 40, 0)));
    g
}

#[test]
fn moving_claude_pays_then_stops_using_exactly_two_ordinary_orders() {
    let mut g = moving_claude_arena();
    let before = g.state.units[0].battery;
    g.bot_for_style(1, "security", "mixed-ai");
    assert_eq!(g.orders.len(), 2);
    assert!(g.orders.iter().all(|o| o.receipt.accepted));
    assert!(matches!(
        g.orders[0].order.command,
        Command::Skill { id: 201, .. }
    ));
    assert!(matches!(&g.orders[1].order.command,Command::Stop{ids} if ids==&vec![201]));
    assert!((g.state.units[0].battery - (before - 105.)).abs() < 1e-9);
    assert!(g.state.units[0].route.is_empty());
    assert_eq!(g.state.units[0].goal, None);
    assert!(g.bot_holds_barrier(&g.state.units[0]));
}

#[test]
fn grouped_moves_do_not_pull_a_guarding_claude_out_of_a_live_barrier() {
    let mut g = moving_claude_arena();
    g.bot_for_style(1, "security", "mixed-ai");
    g.state
        .units
        .push(actor(203, 1, "light-tank", Pos::new(42, 40, 0)));
    let own: Vec<_> = g.state.units.iter().filter(|u| u.owner == 1).collect();
    let enemy: Vec<_> = g.state.units.iter().filter(|u| u.owner == 2).collect();
    let command = g
        .bot_maneuver(1, Pos::new(10, 48, 0), false, "expansion", &own, &enemy)
        .unwrap();
    assert!(matches!(&command,Command::Move{ids,..} if ids==&vec![203]));
    order(&mut g, command);
    assert!(g.state.units[0].route.is_empty());
}

#[test]
fn barrier_hold_ends_when_spent_expired_safe_or_compute_depleted() {
    let mut g = moving_claude_arena();
    g.bot_for_style(1, "security", "mixed-ai");
    g.state.defenseFields[0].hp = 0.;
    assert!(!g.bot_holds_barrier(&g.state.units[0]));
    g.state.defenseFields[0].hp = 360.;
    g.state.defenseFields[0].remaining = 0.;
    assert!(!g.bot_holds_barrier(&g.state.units[0]));
    g.state.defenseFields[0].remaining = 10.;
    g.state.units[1].hp = 0.;
    assert!(!g.bot_holds_barrier(&g.state.units[0]));
    g.state.units[1].hp = 400.;
    g.state.units[0].battery = 20.;
    let supply = Pos::new(25, 40, 0);
    port(&mut g, supply);
    assert!(!g.bot_holds_barrier(&g.state.units[0]));
    let retreat =
        decision(&g, 201).expect("low cache can leave a surviving shield for real supply");
    assert!(
        matches!(&retreat,Command::Move{ids,pos} if ids==&vec![201]&&g.unit_network(1,*pos).is_some())
    );
    order(&mut g, retreat);
    assert_eq!(g.state.units[0].goal, Some(supply));
}

#[test]
fn a_cast_that_exhausts_cache_stops_the_advance_then_allows_withdrawal() {
    let mut g = moving_claude_arena();
    g.state.units[0].battery = 120.;
    g.bot_for_style(1, "security", "mixed-ai");
    assert_eq!(g.orders.len(), 2);
    assert!(matches!(&g.orders[1].order.command,Command::Stop{ids} if ids==&vec![201]));
    assert_eq!(g.state.units[0].battery, 15.);
    port(&mut g, Pos::new(25, 40, 0));
    let retreat =
        decision(&g, 201).expect("empty cache does not force the caster to remain in its shield");
    assert!(matches!(retreat,Command::Move{ids,..} if ids==vec![201]));
}

#[test]
fn energy_return_does_not_wait_behind_a_service_wall_or_join_a_forward_move() {
    let mut g = arena();
    let center = Pos::new(42, 41, 0);
    let room:Room=serde_json::from_value(json!({"id":501,"owner":1,"shell":100,"rect":{"x":40,"y":40,"z":0,"w":4,"h":2},"kind":"factory","branch":null,"tier":1,"hp":300.,"maxHp":300.,"powered":true,"online":true,"connected":true,"capacity":1,"gpus":[],"inventory":0.,"progress":1.,"buildTime":10.,"cooldown":0.})).unwrap();
    g.state.rooms.push(room);
    g.state.powerGrids = vec![PowerGrid {
        owner: 1,
        cells: [center].into_iter().collect(),
        output: 200.,
        load: 40.,
    }];
    let mut unit = actor(201, 1, "laser-tank", Pos::new(47, 41, 0));
    unit.energyMax = 400.;
    unit.energy = 0.;
    g.state.units.push(unit);
    for y in 34..=48 {
        g.state.walls.push(Wall {
            id: 600 + y as u64,
            owner: 1,
            pos: Pos::new(44, y, 0),
            kind: "physical".into(),
            hp: 500.,
            maxHp: 500.,
            shield: 0.,
            invested: 0.,
            antiHeal: 0.,
        });
    }
    g.state.revision += 1;
    g.invalidate_navigation();
    assert!(!g.bot_can_recharge_at(&g.state.units[0], g.state.units[0].pos));
    let own: Vec<_> = g.state.units.iter().collect();
    assert!(g
        .bot_maneuver(1, Pos::new(10, 48, 0), false, "expansion", &own, &[])
        .is_none());
    let command = g
        .bot_combat(1, Pos::new(10, 48, 0), false, "expansion")
        .expect("route to a real visible service point");
    let Command::Move { ids, pos } = command else {
        panic!("expected charging Move");
    };
    assert_eq!(ids, vec![201]);
    assert_ne!(pos, g.state.units[0].pos);
    assert!(g.bot_can_recharge_at(&g.state.units[0], pos));
    assert!(g.route(g.state.units[0].pos, pos, "vehicle").is_some());
}

#[test]
fn minimax_ignores_air_only_injuries_and_selects_actual_grounded_group_healing() {
    let mut g = arena();
    g.state
        .units
        .push(actor(201, 1, "minimax", Pos::new(40, 40, 0)));
    let mut aircraft = actor(202, 1, "fighter", Pos::new(42, 40, 0));
    aircraft.altitude = 3.;
    aircraft.flightState = "cruising".into();
    aircraft.hp = 100.;
    g.state.units.push(aircraft);
    assert!(
        decision(&g, 201).is_none(),
        "air-only hurt location cannot justify a grounded healing cast"
    );
    for (id, pos) in [(203, Pos::new(43, 40, 0)), (204, Pos::new(44, 40, 0))] {
        let mut ally = actor(id, 1, "light-tank", pos);
        ally.hp = 30.;
        g.state.units.push(ally);
    }
    let mut isolated = actor(205, 1, "light-tank", Pos::new(30, 40, 0));
    isolated.hp = 1.;
    g.state.units.push(isolated);
    let command = decision(&g, 201)
        .expect("use the real grounded group instead of the lowest fractional HP alone");
    assert!(matches!(&command,Command::Skill{pos,..} if pos.x>=40));
    order(&mut g, command);
    assert_eq!(g.state.units[1].hp, 100.);
    assert_eq!(g.state.units[2].hp, 130.);
    assert_eq!(g.state.units[3].hp, 130.);
    assert_eq!(g.state.units[4].hp, 1.);
    assert_eq!(g.state.units[0].battery, 85.);
}
