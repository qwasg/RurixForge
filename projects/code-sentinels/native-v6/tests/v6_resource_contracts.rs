//! Flat-world resource / opening contracts (no shipments or multi-floor).
use sentinels_v6::{Command, Game, Order, Rect};
#[path = "support/paid_opening.rs"]
mod support;
use support::{opening, wait};

#[test]
fn paid_opening_leaves_credit_reserve_and_powered_facilities() {
    let g = opening();
    assert!(g.player(1).unwrap().credits >= 500.);
    assert!(g
        .state
        .rooms
        .iter()
        .filter(|r| r.owner == 1)
        .all(|r| r.powered && r.connected));
    assert!(g
        .state
        .buildings
        .iter()
        .any(|b| b.owner == 1 && b.kind == "extractor" && b.progress >= 1.));
}

#[test]
fn extractor_credits_owner_and_records_ore_mined_totals() {
    let mut g = opening();
    let before = g.player(1).unwrap().credits;
    let mined = g
        .player(1)
        .unwrap()
        .totals
        .get("ore-mined")
        .copied()
        .unwrap_or(0.);
    wait(&mut g, 5);
    let after = g.player(1).unwrap().credits;
    let mined_after = g
        .player(1)
        .unwrap()
        .totals
        .get("ore-mined")
        .copied()
        .unwrap_or(0.);
    assert!(
        after > before || mined_after > mined,
        "extractor should credit or record ore-mined"
    );
    assert!(mined_after >= mined);
}

#[test]
fn shell_on_non_zero_level_is_rejected() {
    let mut g = Game::new(1, false);
    let cash = g.player(1).unwrap().credits;
    let r = g.order(Order {
        owner: 1,
        sequence: 1,
        command: Command::Shell {
            rect: Rect {
                x: 20,
                y: 40,
                level: 1,
                width: 6,
                height: 4,
            },
        },
    });
    assert!(!r.accepted);
    assert_eq!(g.player(1).unwrap().credits, cash);
}
