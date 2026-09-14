//! Isolated physics fixtures, not earned campaign progress or economy/balance acceptance.
use sentinels_v6::{catalog, Building, Entrance, Game, Pos, Projectile, Room, Unit, Wall};
use serde_json::json;

fn world() -> Game {
    let mut g = Game::new(31, false);
    g.state.terrain.fill(0);
    g.state.units.clear();
    g.state.rooms.clear();
    g.state.links.clear();
    g.state.walls.clear();
    g.state.entrances.clear();
    g.state.projectiles.clear();
    g.state.events.clear();
    g.state.defenseFields.clear();
    g.state.shieldRegions.clear();
    g.next_id = 10_000;
    g
}
fn unit(id: u64, owner: u32, kind: &str, x: i32, y: i32, z: i32) -> Unit {
    let def = catalog::unit_ref(kind).unwrap();
    serde_json::from_value(json!({
        "id":id,"owner":owner,"kind":kind,"pos":{"x":x,"y":y,"z":z},"x":x as f64+0.5,"y":y as f64+0.5,"z":z,
        "tier":def.tier,"hp":1000.,"maxHp":1000.,"battery":1000.,"batteryMax":1000.,"covered":false,"wired":false,
        "ammo":1000.,"ammoMax":1000.,"energy":1000.,"energyMax":1000.,"fuel":1000.,"fuelMax":1000.,
        "route":[],"target":null,"cooldown":999.,"skillCooldown":0.,"plugins":[],"statuses":{},"invested":100.,
        "moving":false,"attackCount":0,"branch":def.branch,"facing":0,"altitude":0.,"flightState":"landed","sourceFacility":0
    })).unwrap()
}
fn building(id: u64, owner: u32, kind: &str, x: i32, y: i32, z: i32, w: i32, h: i32) -> Building {
    serde_json::from_value(json!({
        "id":id,"owner":owner,"kind":kind,"rect":{"x":x,"y":y,"z":z,"w":w,"h":h},"tier":1,
        "hp":1000.,"maxHp":1000.,"progress":1.,"buildTime":8.,"powered":true,"connected":false,
        "power":0.,"demand":0.,"capacity":0,"branch":null,"inventory":0.,"invested":200.,"jam":0.,"shield":0.,"born":0
    })).unwrap()
}
fn projectile(
    kind: &str,
    origin: [f64; 3],
    aim: [f64; 3],
    target: Option<u64>,
    penetration: f64,
    radius: f64,
) -> Projectile {
    serde_json::from_value(json!({
        "id":500,"owner":1,"source":499,"target":target,
        "origin":{"x":origin[0].floor()as i32,"y":origin[1].floor()as i32,"z":origin[2].floor().clamp(-2.,5.)as i32},
        "destination":{"x":aim[0].floor()as i32,"y":aim[1].floor()as i32,"z":aim[2].floor().clamp(-2.,5.)as i32},
        "x":origin[0],"y":origin[1],"z":origin[2],"age":0.,"duration":1.,"damage":100.,"radius":radius,
        "kind":kind,"damageType":"kinetic","penetration":penetration,"structureMultiplier":1.,
        "launchPosition":origin,"aimPosition":aim,"passedSurfaces":[],"sourceAltitude":origin[2],"targetAltitude":aim[2]
    })).unwrap()
}
fn steps(g: &mut Game, n: usize) {
    for _ in 0..n {
        g.step();
    }
}
fn hp(g: &Game, id: u64) -> f64 {
    g.state
        .units
        .iter()
        .find(|u| u.id == id)
        .map(|u| u.hp)
        .or_else(|| g.state.buildings.iter().find(|u| u.id == id).map(|u| u.hp))
        .unwrap()
}
fn ordinary_round() -> Projectile {
    projectile(
        "direct",
        [20.5, 45.5, 0.5],
        [30.5, 45.5, 0.5],
        Some(101),
        0.,
        0.,
    )
}

#[test]
fn shortage_observations_count_only_ready_targeted_firing_unit_seconds() {
    for (kind, key) in [
        ("laser-tank", "energy-starved-firing-unit-seconds"),
        ("light-tank", "ammo-starved-firing-unit-seconds"),
        ("glm", "compute-starved-firing-unit-seconds"),
    ] {
        let mut g = world();
        let mut shooter = unit(100, 1, kind, 20, 40, 0);
        shooter.cooldown = 0.;
        shooter.ammo = 0.;
        shooter.energy = 0.;
        shooter.battery = 0.;
        shooter.target = Some(101);
        g.state.units.push(shooter);
        g.state.units.push(unit(101, 2, "vscode", 23, 40, 0));
        steps(&mut g, 2);
        let observed = g.player(1).unwrap().totals.get(key).copied().unwrap_or(0.);
        assert!((observed - 2. / 60.).abs() < 1e-9, "{kind}: {observed}");
        assert_eq!(g.state.units[0].attackCount, 0);
        assert!(g.state.projectiles.is_empty());
        g.state.units[0].cooldown = 2.;
        g.step();
        assert_eq!(g.player(1).unwrap().totals[key], observed);
        g.state.units.retain(|u| u.owner == 1);
        g.state.units[0].cooldown = 0.;
        g.state.units[0].target = None;
        g.step();
        assert_eq!(g.player(1).unwrap().totals[key], observed);
    }
}

#[test]
fn delayed_area_observations_count_actual_hp_loss_without_overkill_or_empty_ground() {
    let mut g = world();
    let mut victim = unit(101, 2, "vscode", 24, 45, 0);
    victim.hp = 25.;
    victim.maxHp = 25.;
    g.state.units.push(victim);
    let mut hit = projectile(
        "delayed-area",
        [24.5, 45.5, 4.],
        [24.5, 45.5, 0.02],
        None,
        0.,
        4.,
    );
    hit.damage = 10_000.;
    g.state.projectiles.push(hit);
    steps(&mut g, 75);
    assert!(!g.state.units.iter().any(|u| u.id == 101));
    assert_eq!(g.player(1).unwrap().totals["delayed-area-impacts"], 1.);
    assert_eq!(
        g.player(1).unwrap().totals["delayed-area-hp-damaging-impacts"],
        1.
    );
    assert_eq!(g.player(1).unwrap().totals["delayed-area-hp-damage"], 25.);
    let mut empty = projectile(
        "delayed-area",
        [50.5, 70.5, 4.],
        [50.5, 70.5, 0.02],
        None,
        0.,
        4.,
    );
    empty.id = 501;
    g.state.projectiles.push(empty);
    steps(&mut g, 75);
    assert_eq!(g.player(1).unwrap().totals["delayed-area-impacts"], 2.);
    assert_eq!(
        g.player(1).unwrap().totals["delayed-area-hp-damaging-impacts"],
        1.
    );
    assert_eq!(g.player(1).unwrap().totals["delayed-area-hp-damage"], 25.);
}

fn wall(owner: u32) -> Wall {
    Wall {
        id: 201,
        owner,
        pos: Pos::new(24, 45, 0),
        kind: "physical".into(),
        hp: 500.,
        maxHp: 500.,
        shield: 0.,
        invested: 20.,
        antiHeal: 0.,
    }
}

#[test]
fn damage_occurs_when_a_round_arrives_not_when_it_is_created() {
    let mut g = world();
    g.state.units.push(unit(101, 2, "vscode", 30, 45, 0));
    g.state.projectiles.push(ordinary_round());
    steps(&mut g, 1);
    assert_eq!(hp(&g, 101), 1000.);
    assert!(!g.state.projectiles.is_empty());
    steps(&mut g, 70);
    assert!((hp(&g, 101) - 900.).abs() < 0.001);
}
#[test]
fn a_penetrating_round_delivers_the_full_remaining_payload_to_its_primary_target() {
    for penetration in [0., 1., 3.] {
        let mut g = world();
        g.state.units.push(unit(101, 2, "vscode", 30, 45, 0));
        let mut round = ordinary_round();
        round.penetration = penetration;
        g.state.projectiles.push(round);
        steps(&mut g, 70);
        assert!(
            (hp(&g, 101) - 900.).abs() < 0.001,
            "penetration incorrectly discarded damage at the intended target"
        );
        assert!(g.state.projectiles.is_empty());
    }
}
#[test]
fn a_large_shell_can_be_targeted_and_hit_at_its_near_wall_outside_center_range() {
    let mut g = world();
    let mut attacker = unit(100, 1, "vscode", 20, 45, 0);
    attacker.cooldown = 0.;
    g.state.units.push(attacker);
    g.state
        .buildings
        .push(building(301, 2, "shell", 24, 40, 0, 24, 24));
    g.refresh_fog();
    let center = g
        .state
        .buildings
        .iter()
        .find(|b| b.id == 301)
        .unwrap()
        .rect
        .center();
    assert!(g.state.units[0].pos.distance(center) > catalog::unit_ref("vscode").unwrap().range);
    assert!(!g.visible_to(1, center));
    assert!(g.target_info_for(301, 1).is_some());
    steps(&mut g, 12);
    assert!(
        hp(&g, 301) < 1000.,
        "near visible facade was not hit because its distant center was used for firing range"
    );
    assert_eq!(g.state.units[0].attackCount, 1);
}
#[test]
fn own_wall_stops_a_direct_round_without_friendly_fire() {
    let mut g = world();
    g.state.units.push(unit(101, 2, "vscode", 30, 45, 0));
    g.state.walls.push(wall(1));
    g.state.projectiles.push(ordinary_round());
    steps(&mut g, 70);
    assert_eq!(hp(&g, 101), 1000.);
    assert_eq!(g.state.walls[0].hp, 500.);
    assert!(g.state.projectiles.is_empty());
}
#[test]
fn cached_geometry_observes_gate_toggle_before_the_next_round() {
    let mut g = world();
    g.state.units.push(unit(101, 2, "vscode", 26, 45, 0));
    g.state
        .buildings
        .push(building(301, 2, "shell", 24, 43, 0, 6, 6));
    g.state.entrances.push(Entrance {
        id: 302,
        owner: 2,
        pos: Pos::new(24, 45, 0),
        toLevel: 0,
        kind: "door".into(),
        hp: 250.,
        open: false,
        width: 1,
        axis: "y".into(),
        powered: false,
    });
    steps(&mut g, 1); // Warm the closed-shell collision cache before editing its opening.
    g.state.entrances[0].open = true;
    let shot = || {
        projectile(
            "direct",
            [20.5, 45.5, 0.5],
            [26.5, 45.5, 0.5],
            Some(101),
            0.,
            0.,
        )
    };
    g.state.projectiles.push(shot());
    steps(&mut g, 70);
    assert!((hp(&g, 101) - 900.).abs() < 0.001);
    g.state.entrances[0].open = false;
    g.state.projectiles.push(shot());
    steps(&mut g, 70);
    assert!(
        (hp(&g, 101) - 900.).abs() < 0.001,
        "closing the warmed gate must block the next round"
    );
    assert!(g.state.entrances[0].hp < 250.);
}
#[test]
fn moving_body_and_aim_refresh_even_when_static_geometry_is_reused() {
    let mut g = world();
    g.state.units.push(unit(101, 2, "vscode", 30, 45, 0));
    steps(&mut g, 1);
    g.state.units[0].pos = Pos::new(30, 47, 0);
    g.state.units[0].y = 47.5;
    g.state.projectiles.push(ordinary_round());
    steps(&mut g, 70);
    assert_eq!(
        hp(&g, 101),
        1000.,
        "a moved unit must leave its previous collider"
    );
    g.state.projectiles.push(projectile(
        "direct",
        [20.5, 45.5, 0.5],
        [30.5, 47.5, 0.5],
        Some(101),
        0.,
        0.,
    ));
    steps(&mut g, 70);
    assert!((hp(&g, 101) - 900.).abs() < 0.001);
}
#[test]
fn a_later_uninitialized_round_uses_live_fallback_after_its_target_dies() {
    let mut g = world();
    g.state.units.push(unit(101, 2, "vscode", 30, 45, 0));
    g.state.units.push(unit(102, 2, "vscode", 40, 50, 0));
    let mut first = projectile(
        "beam",
        [20.5, 45.5, 0.5],
        [30.5, 45.5, 0.5],
        Some(101),
        0.,
        0.,
    );
    first.damage = 1000.;
    first.duration = 0.001;
    let mut later = projectile(
        "direct",
        [20.5, 50.5, 0.5],
        [40.5, 50.5, 0.45],
        Some(101),
        0.,
        0.,
    );
    later.id = 501;
    later.aimPosition = None;
    g.state.projectiles.extend([first, later]);
    steps(&mut g, 70);
    assert!(g.state.units.iter().all(|u| u.id != 101));
    assert!(
        (hp(&g, 102) - 900.).abs() < 0.001,
        "the later round must use its destination, not the removed target's cached point"
    );
}
#[test]
fn cloned_game_does_not_share_mutable_collision_layout() {
    let mut original = world();
    original.state.units.push(unit(101, 2, "vscode", 30, 45, 0));
    original.state.walls.push(wall(1));
    steps(&mut original, 1);
    let mut edited = original.clone();
    edited.state.walls[0].pos.y = 47;
    for game in [&mut original, &mut edited] {
        game.state.projectiles.push(ordinary_round());
        steps(game, 70);
    }
    assert_eq!(hp(&original, 101), 1000.);
    assert!((hp(&edited, 101) - 900.).abs() < 0.001);
}
#[test]
fn shared_target_list_keeps_nearest_ties_and_viewer_ownership() {
    let mut g = world();
    for (id, owner, x, y) in [(100, 1, 20, 45), (101, 2, 24, 45), (102, 2, 20, 49)] {
        let mut u = unit(id, owner, "vscode", x, y, 0);
        if id == 100 {
            u.cooldown = 0.;
        }
        g.state.units.push(u);
    }
    g.refresh_fog();
    steps(&mut g, 1);
    assert_eq!(
        g.state
            .projectiles
            .iter()
            .find(|p| p.source == 100)
            .unwrap()
            .target,
        Some(101)
    );
    assert_eq!(g.state.units[0].attackCount, 1);
    assert_eq!(g.state.units[1].attackCount, 0);
}
#[test]
fn hollow_shell_allows_a_shot_through_an_open_door_and_closed_door_stops_it() {
    for open in [true, false] {
        let mut g = world();
        g.state.units.push(unit(101, 2, "vscode", 26, 45, 0));
        g.state
            .buildings
            .push(building(301, 2, "shell", 24, 43, 0, 6, 6));
        g.state.entrances.push(Entrance {
            id: 302,
            owner: 2,
            pos: Pos::new(24, 45, 0),
            toLevel: 0,
            kind: "door".into(),
            hp: 250.,
            open,
            width: 1,
            axis: "y".into(),
            powered: false,
        });
        g.state.projectiles.push(projectile(
            "direct",
            [20.5, 45.5, 0.5],
            [26.5, 45.5, 0.5],
            Some(101),
            0.,
            0.,
        ));
        steps(&mut g, 70);
        if open {
            assert!(
                hp(&g, 101) < 1000.,
                "open doorway was incorrectly a solid room-sized box"
            );
        } else {
            assert_eq!(hp(&g, 101), 1000.);
            assert!(g.state.entrances.iter().find(|e| e.id == 302).unwrap().hp < 250.);
        }
    }
}
#[test]
fn destroying_a_door_leaves_a_real_opening_for_the_next_round() {
    let mut g = world();
    g.state.units.push(unit(101, 2, "vscode", 26, 45, 0));
    g.state
        .buildings
        .push(building(301, 2, "shell", 24, 43, 0, 6, 6));
    g.state.entrances.push(Entrance {
        id: 302,
        owner: 2,
        pos: Pos::new(24, 45, 0),
        toLevel: 0,
        kind: "door".into(),
        hp: 50.,
        open: false,
        width: 1,
        axis: "y".into(),
        powered: false,
    });
    g.state.projectiles.push(ordinary_round());
    steps(&mut g, 30);
    let opening = g.state.entrances.iter().find(|e| e.id == 302).unwrap();
    assert_eq!(opening.hp, 0.);
    assert!(opening.open);
    assert_eq!(hp(&g, 101), 1000.);
    let mut next = ordinary_round();
    next.id = 501;
    g.state.projectiles.push(next);
    steps(&mut g, 70);
    assert!(
        (hp(&g, 101) - 900.).abs() < 0.001,
        "the destroyed door regenerated an invisible shell wall"
    );
}
#[test]
fn a_freestanding_closed_gate_is_a_real_ballistic_target() {
    let mut g = world();
    g.state.units.push(unit(101, 2, "vscode", 30, 45, 0));
    for (id, y) in [(202, 44), (203, 46)] {
        let mut post = wall(2);
        post.id = id;
        post.pos.y = y;
        g.state.walls.push(post);
    }
    g.state.entrances.push(Entrance {
        id: 302,
        owner: 2,
        pos: Pos::new(24, 45, 0),
        toLevel: 0,
        kind: "door".into(),
        hp: 250.,
        open: false,
        width: 1,
        axis: "y".into(),
        powered: false,
    });
    g.state.projectiles.push(ordinary_round());
    steps(&mut g, 70);
    assert_eq!(hp(&g, 101), 1000.);
    assert_eq!(
        g.state.entrances.iter().find(|e| e.id == 302).unwrap().hp,
        150.
    );
}
#[test]
fn penetration_is_spent_once_per_crossing_and_attenuates_the_remaining_payload() {
    let mut g = world();
    g.state.units.push(unit(101, 2, "vscode", 30, 45, 0));
    g.state.walls.push(wall(2));
    let mut round = ordinary_round();
    round.penetration = 1.;
    g.state.projectiles.push(round);
    steps(&mut g, 23);
    let wall_hp = g.state.walls[0].hp;
    assert!(wall_hp < 500.);
    assert!((g.state.projectiles[0].damage - 65.).abs() < 0.001);
    assert!((g.state.projectiles[0].penetration - 0.2).abs() < 0.001);
    steps(&mut g, 3);
    assert!(
        (g.state.walls[0].hp - wall_hp).abs() < 0.001,
        "same wall was charged again while the round was still inside"
    );
    steps(&mut g, 50);
    assert!((hp(&g, 101) - 935.).abs() < 0.001);
}
#[test]
fn arcing_fire_can_clear_a_wall_while_direct_fire_cannot() {
    let mut g = world();
    g.state.units.push(unit(101, 2, "vscode", 30, 45, 0));
    g.state.walls.push(wall(2));
    let mut round = ordinary_round();
    round.kind = "arc".into();
    g.state.projectiles.push(round);
    steps(&mut g, 75);
    assert!(hp(&g, 101) < 1000.);
    assert_eq!(g.state.walls[0].hp, 500.);
}
#[test]
fn roof_and_floor_protect_room_occupants_from_unpenetrating_orbital_aoe() {
    let mut g = world();
    g.state.units.push(unit(101, 2, "vscode", 26, 45, 0));
    g.state
        .buildings
        .push(building(301, 2, "shell", 25, 43, 0, 4, 4));
    g.state.projectiles.push(projectile(
        "orbital",
        [26.5, 45.5, 10.],
        [26.5, 45.5, 0.45],
        Some(101),
        0.,
        4.,
    ));
    steps(&mut g, 75);
    assert!(hp(&g, 301) < 1000.);
    assert_eq!(hp(&g, 101), 1000., "AOE leaked through an intact roof");
}
#[test]
fn earth_cover_protects_an_excavated_basement_without_a_building_roof() {
    let mut g = world();
    g.state.excavated.insert(Pos::new(26, 45, -1));
    g.state.units.push(unit(101, 2, "vscode", 26, 45, -1));
    g.state.projectiles.push(projectile(
        "orbital",
        [26.5, 45.5, 10.],
        [26.5, 45.5, -0.55],
        Some(101),
        0.,
        4.,
    ));
    steps(&mut g, 75);
    assert_eq!(hp(&g, 101), 1000.);
}
#[test]
fn structure_multiplier_applies_to_splash_as_well_as_direct_hits() {
    let mut losses = Vec::new();
    for multiplier in [1., 0.5] {
        let mut g = world();
        g.state
            .buildings
            .push(building(301, 2, "wind-power", 26, 46, 0, 2, 2));
        let mut round = projectile(
            "delayed-area",
            [24.5, 45.5, 0.4],
            [24.5, 45.5, 0.4],
            None,
            0.,
            4.,
        );
        round.structureMultiplier = multiplier;
        g.state.projectiles.push(round);
        steps(&mut g, 70);
        losses.push(1000. - hp(&g, 301));
    }
    assert!(losses[0] > 0.);
    assert!((losses[1] / losses[0] - 0.5).abs() < 0.0001);
}
#[test]
fn blast_penetration_reaches_a_wall_before_spending_the_budget_on_a_farther_floor() {
    let mut g = world();
    g.state.units.push(unit(101, 2, "vscode", 30, 45, 0));
    g.state
        .buildings
        .push(building(301, 2, "shell", 28, 43, 0, 6, 6));
    let mut cover = wall(2);
    cover.pos = Pos::new(26, 45, 1);
    g.state.walls.push(cover);
    g.state.projectiles.push(projectile(
        "delayed-area",
        [24.5, 45.5, 2.],
        [24.5, 45.5, 2.],
        None,
        1.,
        12.,
    ));
    steps(&mut g, 70);
    assert_eq!(
        hp(&g, 101),
        1000.,
        "blast spent its budget in collider storage order and leaked through the roof"
    );
}
#[test]
fn guidance_jamming_spends_finite_defense_charge_and_freezes_the_aim() {
    let mut g = world();
    g.state.units.push(unit(101, 2, "vscode", 30, 45, 0));
    let guard:Room=serde_json::from_value(json!({"id":401,"shell":0,"owner":2,"rect":{"x":23,"y":50,"z":0,"w":2,"h":2},"kind":"network-defense","branch":null,"tier":1,"hp":1000.,"maxHp":1000.,"powered":true,"connected":true,"capacity":0,"gpus":[],"inventory":40.,"progress":1.,"buildTime":5.,"cooldown":0.})).unwrap();
    g.state.rooms.push(guard);
    let mut round = ordinary_round();
    round.kind = "guided".into();
    round.duration = 2.;
    g.state.projectiles.push(round);
    steps(&mut g, 1);
    assert!(g.state.projectiles[0].jammed);
    assert!((g.state.rooms[0].inventory - 32.).abs() < 0.001);
    let aim = g.state.projectiles[0].aimPosition;
    g.state.units[0].x += 2.;
    g.state.units[0].pos.x += 2;
    steps(&mut g, 1);
    assert_eq!(g.state.projectiles[0].aimPosition, aim);
    assert!((g.state.rooms[0].inventory - 32.).abs() < 0.001);
}
#[test]
fn a_burst_uses_one_rounds_defined_damage_and_compute_cost() {
    let mut g = world();
    let mut attacker = unit(100, 1, "kimi", 20, 45, 0);
    attacker.cooldown = 0.;
    attacker.target = Some(101);
    g.state.units.push(attacker);
    g.state.units.push(unit(101, 2, "vscode", 26, 45, 0));
    g.refresh_fog();
    steps(&mut g, 1);
    let d = catalog::unit_ref("kimi").unwrap();
    let rounds = g
        .state
        .projectiles
        .iter()
        .filter(|p| p.source == 100)
        .collect::<Vec<_>>();
    assert_eq!(rounds.len(), 3);
    assert!((rounds.iter().map(|p| p.damage).sum::<f64>() - d.damage).abs() < 0.001);
    assert!((g.state.units[0].battery - (1000. - d.compute_per_attack)).abs() < 0.001);
    assert_eq!(
        g.state
            .events
            .iter()
            .filter(|e| e.kind == "compute-spent" && e.subject == 100)
            .count(),
        1
    );
}
#[test]
fn a_charged_burst_applies_one_multiplier_to_the_whole_attack_and_consumes_it_once() {
    let mut g = world();
    let mut attacker = unit(100, 1, "kimi", 20, 45, 0);
    attacker.cooldown = 0.;
    attacker.target = Some(101);
    attacker.statuses.insert("charged-shot".into(), 10.);
    attacker.chargedShotMultiplier = 1.6;
    g.state.units.push(attacker);
    g.state.units.push(unit(101, 2, "vscode", 26, 45, 0));
    g.refresh_fog();
    steps(&mut g, 1);
    let damage: f64 = g
        .state
        .projectiles
        .iter()
        .filter(|p| p.source == 100)
        .map(|p| p.damage)
        .sum();
    assert!((damage - catalog::unit_ref("kimi").unwrap().damage * 1.6).abs() < 0.001);
    assert!(!g.state.units[0].statuses.contains_key("charged-shot"));
    assert_eq!(g.state.units[0].chargedShotMultiplier, 1.);
    assert_eq!(g.state.units[0].lastAttackTick, Some(g.state.tick));
}
#[test]
fn a_failed_ammo_payment_preserves_charge_and_does_not_debit_compute() {
    let def = catalog::units()
        .into_iter()
        .find(|d| d.ammo_per_shot > 0. && d.tier == 1 && d.target_ground)
        .unwrap();
    let mut g = world();
    let mut attacker = unit(100, 1, &def.id, 20, 45, 0);
    attacker.cooldown = 0.;
    attacker.target = Some(101);
    attacker.ammo = 0.;
    attacker.statuses.insert("charged-shot".into(), 10.);
    attacker.chargedShotMultiplier = 1.92;
    g.state.units.push(attacker);
    g.state.units.push(unit(101, 2, "vscode", 26, 45, 0));
    g.refresh_fog();
    steps(&mut g, 1);
    assert!(g.state.units[0].statuses.contains_key("charged-shot"));
    assert_eq!(g.state.units[0].chargedShotMultiplier, 1.92);
    assert_eq!(g.state.units[0].battery, 1000.);
    assert_eq!(g.state.units[0].attackCount, 0);
    assert!(g.state.projectiles.iter().all(|p| p.source != 100));
}
#[test]
fn target_lock_prioritizes_and_buffs_only_the_visible_in_range_locked_target() {
    let mut g = world();
    let mut attacker = unit(100, 1, "vscode", 20, 45, 0);
    attacker.cooldown = 0.;
    attacker.statuses.insert("target-lock:102".into(), 10.);
    g.state.units.push(attacker);
    g.state.units.push(unit(101, 2, "vscode", 23, 45, 0));
    g.state.units.push(unit(102, 2, "vscode", 26, 45, 0));
    g.refresh_fog();
    steps(&mut g, 1);
    let round = g
        .state
        .projectiles
        .iter()
        .find(|p| p.source == 100)
        .unwrap();
    assert_eq!(round.target, Some(102));
    let base = catalog::unit_ref("vscode").unwrap().damage;
    assert!((round.damage - base).abs() < 0.001);
    assert_eq!(round.targetMultiplier, 1.25);
    g.state.units[0].cooldown = 999.;
    steps(&mut g, 70);
    assert!(
        (1000. - hp(&g, 101) - base).abs() < 0.001,
        "an intervening unit incorrectly received the lock bonus"
    );
    assert_eq!(hp(&g, 102), 1000.);
}
#[test]
fn status_payload_is_applied_after_actual_damage_and_not_at_shot_creation() {
    let mut g = world();
    g.state.units.push(unit(101, 2, "vscode", 30, 45, 0));
    let mut round = ordinary_round();
    round.targetMultiplier = 1.25;
    round.onHitStatus = Some(sentinels_v6::HitStatus {
        kind: "marked".into(),
        duration: 9.,
    });
    g.state.projectiles.push(round);
    steps(&mut g, 1);
    assert!(!g.state.units[0].statuses.contains_key("marked"));
    steps(&mut g, 70);
    assert!((hp(&g, 101) - 875.).abs() < 0.001);
    assert!(g.state.units[0]
        .statuses
        .get("marked")
        .is_some_and(|seconds| *seconds > 8.));
}
#[test]
fn a_landed_aircraft_is_a_ground_target_but_airborne_aircraft_is_not() {
    let tank = catalog::units()
        .into_iter()
        .find(|d| d.branch == "algorithm" && d.chassis == "tank")
        .unwrap();
    let aircraft = catalog::units()
        .into_iter()
        .find(|d| d.branch == "speed" && d.chassis == "attack-aircraft")
        .unwrap();
    for altitude in [0., 2.] {
        let mut g = world();
        let mut attacker = unit(100, 1, &tank.id, 20, 45, 0);
        attacker.cooldown = 0.;
        attacker.target = Some(101);
        g.state.units.push(attacker);
        let mut target = unit(101, 2, &aircraft.id, 26, 45, 0);
        target.altitude = altitude;
        target.flightState = if altitude > 0. { "cruising" } else { "landed" }.into();
        g.state.units.push(target);
        g.state
            .buildings
            .push(building(301, 2, "airstrip", 25, 44, 0, 4, 4));
        g.refresh_fog();
        steps(&mut g, 1);
        assert_eq!(
            g.state
                .projectiles
                .iter()
                .any(|p| p.source == 100 && p.target == Some(101)),
            altitude == 0.
        );
        if altitude == 0. {
            steps(&mut g, 13);
            assert!(
                hp(&g, 101) < 1000.,
                "runway deck incorrectly shielded a parked aircraft"
            );
            assert_eq!(
                hp(&g, 301),
                1000.,
                "horizontal fire collided with an oversized runway box"
            );
        }
    }
}
#[test]
fn automatic_targeting_skips_an_out_of_range_high_altitude_decoy() {
    let gun = catalog::units()
        .into_iter()
        .find(|d| d.tier == 1 && d.target_air && d.target_ground)
        .unwrap();
    let aircraft = catalog::units()
        .into_iter()
        .find(|d| d.category == "air" && d.tier == 5)
        .unwrap();
    let mut g = world();
    let mut attacker = unit(100, 1, &gun.id, 20, 45, 0);
    attacker.cooldown = 0.;
    g.state.units.push(attacker);
    let mut decoy = unit(101, 2, &aircraft.id, 20, 45, 0);
    decoy.altitude = 4.;
    decoy.flightState = "cruising".into();
    g.state.units.push(decoy);
    g.state.units.push(unit(102, 2, "vscode", 26, 45, 0));
    g.refresh_fog();
    steps(&mut g, 1);
    assert!(g
        .state
        .projectiles
        .iter()
        .any(|p| p.source == 100 && p.target == Some(102)));
    assert!(!g
        .state
        .projectiles
        .iter()
        .any(|p| p.source == 100 && p.target == Some(101)));
}
#[test]
fn aircraft_cannot_fire_while_taking_off() {
    let def = catalog::units()
        .into_iter()
        .find(|d| d.branch == "speed" && d.chassis == "attack-aircraft")
        .unwrap();
    let mut g = world();
    let mut attacker = unit(100, 1, &def.id, 20, 45, 0);
    attacker.cooldown = 0.;
    attacker.target = Some(101);
    attacker.altitude = 0.1;
    attacker.flightState = "taking-off".into();
    g.state.units.push(attacker);
    g.state.units.push(unit(101, 2, "vscode", 26, 45, 0));
    g.refresh_fog();
    steps(&mut g, 1);
    assert_eq!(g.state.units[0].attackCount, 0);
    assert!(g.state.projectiles.iter().all(|p| p.source != 100));
}
#[test]
fn light_payload_plugin_reduces_actual_ai_compute_debit() {
    let mut g = world();
    let mut actor = unit(100, 1, "glm", 20, 45, 0);
    actor.cooldown = 0.;
    actor.plugins.push("lightweight-attack".into());
    actor.target = Some(101);
    g.state.units.push(actor);
    g.state.units.push(unit(101, 2, "vscode", 26, 45, 0));
    g.refresh_fog();
    steps(&mut g, 1);
    let debit = g
        .state
        .events
        .iter()
        .find(|e| e.kind == "compute-spent" && e.subject == 100)
        .unwrap()
        .magnitude;
    assert!((debit - catalog::unit_ref("glm").unwrap().compute_per_attack * 0.85).abs() < 1e-7);
    assert!((g.state.units[0].battery - (1000. - debit)).abs() < 1e-7);
}
#[test]
fn deepseek_normal_hit_marks_only_after_actual_damage() {
    let mut g = world();
    let mut actor = unit(100, 1, "deepseek", 20, 45, 0);
    actor.cooldown = 0.;
    actor.target = Some(101);
    g.state.units.push(actor);
    g.state.units.push(unit(101, 2, "vscode", 26, 45, 0));
    g.refresh_fog();
    steps(&mut g, 1);
    assert!(!g.state.units[1].statuses.contains_key("marked"));
    g.state.units[0].cooldown = 999.;
    steps(&mut g, 35);
    assert!(hp(&g, 101) < 1000.);
    assert!(
        g.state.units[1]
            .statuses
            .get("marked")
            .copied()
            .unwrap_or(0.)
            > 2.
    );
}
#[test]
fn minimax_fourth_round_marks_the_payload_and_real_structure_hit() {
    for count in [2, 3] {
        let mut g = world();
        let mut actor = unit(100, 1, "minimax", 20, 45, 0);
        actor.cooldown = 0.;
        actor.target = Some(101);
        actor.attackCount = count;
        g.state.units.push(actor);
        g.state
            .buildings
            .push(building(101, 2, "wind-power", 26, 44, 0, 2, 2));
        g.refresh_fog();
        steps(&mut g, 1);
        g.state.units[0].cooldown = 999.;
        steps(&mut g, 40);
        let target = g.state.buildings.iter().find(|b| b.id == 101).unwrap();
        assert!(target.hp < 1000.);
        assert_eq!(target.antiHeal > 0., count == 3);
    }
}
#[test]
fn gemini_counts_successful_rounds_and_does_not_award_a_failed_attack() {
    let mut g = world();
    let mut actor = unit(100, 1, "gemini", 20, 45, 0);
    actor.cooldown = 0.;
    actor.target = Some(101);
    actor.attackCount = 2;
    actor.battery = 0.;
    g.state.units.push(actor);
    g.state.units.push(unit(101, 2, "vscode", 26, 45, 0));
    g.refresh_fog();
    steps(&mut g, 1);
    assert_eq!(g.state.units[0].attackCount, 2);
    assert!(!g.state.units[0].statuses.contains_key("gemini-ready"));
    g.state.units[0].battery = 1000.;
    steps(&mut g, 1);
    assert_eq!(g.state.units[0].attackCount, 3);
    assert_eq!(g.state.units[0].statuses.get("gemini-ready"), Some(&20.));
}
#[test]
fn a_paid_charge_does_not_change_when_an_attack_plugin_is_installed_later() {
    let gun = catalog::units()
        .into_iter()
        .find(|d| d.branch == "science" && d.chassis == "tank")
        .unwrap();
    let mut g = world();
    let mut actor = unit(100, 1, &gun.id, 20, 45, 0);
    actor.cooldown = 0.;
    actor.target = Some(101);
    actor.statuses.insert("charged-shot".into(), 10.);
    actor.chargedShotMultiplier = 1.6;
    actor.plugins.push("science-attack".into());
    g.state.units.push(actor);
    g.state.units.push(unit(101, 2, "vscode", 26, 45, 0));
    g.refresh_fog();
    steps(&mut g, 1);
    // Beam impacts in the same tick; inspect the emitted damage event/health.
    let expected = gun.damage * 1.6;
    let payload = g
        .state
        .projectiles
        .iter()
        .find(|p| p.source == 100)
        .map(|p| p.damage);
    if let Some(payload) = payload {
        assert!((payload - expected).abs() < 1e-7);
    } else {
        assert!((1000. - hp(&g, 101) - expected).abs() < 1e-7);
    }
    assert_eq!(g.state.units[0].chargedShotMultiplier, 1.);
}
