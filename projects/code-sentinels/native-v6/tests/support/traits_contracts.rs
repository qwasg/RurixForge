//! Isolated trait fixtures, not earned progression or executed balance evidence.
use super::*;
use serde_json::json;
fn unit(id: u64, owner: u32, kind: &str, x: i32, y: i32) -> Unit {
    let d = catalog::unit_ref(kind).unwrap();
    serde_json::from_value(json!({"id":id,"owner":owner,"kind":kind,"pos":{"x":x,"y":y,"z":0},"x":x as f64+0.5,"y":y as f64+0.5,"z":0,"tier":5,"hp":1000.,"maxHp":1000.,"battery":100.,"batteryMax":300.,"covered":false,"wired":false,"ammo":0.,"ammoMax":100.,"route":[],"target":null,"cooldown":999.,"skillCooldown":0.,"plugins":[],"statuses":{},"invested":100.,"moving":false,"attackCount":0,"branch":d.branch})).unwrap()
}
fn room(id: u64, kind: &str) -> Room {
    serde_json::from_value(json!({"id":id,"shell":1,"owner":1,"rect":{"x":21,"y":19,"z":0,"w":2,"h":2},"kind":kind,"branch":null,"tier":5,"hp":500.,"maxHp":500.,"powered":true,"online":true,"connected":true,"capacity":100,"gpus":[],"inventory":0.,"progress":1.,"buildTime":1.,"cooldown":0.})).unwrap()
}
fn world() -> Game {
    let mut g = Game::new(55, false);
    g.state.terrain.fill(0);
    g.state.units.clear();
    g.state.rooms.clear();
    g.state.links.clear();
    g.state.projectiles.clear();
    g.state.shipments.clear();
    g.state.walls.clear();
    g.state.defenseFields.clear();
    g.state.shieldRegions.clear();
    g.state.events.clear();
    g.state.networkStores.clear();
    g.next_id = 10000;
    g.invalidate_navigation();
    g
}
fn network(g: &mut Game, points: &[Pos]) {
    g.state.networkStores.push(NetworkStore {
        owner: 1,
        anchor: points[0],
        cells: points.iter().copied().collect(),
        compute: 100.,
        capacity: 100.,
        production: 0.,
    });
    g.state.links.push(Link {
        unitEndpoints: g.state.units.iter().filter(|u|u.pos==points[0]&&catalog::unit_ref(&u.kind).is_some_and(|d|d.category=="ai")).map(|u|u.id).collect(),
        id: 401,
        owner: 1,
        kind: "compute".into(),
        path: vec![Pos::new(points[0].x - 1, points[0].y, 0), points[0]],
        hp: 100.,
        active: true,
        invested: 0.,
    });
}
fn wall() -> Wall {
    Wall {
        id: 801,
        owner: 2,
        pos: Pos::new(22, 20, 0),
        kind: "physical".into(),
        hp: 1000.,
        maxHp: 1000.,
        shield: 0.,
        invested: 0.,
        antiHeal: 0.,
    }
}
fn missile(id: u64, target: u64) -> Projectile {
    serde_json::from_value(json!({"id":id,"owner":2,"source":999,"target":target,"origin":{"x":30,"y":20,"z":0},"destination":{"x":24,"y":20,"z":0},"x":23.5,"y":20.5,"z":0.5,"age":0.,"duration":2.,"damage":100.,"radius":0.,"kind":"guided","damageType":"kinetic","penetration":0.,"structureMultiplier":1.})).unwrap()
}
#[test]
fn facilities_require_live_same_owner_same_layer_same_component_and_do_not_stack() {
    for (kind, bonus) in [
        ("rapid-logistics", 0.25),
        ("secure-relay", 0.35),
        ("targeting-array", 0.2),
        ("particle-foundry", 0.2),
        ("modular-workshop", 0.1),
    ] {
        let mut g = world();
        g.state.units.push(unit(100, 1, "vscode", 20, 20));
        g.state.rooms.push(room(301, kind));
        network(&mut g, &[Pos::new(20, 20, 0), Pos::new(22, 20, 0)]);
        assert_eq!(g.facility_bonus_for_unit(&g.state.units[0], kind), bonus);
        g.state.rooms.push(room(302, kind));
        assert_eq!(g.facility_bonus_for_unit(&g.state.units[0], kind), bonus);
        g.state.rooms.pop();
        for mode in 0..7 {
            let mut bad = g.clone();
            match mode {
                0 => bad.state.rooms[0].owner = 2,
                1 => bad.state.rooms[0].powered = false,
                2 => bad.state.rooms[0].online = false,
                3 => bad.state.rooms[0].connected = false,
                4 => bad.state.rooms[0].hp = 0.,
                5 => bad.state.rooms[0].rect.level = 1,
                _ => {
                    bad.state.networkStores[0]
                        .cells
                        .remove(&Pos::new(22, 20, 0));
                    bad.state.networkStores.push(NetworkStore {
                        owner: 1,
                        anchor: Pos::new(22, 20, 0),
                        cells: [Pos::new(22, 20, 0)].into_iter().collect(),
                        compute: 100.,
                        capacity: 100.,
                        production: 0.,
                    });
                }
            }
            assert_eq!(bad.facility_bonus_for_unit(&bad.state.units[0], kind), 0.);
        }
        g.state.rooms[0].equipmentShare = 0.01;
        assert_eq!(g.facility_bonus_for_unit(&g.state.units[0], kind), 0.);
    }
}
#[test]
fn armor_support_is_local_excludes_self_and_never_stacks() {
    let mut g = world();
    g.state.units.push(unit(100, 1, "vscode", 20, 20));
    let mut escort = unit(101, 1, "claude", 23, 20);
    escort.plugins.push("security-support".into());
    g.state.units.push(escort.clone());
    assert_eq!(g.ally_armor_at(1, Pos::new(20, 20, 0), 100), 0.15);
    escort.id = 102;
    g.state.units.push(escort);
    assert_eq!(g.ally_armor_at(1, Pos::new(20, 20, 0), 100), 0.15);
    g.state.units.pop();
    assert_eq!(g.ally_armor_at(1, Pos::new(23, 20, 0), 101), 0.);
    g.state.walls.push(wall());
    assert_eq!(g.ally_armor_at(1, Pos::new(20, 20, 0), 100), 0.);
}
#[test]
fn claude_intercepts_one_real_payload_and_pays_only_once_per_cooldown() {
    let mut g = world();
    g.state.units.push(unit(100, 1, "claude", 20, 20));
    g.state.units.push(unit(101, 1, "vscode", 24, 20));
    g.state
        .projectiles
        .extend([missile(501, 101), missile(502, 101)]);
    g.passives_tick();
    assert_eq!(g.state.units[0].battery, 92.);
    assert_eq!(g.state.projectiles[0].damage, 80.);
    assert_eq!(g.state.projectiles[1].damage, 100.);
    assert_eq!(
        g.state.units[0].statuses["passive-guardian-intercept-cooldown"],
        8.
    );
    let before = serde_json::to_value(&g.state).unwrap();
    g.passives_tick();
    assert_eq!(before, serde_json::to_value(&g.state).unwrap());
}
#[test]
fn claude_cannot_intercept_through_walls_or_without_a_valid_affordable_friendly_target() {
    for scenario in 0..5 {
        let mut g = world();
        g.state.units.push(unit(100, 1, "claude", 20, 20));
        g.state.units.push(unit(101, 1, "vscode", 24, 20));
        g.state.projectiles.push(missile(501, 101));
        match scenario {
            0 => g.state.walls.push(wall()),
            1 => g.state.units[0].battery = 7.,
            2 => g.state.units[1].owner = 2,
            3 => {
                g.state.units[1].level = 1;
                g.state.units[1].pos.level = 1;
            }
            _ => g.state.projectiles[0].kind = "direct".into(),
        };
        let before = serde_json::to_value(&g.state).unwrap();
        g.passives_tick();
        assert_eq!(before, serde_json::to_value(&g.state).unwrap());
    }
}
#[test]
fn gpt_pays_connected_compute_heals_one_unit_and_does_not_apply_material_efficiency_to_healing() {
    let mut g = world();
    g.state.units.push(unit(100, 1, "gpt", 20, 20));
    let mut hurt = unit(101, 1, "glm", 24, 20);
    hurt.hp = 500.;
    hurt.plugins.push("lightweight-support".into());
    hurt.statuses.insert("anti-heal".into(), 10.);
    g.state.units.push(hurt);
    let mut other = unit(102, 1, "vscode", 25, 20);
    other.hp = 700.;
    g.state.units.push(other);
    network(&mut g, &[Pos::new(20, 20, 0)]);
    g.passives_tick();
    assert!((g.state.units[1].hp - 504.2).abs() < 1e-8);
    assert_eq!(g.state.units[2].hp, 700.);
    assert_eq!(g.state.networkStores[0].compute, 94.);
    assert_eq!(g.state.units[0].battery, 100.);
    assert_eq!(g.material_repair_factor(101), 1.2);
    assert_eq!(g.repair_multiplier(101), 0.35);
    let before = serde_json::to_value(&g.state).unwrap();
    g.passives_tick();
    assert_eq!(before, serde_json::to_value(&g.state).unwrap());
}
#[test]
fn gpt_never_spends_offline_or_without_an_affordable_visible_wounded_ally() {
    for scenario in 0..4 {
        let mut g = world();
        g.state.units.push(unit(100, 1, "gpt", 20, 20));
        let mut hurt = unit(101, 1, "vscode", 24, 20);
        hurt.hp = 500.;
        g.state.units.push(hurt);
        network(&mut g, &[Pos::new(20, 20, 0)]);
        match scenario {
            0 => g.state.links.clear(),
            1 => g.state.networkStores[0].compute = 5.,
            2 => g.state.units[1].hp = 1000.,
            _ => g.state.walls.push(wall()),
        };
        let before = serde_json::to_value(&g.state).unwrap();
        g.passives_tick();
        assert_eq!(before, serde_json::to_value(&g.state).unwrap());
    }
}
#[test]
fn trait_queries_preserve_precise_vision_and_cache_rules_without_mutating_state() {
    let mut g = world();
    let mut observer = unit(100, 1, "deepseek", 20, 20);
    observer.plugins.push("algorithm-support".into());
    g.state.units.push(observer);
    let before = serde_json::to_value(&g.state).unwrap();
    assert!((vision_radius(&g.state.units[0]) - 20.7).abs() < 1e-8);
    assert_eq!(vision_radius(&unit(101, 1, "kimi", 20, 20)), 26.);
    assert_eq!(
        initial_compute_capacity(catalog::unit_ref("glm").unwrap()),
        300.
    );
    assert_eq!(
        initial_compute_capacity(catalog::unit_ref("gpt").unwrap()),
        240.
    );
    assert_eq!(before, serde_json::to_value(&g.state).unwrap());
}
#[test]
fn hit_statuses_extend_but_never_shorten_and_do_not_resurrect_dead_targets() {
    let mut g = world();
    g.state.units.push(unit(100, 1, "minimax", 20, 20));
    g.apply_hit_status(100, "anti-heal", 10.);
    g.apply_hit_status(100, "anti-heal", 3.);
    assert_eq!(g.state.units[0].statuses["anti-heal"], 10.);
    g.apply_hit_status(100, "marked", 9.);
    g.apply_hit_status(100, "marked", 3.);
    assert_eq!(g.state.units[0].statuses["marked"], 9.);
    g.state.units[0].hp = 0.;
    let before = serde_json::to_value(&g.state).unwrap();
    g.apply_hit_status(100, "marked", 20.);
    assert_eq!(before, serde_json::to_value(&g.state).unwrap());
}
