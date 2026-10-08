//! Public-command bot progression. No edited balances, technology, generated
//! airports or alternative victory rules are used by these tests.
#[path = "support/paid_opening.rs"]
mod paid_opening;
use paid_opening::{cmd, wait};
use sentinels_v6::{catalog, Command, Game, Order, Pos};
#[test]
fn a_paid_mobile_relay_is_a_load_and_the_bot_connects_its_actual_power_line() {
    let mut g = paid_opening::opening();
    cmd(
        &mut g,
        1,
        Command::Build {
            pos: Pos::new(13, 38, 0),
            kind: "mobile-relay".into(),
        },
    );
    wait(&mut g, 10);
    let relay = g
        .state
        .buildings
        .iter()
        .find(|b| b.owner == 1 && b.kind == "mobile-relay")
        .unwrap();
    assert_eq!(relay.power, 0.);
    assert_eq!(relay.demand, 10.);
    assert!(!relay.powered);
    let id = relay.id;
    for _ in 0..30 {
        g.bot_for_style(1, "algorithm", "maintech");
        wait(&mut g, 3);
        if g.state
            .buildings
            .iter()
            .find(|b| b.id == id)
            .unwrap()
            .powered
        {
            break;
        }
    }
    let relay = g.state.buildings.iter().find(|b| b.id == id).unwrap();
    assert!(relay.powered);
    assert!(g.state.links.iter().any(|l| l.owner == 1
        && l.kind == "power"
        && l.active
        && l.path.iter().any(|p| relay.rect.contains(*p))));
    assert!(g.orders.iter().any(|o|o.receipt.accepted&&matches!(&o.order.command,Command::Wire{kind,path,..} if kind=="power"&&path.iter().any(|p|relay.rect.contains(*p)))));
}
#[test]
fn starting_technology_cannot_fabricate_an_airport_or_launch_platform() {
    for kind in ["airstrip", "launch-pad"] {
        let mut g = Game::new(1, false);
        let money = g.player(1).unwrap().credits;
        let buildings = g.state.buildings.len();
        let r = g.order(Order {
            owner: 1,
            sequence: 1,
            command: Command::Build {
                pos: Pos::new(13, 42, 0),
                kind: kind.into(),
            },
        });
        assert!(!r.accepted);
        assert!(r.reason.contains("科技"), "{}", r.reason);
        assert_eq!(g.player(1).unwrap().credits, money);
        assert_eq!(g.state.buildings.len(), buildings);
    }
}
#[test]
fn all_five_branches_earn_air_and_orbital_production_in_planned_garrison() {
    // This is a planned procurement/permission contract, not autonomous PvP.
    // The bot still buys its entire economy and army through normal orders.
    // Ordinary tactical overrides keep exactly one capture actor at the middle
    // node, one survey vehicle at friendly mineral sites, and the rest in a
    // real rear garrison. Neither victory nor resources are modified.
    fn direct(g: &mut Game, command: Command) {
        let receipt = g.order(Order { owner: 1, sequence: g.sequences[0] + 1, command: command.clone() });
        assert!(receipt.accepted, "planned garrison order {command:?}: {}", receipt.reason);
    }
    fn assign_garrison(
        g: &mut Game,
        assignments: &mut std::collections::BTreeMap<u64, Pos>,
        capturer: &mut Option<u64>,
        surveyor: &mut Option<u64>,
    ) {
        let units: Vec<_> = g.state.units.iter().filter(|u| u.owner == 1 && u.hp > 0.).cloned().collect();
        assignments.retain(|id, _| units.iter().any(|u| u.id == *id));
        if capturer.is_some_and(|id| !units.iter().any(|u| u.id == id)) { *capturer = None; }
        if surveyor.is_some_and(|id| !units.iter().any(|u| u.id == id)) { *surveyor = None; }
        for unit in units {
            let def = catalog::unit_ref(&unit.kind).unwrap();
            if def.speed <= 0. || unit.wired { continue; }
            if def.category == "vehicle" && capturer.is_none() { *capturer = Some(unit.id); }
            else if def.category == "vehicle" && *capturer != Some(unit.id) && surveyor.is_none() { *surveyor = Some(unit.id); }
            let desired = if *capturer == Some(unit.id) {
                Some(Pos::new(64, 48, 0))
            } else if *surveyor == Some(unit.id) {
                let central_mine = g.state.buildings.iter().any(|b| b.owner == 1 && b.kind == "extractor" && b.hp > 0. && b.progress >= 1. && b.rect.center().distance(Pos::new(44, 34, 0)) < 8.);
                let airfield_without_runway = g.state.rooms.iter().find(|r| r.owner == 1 && r.kind == "airfield" && r.progress >= 1. && !g.state.buildings.iter().any(|b| b.owner == 1 && b.kind == "airstrip" && b.rect.center().distance(r.rect.center()) < 24.));
                Some(if let Some(field) = airfield_without_runway {
                    // A unit parked inside the building cannot see through its
                    // real exterior walls. Survey actual surrounding ground.
                    let c=field.rect.center();let offsets=[(0,-9),(-9,0),(9,0),(0,9)];
                    let (dx,dy)=offsets[((g.state.tick/1800)%4)as usize];Pos::new(c.x+dx,c.y+dy,0)
                }
                    else if central_mine { Pos::new(28, 62, 0) } else { Pos::new(40, 38, 0) })
            } else {
                assignments.get(&unit.id).copied()
            };
            let desired = desired.filter(|p| g.route(unit.pos, *p, &def.category).is_some()).or_else(|| {
                let mut candidates = Vec::new();
                let target = desired.unwrap_or(Pos::new(6, 61, 0));
                for y in (target.y - 8).max(1)..=(target.y + 8).min(94) {
                    for x in (target.x - 8).max(1)..=(target.x + 8).min(126) {
                        let p = Pos::new(x, y, 0);
                        if *capturer == Some(unit.id) && p.distance(Pos::new(64,48,0)) >= 7. { continue; }
                        if *capturer != Some(unit.id) && g.state.resources.iter().any(|r| r.kind == "node" && p.distance(r.pos) < 12.) { continue; }
                        if assignments.iter().any(|(id,other)| *id != unit.id && p.distance(*other) < 3.) { continue; }
                        if g.walkable(p, &def.category) { candidates.push(p); }
                    }
                }
                candidates.sort_by(|a,b| a.distance(target).total_cmp(&b.distance(target)).then(a.cmp(b)));
                candidates.into_iter().find(|p| g.route(unit.pos,*p,&def.category).is_some())
            });
            if let Some(goal) = desired {
                assignments.insert(unit.id, goal);
                if unit.pos == goal {
                    if unit.goal.is_some() || !unit.route.is_empty() || unit.target.is_some() {
                        direct(g, Command::Stop { ids: vec![unit.id] });
                    }
                } else if unit.goal != Some(goal) || unit.route.is_empty() || unit.target.is_some() {
                    direct(g, Command::Move { ids: vec![unit.id], pos: goal });
                }
            }
        }
    }
    for (i, branch) in catalog::BRANCHES.iter().enumerate() {
        let mut g = Game::new(910 + i as u64, false);
        assert!(g.state.players.iter().all(|p| p.credits == catalog::STARTING_CREDITS));
        let mut garrison = std::collections::BTreeMap::new();
        let (mut capturer, mut surveyor) = (None, None);
        let mut paid_classes = std::collections::BTreeMap::<String, u64>::new();
        let (mut saw_air, mut saw_orbit, mut saw_ground, mut saw_aa) = (false, false, false, false);
        for _ in 0..45 * 60 * 60 {
            if g.state.tick % 180 == 0 {
                let first = g.orders.len();
                let tier = g.tech(1, branch);
                let max_tier = g.tech(1, "");
                g.bot_for_style(1, branch, "maintech");
                for logged in &g.orders[first..] {
                    if !logged.receipt.accepted {
                        continue;
                    }
                    assert_eq!(logged.order.owner, 1);
                    match &logged.order.command {
                        Command::Deploy { room, kind, .. } => {
                            let def = catalog::unit_ref(kind).unwrap();
                            let unit = g.state.units.iter().filter(|u| u.owner == 1 && u.kind == *kind).max_by_key(|u| u.id).unwrap();
                            assert!(unit.invested >= def.cost && unit.hp > 0.);
                            *paid_classes.entry(if def.chassis == "aa-launcher" { "anti-air".into() } else { def.category.clone() }).or_default() += 1;
                            if !def.branch.is_empty() {
                                assert_eq!(def.branch, *branch);
                                assert!(def.tier <= tier);
                            }
                            let source = g.state.rooms.iter().find(|r| r.id == *room).unwrap();
                            if def.category == "air" {
                                saw_air = true;
                                assert_eq!(source.kind, "airfield");
                                assert!(source.powered && source.online);
                                assert!(g.state.buildings.iter().any(|b| b.owner == 1
                                    && b.kind == "airstrip"
                                    && b.powered
                                    && b.progress >= 1.));
                            }
                            if def.category == "orbital" {
                                saw_orbit = true;
                                assert_eq!(source.kind, "orbital-control");
                                assert!(source.online && source.powered && source.connected);
                                assert!(g.state.buildings.iter().any(|b| b.owner == 1
                                    && b.kind == "launch-pad"
                                    && b.powered
                                    && b.progress >= 1.));
                            }
                            saw_ground |= def.category == "vehicle";
                            saw_aa |= def.chassis == "aa-launcher";
                        }
                        Command::Build { kind, .. } => {
                            assert!(catalog::outdoor_building(kind).unwrap().tier <= max_tier)
                        }
                        Command::Room { kind, .. } => {
                            assert!(catalog::facility_ref(kind).unwrap().tier <= max_tier)
                        }
                        _ => {}
                    }
                }
                assign_garrison(&mut g, &mut garrison, &mut capturer, &mut surveyor);
                assert!(g.player(1).unwrap().credits >= 0.);
                if saw_air && saw_orbit && saw_ground && saw_aa {
                    break;
                }
            }
            g.step();
            assert!(g.state.resources.iter().filter(|r| r.kind == "node" && r.owner == 1).count() <= 1,
                "planned garrison must not quietly acquire a second strategic node");
            if g.state.winner.is_some() {
                break;
            }
        }
        if let Ok(directory) = std::env::var("V6_BOT_DIAGNOSTICS") {
            std::fs::create_dir_all(&directory).unwrap();
            let path = std::path::Path::new(&directory).join(format!("{branch}.save.json"));
            let file = std::fs::OpenOptions::new().write(true).create_new(true).open(path).unwrap();
            serde_json::to_writer(file,&g.save()).unwrap();
            let report = serde_json::json!({"scope":"planned_garrison, ordinary paid procurement and real single-node mineral/science income; not autonomous PvP", "branch":branch,"rulesVersion":sentinels_v6::RULES_VERSION,"rulesFingerprint":sentinels_v6::RULES_FINGERPRINT,"seconds":g.state.tick as f64/60.,"winner":g.state.winner,"winReason":g.state.winReason,"paidClasses":paid_classes,"coverage":{"ground":saw_ground,"antiAir":saw_aa,"air":saw_air,"orbital":saw_orbit},"players":g.state.players});
            let report_path = std::path::Path::new(&directory).join(format!("{branch}.report.json"));
            let file = std::fs::OpenOptions::new().write(true).create_new(true).open(report_path).unwrap();
            serde_json::to_writer_pretty(file,&report).unwrap();
        }
        assert!(
            saw_ground && saw_aa,
            "{branch}: no mixed ground/AA defense by tick {}",
            g.state.tick
        );
        assert!(
            saw_air,
            "{branch}: no earned aircraft by tick {}, tier {} credits {}",
            g.state.tick,
            g.tech(1, branch),
            g.player(1).unwrap().credits
        );
        assert!(
            saw_orbit,
            "{branch}: no earned orbital platform by tick {}, tier {} credits {}",
            g.state.tick,
            g.tech(1, branch),
            g.player(1).unwrap().credits
        );
        assert!(
            g.player(1)
                .unwrap()
                .totals
                .get("ore-delivered")
                .copied()
                .unwrap_or(0.)
                > 0.
        );
    }
}
