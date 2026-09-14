//! Device accounting fixtures; independent of campaign startup/balance acceptance.
use sentinels_v6::{Building, Command, Game, Link, Order, Pos, Room};
use serde_json::json;
fn fixture() -> Game {
    let mut g = Game::new(8, false);
    g.state.terrain.fill(0);
    g.next_id = 10000;
    let mut shell:Building=serde_json::from_value(json!({"id":700,"owner":1,"kind":"shell","rect":{"x":18,"y":38,"z":0,"w":8,"h":6},"tier":1,"hp":1000.,"maxHp":1000.,"progress":1.,"buildTime":1.,"powered":false,"connected":false,"power":0.,"demand":0.,"capacity":0,"branch":null,"inventory":0.,"invested":240.,"jam":0.,"shield":0.,"born":0})).unwrap();
    g.state.buildings.push(shell.clone());
    shell.id = 701;
    shell.kind = "wind-power".into();
    shell.rect.x = 12;
    shell.rect.width = 3;
    shell.rect.height = 3;
    shell.power = 1000.;
    g.state.buildings.push(shell);
    let mut room:Room=serde_json::from_value(json!({"id":702,"shell":700,"owner":1,"rect":{"x":20,"y":40,"z":0,"w":4,"h":2},"kind":"energy-defense","branch":null,"tier":2,"hp":280.,"maxHp":280.,"powered":false,"connected":false,"capacity":0,"gpus":[],"inventory":0.,"progress":1.,"buildTime":1.,"cooldown":0.,"invested":264.})).unwrap();
    room.capacity = g.room_capacity(&room.kind, room.rect);
    g.state.rooms.push(room);
    let mut path: Vec<_> = (13..=20).map(|x| Pos::new(x, 39, 0)).collect();
    // Both resulting rooms must retain a real power port after the split.
    // A wire ending at x20 only supplies the left half of the original room.
    path.extend((20..=23).map(|x| Pos::new(x, 40, 0)));
    g.state.links.push(Link {
        unitEndpoints: vec![],
        id: 703,
        owner: 1,
        kind: "power".into(),
        path,
        hp: 150.,
        active: false,
        invested: 18.,
    });
    g.networks();
    g
}
fn cmd(g: &mut Game, command: Command) {
    let result = g.order(Order {
        owner: 1,
        sequence: g.sequences[0] + 1,
        command,
    });
    assert!(result.accepted, "{}", result.reason);
}
#[test]
fn split_merge_cannot_duplicate_fixed_defense_capacity_or_recharge() {
    let mut g = fixture();
    assert!(g.state.rooms[0].powered);
    let demand = g.player(1).unwrap().demand;
    cmd(
        &mut g,
        Command::SplitRoom {
            id: 702,
            axis: "x".into(),
            offset: 2,
        },
    );
    assert_eq!(g.state.rooms.len(), 2);
    assert_eq!(
        g.state.rooms.iter().map(|r| r.equipmentShare).sum::<f64>(),
        1.
    );
    assert!((g.player(1).unwrap().demand - demand).abs() < 1e-6);
    assert!(g.state.rooms.iter().all(|r| r.powered));
    for _ in 0..60 {
        g.step();
    }
    assert!((g.state.rooms.iter().map(|r| r.inventory).sum::<f64>() - 15.).abs() < 1e-6);
    for _ in 0..1800 {
        g.step();
    }
    assert!((g.state.rooms.iter().map(|r| r.inventory).sum::<f64>() - 400.).abs() < 1e-6);
    let ids = g.state.rooms.iter().map(|r| r.id).collect();
    cmd(&mut g, Command::MergeRooms { ids });
    assert_eq!(g.state.rooms.len(), 1);
    assert_eq!(g.state.rooms[0].equipmentShare, 1.);
    assert_eq!(g.state.rooms[0].inventory, 400.);
}
#[test]
fn conversion_buys_a_full_device_and_cancel_restores_the_original_share() {
    let mut g = fixture();
    cmd(
        &mut g,
        Command::SplitRoom {
            id: 702,
            axis: "x".into(),
            offset: 2,
        },
    );
    let cash = g.player(1).unwrap().credits;
    assert_eq!(
        g.state
            .rooms
            .iter()
            .find(|r| r.id == 702)
            .unwrap()
            .equipmentShare,
        0.5
    );
    cmd(
        &mut g,
        Command::ConvertRoom {
            id: 702,
            kind: "depot".into(),
            branch: None,
        },
    );
    assert_eq!(
        g.state
            .rooms
            .iter()
            .find(|r| r.id == 702)
            .unwrap()
            .equipmentShare,
        1.
    );
    assert!(g.player(1).unwrap().credits < cash);
    cmd(&mut g, Command::Cancel { id: 702 });
    assert_eq!(
        g.state
            .rooms
            .iter()
            .find(|r| r.id == 702)
            .unwrap()
            .equipmentShare,
        0.5
    );
    assert!(g.player(1).unwrap().credits <= cash);
    let mut bad = g.save();
    bad.snapshot.rooms[0].equipmentShare = f64::NAN;
    assert!(Game::load(bad).is_err());
}
