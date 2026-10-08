//! Earned two-player siege, entirely ordinary orders from the standard2000 start.
//! Neither state mutation nor private damage/settlement functions are used.
use sentinels_v6::{catalog, Command, Game, Order, Pos, Rect};
use serde_json::{json, Value};
use std::{collections::BTreeMap, path::PathBuf};
#[path = "support/paid_opening.rs"]
mod support;

struct Siege {
    g: Game,
    core: u64,
    last_event: u64,
    ammo: BTreeMap<u64, f64>,
    refilled: f64,
    core_hits: Vec<Value>,
    arrivals: Vec<Value>,
    paid_units: Vec<Value>,
    commands: Vec<Value>,
    success: bool,
    label: &'static str,
}
impl Siege {
    fn new(label: &'static str) -> Self {
        let g = support::opening();
        let core = g
            .state
            .buildings
            .iter()
            .find(|b| b.owner == 2 && b.kind == "core")
            .unwrap()
            .id;
        Self {
            g,
            core,
            last_event: 0,
            ammo: BTreeMap::new(),
            refilled: 0.,
            core_hits: vec![],
            arrivals: vec![],
            paid_units: vec![],
            commands: vec![],
            success: false,
            label,
        }
    }
    fn tick(&mut self) {
        assert!(
            self.g.state.tick < 55 * 60 * 60,
            "siege exceeded55 simulated minutes"
        );
        self.g.step();
        for u in self.g.state.units.iter().filter(|u| u.owner == 1) {
            if let Some(old) = self.ammo.insert(u.id, u.ammo) {
                self.refilled += (u.ammo - old).max(0.);
            }
        }
        for e in self
            .g
            .state
            .events
            .iter()
            .rev()
            .take_while(|e| e.id > self.last_event)
        {
            if e.subject == self.core
                && matches!(
                    e.kind.as_str(),
                    "impact" | "explosive-impact" | "energy-impact"
                )
            {
                self.core_hits.push(json!({"tick":e.tick,"kind":e.kind,"damage":e.magnitude,"attacker":e.owner,"coreHp":self.g.state.buildings.iter().find(|b|b.id==self.core).map(|b|b.hp).unwrap_or(0.)}));
            }
            if e.kind == "shipment-arrive"
                && e.owner == 1
                && self
                    .paid_units
                    .iter()
                    .any(|u| u["id"].as_u64() == Some(e.subject))
            {
                self.arrivals
                    .push(json!({"tick":e.tick,"unit":e.subject,"amount":e.magnitude,"pos":e.pos}));
            }
        }
        self.last_event = self
            .g
            .state
            .events
            .last()
            .map(|e| e.id)
            .unwrap_or(self.last_event);
    }
    fn wait(&mut self, seconds: u64) {
        for _ in 0..seconds * 60 {
            if self.g.state.winner.is_some() {
                return;
            }
            self.tick();
        }
    }
    fn order(&mut self, owner: u32, command: Command) {
        let before = self.g.player(owner).unwrap().credits;
        let sequence = self.g.sequences[(owner - 1) as usize] + 1;
        let receipt = self.g.order(Order {
            owner,
            sequence,
            command: command.clone(),
        });
        self.commands.push(json!({"owner":owner,"command":command,"receipt":receipt,"creditsPaid":before-self.g.player(owner).unwrap().credits}));
        assert!(
            receipt.accepted,
            "ordinary order {command:?}: {} at{}sec",
            receipt.reason,
            self.g.state.tick / 60
        );
        if let Command::Deploy { kind, .. } = command {
            let u = self
                .g
                .state
                .units
                .iter()
                .filter(|u| u.owner == owner && u.kind == kind)
                .max_by_key(|u| u.id)
                .unwrap();
            self.paid_units.push(json!({"id":u.id,"owner":owner,"kind":kind,"tick":self.g.state.tick,"cost":before-self.g.player(owner).unwrap().credits,"sourceFacility":u.sourceFacility,"spawn":u.pos,"initialAmmo":u.ammo}));
        }
    }
    fn paid(&mut self, owner: u32, command: Command) {
        loop {
            let mut preview = self.g.clone();
            match preview.execute(owner, command.clone()) {
                Ok(()) => break,
                Err(reason) if reason.contains("金币不足") || reason.contains("算力不足") =>
                {
                    assert!(
                        self.g.state.winner.is_none(),
                        "game ended while funding {command:?}"
                    );
                    self.wait(1);
                }
                Err(reason) => panic!(
                    "public preview {command:?}: {reason} at{}sec",
                    self.g.state.tick / 60
                ),
            }
        }
        self.order(owner, command);
    }
    fn room(&self, owner: u32, kind: &str) -> u64 {
        self.g
            .state
            .rooms
            .iter()
            .find(|r| r.owner == owner && r.kind == kind)
            .unwrap()
            .id
    }
    fn report(&self) -> Value {
        json!({"rulesVersion":sentinels_v6::RULES_VERSION,"rulesFingerprint":sentinels_v6::RULES_FINGERPRINT,"scope":"Standard2000-credit start, ordinary paid mining/construction/research/production/orders; no injected state, direct damage or winner override; defender buys a normal mirrored opening and physical wall","acceptanceGoal":self.label,"coreVictoryAchieved":self.g.state.winReason=="指挥核心被摧毁","passed":self.success,"seconds":self.g.state.tick as f64/60.,"winner":self.g.state.winner,"winReason":self.g.state.winReason,"coreId":self.core,"coreHits":self.core_hits,"unitAmmoActuallyReplenished":self.refilled,"unitShipmentArrivals":self.arrivals,"paidUnits":self.paid_units,"commands":self.commands,"players":self.g.state.players,"remainingUnits":self.g.state.units})
    }
}
impl Drop for Siege {
    fn drop(&mut self) {
        let dir = std::env::var_os("V6_CORE_SIEGE_OUT")
            .map(PathBuf::from)
            .unwrap_or_else(|| {
                PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                    .join("../game/v6/core-siege-development-20260911")
            })
            .join(self.label);
        let _ = std::fs::create_dir_all(&dir);
        let _ = std::fs::write(
            dir.join("report.json"),
            serde_json::to_vec_pretty(&self.report()).unwrap(),
        );
        let _ = std::fs::write(
            dir.join("save.json"),
            serde_json::to_vec(&self.g.save()).unwrap(),
        );
    }
}
fn mirror(p: Pos) -> Pos {
    Pos::new(127 - p.x, 95 - p.y, p.level)
}
fn rect_mirror(r: Rect) -> Rect {
    Rect {
        x: 128 - r.x - r.width,
        y: 96 - r.y - r.height,
        ..r
    }
}

#[test]
fn earned_tank_army_breaks_a_defended_core_with_real_ammunition_and_exact_replay() {
    run_siege(false);
}
#[test]
fn paid_longer_range_garrison_really_returns_fire_and_replays_the_outcome() {
    run_siege(true);
}
fn run_siege(add_mortar: bool) {
    let mut s = Siege::new(if add_mortar {
        "active-defense-and-replay"
    } else {
        "core-victory-and-replay"
    });
    assert!(s
        .g
        .state
        .buildings
        .iter()
        .filter(|b| b.kind == "core")
        .all(|b| b.hp == 6000.));
    // Reissue the known legal opening at the opposite spawn. IDs come from the
    // real red entities, never copied from blue or from a constructed snapshot.
    let script = s.g.orders.clone();
    let start = s.g.state.tick;
    for logged in script
        .into_iter()
        .filter(|l| l.order.owner == 1 && l.receipt.accepted)
    {
        while s.g.state.tick < start + logged.tick {
            s.tick();
        }
        let remap_room = |g: &Game, id: u64| {
            let r = rect_mirror(g.state.rooms.iter().find(|r| r.id == id).unwrap().rect);
            g.state
                .rooms
                .iter()
                .find(|room| room.owner == 2 && room.rect == r)
                .unwrap()
                .id
        };
        let command = match logged.order.command {
            Command::Shell { rect } => Command::Shell {
                rect: rect_mirror(rect),
            },
            Command::Build { pos, kind } => {
                let d = catalog::outdoor_ref(&kind).unwrap();
                Command::Build {
                    pos: Pos::new(
                        128 - pos.x - d.width as i32,
                        96 - pos.y - d.height as i32,
                        pos.level,
                    ),
                    kind,
                }
            }
            Command::Room {
                shell,
                rect,
                kind,
                branch,
            } => {
                let base = rect_mirror(
                    s.g.state
                        .buildings
                        .iter()
                        .find(|b| b.id == shell)
                        .unwrap()
                        .rect,
                );
                let red =
                    s.g.state
                        .buildings
                        .iter()
                        .find(|b| b.owner == 2 && b.kind == "shell" && b.rect == base)
                        .unwrap()
                        .id;
                Command::Room {
                    shell: red,
                    rect: rect_mirror(rect),
                    kind,
                    branch,
                }
            }
            Command::Wire { kind, path, .. } => Command::Wire {
                kind,
                path: path.into_iter().map(mirror).collect(),
                unit_endpoints: vec![],
            },
            Command::InstallGpu { room, model } => Command::InstallGpu {
                room: remap_room(&s.g, room),
                model,
            },
            Command::Deploy { room, kind, pos } => Command::Deploy {
                room: remap_room(&s.g, room),
                kind,
                pos: mirror(pos),
            },
            other => panic!("unexpected opening command {other:?}"),
        };
        s.order(2, command);
    }
    s.wait(3);
    assert!(s.g.state.units.iter().filter(|u| u.owner == 2).count() >= 2);
    assert!(
        s.g.player(2).unwrap().production > 0.,
        "the defender must have real GPU output"
    );
    assert!(
        s.g.state
            .units
            .iter()
            .filter(|u| u.owner == 2)
            .all(|u| u.covered),
        "the initial defender guns must be genuinely connected"
    );
    s.paid(
        2,
        Command::Wall {
            kind: "physical".into(),
            path: (115..=120).map(|x| Pos::new(x, 44, 0)).collect(),
        },
    );
    if add_mortar {
        let guard = catalog::units()
            .into_iter()
            .find(|u| u.branch == "algorithm" && u.chassis == "light-mortar")
            .unwrap();
        s.paid(
            2,
            Command::Deploy {
                room: s.room(2, "research-lab"),
                kind: guard.id,
                pos: Pos::new(113, 42, 0),
            },
        );
    }
    let lab = s.room(1, "research-lab");
    s.paid(
        1,
        Command::Research {
            room: lab,
            branch: "algorithm".into(),
        },
    );
    s.wait(70);
    assert_eq!(s.g.tech(1, "algorithm"), 2);
    let rect = Rect {
        x: 2,
        y: 38,
        level: 0,
        width: 6,
        height: 4,
    };
    s.paid(1, Command::Shell { rect });
    s.wait(20);
    let shell =
        s.g.state
            .buildings
            .iter()
            .find(|b| b.owner == 1 && b.kind == "shell" && b.rect == rect)
            .unwrap()
            .id;
    s.paid(
        1,
        Command::Room {
            shell,
            rect: Rect {
                x: 3,
                y: 38,
                width: 4,
                height: 2,
                level: 0,
            },
            kind: "factory".into(),
            branch: None,
        },
    );
    s.paid(
        1,
        Command::Build {
            pos: Pos::new(8, 38, 0),
            kind: "wind-power".into(),
        },
    );
    let original =
        s.g.state
            .buildings
            .iter()
            .find(|b| b.owner == 1 && b.kind == "shell" && b.id != shell)
            .unwrap()
            .id;
    s.paid(
        1,
        Command::Room {
            shell: original,
            rect: Rect {
                x: 15,
                y: 48,
                width: 2,
                height: 2,
                level: 0,
            },
            kind: "ammunition-workshop".into(),
            branch: None,
        },
    );
    s.paid(
        1,
        Command::Room {
            shell: original,
            rect: Rect {
                x: 17,
                y: 48,
                width: 2,
                height: 2,
                level: 0,
            },
            kind: "depot".into(),
            branch: None,
        },
    );
    s.wait(30);
    let factory = s.room(1, "factory");
    for (a, b) in [
        (Pos::new(8, 38, 0), Pos::new(3, 38, 0)),
        (Pos::new(8, 38, 0), Pos::new(13, 42, 0)),
        (Pos::new(17, 46, 0), Pos::new(17, 49, 0)),
        (Pos::new(17, 49, 0), Pos::new(15, 49, 0)),
    ] {
        s.paid(
            1,
            Command::Wire {
                kind: "power".into(),
                path: support::line(a, b),
                unit_endpoints: vec![],
            },
        );
    }
    s.wait(5);
    let tank = catalog::units()
        .into_iter()
        .find(|u| u.branch == "algorithm" && u.chassis == "tank")
        .unwrap();
    let mut army = Vec::new();
    for i in 0..6 {
        s.paid(
            1,
            Command::Deploy {
                room: factory,
                kind: tank.id.clone(),
                pos: Pos::new(6, 45, 0),
            },
        );
        let id =
            s.g.state
                .units
                .iter()
                .filter(|u| u.owner == 1 && u.kind == tank.id)
                .max_by_key(|u| u.id)
                .unwrap()
                .id;
        army.push(id);
        s.order(
            1,
            Command::Move {
                ids: vec![id],
                pos: Pos::new(100, 36 + i * 3, 0),
            },
        );
        s.wait(5);
    }
    // Scouts/tanks acquire only actually visible targets. Attacks must clear
    // enemy guns/walls before the core; no hidden-ID attack command is issued.
    for _ in 0..40 * 60 {
        if s.g.state.winner.is_some() {
            break;
        }
        if !s
            .g
            .state
            .units
            .iter()
            .any(|u| army.contains(&u.id) && u.hp > 0.)
        {
            break;
        }
        let view = s.g.snapshot(Some(1));
        let target = view
            .units
            .iter()
            .find(|u| u.owner == 2)
            .map(|u| u.id)
            .or_else(|| view.walls.iter().find(|w| w.owner == 2).map(|w| w.id))
            .or_else(|| view.buildings.iter().find(|b| b.id == s.core).map(|b| b.id));
        if let Some(target) = target {
            let ids =
                s.g.state
                    .units
                    .iter()
                    .filter(|u| army.contains(&u.id) && u.hp > 0. && u.target != Some(target))
                    .map(|u| u.id)
                    .collect::<Vec<_>>();
            if !ids.is_empty() {
                s.order(1, Command::Attack { ids, target });
            }
        } else if s.g.state.tick % 300 == 0 {
            for (i, id) in army.clone().into_iter().enumerate() {
                if s.g
                    .state
                    .units
                    .iter()
                    .any(|u| u.id == id && u.route.is_empty())
                {
                    s.order(
                        1,
                        Command::Move {
                            ids: vec![id],
                            pos: Pos::new(112, 36 + i as i32 * 2, 0),
                        },
                    );
                }
            }
        }
        s.wait(1);
    }
    if !add_mortar {
        assert_eq!(s.g.state.winner, Some(1), "{}", s.report());
        assert_eq!(s.g.state.winReason, "指挥核心被摧毁");
        assert!(!s.core_hits.is_empty());
    }
    assert!(
        s.refilled > 0.,
        "siege must actually receive consumed ammunition"
    );
    assert!(!s.arrivals.is_empty());
    assert!(
        s.g.player(1)
            .unwrap()
            .totals
            .get("ore-delivered")
            .copied()
            .unwrap_or(0.)
            > 500.
    );
    assert!(
        s.g.player(1)
            .unwrap()
            .totals
            .get("ammo-spent")
            .copied()
            .unwrap_or(0.)
            > 0.
    );
    if add_mortar {
        assert!(
            s.g.player(2)
                .unwrap()
                .totals
                .get("fire")
                .copied()
                .unwrap_or(0.)
                > 0.,
            "the paid basic garrison must actually return fire"
        );
        assert!(
            s.g.player(2)
                .unwrap()
                .totals
                .get("damage-dealt")
                .copied()
                .unwrap_or(0.)
                > 0.,
            "defender projectiles must actually hit"
        );
    }
    let save = s.g.save();
    assert_eq!(
        serde_json::to_value(&Game::load(save.clone()).unwrap().state).unwrap(),
        serde_json::to_value(&s.g.state).unwrap()
    );
    assert!(Game::replay(&save).unwrap());
    s.success = true;
}
