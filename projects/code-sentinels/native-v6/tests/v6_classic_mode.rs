//! Classic ruleset: single-layer tower defense with credits-only placement.
use sentinels_v6::{catalog, types::*, Game};

#[path = "support/paid_opening.rs"]
mod support;
use support::wait;

fn classic() -> Game {
    Game::new_ruleset(1, false, "river", RULESET_CLASSIC)
}

fn core(g: &Game, owner: u32) -> Building {
    g.state
        .buildings
        .iter()
        .find(|b| b.owner == owner && b.kind == "core")
        .cloned()
        .unwrap()
}

fn send(g: &mut Game, owner: u32, command: Command) -> Receipt {
    let sequence = g.sequences[(owner - 1) as usize] + 1;
    g.order(Order {
        owner,
        sequence,
        command,
    })
}

fn accept(g: &mut Game, owner: u32, command: Command) {
    let receipt = send(g, owner, command);
    assert!(receipt.accepted, "{}", receipt.reason);
}

/// Ground cell at `distance` cells east of the core centre.
fn east_of_core(g: &Game, owner: u32, distance: i32) -> Pos {
    let centre = core(g, owner).rect.center();
    Pos::new(centre.x + distance, centre.y, 0)
}

#[test]
fn new_ruleset_records_classic_and_default_stays_full() {
    assert_eq!(classic().state.ruleset, RULESET_CLASSIC);
    assert_eq!(Game::new(1, false).state.ruleset, RULESET_FULL);
    assert_eq!(
        Game::new_theme(1, false, "mining").state.ruleset,
        RULESET_FULL
    );
    assert_eq!(
        Game::new_ruleset(1, false, "river", "nonsense").state.ruleset,
        RULESET_FULL
    );
}

#[test]
fn base_building_commands_are_rejected() {
    let mut g = classic();
    g.state.visible[0] = (0..96)
        .flat_map(|y| (0..128).map(move |x| Pos::new(x, y, 0)))
        .collect();
    let rect = Rect {
        x: 20,
        y: 44,
        level: 0,
        width: 6,
        height: 4,
    };
    for command in [
        Command::Shell { rect },
        Command::ExpandShell { id: 1, rect },
        Command::Room {
            shell: 1,
            rect,
            kind: "data-center".into(),
            branch: None,
        },
        Command::SplitRoom {
            id: 1,
            axis: "x".into(),
            offset: 2,
        },
        Command::MergeRooms { ids: vec![1, 2] },
        Command::ConvertRoom {
            id: 1,
            kind: "factory".into(),
            branch: None,
        },
        Command::InstallGpu {
            room: 1,
            model: "rtx-5060".into(),
        },
        Command::RemoveGpu { room: 1, bay: 0 },
        Command::Wire {
            kind: "power".into(),
            path: vec![Pos::new(20, 44, 0), Pos::new(21, 44, 0)],
            unit_endpoints: vec![],
        },
        Command::Wall {
            kind: "cuda".into(),
            path: vec![Pos::new(20, 44, 0)],
        },
        Command::Shield { enabled: false },
    ] {
        let receipt = send(&mut g, 1, command.clone());
        assert!(!receipt.accepted, "classic must reject {command:?}");
        assert_eq!(receipt.reason, "单层塔防模式不支持该操作");
    }
    assert!(g.state.buildings.iter().all(|b| b.kind != "shell"));
}

#[test]
fn physical_walls_and_allowed_outdoor_buildings_still_work() {
    let mut g = classic();
    g.state.visible[0] = (0..96)
        .flat_map(|y| (0..128).map(move |x| Pos::new(x, y, 0)))
        .collect();
    accept(
        &mut g,
        1,
        Command::Wall {
            kind: "physical".into(),
            path: vec![Pos::new(14, 46, 0), Pos::new(14, 47, 0)],
        },
    );
    let ore = g
        .state
        .resources
        .iter()
        .filter(|r| r.kind == "ore")
        .min_by(|a, b| {
            let home = Pos::new(10, 48, 0);
            a.pos.distance(home).total_cmp(&b.pos.distance(home))
        })
        .unwrap()
        .pos;
    accept(
        &mut g,
        1,
        Command::Build {
            pos: Pos::new(ore.x, ore.y, 0),
            kind: "extractor".into(),
        },
    );
    for kind in ["wind-power", "mobile-relay"] {
        let receipt = send(
            &mut g,
            1,
            Command::Build {
                pos: Pos::new(26, 60, 0),
                kind: kind.into(),
            },
        );
        assert!(!receipt.accepted, "classic must reject {kind}");
        assert_eq!(receipt.reason, "单层塔防模式只能建造采集器与露天跑道");
    }
}

#[test]
fn turrets_deploy_without_a_lab_inside_the_radius() {
    let mut g = classic();
    let near = east_of_core(&g, 1, 6);
    accept(
        &mut g,
        1,
        Command::Deploy {
            room: 0,
            kind: "vscode".into(),
            pos: near,
        },
    );
    let unit = g.state.units.iter().find(|u| u.kind == "vscode").unwrap();
    assert_eq!(unit.pos, near);
    assert_eq!(unit.sourceFacility, core(&g, 1).id);
    assert!(g.state.rooms.is_empty(), "classic never creates rooms");
}

#[test]
fn deploying_beyond_the_radius_is_rejected() {
    let mut g = classic();
    let far = east_of_core(&g, 1, catalog::CLASSIC_DEPLOY_RADIUS as i32 + 1);
    let receipt = send(
        &mut g,
        1,
        Command::Deploy {
            room: 0,
            kind: "vscode".into(),
            pos: far,
        },
    );
    assert!(!receipt.accepted);
    assert_eq!(receipt.reason, "部署位置须在指挥核心或己方已完工建筑12格内");
}

#[test]
fn a_completed_extractor_extends_the_deploy_anchor() {
    let mut g = classic();
    g.state.visible[0] = (0..96)
        .flat_map(|y| (0..128).map(move |x| Pos::new(x, y, 0)))
        .collect();
    let home = core(&g, 1).rect.center();
    let ore = g
        .state
        .resources
        .iter()
        .filter(|r| {
            matches!(r.kind.as_str(), "ore" | "coal")
                && r.pos.distance(home) > catalog::CLASSIC_DEPLOY_RADIUS + 4.
        })
        .min_by(|a, b| a.pos.distance(home).total_cmp(&b.pos.distance(home)))
        .expect("fixture needs a mine outside the core radius")
        .pos;
    accept(
        &mut g,
        1,
        Command::Build {
            pos: ore,
            kind: "extractor".into(),
        },
    );
    wait(&mut g, 40);
    let extractor = g
        .state
        .buildings
        .iter()
        .find(|b| b.kind == "extractor")
        .cloned()
        .unwrap();
    assert!(
        extractor.progress >= 1.,
        "extractor must finish: {:?}",
        g.state.jobs
    );
    assert!(extractor.powered, "classic keeps completed buildings online");
    let centre = extractor.rect.center();
    let beside = (1..=4)
        .flat_map(|d| {
            [
                Pos::new(centre.x - d, centre.y, 0),
                Pos::new(centre.x + d, centre.y, 0),
                Pos::new(centre.x, centre.y - d, 0),
                Pos::new(centre.x, centre.y + d, 0),
            ]
        })
        .find(|p| {
            p.distance(home) > catalog::CLASSIC_DEPLOY_RADIUS && g.walkable(*p, "turret")
        })
        .expect("a walkable cell beside the extractor");
    accept(
        &mut g,
        1,
        Command::Deploy {
            room: 0,
            kind: "pycharm".into(),
            pos: beside,
        },
    );
}

#[test]
fn deploy_costs_credits_only_and_never_compute() {
    let mut g = classic();
    let before = g.player(1).unwrap().credits;
    assert_eq!(g.player(1).unwrap().compute, 0.);
    let post = east_of_core(&g, 1, 5);
    accept(
        &mut g,
        1,
        Command::Deploy {
            room: 0,
            kind: "vscode".into(),
            pos: post,
        },
    );
    let cost = catalog::unit("vscode").unwrap().cost;
    assert!((before - g.player(1).unwrap().credits - cost).abs() < 1e-6);
    assert_eq!(g.player(1).unwrap().compute, 0.);
}

#[test]
fn research_runs_at_the_core_for_credits_only() {
    let mut g = classic();
    let before = g.player(1).unwrap().credits;
    accept(
        &mut g,
        1,
        Command::Research {
            room: 0,
            branch: "algorithm".into(),
        },
    );
    assert!((before - g.player(1).unwrap().credits - catalog::RESEARCH_CREDITS[0]).abs() < 1e-6);
    let task = g.player(1).unwrap().researches.first().cloned().unwrap();
    assert_eq!(task.lab, core(&g, 1).id);
    assert_eq!(task.target, 1);
    wait(&mut g, catalog::RESEARCH_SECONDS[0] as u64 + 2);
    assert_eq!(g.tech(1, "algorithm"), 1);
    assert!(g.player(1).unwrap().researches.is_empty());
    let receipt = send(
        &mut g,
        1,
        Command::Research {
            room: 0,
            branch: "nonsense".into(),
        },
    );
    assert!(!receipt.accepted);
    assert_eq!(receipt.reason, "未知科技分支");
}

#[test]
fn researched_tiers_unlock_branch_units_without_rooms() {
    let mut g = classic();
    for _ in 0..2 {
        accept(
            &mut g,
            1,
            Command::Research {
                room: 0,
                branch: "algorithm".into(),
            },
        );
        let target = g.player(1).unwrap().researches[0].target;
        wait(&mut g, catalog::RESEARCH_SECONDS[(target - 1) as usize] as u64 + 2);
    }
    assert_eq!(g.tech(1, "algorithm"), 2);
    let def = catalog::units()
        .into_iter()
        .find(|d| d.branch == "algorithm" && d.category == "vehicle" && d.tier == 2)
        .unwrap();
    g.player_mut(1).unwrap().credits += def.cost * 2.;
    let post = east_of_core(&g, 1, 7);
    accept(
        &mut g,
        1,
        Command::Deploy {
            room: 0,
            kind: def.id.clone(),
            pos: post,
        },
    );
    let unit = g.state.units.iter().find(|u| u.kind == def.id).unwrap();
    assert_eq!(unit.tier, 2);
    assert_eq!(
        unit.pos, post,
        "classic vehicles spawn on the requested cell, not a factory ring"
    );
}

#[test]
fn turrets_keep_firing_without_power_or_compute() {
    let mut g = classic();
    let post = east_of_core(&g, 1, 5);
    accept(
        &mut g,
        1,
        Command::Deploy {
            room: 0,
            kind: "vscode".into(),
            pos: post,
        },
    );
    let turret = g.state.units.iter().find(|u| u.kind == "vscode").unwrap();
    let (id, battery) = (turret.id, turret.battery);
    assert_eq!(battery, turret.batteryMax, "classic caches start full");
    // A durable dummy so the exchange lasts the whole measurement window.
    let mut intruder = turret.clone();
    intruder.id = 90_001;
    intruder.owner = 2;
    intruder.pos = Pos::new(post.x + 2, post.y, 0);
    intruder.x = intruder.pos.x as f64 + 0.5;
    intruder.y = intruder.pos.y as f64 + 0.5;
    intruder.hp = 1e6;
    intruder.maxHp = 1e6;
    g.state.units.push(intruder);
    if let Some(u) = g.state.units.iter_mut().find(|u| u.id == id) {
        u.hp = 1e6;
        u.maxHp = 1e6;
    }
    g.refresh_fog();
    wait(&mut g, 12);
    let turret = g.state.units.iter().find(|u| u.id == id).unwrap();
    assert_eq!(turret.battery, battery, "classic never drains the cache");
    assert_eq!(turret.energy, turret.energyMax, "classic never drains energy");
    assert!(
        g.player(1).unwrap().totals.get("fire").copied().unwrap_or(0.) > 0.,
        "the turret must have actually fired"
    );
}

#[test]
fn saves_carry_the_ruleset_and_replay_deterministically() {
    let mut g = classic();
    let post = east_of_core(&g, 1, 4);
    accept(
        &mut g,
        1,
        Command::Deploy {
            room: 0,
            kind: "vscode".into(),
            pos: post,
        },
    );
    accept(
        &mut g,
        1,
        Command::Research {
            room: 0,
            branch: "speed".into(),
        },
    );
    wait(&mut g, 20);
    let save = g.save();
    assert_eq!(save.snapshot.ruleset, RULESET_CLASSIC);
    let loaded = Game::load(save.clone()).unwrap();
    assert!(loaded.classic());
    assert_eq!(loaded.state.tick, g.state.tick);
    assert!(Game::replay(&save).unwrap(), "classic replay must match");
}

#[test]
fn a_snapshot_with_an_unknown_ruleset_is_rejected() {
    let g = classic();
    let mut bad = g.state.clone();
    bad.ruleset = "arcade".into();
    assert!(bad.validate(true).is_err());
}

#[test]
fn the_classic_opponent_mines_and_fields_an_army() {
    let mut g = Game::new_ruleset(7, true, "river", RULESET_CLASSIC);
    wait(&mut g, 260);
    assert!(
        g.state
            .buildings
            .iter()
            .any(|b| b.owner == 2 && b.kind == "extractor"),
        "classic bot must build extractors"
    );
    assert!(
        g.state.units.iter().any(|u| u.owner == 2),
        "classic bot must deploy units"
    );
    assert!(
        g.state.buildings.iter().all(|b| b.kind != "shell"),
        "classic bot must not attempt shells"
    );
    assert!(g.state.rooms.is_empty(), "classic bot must not build rooms");
    assert!(g.state.links.is_empty(), "classic bot must not lay wires");
}

#[test]
fn the_full_ruleset_still_requires_a_lab_before_deploying() {
    let mut g = Game::new(1, false);
    let post = east_of_core(&g, 1, 5);
    let receipt = send(
        &mut g,
        1,
        Command::Deploy {
            room: 0,
            kind: "vscode".into(),
            pos: post,
        },
    );
    assert!(!receipt.accepted);
    assert_eq!(receipt.reason, "需要初始研究所及对应分支等级");
}
