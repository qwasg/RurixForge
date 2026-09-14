// Ordinary paid-command setup copied from the existing gameplay tests; no injected resources.
use sentinels_v6::{Command, Game, Order, Pos, Rect};
pub fn cmd(g: &mut Game, owner: u32, command: Command) {
    let sequence = g.sequences[(owner - 1) as usize] + 1;
    let receipt = g.order(Order {
        owner,
        sequence,
        command,
    });
    assert!(receipt.accepted, "{}", receipt.reason);
}
pub fn wait(g: &mut Game, seconds: u64) {
    for _ in 0..seconds * 60 {
        g.step();
    }
}
pub fn line(a: Pos, b: Pos) -> Vec<Pos> {
    let mut p = a;
    let mut out = vec![p];
    while p.x != b.x {
        p.x += (b.x - p.x).signum();
        out.push(p);
    }
    while p.y != b.y {
        p.y += (b.y - p.y).signum();
        out.push(p);
    }
    out
}
pub fn opening() -> Game {
    let mut g = Game::new(1, false);
    cmd(
        &mut g,
        1,
        Command::Shell {
            rect: Rect {
                x: 13,
                y: 46,
                level: 0,
                width: 6,
                height: 4,
            },
        },
    );
    cmd(
        &mut g,
        1,
        Command::Build {
            pos: Pos::new(13, 42, 0),
            kind: "wind-power".into(),
        },
    );
    cmd(
        &mut g,
        1,
        Command::Build {
            pos: Pos::new(16, 40, 0),
            kind: "extractor".into(),
        },
    );
    wait(&mut g, 20);
    let shell = g
        .state
        .buildings
        .iter()
        .find(|b| b.kind == "shell")
        .unwrap()
        .id;
    assert_eq!(
        g.state
            .buildings
            .iter()
            .find(|b| b.id == shell)
            .unwrap()
            .progress,
        1.
    );
    cmd(
        &mut g,
        1,
        Command::Entrance {
            pos: Pos::new(13, 48, 0),
            to_level: 0,
            kind: "door".into(),
            width: 1,
        },
    );
    cmd(
        &mut g,
        1,
        Command::Room {
            shell,
            rect: Rect {
                x: 13,
                y: 46,
                level: 0,
                width: 2,
                height: 2,
            },
            kind: "data-center".into(),
            branch: None,
        },
    );
    cmd(
        &mut g,
        1,
        Command::Room {
            shell,
            rect: Rect {
                x: 17,
                y: 46,
                level: 0,
                width: 2,
                height: 2,
            },
            kind: "research-lab".into(),
            branch: Some("algorithm".into()),
        },
    );
    wait(&mut g, 25);
    let dc = g
        .state
        .rooms
        .iter()
        .find(|r| r.kind == "data-center")
        .unwrap()
        .id;
    let lab = g
        .state
        .rooms
        .iter()
        .find(|r| r.kind == "research-lab")
        .unwrap()
        .id;
    assert!(
        g.state.rooms.iter().all(|r| r.progress >= 1.),
        "{:?}",
        g.state.jobs
    );
    let mut power = line(Pos::new(13, 42, 0), Pos::new(13, 46, 0));
    power.extend(
        line(Pos::new(13, 46, 0), Pos::new(18, 46, 0))
            .into_iter()
            .skip(1),
    );
    cmd(
        &mut g,
        1,
        Command::Wire {
            unit_endpoints: vec![],
            kind: "power".into(),
            path: power,
        },
    );
    cmd(
        &mut g,
        1,
        Command::InstallGpu {
            room: dc,
            model: "rtx-5060".into(),
        },
    );
    cmd(
        &mut g,
        1,
        Command::Wire {
            unit_endpoints: vec![],
            kind: "compute".into(),
            path: line(Pos::new(13, 46, 0), Pos::new(18, 46, 0)),
        },
    );
    cmd(
        &mut g,
        1,
        Command::Deploy {
            room: lab,
            kind: "vscode".into(),
            pos: Pos::new(12, 52, 0),
        },
    );
    cmd(
        &mut g,
        1,
        Command::Deploy {
            room: lab,
            kind: "pycharm".into(),
            pos: Pos::new(10, 52, 0),
        },
    );
    let mut cable = line(Pos::new(13, 46, 0), Pos::new(12, 46, 0));
    cable.extend(
        line(Pos::new(12, 46, 0), Pos::new(12, 52, 0))
            .into_iter()
            .skip(1),
    );
    cmd(
        &mut g,
        1,
        Command::Wire {
            unit_endpoints: vec![],
            kind: "compute".into(),
            path: cable,
        },
    );
    cmd(
        &mut g,
        1,
        Command::Wire {
            unit_endpoints: vec![],
            kind: "compute".into(),
            path: line(Pos::new(12, 52, 0), Pos::new(10, 52, 0)),
        },
    );
    wait(&mut g, 2);
    g
}
