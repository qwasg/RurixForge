//! Direct ore→credits income under abstract supply.
use sentinels_v6::{types::*, Game};

#[test]
fn powered_extractor_credits_owner_and_records_ore_mined() {
    let mut g = Game::new(21, false);
    let ore = g
        .state
        .resources
        .iter()
        .find(|r| r.kind == "ore" && r.remaining > 10.)
        .cloned()
        .expect("ore deposit");
    let mut mine = g.state.buildings[0].clone();
    mine.id = 700;
    mine.owner = 1;
    mine.kind = "extractor".into();
    mine.progress = 1.;
    mine.hp = 200.;
    mine.powered = true;
    mine.rect = Rect {
        x: ore.pos.x - 1,
        y: ore.pos.y - 1,
        level: 0,
        width: 2,
        height: 2,
    };
    g.state.buildings.push(mine);
    let before = g.player(1).unwrap().credits;
    let remaining = g
        .state
        .resources
        .iter()
        .find(|r| r.id == ore.id)
        .unwrap()
        .remaining;
    for _ in 0..60 {
        g.step();
    }
    let after = g.player(1).unwrap().credits;
    let mined = g
        .player(1)
        .unwrap()
        .totals
        .get("ore-mined")
        .copied()
        .unwrap_or(0.);
    let left = g
        .state
        .resources
        .iter()
        .find(|r| r.id == ore.id)
        .unwrap()
        .remaining;
    assert!(after > before, "credits should rise from mining");
    assert!(mined > 0., "ore-mined total should increase");
    assert!(left < remaining, "deposit should deplete");
}
