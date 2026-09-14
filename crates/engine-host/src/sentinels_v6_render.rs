//! Instanced Rurix/Vulkan isometric geometry. One native raster pass draws all
//! visible modular terrain, floors, walls and equipment; no per-cell draw slots.
use crate::sentinels_v6::{iso, View};
use crate::sentinels_v6_assets::{self as assets, Sprite};
use forge_scene::{Component, Entity, Scene, Transform};
use rurix_rt::render_exec as rex;
use sentinels_v6::{Pos, Snapshot};
use serde_json::json;
use std::collections::{BTreeMap, BTreeSet, HashMap, VecDeque};
use std::sync::{Arc, Mutex, OnceLock};
const MAX: usize = 32768;
type RenderTiming = [VecDeque<f64>; 5];
static RENDER_TIMINGS: OnceLock<Mutex<BTreeMap<i32, RenderTiming>>> = OnceLock::new();
fn record_timing(layer: i32, values: [f64; 5]) {
    let mut timings = RENDER_TIMINGS
        .get_or_init(|| Mutex::new(BTreeMap::new()))
        .lock()
        .unwrap();
    let rows = timings.entry(layer).or_default();
    for (row, value) in rows.iter_mut().zip(values) {
        if row.len() >= 8192 {
            row.pop_front();
        }
        row.push_back(value);
    }
}
pub fn render_metrics() -> serde_json::Value {
    let timings = RENDER_TIMINGS
        .get_or_init(|| Mutex::new(BTreeMap::new()))
        .lock()
        .unwrap();
    json!(timings.iter().map(|(layer,rows)|(*layer,json!({"compose":crate::sentinels_v6::metric(&rows[0]),"prepareUploads":crate::sentinels_v6::metric(&rows[1]),"executeAndReadback":crate::sentinels_v6::metric(&rows[2]),"pixelStats":crate::sentinels_v6::metric(&rows[3]),"total":crate::sentinels_v6::metric(&rows[4])}))).collect::<BTreeMap<_,_>>())
}
type Record = [f32; 12];
struct Batch {
    records: Vec<Record>,
    sprites: Vec<Sprite>,
    terrain: Arc<TerrainBatch>,
    view: [f32; 4],
    fallbacks: usize,
}
fn record(x: f64, y: f64, z: f64, w: f64, h: f64, height: f64, color: [f32; 4]) -> Record {
    [
        x as f32,
        y as f32,
        z as f32,
        0.,
        w as f32,
        h as f32,
        height as f32,
        1.,
        color[0],
        color[1],
        color[2],
        color[3],
    ]
}
fn room_floor(rect: sentinels_v6::Rect, color: [f32; 4]) -> Record {
    record(rect.x as f64 + 0.12, rect.y as f64 + 0.12,
        rect.level as f64 + 0.13, rect.width as f64 - 0.24,
        rect.height as f64 - 0.24, 0.08, color)
}
/// Intersect the view ray with the same f32 box sent to VS. Its nearest surface
/// is one of the three rendered faces (+X, +Y, +Z), and shares their depth key.
fn record_pick_depth(item: &Record, screen: (f64, f64)) -> Option<f64> {
    let base_x = screen.0 - 2. * screen.1;
    let base_y = -screen.0 - 2. * screen.1;
    let lo = (item[2] as f64)
        .max((item[0] as f64 - base_x) / 3.)
        .max((item[1] as f64 - base_y) / 3.);
    let hi = ((item[2] + item[6]) as f64)
        .min(((item[0] + item[4]) as f64 - base_x) / 3.)
        .min(((item[1] + item[5]) as f64 - base_y) / 3.);
    (lo <= hi).then_some(base_x + base_y + 8. * hi)
}
fn team(owner: u32) -> [f32; 4] {
    if owner == 1 {
        [0.18, 0.68, 0.76, 1.]
    } else {
        [0.88, 0.34, 0.23, 1.]
    }
}
fn sprite_depth_key(x:f64,y:f64,z:f64,screen_delta_y:f64)->f64{x+y+z*2.+0.25+(-4.*screen_delta_y).max(0.)}
#[derive(Clone)]
struct TerrainBatch {
    layer: i32,
    camera: [u64; 3],
    local_player: u32,
    terrain: Vec<u8>,
    visible: BTreeSet<Pos>,
    explored: BTreeSet<Pos>,
    excavated: BTreeSet<Pos>,
    at: std::time::Instant,
    records: Vec<Record>,
    sprites: Vec<Sprite>,
}
static TERRAIN_BATCH: OnceLock<Mutex<Option<Arc<TerrainBatch>>>> = OnceLock::new();
fn terrain_batch(s: &Snapshot, v: &View) -> Arc<TerrainBatch> {
    let mut cache = TERRAIN_BATCH
        .get_or_init(|| Mutex::new(None))
        .lock()
        .unwrap();
    let camera = [v.center_x.to_bits(), v.center_y.to_bits(), v.zoom.to_bits()];
    let visible = &s.visible[(v.local_player - 1) as usize];
    let explored = &s.explored[(v.local_player - 1) as usize];
    if let Some(old) = cache.as_ref() {
        if old.layer == v.layer
            && old.camera == camera
            && old.local_player == v.local_player
            && old.terrain == s.terrain
            && old.visible == *visible
            && old.explored == *explored
            && old.excavated == s.excavated
            && old.at.elapsed().as_secs_f64() < 2.
        {
            return old.clone();
        }
    }
    let half = 24. / v.zoom;
    let (cx, cy) = iso(v.center_x, v.center_y, v.layer as f64);
    let in_view = |x: f64, y: f64, z: f64, w: f64, h: f64| {
        let (px, py) = iso(x, y, z);
        (px - cx).abs() < half * 16. / 9. + w + h + 3. && (py - cy).abs() < half + w + h + 3.
    };
    let mut sprites = Vec::new();
    let mut records = Vec::new();
    for y in 0..96 {
        for x in 0..128 {
            if !in_view(x as f64, y as f64, v.layer as f64, 1., 1.) {
                continue;
            }
            let p = Pos::new(x, y, v.layer);
            let ground = v.layer == 0 || v.layer < 0 && s.excavated.contains(&p);
            if !ground {
                continue;
            }
            let t = s.terrain[(y * 128 + x) as usize];
            let mut c = if v.layer < 0 {
                [0.19, 0.23, 0.27, 1.]
            } else {
                match t {
                    1 => [0.32, 0.34, 0.37, 1.],
                    2 => [0.08, 0.24, 0.34, 1.],
                    3 => [0.42, 0.39, 0.3, 1.],
                    4 => [0.27, 0.38, 0.28, 1.],
                    5 => [0.26, 0.56, 0.62, 1.],
                    6 => [0.18, 0.19, 0.22, 1.],
                    _ => [0.20, 0.30, 0.23, 1.],
                }
            };
            let shade = if visible.contains(&p) {
                1.
            } else if explored.contains(&p) {
                0.46
            } else {
                0.15
            };
            for k in 0..3 {
                c[k] *= shade;
            }
            let noise = ((x * 17 + y * 29) as f32).sin() * 0.015;
            for k in 0..3 {
                c[k] += noise;
            }
            let terrain_key = if v.layer < 0 {
                "soil"
            } else {
                match t {
                    1 => "rock",
                    2 => "water",
                    3 => "road",
                    4 => "highland",
                    5 => "ore",
                    6 => "coal",
                    _ => "grass",
                }
            };
            let before = sprites.len();
            let terrain_tint = assets::terrain_tint(terrain_key);
            if sprite(
                &mut sprites,
                &format!("terrain:{terrain_key}"),
                x as f64 + 0.5,
                y as f64 + 0.5,
                v.layer as f64 + if t == 4 { 0.12 } else { 0.03 },
                1.,
                0,
                "idle",
                0.,
                [
                    shade * terrain_tint[0],
                    shade * terrain_tint[1],
                    shade * terrain_tint[2],
                    1.,
                ],
            ) {
                if sprites.len() > before {
                    sprites.last_mut().unwrap().ground = true;
                }
            }
            records.push(record(
                x as f64,
                y as f64,
                v.layer as f64,
                1.,
                1.,
                if t == 4 { 0.11 } else { 0.02 },
                c,
            ));
        }
    }
    let result = Arc::new(TerrainBatch {
        layer: v.layer,
        camera,
        local_player: v.local_player,
        terrain: s.terrain.clone(),
        visible: visible.clone(),
        explored: explored.clone(),
        excavated: s.excavated.clone(),
        at: std::time::Instant::now(),
        records,
        sprites,
    });
    *cache = Some(result.clone());
    result
}
fn compose(s: &Snapshot, v: &View, visual_seconds: f64) -> Batch {
    let mut sprites: Vec<Sprite> = Vec::new();
    let mut records: Vec<Record> = Vec::new();
    // Only share immutable metadata results within this composition. A later
    // frame still observes normal asset invalidation and animation duration.
    let mut effect_durations: HashMap<String, f64> = HashMap::new();
    let half = 24. / v.zoom;
    let (cx, cy) = iso(v.center_x, v.center_y, v.layer as f64);
    let in_view = |x: f64, y: f64, z: f64, w: f64, h: f64| {
        let (px, py) = iso(x, y, z);
        (px - cx).abs() < half * 16. / 9. + w + h + 3. && (py - cy).abs() < half + w + h + 3.
    };
    let terrain = terrain_batch(s, v);
    records.extend_from_slice(&terrain.records);
    let mut objects: Vec<Record> = Vec::new();
    for b in &s.buildings {
        if b.rect.level > v.layer && v.cutaway || b.rect.level < v.layer - 1 {
            continue;
        }
        let r = b.rect;
        let z = r.level as f64;
        if !in_view(r.x as f64, r.y as f64, z, r.width as f64, r.height as f64) {
            continue;
        }
        let c = team(b.owner);
        let model = if b.kind == "core" {
            "command-core"
        } else {
            b.kind.as_str()
        };
        if b.kind != "shell"
            && sprite(
                &mut sprites,
                model,
                r.x as f64 + r.width as f64 * 0.5,
                r.y as f64 + r.height as f64 * 0.5,
                z,
                1.,
                8,
                if b.powered && b.progress >= 1. {
                    "work"
                } else {
                    "idle"
                },
                visual_seconds,
                [1., 1., 1., (0.15 + b.progress * 0.85) as f32],
            )
        {
            tag_last(&mut sprites, "building", b.id, b.owner, b.rect.center());
            continue;
        }
        if b.kind == "shell" {
            objects.push(record(
                r.x as f64,
                r.y as f64,
                z,
                r.width as f64,
                r.height as f64,
                0.12,
                [0.35, 0.39, 0.43, 1.],
            ));
            let h = if r.level == v.layer && v.cutaway {
                0.35
            } else {
                0.95
            } * b.progress.max(0.15);
            let mut edges = Vec::new();
            for x in r.x..r.x + r.width {
                edges.push((Pos::new(x, r.y, r.level), 5, false));
                edges.push((Pos::new(x, r.y + r.height - 1, r.level), 1, true));
            }
            for y in r.y..r.y + r.height {
                edges.push((Pos::new(r.x, y, r.level), 3, false));
                edges.push((Pos::new(r.x + r.width - 1, y, r.level), 7, true));
            }
            for (p, dir, front) in edges {
                if front && v.cutaway && r.level == v.layer {
                    continue;
                }
                if s.entrances
                    .iter()
                    .any(|e| (e.kind == "door" || e.hp > 0. && e.kind == "window") && e.covers(p))
                {
                    continue;
                }
                if sprite(
                    &mut sprites,
                    "wall",
                    p.x as f64 + 0.5,
                    p.y as f64 + 0.5,
                    z,
                    1.,
                    dir,
                    "idle",
                    0.,
                    [1., 1., 1., (0.15 + b.progress * 0.85) as f32],
                ) {
                    tag_last(&mut sprites, "building", b.id, b.owner, b.rect.center());
                } else {
                    objects.push(record(
                        p.x as f64,
                        p.y as f64,
                        z,
                        if dir % 4 == 1 { 0.92 } else { 0.16 },
                        if dir % 4 == 1 { 0.16 } else { 0.92 },
                        h,
                        c,
                    ));
                }
            }
            if !v.cutaway && b.progress > 0.75 {
                for y in r.y..r.y + r.height {
                    for x in r.x..r.x + r.width {
                        if s.buildings.iter().any(|other| {
                            other.kind == "shell"
                                && other.hp > 0.
                                && other.rect.level == r.level + 1
                                && other.rect.contains(Pos::new(x, y, r.level + 1))
                        }) {
                            continue;
                        }
                        if sprite(
                            &mut sprites,
                            "roof",
                            x as f64 + 0.5,
                            y as f64 + 0.5,
                            z + 0.9,
                            1.,
                            8,
                            "idle",
                            0.,
                            [1.; 4],
                        ) {
                            tag_last(&mut sprites, "building", b.id, b.owner, b.rect.center());
                        } else {
                            objects.push(record(
                                x as f64,
                                y as f64,
                                z + 0.9,
                                1.,
                                1.,
                                0.08,
                                [0.4, 0.43, 0.48, 1.],
                            ));
                        }
                    }
                }
            }
        } else {
            objects.push(record(
                r.x as f64,
                r.y as f64,
                z,
                r.width as f64,
                r.height as f64,
                if b.kind == "wind-power" { 2.3 } else { 1.1 } * b.progress.max(0.1),
                c,
            ));
            if b.kind == "core" {
                objects.push(record(
                    (r.x + 1) as f64,
                    (r.y + 1) as f64,
                    z + 1.1,
                    2.,
                    2.,
                    0.8,
                    [0.78, 0.76, 0.56, 1.],
                ));
            }
        }
    }
    for r in &s.rooms {
        if r.rect.level != v.layer {
            continue;
        }
        let p = r.rect;
        let color = match r.kind.as_str() {
            "data-center" => [0.12, 0.44, 0.60, 1.],
            "research-lab" => [0.40, 0.29, 0.58, 1.],
            "depot" => [0.48, 0.39, 0.19, 1.],
            _ => [0.28, 0.37, 0.42, 1.],
        };
        objects.push(room_floor(p, color));
        if r.progress >= 1. {
            let count = if r.kind == "data-center" {
                r.gpus.len().max(1)
            } else {
                1usize
            };
            for i in 0..count.min(64) {
                let x = p.x as f64 + 0.5 + (i as i32 % (p.width - 1).max(1)) as f64;
                let y = p.y as f64 + 0.5 + (i as i32 / (p.width - 1).max(1)) as f64;
                if y < (p.y + p.height) as f64 {
                    if sprite(
                        &mut sprites,
                        &r.kind,
                        x,
                        y,
                        p.level as f64 + 0.2,
                        if r.kind == "data-center" { 0.6 } else { 1. },
                        8,
                        if r.powered
                            && (r.kind != "research-lab" || r.online)
                            && (r.kind != "research-lab"
                                || s.players
                                    .iter()
                                    .any(|p| p.researches.iter().any(|task| task.lab == r.id)))
                            && (r.kind != "data-center" || !r.gpus.is_empty())
                        {
                            "work"
                        } else {
                            "idle"
                        },
                        visual_seconds,
                        if r.powered {
                            [1.; 4]
                        } else {
                            [0.6, 0.6, 0.6, 1.]
                        },
                    ) {
                        tag_last(&mut sprites, "room", r.id, r.owner, r.rect.center());
                        continue;
                    }
                    objects.push(record(
                        x,
                        y,
                        p.level as f64 + 0.2,
                        0.55,
                        0.65,
                        0.55,
                        if r.powered {
                            [0.16, 0.76, 0.80, 1.]
                        } else {
                            [0.32, 0.35, 0.38, 1.]
                        },
                    ));
                }
            }
        }
    }
    let wall_positions: BTreeSet<Pos> = s.walls.iter().map(|w| w.pos).collect();
    for w in &s.walls {
        if w.pos.level == v.layer {
            if sprite(
                &mut sprites,
                if w.kind == "moat" {
                    "moat"
                } else if w.kind == "physical" {
                    "physical-wall"
                } else {
                    "cuda-wall"
                },
                w.pos.x as f64 + 0.5,
                w.pos.y as f64 + 0.5,
                w.pos.level as f64,
                1.,
                if wall_positions.contains(&Pos::new(w.pos.x + 1, w.pos.y, w.pos.level))
                    || wall_positions.contains(&Pos::new(w.pos.x - 1, w.pos.y, w.pos.level))
                {
                    5
                } else {
                    3
                },
                "idle",
                0.,
                [1.; 4],
            ) {
                tag_last(&mut sprites, "wall", w.id, w.owner, w.pos);
                continue;
            }
            objects.push(record(
                w.pos.x as f64,
                w.pos.y as f64,
                w.pos.level as f64,
                0.9,
                0.9,
                if w.kind == "moat" { 0.1 } else { 0.7 },
                if w.kind == "physical" {
                    [0.49, 0.49, 0.46, 1.]
                } else {
                    [0.16, 0.59, 0.74, 1.]
                },
            ));
        }
    }
    for l in &s.links {
        for p in l.path.iter().filter(|p| p.level == v.layer) {
            objects.push(record(
                p.x as f64 + 0.4,
                p.y as f64 + 0.4,
                p.level as f64 + 0.15,
                0.18,
                0.18,
                0.06,
                if l.active {
                    if l.kind == "power" {
                        [0.95, 0.68, 0.19, 1.]
                    } else {
                        [0.27, 0.88, 0.88, 1.]
                    }
                } else {
                    [0.45, 0.23, 0.20, 1.]
                },
            ));
        }
    }
    for r in &s.resources {
        if r.pos.level == v.layer {
            let model = if r.kind == "node" {
                "strategic-node"
            } else if r.kind == "coal" {
                "coal-node"
            } else if r.kind == "ore" {
                "ore-node"
            } else {
                "depot"
            };
            if sprite(
                &mut sprites,
                model,
                r.pos.x as f64 + 0.5,
                r.pos.y as f64 + 0.5,
                r.pos.level as f64 + 0.1,
                if r.kind == "node" { 1. } else { 0.65 },
                8,
                "idle",
                visual_seconds,
                if r.kind == "node" && r.owner > 0 {
                    let c = team(r.owner);
                    [0.6 + c[0] * 0.4, 0.6 + c[1] * 0.4, 0.6 + c[2] * 0.4, 1.]
                } else {
                    [1.; 4]
                },
            ) {
                tag_last(&mut sprites, "resource", r.id, r.owner, r.pos);
                continue;
            }
            objects.push(record(
                r.pos.x as f64,
                r.pos.y as f64,
                r.pos.level as f64,
                1.5,
                1.5,
                if r.kind == "node" { 1.2 } else { 0.45 },
                if r.owner == 0 {
                    [0.74, 0.64, 0.30, 1.]
                } else {
                    team(r.owner)
                },
            ));
        }
    }
    for u in &s.units {
        if u.level == v.layer {
            let def = sentinels_v6::catalog::unit_ref(&u.kind).unwrap();
            let cat = def.category.clone();
            let model = if assets::available(&u.kind) {
                u.kind.clone()
            } else {
                def.chassis.clone()
            };
            let dir = u.facing as usize;
            let since_cast = u
                .lastCastTick
                .map(|tick| (visual_seconds - tick as f64 / 60.).max(0.))
                .unwrap_or(100.);
            let since_attack = u
                .lastAttackTick
                .map(|tick| (visual_seconds - tick as f64 / 60.).max(0.))
                .unwrap_or(100.);
            let since_hit = u
                .lastHitTick
                .map(|tick| (visual_seconds - tick as f64 / 60.).max(0.))
                .unwrap_or(100.);
            let cast_duration = assets::animation_duration(&model, "cast", dir).unwrap_or(1.8);
            let hit_duration = assets::animation_duration(&model, "hit", dir).unwrap_or(0.4);
            let attack_duration = assets::animation_duration(&model, "attack", dir).unwrap_or(0.7);
            let attack_window =
                attack_duration.min((u.cooldown + since_attack).min(def.period).max(0.08));
            let (action, age) = if since_hit < hit_duration && since_hit < since_cast {
                ("hit", since_hit)
            } else if since_cast < cast_duration {
                ("cast", since_cast)
            } else if since_attack < attack_window {
                ("attack", since_attack * attack_duration / attack_window)
            } else if u.moving {
                ("walk", visual_seconds)
            } else {
                ("idle", visual_seconds)
            };
            if sprite(
                &mut sprites,
                &model,
                u.x,
                u.y,
                u.elevation() + 0.1,
                1.,
                dir,
                action,
                age,
                [1.; 4],
            ) {
                tag_last(&mut sprites, "unit", u.id, u.owner, u.pos);
                let heading = std::f64::consts::FRAC_PI_4 * (u.facing as f64 + 1.);
                for (slot, plugin) in u.plugins.iter().enumerate() {
                    let side = (slot as f64 - 1.) * 0.25;
                    let px = u.x - heading.sin() * side;
                    let py = u.y + heading.cos() * side;
                    if sprite(
                        &mut sprites,
                        &format!("plugin-{plugin}"),
                        px,
                        py,
                        u.elevation() + 0.6,
                        0.55,
                        dir,
                        "idle",
                        0.,
                        [1.; 4],
                    ) {
                        tag_last(&mut sprites, "unit", u.id, u.owner, u.pos);
                    }
                }
                continue;
            }
            objects.push(record(
                u.x - 0.35,
                u.y - 0.35,
                u.level as f64 + u.altitude + 0.2,
                0.7,
                0.7,
                if cat == "ai" { 0.8 } else { 0.4 },
                team(u.owner),
            ));
        }
    }
    for truck in &s.shipments {
        if truck.pos.level == v.layer {
            let next = truck.route.first().copied().unwrap_or(truck.pos);
            let progress = truck.progress.clamp(0., 1.);
            let px = truck.pos.x as f64 + 0.5 + (next.x - truck.pos.x) as f64 * progress;
            let py = truck.pos.y as f64 + 0.5 + (next.y - truck.pos.y) as f64 * progress;
            let pz = truck.pos.level as f64
                + truck.altitude
                + 0.1
                + (next.level - truck.pos.level) as f64 * progress;
            let dir = truck
                .route
                .first()
                .map(|p| assets::direction((p.x - truck.pos.x) as f64, (p.y - truck.pos.y) as f64))
                .unwrap_or(0);
            if sprite(
                &mut sprites,
                if truck.mode == "air" {
                    "cargo-aircraft"
                } else {
                    "cargo-truck"
                },
                px,
                py,
                pz,
                if truck.mode == "air" { 1. } else { 0.75 },
                dir,
                if truck.mode == "air" && truck.flightState != "landed" {
                    "work"
                } else {
                    "idle"
                },
                visual_seconds,
                [1.; 4],
            ) {
                tag_last(&mut sprites, "shipment", truck.id, truck.owner, truck.pos);
                continue;
            }
            objects.push(record(px, py, pz, 1.1, 0.65, 0.4, [0.79, 0.59, 0.27, 1.]));
        }
    }
    for p in &s.projectiles {
        if p.destination.level == v.layer {
            objects.push(record(p.x, p.y, p.z, 0.18, 0.18, 0.18, [1., 0.78, 0.3, 1.]));
        }
    }
    for e in s.entrances.iter().filter(|e| e.hp > 0.) {
        if e.serves(v.layer) {
            for offset in 0..e.width {
                let pos = Pos::new(
                    e.pos.x + if e.axis == "x" { offset as i32 } else { 0 },
                    e.pos.y + if e.axis == "y" { offset as i32 } else { 0 },
                    v.layer,
                );
                let model = if e.kind == "window" {
                    "window-wall"
                } else {
                    e.kind.as_str()
                };
                let action = if e.kind == "door" {
                    if e.open {
                        "open"
                    } else {
                        "closed"
                    }
                } else {
                    "idle"
                };
                if sprite(
                    &mut sprites,
                    model,
                    pos.x as f64 + 0.5,
                    pos.y as f64 + 0.5,
                    v.layer as f64 + 0.15,
                    1.,
                    if e.axis == "y" { 3 } else { 5 },
                    action,
                    0.,
                    [1.; 4],
                ) {
                    tag_last(&mut sprites, "entrance", e.id, e.owner, pos);
                }
            }
        }
    }
    for j in &s.jobs {
        if j.rect.level == v.layer && j.progress > 0. && !j.blocked {
            sprite(
                &mut sprites,
                "fx:construction-dust",
                j.rect.center().x as f64,
                j.rect.center().y as f64,
                j.rect.level as f64 + 0.1,
                1.,
                0,
                "oneshot",
                visual_seconds % assets::animation_duration("fx:construction-dust","oneshot",0).unwrap_or(1.).max(1./120.),
                [1.; 4],
            );
        }
        if j.worker.level == v.layer {
            sprite(
                &mut sprites,
                "engineer-drone",
                j.worker.x as f64 + 0.5,
                j.worker.y as f64 + 0.5,
                j.worker.level as f64 + 0.5,
                0.55,
                0,
                "idle",
                0.,
                [1.; 4],
            );
        }
    }
    let mut impact_groups: BTreeMap<u64, Vec<(Pos, f64)>> = BTreeMap::new();
    for event in s.events.iter().filter(|e| e.kind == "projectile-impact") {
        impact_groups
            .entry(event.tick)
            .or_default()
            .push((event.pos, event.magnitude.max(1.5)));
    }
    for event in &s.events {
        let age = (visual_seconds - event.tick as f64 / 60.).max(0.);
        if event.pos.level != v.layer {
            continue;
        }
        if let Some(kind) = event
            .kind
            .strip_prefix("unit-death:")
            .or_else(|| (event.kind == "transport-destroy").then_some(event.subjectKind.as_str()))
        {
            let def = sentinels_v6::catalog::unit_ref(kind);
            let character = def.is_some_and(|d| d.category == "ai");
            let model = if assets::available(kind) {
                kind
            } else {
                def.map(|d| d.chassis.as_str()).unwrap_or(kind)
            };
            let location = event.presentationPosition.unwrap_or([
                event.pos.x as f64 + 0.5,
                event.pos.y as f64 + 0.5,
                event.pos.level as f64 + 0.1,
            ]);
            if age < 32. {
                let before = sprites.len();
                sprite(
                    &mut sprites,
                    model,
                    location[0],
                    location[1],
                    location[2]
                        - if character {
                            0.
                        } else {
                            (age * age * 0.4).min((location[2] - event.pos.level as f64).max(0.))
                        },
                    1.,
                    event
                        .facing
                        .map(|v| v as usize)
                        .or_else(|| {
                            event.direction.map(|p| {
                                assets::direction(
                                    (p.x - event.pos.x) as f64,
                                    (p.y - event.pos.y) as f64,
                                )
                            })
                        })
                        .unwrap_or(0),
                    if character { "death" } else { "idle" },
                    age,
                    [1.; 4],
                );
                if sprites.len() > before {
                    let last = sprites.last_mut().unwrap();
                    last.transient = true;
                    let duration = if character {
                        assets::describe(last)
                            .ok()
                            .flatten()
                            .map(|f| f.duration)
                            .unwrap_or(2.5)
                    } else {
                        2.5
                    };
                    if age >= duration {
                        sprites.pop();
                    } else if !character {
                        last.tint = [0.65, 0.65, 0.65, (1. - age / duration) as f32];
                        last.rotation = age as f32 * 0.25;
                    }
                }
                if !character && age < 1.8 {
                    sprite(
                        &mut sprites,
                        "fx:heavy-impact",
                        location[0],
                        location[1],
                        location[2],
                        1.,
                        0,
                        "oneshot",
                        age,
                        [1.; 4],
                    );
                }
            }
            continue;
        }
        if event.kind == "destroy" && age < 2.5 {
            if let Some(rect) = event.rect {
                let fade = (1. - age / 2.5).max(0.) as f32;
                let fall = age * age * 0.35;
                if event.subjectKind == "shell" {
                    for i in 0..(rect.width + rect.height).min(36) {
                        let x = rect.x + (i % rect.width);
                        let y = rect.y + (i / rect.width);
                        let before = sprites.len();
                        sprite(
                            &mut sprites,
                            "wall",
                            x as f64 + 0.5,
                            y as f64 + 0.5,
                            rect.level as f64 - fall,
                            1.,
                            0,
                            "idle",
                            0.,
                            [0.65, 0.65, 0.65, fade],
                        );
                        if sprites.len() > before {
                            sprites.last_mut().unwrap().rotation = age as f32 * 0.5;
                        }
                    }
                } else {
                    let model = if event.subjectKind == "core" {
                        "command-core"
                    } else {
                        &event.subjectKind
                    };
                    sprite(
                        &mut sprites,
                        model,
                        rect.x as f64 + rect.width as f64 * 0.5,
                        rect.y as f64 + rect.height as f64 * 0.5,
                        rect.level as f64 - fall,
                        1.,
                        8,
                        "idle",
                        age,
                        [0.55, 0.55, 0.55, fade],
                    );
                }
            }
        }
        if age < 8. {
            if matches!(
                event.kind.as_str(),
                "impact" | "energy-impact" | "explosive-impact"
            ) && impact_groups.get(&event.tick).is_some_and(|hits| {
                hits.iter()
                    .any(|(p, r)| p.level == event.pos.level && p.distance(event.pos) <= *r + 1.)
            }) {
                continue;
            }
            let fx = match event.kind.as_str() {
                "projectile-impact" => match event.subjectKind.as_str() {
                    "orbital" | "delayed-area" => "orbital-strike",
                    "arc" | "guided" => "heavy-impact",
                    "beam" => "plasma-hit",
                    _ => "kinetic-hit",
                },
                "impact" => "kinetic-hit",
                "fire" => {
                    if sentinels_v6::catalog::unit_ref(&event.subjectKind)
                        .is_some_and(|d| d.energy_per_attack > 0. || d.category == "ai")
                    {
                        "power-arc"
                    } else {
                        "kinetic-hit"
                    }
                }
                "energy-impact" => "plasma-hit",
                "explosive-impact" => "heavy-impact",
                "destroy" => "collapse-explosion",
                "collapse" => "floor-collapse",
                "construction-complete" | "research" => "upgrade",
                "repair" | "skill-repair-armor" => "repair-field",
                "passive-maintenance-daemon" => "repair-field",
                "shield-absorb" => "shield-hit",
                "guidance-jammed" => "power-arc",
                "skill-directional-dash"
                | "skill-penetrating-mark"
                | "skill-dash-strike"
                | "skill-piercing-mark"
                | "skill-precision" => "directional-beam",
                "skill-cone-interception"
                | "skill-intercept-barrier"
                | "skill-guard"
                | "intercept" => "energy-barrier",
                "passive-guardian-intercept" => "energy-barrier",
                "skill-repair-heal-cut" | "skill-debug-cone" => "cone-shockwave",
                "skill-telegraphed-bombardment" => continue,
                "skill-target-support"
                | "skill-targeted-support"
                | "skill-field-resupply"
                | "field-resupply" => "repair-field",
                "skill-target-lock" => "network-shield",
                "skill-overclock" | "skill-charged-shot" => "upgrade",
                "telegraph" => "network-shield",
                _ => continue,
            };
            let fx_asset = format!("fx:{fx}");
            let duration = if event.kind == "telegraph" {
                2.2
            } else {
                *effect_durations.entry(fx_asset.clone()).or_insert_with(|| {
                    assets::animation_duration(&fx_asset, "oneshot", 0).unwrap_or(1.8)
                })
            };
            if age >= duration {
                continue;
            }
            let before = sprites.len();
            let origin = if matches!(event.kind.as_str(), "fire" | "projectile-impact") {
                event.presentationPosition.unwrap_or([
                    event.pos.x as f64 + 0.5,
                    event.pos.y as f64 + 0.5,
                    event.pos.level as f64 + 0.4,
                ])
            } else {
                [
                    event.pos.x as f64 + 0.5,
                    event.pos.y as f64 + 0.5,
                    event.pos.level as f64 + 0.4,
                ]
            };
            sprite(
                &mut sprites,
                &fx_asset,
                origin[0],
                origin[1],
                origin[2],
                if event.kind == "projectile-impact" && event.magnitude > 0. {
                    (event.magnitude * 0.55).clamp(0.5, 6.)
                } else if event.kind.starts_with("skill-") {
                    1.8
                } else if event.kind == "fire" {
                    0.3
                } else {
                    1.
                },
                0,
                "oneshot",
                age,
                [1.; 4],
            );
            if let Some(target) = event.direction {
                if sprites.len() > before && matches!(fx, "directional-beam" | "cone-shockwave") {
                    let (dx, dy) = iso(
                        (target.x - event.pos.x) as f64,
                        (target.y - event.pos.y) as f64,
                        0.,
                    );
                    sprites.last_mut().unwrap().rotation = dy.atan2(dx) as f32;
                }
            }
        }
    }
    for rubble in &s.rubble {
        if rubble.rect.level == v.layer {
            for i in 0..rubble.rect.area().min(24) {
                let x = rubble.rect.x + i % rubble.rect.width;
                let y = rubble.rect.y + i / rubble.rect.width;
                if sprite(
                    &mut sprites,
                    "floor",
                    x as f64 + 0.5,
                    y as f64 + 0.5,
                    rubble.rect.level as f64 + 0.08,
                    0.45,
                    8,
                    "idle",
                    0.,
                    [0.35, 0.31, 0.28, 1.],
                ) {
                    tag_last(
                        &mut sprites,
                        "rubble",
                        rubble.id,
                        rubble.owner,
                        rubble.rect.center(),
                    );
                }
            }
        }
    }
    for field in &s.defenseFields {
        if field.pos.level == v.layer {
            let before = sprites.len();
            sprite(
                &mut sprites,
                "fx:energy-barrier",
                field.pos.x as f64 + 0.5,
                field.pos.y as f64 + 0.5,
                field.pos.level as f64 + 0.2,
                2.,
                0,
                "oneshot",
                visual_seconds % assets::animation_duration("fx:energy-barrier","oneshot",0).unwrap_or(1.).max(1./120.),
                [1.; 4],
            );
            if sprites.len() > before {
                let (dx, dy) = iso(
                    (field.direction.x - field.pos.x) as f64,
                    (field.direction.y - field.pos.y) as f64,
                    0.,
                );
                sprites.last_mut().unwrap().rotation = dy.atan2(dx) as f32;
            }
        }
    }
    sprites.retain(|p| in_view(p.x, p.y, p.z, 3., 3.));
    objects.retain(|r| {
        in_view(
            r[0] as f64,
            r[1] as f64,
            r[2] as f64,
            r[4] as f64,
            r[5] as f64,
        )
    });
    objects.sort_by(|a, b| (a[0] + a[1] + a[2] * 0.05).total_cmp(&(b[0] + b[1] + b[2] * 0.05)));
    records.extend(objects);
    let fallbacks = s
        .units
        .iter()
        .filter(|u| {
            u.level == v.layer
                && !assets::available(&u.kind)
                && sentinels_v6::catalog::unit_ref(&u.kind)
                    .is_none_or(|d| !assets::available(&d.chassis))
        })
        .count()
        + s.buildings
            .iter()
            .filter(|b| {
                b.progress >= 1.
                    && b.kind != "shell"
                    && b.rect.level <= v.layer
                    && !assets::available(if b.kind == "core" {
                        "command-core"
                    } else {
                        &b.kind
                    })
            })
            .count()
        + s.rooms
            .iter()
            .filter(|r| r.rect.level == v.layer && r.progress >= 1. && !assets::available(&r.kind))
            .count();
    Batch {
        records,
        sprites,
        terrain,
        view: [cx as f32, cy as f32, half as f32, 16. / 9.],
        fallbacks,
    }
}
struct Stage {
    world: Snapshot,
    view: View,
    received: std::time::Instant,
    previous: BTreeMap<u64, [f64; 3]>,
    transition: f64,
}
static STAGED: OnceLock<Mutex<Option<Arc<Stage>>>> = OnceLock::new();
static PAUSED_AT: OnceLock<Mutex<Option<f64>>> = OnceLock::new();
fn visual_time(stage: &Stage) -> f64 {
    if let Some(value) = *PAUSED_AT.get_or_init(|| Mutex::new(None)).lock().unwrap() {
        return value;
    }
    if stage.world.playback.as_ref().is_some_and(|p| p.paused) && stage.world.winner.is_none() {
        return stage.world.tick as f64 / 60.;
    }
    stage.world.tick as f64 / 60. + stage.received.elapsed().as_secs_f64()
}
pub fn set_paused(paused: bool) {
    let staged = STAGED.get().and_then(|s| s.lock().unwrap().clone());
    if paused {
        if let Some(stage) = staged {
            *PAUSED_AT.get_or_init(|| Mutex::new(None)).lock().unwrap() =
                Some(stage.world.tick as f64 / 60. + stage.received.elapsed().as_secs_f64());
        }
    } else {
        let old = PAUSED_AT
            .get_or_init(|| Mutex::new(None))
            .lock()
            .unwrap()
            .take();
        if let (Some(value), Some(stage)) = (old, staged) {
            let extra = (value - stage.world.tick as f64 / 60.).max(0.);
            if let Some(storage) = STAGED.get() {
                *storage.lock().unwrap() = Some(Arc::new(Stage {
                    world: stage.world.clone(),
                    view: stage.view.clone(),
                    received: std::time::Instant::now()
                        - std::time::Duration::from_secs_f64(extra.min(3600.)),
                    previous: stage.previous.clone(),
                    transition: stage.transition,
                }));
            }
        }
    }
}
pub fn close() {
    crate::sentinels_v6_backend_metrics::clear();
    if let Some(t) = TERRAIN_BATCH.get() {
        *t.lock().unwrap() = None;
    }
    if let Some(t) = RENDER_TIMINGS.get() {
        t.lock().unwrap().clear();
    }
    assets::clear_caches();
    if let Some(s) = STAGED.get() {
        *s.lock().unwrap() = None;
    }
    if let Some(p) = PAUSED_AT.get() {
        *p.lock().unwrap() = None;
    }
    if let Some(gpu) = GPU.get() {
        *gpu.lock().unwrap() = None;
    }
}
pub fn reset_scene() {
    crate::sentinels_v6_backend_metrics::clear();
    if let Some(t) = TERRAIN_BATCH.get() {
        *t.lock().unwrap() = None;
    }
    if let Some(t) = RENDER_TIMINGS.get() {
        t.lock().unwrap().clear();
    }
    if let Some(s) = STAGED.get() {
        *s.lock().unwrap() = None;
    }
    if let Some(p) = PAUSED_AT.get() {
        *p.lock().unwrap() = None;
    }
}
pub fn scene(s: &Snapshot, v: &View) -> Scene {
    let cell = STAGED.get_or_init(|| Mutex::new(None));
    let mut guard = cell.lock().unwrap();
    let mut previous = BTreeMap::new();
    let mut transition = 0.;
    let mut received = std::time::Instant::now();
    let mut world = s.clone();
    if let Some(old) = guard.as_ref().filter(|old| {
        old.world.seed == s.seed && old.world.theme == s.theme && s.tick >= old.world.tick
    }) {
        if s.tick > old.world.tick {
            previous = old
                .world
                .units
                .iter()
                .map(|u| (u.id, [u.x, u.y, u.elevation() + 0.1]))
                .collect();
            transition = ((s.tick - old.world.tick) as f64 / 60.).clamp(1. / 60., 0.1);
        } else {
            previous = old.previous.clone();
            transition = old.transition;
            received = old.received;
        }
        // Local visual ghosts are absent from authoritative occupancy and damage.
        // Keep even when heavy combat has advanced the network event ring.
        let now = s.tick as f64 / 60. + received.elapsed().as_secs_f64();
        let existing: BTreeSet<_> = world.events.iter().map(|e| e.id).collect();
        world.events.extend(
            old.world
                .events
                .iter()
                .filter(|e| {
                    !existing.contains(&e.id)
                        && matches!(
                            e.kind.as_str(),
                            "destroy" | "collapse" | "transport-destroy"
                        )
                        || !existing.contains(&e.id) && e.kind.starts_with("unit-death:")
                })
                .filter(|e| now - e.tick as f64 / 60. < 32.)
                .cloned(),
        );
    }
    *guard = Some(Arc::new(Stage {
        world,
        view: v.clone(),
        received,
        previous,
        transition,
    }));
    Scene {
        name: "Code Sentinels V6".into(),
        entities: vec![Entity {
            id: 1,
            name: "Native V6 presentation".into(),
            transform: Transform::default(),
            components: vec![Component::new("SentinelsV6Batch", json!({"tick":s.tick}))],
        }],
        next_id: 2,
        mode: "2d".into(),
        gravity: [0.; 3],
    }
}
fn interpolate(batch: &mut Batch, stage: &Stage) {
    if stage.transition <= 0. {
        return;
    }
    let elapsed = PAUSED_AT
        .get_or_init(|| Mutex::new(None))
        .lock()
        .unwrap()
        .map(|t| (t - stage.world.tick as f64 / 60.).max(0.))
        .unwrap_or_else(|| stage.received.elapsed().as_secs_f64());
    let factor = (elapsed / stage.transition).clamp(0., 1.);
    let current: BTreeMap<u64, [f64; 3]> = stage
        .world
        .units
        .iter()
        .map(|u| (u.id, [u.x, u.y, u.elevation() + 0.1]))
        .collect();
    for s in &mut batch.sprites {
        if s.entity_kind == "unit" {
            if let (Some(old), Some(now)) = (stage.previous.get(&s.entity), current.get(&s.entity))
            {
                if (now[0] - old[0]).abs() + (now[1] - old[1]).abs() < 8. {
                    s.x += (old[0] - now[0]) * (1. - factor);
                    s.y += (old[1] - now[1]) * (1. - factor);
                    s.z += (old[2] - now[2]) * (1. - factor);
                }
            }
        }
    }
}
fn tag_last(sprites: &mut [Sprite], kind: &str, id: u64, owner: u32, pos: Pos) {
    if let Some(sprite) = sprites.last_mut() {
        sprite.entity = id;
        sprite.owner = owner;
        sprite.entity_kind = kind.into();
        sprite.entity_pos = Some(pos);
    }
}
fn sprite(
    sprites: &mut Vec<Sprite>,
    asset: &str,
    x: f64,
    y: f64,
    z: f64,
    scale: f64,
    direction: usize,
    action: &str,
    seconds: f64,
    tint: [f32; 4],
) -> bool {
    if assets::available(asset) {
        sprites.push(Sprite {
            asset: asset.into(),
            action: action.into(),
            direction,
            seconds,
            x,
            y,
            z,
            scale,
            tint,
            rotation: 0.,
            ground: false,
            entity: 0,
            owner: 0,
            entity_kind: String::new(),
            entity_pos: None,
            transient: false,
        });
        true
    } else {
        false
    }
}
// WGSL uses +Y up. Naga SPIR-V defaults already flip Y for the positive Vulkan viewport.
const VS: &str = r#"
struct Camera { view:vec4<f32>, info:vec4<f32> };
struct Item { pos:vec4<f32>, size:vec4<f32>, color:vec4<f32> };
@group(0) @binding(1) var<uniform> cam:Camera;
@group(0) @binding(0) var<storage,read> items:array<Item>;
struct Out { @builtin(position) p:vec4<f32>, @location(0) color:vec4<f32> };
@vertex fn main(@builtin(vertex_index) vi:u32,@builtin(instance_index) ii:u32)->Out {
 var o:Out; if(ii>=u32(cam.info.x)){o.p=vec4<f32>(3.,3.,0.,1.);o.color=vec4<f32>(0.);return o;}
 let a=items[ii]; let face=vi/6u;let k=vi%6u;
 let uv=array<vec2<f32>,6>(vec2<f32>(0.,0.),vec2<f32>(1.,0.),vec2<f32>(1.,1.),vec2<f32>(0.,0.),vec2<f32>(1.,1.),vec2<f32>(0.,1.));
 let t=uv[k];var p:vec3<f32>;var shade:f32;
 if(face==0u){p=vec3<f32>(t.x*a.size.x,t.y*a.size.y,a.size.z);shade=1.;}
 else if(face==1u){p=vec3<f32>(a.size.x,t.x*a.size.y,t.y*a.size.z);shade=0.65;}
 else{p=vec3<f32>(t.x*a.size.x,a.size.y,t.y*a.size.z);shade=0.82;}
 p+=a.pos.xyz;let iso=vec2<f32>((p.x-p.y)*0.5,-(p.x+p.y)*0.25+p.z*1.5);
 let clip=(iso-cam.view.xy)/vec2<f32>(cam.view.z*cam.view.w,cam.view.z);
 o.p=vec4<f32>(clip.x,clip.y,clamp(0.95-(p.x+p.y+p.z*2.)*0.003,0.01,0.99),1.);o.color=vec4<f32>(a.color.rgb*shade,a.color.a);return o;
}"#;
const FS: &str =
    r#"@fragment fn main(@location(0) color:vec4<f32>)->@location(0) vec4<f32>{return color;}"#;
struct Renderer {
    width: u32,
    height: u32,
    session: rex::DeviceFrameSession<'static>,
    device: String,
    frames: BTreeMap<String, usize>,
    used: Vec<u64>,
    alpha_bounds: Vec<AlphaBounds>,
    clock: u64,
}
unsafe impl Send for Renderer {}
static GPU: OnceLock<Mutex<Option<Renderer>>> = OnceLock::new();
const SMALL_SLOTS: usize = 1024;
const LARGE_SLOTS: usize = 512;
const FRAME_SLOTS: usize = SMALL_SLOTS + LARGE_SLOTS;
const PAGE_BYTES: usize = 128 * 1024 * 1024;
#[derive(Clone, Copy, Debug, PartialEq)]
struct AlphaBounds([f32; 2]);
impl AlphaBounds {
    const FULL: Self = Self([-1., -1.]);

    fn from_pixels(pixels: &[u8], tile_size: u32, sample: [u32; 2]) -> Self {
        // Numeric packing is exact in f32: each coordinate is <=512, so
        // x + 1024*y is <=524800 (<2^20). Never carry integer bits through
        // NaNs/subnormals. Unknown dimensions keep the previous full sampling.
        if tile_size == 0 || tile_size > 512 || sample.contains(&0)
            || sample.iter().any(|&side| side > tile_size)
            || pixels.len() != tile_size as usize * tile_size as usize * 4
        {
            return Self::FULL;
        }
        let mut lo = sample;
        let mut hi = [0, 0];
        for y in 0..sample[1] {
            for x in 0..sample[0] {
                if pixels[((y * tile_size + x) * 4 + 3) as usize] != 0 {
                    lo[0] = lo[0].min(x);
                    lo[1] = lo[1].min(y);
                    hi[0] = hi[0].max(x + 1);
                    hi[1] = hi[1].max(y + 1);
                }
            }
        }
        if hi == [0, 0] {
            return Self([0., 0.]);
        }
        // Include alpha=1 and two neighbouring texels around every nonzero
        // edge. The shader currently samples nearest; this conservative guard
        // also leaves room for the existing UV edge/clamp precision.
        for axis in 0..2 {
            lo[axis] = lo[axis].saturating_sub(2);
            hi[axis] = (hi[axis] + 2).min(sample[axis]);
        }
        Self([(lo[0] + 1024 * lo[1]) as f32, (hi[0] + 1024 * hi[1]) as f32])
    }
}
fn cache_location(slot: usize) -> (usize, usize) {
    if slot < SMALL_SLOTS {
        (slot / 512, (slot % 512) * 256 * 256 * 4)
    } else {
        let large = slot - SMALL_SLOTS;
        (2 + large / 128, (large % 128) * 512 * 512 * 4)
    }
}
const SPRITE_SHADER: &str = r#"
const SPRITE_CLASS:u32=0u;
struct Camera { view:vec4<f32>, info:vec4<f32> };
struct Item { pos:vec4<f32>, size:vec4<f32>, color:vec4<f32>, extent:vec4<f32> };
@group(0) @binding(0) var<storage,read> items:array<Item>;
@group(0) @binding(1) var<storage,read> pixels0:array<u32>;
@group(0) @binding(2) var<storage,read> pixels1:array<u32>;
@group(0) @binding(3) var<storage,read> pixels2:array<u32>;
@group(0) @binding(4) var<storage,read> pixels3:array<u32>;
@group(0) @binding(5) var<storage,read> pixels4:array<u32>;
@group(0) @binding(6) var<storage,read> pixels5:array<u32>;
@group(0) @binding(7) var<uniform> cam:Camera;
struct Out { @builtin(position) p:vec4<f32>, @location(0) uv:vec2<f32>, @location(1) @interpolate(flat) slot:u32, @location(2) color:vec4<f32>, @location(3) @interpolate(flat) shape:vec2<u32>, @location(4) depth_map:vec2<f32>, @location(5) @interpolate(flat) bounds:vec4<u32> };
@vertex fn vertex_main(@builtin(vertex_index) vi:u32,@builtin(instance_index) ii:u32)->Out{
 var o:Out;o.p=vec4<f32>(3.,3.,0.,1.);o.uv=vec2<f32>(0.);o.slot=0u;o.color=vec4<f32>(0.);o.shape=vec2<u32>(256u);o.depth_map=vec2<f32>(0.);o.bounds=vec4<u32>(0u);
 if(ii>=u32(cam.info.y)){return o;}
 let a=items[ii];if(u32(a.pos.w)/4096u!=SPRITE_CLASS){return o;}
 let points=array<vec2<f32>,6>(vec2<f32>(0.,0.),vec2<f32>(1.,0.),vec2<f32>(1.,1.),vec2<f32>(0.,0.),vec2<f32>(1.,1.),vec2<f32>(0.,1.));let uv=points[vi];
 let is_ground=u32(a.pos.w)%4096u>=2048u;var logical=a.pos.xyz;if(is_ground){logical+=vec3<f32>((uv.x-0.5)*a.size.x,(uv.y-0.5)*a.size.x,0.);}
 let foot=vec2<f32>((logical.x-logical.y)*0.5,-(logical.x+logical.y)*0.25+logical.z*1.5);let delta=vec2<f32>((uv.x-a.size.y)*a.size.x,(a.size.z-uv.y)*a.size.x*a.extent.y/a.extent.x);let turn=a.size.w;
 let rotated=select(vec2<f32>(delta.x*cos(turn)-delta.y*sin(turn),delta.x*sin(turn)+delta.y*cos(turn)),vec2<f32>(0.),is_ground);let pt=foot+rotated;let clip=(pt-cam.view.xy)/vec2<f32>(cam.view.z*cam.view.w,cam.view.z);
 let depth_key=logical.x+logical.y+logical.z*2.+select(0.25,0.,is_ground);
 o.p=vec4<f32>(clip.x,clip.y,clamp(0.95-depth_key*0.003,0.01,0.99),1.);o.uv=uv;o.slot=u32(a.pos.w)%2048u;o.color=a.color;o.shape=vec2<u32>(a.extent.xy);o.depth_map=vec2<f32>(depth_key,rotated.y);
 o.bounds=vec4<u32>(vec2<u32>(0u),o.shape);
 if(a.extent.z>=0. && a.extent.w>=0.){let lo=u32(a.extent.z);let hi=u32(a.extent.w);o.bounds=vec4<u32>(lo&1023u,lo>>10u,hi&1023u,hi>>10u);}
 return o;
}
struct FragmentOut { @location(0) color:vec4<f32>, @builtin(frag_depth) depth:f32 };
@fragment fn fragment_main(@location(0) uv:vec2<f32>,@location(1) @interpolate(flat) slot:u32,@location(2) tint:vec4<f32>,@location(3) @interpolate(flat) shape:vec2<u32>,@location(4) depth_map:vec2<f32>,@location(5) @interpolate(flat) bounds:vec4<u32>)->FragmentOut{
 let p=vec2<u32>(clamp(uv,vec2<f32>(0.),vec2<f32>(0.99999))*vec2<f32>(shape));
 if(any(p<bounds.xy)||any(p>=bounds.zw)){discard;}
 var page=slot/512u;var layer=slot%512u;var side=256u;
 if(slot>=1024u){page=2u+(slot-1024u)/128u;layer=(slot-1024u)%128u;side=512u;}
 let offset=layer*side*side+p.y*side+p.x;var rgba:u32;
 switch(page){case 0u:{rgba=pixels0[offset];}case 1u:{rgba=pixels1[offset];}case 2u:{rgba=pixels2[offset];}case 3u:{rgba=pixels3[offset];}case 4u:{rgba=pixels4[offset];}default:{rgba=pixels5[offset];}}
 let c=unpack4x8unorm(rgba)*tint;if(c.a<0.01){discard;}
 // A below-foot texel belongs to a closer point of the same support plane.
 // Keep the authored anchor and depth-test against real walls/floors; do not
 // lift the whole billboard or turn off occlusion to hide corpse cropping.
 var result:FragmentOut;result.color=c;result.depth=clamp(0.95-(depth_map.x+max(0.,-4.*depth_map.y))*0.003,0.01,0.99);return result;
}
"#;
fn build(width: u32, height: u32) -> Result<Renderer, String> {
    let caps = rex::probe_device_caps().map_err(|e| e.to_string())?;
    let vs = crate::viewport::compile_wgsl(VS, "v6-vs")?;
    let fs = crate::viewport::compile_wgsl(FS, "v6-fs")?;
    let sprite_shaders: Vec<_> = (0..3)
        .map(|class| {
            let source =
                SPRITE_SHADER.replace("SPRITE_CLASS:u32=0u", &format!("SPRITE_CLASS:u32={class}u"));
            Ok((
                crate::viewport::compile_wgsl(
                    &source.replace("vertex_main", "main"),
                    "v6-textured-vs",
                )?,
                crate::viewport::compile_wgsl(
                    &source.replace("fragment_main", "main"),
                    "v6-textured-fs",
                )?,
            ))
        })
        .collect::<Result<_, String>>()?;
    let mut resource_list = vec![
        rex::ResourceDesc::Buffer(rex::BufferDesc {
            size: 32,
            usage: rex::BufferUsage {
                uniform: true,
                ..Default::default()
            },
            data: None,
            device_local: false,
        }),
        rex::ResourceDesc::Buffer(rex::BufferDesc {
            size: (MAX * 48) as u64,
            usage: rex::BufferUsage {
                storage: true,
                ..Default::default()
            },
            data: None,
            device_local: false,
        }),
        rex::ResourceDesc::Texture(rex::TextureDesc {
            width,
            height,
            format: rex::TexFormat::Rgba8Unorm,
            usage: rex::TextureUsage {
                color: true,
                ..Default::default()
            },
            data: None,
        }),
        rex::ResourceDesc::Texture(rex::TextureDesc {
            width,
            height,
            format: rex::TexFormat::Depth32Float,
            usage: rex::TextureUsage {
                depth: true,
                ..Default::default()
            },
            data: None,
        }),
        rex::ResourceDesc::Buffer(rex::BufferDesc {
            size: (MAX * 64) as u64,
            usage: rex::BufferUsage {
                storage: true,
                ..Default::default()
            },
            data: None,
            device_local: false,
        }),
        rex::ResourceDesc::Buffer(rex::BufferDesc {
            size: PAGE_BYTES as u64,
            usage: rex::BufferUsage {
                storage: true,
                ..Default::default()
            },
            data: None,
            device_local: false,
        }),
    ];
    for _ in 1..6 {
        resource_list.push(rex::ResourceDesc::Buffer(rex::BufferDesc {
            size: PAGE_BYTES as u64,
            usage: rex::BufferUsage {
                storage: true,
                ..Default::default()
            },
            data: None,
            device_local: false,
        }));
    }
    let resources = Box::leak(resource_list.into_boxed_slice());
    let mut pass_list = vec![rex::Pass::Raster(rex::RasterPass {
        name: "sentinels_v6_instanced_isometric",
        blend: rex::BlendMode::Opaque,
        vs_spirv: vs,
        fs_spirv: fs,
        vertex: rex::VertexData::Pull,
        draw: rex::DrawSpec::Direct {
            vertex_count: 18,
            instance_count: MAX as u32,
            first_vertex: 0,
            first_instance: 0,
        },
        colors: vec![rex::ColorAttachmentRef {
            res: 2,
            clear: Some([0.025, 0.04, 0.06, 1.]),
        }],
        depth: Some(rex::DepthAttachmentRef {
            res: 3,
            clear: Some(1.),
        }),
        viewport: None,
        bindings: rex::Bindings {
            uniform: Some(rex::UniformRef {
                res: 0,
                offset: 0,
                size: 32,
            }),
            storage_buffers: vec![1],
            ..Default::default()
        },
        conservative: None,
    })];
    for (class, (vs, fs)) in sprite_shaders.into_iter().enumerate() {
        pass_list.push(rex::Pass::Raster(rex::RasterPass {
            name: match class {
                0 => "sentinels_v6_real_frame_assets",
                1 => "sentinels_v6_alpha_effects",
                _ => "sentinels_v6_additive_effects",
            },
            blend: match class {
                0 => rex::BlendMode::AlphaDepth,
                1 => rex::BlendMode::Alpha,
                _ => rex::BlendMode::Additive,
            },
            vs_spirv: vs,
            fs_spirv: fs,
            vertex: rex::VertexData::Pull,
            draw: rex::DrawSpec::Direct {
                vertex_count: 6,
                instance_count: MAX as u32,
                first_vertex: 0,
                first_instance: 0,
            },
            colors: vec![rex::ColorAttachmentRef {
                res: 2,
                clear: None,
            }],
            depth: Some(rex::DepthAttachmentRef {
                res: 3,
                clear: None,
            }),
            viewport: None,
            bindings: rex::Bindings {
                uniform: Some(rex::UniformRef {
                    res: 0,
                    offset: 0,
                    size: 32,
                }),
                storage_buffers: vec![4, 5, 6, 7, 8, 9, 10],
                ..Default::default()
            },
            conservative: None,
        }));
    }
    let passes = Box::leak(pass_list.into_boxed_slice());
    let mut barriers: Vec<&'static [(u32, rex::TargetState)]> = vec![Box::leak(
        vec![
            (0, rex::TargetState::UniformRead),
            (1, rex::TargetState::StorageReadWrite),
            (2, rex::TargetState::ColorAttachmentWrite),
            (3, rex::TargetState::DepthAttachmentWrite),
        ]
        .into_boxed_slice(),
    )];
    for _ in 0..3 {
        barriers.push(Box::leak(
            vec![
                (0, rex::TargetState::UniformRead),
                (4, rex::TargetState::StorageReadWrite),
                (5, rex::TargetState::StorageReadWrite),
                (6, rex::TargetState::StorageReadWrite),
                (7, rex::TargetState::StorageReadWrite),
                (8, rex::TargetState::StorageReadWrite),
                (9, rex::TargetState::StorageReadWrite),
                (10, rex::TargetState::StorageReadWrite),
                (2, rex::TargetState::ColorAttachmentWrite),
                (3, rex::TargetState::DepthAttachmentWrite),
            ]
            .into_boxed_slice(),
        ));
    }
    let session = rex::DeviceFrameSession::new(
        resources,
        passes,
        Box::leak(barriers.into_boxed_slice()),
        Box::leak(vec![rex::Readback::Texture { res: 2 }].into_boxed_slice()),
        2,
    )
    .map_err(|e| e.to_string())?;
    Ok(Renderer {
        width,
        height,
        session,
        device: caps.device_name,
        frames: BTreeMap::new(),
        used: vec![0; FRAME_SLOTS],
        alpha_bounds: vec![AlphaBounds::FULL; FRAME_SLOTS],
        clock: 0,
    })
}
fn upload_sprites(
    r: &mut Renderer,
    requests: &[Sprite],
    ground: &[Sprite],
    update: &mut rex::FrameUpdate,
) -> Result<usize, String> {
    r.clock += 1;
    let mut descriptors: HashMap<(&str, &str, usize, u64), Option<Arc<assets::Frame>>> =
        HashMap::new();
    let mut resolved: Vec<(&Sprite, Arc<assets::Frame>)> =
        Vec::with_capacity(requests.len() + ground.len());
    for sprite in ground.iter().chain(requests) {
        let key = (
            sprite.asset.as_str(),
            sprite.action.as_str(),
            sprite.direction,
            (sprite.seconds * 64.).max(0.) as u64,
        );
        if !descriptors.contains_key(&key) {
            descriptors.insert(key, assets::describe(sprite)?.map(Arc::new));
        }
        if let Some(frame) = descriptors[&key].clone() {
            resolved.push((sprite, frame));
        }
    }
    if resolved.len() > MAX {
        return Err("native sprite instance budget exceeded".into());
    }
    let class = |s: &Sprite, f: &assets::Frame| {
        if f.effect || s.transient {
            if f.additive {
                2u32
            } else {
                1u32
            }
        } else {
            0u32
        }
    };
    resolved.sort_by(|(a, af), (b, bf)| {
        class(a, af)
            .cmp(&class(b, bf))
            .then_with(|| b.ground.cmp(&a.ground))
            .then_with(|| (a.x + a.y + a.z * 2.).total_cmp(&(b.x + b.y + b.z * 2.)))
    });
    let needed: BTreeSet<&str> = resolved.iter().map(|(_, f)| f.key.as_str()).collect();
    let small: BTreeSet<_> = resolved
        .iter()
        .filter(|(_, f)| f.tile_size == 256)
        .map(|(_, f)| f.key.as_str())
        .collect();
    let large: BTreeSet<_> = resolved
        .iter()
        .filter(|(_, f)| f.tile_size == 512)
        .map(|(_, f)| f.key.as_str())
        .collect();
    if small.len() > SMALL_SLOTS || large.len() > LARGE_SLOTS {
        return Err(format!(
            "native frame banks exceed budget: {} normal, {} full-density",
            small.len(),
            large.len()
        ));
    }
    if needed.len() > FRAME_SLOTS {
        return Err(format!(
            "{} simultaneous unique native frames exceed cache budget",
            needed.len()
        ));
    }
    let mut buffer = Vec::with_capacity(resolved.len() * 64);
    let mut reserved: BTreeSet<usize> = r
        .frames
        .iter()
        .filter(|(key, _)| needed.contains(key.as_str()))
        .map(|(_, slot)| *slot)
        .collect();
    for (s, f) in &resolved {
        let slot = if let Some(slot) = r.frames.get(&f.key) {
            *slot
        } else {
            let range = if f.tile_size == 512 {
                SMALL_SLOTS..FRAME_SLOTS
            } else {
                0..SMALL_SLOTS
            };
            let free = range
                .filter(|slot| !reserved.contains(slot))
                .min_by_key(|slot| r.used[*slot])
                .ok_or("native frame cache exhausted")?;
            r.frames.retain(|_, slot| *slot != free);
            let pixels = assets::pixels(f)?;
            // Slot reuse must replace the previous frame's bounds at exactly
            // the same time as its pixel upload (including fully empty pages).
            r.alpha_bounds[free] = AlphaBounds::from_pixels(&pixels, f.tile_size, f.sample_size);
            let (page, offset) = cache_location(free);
            update.buffer_uploads.push((
                rex::StableResourceId(6 + page as u64),
                offset as u64,
                pixels,
            ));
            r.frames.insert(f.key.clone(), free);
            reserved.insert(free);
            free
        };
        r.used[slot] = r.clock;
        for value in [
            s.x as f32,
            s.y as f32,
            s.z as f32,
            slot as f32 + if s.ground { 2048. } else { 0. } + class(s, f) as f32 * 4096.,
            if s.ground {
                s.scale as f32
            } else {
                f.span * s.scale as f32
            },
            f.pivot[0],
            f.pivot[1],
            s.rotation,
            s.tint[0],
            s.tint[1],
            s.tint[2],
            s.tint[3],
            f.sample_size[0] as f32,
            f.sample_size[1] as f32,
            r.alpha_bounds[slot].0[0],
            r.alpha_bounds[slot].0[1],
        ] {
            buffer.extend_from_slice(&value.to_le_bytes());
        }
    }
    if !buffer.is_empty() {
        update
            .buffer_uploads
            .push((rex::StableResourceId(5), 0, buffer));
    }
    Ok(resolved.len())
}
pub fn render(
    _scene: &Scene,
    width: u32,
    height: u32,
    want_readback: bool,
    want_stats: bool,
) -> Result<crate::viewport::FramePixels, String> {
    let total_start = std::time::Instant::now();
    let staged = STAGED
        .get()
        .ok_or("no V6 presentation state")?
        .lock()
        .unwrap()
        .clone()
        .ok_or("no V6 world")?;
    let visual_seconds = visual_time(&staged);
    let mut scene = compose(&staged.world, &staged.view, visual_seconds);
    interpolate(&mut scene, &staged);
    let compose_ms = total_start.elapsed().as_secs_f64() * 1000.;
    let prepare_start = std::time::Instant::now();
    let records = &scene.records;
    if records.len() > MAX {
        return Err(format!("V6 geometry capacity exceeded: {}", records.len()));
    }
    let mut data = Vec::with_capacity(records.len() * 48);
    for record in records {
        for value in record {
            data.extend_from_slice(&value.to_le_bytes());
        }
    }
    let mut camera = Vec::with_capacity(32);
    for (i, value) in scene.view.iter().enumerate() {
        camera.extend_from_slice(
            &(if i == 3 {
                width as f32 / height as f32
            } else {
                *value
            })
            .to_le_bytes(),
        );
    }
    for value in [records.len() as f32, 0., 0., 0.] {
        camera.extend_from_slice(&value.to_le_bytes());
    }
    let mut guard = GPU.get_or_init(|| Mutex::new(None)).lock().unwrap();
    if guard.as_ref().map(|r| (r.width, r.height)) != Some((width, height)) {
        *guard = Some(build(width, height)?);
    }
    let r = guard.as_mut().unwrap();
    let mut update = rex::FrameUpdate::default();
    let sprite_count = match upload_sprites(r, &scene.sprites, &scene.terrain.sprites, &mut update)
    {
        Ok(n) => n,
        Err(e) => {
            r.frames.clear();
            r.used.fill(0);
            return Err(e);
        }
    };
    camera[20..24].copy_from_slice(&(sprite_count as f32).to_le_bytes());
    update
        .buffer_uploads
        .push((rex::StableResourceId(1), 0, camera));
    if !data.is_empty() {
        update
            .buffer_uploads
            .push((rex::StableResourceId(2), 0, data));
    }
    update.readback_subset = if want_readback { Some(vec![0]) } else { None };
    let prepare_ms = prepare_start.elapsed().as_secs_f64() * 1000.;
    let execute_start = std::time::Instant::now();
    let provenance = match r.session.next_provenance_with_update(&update) {
        Ok(p) => p,
        Err(e) => {
            r.frames.clear();
            r.used.fill(0);
            return Err(e.to_string());
        }
    };
    let provenance_ns=execute_start.elapsed().as_secs_f64()*1e9;
    let execute_call_start=std::time::Instant::now();
    let result = match r.session.execute_with_frame_update(&provenance, &update) {
        Ok(value) => value,
        Err(e) => {
            r.frames.clear();
            r.used.fill(0);
            return Err(e.to_string());
        }
    };
    let execute_call_ns=execute_call_start.elapsed().as_secs_f64()*1e9;
    let execute_ms = execute_start.elapsed().as_secs_f64() * 1000.;
    crate::sentinels_v6_backend_metrics::record(staged.view.layer,provenance_ns,execute_call_ns,&result.telemetry);
    let stats_start = std::time::Instant::now();
    let rgba8 = if want_readback {
        result
            .readbacks
            .into_iter()
            .next()
            .ok_or("missing native image")?
    } else {
        vec![]
    };
    let nonzero = if want_stats {
        rgba8
            .chunks_exact(4)
            .filter(|p| p[0] > 20 || p[1] > 20 || p[2] > 25)
            .count()
    } else {
        0
    };
    let mesh_classes = scene
        .terrain.sprites
        .iter().chain(&scene.sprites)
        .map(|s| s.asset.as_str())
        .collect::<BTreeSet<_>>()
        .len();
    record_timing(
        staged.view.layer,
        [
            compose_ms,
            prepare_ms,
            execute_ms,
            stats_start.elapsed().as_secs_f64() * 1000.,
            total_start.elapsed().as_secs_f64() * 1000.,
        ],
    );
    Ok(crate::viewport::FramePixels {
        width,
        height,
        rgba8,
        device_name: r.device.clone(),
        draws: 4,
        truncated: false,
        nonzero,
        triangles: records.len() * 6 + sprite_count * 2,
        mesh_fallbacks: scene.fallbacks,
        mesh_classes,
        imported: sprite_count > 0,
    })
}

/// Read-only screen hit test against the same visible frame, alpha, pivot and
/// depth convention as the native raster pass. Construction still uses planes.
pub fn pick(params: &serde_json::Value) -> Result<serde_json::Value, String> {
    let staged = STAGED
        .get()
        .ok_or("V6 presentation is not ready")?
        .lock()
        .unwrap()
        .clone()
        .ok_or("V6 presentation is not ready")?;
    let view: View = if params["view"].is_object() {
        serde_json::from_value(params["view"].clone()).map_err(|e| e.to_string())?
    } else {
        staged.view.clone()
    };
    let width = params["width"].as_f64().unwrap_or(1280.);
    let height = params["height"].as_f64().unwrap_or(720.);
    let x = params["screenX"].as_f64().ok_or("screenX missing")?;
    let y = params["screenY"].as_f64().ok_or("screenY missing")?;
    if !width.is_finite()
        || !height.is_finite()
        || width <= 0.
        || height <= 0.
        || width > 32768.
        || height > 32768.
        || !x.is_finite()
        || !y.is_finite()
        || !(0.4..=4.).contains(&view.zoom)
        || !view.center_x.is_finite()
        || !view.center_y.is_finite()
        || !(-2..=5).contains(&view.layer)
        || !(1..=2).contains(&view.local_player)
    {
        return Err("invalid screen coordinates".into());
    }
    if x < 0. || y < 0. || x > width || y > height {
        return Ok(serde_json::Value::Null);
    }
    let s = &staged.world;
    let now = visual_time(&staged);
    let mut batch = compose(s, &view, now);
    interpolate(&mut batch, &staged);
    let (cx, cy) = iso(view.center_x, view.center_y, view.layer as f64);
    let half = 24. / view.zoom;
    let world_x = cx + (x / width * 2. - 1.) * half * (width / height);
    let world_y = cy + (1. - y / height * 2.) * half;
    let mut hits: Vec<(f64, &str, u64, u32, Pos)> = Vec::new();
    for sprite in &batch.sprites {
        if sprite.ground {
            continue;
        }
        let identity = if sprite.entity > 0 {
            Some((
                sprite.entity_kind.as_str(),
                sprite.entity,
                sprite.owner,
                sprite.entity_pos.unwrap_or_default(),
            ))
        } else {
            None
        };
        let Some((kind, id, owner, pos)) = identity else {
            continue;
        };
        let Some(frame) = assets::describe(sprite)? else {
            continue;
        };
        let (fx, fy) = iso(sprite.x, sprite.y, sprite.z);
        let span = frame.span as f64 * sprite.scale;
        let dx = world_x - fx;
        let dy = world_y - fy;
        let angle = sprite.rotation as f64;
        let tx = dx * angle.cos() + dy * angle.sin();
        let ty = -dx * angle.sin() + dy * angle.cos();
        let u = tx / span + frame.pivot[0] as f64;
        let v = frame.pivot[1] as f64
            - ty / (span * frame.sample_size[1] as f64 / frame.sample_size[0] as f64);
        if !(0.0..1.0).contains(&u) || !(0.0..1.0).contains(&v) {
            continue;
        }
        let pixels = assets::pixels(&frame)?;
        let ix = (u * frame.sample_size[0] as f64) as usize;
        let iy = (v * frame.sample_size[1] as f64) as usize;
        if pixels[(iy * frame.tile_size as usize + ix) * 4 + 3] < 20 {
            continue;
        }
        hits.push((
            sprite_depth_key(sprite.x,sprite.y,sprite.z,dy),
            kind,
            id,
            owner,
            pos,
        ));
    }
    // Empty and unfinished rooms still have a visible floor. Use its actual
    // raster geometry instead of relying on a rack sprite or shell footprint.
    for room in &s.rooms {
        if room.rect.level != view.layer { continue; }
        if let Some(depth) = record_pick_depth(&room_floor(room.rect, [1.; 4]), (world_x, world_y)) {
            hits.push((depth, "room", room.id, room.owner, room.rect.center()));
        }
    }
    // Shell floor selection shares its drawn slab geometry, so the raised room
    // floor wins where visible; front walls and uncut roofs retain their depth.
    for b in &s.buildings {
        if b.rect.level != view.layer || b.progress >= 1. && b.kind != "shell" {
            continue;
        }
        let z = b.rect.level as f64;
        if b.kind == "shell" {
            let r = b.rect;
            if let Some(depth) = record_pick_depth(&record(r.x as f64, r.y as f64, z,
                r.width as f64, r.height as f64, 0.12, [1.; 4]), (world_x, world_y)) {
                hits.push((depth, "building", b.id, b.owner, r.center()));
            }
            continue;
        }
        let py = world_y - z * 1.5;
        let gx = world_x - 2. * py;
        let gy = -world_x - 2. * py;
        if gx >= b.rect.x as f64
            && gx < (b.rect.x + b.rect.width) as f64
            && gy >= b.rect.y as f64
            && gy < (b.rect.y + b.rect.height) as f64
        {
            hits.push((gx + gy + z * 2., "building", b.id, b.owner, b.rect.center()));
        }
    }
    if hits.is_empty() {
        for link in &s.links {
            for pos in &link.path {
                if pos.level != view.layer {
                    continue;
                }
                let (px, py) = iso(
                    pos.x as f64 + 0.5,
                    pos.y as f64 + 0.5,
                    pos.level as f64 + 0.15,
                );
                if ((px - world_x).powi(2) + (py - world_y).powi(2)).sqrt() < half * 10. / height {
                    hits.push((px + py, "link", link.id, link.owner, *pos));
                }
            }
        }
    }
    if let Some((_, kind, id, owner, pos)) = hits.into_iter().max_by(|a, b| a.0.total_cmp(&b.0)) {
        Ok(json!({"kind":kind,"id":id,"owner":owner,"pos":pos}))
    } else {
        Ok(serde_json::Value::Null)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn empty_room_floor_pick_matches_visible_geometry_on_every_layer() {
        for level in -2..=5 {
            let room = sentinels_v6::Rect { x:15, y:46, width:2, height:2, level };
            let floor = room_floor(room, [1.;4]);
            let shell = record(13.,46.,level as f64,6.,4.,0.12,[1.;4]);
            let center = iso(16.,47.,level as f64+0.21);
            let floor_depth = record_pick_depth(&floor,center).expect("empty room top is pickable");
            assert!(floor_depth > record_pick_depth(&shell,center).unwrap());
            let roof = record(13.,46.,level as f64+0.9,6.,4.,0.08,[1.;4]);
            assert!(record_pick_depth(&roof,center).unwrap() > floor_depth,
                "a solid roof must occlude an interior, not be overridden by room priority");
            let side = iso(16.88,47.,level as f64+0.17);
            assert!(record_pick_depth(&floor,side).is_some(),"visible front side is pickable");
            assert!(record_pick_depth(&floor,iso(20.,50.,level as f64+0.21)).is_none());
        }
    }
    #[test]
    fn frame_bank_offsets_never_cross_their_storage_allocation() {
        for slot in 0..FRAME_SLOTS {
            let (page, offset) = cache_location(slot);
            let bytes = if slot < SMALL_SLOTS {
                256 * 256 * 4
            } else {
                512 * 512 * 4
            };
            assert!(page < 6);
            assert!(offset + bytes <= PAGE_BYTES);
        }
        assert_eq!(cache_location(SMALL_SLOTS), (2, 0));
        assert_eq!(cache_location(512), (1, 0));
    }
    #[test]
    fn alpha_bounds_numeric_transport_is_exact_for_every_supported_coordinate() {
        for y in 0..=512u32 {
            for x in 0..=512u32 {
                let encoded = (x + 1024 * y) as f32;
                assert!(encoded.is_finite() && (encoded == 0. || encoded.is_normal()));
                let decoded = encoded as u32;
                assert_eq!((decoded & 1023, decoded >> 10), (x, y));
            }
        }
    }
    #[test]
    fn alpha_bounds_never_reject_nonzero_alpha_or_the_two_texel_guard() {
        let side = 512u32;
        let sample = [350, 264];
        let mut pixels = vec![0u8; side as usize * side as usize * 4];
        // Transparent RGB is intentionally nonzero. Padding outside sampleSize
        // is also nonzero-alpha: neither may change the visible sampling bounds.
        for pixel in pixels.chunks_exact_mut(4) { pixel[..3].copy_from_slice(&[251, 17, 83]); }
        for (x, y, alpha) in [(5u32, 7u32, 1u8), (347, 260, 255), (511, 511, 255)] {
            pixels[((y * side + x) * 4 + 3) as usize] = alpha;
        }
        let bounds = AlphaBounds::from_pixels(&pixels, side, sample).0;
        let lo = bounds[0] as u32;
        let hi = bounds[1] as u32;
        let rect = [lo & 1023, lo >> 10, hi & 1023, hi >> 10];
        assert_eq!(rect, [3, 5, 350, 263]);
        for y in 0..sample[1] {
            for x in 0..sample[0] {
                let rejected = x < rect[0] || y < rect[1] || x >= rect[2] || y >= rect[3];
                if rejected { assert_eq!(pixels[((y * side + x) * 4 + 3) as usize], 0); }
            }
        }
        // Simulate reusing the same slot for empty and boundary-touching frames;
        // its published bounds come from each new page, never the old crop.
        pixels.fill(0);
        assert_eq!(AlphaBounds::from_pixels(&pixels, side, sample).0, [0., 0.]);
        pixels[3] = 1;
        pixels[(((sample[1] - 1) * side + sample[0] - 1) * 4 + 3) as usize] = 1;
        assert_eq!(AlphaBounds::from_pixels(&pixels, side, sample).0, [0., (350 + 1024 * 264) as f32]);
    }
    #[test]
    fn alpha_bounds_unsupported_or_incomplete_pages_keep_full_sampling() {
        assert_eq!(AlphaBounds::from_pixels(&[], 512, [350, 264]), AlphaBounds::FULL);
        assert_eq!(AlphaBounds::from_pixels(&[], 1024, [1024, 1024]), AlphaBounds::FULL);
        assert_eq!(AlphaBounds::from_pixels(&[0; 64], 4, [5, 4]), AlphaBounds::FULL);
        assert_eq!(AlphaBounds::from_pixels(&[0; 64], 4, [0, 4]), AlphaBounds::FULL);
    }
    #[test]
    fn shaders_compile() {
        crate::viewport::compile_wgsl(VS, "v6vs").unwrap();
        crate::viewport::compile_wgsl(FS, "v6fs").unwrap();
        crate::viewport::compile_wgsl(SPRITE_SHADER, "v6sprite").unwrap();
    }
    #[test]
    fn below_foot_depth_clears_its_support_plane_but_not_a_nearer_solid_surface(){
        let(x,y,z)=(40.,50.,0.1);let(_,foot_y)=iso(x,y,z);let anchor=sprite_depth_key(x,y,z,0.);
        for delta in [-3.,-1.,-0.25,-0.01,0.]{let key=sprite_depth_key(x,y,z,delta);let at_y=foot_y+delta;
            let ground_sum=(0.03*1.5-at_y)*4.;assert!(key>ground_sum+2.*0.03);
            let wall_height=0.8;let wall_sum=(wall_height*1.5-at_y)*4.;assert!(key<wall_sum+2.*wall_height,"solid foreground wall must retain depth priority");}
        assert_eq!(sprite_depth_key(x,y,z,2.),anchor,"above-foot billboard depth remains anchored");
    }
    #[test]
    fn scene_uses_native_instancing_and_shared_projection() {
        let s = sentinels_v6::Game::new(1, false).state;
        let sc = compose(&s, &View::default(), 0.);
        assert!(sc.records.len() > 100);
        assert_eq!(iso(2., 0., 0.), (1., -0.5));
    }
}
