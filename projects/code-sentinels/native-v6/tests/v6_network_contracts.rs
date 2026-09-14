//! Explicit topology fixtures; these do not claim earned campaign progression.
use sentinels_v6::{catalog, Building, Entrance, Game, Link, Pos, PowerGrid, Room, Unit};
use serde_json::json;

fn world() -> Game {
    let mut g = Game::new(71, false);
    g.state.terrain.fill(0);
    g.state.buildings.clear();
    g.state.rooms.clear();
    g.state.units.clear();
    g.state.links.clear();
    g.state.entrances.clear();
    g.state.networkStores.clear();
    g.state.powerGrids.clear();
    g
}
fn rect(x: i32, y: i32, z: i32) -> serde_json::Value {
    json!({"x":x,"y":y,"z":z,"w":2,"h":2})
}
fn building(id: u64, owner: u32, kind: &str, x: i32, y: i32, z: i32, power: f64) -> Building {
    serde_json::from_value(json!({"id":id,"owner":owner,"rect":rect(x,y,z),"kind":kind,"tier":1,"hp":700.,"maxHp":700.,"progress":1.,"buildTime":5.,"powered":true,"connected":false,"power":power,"demand":0.,"capacity":0,"branch":null,"inventory":0.,"invested":100.,"jam":0.,"shield":0.,"born":0})).unwrap()
}
fn room(id: u64, owner: u32, x: i32, y: i32, z: i32, kind: &str) -> Room {
    serde_json::from_value(json!({"id":id,"shell":0,"owner":owner,"rect":rect(x,y,z),"kind":kind,"branch":null,"tier":1,"hp":400.,"maxHp":400.,"progress":1.,"buildTime":5.,"powered":true,"connected":false,"capacity":1,"gpus":if kind=="data-center"{vec!["rtx-5060"]}else{vec![]},"inventory":0.,"cooldown":0.})).unwrap()
}
fn path(a: Pos, b: Pos) -> Vec<Pos> {
    let mut out = vec![a];
    let mut p = a;
    while p.x != b.x {
        p.x += (b.x - p.x).signum();
        out.push(p);
    }
    while p.y != b.y {
        p.y += (b.y - p.y).signum();
        out.push(p);
    }
    while p.level != b.level {
        p.level += (b.level - p.level).signum();
        out.push(p);
    }
    out
}
fn link(g: &mut Game, owner: u32, kind: &str, a: Pos, b: Pos) {
    g.state.links.push(Link {
        id: 1000 + g.state.links.len() as u64,
        owner,
        kind: kind.into(),
        path: path(a, b),
        unitEndpoints: vec![],
        hp: 150.,
        active: false,
        invested: 20.,
    });
}
fn dc(g: &mut Game, id: u64, owner: u32, x: i32, y: i32, z: i32) {
    g.state
        .buildings
        .push(building(id + 100, owner, "wind-power", x, y - 5, z, 500.));
    g.state.rooms.push(room(id, owner, x, y, z, "data-center"));
    link(g, owner, "power", Pos::new(x, y - 4, z), Pos::new(x, y, z));
}
#[test]
fn radio_uses_the_nearest_real_dc_component_without_draining_its_disconnected_neighbor() {
    let mut g = world();
    dc(&mut g, 1, 1, 20, 40, 0);
    dc(&mut g, 2, 1, 47, 40, 0);
    g.state
        .buildings
        .push(building(300, 1, "mobile-relay", 48, 42, 0, 0.));
    g.networks();
    assert_eq!(g.state.networkStores.len(), 2);
    for s in &mut g.state.networkStores {
        s.compute = if s.anchor.x < 30 { 10. } else { 70. };
    }
    let p = Pos::new(53, 44, 0);
    let i = g.unit_network(1, p).unwrap();
    assert!(g.state.networkStores[i].anchor.x > 30);
    assert!(g.consume_unit_compute(1, p, 50.));
    assert_eq!(
        g.state
            .networkStores
            .iter()
            .find(|s| s.anchor.x < 30)
            .unwrap()
            .compute,
        10.
    );
    assert_eq!(g.state.networkStores[i].compute, 20.);
    assert!(!g.consume_unit_compute(1, p, 30.));
}
#[test]
fn a_nearby_dc_can_backhaul_a_component_whose_anchor_is_far_away() {
    let mut g = world();
    dc(&mut g, 1, 1, 8, 40, 0);
    dc(&mut g, 2, 1, 50, 40, 0);
    link(
        &mut g,
        1,
        "compute",
        Pos::new(9, 41, 0),
        Pos::new(51, 41, 0),
    );
    g.state
        .buildings
        .push(building(300, 1, "mobile-relay", 74, 42, 0, 0.));
    g.networks();
    assert_eq!(g.state.networkStores.len(), 1);
    assert!(
        g.state.networkStores[0]
            .anchor
            .distance(Pos::new(75, 43, 0))
            > 30.
    );
    assert_eq!(g.unit_network(1, Pos::new(80, 44, 0)), Some(0));
}
#[test]
fn radio_does_not_leak_through_floors_or_across_player_ownership() {
    let mut g = world();
    dc(&mut g, 1, 1, 20, 40, 0);
    g.state
        .buildings
        .push(building(300, 1, "mobile-relay", 23, 42, 0, 0.));
    g.networks();
    assert!(g.unit_network(1, Pos::new(24, 43, 0)).is_some());
    assert!(g.unit_network(1, Pos::new(24, 43, -1)).is_none());
    assert!(g.unit_network(2, Pos::new(24, 43, 0)).is_none());
    g.state
        .buildings
        .iter_mut()
        .find(|b| b.id == 300)
        .unwrap()
        .jam = 5.;
    assert!(g.unit_network(1, Pos::new(24, 43, 0)).is_none());
}
#[test]
fn destruction_of_a_vertical_shaft_breaks_both_power_and_compute() {
    let mut g = world();
    dc(&mut g, 1, 1, 20, 40, 0);
    g.state.rooms.push(room(2, 1, 24, 40, 1, "research-lab"));
    let upper = Pos::new(24, 40, 1);
    let lower = Pos::new(24, 40, 0);
    g.state.entrances.push(Entrance {
        id: 400,
        owner: 1,
        pos: lower,
        toLevel: 1,
        kind: "stairs".into(),
        hp: 200.,
        open: true,
        width: 1,
        axis: "x".into(),
        powered: false,
    });
    link(&mut g, 1, "power", Pos::new(20, 40, 0), upper);
    link(&mut g, 1, "compute", Pos::new(20, 40, 0), upper);
    g.networks();
    assert!(g.state.rooms[1].powered && g.state.rooms[1].connected);
    assert!(g.unit_network(1, upper).is_some());
    g.state.entrances.clear();
    g.networks();
    assert!(!g.state.rooms[1].powered);
    assert!(!g.state.rooms[1].connected);
    assert!(g.unit_network(1, upper).is_none());
    assert!(g
        .state
        .links
        .iter()
        .filter(|l| l.path.last() == Some(&upper))
        .all(|l| !l.active));
}
#[test]
fn a_wire_only_supplies_units_at_an_actual_endpoint() {
    let mut g = world();
    dc(&mut g, 1, 1, 20, 40, 0);
    let end = Pos::new(30, 41, 0);
    link(&mut g, 1, "compute", Pos::new(21, 41, 0), end);
    g.networks();
    assert!(g.compute_network(1, Pos::new(26, 41, 0)).is_some());
    assert!(g.unit_network(1, Pos::new(26, 41, 0)).is_none());
    assert!(g.unit_network(1, end).is_some());
}
#[test]
fn invalid_compute_withdrawals_cannot_create_resources() {
    let mut g = world();
    dc(&mut g, 1, 1, 20, 40, 0);
    let end = Pos::new(30, 41, 0);
    link(&mut g, 1, "compute", Pos::new(21, 41, 0), end);
    g.networks();
    g.state.networkStores[0].compute = 50.;
    for amount in [-1., f64::NAN, f64::INFINITY] {
        assert!(!g.consume_unit_compute(1, end, amount));
    }
    assert_eq!(g.state.networkStores[0].compute, 50.);
}
fn energy_unit(kind: &str, pos: Pos, altitude: f64, flight: &str) -> Unit {
    let def = catalog::unit_ref(kind).unwrap();
    serde_json::from_value(json!({"id":600,"owner":1,"kind":kind,"pos":pos,"x":pos.x as f64+0.5,"y":pos.y as f64+0.5,"z":pos.level,"tier":def.tier,"hp":400.,"maxHp":400.,"battery":0.,"batteryMax":0.,"covered":false,"wired":false,"ammo":0.,"route":[],"target":null,"cooldown":0.,"skillCooldown":0.,"plugins":[],"statuses":{},"invested":300.,"moving":false,"attackCount":0,"energy":0.,"energyMax":100.,"altitude":altitude,"flightState":flight})).unwrap()
}
#[test]
fn service_charging_does_not_cross_floors_or_refuel_aircraft_hovering_over_a_factory() {
    let ground = catalog::units()
        .into_iter()
        .find(|d| d.category == "vehicle" && d.energy_capacity > 0.)
        .unwrap();
    let plane = catalog::units()
        .into_iter()
        .find(|d| d.category == "air" && d.energy_capacity > 0.)
        .unwrap();
    for (kind, pos, altitude, flight) in [
        (&ground.id, Pos::new(22, 40, 1), 0., "ground"),
        (&plane.id, Pos::new(22, 40, 0), 2., "cruising"),
    ] {
        let mut g = world();
        let mut station = room(20, 1, 20, 40, 0, "factory");
        station.powered = true;
        g.state.powerGrids.push(PowerGrid {
            owner: 1,
            cells: [station.rect.center()].into_iter().collect(),
            output: 200.,
            load: 40.,
        });
        g.state.rooms.push(station);
        g.state.units.push(energy_unit(kind, pos, altitude, flight));
        g.recharge_energy(1.);
        assert_eq!(g.state.units[0].energy, 0.);
    }
}
#[test]
fn an_aircraft_charges_only_after_landing_on_an_own_powered_runway() {
    let plane = catalog::units()
        .into_iter()
        .find(|d| d.category == "air" && d.energy_capacity > 0.)
        .unwrap();
    let mut g = world();
    let mut runway = building(300, 1, "airstrip", 30, 44, 0, 0.);
    runway.rect.width = 8;
    runway.rect.height = 4;
    g.state.powerGrids.push(PowerGrid {
        owner: 1,
        cells: runway.rect.cells().into_iter().collect(),
        output: 200.,
        load: 20.,
    });
    g.state.buildings.push(runway);
    g.state
        .units
        .push(energy_unit(&plane.id, Pos::new(32, 45, 0), 0., "landed"));
    g.recharge_energy(1.);
    assert_eq!(g.state.units[0].energy, 35.);
    g.state.buildings[0].powered = false;
    g.recharge_energy(1.);
    assert_eq!(g.state.units[0].energy, 35.);
}

#[test]
fn a_nearby_energy_service_needs_a_clear_door_or_an_actual_cable_endpoint() {
    let mut g = world();
    let mut shell = building(2, 1, "shell", 20, 40, 0, 0.);
    shell.rect.width = 4;
    shell.rect.height = 4;
    g.state.buildings.push(shell);
    let mut station = room(20, 1, 20, 40, 0, "factory");
    station.shell = 2;
    let service_pos = station.rect.center();
    let stand = Pos::new(24, 41, 0);
    assert!(service_pos.distance(stand) < 5.);
    g.state.rooms.push(station);
    g.state.powerGrids.push(PowerGrid {
        owner: 1,
        cells: [service_pos].into_iter().collect(),
        output: 200.,
        load: 40.,
    });
    g.state
        .units
        .push(energy_unit("laser-tank", stand, 0., "ground"));

    // Proximity is insufficient on the wrong side of the service room's wall.
    g.recharge_energy(1.);
    assert_eq!(g.state.units[0].energy, 0.);
    g.state.entrances.push(Entrance {
        id: 500,
        owner: 1,
        pos: Pos::new(23, 41, 0),
        toLevel: 0,
        kind: "door".into(),
        hp: 200.,
        open: true,
        powered: true,
        width: 1,
        axis: "y".into(),
    });
    g.recharge_energy(1.);
    assert_eq!(g.state.units[0].energy, 35.);
    g.state.entrances[0].open = false;
    g.recharge_energy(1.);
    assert_eq!(g.state.units[0].energy, 35.);
    assert_eq!(g.player(1).unwrap().totals["energy-recharged"], 35.);
    assert_eq!(g.player(1).unwrap().totals["energy-charging-unit-seconds"], 1.);

    // A genuine active power cable deliberately provides a charging terminal
    // outside the wall; it neither supplies another owner nor creates power.
    link(&mut g, 1, "power", service_pos, stand);
    g.state.links.last_mut().unwrap().active = true;
    g.state.powerGrids[0].cells.insert(stand);
    g.recharge_energy(1.);
    assert_eq!(g.state.units[0].energy, 70.);
    g.state.powerGrids[0].output = g.state.powerGrids[0].load;
    g.recharge_energy(1.);
    assert_eq!(g.state.units[0].energy, 70.);
    g.state.powerGrids[0].output = 200.;
    g.state.units[0].owner = 2;
    g.recharge_energy(1.);
    assert_eq!(g.state.units[0].energy, 70.);
    assert_eq!(g.player(1).unwrap().totals["energy-recharged"], 70.);
    assert_eq!(g.player(1).unwrap().totals["energy-charging-unit-seconds"], 2.);
    assert!(!g.player(2).unwrap().totals.contains_key("energy-recharged"));
}

#[test]
fn removing_a_relays_power_connection_stops_its_radio_backhaul() {
    let mut g = world();
    dc(&mut g, 1, 1, 20, 40, 0);
    let mut relay = building(300, 1, "mobile-relay", 24, 42, 0, 0.);
    relay.demand = 10.;
    g.state.buildings.push(relay);
    link(&mut g, 1, "power", Pos::new(20, 40, 0), Pos::new(24, 42, 0));
    let power_link = g.state.links.last().unwrap().id;
    g.networks();
    let receiver = Pos::new(29, 44, 0);
    assert!(g.unit_network(1, receiver).is_some());
    g.state
        .links
        .iter_mut()
        .find(|l| l.id == power_link)
        .unwrap()
        .hp = 0.;
    g.networks();
    assert!(
        !g.state
            .buildings
            .iter()
            .find(|b| b.id == 300)
            .unwrap()
            .powered
    );
    assert!(g.unit_network(1, receiver).is_none());
}
#[test]
fn an_unfinished_room_does_not_bridge_or_overlap_two_compute_components() {
    let mut g = world();
    dc(&mut g, 1, 1, 20, 40, 0);
    dc(&mut g, 2, 1, 26, 40, 0);
    link(
        &mut g,
        1,
        "compute",
        Pos::new(21, 40, 0),
        Pos::new(22, 40, 0),
    );
    link(
        &mut g,
        1,
        "compute",
        Pos::new(27, 40, 0),
        Pos::new(25, 40, 0),
    );
    let mut unfinished = room(3, 1, 22, 40, 0, "depot");
    unfinished.rect.width = 4;
    unfinished.progress = 0.4;
    g.state.rooms.push(unfinished);
    g.networks();
    assert_eq!(g.state.networkStores.len(), 2);
    assert!(g.state.networkStores[0]
        .cells
        .is_disjoint(&g.state.networkStores[1].cells));
    assert!(!g.state.rooms.iter().find(|r| r.id == 3).unwrap().connected);
    g.state
        .rooms
        .iter_mut()
        .find(|r| r.id == 3)
        .unwrap()
        .progress = 1.;
    g.networks();
    assert_eq!(g.state.networkStores.len(), 1);
}
#[test]
fn basement_wireless_service_requires_actual_vertical_backhaul_and_own_floor() {
    let mut g = world();
    dc(&mut g, 1, 1, 20, 40, 0);
    let mut relay = room(3, 1, 24, 40, -1, "wireless-relay");
    relay.rect.width = 4;
    g.state.rooms.push(relay);
    g.state.entrances.push(Entrance {
        id: 400,
        owner: 1,
        pos: Pos::new(24, 40, 0),
        toLevel: -1,
        kind: "stairs".into(),
        hp: 200.,
        open: true,
        width: 1,
        axis: "x".into(),
        powered: false,
    });
    link(
        &mut g,
        1,
        "power",
        Pos::new(20, 40, 0),
        Pos::new(24, 40, -1),
    );
    link(
        &mut g,
        1,
        "compute",
        Pos::new(21, 40, 0),
        Pos::new(24, 40, -1),
    );
    let cable = g.state.links.last().unwrap().id;
    g.networks();
    for store in &mut g.state.networkStores {
        store.compute = 100.;
    }
    g.maintain_rooms(1. / 60.);
    let receiver = Pos::new(30, 41, -1);
    assert!(g.unit_network(1, receiver).is_some());
    assert!(g.unit_network(1, Pos::new(30, 41, 0)).is_none());
    assert!(g.unit_network(2, receiver).is_none());
    g.state.links.iter_mut().find(|l| l.id == cable).unwrap().hp = 0.;
    g.networks();
    assert!(g.unit_network(1, receiver).is_none());
}
#[test]
fn one_elevator_load_is_charged_once_and_a_power_cut_invalidates_its_cached_portal() {
    let mut g = world();
    dc(&mut g, 1, 1, 20, 40, 0);
    g.networks();
    let base_load = g.player(1).unwrap().demand;
    for level in [0, 1] {
        let mut shell = building(800 + level as u64, 1, "shell", 24, 38, level, 0.);
        shell.rect.width = 4;
        shell.rect.height = 4;
        g.state.buildings.push(shell);
    }
    let bottom = Pos::new(24, 40, 0);
    let top = Pos::new(24, 40, 1);
    g.state.entrances.push(Entrance {
        id: 400,
        owner: 1,
        pos: bottom,
        toLevel: 1,
        kind: "elevator".into(),
        hp: 200.,
        open: true,
        width: 1,
        axis: "x".into(),
        powered: false,
    });
    link(&mut g, 1, "power", Pos::new(20, 40, 0), top);
    let power_link = g.state.links.last().unwrap().id;
    g.networks();
    assert!(
        (g.player(1).unwrap().demand - base_load - catalog::ELEVATOR_POWER_DEMAND).abs() < 0.001
    );
    assert!(g.state.entrances[0].powered);
    assert!(g.can_step(bottom, top, "ai"));
    g.state
        .links
        .iter_mut()
        .find(|l| l.id == power_link)
        .unwrap()
        .hp = 0.;
    g.networks();
    assert!(!g.state.entrances[0].powered);
    assert!(!g.can_step(bottom, top, "ai"));
}
#[test]
fn a_skill_can_atomically_combine_its_actual_network_and_carried_compute() {
    for enough in [true, false] {
        let mut g = world();
        dc(&mut g, 1, 1, 20, 40, 0);
        let endpoint = Pos::new(30, 41, 0);
        link(&mut g, 1, "compute", Pos::new(21, 41, 0), endpoint);
        let cost = catalog::unit_ref("claude").unwrap().skill_cost;
        let mut actor = energy_unit("claude", endpoint, 0., "ground");
        actor.batteryMax = cost;
        actor.battery = cost * if enough { 0.6 } else { 0.3 };
        g.state.units.push(actor);
        g.networks();
        g.refresh_fog();
        g.state.networkStores[0].compute = cost * if enough { 0.5 } else { 0.2 };
        let before = (g.state.networkStores[0].compute, g.state.units[0].battery);
        let result = g.cast(1, 600, Pos::new(31, 41, 0), Some(Pos::new(31, 41, 0)));
        if enough {
            assert!(result.is_ok());
            assert!(g.state.networkStores[0].compute.abs() < 1e-8);
            assert!((g.state.units[0].battery - cost * 0.1).abs() < 1e-8);
            assert_eq!(
                g.state
                    .events
                    .iter()
                    .filter(|e| e.kind == "compute-spent" && e.subject == 600)
                    .count(),
                1
            );
        } else {
            assert!(result.is_err());
            assert_eq!(
                (g.state.networkStores[0].compute, g.state.units[0].battery),
                before
            );
            assert_eq!(g.state.units[0].skillCooldown, 0.);
        }
    }
}
#[test]
fn a_vehicle_at_a_factory_cable_endpoint_keeps_its_exit_route() {
    let mut g = world();
    dc(&mut g, 1, 1, 20, 40, 0);
    g.state.rooms.push(room(2, 1, 24, 40, 0, "factory"));
    let pos = Pos::new(25, 41, 0);
    link(&mut g, 1, "compute", Pos::new(20, 40, 0), pos);
    let mut vehicle = energy_unit("speed-scout", pos, 0., "landed");
    vehicle.route = vec![Pos::new(26, 41, 0), Pos::new(27, 41, 0)];
    vehicle.goal = Some(Pos::new(27, 41, 0));
    g.state.units.push(vehicle);
    g.networks();
    assert!(
        g.state.units[0].covered,
        "factory port must still supply command compute"
    );
    assert!(
        !g.state.units[0].wired,
        "mechanical transport must not be tethered by standing at a port"
    );
    assert_eq!(g.state.units[0].route.len(), 2);
    // A corrupt pre-validation fixture also cannot turn a mechanical unit into an AI tether.
    g.state.links.last_mut().unwrap().unitEndpoints.push(600);
    g.networks();
    assert!(!g.state.units[0].wired);
}
#[test]
fn ai_requires_an_explicit_unit_binding_and_stays_tethered_during_power_loss() {
    let mut g = world();
    dc(&mut g, 1, 1, 20, 40, 0);
    let pos = Pos::new(25, 41, 0);
    link(&mut g, 1, "compute", Pos::new(20, 40, 0), pos);
    let mut ai = energy_unit("kimi", pos, 0., "landed");
    ai.route = vec![Pos::new(26, 41, 0)];
    g.state.units.push(ai);
    g.networks();
    assert!(!g.state.units[0].wired);
    assert_eq!(g.state.units[0].route.len(), 1);
    g.state.links.last_mut().unwrap().unitEndpoints.push(600);
    g.networks();
    assert!(g.state.units[0].wired);
    assert!(g.state.units[0].route.is_empty());
    g.state.buildings[0].power = 0.;
    g.networks();
    assert!(
        g.state.units[0].wired,
        "a power outage does not remove a physical tether"
    );
    g.state.links.last_mut().unwrap().hp = 0.;
    g.networks();
    assert!(!g.state.units[0].wired);
}
#[test]
fn cached_connectivity_matches_a_fresh_game_after_physical_and_supply_changes() {
    fn compare(g: &mut Game) {
        let mut fresh = Game::new(71, false);
        fresh.state = g.state.clone();
        fresh.networks();
        g.networks();
        assert_eq!(
            serde_json::to_value(&g.state).unwrap(),
            serde_json::to_value(&fresh.state).unwrap()
        );
    }
    let mut g = world();
    dc(&mut g, 1, 1, 20, 40, 0);
    dc(&mut g, 2, 1, 28, 40, 0);
    link(
        &mut g,
        1,
        "compute",
        Pos::new(20, 40, 0),
        Pos::new(28, 40, 0),
    );
    compare(&mut g);
    g.state.networkStores[0].compute = 270.;
    compare(&mut g);
    g.state.rooms[0].gpus = vec!["rtx-5070".into()];
    compare(&mut g);
    assert_eq!(g.state.networkStores[0].capacity, 750.);
    g.state.buildings[0].power = 0.;
    compare(&mut g);
    assert!(!g.state.rooms[0].powered);
    g.state.buildings[0].power = 500.;
    compare(&mut g);
    g.state.links.last_mut().unwrap().hp = 0.;
    compare(&mut g);
    assert_eq!(g.state.networkStores.len(), 2);
    g.state.links.last_mut().unwrap().hp = 150.;
    compare(&mut g);
    g.state.rooms.swap(0, 1);
    compare(&mut g);
    g.state.rooms[0].progress = 0.5;
    compare(&mut g);
    g.state.rooms[0].progress = 1.;
    compare(&mut g);
    g.state.rooms[0].owner = 2;
    compare(&mut g);
    g.state.rooms[0].owner = 1;
    compare(&mut g);
    g.state.rooms[0].hp = 0.;
    compare(&mut g);
}
