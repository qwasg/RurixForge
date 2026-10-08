//! Flat-world contracts for V6.3 (z always 0, solid footprints, gross area capacity).
use sentinels_v6::{catalog, types::*, Game};

#[test]
fn rejects_nonzero_level_positions_in_validation() {
    let mut g = Game::new(42, false);
    let mut bad = g.state.clone();
    bad.minLevel = 0;
    bad.maxLevel = 0;
    if let Some(b) = bad.buildings.first_mut() {
        b.rect.level = 1;
    }
    assert!(bad.validate(true).is_err());
    let _ = g;
}

#[test]
fn room_capacity_equals_area_times_rate() {
    let g = Game::new(7, false);
    let rect = Rect {
        x: 20,
        y: 20,
        level: 0,
        width: 4,
        height: 4,
    };
    let capacity = g.room_capacity("data-center", rect);
    let rate = catalog::facility_ref("data-center")
        .map(|f| f.capacity_per_area)
        .unwrap_or(0.25);
    assert_eq!(capacity, ((rect.area() as f64) * rate).floor() as u32);
    assert_eq!(capacity, (16.0 * rate).floor() as u32);
}

#[test]
fn solid_buildings_block_ground_but_not_air() {
    let mut g = Game::new(9, false);
    let mut shell = g.state.buildings[0].clone();
    shell.id = 900;
    shell.kind = "shell".into();
    shell.progress = 1.;
    shell.hp = 500.;
    shell.rect = Rect {
        x: 40,
        y: 40,
        level: 0,
        width: 6,
        height: 6,
    };
    g.state.buildings.push(shell);
    g.invalidate_navigation();
    let inside = Pos::new(42, 42, 0);
    assert!(!g.walkable(inside, "ai"));
    assert!(!g.walkable(inside, "vehicle"));
    assert!(g.walkable(inside, "air"));
}

#[test]
fn wire_paths_must_stay_on_level_zero() {
    let mut g = Game::new(11, false);
    g.state.visible[0] = (0..96)
        .flat_map(|y| (0..128).map(move |x| Pos::new(x, y, 0)))
        .collect();
    let r = g.order(Order {
        owner: 1,
        sequence: 1,
        command: Command::Wire {
            kind: "power".into(),
            path: vec![Pos::new(10, 10, 1), Pos::new(11, 10, 1)],
            unit_endpoints: vec![],
        },
    });
    assert!(!r.accepted);
}
