//! Abstract supply: no ammo gate, aircraft endurance, credit repair, repair aura, plant upkeep.
use sentinels_v6::{catalog, types::*, Game};
use serde_json::json;

fn unit(id: u64, owner: u32, kind: &str, pos: Pos) -> Unit {
    let d = catalog::unit_ref(kind).unwrap();
    serde_json::from_value(json!({
        "id":id,"owner":owner,"kind":kind,"pos":pos,
        "x":pos.x as f64+0.5,"y":pos.y as f64+0.5,"z":0,
        "tier":d.tier,"hp":d.hp,"maxHp":d.hp,
        "battery":240.,"batteryMax":240.,
        "covered":false,"wired":false,
        "ammo":0.,"ammoMax":0.,
        "fuel":d.fuel_capacity,"fuelMax":d.fuel_capacity,
        "energy":d.energy_capacity.max(40.),"energyMax":d.energy_capacity.max(40.),
        "route":[],"target":null,"cooldown":0.,"skillCooldown":0.,
        "plugins":[],"statuses":{},"invested":d.cost,
        "moving":false,"attackCount":0,"branch":d.branch,
        "facing":0,"altitude":0.,"flightState":"ground"
    }))
    .unwrap()
}

#[test]
fn ground_unit_can_fire_with_zero_ammo() {
    let mut g = Game::new(33, false);
    g.state.terrain.fill(0);
    g.state.visible[0] = (0..96)
        .flat_map(|y| (0..128).map(move |x| Pos::new(x, y, 0)))
        .collect();
    g.state.explored[0] = g.state.visible[0].clone();
    g.state.units.clear();
    g.state.units.push(unit(1, 1, "scout-buggy", Pos::new(30, 40, 0)));
    g.state.units.push(unit(2, 2, "scout-buggy", Pos::new(34, 40, 0)));
    g.state.units[0].ammo = 0.;
    g.state.units[0].ammoMax = 0.;
    g.state.units[0].energy = 100.;
    g.state.units[0].energyMax = 100.;
    let hp = g.state.units[1].hp;
    for _ in 0..180 {
        g.step();
        if g.state.units[1].hp < hp {
            break;
        }
    }
    assert!(
        g.state.units[1].hp < hp || g.state.units[0].attackCount > 0,
        "zero-ammo ground unit must still be able to engage"
    );
}

#[test]
fn coal_power_burns_credits_as_upkeep() {
    let mut g = Game::new(35, false);
    let mut plant = g.state.buildings[0].clone();
    plant.id = 800;
    plant.owner = 1;
    plant.kind = "coal-power".into();
    plant.progress = 1.;
    plant.hp = 500.;
    plant.powered = true;
    plant.power = 120.;
    plant.rect = Rect {
        x: 20,
        y: 20,
        level: 0,
        width: 3,
        height: 3,
    };
    g.state.buildings.push(plant);
    g.player_mut(1).unwrap().credits = 50.;
    for _ in 0..60 {
        g.step();
    }
    assert!(
        g.player(1).unwrap().credits < 50.,
        "coal upkeep should spend credits"
    );
}

#[test]
fn credit_repair_restores_unit_without_materials() {
    let mut g = Game::new(37, false);
    g.state.units.clear();
    let mut u = unit(201, 1, "gemini", Pos::new(22, 44, 0));
    u.hp = 40.;
    g.state.units.push(u);
    for b in &mut g.state.buildings {
        b.stock.clear();
    }
    let credits = g.player(1).unwrap().credits;
    let r = g.order(Order {
        owner: 1,
        sequence: 1,
        command: Command::Repair { id: 201 },
    });
    assert!(r.accepted, "{}", r.reason);
    assert_eq!(g.player(1).unwrap().credits, credits - 20.);
    for _ in 0..1200 {
        g.step();
        if g.state.units[0].hp >= g.state.units[0].maxHp {
            break;
        }
    }
    assert_eq!(g.state.units[0].hp, g.state.units[0].maxHp);
}
