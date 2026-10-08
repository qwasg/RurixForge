//! Isolated, explicitly funded skill physics fixtures. These are not campaign
//! progression, 45-minute LAN acceptance or executed balance results.
use sentinels_v6::{
    catalog, Building, ConstructionJob, Game, Link, NetworkStore, Pos, Room, Shipment, Unit, Wall,
};
use serde_json::json;
fn world() -> Game {
    let mut g = Game::new(731, false);
    g.state.terrain.fill(0);
    g.state.units.clear();
    g.state.rooms.clear();
    g.state.links.clear();
    g.state.walls.clear();
    g.state.projectiles.clear();
    g.state.events.clear();
    g.state.shipments.clear();
    g.state.jobs.clear();
    g.state.defenseFields.clear();
    g.state.shieldRegions.clear();
    g.next_id = 10_000;
    for owner in 0..2 {
        g.state.visible[owner] = (-2..=5)
            .flat_map(|z| (0..96).flat_map(move |y| (0..128).map(move |x| Pos::new(x, y, z))))
            .collect();
    }
    g
}
fn unit(id: u64, owner: u32, kind: &str, x: i32, y: i32, z: i32) -> Unit {
    let d = catalog::unit_ref(kind).unwrap();
    serde_json::from_value(json!({"id":id,"owner":owner,"kind":kind,"pos":{"x":x,"y":y,"z":z},"x":x as f64+0.5,"y":y as f64+0.5,"z":z,"tier":d.tier,"hp":1000.,"maxHp":1000.,"battery":1000.,"batteryMax":1000.,"covered":false,"wired":false,"ammo":d.ammo_capacity,"ammoMax":d.ammo_capacity,"fuel":d.fuel_capacity,"fuelMax":d.fuel_capacity,"energy":d.energy_capacity,"energyMax":d.energy_capacity,"route":[],"target":null,"cooldown":999.,"skillCooldown":0.,"plugins":[],"statuses":{},"invested":100.,"moving":false,"attackCount":0,"branch":d.branch,"facing":0,"altitude":0.,"flightState":"ground","sourceFacility":0})).unwrap()
}
fn building(id: u64, owner: u32, kind: &str, x: i32, y: i32, z: i32) -> Building {
    serde_json::from_value(json!({"id":id,"owner":owner,"kind":kind,"rect":{"x":x,"y":y,"z":z,"w":1,"h":1},"tier":1,"hp":500.,"maxHp":1000.,"progress":1.,"buildTime":8.,"powered":true,"connected":false,"power":0.,"demand":0.,"capacity":0,"branch":null,"inventory":0.,"invested":200.,"jam":0.,"shield":0.,"born":0})).unwrap()
}
fn room(id: u64, owner: u32, x: i32, y: i32, z: i32) -> Room {
    serde_json::from_value(json!({"id":id,"shell":1,"owner":owner,"rect":{"x":x,"y":y,"z":z,"w":1,"h":1},"kind":"depot","branch":null,"tier":1,"hp":500.,"maxHp":1000.,"powered":true,"connected":false,"capacity":100,"gpus":[],"inventory":0.,"progress":1.,"buildTime":8.,"cooldown":0.})).unwrap()
}
fn wall(id: u64, owner: u32, x: i32, y: i32) -> Wall {
    Wall {
        id,
        owner,
        pos: Pos::new(x, y, 0),
        kind: "physical".into(),
        hp: 1000.,
        maxHp: 1000.,
        shield: 0.,
        invested: 0.,
        antiHeal: 0.,
    }
}
fn hp(g: &Game, id: u64) -> f64 {
    g.state
        .units
        .iter()
        .find(|u| u.id == id)
        .map(|u| u.hp)
        .or_else(|| g.state.buildings.iter().find(|b| b.id == id).map(|b| b.hp))
        .or_else(|| g.state.rooms.iter().find(|r| r.id == id).map(|r| r.hp))
        .unwrap()
}
fn steps(g: &mut Game, n: usize) {
    for _ in 0..n {
        g.step();
    }
}
fn mechanical(branch: &str, chassis: &str) -> String {
    catalog::units()
        .into_iter()
        .find(|d| d.branch == branch && d.chassis == chassis)
        .unwrap()
        .id
}
fn caster_network(g: &mut Game, id: u64) {
    let u = g.state.units.iter_mut().find(|u| u.id == id).unwrap();
    u.covered = true;
    u.battery = 0.;
    u.batteryMax = 0.;
    let pos = u.pos;
    let owner = u.owner;
    g.state.networkStores.push(NetworkStore {
        owner,
        anchor: pos,
        cells: [pos].into_iter().collect(),
        compute: 1000.,
        capacity: 1000.,
        production: 0.,
    });
    g.state.links.push(Link {
        unitEndpoints: g.state.units.iter().filter(|u|u.pos==pos&&catalog::unit_ref(&u.kind).is_some_and(|d|d.category=="ai")).map(|u|u.id).collect(),
        id: 700,
        owner,
        kind: "compute".into(),
        path: vec![pos],
        hp: 100.,
        active: true,
        invested: 0.,
    });
}
#[test]
fn rejected_casts_do_not_spend_or_publish_success() {
    for scenario in 0..5 {
        let mut g = world();
        g.state.units.push(unit(100, 1, "vscode", 20, 45, 0));
        g.state.units.push(unit(101, 2, "vscode", 26, 45, 0));
        let pos = match scenario {
            0 => Pos::new(22, 48, 0),
            1 => Pos::new(26, 45, 1),
            2 => Pos::new(50, 45, 0),
            _ => Pos::new(26, 45, 0),
        };
        if scenario == 3 {
            g.state.units[0].battery = 54.;
        }
        if scenario == 4 {
            g.state.walls.push(wall(200, 1, 24, 45));
        }
        let before = serde_json::to_value(&g.state).unwrap();
        assert!(g.cast(1, 100, pos, None).is_err());
        assert_eq!(before, serde_json::to_value(&g.state).unwrap());
    }
}
#[test]
fn precision_is_one_paid_projectile_and_publishes_actual_cast_tick() {
    let mut g = world();
    g.state.tick = 100;
    g.state.units.push(unit(100, 1, "vscode", 20, 45, 0));
    g.state.units.push(unit(101, 2, "vscode", 26, 45, 0));
    g.cast(1, 100, Pos::new(26, 45, 0), None).unwrap();
    assert_eq!(hp(&g, 101), 1000.);
    assert_eq!(g.state.projectiles.len(), 1);
    assert_eq!(g.state.units[0].battery, 945.);
    assert_eq!(g.state.units[0].lastCastTick, Some(100));
    steps(&mut g, 30);
    assert!((hp(&g, 101) - 860.).abs() < 1e-5);
}
#[test]
fn deepseek_scan_is_unique_directional_and_stops_at_walls_and_floors() {
    let mut g = world();
    g.state.units.push(unit(100, 1, "deepseek", 20, 45, 0));
    for (id, owner, x, y, z) in [
        (101, 2, 24, 45, 0),
        (102, 2, 26, 45, 0),
        (103, 2, 18, 45, 0),
        (104, 2, 24, 47, 0),
        (105, 2, 24, 45, 1),
        (106, 1, 22, 45, 0),
        (107, 2, 30, 45, 0),
        (108, 2, 40, 45, 0),
    ] {
        g.state.units.push(unit(id, owner, "vscode", x, y, z));
    }
    g.state.walls.push(wall(200, 2, 28, 45));
    g.cast(1, 100, Pos::new(38, 45, 0), None).unwrap();
    for id in [101, 102] {
        assert_eq!(hp(&g, id), 870.);
        assert_eq!(
            g.state
                .units
                .iter()
                .find(|u| u.id == id)
                .unwrap()
                .statuses
                .get("marked"),
            Some(&9.)
        );
    }
    for id in [103, 104, 105, 106, 107, 108] {
        assert_eq!(hp(&g, id), 1000.);
        assert!(!g
            .state
            .units
            .iter()
            .find(|u| u.id == id)
            .unwrap()
            .statuses
            .contains_key("marked"));
    }
}
#[test]
fn directional_scan_ends_at_the_visible_aim_endpoint() {
    let mut g = world();
    g.state.units.push(unit(100, 1, "deepseek", 20, 45, 0));
    g.state.units.push(unit(101, 2, "vscode", 28, 45, 0));
    g.cast(1, 100, Pos::new(24, 45, 0), None).unwrap();
    assert_eq!(hp(&g, 101), 1000.);
}
#[test]
fn pycharm_cone_excludes_rear_airborne_and_occluded_targets() {
    let mut g = world();
    g.state.units.push(unit(100, 1, "pycharm", 20, 45, 0));
    for (id, x, y) in [
        (101, 24, 45),
        (102, 18, 45),
        (103, 22, 49),
        (104, 28, 45),
        (105, 24, 44),
    ] {
        g.state.units.push(unit(id, 2, "vscode", x, y, 0));
    }
    g.state.units.last_mut().unwrap().altitude = 2.;
    g.state.walls.push(wall(200, 2, 26, 45));
    g.cast(1, 100, Pos::new(28, 45, 0), None).unwrap();
    assert_eq!(hp(&g, 101), 900.);
    for id in [102, 103, 104, 105] {
        assert_eq!(hp(&g, id), 1000.);
    }
}
#[test]
fn kimi_hits_during_actual_movement_only_once_even_after_serialization() {
    let mut g = world();
    g.state.units.push(unit(100, 1, "kimi", 20, 45, 0));
    g.state.units.push(unit(101, 2, "vscode", 24, 45, 0));
    g.cast(1, 100, Pos::new(28, 45, 0), None).unwrap();
    assert_eq!(hp(&g, 101), 1000.);
    steps(&mut g, 22);
    assert_eq!(hp(&g, 101), 875.);
    let saved = serde_json::to_value(g.state.units.iter().find(|u| u.id == 100).unwrap()).unwrap();
    let restored: Unit = serde_json::from_value(saved).unwrap();
    assert!(restored.dash.as_ref().unwrap().hitTargets.contains(&101));
    g.state.units[0] = restored;
    steps(&mut g, 50);
    assert_eq!(hp(&g, 101), 875.);
    assert!(g.state.units[0].dash.is_none());
    assert_eq!(g.state.units[0].pos, Pos::new(28, 45, 0));
}
#[test]
fn kimi_cannot_dash_while_wired_and_new_obstacles_stop_damage() {
    let mut g = world();
    g.state.units.push(unit(100, 1, "kimi", 20, 45, 0));
    g.state.units.push(unit(101, 2, "vscode", 26, 45, 0));
    g.state.units[0].wired = true;
    assert!(g.cast(1, 100, Pos::new(28, 45, 0), None).is_err());
    assert_eq!(g.state.units[0].battery, 1000.);
    g.state.units[0].wired = false;
    g.cast(1, 100, Pos::new(28, 45, 0), None).unwrap();
    g.state.walls.push(wall(200, 2, 22, 45));
    steps(&mut g, 50);
    assert_eq!(hp(&g, 101), 1000.);
    assert!(g.state.units[0].pos.x < 22);
}
#[test]
fn repair_area_respects_layers_walls_and_structure_heal_cut() {
    let mut g = world();
    g.state.units.push(unit(100, 1, "gpt", 20, 45, 0));
    for (id, x, y, z) in [(101, 21, 45, 0), (102, 20, 45, 1), (103, 24, 45, 0)] {
        let mut u = unit(id, 1, "vscode", x, y, z);
        u.hp = 500.;
        g.state.units.push(u);
    }
    let mut b = building(301, 1, "wind-power", 21, 47, 0);
    b.antiHeal = 10.;
    g.state.buildings.push(b);
    let mut r = room(302, 1, 21, 43, 0);
    r.antiHeal = 10.;
    g.state.rooms.push(r);
    g.state.walls.push(wall(200, 1, 23, 45));
    g.cast(1, 100, Pos::new(20, 45, 0), None).unwrap();
    assert_eq!(hp(&g, 101), 660.);
    assert_eq!(hp(&g, 102), 500.);
    assert_eq!(hp(&g, 103), 500.);
    assert_eq!(hp(&g, 301), 542.);
    assert_eq!(hp(&g, 302), 542.);
}
#[test]
fn minimax_heals_allies_and_debuffs_enemy_structures_without_crossing_walls() {
    let mut g = world();
    g.state.units.push(unit(100, 1, "minimax", 20, 45, 0));
    let mut ally = unit(101, 1, "vscode", 24, 44, 0);
    ally.hp = 500.;
    g.state.units.push(ally);
    g.state.units.push(unit(102, 2, "vscode", 25, 45, 0));
    g.state.units.push(unit(103, 2, "vscode", 24, 49, 0));
    g.state.units.push(unit(104, 2, "vscode", 24, 45, 1));
    g.state
        .buildings
        .push(building(301, 2, "wind-power", 26, 46, 0));
    g.state.rooms.push(room(302, 2, 22, 46, 0));
    g.state.walls.push(wall(200, 2, 24, 48));
    g.cast(1, 100, Pos::new(24, 45, 0), None).unwrap();
    assert_eq!(hp(&g, 101), 600.);
    assert_eq!(hp(&g, 102), 935.);
    assert_eq!(hp(&g, 103), 1000.);
    assert_eq!(hp(&g, 104), 1000.);
    for id in [102, 301, 302, 200] {
        assert_eq!(g.repair_multiplier(id), 0.35);
    }
}
#[test]
fn construction_repair_and_physical_repair_cargo_honor_the_same_debuff() {
    let mut g = world();
    let mut b = building(301, 1, "wind-power", 22, 45, 0);
    b.antiHeal = 10.;
    g.state.buildings.push(b);
    g.state.jobs.push(ConstructionJob {
        id: 500,
        owner: 1,
        target: 301,
        rect: g.state.buildings.last().unwrap().rect,
        kind: "repair".into(),
        worker: Pos::new(21, 45, 0),
        route: vec![],
        progress: 0.999,
        duration: 1.,
        invested: 20.,
        blocked: false,
        beforeBuilding: None,
        beforeRoom: None,
    });
    let mut u = unit(101, 1, "vscode", 25, 45, 0);
    u.hp = 500.;
    u.statuses.insert("anti-heal".into(), 10.);
    g.state.units.push(u);
    g.state.shipments.push(Shipment {
        unloadProgress: 0.,
        mode: "ground".into(),
        altitude: 0.,
        flightState: "ground".into(),
        flightTimer: 0.,
        fuel: 0.,
        fuelMax: 0.,
        waypoints: vec![],
        manualRoute: false,
        id: 501,
        owner: 1,
        from: 1,
        to: 101,
        pos: Pos::new(25, 45, 0),
        route: vec![],
        amount: 10.,
        hp: 100.,
        progress: 0.,
        cargo: "repair".into(),
    });
    steps(&mut g, 16);
    assert!((hp(&g, 301) - 622.5).abs() < 1e-5);
    assert!((hp(&g, 101) - 514.).abs() < 1e-5);
    assert!(!g.state.shipments.iter().any(|s| s.id == 501));
}
#[test]
fn glm_support_rejects_nearby_but_out_of_range_or_enemy_units() {
    let mut g = world();
    g.state.units.push(unit(100, 1, "glm", 20, 45, 0));
    g.state.units.push(unit(101, 1, "vscode", 39, 45, 0));
    g.state.units.push(unit(102, 2, "vscode", 28, 45, 0));
    assert!(g.cast(1, 100, Pos::new(38, 45, 0), None).is_err());
    assert!(g.cast(1, 100, Pos::new(28, 45, 0), None).is_err());
    assert_eq!(g.state.units[0].battery, 1000.);
    g.state.units[1].pos = Pos::new(35, 45, 0);
    g.state.units[1].x = 35.5;
    g.cast(1, 100, Pos::new(35, 45, 0), None).unwrap();
    assert_eq!(g.state.units[1].statuses.get("support-boost"), Some(&12.));
    assert_eq!(
        g.state.units[1].statuses.get("compute-efficiency"),
        Some(&12.)
    );
}
#[test]
fn gemini_payload_has_exact_cell_center_and_hits_a_roof_before_lower_floor() {
    let mut g = world();
    g.state.units.push(unit(100, 1, "gemini", 20, 45, 0));
    g.state.units.push(unit(101, 2, "vscode", 26, 45, 0));
    let mut b = building(301, 2, "shell", 24, 43, 1);
    b.rect.width = 6;
    b.rect.height = 6;
    b.hp = 1000.;
    g.state.buildings.push(b);
    g.cast(1, 100, Pos::new(26, 45, 0), None).unwrap();
    let p = &g.state.projectiles[0];
    assert_eq!(p.launchPosition, Some([26.5, 45.5, 4.]));
    assert_eq!(p.aimPosition, Some([26.5, 45.5, 0.02]));
    assert_eq!(hp(&g, 101), 1000.);
    steps(&mut g, 145);
    assert_eq!(hp(&g, 101), 1000.);
    assert!(hp(&g, 301) < 1000.);
}
#[test]
fn mechanical_skills_are_distinct_and_use_real_connected_compute() {
    for (branch, status) in [
        ("speed", "haste"),
        ("security", "fortify"),
        ("algorithm", "target-lock:101"),
        ("science", "charged-shot"),
    ] {
        let kind = mechanical(branch, "tank");
        let mut g = world();
        g.state.units.push(unit(100, 1, &kind, 20, 45, 0));
        g.state.units.push(unit(101, 2, "vscode", 24, 45, 0));
        caster_network(&mut g, 100);
        let cost = catalog::unit_ref(&kind).unwrap().skill_cost;
        let target = if branch == "algorithm" {
            Pos::new(24, 45, 0)
        } else {
            Pos::new(20, 45, 0)
        };
        g.cast(1, 100, target, None).unwrap();
        assert!(g.state.units[0].statuses.contains_key(status));
        assert_eq!(g.state.networkStores[0].compute, 1000. - cost);
        assert_eq!(g.state.units[0].battery, 0.);
        assert_eq!(g.state.units[0].lastCastTick, Some(0));
    }
}
#[test]
fn field_resupply_moves_finite_stock_and_reserves_incoming_cargo() {
    let kind = mechanical("lightweight", "tank");
    let mut g = world();
    let mut u = unit(100, 1, &kind, 20, 45, 0);
    u.ammo = 0.;
    u.fuel = 0.;
    g.state.units.push(u);
    caster_network(&mut g, 100);
    let mut depot = building(301, 1, "airstrip", 22, 45, 0);
    depot.stock.insert("ammo".into(), 40.);
    depot.stock.insert("fuel".into(), 40.);
    depot.inventory = 80.;
    g.state.buildings.push(depot);
    let ammo = g.state.units[0].ammoMax;
    let fuel = g.state.units[0].fuelMax;
    g.state.shipments.push(Shipment {
        unloadProgress: 0.,
        mode: "ground".into(),
        altitude: 0.,
        flightState: "ground".into(),
        flightTimer: 0.,
        fuel: 0.,
        fuelMax: 0.,
        waypoints: vec![],
        manualRoute: false,
        id: 500,
        owner: 1,
        from: 301,
        to: 100,
        pos: Pos::new(22, 45, 0),
        route: vec![],
        amount: ammo - 1.,
        hp: 100.,
        progress: 0.,
        cargo: "ammo".into(),
    });
    g.cast(1, 100, Pos::new(20, 45, 0), None).unwrap();
    assert_eq!(g.state.units[0].ammo, 1.);
    assert_eq!(g.state.buildings.last().unwrap().stock["ammo"], 39.);
    let moved = fuel.mul_add(0.2, 0.).min(40.);
    assert_eq!(g.state.units[0].fuel, moved);
    assert!((g.state.buildings.last().unwrap().stock["fuel"] + moved - 40.).abs() < 1e-5);
}
#[test]
fn field_resupply_cannot_create_resources_or_refuel_airborne_units() {
    let kind = mechanical("lightweight", "attack-aircraft");
    let mut g = world();
    let mut u = unit(100, 1, &kind, 20, 45, 0);
    u.ammo = 0.;
    u.fuel = 0.;
    u.flightState = "cruising".into();
    u.altitude = 2.;
    g.state.units.push(u);
    caster_network(&mut g, 100);
    let mut depot = building(301, 1, "airstrip", 22, 45, 0);
    depot.stock.insert("ammo".into(), 40.);
    depot.stock.insert("fuel".into(), 40.);
    g.state.buildings.push(depot);
    let before = serde_json::to_value(&g.state).unwrap();
    assert!(g.cast(1, 100, Pos::new(20, 45, 0), None).is_err());
    assert_eq!(before, serde_json::to_value(&g.state).unwrap());
    g.state.units[0].flightState = "landed".into();
    g.state.units[0].altitude = 0.;
    g.state.buildings.last_mut().unwrap().stock.clear();
    assert!(g.cast(1, 100, Pos::new(20, 45, 0), None).is_err());
    assert_eq!(g.state.networkStores[0].compute, 1000.);
}

#[test]
fn a_fully_absorbed_scan_does_not_apply_a_hit_mark() {
    let mut g = world();
    g.state.units.push(unit(100, 1, "deepseek", 20, 45, 0));
    g.state.units.push(unit(101, 2, "vscode", 24, 45, 0));
    g.state.shieldRegions.push(sentinels_v6::ShieldRegion {
        anchor: Pos::new(24, 45, 0),
        owner: 2,
        cells: [Pos::new(24, 45, 0)].into_iter().collect(),
        current: 200.,
        capacity: 200.,
        network: Pos::new(24, 45, 0),
    });
    g.cast(1, 100, Pos::new(26, 45, 0), None).unwrap();
    assert_eq!(hp(&g, 101), 1000.);
    assert!(!g.state.units[1].statuses.contains_key("marked"));
    assert_eq!(g.state.shieldRegions[0].current, 70.);
}
#[test]
fn a_new_friendly_wall_intercepts_precision_without_receiving_friendly_damage() {
    let mut g = world();
    g.state.units.push(unit(100, 1, "vscode", 20, 45, 0));
    g.state.units.push(unit(101, 2, "vscode", 28, 45, 0));
    g.cast(1, 100, Pos::new(28, 45, 0), None).unwrap();
    g.state.walls.push(wall(200, 1, 24, 45));
    steps(&mut g, 40);
    assert_eq!(hp(&g, 101), 1000.);
    assert_eq!(g.state.walls[0].hp, 1000.);
}
#[test]
fn interception_field_uses_its_catalog_radius_and_angle() {
    let mut g = world();
    g.state.units.push(unit(100, 1, "claude", 20, 45, 0));
    g.cast(1, 100, Pos::new(28, 45, 0), None).unwrap();
    let d = catalog::unit_ref("claude").unwrap();
    let f = &g.state.defenseFields[0];
    assert_eq!(f.radius, d.skill_radius);
    assert_eq!(f.angle, d.skill_angle);
    assert_eq!(f.hp, 360.);
}

#[test]
fn airborne_target_lock_obeys_true_height_range_and_flight_state() {
    let kind = mechanical("algorithm", "attack-aircraft");
    for (altitude, phase, success) in [
        (2., "cruising", true),
        (20., "cruising", false),
        (2., "taking-off", false),
    ] {
        let mut g = world();
        let mut source = unit(100, 1, &kind, 20, 45, 0);
        source.flightState = phase.into();
        source.altitude = altitude;
        g.state.units.push(source);
        let mut target = unit(101, 2, &kind, 24, 45, 0);
        target.altitude = 2.;
        target.flightState = "cruising".into();
        g.state.units.push(target);
        caster_network(&mut g, 100);
        let before = g.state.networkStores[0].compute;
        let result = g.cast(1, 100, Pos::new(24, 45, 0), None);
        assert_eq!(result.is_ok(), success);
        if success {
            assert!(g.state.units[0].statuses.contains_key("target-lock:101"));
            assert!(g.state.networkStores[0].compute < before);
        } else {
            assert_eq!(g.state.networkStores[0].compute, before);
            assert_eq!(g.state.units[0].lastCastTick, None);
        }
    }
}
#[test]
fn charged_shot_waits_for_a_paid_round_and_scales_its_payload() {
    let kind = mechanical("science", "tank");
    let mut g = world();
    let source = unit(100, 1, &kind, 20, 45, 0);
    let damage = catalog::unit_ref(&kind).unwrap().damage;
    g.state.units.push(source);
    g.state.units.push(unit(101, 2, "vscode", 26, 45, 0));
    caster_network(&mut g, 100);
    g.cast(1, 100, Pos::new(20, 45, 0), None).unwrap();
    g.state.units[0].cooldown = 0.;
    g.state.units[0].energy = 0.;
    g.step();
    assert!(g.state.units[0].statuses.contains_key("charged-shot"));
    assert_eq!(hp(&g, 101), 1000.);
    g.state.units[0].energy = g.state.units[0].energyMax;
    g.state.units[0].cooldown = 0.;
    steps(&mut g, 25);
    assert!(!g.state.units[0].statuses.contains_key("charged-shot"));
    assert!((hp(&g, 101) - (1000. - damage * 1.6)).abs() < 1e-5);
}

#[test]
fn a_unit_between_floors_cannot_pay_for_or_start_an_active_skill() {
    for kind in ["kimi", "deepseek", "gpt"] {
        let mut g = world();
        let mut source = unit(100, 1, kind, 20, 45, 0);
        source.transitProgress = 0.5;
        source.route = vec![Pos::new(20, 45, 1)];
        source.goal = Some(Pos::new(20, 45, 1));
        g.state.units.push(source);
        let before = serde_json::to_value(&g.state).unwrap();
        let result = g.cast(1, 100, Pos::new(24, 45, 0), None);
        assert!(result.is_err());
        assert_eq!(before, serde_json::to_value(&g.state).unwrap());
    }
}

fn giant_facade(kind: &str, source_x: i32) -> Game {
    let mut g = world();
    g.state.units.push(unit(100, 1, kind, source_x, 50, 0));
    let mut shell = building(301, 2, "shell", 30, 30, 0);
    shell.rect.width = 24;
    shell.rect.height = 24;
    shell.hp = 3000.;
    shell.maxHp = 3000.;
    g.state.buildings.push(shell);
    g.state.visible[0] = [
        Pos::new(source_x, 50, 0),
        Pos::new(30, 30, 0),
        Pos::new(30, 50, 0),
        Pos::new(53, 50, 0),
    ]
    .into_iter()
    .collect();
    g.invalidate_navigation();
    g
}
#[test]
fn large_facade_clicks_use_the_visible_footprint_and_true_near_surface() {
    for kind in ["vscode".to_owned(), mechanical("algorithm", "tank")] {
        let mut g = giant_facade(&kind, 20);
        let click = Pos::new(53, 50, 0);
        assert_eq!(g.target_info_for(301, 1).unwrap().0, Pos::new(30, 30, 0));
        assert!(
            g.state.units[0].pos.distance(click) > catalog::unit_ref(&kind).unwrap().skill_range
        );
        g.cast(1, 100, click, None).unwrap();
        if kind == "vscode" {
            let p = &g.state.projectiles[0];
            assert_eq!(p.target, Some(301));
            let aim = p.aimPosition.unwrap();
            assert!((aim[0] - 30.).abs() < 0.15);
            assert!((aim[1] - 50.5).abs() < 0.15);
            assert_eq!(p.destination.x, aim[0].floor() as i32);
            assert!(p.duration < 0.5);
            steps(&mut g, 30);
            assert!(hp(&g, 301) < 3000.);
        } else {
            assert_eq!(g.state.units[0].target, Some(301));
            assert!(g.state.units[0].statuses.contains_key("target-lock:301"));
        }
    }
}
#[test]
fn a_visible_facade_cannot_reveal_or_select_an_unseen_interior_room() {
    let mut g = giant_facade("vscode", 20);
    let mut hidden = room(302, 2, 33, 49, 0);
    hidden.shell = 301;
    hidden.kind = "data-center".into();
    hidden.rect.width = 4;
    hidden.rect.height = 4;
    let center = hidden.rect.center();
    g.state.rooms.push(hidden);
    let click = Pos::new(33, 50, 0);
    g.state.visible[0].insert(click);
    assert!(!g.visible_to(1, center));
    assert!(!g.snapshot(Some(1)).rooms.iter().any(|r| r.id == 302));
    g.cast(1, 100, click, None).unwrap();
    assert_eq!(g.state.projectiles[0].target, Some(301));
    steps(&mut g, 30);
    assert_eq!(hp(&g, 302), 500.);
    let mut unseen_click = giant_facade("vscode", 20);
    let before = serde_json::to_value(&unseen_click.state).unwrap();
    assert!(unseen_click.cast(1, 100, center, None).is_err());
    assert_eq!(before, serde_json::to_value(&unseen_click.state).unwrap());
}
#[test]
fn an_out_of_range_facade_rejects_the_cast_without_debit_or_cooldown() {
    for kind in ["vscode".to_owned(), mechanical("algorithm", "tank")] {
        let mut g = giant_facade(&kind, 10);
        let before = serde_json::to_value(&g.state).unwrap();
        assert!(g.cast(1, 100, Pos::new(30, 50, 0), None).is_err());
        assert_eq!(before, serde_json::to_value(&g.state).unwrap());
    }
}

#[test]
fn field_resupply_service_radius_shrinks_with_split_room_equipment() {
    let kind = mechanical("lightweight", "tank");
    let mut g = world();
    let mut source = unit(100, 1, &kind, 20, 45, 0);
    source.ammo = 0.;
    g.state.units.push(source);
    let mut depot = room(301, 1, 22, 44, 0);
    depot.rect.width = 2;
    depot.rect.height = 2;
    depot.equipmentShare = 0.25;
    depot.stock.insert("ammo".into(), 40.);
    g.state.rooms.push(depot);
    let before = serde_json::to_value(&g.state).unwrap();
    assert!(g.cast(1, 100, Pos::new(20, 45, 0), None).is_err());
    assert_eq!(before, serde_json::to_value(&g.state).unwrap());
    let mut full = g.clone();
    full.state.rooms[0].equipmentShare = 1.;
    assert!(full.cast(1, 100, Pos::new(20, 45, 0), None).is_ok());
    g.state.units[0].pos = Pos::new(22, 45, 0);
    g.state.units[0].x = 22.5;
    g.cast(1, 100, Pos::new(22, 45, 0), None).unwrap();
    assert!(g.state.units[0].ammo > 0.);
    assert!((g.state.units[0].ammo + g.state.rooms[0].stock["ammo"] - 40.).abs() < 1e-8);
}

#[test]
fn science_skill_power_increases_paid_payload_and_gemini_ready_is_consumed_only_on_success() {
    let mut g = world();
    let mut source = unit(100, 1, "gemini", 20, 45, 0);
    source.tier = 3;
    source.plugins.push("science-attack".into());
    source.statuses.insert("gemini-ready".into(), 20.);
    g.state.units.push(source);
    let before = serde_json::to_value(&g.state).unwrap();
    assert!(g.cast(1, 100, Pos::new(24, 45, 1), None).is_err());
    assert_eq!(before, serde_json::to_value(&g.state).unwrap());
    g.cast(1, 100, Pos::new(24, 45, 0), None).unwrap();
    assert!((g.state.units[0].battery - (1000. - 120. * 1.2 * 0.9)).abs() < 1e-8);
    assert!((g.state.projectiles[0].damage - 180. * 1.4 * 1.2).abs() < 1e-8);
    assert!(!g.state.units[0].statuses.contains_key("gemini-ready"));
}
#[test]
fn science_mechanical_charge_freezes_plugin_power_and_pays_the_corresponding_cost() {
    let kind = mechanical("science", "tank");
    let mut g = world();
    let mut source = unit(100, 1, &kind, 20, 45, 0);
    source.tier = 3;
    source.plugins.push("science-attack".into());
    g.state.units.push(source);
    g.cast(1, 100, Pos::new(20, 45, 0), None).unwrap();
    assert!((g.state.units[0].chargedShotMultiplier - 1.92).abs() < 1e-8);
    assert!(
        (g.state.units[0].battery - (1000. - catalog::unit_ref(&kind).unwrap().skill_cost * 1.2))
            .abs()
            < 1e-8
    );
}
#[test]
fn claude_interception_plugin_strengthens_its_real_barrier() {
    let mut g = world();
    let mut source = unit(100, 1, "claude", 20, 45, 0);
    source.tier = 3;
    source.plugins.push("security-attack".into());
    g.state.units.push(source);
    g.cast(1, 100, Pos::new(24, 45, 0), None).unwrap();
    assert!((g.state.defenseFields[0].hp - 360. * 1.4 * 1.2).abs() < 1e-8);
    assert_eq!(g.state.units[0].battery, 895.);
}
