//! Real 2000-credit ordinary-command openings, no edited terrain or technology.
use sentinels_v6::{catalog, Game};
#[test]
fn both_players_produce_and_exit_their_mirrored_factories() {
    let mut factories = Vec::new();
    for owner in [1, 2] {
        let mut g = Game::new(1000, false);
        let mut exited = false;
        for _ in 0..420 * 60 {
            if g.state.tick % 180 == 0 {
                g.bot_for_style(owner, "speed", "mixed-ai");
            }
            g.step();
            if let Some(unit) = g.state.units.iter().find(|u| {
                u.owner == owner
                    && catalog::unit_ref(&u.kind).is_some_and(|d| d.category == "vehicle")
            }) {
                let room = g
                    .state
                    .rooms
                    .iter()
                    .find(|r| r.id == unit.sourceFacility)
                    .unwrap();
                let shell = g
                    .state
                    .buildings
                    .iter()
                    .find(|b| b.id == room.shell)
                    .unwrap();
                if [(0, 0), (1, 0), (0, 1), (1, 1)]
                    .into_iter()
                    .all(|(dx, dy)| {
                        !shell.rect.contains(sentinels_v6::Pos::new(
                            unit.pos.x + dx,
                            unit.pos.y + dy,
                            unit.pos.level,
                        ))
                    })
                {
                    factories.push(room.rect);
                    exited = true;
                    eprintln!("owner={owner} first factory {:?}, real vehicle {} outside at {:?} after {:.1}s",room.rect,unit.id,unit.pos,g.state.tick as f64/60.);
                    break;
                }
            }
        }
        assert!(
            exited,
            "owner {owner}: no paid vehicle left its factory; credits={} rooms={:?}",
            g.player(owner).unwrap().credits,
            g.state
                .rooms
                .iter()
                .filter(|r| r.owner == owner)
                .map(|r| (&r.kind, r.rect, r.powered, r.connected))
                .collect::<Vec<_>>()
        );
        assert!(g.orders.iter().all(|o| o.order.owner == owner));
        assert!(g.player(owner).unwrap().credits >= 0.);
    }
    let left = factories[0].center();
    let right = factories[1].center();
    assert!(
        (left.x + right.x - 128).abs() <= 2,
        "asymmetric factory X: {left:?}/{right:?}"
    );
    assert!(
        (left.y + right.y - 96).abs() <= 2,
        "asymmetric factory Y: {left:?}/{right:?}"
    );
}

#[test]
fn paid_t1_scout_reveals_a_forward_mine_before_the_second_ai_is_bought() {
    let mut g = Game::new(1000, false);
    let home = g
        .state
        .buildings
        .iter()
        .find(|b| b.owner == 1 && b.kind == "core")
        .unwrap()
        .rect
        .center();
    let mut first_vehicle_tier = None;
    let mut first_vehicle_time = None;
    let mut revealed = None;
    for _ in 0..360 * 60 {
        if g.state.tick % 180 == 0 {
            let before = g.orders.len();
            g.bot_for_style(1, "speed", "mixed-ai");
            for order in &g.orders[before..] {
                if order.receipt.accepted {
                    if let sentinels_v6::Command::Deploy { kind, .. } = &order.order.command {
                        let def = catalog::unit_ref(kind).unwrap();
                        if def.category == "vehicle" && first_vehicle_tier.is_none() {
                            assert_eq!(def.chassis, "scout");
                            first_vehicle_tier = Some(g.tech(1, "speed"));
                            first_vehicle_time = Some(g.state.tick as f64 / 60.);
                        }
                    }
                }
            }
        }
        g.step();
        if g.state.resources.iter().any(|r| {
            r.kind == "ore" && r.pos.distance(home) > 25. && g.state.explored[0].contains(&r.pos)
        }) {
            let bought_ai=g.orders.iter().filter(|o|o.order.owner==1&&o.receipt.accepted
                &&matches!(&o.order.command,sentinels_v6::Command::Deploy{kind,..} if catalog::unit_ref(kind).is_some_and(|d|d.category=="ai"))).count();
            assert!(
                bought_ai < 2,
                "exploration remained serial behind two expensive AI purchases"
            );
            revealed = Some(g.state.tick as f64 / 60.);
            break;
        }
    }
    assert_eq!(
        first_vehicle_tier,
        Some(1),
        "first T2 saving must not prevent paid T1 reconnaissance"
    );
    assert!(
        revealed.is_some(),
        "normal scout did not reveal forward ore by six minutes"
    );
    eprintln!(
        "normal paid scout at {:?}sec, forward mine revealed {:?}sec",
        first_vehicle_time, revealed
    );
    assert!(g.player(1).unwrap().credits >= 0.);
}
