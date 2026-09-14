//! Isolated logistics fixtures advance only the logistics clock. Inventories and
//! powered airports below are explicit physical fixtures, not earned progression.
use super::*;
use serde_json::json;
fn world() -> Game {
    let mut g = Game::new(819, false);
    g.state.terrain.fill(0);
    g.state.units.clear();
    g.state.rooms.clear();
    g.state.links.clear();
    g.state.walls.clear();
    g.state.shipments.clear();
    g.state.resources.clear();
    g.state.events.clear();
    g.next_id = 10_000;
    g.state.visible[0] = (0..96)
        .flat_map(|y| (0..128).map(move |x| Pos::new(x, y, 0)))
        .collect();
    g
}
fn facility(id: u64, kind: &str, x: i32, y: i32) -> Building {
    serde_json::from_value(json!({"id":id,"owner":1,"kind":kind,"rect":{"x":x,"y":y,"z":0,"w":2,"h":2},"tier":3,"hp":1000.,"maxHp":1000.,"progress":1.,"buildTime":8.,"powered":true,"connected":false,"power":0.,"demand":0.,"capacity":320,"branch":null,"inventory":0.,"invested":200.,"jam":0.,"shield":0.,"born":0,"stock":{}})).unwrap()
}
fn ground(g: &mut Game) -> u64 {
    let mut source = facility(301, "airstrip", 20, 40);
    source.stock.insert("ammo".into(), 80.);
    source.inventory = 80.;
    g.state.buildings.push(source);
    g.state.buildings.push(facility(302, "airstrip", 34, 40));
    g.dispatch_supply(1, 301, 302, 40., "ammo".into()).unwrap();
    g.state.shipments[0].id
}
fn air(g: &mut Game) -> u64 {
    let mut source = facility(301, "airstrip", 20, 40);
    source.stock.insert("ammo".into(), 100.);
    source.stock.insert("fuel".into(), 80.);
    source.inventory = 180.;
    g.state.buildings.push(source);
    g.state.buildings.push(facility(302, "airstrip", 40, 40));
    g.dispatch_supply_mode(1, 301, 302, 60., "ammo".into(), "air".into())
        .unwrap();
    g.state.shipments[0].id
}
fn tick(g: &mut Game, n: usize) {
    for _ in 0..n {
        g.state.tick += 1;
        g.advance_shipments();
    }
}
fn snapshot(g: &Game) -> serde_json::Value {
    serde_json::to_value(&g.state).unwrap()
}
fn stock(g: &Game, id: u64, cargo: &str) -> f64 {
    g.state
        .buildings
        .iter()
        .find(|b| b.id == id)
        .unwrap()
        .stock
        .get(cargo)
        .copied()
        .unwrap_or(0.)
}
#[test]
fn a_full_mixed_coal_warehouse_dispatches_its_real_tail_without_discarding_fuel() {
    let mut g = world();
    let mut mine = facility(301, "extractor", 20, 40);
    mine.stock.insert("fuel".into(), 765.);
    mine.stock.insert("ore".into(), 35.);
    mine.inventory = 800.;
    g.state.buildings.push(mine);
    g.state.resources.push(Resource {
        id: 302,
        kind: "coal".into(),
        pos: Pos::new(21, 41, 0),
        remaining: 1000.,
        owner: 1,
        capturer: 0,
        capture: 0.,
        contested: false,
    });
    let cash = g.player(1).unwrap().credits;
    g.logistics_second();
    assert_eq!(g.state.shipments.len(), 1);
    assert_eq!(g.state.shipments[0].amount, 35.);
    assert_eq!(stock(&g, 301, "fuel"), 765.);
    assert_eq!(stock(&g, 301, "ore"), 0.);
    assert_eq!(g.state.resources[0].remaining, 1000.);
    assert_eq!(
        g.player(1).unwrap().credits,
        cash - crate::catalog::TRANSPORT_DISPATCH_COST
    );
    tick(&mut g, 800);
    assert!(g.state.shipments.is_empty());
    assert_eq!(
        g.player(1).unwrap().credits,
        cash - crate::catalog::TRANSPORT_DISPATCH_COST + 35.
    );
    g.logistics_second();
    assert_eq!(g.state.resources[0].remaining, 995.);
    assert_eq!(stock(&g, 301, "fuel"), 766.5);
    assert_eq!(stock(&g, 301, "ore"), 3.5);
}
#[test]
fn exhausted_mine_tail_is_delivered_but_an_uneconomic_fragment_stays_in_stock() {
    for amount in [35., 5.] {
        let mut g = world();
        let mut mine = facility(301, "extractor", 20, 40);
        mine.stock.insert("ore".into(), amount);
        mine.inventory = amount;
        g.state.buildings.push(mine);
        let cash = g.player(1).unwrap().credits;
        g.logistics_second();
        if amount > crate::catalog::TRANSPORT_DISPATCH_COST {
            assert_eq!(g.state.shipments.len(), 1);
            assert_eq!(g.state.shipments[0].amount, amount);
            assert_eq!(stock(&g, 301, "ore"), 0.);
        } else {
            assert!(g.state.shipments.is_empty());
            assert_eq!(g.player(1).unwrap().credits, cash);
            assert_eq!(stock(&g, 301, "ore"), amount);
        }
    }
}
#[test]
fn manual_ground_route_preserves_partial_edge_and_is_idempotent() {
    let mut g = world();
    let id = ground(&mut g);
    tick(&mut g, 7);
    let before = g.state.shipments[0].clone();
    assert!(before.progress > 0.);
    let cash = g.player(1).unwrap().credits;
    let points = vec![Pos::new(27, 35, 0), Pos::new(35, 35, 0)];
    g.reroute_shipment(1, id, points.clone()).unwrap();
    let after = &g.state.shipments[0];
    assert_eq!(before.pos, after.pos);
    assert_eq!(before.progress, after.progress);
    assert_eq!(before.route[0], after.route[0]);
    assert_eq!(before.amount, after.amount);
    assert_eq!(cash - 2., g.player(1).unwrap().credits);
    assert_eq!(after.waypoints, points);
    let changed = snapshot(&g);
    g.reroute_shipment(1, id, points).unwrap();
    assert_eq!(snapshot(&g), changed);
    g.reroute_shipment(1, id, vec![]).unwrap();
    assert!(!g.state.shipments[0].manualRoute);
    assert_eq!(g.state.shipments[0].progress, before.progress);
    assert_eq!(g.player(1).unwrap().credits, cash - 4.);
}
#[test]
fn invalid_ground_routes_do_not_mutate_stock_cash_or_position() {
    let mut g = world();
    let id = ground(&mut g);
    for (owner, points) in [
        (2, vec![Pos::new(25, 35, 0)]),
        (1, vec![Pos::new(25, 35, 1)]),
        (1, vec![Pos::new(25, 35, 0); 17]),
        (1, vec![Pos::new(-1, 0, 0)]),
    ] {
        let before = snapshot(&g);
        assert!(g.reroute_shipment(owner, id, points).is_err());
        assert_eq!(before, snapshot(&g));
    }
    g.player_mut(1).unwrap().credits = 1.;
    let before = snapshot(&g);
    assert!(g
        .reroute_shipment(1, id, vec![Pos::new(25, 35, 0)])
        .is_err());
    assert_eq!(before, snapshot(&g));
}
#[test]
fn blocked_manual_waypoint_is_retained_until_it_can_be_reached() {
    let mut g = world();
    let id = ground(&mut g);
    let waypoint = Pos::new(27, 35, 0);
    g.reroute_shipment(1, id, vec![waypoint]).unwrap();
    g.state.walls.push(Wall {
        id: 900,
        owner: 2,
        pos: waypoint,
        kind: "physical".into(),
        hp: 500.,
        maxHp: 500.,
        shield: 0.,
        invested: 0.,
        antiHeal: 0.,
    });
    tick(&mut g, 300);
    assert_eq!(g.state.shipments[0].waypoints, vec![waypoint]);
    assert_eq!(stock(&g, 302, "ammo"), 0.);
    g.state.walls.clear();
    let mut visited = false;
    for _ in 0..1400 {
        if g.state.shipments.first().is_some_and(|s| s.pos == waypoint) {
            visited = true;
        }
        tick(&mut g, 1);
        if g.state.shipments.is_empty() {
            break;
        }
    }
    assert!(visited);
    assert!(g.state.shipments.is_empty());
    assert_eq!(stock(&g, 302, "ammo"), 40.);
    assert_eq!(stock(&g, 301, "ammo"), 40.);
}
#[test]
fn passing_destination_does_not_skip_remaining_waypoints() {
    let mut g = world();
    let id = ground(&mut g);
    let destination = g.state.shipments[0].route.last().copied().unwrap();
    g.reroute_shipment(1, id, vec![destination, Pos::new(40, 40, 0)])
        .unwrap();
    let mut passed = false;
    for _ in 0..1500 {
        if let Some(s) = g.state.shipments.first() {
            if s.pos == destination && !s.waypoints.is_empty() {
                passed = true;
                assert_eq!(stock(&g, 302, "ammo"), 0.);
            }
        }
        tick(&mut g, 1);
        if g.state.shipments.is_empty() {
            break;
        }
    }
    assert!(passed);
    assert_eq!(stock(&g, 302, "ammo"), 40.);
}
#[test]
fn air_dispatch_requires_power_fuel_capacity_and_debits_once() {
    let mut g = world();
    let id = air(&mut g);
    let s = &g.state.shipments[0];
    assert_eq!(s.id, id);
    assert_eq!(s.mode, "air");
    assert_eq!(s.flightState, "taking-off");
    assert_eq!(s.hp, 260.);
    assert_eq!(stock(&g, 301, "ammo"), 40.);
    assert!((stock(&g, 301, "fuel") + s.fuel - 80.).abs() < 1e-7);
    assert_eq!(
        g.player(1).unwrap().credits,
        crate::catalog::STARTING_CREDITS - 80.
    );
    for scenario in 0..3 {
        let mut h = world();
        let mut source = facility(301, "airstrip", 20, 40);
        source.stock.insert("ammo".into(), 80.);
        source
            .stock
            .insert("fuel".into(), if scenario == 0 { 0. } else { 40. });
        let mut dest = facility(302, "airstrip", 40, 40);
        if scenario == 1 {
            dest.powered = false;
        }
        if scenario == 2 {
            dest.stock.insert("repair".into(), 300.);
        }
        h.state.buildings.extend([source, dest]);
        let before = snapshot(&h);
        assert!(h
            .dispatch_supply_mode(1, 301, 302, 40., "ammo".into(), "air".into())
            .is_err());
        assert_eq!(before, snapshot(&h));
    }
}
#[test]
fn air_cargo_is_delivered_after_real_takeoff_cruise_and_landing_and_fuel_is_conserved() {
    let mut g = world();
    air(&mut g);
    let origin = g.state.shipments[0].pos;
    tick(&mut g, 60);
    assert_eq!(g.state.shipments[0].pos, origin);
    assert!((g.state.shipments[0].altitude - 1.).abs() < 1e-5);
    assert_eq!(stock(&g, 302, "ammo"), 0.);
    let mut landing_seen = false;
    for _ in 0..1200 {
        if g.state
            .shipments
            .first()
            .is_some_and(|s| s.flightState == "landing")
        {
            landing_seen = true;
            assert_eq!(stock(&g, 302, "ammo"), 0.);
        }
        tick(&mut g, 1);
        if g.state.shipments.is_empty() {
            break;
        }
    }
    assert!(landing_seen);
    assert!(g.state.shipments.is_empty());
    assert_eq!(stock(&g, 302, "ammo"), 60.);
    assert_eq!(stock(&g, 301, "ammo"), 40.);
    let burned = g.player(1).unwrap().totals["transport-fuel-spent"];
    assert!((stock(&g, 301, "fuel") + stock(&g, 302, "fuel") + burned - 80.).abs() < 1e-5);
    assert!((burned - 5.2).abs() < 1e-5);
}
#[test]
fn destroyed_aircraft_drops_only_fractional_cargo_and_remaining_fuel() {
    let mut g = world();
    let id = air(&mut g);
    tick(&mut g, 150);
    let fuel = g.state.shipments[0].fuel;
    g.damage(id, 10000., "kinetic", 2);
    tick(&mut g, 1);
    assert!(g.state.shipments.is_empty());
    let ammo: f64 = g
        .state
        .resources
        .iter()
        .filter(|r| r.kind == "salvage-ammo")
        .map(|r| r.remaining)
        .sum();
    let recovered_fuel: f64 = g
        .state
        .resources
        .iter()
        .filter(|r| r.kind == "salvage-fuel")
        .map(|r| r.remaining)
        .sum();
    assert!((ammo - 36.).abs() < 1e-5);
    assert!((recovered_fuel - fuel * 0.6).abs() < 1e-5);
    assert_eq!(stock(&g, 302, "ammo"), 0.);
    assert_eq!(
        g.player(1).unwrap().credits,
        crate::catalog::STARTING_CREDITS - 80.
    );
}
#[test]
fn a_lost_destination_returns_real_cargo_without_a_refund_or_copy() {
    let mut g = world();
    air(&mut g);
    tick(&mut g, 180);
    g.state
        .buildings
        .iter_mut()
        .find(|b| b.id == 302)
        .unwrap()
        .powered = false;
    let mut diverted = false;
    for _ in 0..1400 {
        tick(&mut g, 1);
        if g.state.shipments.first().is_some_and(|s| s.to == 301) {
            diverted = true;
        }
        if g.state.shipments.is_empty() {
            break;
        }
    }
    assert!(diverted);
    assert_eq!(stock(&g, 301, "ammo"), 100.);
    assert_eq!(stock(&g, 302, "ammo"), 0.);
    assert_eq!(
        g.player(1).unwrap().credits,
        crate::catalog::STARTING_CREDITS - 80.
    );
}
#[test]
fn air_waypoints_obey_fuel_and_geometry_without_rewinding() {
    let mut g = world();
    let id = air(&mut g);
    tick(&mut g, 123);
    let original = g.state.shipments[0].clone();
    g.reroute_shipment(1, id, vec![Pos::new(30, 43, 0)])
        .unwrap();
    assert_eq!(g.state.shipments[0].pos, original.pos);
    assert_eq!(g.state.shipments[0].progress, original.progress);
    assert_eq!(g.state.shipments[0].route[0], original.route[0]);
    assert_eq!(g.state.shipments[0].fuel, original.fuel);
    let before = snapshot(&g);
    assert!(g
        .reroute_shipment(1, id, vec![Pos::new(120, 80, 0), Pos::new(2, 2, 0)])
        .is_err());
    assert_eq!(before, snapshot(&g));
    let mut tall = facility(901, "shell", 29, 44);
    tall.rect.level = 1;
    tall.rect.width = 3;
    tall.rect.height = 3;
    g.state.buildings.push(tall);
    let before = snapshot(&g);
    assert!(g
        .reroute_shipment(1, id, vec![Pos::new(30, 45, 0)])
        .is_err());
    assert_eq!(before, snapshot(&g));
}
#[test]
fn airport_ore_remains_inventory_until_ground_delivery_reaches_the_core() {
    let mut g = world();
    let mut source = facility(301, "airstrip", 20, 40);
    source.stock.insert("ore".into(), 90.);
    source.stock.insert("fuel".into(), 50.);
    g.state.buildings.push(source);
    g.state.buildings.push(facility(302, "airstrip", 40, 40));
    g.dispatch_supply_mode(1, 301, 302, 60., "credits".into(), "air".into())
        .unwrap();
    assert_eq!(g.state.shipments[0].cargo, "ore");
    tick(&mut g, 1000);
    assert_eq!(stock(&g, 302, "ore"), 60.);
    assert_eq!(
        g.player(1)
            .unwrap()
            .totals
            .get("ore-delivered")
            .copied()
            .unwrap_or(0.),
        0.
    );
    let cash = g.player(1).unwrap().credits;
    let core = g.state.buildings[0].id;
    g.dispatch_supply(1, 302, core, 60., "ore".into()).unwrap();
    tick(&mut g, 2400);
    assert_eq!(g.player(1).unwrap().credits, cash - 6. + 60.);
    assert_eq!(g.player(1).unwrap().totals["ore-delivered"], 60.);
    assert_eq!(stock(&g, 302, "ore"), 0.);
}

fn vertical_courier() -> Game {
    let mut g = world();
    let mut lower = facility(501, "shell", 40, 40);
    lower.rect.width = 6;
    lower.rect.height = 4;
    let mut upper = lower.clone();
    upper.id = 502;
    upper.rect.level = 1;
    g.state.buildings.extend([lower, upper.clone()]);
    g.state.rooms.push(serde_json::from_value(json!({"id":601,"shell":502,"owner":1,"rect":{"x":43,"y":41,"z":1,"w":1,"h":1},"kind":"depot","branch":null,"tier":1,"hp":500.,"maxHp":500.,"powered":true,"connected":false,"capacity":20,"gpus":[],"inventory":0.,"progress":1.,"buildTime":1.,"cooldown":0.})).unwrap());
    g.state.entrances.push(serde_json::from_value(json!({"id":701,"owner":1,"pos":{"x":41,"y":41,"z":0},"toLevel":1,"kind":"elevator","hp":250.,"open":true,"width":1,"axis":"x","powered":true})).unwrap());
    g.state.shipments.push(serde_json::from_value(json!({"id":900,"owner":1,"from":1,"to":601,"pos":{"x":41,"y":41,"z":0},"route":[{"x":41,"y":41,"z":1},{"x":42,"y":41,"z":1},{"x":43,"y":41,"z":1}],"amount":10.,"hp":100.,"progress":0.,"cargo":"ammo"})).unwrap());
    g.state.visible[0].extend(upper.rect.cells());
    g.invalidate_navigation();
    g
}
#[test]
fn crossing_one_floor_takes_four_cell_lengths_without_creating_cargo_or_fuel() {
    let mut vertical = vertical_courier();
    let mut flat = vertical.clone();
    flat.state.shipments[0].route = vec![Pos::new(42, 41, 0)];
    tick(&mut flat, 20);
    assert_eq!(flat.state.shipments[0].pos, Pos::new(42, 41, 0));
    assert!(flat.state.shipments[0].progress.abs() < 1e-7);
    let cash = vertical.player(1).unwrap().credits;
    tick(&mut vertical, 20);
    assert_eq!(vertical.state.shipments[0].pos, Pos::new(41, 41, 0));
    assert!((vertical.state.shipments[0].progress - 0.25).abs() < 1e-7);
    tick(&mut vertical, 60);
    assert_eq!(vertical.state.shipments[0].pos, Pos::new(41, 41, 1));
    assert!(vertical.state.shipments[0].progress.abs() < 1e-7);
    assert_eq!(vertical.state.shipments[0].amount, 10.);
    assert_eq!(vertical.state.shipments[0].fuel, 0.);
    tick(&mut vertical, 40);
    assert!(!vertical.state.shipments.is_empty());
    tick(&mut vertical, 15);
    assert!(vertical.state.shipments.is_empty());
    assert_eq!(vertical.state.rooms[0].stock["ammo"], 10.);
    assert_eq!(vertical.player(1).unwrap().credits, cash);
}
#[test]
fn elevator_power_loss_and_rerouting_preserve_partial_vertical_progress() {
    let mut g = vertical_courier();
    tick(&mut g, 40);
    let old = g.state.shipments[0].clone();
    assert!((old.progress - 0.5).abs() < 1e-7);
    g.reroute_shipment(1, 900, vec![Pos::new(44, 42, 1)])
        .unwrap();
    assert_eq!(g.state.shipments[0].pos, old.pos);
    assert_eq!(g.state.shipments[0].progress, old.progress);
    assert_eq!(g.state.shipments[0].route[0], old.route[0]);
    assert_eq!(g.state.shipments[0].amount, old.amount);
    let event_subject = g.state.shipments[0].clone();
    g.transport_destroy_event(&event_subject);
    assert!((g.state.events.last().unwrap().presentationPosition.unwrap()[2] - 0.6).abs() < 1e-7);
    g.state.entrances[0].powered = false;
    g.invalidate_navigation();
    tick(&mut g, 60);
    assert_eq!(g.state.shipments[0].pos, old.pos);
    assert_eq!(g.state.shipments[0].progress, old.progress);
    assert_eq!(g.state.shipments[0].waypoints, vec![Pos::new(44, 42, 1)]);
    g.state.entrances[0].powered = true;
    g.invalidate_navigation();
    tick(&mut g, 40);
    assert_eq!(g.state.shipments[0].pos, Pos::new(41, 41, 1));
    assert!(g.state.shipments[0].progress.abs() < 1e-7);
}
#[test]
fn crossing_into_a_shorter_edge_preserves_remaining_distance() {
    let mut g = vertical_courier();
    g.state.shipments[0].progress = 0.995;
    tick(&mut g, 1);
    assert_eq!(g.state.shipments[0].pos, Pos::new(41, 41, 1));
    assert!((g.state.shipments[0].progress - 0.03).abs() < 1e-7);
    assert_eq!(g.state.shipments[0].amount, 10.);
}

fn workshop_fixture() -> Game {
    let mut g = world();
    let mut shell = facility(501, "shell", 40, 40);
    shell.rect.width = 10;
    shell.rect.height = 4;
    g.state.buildings.push(shell);
    let mut generator = facility(401, "wind-power", 35, 40);
    generator.power = 500.;
    g.state.buildings.push(generator);
    g.state.rooms.push(serde_json::from_value(json!({"id":601,"shell":501,"owner":1,"rect":{"x":40,"y":40,"z":0,"w":10,"h":2},"kind":"ammunition-workshop","branch":null,"tier":1,"hp":500.,"maxHp":500.,"powered":true,"connected":false,"capacity":200,"gpus":[],"inventory":0.,"progress":1.,"buildTime":1.,"cooldown":0.,"invested":240.})).unwrap());
    g.state.links.push(Link {
        unitEndpoints: vec![],
        id: 701,
        owner: 1,
        kind: "power".into(),
        path: (36..=49).map(|x| Pos::new(x, 41, 0)).collect(),
        hp: 150.,
        active: true,
        invested: 20.,
    });
    g.state.entrances.push(serde_json::from_value(json!({"id":702,"owner":1,"pos":{"x":40,"y":42,"z":0},"toLevel":0,"kind":"door","hp":250.,"open":true,"width":2,"axis":"y"})).unwrap());
    g.invalidate_navigation();
    g.networks();
    g
}
fn workshop_cycle(g: &mut Game) {
    for _ in 0..6 {
        g.state.tick += 60;
        g.logistics_second();
    }
}
fn room_material(g: &Game, cargo: &str) -> f64 {
    g.state
        .rooms
        .iter()
        .map(|r| r.stock.get(cargo).copied().unwrap_or(0.))
        .sum()
}
fn split_equipment(g: &mut Game) -> Vec<u64> {
    let r = g.order(Order {
        owner: 1,
        sequence: g.sequences[0] + 1,
        command: Command::SplitRoom {
            id: 601,
            axis: "x".into(),
            offset: 4,
        },
    });
    assert!(r.accepted, "{}", r.reason);
    g.state.rooms.iter().map(|r| r.id).collect()
}
#[test]
fn splitting_and_merging_a_workshop_preserves_cycle_output_cost_and_storage() {
    let mut original = workshop_fixture();
    let mut divided = original.clone();
    let initial_cash = original.player(1).unwrap().credits;
    let capacity = Game::room_stock_limit(&original.state.rooms[0]);
    let ids = split_equipment(&mut divided);
    assert!(
        (divided
            .state
            .rooms
            .iter()
            .map(|r| r.equipmentShare)
            .sum::<f64>()
            - 1.)
            .abs()
            < 1e-12
    );
    assert_eq!(
        divided
            .state
            .rooms
            .iter()
            .map(Game::room_stock_limit)
            .sum::<f64>(),
        capacity
    );
    // Recipe phases are ID-staggered, so compare one complete six-second
    // material cycle; nominal total throughput is linear on every second.
    workshop_cycle(&mut original);
    workshop_cycle(&mut divided);
    for cargo in ["ammo", "fuel", "repair"] {
        assert!((room_material(&original, cargo) - room_material(&divided, cargo)).abs() < 1e-10);
    }
    assert!(
        (original.player(1).unwrap().credits - divided.player(1).unwrap().credits).abs() < 1e-9
    );
    assert!((initial_cash - original.player(1).unwrap().credits - 9.2).abs() < 1e-9);
    let receipt = divided.order(Order {
        owner: 1,
        sequence: divided.sequences[0] + 1,
        command: Command::MergeRooms { ids },
    });
    assert!(receipt.accepted, "{}", receipt.reason);
    assert!((divided.state.rooms[0].equipmentShare - 1.).abs() < 1e-12);
    workshop_cycle(&mut original);
    workshop_cycle(&mut divided);
    for cargo in ["ammo", "fuel", "repair"] {
        assert!((room_material(&original, cargo) - room_material(&divided, cargo)).abs() < 1e-10);
    }
    assert!(
        (original.player(1).unwrap().credits - divided.player(1).unwrap().credits).abs() < 1e-9
    );
}
#[test]
fn a_tiny_equipment_share_has_fractional_output_and_fractional_cost_without_a_minimum_item() {
    let mut g = workshop_fixture();
    g.state.rooms[0].equipmentShare = 1e-6;
    let capacity = Game::room_stock_limit(&g.state.rooms[0]);
    let money = g.player(1).unwrap().credits;
    workshop_cycle(&mut g);
    let output: [f64; 3] = [
        room_material(&g, "ammo"),
        room_material(&g, "fuel"),
        room_material(&g, "repair"),
    ];
    assert!((output.iter().sum::<f64>() - 24e-6).abs() < 1e-12);
    assert!((money - g.player(1).unwrap().credits - 9.2e-6).abs() < 1e-9);
    assert_eq!(Game::room_stock_limit(&g.state.rooms[0]), capacity);
    assert!(output.iter().all(|x| *x < 1.));
}
#[test]
fn splitting_a_factory_does_not_duplicate_automatic_stock_targets() {
    let mut original = workshop_fixture();
    original.state.rooms[0].kind = "factory".into();
    original.state.rooms[0].capacity = 10;
    original.networks();
    let mut divided = original.clone();
    split_equipment(&mut divided);
    original.logistics_second();
    divided.logistics_second();
    let initial_fuel: f64 = original
        .state
        .shipments
        .iter()
        .filter(|s| s.cargo == "fuel")
        .map(|s| s.amount)
        .sum();
    assert_eq!(
        initial_fuel, 100.,
        "one courier respects the existing 100-unit dispatch batch limit"
    );
    // Compare settled targets, not the first packet: the unsplit factory needs
    // a second 20-unit fuel delivery after its first 100-unit load arrives.
    for _ in 0..1800 {
        tick(&mut original, 1);
        tick(&mut divided, 1);
        if original.state.tick % 60 == 0 {
            original.logistics_second();
            divided.logistics_second();
        }
    }
    for (cargo, wanted) in [("ammo", 40.), ("fuel", 120.)] {
        let single = room_material(&original, cargo);
        let split = room_material(&divided, cargo);
        assert!(
            (single - wanted).abs() < 1e-9,
            "{cargo}: unsplit stock {single}, expected {wanted}"
        );
        assert!(
            (single - split).abs() < 1e-9,
            "{cargo}: unsplit {single}, split {split}"
        );
        for game in [&original, &divided] {
            let in_transit: f64 = game
                .state
                .shipments
                .iter()
                .filter(|s| s.cargo == cargo)
                .map(|s| s.amount)
                .sum();
            let at_source: f64 = game
                .state
                .buildings
                .iter()
                .filter(|b| b.owner == 1)
                .map(|b| b.stock.get(cargo).copied().unwrap_or(0.))
                .sum();
            assert!(
                in_transit.abs() < 1e-9,
                "{cargo} still in transit after settling"
            );
            assert!(
                (at_source + room_material(game, cargo) + in_transit - 180.).abs() < 1e-9,
                "{cargo} mass changed: source={at_source}, rooms={}, transit={in_transit}",
                room_material(game, cargo)
            );
        }
    }
}

fn unloading_fixture(amount: f64, reload: bool, rapid: bool, cargo: &str) -> Game {
    let mut g = world();
    let kind = crate::catalog::units()
        .into_iter()
        .find(|d| d.branch == "speed" && d.chassis == "scout")
        .unwrap()
        .id;
    let mut receiver:Unit=serde_json::from_value(json!({"id":901,"owner":1,"kind":kind,"pos":{"x":20,"y":20,"z":0},"x":20.5,"y":20.5,"z":0,"tier":5,"hp":500.,"maxHp":1000.,"battery":100.,"batteryMax":240.,"covered":false,"wired":false,"ammo":0.,"ammoMax":100.,"route":[],"target":null,"cooldown":999.,"skillCooldown":0.,"plugins":[],"statuses":{},"invested":100.,"moving":false,"attackCount":0})).unwrap();
    if reload {
        receiver.plugins.push("speed-support".into());
    }
    g.state.units.push(receiver);
    g.state.shipments.push(serde_json::from_value(json!({"id":900,"owner":1,"from":1,"to":901,"pos":{"x":20,"y":20,"z":0},"route":[],"amount":amount,"hp":100.,"progress":0.,"cargo":cargo})).unwrap());
    if rapid {
        let r:Room=serde_json::from_value(json!({"id":601,"shell":1,"owner":1,"rect":{"x":22,"y":19,"z":0,"w":2,"h":2},"kind":"rapid-logistics","branch":"speed","tier":2,"hp":500.,"maxHp":500.,"powered":true,"online":true,"connected":true,"capacity":48,"gpus":[],"inventory":0.,"progress":1.,"buildTime":1.,"cooldown":0.})).unwrap();
        g.state.rooms.push(r.clone());
        let mut second = r;
        second.id = 602;
        g.state.rooms.push(second);
        g.state.networkStores.push(NetworkStore {
            owner: 1,
            anchor: Pos::new(20, 20, 0),
            cells: [Pos::new(20, 20, 0), Pos::new(23, 20, 0)]
                .into_iter()
                .collect(),
            compute: 100.,
            capacity: 100.,
            production: 0.,
        });
        g.state.links.push(Link {
            unitEndpoints: vec![],
            id: 603,
            owner: 1,
            kind: "compute".into(),
            path: (20..=23).map(|x| Pos::new(x, 20, 0)).collect(),
            hp: 100.,
            active: true,
            invested: 0.,
        });
    }
    g.invalidate_navigation();
    g
}
#[test]
fn unloading_duration_scales_with_cargo_and_real_boost_without_creating_ammo() {
    for (amount, reload, rapid, ticks) in [
        (40., false, false, 60),
        (80., false, false, 120),
        (40., true, false, 50),
        (40., true, true, 42),
    ] {
        let mut g = unloading_fixture(amount, reload, rapid, "ammo");
        tick(&mut g, ticks - 1);
        assert_eq!(g.state.units[0].ammo, 0.);
        assert_eq!(g.state.shipments[0].amount, amount);
        assert!(g.state.shipments[0].unloadProgress < 1.);
        tick(&mut g, 1);
        assert!(g.state.shipments.is_empty());
        assert_eq!(g.state.units[0].ammo, amount);
    }
}
#[test]
fn destroying_a_partially_unloaded_shipment_never_duplicates_delivered_stock() {
    let mut g = unloading_fixture(40., false, false, "ammo");
    tick(&mut g, 30);
    assert!((g.state.shipments[0].unloadProgress - 0.5).abs() < 1e-8);
    assert_eq!(g.state.units[0].ammo, 0.);
    g.state.shipments[0].hp = 0.;
    tick(&mut g, 1);
    assert!(g.state.shipments.is_empty());
    assert_eq!(g.state.units[0].ammo, 0.);
    assert_eq!(
        g.state
            .resources
            .iter()
            .filter(|r| r.kind == "salvage-ammo")
            .map(|r| r.remaining)
            .sum::<f64>(),
        24.
    );
}
#[test]
fn repair_efficiency_uses_less_material_per_hp_without_affecting_the_unload_payload() {
    let mut g = unloading_fixture(10., false, false, "repair");
    g.state.units[0].kind = "glm".into();
    g.state.units[0].plugins.push("lightweight-support".into());
    let space = g.receiving_space(901, "repair");
    assert!((space - ((500. / 4.8) - 10.)).abs() < 1e-8);
    tick(&mut g, 15);
    assert!(g.state.shipments.is_empty());
    assert!((g.state.units[0].hp - 548.).abs() < 1e-8);
    assert_eq!(
        g.player(1)
            .unwrap()
            .totals
            .get("unit-repair-material-delivered"),
        Some(&10.)
    );
}
#[test]
fn delivery_telemetry_counts_only_actual_unit_stock_and_not_spilled_remainder() {
    let mut g = unloading_fixture(40., false, false, "ammo");
    g.state.units[0].ammo = 90.;
    tick(&mut g, 60);
    assert_eq!(g.state.units[0].ammo, 100.);
    assert_eq!(
        g.player(1).unwrap().totals.get("unit-ammo-delivered"),
        Some(&10.)
    );
    assert_eq!(
        g.state
            .resources
            .iter()
            .filter(|r| r.kind == "salvage-ammo")
            .map(|r| r.remaining)
            .sum::<f64>(),
        30.
    );
    let mut destroyed = unloading_fixture(40., false, false, "ammo");
    destroyed.state.shipments[0].hp = 0.;
    tick(&mut destroyed, 1);
    assert!(!destroyed
        .player(1)
        .unwrap()
        .totals
        .contains_key("unit-ammo-delivered"));
}
#[test]
fn landed_air_cargo_still_requires_unloading_and_needs_no_fuel_to_unload() {
    let mut g = world();
    g.state.buildings.push(facility(301, "airstrip", 20, 40));
    g.state.shipments.push(serde_json::from_value(json!({"id":900,"owner":1,"from":1,"to":301,"pos":{"x":21,"y":41,"z":0},"route":[],"amount":40.,"hp":100.,"progress":0.,"cargo":"ammo","mode":"air","flightState":"landed","altitude":0.,"fuel":0.,"fuelMax":40.})).unwrap());
    tick(&mut g, 59);
    assert_eq!(stock(&g, 301, "ammo"), 0.);
    assert!(g.state.shipments[0].unloadProgress > 0.9);
    tick(&mut g, 1);
    assert_eq!(stock(&g, 301, "ammo"), 40.);
    assert!(g.state.shipments.is_empty());
    assert!(
        !g.player(1)
            .unwrap()
            .totals
            .contains_key("unit-ammo-delivered"),
        "airport stock is not a unit delivery"
    );
}

#[test]
fn automatic_consumers_never_ping_pong_each_others_operating_stock() {
    let mut g = world();
    for (id, x) in [(301, 20), (302, 26)] {
        let mut b = facility(id, "airstrip", x, 40);
        b.stock.insert("ammo".into(), 40.);
        b.inventory = 40.;
        g.state.buildings.push(b);
    }
    for _ in 0..1800 {
        tick(&mut g, 1);
        if g.state.tick % 60 == 0 {
            g.logistics_second();
        }
    }
    assert_eq!(stock(&g, 301, "ammo"), 80.);
    assert_eq!(stock(&g, 302, "ammo"), 80.);
    assert!(g.state.shipments.is_empty());
    let spent = g
        .player(1)
        .unwrap()
        .totals
        .get("transport-spent")
        .copied()
        .unwrap_or(0.);
    for _ in 0..600 {
        tick(&mut g, 1);
        if g.state.tick % 60 == 0 {
            g.logistics_second();
        }
    }
    assert_eq!(
        g.player(1)
            .unwrap()
            .totals
            .get("transport-spent")
            .copied()
            .unwrap_or(0.),
        spent,
        "idle consumers kept paying to exchange their own buffers"
    );
    for (cargo, total) in [("ammo", 260.), ("fuel", 180.)] {
        let mass: f64 = g
            .state
            .buildings
            .iter()
            .filter(|b| b.owner == 1)
            .map(|b| b.stock.get(cargo).copied().unwrap_or(0.))
            .sum();
        assert!((mass - total).abs() < 1e-8, "{cargo} mass changed: {mass}");
    }
}
