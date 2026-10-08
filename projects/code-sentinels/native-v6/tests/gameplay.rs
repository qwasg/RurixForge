use sentinels_v6::{Command, Game, Order, Pos, Rect};
#[path = "support/paid_opening.rs"]
mod paid_opening;
use paid_opening::{cmd, line, opening, wait};

#[test]
fn save_rule_identity_is_required_by_load_and_both_replay_entries() {
    let save = Game::new(79, false).save();
    assert_eq!(save.rulesVersion, sentinels_v6::RULES_VERSION);
    assert_eq!(save.rulesFingerprint, sentinels_v6::RULES_FINGERPRINT);
    assert_eq!(save.rulesFingerprint.len(), 64);
    assert!(save
        .rulesFingerprint
        .bytes()
        .all(|c| c.is_ascii_hexdigit()));
    assert!(Game::load(save.clone()).is_ok());
    assert!(Game::replay(&save).unwrap());
    assert!(sentinels_v6::ReplayController::new(save.clone()).is_ok());
    let mut legacy = serde_json::to_value(&save).unwrap();
    legacy.as_object_mut().unwrap().remove("rulesVersion");
    legacy.as_object_mut().unwrap().remove("rulesFingerprint");
    legacy["allowLegacy"] = serde_json::json!(true);
    let legacy: sentinels_v6::Save = serde_json::from_value(legacy).unwrap();
    assert!(legacy.rulesFingerprint.is_empty());
    let mut wrong_hash = save.clone();
    wrong_hash.rulesFingerprint.replace_range(
        0..1,
        if save.rulesFingerprint.starts_with('0') {
            "1"
        } else {
            "0"
        },
    );
    let mut wrong_version = save;
    wrong_version.rulesVersion.push_str("-other");
    for rejected in [legacy, wrong_hash, wrong_version] {
        assert!(rejected.validate().unwrap_err().contains("指纹"));
        assert!(Game::load(rejected.clone()).is_err());
        assert!(Game::replay(&rejected).is_err());
        assert!(sentinels_v6::ReplayController::new(rejected).is_err());
    }
}

#[test]
fn event_ring_preserves_exact_array_order_after_wrap_and_save_load() {
    let mut g = Game::new(78, false);
    let mut expected = Vec::new();
    for i in 0..5000 {
        g.event("ring-fixture", Pos::new(10, 48, 0), 1, i as f64, 0);
        expected.push(g.state.events.last().unwrap().clone());
    }
    let expected = expected.split_off(expected.len() - 4096);
    assert_eq!(g.state.events.len(), 4096);
    assert_eq!(
        serde_json::to_value(&g.state.events).unwrap(),
        serde_json::to_value(&expected).unwrap()
    );
    let save = g.save();
    let encoded = serde_json::to_string(&save).unwrap();
    let loaded = Game::load(serde_json::from_str(&encoded).unwrap()).unwrap();
    assert_eq!(
        serde_json::to_value(&loaded.state).unwrap(),
        serde_json::to_value(&g.state).unwrap()
    );
}

#[test]
fn extractor_orders_and_preview_clones_require_remaining_ore_or_coal() {
    for (kind, remaining, valid) in [
        ("ore", 100., true),
        ("coal", 100., true),
        ("ore", 0., false),
        ("coal", 0., false),
        ("salvage-ore", 100., false),
        ("salvage", 100., false),
    ] {
        let mut g = Game::new(77, false);
        g.state.terrain.fill(0);
        g.state.resources.clear();
        g.next_id = 1000;
        g.state.resources.push(sentinels_v6::Resource {
            id: 900,
            pos: Pos::new(17, 41, 0),
            kind: kind.into(),
            remaining,
            owner: 0,
            capture: 0.,
            contested: false,
            capturer: 0,
        });
        g.refresh_fog();
        let command = Command::Build {
            pos: Pos::new(16, 40, 0),
            kind: "extractor".into(),
        };
        let cash = g.player(1).unwrap().credits;
        let buildings = g.state.buildings.len();
        let mut preview = g.clone();
        assert_eq!(
            preview.execute(1, command.clone()).is_ok(),
            valid,
            "preview {kind}/{remaining}"
        );
        assert_eq!(g.player(1).unwrap().credits, cash);
        let receipt = g.order(Order {
            owner: 1,
            sequence: 1,
            command,
        });
        assert_eq!(
            receipt.accepted, valid,
            "order {kind}/{remaining}: {}",
            receipt.reason
        );
        if valid {
            assert_eq!(g.state.buildings.len(), buildings + 1);
            assert!(g.player(1).unwrap().credits < cash);
        } else {
            assert_eq!(g.state.buildings.len(), buildings);
            assert_eq!(g.player(1).unwrap().credits, cash);
        }
    }
}

#[test]
fn public_opening_has_real_power_compute_and_cash_reserve() {
    let g = opening();
    assert!(
        g.player(1).unwrap().credits >= 500.,
        "credits {}",
        g.player(1).unwrap().credits
    );
    assert!(g.state.rooms.iter().all(|r| r.powered && r.connected));
    assert!(g.player(1).unwrap().production >= 12.);
    assert_eq!(g.state.units.len(), 2);
    assert!(g.state.units.iter().all(|u| u.covered));
    assert_eq!(
        g.state
            .resources
            .iter()
            .filter(|r| r.kind == "node")
            .count(),
        3
    );
}

#[test]
fn flat_world_rejects_non_ground_shells_and_tiny_footprints() {
    let mut g = Game::new(1, false);
    let before = g.player(1).unwrap().credits;
    for command in [
        Command::Deploy {
            room: 0,
            kind: "vscode".into(),
            pos: Pos::new(14, 45, 0),
        },
        Command::Shell {
            rect: Rect {
                x: 14,
                y: 44,
                level: 1,
                width: 4,
                height: 4,
            },
        },
        Command::Shell {
            rect: Rect {
                x: 14,
                y: 44,
                level: -1,
                width: 4,
                height: 4,
            },
        },
        Command::Shell {
            rect: Rect {
                x: 14,
                y: 44,
                level: 0,
                width: 2,
                height: 4,
            },
        },
    ] {
        let s = g.sequences[0] + 1;
        assert!(!g
            .order(Order {
                owner: 1,
                sequence: s,
                command
            })
            .accepted);
    }
    assert_eq!(g.player(1).unwrap().credits, before);
}

#[test]
fn duplicate_command_is_idempotent_and_wrong_owner_cannot_recycle() {
    let mut g = opening();
    let command = Command::Build {
        pos: Pos::new(15, 54, 0),
        kind: "wind-power".into(),
    };
    let order = Order {
        owner: 1,
        sequence: g.sequences[0] + 1,
        command,
    };
    let first = g.order(order.clone());
    assert!(first.accepted);
    let credits = g.player(1).unwrap().credits;
    assert!(g.order(order).accepted);
    assert_eq!(credits, g.player(1).unwrap().credits);
    let target = g
        .state
        .buildings
        .iter()
        .find(|b| b.owner == 1 && b.kind == "wind-power")
        .unwrap()
        .id;
    let denied = g.order(Order {
        owner: 2,
        sequence: g.sequences[1] + 1,
        command: Command::Recycle { id: target },
    });
    assert!(!denied.accepted);
}

#[test]
#[ignore = "flat opening only seeds tier-1 turrets; AI tether needs paid tier-2 research loop"]
fn same_floor_wire_can_tether_a_stationary_ai() {
    let mut g = opening();
    // Reuse a paid-opening turret (empty branch, tier 1) so replay stays honest.
    let actor = g
        .state
        .units
        .iter()
        .find(|u| u.owner == 1 && u.kind == "vscode")
        .unwrap()
        .id;
    let p = g.state.units.iter().find(|u| u.id == actor).unwrap().pos;
    // Detach any opening tether first, then re-wire locally.
    cmd(
        &mut g,
        1,
        Command::Wire {
            kind: "compute".into(),
            path: line(p, Pos::new(p.x, p.y + 1, 0)),
            unit_endpoints: vec![actor],
        },
    );
    assert!(g.state.units.iter().find(|u| u.id == actor).unwrap().wired);
    Game::load(g.save()).unwrap();
    assert!(Game::replay(&g.save()).unwrap());
}
