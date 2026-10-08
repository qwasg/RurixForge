//! Ordinary 2000-credit autonomous construction evidence. The unopposed
//! scenario proves paid access to a forward service site, not PvP balance.
use sentinels_v6::{catalog, Command, Game, Order, Pos};
#[test]
fn science_pays_for_a_forward_charging_room_and_its_real_power_connection() {
    let mut g = Game::new(1000, false);
    assert_eq!(g.player(1).unwrap().credits, 2000.);
    let home = g
        .state
        .buildings
        .iter()
        .find(|b| b.owner == 1 && b.kind == "core")
        .unwrap()
        .rect
        .center();
    let mut online = None;
    for _ in 0..1500 * 60 {
        if g.state.tick % 180 == 0 {
            g.bot_for_style(1, "science", "mixed-ai");
        }
        g.step();
        if let Some(room) = g.state.rooms.iter().find(|r| {
            r.owner == 1
                && r.kind == "energy-defense"
                && r.progress >= 1.
                && r.powered
                && r.online
                && r.rect.center().distance(home) > 24.
        }) {
            online = Some(room.clone());
            break;
        }
        if g.state.winner.is_some() {
            break;
        }
    }
    let room = online.unwrap_or_else(|| panic!("no real powered forward service: tick={} cash={} rooms={:?} energy vehicles={} nodes={:?}",g.state.tick,g.player(1).unwrap().credits,g.state.rooms.iter().filter(|r|r.owner==1).map(|r|(&r.kind,r.rect,r.progress,r.powered)).collect::<Vec<_>>(),g.state.units.iter().filter(|u|u.owner==1&&catalog::unit_ref(&u.kind).is_some_and(|d|d.energy_per_attack>0.)).count(),g.state.resources.iter().filter(|r|r.kind=="node").map(|r|(r.pos,r.owner)).collect::<Vec<_>>()));
    assert!(room.invested >= catalog::facility_ref("energy-defense").unwrap().cost);
    assert!(g.orders.iter().any(|o|o.receipt.accepted&&matches!(&o.order.command,Command::Room{kind,rect,..} if kind=="energy-defense" && *rect==room.rect)));
    assert!(g.state.links.iter().any(|l| l.owner == 1
        && l.kind == "power"
        && l.active
        && l.path.iter().any(|p| room.rect.contains(*p))));
    assert!(g
        .state
        .powerGrids
        .iter()
        .any(|p| p.owner == 1 && p.cells.contains(&room.rect.center()) && p.output > p.load));
    assert!(
        g.player(1)
            .unwrap()
            .totals
            .get("ore-delivered")
            .copied()
            .unwrap_or(0.)
            > 0.
    );
    assert!(g.orders.iter().all(|o| o.order.owner == 1));
    eprintln!(
        "paid forward service id={} pos={:?} tick={} invested={} cash={} ore={}",
        room.id,
        room.rect,
        g.state.tick,
        room.invested,
        g.player(1).unwrap().credits,
        g.player(1)
            .unwrap()
            .totals
            .get("ore-delivered")
            .copied()
            .unwrap_or(0.)
    );
    // A prepared escort can now build this outpost before its first battle.
    // Establish the charging prerequisite with a real paid vehicle, an ordinary
    // movement order and actual shots; never manufacture an empty energy store.
    if !g.state.units.iter().any(|u| u.owner == 1 && u.hp > 0.
        && u.energyMax > 0. && u.energy < u.energyMax * 0.95
        && catalog::unit_ref(&u.kind).is_some_and(|d| d.category == "vehicle"))
    {
        let tank = g.state.units.iter().find(|u| u.owner == 1 && u.hp > 0.
            && u.energyMax > 0.
            && catalog::unit_ref(&u.kind).is_some_and(|d| d.category == "vehicle"))
            .expect("already purchased energy vehicle").clone();
        let enemy = g.state.buildings.iter().find(|b| b.owner == 2 && b.kind == "core" && b.hp > 0.)
            .expect("existing unopposed enemy core").rect.center();
        let mut approaches: Vec<_> = (enemy.y - 4..=enemy.y + 4)
            .flat_map(|y| (enemy.x - 4..=enemy.x + 4).map(move |x| Pos::new(x, y, 0)))
            .filter(|p| g.walkable(*p, "vehicle")).collect();
        approaches.sort_by(|a, b| a.distance(tank.pos).total_cmp(&b.distance(tank.pos)).then(a.cmp(b)));
        let approach = approaches.into_iter().find(|p| g.route(tank.pos, *p, "vehicle").is_some())
            .expect("ordinary route to a firing position near the enemy core");
        let receipt = g.order(Order { owner: 1, sequence: g.sequences[0] + 1,
            command: Command::Move { ids: vec![tank.id], pos: approach } });
        assert!(receipt.accepted, "{}", receipt.reason);
        let mut fired_and_spent = false;
        for _ in 0..180 * 60 {
            g.step();
            if let Some(unit) = g.state.units.iter().find(|u| u.id == tank.id && u.hp > 0.) {
                if unit.attackCount > tank.attackCount && unit.energy < unit.energyMax * 0.95 {
                    fired_and_spent = true;
                    eprintln!("ordinary firing prerequisite unit={} attacks={} -> {} energy={} -> {} at {:.2}s",
                        tank.id, tank.attackCount, unit.attackCount, tank.energy, unit.energy, g.state.tick as f64 / 60.);
                    break;
                }
            }
            if g.state.winner.is_some() { break; }
        }
        assert!(fired_and_spent, "existing paid vehicle must actually fire and consume its own energy");
    }
    let depleted = g.state.units.iter().filter(|u|u.owner==1 && u.hp>0. && u.energyMax>0. && u.energy<u.energyMax*0.95 && catalog::unit_ref(&u.kind).is_some_and(|d|d.category=="vehicle"))
        .min_by(|a,b|(a.energy/a.energyMax).total_cmp(&(b.energy/b.energyMax))).cloned()
        .unwrap_or_else(||panic!("need a genuinely fired vehicle, not edited empty stores: {:?}",g.state.units.iter().filter(|u|u.owner==1).map(|u|(&u.kind,u.energy,u.energyMax,u.pos)).collect::<Vec<_>>()));
    // Ordinary garrison orders preserve the existing win condition while the
    // already-fired vehicle drives to the paid outpost. No stock is edited.
    let others: Vec<_> = g.state.units.iter().filter(|u|u.owner==1 && u.id!=depleted.id && u.hp>0. && catalog::unit_ref(&u.kind).is_some_and(|d|d.speed>0. && d.category!="air")).map(|u|u.id).collect();
    let stop=g.order(Order{owner:1,sequence:g.sequences[0]+1,command:Command::Stop{ids:others}});
    assert!(stop.accepted,"{}",stop.reason);
    let mut stand=None;
    for p in room.rect.cells().into_iter().chain((room.rect.y..=room.rect.y+3).flat_map(|y|(room.rect.x..=room.rect.x+3).map(move|x|Pos::new(x,y,0)))) {
        if g.walkable(p,"vehicle") && g.route(depleted.pos,p,"vehicle").is_some() { stand=Some(p);break; }
    }
    let stand=stand.expect("actual vehicle path into forward service");
    let receipt=g.order(Order{owner:1,sequence:g.sequences[0]+1,command:Command::Move{ids:vec![depleted.id],pos:stand}});
    assert!(receipt.accepted,"{}",receipt.reason);
    let mut charged=false;
    for _ in 0..180*60 {
        let before=g.state.units.iter().find(|u|u.id==depleted.id).unwrap().energy;
        g.step();
        let unit=g.state.units.iter().find(|u|u.id==depleted.id).unwrap();
        if unit.pos.distance(room.rect.center())<=5. && unit.energy>before+1e-8 {
            assert!(g.state.powerGrids.iter().any(|p|p.owner==1&&p.cells.contains(&room.rect.center())&&p.output>p.load&&unit.energy-before<=(p.output-p.load)/60.+1e-8));
            charged=true; eprintln!("actual forward charge unit={} pos={:?} {} -> {} at {:.2}s",unit.id,unit.pos,before,unit.energy,g.state.tick as f64/60.);break;
        }
        if g.state.winner.is_some(){break;}
    }
    assert!(charged,"paid forward room has no actual charging result, destination={stand:?}");
}
