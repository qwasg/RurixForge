//! Door destruction/placement contracts. The observer is an explicit FOW fixture.
use sentinels_v6::{Command, Game, Order, Pos, Rect, Unit};
use serde_json::json;
fn cmd(g: &mut Game, c: Command) {
    let r = g.order(Order {
        owner: 1,
        sequence: g.sequences[0] + 1,
        command: c,
    });
    assert!(r.accepted, "{}", r.reason);
}
fn observer(g: &mut Game, p: Pos) {
    let id = g.id();
    let unit:Unit=serde_json::from_value(json!({"id":id,"owner":2,"kind":"kimi","pos":p,"x":p.x as f64+0.5,"y":p.y as f64+0.5,"z":p.level,"tier":2,"hp":1000.,"maxHp":1000.,"battery":0.,"batteryMax":240.,"covered":false,"wired":false,"ammo":0.,"route":[],"target":null,"cooldown":999.,"skillCooldown":0.,"plugins":[],"statuses":{},"invested":0.,"moving":false,"attackCount":0})).unwrap();
    g.state.units.push(unit);
}
fn shell() -> (Game, u64, Rect) {
    let mut g = Game::new(11, false);
    g.state.terrain.fill(0);
    let rect = Rect {
        x: 13,
        y: 46,
        level: 0,
        width: 6,
        height: 4,
    };
    cmd(&mut g, Command::Shell { rect });
    for _ in 0..1200 {
        g.step();
    }
    let id = g
        .state
        .buildings
        .iter()
        .find(|b| b.rect == rect)
        .unwrap()
        .id;
    assert!(
        g.state
            .buildings
            .iter()
            .find(|b| b.id == id)
            .unwrap()
            .progress
            >= 1.
    );
    (g, id, rect)
}
#[test]
fn destroyed_shell_door_remains_an_opening_after_save_and_can_be_rebuilt() {
    let (mut g, shell, rect) = shell();
    let p = Pos::new(13, 48, 0);
    cmd(
        &mut g,
        Command::Entrance {
            pos: p,
            to_level: 0,
            kind: "door".into(),
            width: 1,
        },
    );
    let door = g.state.entrances.last().unwrap().id;
    cmd(
        &mut g,
        Command::ToggleEntrance {
            id: door,
            open: false,
        },
    );
    let outside = Pos::new(12, 48, 0);
    let inside = Pos::new(14, 48, 0);
    observer(&mut g, outside);
    g.refresh_fog();
    assert!(!g.can_step(outside, p, "ai"));
    assert!(!g.visible_to(2, inside));
    let net_area = g.room_net_area(rect);
    g.damage(door, 1000., "kinetic", 2);
    g.refresh_fog();
    let hole = g.state.entrances.iter().find(|e| e.id == door).unwrap();
    assert_eq!(hole.hp, 0.);
    assert!(hole.open);
    assert!(g.target_info(door).is_none());
    assert!(g.can_step(outside, p, "ai"));
    assert!(g.can_step(p, inside, "ai"));
    assert!(g.visible_to(2, inside));
    assert_eq!(g.room_net_area(rect), net_area);
    let mut g = Game::load(g.save()).unwrap();
    assert!(g.can_step(outside, p, "ai"));
    let money = g.player(1).unwrap().credits;
    cmd(
        &mut g,
        Command::Entrance {
            pos: p,
            to_level: 0,
            kind: "door".into(),
            width: 1,
        },
    );
    let replacement = g.state.entrances.last().unwrap().id;
    assert_ne!(replacement, door);
    assert!(g.state.entrances.iter().all(|e| e.id != door));
    assert_eq!(g.player(1).unwrap().credits, money - 30.);
    cmd(
        &mut g,
        Command::ToggleEntrance {
            id: replacement,
            open: false,
        },
    );
    assert!(!g.can_step(outside, p, "ai"));
    g.damage(replacement, 1000., "kinetic", 2);
    g.damage(shell, 1e12, "kinetic", 2);
    assert!(g.state.entrances.iter().all(|e| !rect.contains(e.pos)));
}
#[test]
fn a_wide_dead_door_cannot_be_partially_replaced_or_overlapped_by_a_live_door() {
    let (mut g, _, _) = shell();
    let p = Pos::new(13, 47, 0);
    cmd(
        &mut g,
        Command::Entrance {
            pos: p,
            to_level: 0,
            kind: "door".into(),
            width: 3,
        },
    );
    let id = g.state.entrances.last().unwrap().id;
    let money = g.player(1).unwrap().credits;
    let overlap = g.order(Order {
        owner: 1,
        sequence: g.sequences[0] + 1,
        command: Command::Entrance {
            pos: Pos::new(13, 46, 0),
            to_level: 0,
            kind: "door".into(),
            width: 2,
        },
    });
    assert!(!overlap.accepted);
    assert_eq!(g.player(1).unwrap().credits, money);
    g.damage(id, 1000., "kinetic", 2);
    let partial = g.order(Order {
        owner: 1,
        sequence: g.sequences[0] + 1,
        command: Command::Entrance {
            pos: p,
            to_level: 0,
            kind: "door".into(),
            width: 1,
        },
    });
    assert!(!partial.accepted);
    assert_eq!(g.player(1).unwrap().credits, money);
    assert_eq!(
        g.state.entrances.iter().find(|e| e.id == id).unwrap().width,
        3
    );
    cmd(
        &mut g,
        Command::Entrance {
            pos: p,
            to_level: 0,
            kind: "door".into(),
            width: 3,
        },
    );
    assert!(g.state.entrances.iter().all(|e| e.id != id));
}
#[test]
fn wall_gate_removes_the_original_wall_and_a_later_wall_really_blocks_the_hole() {
    let mut g = Game::new(12, false);
    g.state.terrain.fill(0);
    let p = Pos::new(4, 38, 0);
    cmd(
        &mut g,
        Command::Wall {
            kind: "physical".into(),
            path: vec![Pos::new(3, 38, 0), p, Pos::new(5, 38, 0)],
        },
    );
    let original = g.state.walls.iter().find(|w| w.pos == p).unwrap().id;
    cmd(
        &mut g,
        Command::Entrance {
            pos: p,
            to_level: 0,
            kind: "door".into(),
            width: 1,
        },
    );
    let door = g.state.entrances.last().unwrap().id;
    assert!(g.state.walls.iter().all(|w| w.id != original && w.pos != p));
    let money = g.player(1).unwrap().credits;
    let occupied = g.order(Order {
        owner: 1,
        sequence: g.sequences[0] + 1,
        command: Command::Wall {
            kind: "physical".into(),
            path: vec![p],
        },
    });
    assert!(!occupied.accepted);
    assert_eq!(g.player(1).unwrap().credits, money);
    cmd(
        &mut g,
        Command::ToggleEntrance {
            id: door,
            open: false,
        },
    );
    assert!(!g.walkable(p, "ai"));
    g.damage(door, 1000., "kinetic", 2);
    assert!(g.walkable(p, "ai"));
    assert!(g.state.walls.iter().all(|w| w.pos != p));
    observer(&mut g, Pos::new(4, 37, 0));
    g.refresh_fog();
    assert!(g.visible_to(2, Pos::new(4, 39, 0)));
    cmd(
        &mut g,
        Command::Wall {
            kind: "physical".into(),
            path: vec![p],
        },
    );
    g.refresh_fog();
    assert!(!g.walkable(p, "ai"));
    assert!(!g.visible_to(2, Pos::new(4, 39, 0)));
    assert!(g
        .state
        .entrances
        .iter()
        .any(|e| e.id == door && e.hp == 0. && e.open));
    let mut bad = g.save();
    bad.snapshot
        .entrances
        .iter_mut()
        .find(|e| e.id == door)
        .unwrap()
        .open = false;
    assert!(Game::load(bad).is_err());
}
#[test]
fn recycling_a_door_opens_a_hole_and_cannot_refund_it_twice() {
    let (mut g, _, _) = shell();
    let p = Pos::new(13, 48, 0);
    cmd(
        &mut g,
        Command::Entrance {
            pos: p,
            to_level: 0,
            kind: "door".into(),
            width: 1,
        },
    );
    let id = g.state.entrances.last().unwrap().id;
    cmd(&mut g, Command::ToggleEntrance { id, open: false });
    cmd(&mut g, Command::Recycle { id });
    assert!(g
        .state
        .entrances
        .iter()
        .any(|e| e.id == id && e.hp == 0. && e.open));
    let money = g.player(1).unwrap().credits;
    let again = g.order(Order {
        owner: 1,
        sequence: g.sequences[0] + 1,
        command: Command::Recycle { id },
    });
    assert!(!again.accepted);
    assert_eq!(g.player(1).unwrap().credits, money);
    assert!(g.can_step(Pos::new(12, 48, 0), p, "ai"));
}

#[test]
fn any_visible_cell_of_a_wide_door_publishes_the_actual_entrance() {
    let (mut g, _, _) = shell();
    let first = Pos::new(13, 47, 0);
    let last = Pos::new(13, 49, 0);
    cmd(
        &mut g,
        Command::Entrance {
            pos: first,
            to_level: 0,
            kind: "door".into(),
            width: 3,
        },
    );
    let id = g.state.entrances.last().unwrap().id;
    // Explicit observation fixture isolates visibility of the last span cell.
    g.state.visible[1].clear();
    g.state.visible[1].insert(last);
    assert!(!g.visible_to(2, first));
    assert!(g.snapshot(Some(2)).entrances.iter().any(|e| e.id == id));
    assert_eq!(g.target_info_for(id, 2), Some((last, 1)));
    g.damage(id, 1000., "kinetic", 2);
    assert!(g
        .snapshot(Some(2))
        .entrances
        .iter()
        .any(|e| e.id == id && e.hp == 0.));
    assert_eq!(g.target_info_for(id, 2), None);
}
