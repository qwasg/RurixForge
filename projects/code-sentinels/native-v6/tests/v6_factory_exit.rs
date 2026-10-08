//! Ordinary paid commands prove ground vehicles spawn on the factory exterior ring.
use sentinels_v6::{catalog, Command, Pos, Rect};
#[path = "support/paid_opening.rs"]
mod support;
use support::{cmd, line, opening, wait};

#[test]
fn paid_vehicle_spawns_outside_factory_and_drives_to_rally() {
    let mut g = opening();
    while g.player(1).unwrap().credits < 900. {
        wait(&mut g, 1);
        assert!(
            g.state.tick < 600 * 60,
            "opening economy cannot fund factory"
        );
    }
    let rect = Rect {
        x: 2,
        y: 38,
        level: 0,
        width: 6,
        height: 4,
    };
    cmd(&mut g, 1, Command::Shell { rect });
    wait(&mut g, 20);
    let shell = g
        .state
        .buildings
        .iter()
        .find(|b| b.rect == rect && b.kind == "shell")
        .unwrap()
        .id;
    let room = Rect {
        x: 3,
        y: 38,
        level: 0,
        width: 4,
        height: 2,
    };
    cmd(
        &mut g,
        1,
        Command::Room {
            shell,
            rect: room,
            kind: "factory".into(),
            branch: None,
        },
    );
    cmd(
        &mut g,
        1,
        Command::Build {
            pos: Pos::new(8, 38, 0),
            kind: "wind-power".into(),
        },
    );
    wait(&mut g, 30);
    let factory = g
        .state
        .rooms
        .iter()
        .find(|r| r.shell == shell && r.kind == "factory")
        .unwrap()
        .id;
    cmd(
        &mut g,
        1,
        Command::Wire {
            unit_endpoints: vec![],
            kind: "power".into(),
            path: line(Pos::new(8, 38, 0), Pos::new(3, 38, 0)),
        },
    );
    assert!(
        g.state
            .rooms
            .iter()
            .find(|r| r.id == factory)
            .unwrap()
            .powered
    );
    let dc = g
        .state
        .rooms
        .iter()
        .find(|r| r.owner == 1 && r.kind == "data-center")
        .unwrap()
        .rect
        .center();
    cmd(
        &mut g,
        1,
        Command::Wire {
            kind: "compute".into(),
            path: line(dc, Pos::new(3, 39, 0)),
            unit_endpoints: vec![],
        },
    );
    assert!(
        g.state
            .rooms
            .iter()
            .find(|r| r.id == factory)
            .unwrap()
            .connected
    );
    let def = catalog::units()
        .into_iter()
        .filter(|d| d.category == "vehicle" && d.branch == "algorithm" && d.tier == 1)
        .min_by(|a, b| a.cost.total_cmp(&b.cost))
        .expect("branch has an initial scout chassis");
    let rally = Pos::new(6, 45, 0);
    cmd(
        &mut g,
        1,
        Command::Deploy {
            room: factory,
            kind: def.id.clone(),
            pos: rally,
        },
    );
    let created = g.state.units.iter().find(|u| u.kind == def.id).unwrap();
    let id = created.id;
    assert_ne!(created.pos, rally);
    assert!(
        !rect.contains(created.pos),
        "vehicle must spawn on the exterior ring, not inside the solid shell"
    );
    assert_eq!(created.sourceFacility, factory);
    assert_eq!(created.goal, Some(rally));
    assert!(
        !created.wired,
        "a factory compute port must not bind its newly produced vehicle"
    );
    let mut previous = created.pos;
    for _ in 0..480 {
        g.step();
        let unit = g.state.units.iter().find(|u| u.id == id).unwrap();
        assert!(previous.distance(unit.pos) <= 1.0001, "vehicle teleported");
        previous = unit.pos;
    }
    let unit = g.state.units.iter().find(|u| u.id == id).unwrap();
    assert_eq!(unit.pos, rally);
    assert!(unit.route.is_empty());
}
