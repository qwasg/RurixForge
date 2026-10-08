//! Credits-only repair: drone route + 20 credits, no materials/stock.
use sentinels_v6::{catalog, Command, Game, Order, Pos, Rect};
use serde_json::json;

fn fixture() -> Game {
    let mut g = Game::new(91, false);
    g.state.terrain.fill(0);
    g.state.rooms.clear();
    g.state.units.clear();
    g.state.entrances.clear();
    g.state.links.clear();
    g.state.walls.clear();
    g.state.jobs.clear();
    let d = catalog::unit_ref("vscode").unwrap();
    g.state.units.push(
        serde_json::from_value(json!({
            "id":10003,"owner":1,"kind":"vscode",
            "pos":Pos::new(21,48,0),"x":21.5,"y":48.5,"z":0,
            "tier":d.tier,"hp":20.8,"maxHp":d.hp,
            "battery":0.,"batteryMax":0.,"covered":false,"wired":false,
            "ammo":0.,"ammoMax":0.,"fuel":0.,"fuelMax":0.,
            "energy":d.energy_capacity,"energyMax":d.energy_capacity,
            "route":[],"target":null,"cooldown":999.,"skillCooldown":0.,
            "plugins":[],"statuses":{},"invested":d.cost,"moving":false,
            "attackCount":0,"branch":d.branch,"facing":0,"altitude":0.,
            "flightState":"ground","sourceFacility":0
        }))
        .unwrap(),
    );
    g.next_id = 10004;
    g.refresh_fog();
    g.invalidate_navigation();
    g
}

#[test]
fn repair_charges_credits_once_and_queues_a_drone() {
    let mut g = fixture();
    let core = g
        .state
        .buildings
        .iter()
        .find(|b| b.owner == 1 && b.kind == "core")
        .unwrap()
        .clone();
    let rect = Rect {
        x: 21,
        y: 48,
        level: 0,
        width: 1,
        height: 1,
    };
    assert!(g
        .construction_route(core.rect.center(), rect, "repair")
        .is_some());
    let cash = g.player(1).unwrap().credits;
    let result = g.order(Order {
        owner: 1,
        sequence: 1,
        command: Command::Repair { id: 10003 },
    });
    assert!(result.accepted, "{}", result.reason);
    assert_eq!(g.player(1).unwrap().credits, cash - 20.);
    assert_eq!(g.state.jobs.len(), 1);
    assert_eq!(g.state.jobs[0].kind, "repair");
    let after = serde_json::to_value(&g.state).unwrap();
    assert!(g
        .order(Order {
            owner: 1,
            sequence: 1,
            command: Command::Repair { id: 10003 },
        })
        .accepted);
    assert_eq!(serde_json::to_value(&g.state).unwrap(), after);
    assert!(!g
        .order(Order {
            owner: 1,
            sequence: 2,
            command: Command::Repair { id: 10003 },
        })
        .accepted);
    assert_eq!(g.player(1).unwrap().credits, cash - 20.);
    assert_eq!(g.state.jobs.len(), 1);
}

#[test]
fn broke_owner_cannot_start_repair_without_spending() {
    let mut g = fixture();
    g.player_mut(1).unwrap().credits = 0.;
    let r = g.order(Order {
        owner: 1,
        sequence: 1,
        command: Command::Repair { id: 10003 },
    });
    assert!(!r.accepted);
    assert!(g.state.jobs.is_empty());
    assert_eq!(g.player(1).unwrap().credits, 0.);
}

#[test]
fn repair_job_restores_unit_hp_without_materials() {
    let mut g = fixture();
    let before = g.state.units[0].hp;
    assert!(g
        .order(Order {
            owner: 1,
            sequence: 1,
            command: Command::Repair { id: 10003 },
        })
        .accepted);
    for _ in 0..1200 {
        g.step();
        if g.state.jobs.is_empty() {
            break;
        }
    }
    assert!(g.state.jobs.is_empty(), "repair should finish: {:?}", g.state.jobs);
    assert!(g.state.units[0].hp > before);
}
