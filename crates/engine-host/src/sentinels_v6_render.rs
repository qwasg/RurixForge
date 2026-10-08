//! Instanced Rurix/Vulkan isometric geometry. One native raster pass draws all
//! visible modular terrain, floors, walls and equipment; no per-cell draw slots.
use crate::sentinels_v6::{iso, View};
use crate::sentinels_v6_assets::{self as assets, Sprite};
use forge_scene::{Component, Entity, Scene, Transform};
#[cfg(feature = "backend-rurix")]
use rurix_rt::render_exec as rex;
use sentinels_v6::{Pos, Snapshot};
use serde_json::json;
use std::collections::{BTreeMap, BTreeSet, HashMap, VecDeque};
use std::sync::{Arc, Mutex, OnceLock};
// rurix 部分(V6G:原生实例化光栅会话、WGSL、贴图页上传、出帧)在 sentinels_v6_render/rurix.rs(02 §5.2),
// 经 `use super::*` 共用本模块的导入与 CPU 状态(STAGED / compose / visual_time)。
#[cfg(feature = "backend-rurix")]
mod rurix;
#[cfg(feature = "backend-rurix")]
pub use rurix::render;
#[cfg(feature = "backend-rurix")]
pub(crate) use rurix::FRAME_SLOTS;
#[cfg_attr(not(feature = "backend-rurix"), allow(dead_code))]
const MAX: usize = 32768;
type RenderTiming = [VecDeque<f64>; 5];
static RENDER_TIMINGS: OnceLock<Mutex<BTreeMap<i32, RenderTiming>>> = OnceLock::new();
#[cfg_attr(not(feature = "backend-rurix"), allow(dead_code))]
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
#[cfg_attr(not(feature = "backend-rurix"), allow(dead_code))]
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
#[cfg_attr(not(feature = "backend-rurix"), allow(dead_code))]
struct TerrainBatch {
    camera: [u64; 3],
    local_player: u32,
    terrain: Vec<u8>,
    visible: BTreeSet<Pos>,
    explored: BTreeSet<Pos>,
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
        if old.camera == camera
            && old.local_player == v.local_player
            && old.terrain == s.terrain
            && old.visible == *visible
            && old.explored == *explored
            && old.at.elapsed().as_secs_f64() < 2.
        {
            return old.clone();
        }
    }
    let half = 24. / v.zoom;
    let (cx, cy) = iso(v.center_x, v.center_y, 0.);
    let in_view = |x: f64, y: f64, z: f64, w: f64, h: f64| {
        let (px, py) = iso(x, y, z);
        (px - cx).abs() < half * 16. / 9. + w + h + 3. && (py - cy).abs() < half + w + h + 3.
    };
    let mut sprites = Vec::new();
    let mut records = Vec::new();
    for y in 0..96 {
        for x in 0..128 {
            if !in_view(x as f64, y as f64, 0., 1., 1.) {
                continue;
            }
            let p = Pos::new(x, y, 0);
            let t = s.terrain[(y * 128 + x) as usize];
            let mut c = match t {
                1 => [0.32, 0.34, 0.37, 1.],
                2 => [0.08, 0.24, 0.34, 1.],
                3 => [0.42, 0.39, 0.3, 1.],
                4 => [0.27, 0.38, 0.28, 1.],
                5 => [0.26, 0.56, 0.62, 1.],
                6 => [0.18, 0.19, 0.22, 1.],
                _ => [0.20, 0.30, 0.23, 1.],
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
            let terrain_key = match t {
                1 => "rock",
                2 => "water",
                3 => "road",
                4 => "highland",
                5 => "ore",
                6 => "coal",
                _ => "grass",
            };
            let before = sprites.len();
            let terrain_tint = assets::terrain_tint(terrain_key);
            if sprite(
                &mut sprites,
                &format!("terrain:{terrain_key}"),
                x as f64 + 0.5,
                y as f64 + 0.5,
                if t == 4 { 0.12 } else { 0.03 },
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
                0.,
                1.,
                1.,
                if t == 4 { 0.11 } else { 0.02 },
                c,
            ));
        }
    }
    let result = Arc::new(TerrainBatch {
        camera,
        local_player: v.local_player,
        terrain: s.terrain.clone(),
        visible: visible.clone(),
        explored: explored.clone(),
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
    let (cx, cy) = iso(v.center_x, v.center_y, 0.);
    let in_view = |x: f64, y: f64, z: f64, w: f64, h: f64| {
        let (px, py) = iso(x, y, z);
        (px - cx).abs() < half * 16. / 9. + w + h + 3. && (py - cy).abs() < half + w + h + 3.
    };
    let terrain = terrain_batch(s, v);
    records.extend_from_slice(&terrain.records);
    let mut objects: Vec<Record> = Vec::new();
    for b in &s.buildings {
        if b.rect.level != 0 {
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
            let h = 0.95 * b.progress.max(0.15);
            let mut edges = Vec::new();
            for x in r.x..r.x + r.width {
                edges.push((Pos::new(x, r.y, r.level), 5, false));
                edges.push((Pos::new(x, r.y + r.height - 1, r.level), 1, true));
            }
            for y in r.y..r.y + r.height {
                edges.push((Pos::new(r.x, y, r.level), 3, false));
                edges.push((Pos::new(r.x + r.width - 1, y, r.level), 7, true));
            }
            for (p, dir, _front) in edges {
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
            if b.progress > 0.75 {
                for y in r.y..r.y + r.height {
                    for x in r.x..r.x + r.width {
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
        if r.rect.level != 0 {
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
        if w.pos.level == 0 {
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
        for p in l.path.iter().filter(|p| p.level == 0) {
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
        if r.pos.level == 0 {
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
        if u.level == 0 {
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
                u.altitude + 0.1,
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
                        u.altitude + 0.6,
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
                u.altitude + 0.2,
                0.7,
                0.7,
                if cat == "ai" { 0.8 } else { 0.4 },
                team(u.owner),
            ));
        }
    }
    for p in &s.projectiles {
        if p.destination.level == 0 {
            objects.push(record(p.x, p.y, p.z, 0.18, 0.18, 0.18, [1., 0.78, 0.3, 1.]));
        }
    }
    for j in &s.jobs {
        if j.rect.level == 0 && j.progress > 0. && !j.blocked {
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
        if j.worker.level == 0 {
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
        if event.pos.level != 0 {
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
        if rubble.rect.level == 0 {
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
        if field.pos.level == 0 {
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
            u.level == 0
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
                    && b.rect.level == 0
                    && !assets::available(if b.kind == "core" {
                        "command-core"
                    } else {
                        &b.kind
                    })
            })
            .count()
        + s.rooms
            .iter()
            .filter(|r| r.rect.level == 0 && r.progress >= 1. && !assets::available(&r.kind))
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
static V6_SPRITE_PIXEL_CACHE: OnceLock<Mutex<BTreeMap<String, Arc<Vec<u8>>>>> = OnceLock::new();

fn staged_sprite_draws(sprites: &[Sprite]) -> Result<Vec<crate::render_core::list::V6SpriteDraw>, String> {
    let mut draws = Vec::with_capacity(sprites.len());
    for sprite in sprites {
        let Some(frame) = assets::describe(sprite)? else { continue };
        let source = crate::rpc::project_root().join(&frame.atlas);
        let source_meta = std::fs::metadata(&source)
            .map_err(|e| format!("{} atlas unavailable: {e}", sprite.asset))?;
        let source_modified = source_meta
            .modified()
            .map_err(|e| format!("{} atlas timestamp unavailable: {e}", sprite.asset))?
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let frame_key = format!("{}:{}:{}", frame.key, source_meta.len(), source_modified);
        let cache = V6_SPRITE_PIXEL_CACHE.get_or_init(|| Mutex::new(BTreeMap::new()));
        // Release the lookup guard before the miss path locks the cache again.
        // In edition 2021 an if-let scrutinee temporary lives through the else branch.
        let cached = { cache.lock().unwrap().get(&frame_key).cloned() };
        let pixels = if let Some(pixels) = cached {
            pixels
        } else {
            let page = assets::pixels(&frame)?;
            let side = frame.tile_size as usize;
            let width = frame.sample_size[0] as usize;
            let height = frame.sample_size[1] as usize;
            if page.len() != side * side * 4 || width == 0 || height == 0 || width > side || height > side {
                return Err(format!("{} invalid native sprite page geometry", sprite.asset));
            }
            let mut cropped = Vec::with_capacity(width * height * 4);
            for y in 0..height {
                cropped.extend_from_slice(&page[y * side * 4..(y + 1) * side * 4][..width * 4]);
            }
            let pixels = Arc::new(cropped);
            let mut cache = cache.lock().unwrap();
            cache.insert(frame_key.clone(), Arc::clone(&pixels));
            while cache.len() > 64 {
                if let Some(key) = cache.keys().next().cloned() { cache.remove(&key); } else { break; }
            }
            pixels
        };
        draws.push(crate::render_core::list::V6SpriteDraw {
            key: frame_key,
            asset: sprite.asset.clone(),
            pixels,
            width: frame.sample_size[0],
            height: frame.sample_size[1],
            pivot: frame.pivot,
            span: frame.span,
            position: [sprite.x as f32, sprite.y as f32, sprite.z as f32],
            scale: sprite.scale as f32,
            rotation: sprite.rotation,
            tint: sprite.tint,
            ground: sprite.ground,
            additive: frame.additive,
            entity: sprite.entity,
            owner: sprite.owner,
            entity_kind: sprite.entity_kind.clone(),
            entity_pos: sprite.entity_pos,
        });
    }
    Ok(draws)
}

/// Stage 4(Godot 腿):与 rurix `render` 开头同一段 CPU 流程(STAGED → visual_time → compose → interpolate),
/// 把私有的 Batch 拆成后端中立的 V6Frame(render_core::list)。rurix 路径不调用它,行为不变。
/// 地形 records 按 TerrainBatch 的 Arc 身份缓存一份拷贝,Arc 不变就沿用同一份(Godot 据此判断要不要重建地形网格)。
pub(crate) fn staged_frame() -> Result<crate::render_core::list::V6Frame, String> {
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
    type TerrainCache = Option<(usize, Arc<Vec<Record>>)>;
    static TERRAIN: OnceLock<Mutex<TerrainCache>> = OnceLock::new();
    let key = Arc::as_ptr(&scene.terrain) as usize;
    let terrain = {
        let mut g = TERRAIN.get_or_init(|| Mutex::new(None)).lock().unwrap();
        match &*g {
            Some((k, t)) if *k == key => Arc::clone(t),
            _ => {
                let t = Arc::new(scene.terrain.records.clone());
                *g = Some((key, Arc::clone(&t)));
                t
            }
        }
    };
    let n = scene.terrain.records.len().min(scene.records.len());
    let mut sprites = staged_sprite_draws(&scene.terrain.sprites)?;
    sprites.extend(staged_sprite_draws(&scene.sprites)?);
    Ok(crate::render_core::list::V6Frame {
        objects: scene.records[n..].to_vec(),
        terrain,
        view: scene.view,
        fallbacks: scene.fallbacks,
        sprites,
        skipped_sprites: 0,
    })
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
    if let Some(cache) = V6_SPRITE_PIXEL_CACHE.get() { cache.lock().unwrap().clear(); }
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
    // rurix 部分:丢弃 V6 GPU 会话(与拆分前同一顺序:在 CPU 状态之后)。
    #[cfg(feature = "backend-rurix")]
    rurix::close_gpu();
}
pub fn reset_scene() {
    crate::sentinels_v6_backend_metrics::clear();
    if let Some(cache) = V6_SPRITE_PIXEL_CACHE.get() { cache.lock().unwrap().clear(); }
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
                .map(|u| (u.id, [u.x, u.y, u.altitude + 0.1]))
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
    Scene { scene_guid: None,
        name: "Code Sentinels V6".into(),
        entities: vec![Entity { entity_guid: None,
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
        .map(|u| (u.id, [u.x, u.y, u.altitude + 0.1]))
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
    let (cx, cy) = iso(view.center_x, view.center_y, 0.);
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
        if let Some(depth) = record_pick_depth(&room_floor(room.rect, [1.; 4]), (world_x, world_y)) {
            hits.push((depth, "room", room.id, room.owner, room.rect.center()));
        }
    }
    // Shell floor selection shares its drawn slab geometry, so the raised room
    // floor wins where visible; front walls and roofs retain their depth.
    for b in &s.buildings {
        if b.progress >= 1. && b.kind != "shell" {
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
#[cfg(all(test, feature = "backend-rurix"))]
mod tests {
    use super::*;
    use super::rurix::*;
    #[test]
    fn empty_room_floor_pick_matches_visible_geometry_on_ground_level() {
        let level = 0;
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
