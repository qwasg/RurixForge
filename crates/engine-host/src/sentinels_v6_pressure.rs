//! Explicit, private diagnostic fixture runner; never reachable from the LAN game bridge.
use sentinels_v6::{catalog, Game, Pos, Save};
use serde_json::{json, Value};
use std::{collections::BTreeMap, fs, path::PathBuf, time::Instant};
fn quantile(v: &[f64], q: f64) -> f64 {
    v[((v.len() - 1) as f64 * q).ceil() as usize]
}
pub fn run(input: &Value) -> Result<Value, String> {
    if std::env::var("FORGE_V6_DIAGNOSTICS").as_deref() != Ok("1") {
        return Err("显式压力诊断未启用".into());
    }
    let save: Save = serde_json::from_value(input["save"].clone()).map_err(|e| e.to_string())?;
    let mut game = Game::load(save)?;
    if game
        .state
        .buildings
        .iter()
        .filter(|b| b.kind == "shell")
        .count()
        != 128
        || game.state.rooms.len() != 512
        || game.state.units.len() != 200
        || game.state.projectiles.len() != 600
    {
        return Err("压力场景必须有128壳/512房/200单位/600弹体".into());
    }
    let templates = game.state.projectiles.clone();
    let seconds = input["seconds"].as_f64().unwrap_or(60.);
    if !seconds.is_finite() || !(1.0..=120.0).contains(&seconds) {
        return Err("诊断时长须在1..120秒".into());
    }
    let maintain = |g: &mut Game| {
        let mut routes = vec![];
        for u in &g.state.units {
            if u.route.is_empty() {
                let base = 7 + ((u.pos.y - 7) / 22) * 22;
                let goal = Pos::new(
                    u.pos.x,
                    if u.pos.y - base < 6 {
                        base + 9
                    } else {
                        base + 2
                    },
                    u.level,
                );
                let d = catalog::unit_ref(&u.kind).unwrap();
                if let Some(route) = g.route(u.pos, goal, &d.category) {
                    routes.push((u.id, goal, route));
                }
            }
        }
        for u in &mut g.state.units {
            let d = catalog::unit_ref(&u.kind).unwrap();
            u.ammo = d.ammo_capacity;
            u.fuel = d.fuel_capacity;
            u.energy = d.energy_capacity;
            u.battery = u.batteryMax;
        }
        for (id, goal, route) in routes {
            let u = g.state.units.iter_mut().find(|u| u.id == id).unwrap();
            u.goal = Some(goal);
            u.route = route;
        }
        while g.state.projectiles.len() < 600 {
            let mut p = templates[g.state.projectiles.len() % templates.len()].clone();
            p.id = g.id();
            g.state.projectiles.push(p);
        }
    };
    for _ in 0..120 {
        maintain(&mut game);
        game.step();
    }
    sentinels_v6::profiling::enable();
    let mut durations = vec![];
    let mut maintenance = vec![];
    let mut frames = vec![];
    let mut moving_min = 200;
    let mut layer_activity = BTreeMap::<i32, u64>::new();
    let mut min_projectiles = usize::MAX;
    let first_tick = game.state.tick;
    let first_attacks = game.state.units.iter().map(|u| u.attackCount).sum::<u64>();
    let start = Instant::now();
    while start.elapsed().as_secs_f64() < seconds || durations.len() < 3600 {
        let t = Instant::now();
        maintain(&mut game);
        maintenance.push(t.elapsed().as_secs_f64() * 1000.);
        min_projectiles = min_projectiles.min(game.state.projectiles.len());
        let t = Instant::now();
        game.step();
        durations.push(t.elapsed().as_secs_f64() * 1000.);
        let moving = game.state.units.iter().filter(|u| u.moving).count();
        moving_min = moving_min.min(moving);
        for u in &game.state.units {
            if u.moving {
                *layer_activity.entry(u.level).or_default() += 1;
            }
        }
        if frames.len() < 120 && durations.len() % 3 == 0 {
            maintain(&mut game);
            frames.push(json!({"tick":game.state.tick,"revision":game.state.revision,"units":game.state.units,"projectiles":game.state.projectiles,"events":game.state.events,"buildings":game.state.buildings,"rooms":game.state.rooms,"links":game.state.links}));
        }
        if game.state.units.len() != 200
            || game.state.rooms.len() != 512
            || game
                .state
                .buildings
                .iter()
                .filter(|b| b.kind == "shell")
                .count()
                != 128
            || game.state.winner.is_some()
        {
            return Err(format!("压力场景实体数量失稳，tick={}", game.state.tick));
        }
    }
    let phases = sentinels_v6::profiling::report();
    sentinels_v6::profiling::disable();
    let wall = start.elapsed().as_secs_f64();
    let attacks = game.state.units.iter().map(|u| u.attackCount).sum::<u64>() - first_attacks;
    durations.sort_by(f64::total_cmp);
    maintenance.sort_by(f64::total_cmp);
    let output = PathBuf::from(std::env::var("FORGE_PROJECT_ROOT").map_err(|e| e.to_string())?)
        .join("Logs/v6/pressure");
    fs::create_dir_all(&output).map_err(|e| e.to_string())?;
    fs::write(
        output.join("render-frames.json"),
        serde_json::to_vec(&frames).unwrap(),
    )
    .map_err(|e| e.to_string())?;
    let single_layer = layer_activity.len() == 1 && layer_activity.contains_key(&0);
    let report = json!({"scope":"Unearned pressure fixture on a separate Game instance; ammunition/cache refill and minimum600 projectile replenishment are diagnostic only, excluded from actual Game::step timing. No live session or economy changed.","wallSeconds":wall,"samples":durations.len(),"ticks":game.state.tick-first_tick,"counts":{"shells":128,"rooms":512,"units":200,"minimumProjectilesBeforeStep":min_projectiles},"minimumMovingUnits":moving_min,"activeLayerSamples":layer_activity,"actualWeaponAttacks":attacks,"stepMs":{"p50":quantile(&durations,0.5),"p95":quantile(&durations,0.95),"p99":quantile(&durations,0.99),"max":durations.last()},"fixtureMaintenanceMs":{"p50":quantile(&maintenance,0.5),"p99":quantile(&maintenance,0.99)},"simulationThresholdMs":16.7,"simulationPassed":quantile(&durations,0.99)<=16.7&&single_layer&&attacks>0,"renderFrames":frames.len(),"output":output});
    fs::write(
        output.join("phase-profile.json"),
        serde_json::to_vec_pretty(&phases).unwrap(),
    )
    .map_err(|e| e.to_string())?;
    fs::write(
        output.join("simulation-report.json"),
        serde_json::to_vec_pretty(&report).unwrap(),
    )
    .map_err(|e| e.to_string())?;
    Ok(report)
}
