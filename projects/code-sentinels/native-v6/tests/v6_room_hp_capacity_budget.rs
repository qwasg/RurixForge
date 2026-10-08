//! Isolated flat-layout fixtures using actual paid room commands, not balance evidence.
//! Net area equals gross area (no door/window occupancy deductions).
use sentinels_v6::{catalog, Command, Game, Order, Rect, Room};

fn order(g: &mut Game, command: Command) {
    let receipt = g.order(Order {
        owner: 1,
        sequence: g.sequences[0] + 1,
        command,
    });
    assert!(receipt.accepted, "{}", receipt.reason);
}

fn complete_jobs(g: &mut Game) {
    for _ in 0..3600 {
        if g.state.jobs.is_empty() {
            return;
        }
        g.step();
    }
    panic!("ordinary construction did not complete: {:?}", g.state.jobs);
}

fn layout() -> (Game, u64) {
    let mut g = Game::new(603201, false);
    g.state.terrain.fill(0);
    let rect = Rect {
        x: 2,
        y: 38,
        level: 0,
        width: 6,
        height: 4,
    };
    order(&mut g, Command::Shell { rect });
    complete_jobs(&mut g);
    let shell = g
        .state
        .buildings
        .iter()
        .find(|b| b.owner == 1 && b.kind == "shell")
        .unwrap()
        .id;
    (g, shell)
}

fn room(g: &mut Game, shell: u64, x: i32, width: i32, height: i32) -> u64 {
    let rect = Rect {
        x,
        y: 38,
        level: 0,
        width,
        height,
    };
    order(
        g,
        Command::Room {
            shell,
            rect,
            kind: "data-center".into(),
            branch: None,
        },
    );
    g.state
        .rooms
        .iter()
        .find(|r| r.owner == 1 && r.rect == rect)
        .unwrap()
        .id
}

fn get(g: &Game, id: u64) -> &Room {
    g.state.rooms.iter().find(|r| r.id == id).unwrap()
}

fn attempt(g: &mut Game, command: Command) -> bool {
    g.order(Order {
        owner: 1,
        sequence: g.sequences[0] + 1,
        command,
    })
    .accepted
}

fn bpc(g: &Game, id: u64) -> (u32, u32, u32) {
    let r = get(g, id);
    (
        r.capacityBudget.unwrap(),
        r.potentialCapacity.unwrap(),
        r.capacity,
    )
}

fn split(g: &mut Game, id: u64) -> Vec<u64> {
    order(
        g,
        Command::SplitRoom {
            id,
            axis: "x".into(),
            offset: 2,
        },
    );
    g.state
        .rooms
        .iter()
        .filter(|r| r.owner == 1)
        .map(|r| r.id)
        .collect()
}

fn separate_six_cell_rooms() -> (Game, u64, u64) {
    let (mut g, shell) = layout();
    let a = room(&mut g, shell, 2, 2, 3);
    let b = room(&mut g, shell, 4, 2, 3);
    complete_jobs(&mut g);
    (g, a, b)
}

#[test]
fn newly_built_room_hp_grows_sublinearly_from_four_cell_baseline() {
    let (mut g, shell) = layout();
    let id = room(&mut g, shell, 2, 4, 2);
    let r = g.state.rooms.iter().find(|r| r.id == id).unwrap();
    let expected = 140. * 2_f64.powf(0.85);
    assert!((r.maxHp - expected).abs() < 1e-8);
    assert!((r.hp - r.maxHp * 0.1).abs() < 1e-8);
    g.damage(id, expected * 0.05, "kinetic", 2);
    complete_jobs(&mut g);
    assert!((get(&g, id).hp - expected * 0.95).abs() < 1e-7);
}

#[test]
fn two_separately_paid_six_cell_rooms_cannot_gain_a_third_slot_by_merging() {
    let (mut g, shell) = layout();
    let a = room(&mut g, shell, 2, 2, 3);
    let b = room(&mut g, shell, 4, 2, 3);
    complete_jobs(&mut g);
    let old = g
        .state
        .rooms
        .iter()
        .filter(|r| r.id == a || r.id == b)
        .map(|r| r.capacity)
        .sum::<u32>();
    assert_eq!(old, 2);
    order(&mut g, Command::MergeRooms { ids: vec![a, b] });
    assert_eq!(get(&g, a).capacity, old);
    assert_eq!(bpc(&g, a), (2, 3, 2));
}

#[test]
fn hp_baseline_and_marginal_area_growth_are_sublinear() {
    let rect = Rect {
        x: 2,
        y: 38,
        level: 0,
        width: 2,
        height: 2,
    };
    assert_eq!(Game::room_hp(rect), 140.);
    let mut previous_hp = 140.;
    let mut previous_density = 35.;
    for (width, height) in [(4, 2), (4, 4), (8, 4), (8, 8), (24, 24)] {
        let r = Rect {
            width,
            height,
            ..rect
        };
        let hp = Game::room_hp(r);
        assert!(hp > previous_hp && hp / (r.area() as f64) < previous_density);
        previous_hp = hp;
        previous_density = hp / r.area() as f64;
    }
}

#[test]
fn split_remainders_preserve_purchased_budget_hp_and_investment() {
    let (mut g, shell) = layout();
    let id = room(&mut g, shell, 2, 4, 3);
    complete_jobs(&mut g);
    g.damage(id, 40., "kinetic", 2);
    let original = get(&g, id).clone();
    assert_eq!(bpc(&g, id), (3, 3, 3));
    for _ in 0..3 {
        let ids = split(&mut g, id);
        let parts: Vec<_> = ids.iter().map(|id| get(&g, *id)).collect();
        assert_eq!(
            parts.iter().map(|r| r.capacityBudget.unwrap()).sum::<u32>(),
            3
        );
        assert_eq!(parts.iter().map(|r| r.capacity).sum::<u32>(), 2);
        assert_eq!(
            parts
                .iter()
                .map(|r| r.capacityBudget.unwrap())
                .collect::<Vec<_>>(),
            vec![1, 2]
        );
        for (actual, expected) in [
            (parts.iter().map(|r| r.hp).sum::<f64>(), original.hp),
            (parts.iter().map(|r| r.maxHp).sum::<f64>(), original.maxHp),
            (
                parts.iter().map(|r| r.invested).sum::<f64>(),
                original.invested,
            ),
        ] {
            assert!((actual - expected).abs() < 1e-8);
        }
        order(&mut g, Command::MergeRooms { ids });
        assert_eq!(bpc(&g, id), (3, 3, 3));
    }
}

#[test]
fn merged_budget_stays_capped_without_entrance_rounding() {
    let (mut g, a, b) = separate_six_cell_rooms();
    order(&mut g, Command::MergeRooms { ids: vec![a, b] });
    assert_eq!(bpc(&g, a), (2, 3, 2));
    for _ in 0..2 {
        order(
            &mut g,
            Command::InstallGpu {
                room: a,
                model: "rtx-5060".into(),
            },
        );
    }
    let credits = g.player(1).unwrap().credits;
    assert!(!attempt(
        &mut g,
        Command::InstallGpu {
            room: a,
            model: "rtx-5060".into(),
        }
    ));
    assert_eq!(g.player(1).unwrap().credits, credits);
}

#[test]
fn partition_cannot_hide_installed_gpu_overflow() {
    let (mut g, shell) = layout();
    let id = room(&mut g, shell, 2, 4, 3);
    complete_jobs(&mut g);
    for _ in 0..3 {
        order(
            &mut g,
            Command::InstallGpu {
                room: id,
                model: "rtx-5060".into(),
            },
        );
    }
    let saved = get(&g, id).clone();
    let credits = g.player(1).unwrap().credits;
    assert!(!attempt(
        &mut g,
        Command::SplitRoom {
            id,
            axis: "x".into(),
            offset: 2,
        }
    ));
    assert_eq!(bpc(&g, id), (3, 3, 3));
    assert_eq!(get(&g, id).gpus, saved.gpus);
    assert_eq!(g.player(1).unwrap().credits, credits);
}

#[test]
fn same_kind_paid_refit_expands_budget_without_healing() {
    let (mut g, a, b) = separate_six_cell_rooms();
    order(&mut g, Command::MergeRooms { ids: vec![a, b] });
    g.damage(a, 30., "kinetic", 2);
    let original = get(&g, a).clone();
    let credits = g.player(1).unwrap().credits;
    order(
        &mut g,
        Command::ConvertRoom {
            id: a,
            kind: "data-center".into(),
            branch: None,
        },
    );
    assert!((credits - g.player(1).unwrap().credits - 78.).abs() < 1e-8);
    assert_eq!(bpc(&g, a), (3, 3, 3));
    assert_eq!(get(&g, a).hp, original.hp);
    complete_jobs(&mut g);
    assert_eq!(get(&g, a).hp, original.hp);
}

#[test]
fn cancelled_new_room_has_no_remaining_hp_budget_or_duplicate_refund() {
    let (mut g, shell) = layout();
    let credits = g.player(1).unwrap().credits;
    let id = room(&mut g, shell, 2, 4, 2);
    order(&mut g, Command::Cancel { id });
    assert!(!g.state.rooms.iter().any(|r| r.id == id));
    let cash = g.player(1).unwrap().credits;
    assert!(cash < credits);
    assert!(!attempt(&mut g, Command::Cancel { id }));
    assert_eq!(g.player(1).unwrap().credits, cash);
}

#[test]
fn repair_uses_purchased_historical_max_hp_with_credits_only() {
    let (mut g, a, b) = separate_six_cell_rooms();
    order(&mut g, Command::MergeRooms { ids: vec![a, b] });
    let maximum = get(&g, a).maxHp;
    assert!(maximum > Game::room_hp(get(&g, a).rect));
    g.damage(a, maximum * 0.8, "kinetic", 2);
    let investment = get(&g, a).invested;
    let credits = g.player(1).unwrap().credits;
    order(&mut g, Command::Repair { id: a });
    assert!((credits - g.player(1).unwrap().credits - 20.).abs() < 1e-8);
    complete_jobs(&mut g);
    assert!((get(&g, a).hp - maximum * 0.55).abs() < 1e-7);
    assert_eq!(get(&g, a).maxHp, maximum);
    assert_eq!(get(&g, a).invested, investment);
}

#[test]
fn full_save_load_preserves_split_remainder_and_rejects_invalid_capacity() {
    let (mut g, shell) = layout();
    let id = room(&mut g, shell, 2, 4, 3);
    complete_jobs(&mut g);
    let ids = split(&mut g, id);
    let saved = g.save();
    let restored = Game::load(saved.clone()).unwrap();
    assert_eq!(
        serde_json::to_value(&g.state).unwrap(),
        serde_json::to_value(&restored.state).unwrap()
    );
    assert_eq!(
        restored
            .state
            .rooms
            .iter()
            .map(|r| r.capacityBudget.unwrap())
            .sum::<u32>(),
        3
    );
    let mut merged = restored;
    order(&mut merged, Command::MergeRooms { ids });
    assert_eq!(bpc(&merged, id), (3, 3, 3));
}

#[test]
fn ordinary_paid_refit_replays_its_exact_budget_and_hp_history() {
    let mut g = Game::new(63, false);
    let shell_rect = Rect {
        x: 13,
        y: 46,
        level: 0,
        width: 6,
        height: 4,
    };
    order(&mut g, Command::Shell { rect: shell_rect });
    complete_jobs(&mut g);
    let shell = g
        .state
        .buildings
        .iter()
        .find(|b| b.owner == 1 && b.kind == "shell")
        .unwrap()
        .id;
    for x in [13, 15] {
        order(
            &mut g,
            Command::Room {
                shell,
                rect: Rect {
                    x,
                    y: 46,
                    level: 0,
                    width: 2,
                    height: 3,
                },
                kind: "data-center".into(),
                branch: None,
            },
        );
    }
    complete_jobs(&mut g);
    let ids = g
        .state
        .rooms
        .iter()
        .filter(|r| r.owner == 1)
        .map(|r| r.id)
        .collect::<Vec<_>>();
    let id = ids[0];
    order(&mut g, Command::MergeRooms { ids });
    assert_eq!(bpc(&g, id), (2, 3, 2));
    order(
        &mut g,
        Command::ConvertRoom {
            id,
            kind: "data-center".into(),
            branch: None,
        },
    );
    let pending = g.save();
    assert!(Game::replay(&pending).unwrap());
    complete_jobs(&mut g);
    assert_eq!(bpc(&g, id), (3, 3, 3));
    assert_eq!(catalog::facility_ref("data-center").unwrap().cost, 120.);
}
