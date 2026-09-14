//! Corrupted-state and aviation rule fixtures, not campaign/balance acceptance.
use sentinels_v6::{catalog, Building, Command, DashState, Game, Order, Pos, Unit};
use serde_json::json;
fn unit(id: u64, kind: &str, pos: Pos) -> Unit {
    let d = catalog::unit_ref(kind).unwrap();
    serde_json::from_value(json!({"id":id,"owner":1,"kind":kind,"pos":pos,"x":pos.x as f64+0.5,"y":pos.y as f64+0.5,"z":pos.level,"tier":d.tier,"hp":d.hp,"maxHp":d.hp,"battery":120.,"batteryMax":240.,"covered":false,"wired":false,"ammo":d.ammo_capacity,"ammoMax":d.ammo_capacity,"energy":d.energy_capacity,"energyMax":d.energy_capacity,"fuel":d.fuel_capacity,"fuelMax":d.fuel_capacity,"route":[],"target":null,"cooldown":0.,"skillCooldown":0.,"plugins":[],"statuses":{},"invested":d.cost,"moving":false,"attackCount":0,"branch":d.branch,"facing":0,"altitude":0.,"flightState":if d.category=="air"{"landed"}else{"ground"},"sourceFacility":0})).unwrap()
}
fn field(id: u64) -> Building {
    serde_json::from_value(json!({"id":id,"owner":1,"kind":"airstrip","rect":{"x":20,"y":45,"z":0,"w":8,"h":4},"tier":1,"hp":700.,"maxHp":700.,"progress":1.,"buildTime":5.,"powered":true,"connected":false,"power":0.,"demand":20.,"capacity":0,"branch":null,"inventory":0.,"invested":400.,"jam":0.,"shield":0.,"born":0})).unwrap()
}
#[test]
fn new_world_and_filtered_snapshot_roundtrip_are_valid() {
    for theme in ["river", "mining", "highland"] {
        let mut g = Game::new_theme(123, false, theme);
        for _ in 0..120 {
            g.step();
        }
        g.save().validate().unwrap();
        for owner in [1, 2] {
            g.snapshot(Some(owner)).validate(true).unwrap();
        }
        let serialized = serde_json::to_string(&g.save()).unwrap();
        Game::load(serde_json::from_str(&serialized).unwrap()).unwrap();
    }
}
#[test]
fn player_order_duplicate_entity_and_reused_next_id_are_rejected() {
    let initial = Game::new(1, false).save();
    let mut bad = initial.clone();
    bad.snapshot.players.swap(0, 1);
    assert!(Game::load(bad).is_err());
    let mut bad = initial.clone();
    bad.snapshot.buildings[1].id = bad.snapshot.buildings[0].id;
    assert!(Game::load(bad).is_err());
    let mut bad = initial.clone();
    bad.nextId = bad.snapshot.resources.last().unwrap().id;
    assert!(Game::load(bad).is_err());
    let mut bad = initial;
    bad.sequences[0] = 1;
    assert!(Game::load(bad).is_err());
}
#[test]
fn active_dash_and_flight_values_are_bounded() {
    let mut g = Game::new(5, false);
    let id = g.id();
    let mut u = unit(id, "kimi", Pos::new(16, 48, 0));
    u.dash = Some(DashState {
        previous: [u.x, u.y],
        hitTargets: vec![],
        remaining: 1.,
        damage: 30.,
        width: catalog::unit_ref("kimi").unwrap().skill_width,
    });
    g.state.units.push(u);
    g.state.validate(false).unwrap();
    g.state.units[0].dash.as_mut().unwrap().width = 999.;
    assert!(g.state.validate(false).is_err());
    g.state.units[0].dash = None;
    g.state.units[0].altitude = f64::NAN;
    assert!(g.state.validate(false).is_err());
}
#[test]
fn forfeit_and_rejected_orders_replay_without_fabricating_state() {
    let mut g = Game::new(12, false);
    let r = g.order(Order {
        owner: 1,
        sequence: 1,
        command: Command::Recycle { id: 999 },
    });
    assert!(!r.accepted);
    for _ in 0..80 {
        g.step();
    }
    g.forfeit(2, "连接超时".into()).unwrap();
    assert!(Game::replay(&g.save()).unwrap());
    let mut bad = g.save();
    bad.orders[0].receipt.sequence = 88;
    assert!(Game::load(bad).is_err());
}
#[test]
fn aircraft_return_preserves_sortie_while_climbing() {
    let mut g = Game::new(1, false);
    g.state.terrain.fill(0);
    g.state.buildings.push(field(500));
    let name = catalog::units()
        .into_iter()
        .find(|d| d.category == "air" && d.ammo_capacity > 0.)
        .unwrap()
        .id;
    let mut u = unit(501, &name, Pos::new(40, 47, 0));
    u.altitude = 0.8;
    u.flightState = "taking-off".into();
    u.ammo = 0.;
    u.goal = Some(Pos::new(70, 47, 0));
    u.route = vec![Pos::new(70, 47, 0)];
    g.state.units.push(u);
    for _ in 0..150 {
        g.state.tick += 1;
        g.aviation_tick();
    }
    let u = &g.state.units[0];
    assert_eq!(u.sortieTarget, Some(Pos::new(70, 47, 0)));
    assert_eq!(u.goal, Some(Pos::new(24, 47, 0)));
    assert_eq!(u.flightState, "returning");
    assert!(u.fuel < u.fuelMax);
}
#[test]
fn aircraft_only_takes_off_from_a_powered_runway_and_crashes_without_fuel() {
    let mut g = Game::new(4, false);
    g.state.terrain.fill(0);
    g.state.buildings.push(field(500));
    let name = catalog::units()
        .into_iter()
        .find(|d| d.category == "air")
        .unwrap()
        .id;
    let mut u = unit(501, &name, Pos::new(24, 47, 0));
    u.route = vec![Pos::new(50, 47, 0)];
    u.goal = Some(Pos::new(50, 47, 0));
    u.flightState = "taking-off".into();
    g.state.units.push(u);
    g.aviation_tick();
    assert!(g.state.units[0].altitude > 0.);
    g.state.units[0].altitude = 0.;
    g.state.buildings.last_mut().unwrap().powered = false;
    g.aviation_tick();
    assert_eq!(g.state.units[0].altitude, 0.);
    assert_eq!(g.state.units[0].flightState, "landed");
    g.state.units[0].altitude = 0.02;
    g.state.units[0].fuel = 0.;
    g.aviation_tick();
    assert_eq!(g.state.units[0].hp, 0.);
    assert_eq!(g.state.units[0].flightState, "emergency");
}
#[test]
fn new_move_cancels_paid_dash_without_refund() {
    let mut g = Game::new(5, false);
    g.state.terrain.fill(0);
    let mut u = unit(501, "kimi", Pos::new(16, 48, 0));
    u.dash = Some(DashState {
        previous: [u.x, u.y],
        hitTargets: vec![],
        remaining: 2.,
        damage: 10.,
        width: 1.5,
    });
    g.state.units.push(u);
    let cash = g.player(1).unwrap().credits;
    let receipt = g.order(Order {
        owner: 1,
        sequence: 1,
        command: Command::Move {
            ids: vec![501],
            pos: Pos::new(22, 48, 0),
        },
    });
    assert!(receipt.accepted, "{}", receipt.reason);
    assert!(g.state.units[0].dash.is_none());
    assert_eq!(cash, g.player(1).unwrap().credits);
}

#[test]
fn enemy_capture_pauses_owned_majority_and_lost_majority_regresses() {
    let mut g = Game::new(9, false);
    g.state.terrain.fill(0);
    g.state.tick = 18 * 60 * 60 - 1;
    for resource in g.state.resources.iter_mut().filter(|r| r.kind == "node") {
        resource.owner = 1;
    }
    g.player_mut(1).unwrap().dominance = 30.;
    let mut invader = unit(700, "kimi", Pos::new(64, 24, 0));
    invader.owner = 2;
    invader.cooldown = 999.;
    g.state.units.push(invader);
    g.step();
    assert_eq!(g.player(1).unwrap().dominance, 30.);
    let node = g
        .state
        .resources
        .iter()
        .find(|r| r.kind == "node" && r.pos.y == 24)
        .unwrap();
    assert!(node.contested);
    assert_eq!(node.capture, 1.);
    assert_eq!(node.capturer, 2);
    g.state.units.clear();
    for resource in g
        .state
        .resources
        .iter_mut()
        .filter(|r| r.kind == "node" && r.pos.y != 24)
    {
        resource.owner = 2;
    }
    for _ in 0..60 {
        g.step();
    }
    assert_eq!(g.player(1).unwrap().dominance, 28.);
}
#[test]
fn capture_progress_cannot_transfer_to_another_player_and_core_result_is_final() {
    let mut g = Game::new(10, false);
    g.state.terrain.fill(0);
    g.state.tick = 59;
    let node = g
        .state
        .resources
        .iter_mut()
        .find(|r| r.kind == "node" && r.pos.y == 24)
        .unwrap();
    node.owner = 0;
    node.capture = 19.;
    node.capturer = 2;
    let mut scout = unit(700, "kimi", Pos::new(64, 24, 0));
    scout.cooldown = 999.;
    g.state.units.push(scout);
    g.step();
    let node = g
        .state
        .resources
        .iter()
        .find(|r| r.kind == "node" && r.pos.y == 24)
        .unwrap();
    assert_eq!(node.owner, 0);
    assert_eq!(node.capturer, 1);
    assert_eq!(node.capture, 1.);
    g.player_mut(1).unwrap().dominance = 360.;
    let core = g
        .state
        .buildings
        .iter()
        .find(|b| b.kind == "core" && b.owner == 1)
        .unwrap()
        .id;
    g.damage(core, 1e12, "kinetic", 2);
    g.step();
    assert_eq!(g.state.winner, Some(2));
}

#[test]
fn replay_fractional_speed_pause_and_seek_have_no_tick_debt() {
    use sentinels_v6::ReplayController;
    let mut source = Game::new(20, false);
    for _ in 0..200 {
        source.step();
    }
    let mut replay = ReplayController::new(source.save()).unwrap();
    let mut g = Game::new(20, false);
    replay.speed = 0.5;
    replay.advance(&mut g);
    assert_eq!(g.state.tick, 0);
    replay.advance(&mut g);
    assert_eq!(g.state.tick, 1);
    replay.speed = 2.;
    replay.advance(&mut g);
    assert_eq!(g.state.tick, 3);
    replay.paused = true;
    for _ in 0..10 {
        replay.advance(&mut g);
    }
    assert_eq!(g.state.tick, 3);
    replay.paused = false;
    replay.speed = 0.5;
    replay.advance(&mut g);
    assert_eq!(g.state.tick, 3);
    replay.advance(&mut g);
    assert_eq!(g.state.tick, 4);
    g = replay.rewind();
    replay.seek = Some(80);
    replay.paused = false;
    for _ in 0..3 {
        replay.advance(&mut g);
    }
    assert_eq!(g.state.tick, 80);
    assert!(replay.paused);
    assert_eq!(replay.phase, 0.);
}

#[test]
fn walls_join_existing_friendly_endpoints_without_charging_twice() {
    let mut g = Game::new(5, false);
    g.state.terrain.fill(0);
    let first = vec![Pos::new(14, 45, 0), Pos::new(15, 45, 0)];
    assert!(
        g.order(Order {
            owner: 1,
            sequence: 1,
            command: Command::Wall {
                kind: "physical".into(),
                path: first.clone()
            }
        })
        .accepted
    );
    let cash = g.player(1).unwrap().credits;
    assert!(
        g.order(Order {
            owner: 1,
            sequence: 2,
            command: Command::Wall {
                kind: "physical".into(),
                path: vec![first[1], Pos::new(16, 45, 0)]
            }
        })
        .accepted
    );
    assert_eq!(g.state.walls.len(), 3);
    assert_eq!(g.player(1).unwrap().credits, cash - 10.);
    assert!(
        g.order(Order {
            owner: 1,
            sequence: 3,
            command: Command::Wall {
                kind: "physical".into(),
                path: first
            }
        })
        .accepted
    );
    assert_eq!(g.player(1).unwrap().credits, cash - 10.);
    let bad = g.order(Order {
        owner: 1,
        sequence: 4,
        command: Command::Wall {
            kind: "physical".into(),
            path: vec![Pos::new(i32::MIN, 0, 0), Pos::new(i32::MAX, 0, 0)],
        },
    });
    assert!(!bad.accepted);
}

#[test]
fn plugin_slot_requires_the_units_own_tier_even_with_advanced_research() {
    let mut g = Game::new(5, false);
    g.player_mut(1).unwrap().branches.insert("speed".into(), 5);
    let mut u = unit(701, "kimi", Pos::new(16, 48, 0));
    u.tier = 2;
    u.battery = 1000.;
    u.batteryMax = 1000.;
    g.state.units.push(u);
    let plugin = catalog::plugins()
        .into_iter()
        .find(|p| p.branch == "speed" && p.tier > 2)
        .unwrap();
    let cash = g.player(1).unwrap().credits;
    let receipt = g.order(Order {
        owner: 1,
        sequence: 1,
        command: Command::Plugin {
            id: 701,
            plugin: plugin.id,
        },
    });
    assert!(!receipt.accepted);
    assert_eq!(g.player(1).unwrap().credits, cash);
    assert!(g.state.units[0].plugins.is_empty());
}

#[test]
fn queued_movement_keeps_each_goal_through_obstruction_and_save() {
    use sentinels_v6::Wall;
    let mut g = Game::new(5, false);
    g.state.terrain.fill(0);
    let id = g.id();
    let mut u = unit(id, "kimi", Pos::new(16, 48, 0));
    u.cooldown = 999.;
    g.state.units.push(u);
    let a = Pos::new(18, 48, 0);
    let b = Pos::new(18, 50, 0);
    for (sequence, command) in [
        (
            1,
            Command::Move {
                ids: vec![id],
                pos: a,
            },
        ),
        (
            2,
            Command::QueueMove {
                ids: vec![id],
                pos: b,
            },
        ),
    ] {
        let r = g.order(Order {
            owner: 1,
            sequence,
            command,
        });
        assert!(r.accepted, "{}", r.reason);
    }
    assert_eq!(
        Game::load(g.save()).unwrap().state.units[0].queuedGoals,
        vec![b]
    );
    for p in [
        Pos::new(15, 48, 0),
        Pos::new(17, 48, 0),
        Pos::new(16, 47, 0),
        Pos::new(16, 49, 0),
    ] {
        let wid = g.id();
        g.state.walls.push(Wall {
            id: wid,
            owner: 1,
            pos: p,
            kind: "physical".into(),
            hp: 260.,
            maxHp: 260.,
            shield: 0.,
            invested: 10.,
            antiHeal: 0.,
        });
    }
    g.invalidate_navigation();
    for _ in 0..180 {
        g.step();
    }
    assert_eq!(g.state.units[0].goal, Some(a));
    assert_eq!(g.state.units[0].queuedGoals, vec![b]);
    g.state.walls.clear();
    g.invalidate_navigation();
    let mut visited_a = false;
    for _ in 0..180 {
        g.step();
        visited_a |= g.state.units[0].pos == a;
    }
    assert!(visited_a);
    assert_eq!(g.state.units[0].pos, b);
    assert!(g.state.units[0].queuedGoals.is_empty());
    let bad = g.order(Order {
        owner: 1,
        sequence: 3,
        command: Command::QueueMove {
            ids: vec![id, id],
            pos: a,
        },
    });
    assert!(!bad.accepted);
}

#[test]
fn construction_blueprint_cannot_scout_before_workers_arrive() {
    let mut g = Game::new(4, false);
    g.state.terrain.fill(0);
    let initial = g.state.visible[0].clone();
    let mut blueprint = field(900);
    blueprint.kind = "wind-power".into();
    blueprint.rect.x = 90;
    blueprint.rect.y = 70;
    blueprint.progress = 0.;
    g.state.buildings.push(blueprint);
    g.refresh_fog();
    assert!(!g.visible_to(1, Pos::new(91, 70, 0)));
    assert!(initial.contains(&g.state.buildings[0].rect.center()));
}

#[test]
fn queue_append_between_goal_completion_and_next_tick_preserves_fifo() {
    let mut g = Game::new(1, false);
    g.state.terrain.fill(0);
    let id = g.id();
    let mut u = unit(id, "kimi", Pos::new(20, 40, 0));
    u.goal = None;
    u.route.clear();
    u.queuedGoals = vec![Pos::new(20, 50, 0)];
    g.state.units.push(u);
    let receipt = g.order(Order {
        owner: 1,
        sequence: 1,
        command: Command::QueueMove {
            ids: vec![id],
            pos: Pos::new(30, 40, 0),
        },
    });
    assert!(receipt.accepted, "{}", receipt.reason);
    assert_eq!(g.state.units[0].goal, None);
    assert_eq!(
        g.state.units[0].queuedGoals,
        vec![Pos::new(20, 50, 0), Pos::new(30, 40, 0)]
    );
    g.step();
    assert_eq!(g.state.units[0].goal, Some(Pos::new(20, 50, 0)));
    assert_eq!(g.state.units[0].queuedGoals, vec![Pos::new(30, 40, 0)]);
}

#[test]
fn floor_transit_takes_distance_time_and_power_loss_cannot_complete_it() {
    use sentinels_v6::Entrance;
    let mut g = Game::new(2, false);
    g.state.terrain.fill(0);
    let mut lower = field(800);
    lower.kind = "shell".into();
    let mut upper = lower.clone();
    upper.id = 801;
    upper.rect.level = 1;
    g.state.buildings.extend([lower, upper]);
    g.next_id = 900;
    g.state.entrances.push(Entrance {
        id: 802,
        owner: 1,
        pos: Pos::new(22, 47, 0),
        toLevel: 1,
        kind: "stairs".into(),
        hp: 250.,
        open: true,
        powered: false,
        width: 1,
        axis: "x".into(),
    });
    let id = g.id();
    let mut u = unit(id, "kimi", Pos::new(22, 47, 0));
    u.cooldown = 999.;
    g.state.units.push(u);
    let result = g.order(Order {
        owner: 1,
        sequence: 1,
        command: Command::Move {
            ids: vec![id],
            pos: Pos::new(22, 47, 1),
        },
    });
    assert!(result.accepted, "{}", result.reason);
    g.step();
    assert_eq!(g.state.units[0].level, 0);
    assert!(g.state.units[0].transitProgress > 0. && g.state.units[0].transitProgress < 1.);
    assert!(g.state.units[0].elevation() > 0.);
    Game::load(g.save()).unwrap();
    for _ in 0..70 {
        g.step();
    }
    assert_eq!(g.state.units[0].level, 1);
    assert_eq!(g.state.units[0].transitProgress, 0.);
    let mut g = Game::new(3, false);
    g.state.terrain.fill(0);
    let mut lower = field(800);
    lower.kind = "shell".into();
    let mut upper = lower.clone();
    upper.id = 801;
    upper.rect.level = 1;
    g.state.buildings.extend([lower, upper]);
    g.next_id = 900;
    g.state.entrances.push(Entrance {
        id: 802,
        owner: 1,
        pos: Pos::new(22, 47, 0),
        toLevel: 1,
        kind: "elevator".into(),
        hp: 250.,
        open: true,
        powered: true,
        width: 1,
        axis: "x".into(),
    });
    let id = g.id();
    let mut u = unit(id, "kimi", Pos::new(22, 47, 0));
    u.route = vec![Pos::new(22, 47, 1)];
    u.goal = Some(Pos::new(22, 47, 1));
    u.cooldown = 999.;
    g.state.units.push(u);
    g.step();
    let partial = g.state.units[0].transitProgress;
    assert!(partial > 0.);
    g.state.entrances[0].powered = false;
    g.invalidate_navigation();
    g.step();
    assert_eq!(g.state.units[0].transitProgress, partial);
    assert_eq!(g.state.units[0].level, 0);
    let stop = g.order(Order {
        owner: 1,
        sequence: 1,
        command: Command::Stop { ids: vec![id] },
    });
    assert!(stop.accepted);
    assert_eq!(g.state.units[0].transitProgress, 0.);
    assert!(g.state.units[0].route.is_empty());
}

#[test]
fn a_visible_large_building_boundary_reveals_the_structure_without_its_rooms() {
    let mut g = Game::new(1, false);
    g.state.terrain.fill(0);
    let mut building = field(800);
    building.kind = "shell".into();
    building.owner = 2;
    building.rect = sentinels_v6::Rect {
        x: 40,
        y: 40,
        level: 0,
        width: 24,
        height: 24,
    };
    let center = building.rect.center();
    g.state.buildings.push(building);
    g.state.rooms.push(serde_json::from_value(serde_json::json!({"id":803,"shell":800,"owner":2,"rect":{"x":50,"y":50,"z":0,"w":2,"h":2},"kind":"data-center","branch":null,"tier":1,"hp":140.,"maxHp":140.,"powered":false,"connected":false,"capacity":1,"gpus":[],"inventory":0.,"progress":1.,"buildTime":5.,"cooldown":0.})).unwrap());
    let scout = unit(801, "kimi", Pos::new(38, 45, 0));
    g.state.units.push(scout);
    g.refresh_fog();
    assert!(!g.visible_to(1, center));
    assert!(g.snapshot(Some(1)).rooms.iter().all(|r| r.id != 803));
    assert!(g.snapshot(Some(1)).buildings.iter().any(|b| b.id == 800));
    let (edge, owner) = g.target_info_for(800, 1).unwrap();
    assert_eq!(owner, 2);
    assert!(g.visible_to(1, edge));
    assert!(edge.x == 40 || edge.x == 63 || edge.y == 40 || edge.y == 63);
    let mut hidden = field(802);
    hidden.owner = 2;
    hidden.rect.x = 100;
    hidden.rect.y = 80;
    g.state.buildings.push(hidden);
    assert!(g.target_info_for(802, 1).is_none());
}

#[test]
fn shield_automation_toggle_is_saved_and_only_own_flag_is_published() {
    let mut g = Game::new(5, false);
    assert_eq!(g.state.shieldAuto, [true, true]);
    let result = g.order(Order {
        owner: 1,
        sequence: 1,
        command: Command::Shield { enabled: false },
    });
    assert!(result.accepted);
    assert_eq!(g.state.shieldAuto, [false, true]);
    assert_eq!(g.snapshot(Some(2)).shieldAuto, [false, true]);
    let restored = Game::load(g.save()).unwrap();
    assert_eq!(restored.state.shieldAuto, [false, true]);
    assert_eq!(restored.shield_auto, [false, true]);
    assert!(Game::replay(&g.save()).unwrap());
}

#[test]
fn hidden_global_power_and_compute_capacity_are_not_disclosed_to_the_other_owner() {
    let mut g = Game::new(6, false);
    g.player_mut(2).unwrap().power = 900.;
    g.player_mut(2).unwrap().demand = 300.;
    g.player_mut(2).unwrap().computeCapacity = 12000.;
    let view = g.snapshot(Some(1));
    assert_eq!(
        (
            view.players[1].power,
            view.players[1].demand,
            view.players[1].computeCapacity
        ),
        (0., 0., 0.)
    );
    assert_eq!(g.player(2).unwrap().power, 900.);
}
