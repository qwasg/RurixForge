//! One native damage path for all weapons. Shapes are expressed in cell-edge coordinates.
use crate::skills::cone;
use crate::{catalog, types::*, Game};
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::ops::Index;
use std::sync::Arc;
use std::time::Instant;
const DT: f64 = 1. / 60.;
const EPS: f64 = 1e-7;
type Vec3 = [f64; 3];
type SurfaceKey = [u64; 2];
// Opt-in nested timing only. Never updates profiling's main phase timestamp or
// game state; scopes include children and are not additive across levels.
pub(crate) struct CombatObservation {
    name: &'static str,
    start: Option<Instant>,
}
impl CombatObservation {
    pub(crate) fn new(name: &'static str) -> Self {
        Self {
            name,
            start: crate::profiling::enabled().then(Instant::now),
        }
    }
}
impl Drop for CombatObservation {
    fn drop(&mut self) {
        if let Some(start) = self.start {
            crate::profiling::record_substage(self.name, start.elapsed().as_secs_f64() * 1000.);
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BodyKind {
    Wall,
    Floor,
    Solid,
    Equipment,
    Unit,
    Cable,
    Terrain,
}
#[derive(Debug, Clone, PartialEq)]
struct Body {
    id: u64,
    owner: u32,
    key: SurfaceKey,
    min: Vec3,
    max: Vec3,
    level: i32,
    kind: BodyKind,
    resistance: f64,
}
impl Body {
    fn contains(&self, p: Vec3) -> bool {
        (0..3).all(|i| p[i] >= self.min[i] - EPS && p[i] <= self.max[i] + EPS)
    }
    fn structural(&self) -> bool {
        !matches!(self.kind, BodyKind::Unit | BodyKind::Terrain)
    }
    fn opaque(&self) -> bool {
        matches!(
            self.kind,
            BodyKind::Wall | BodyKind::Floor | BodyKind::Solid | BodyKind::Terrain
        )
    }
    fn floor(&self) -> bool {
        self.kind == BodyKind::Floor
            || self.kind == BodyKind::Terrain && self.max[2] - self.min[2] < 0.3
    }
    fn nearest(&self, p: Vec3) -> Vec3 {
        std::array::from_fn(|i| p[i].clamp(self.min[i], self.max[i]))
    }
}
fn hit(a: Vec3, b: Vec3, body: &Body) -> Option<(f64, f64)> {
    let (mut lo, mut hi) = (0f64, 1f64);
    for axis in 0..3 {
        let d = b[axis] - a[axis];
        if d.abs() < EPS {
            if a[axis] < body.min[axis] || a[axis] > body.max[axis] {
                return None;
            }
        } else {
            let x = (body.min[axis] - a[axis]) / d;
            let y = (body.max[axis] - a[axis]) / d;
            lo = lo.max(x.min(y));
            hi = hi.min(x.max(y));
            if lo > hi {
                return None;
            }
        }
    }
    Some((lo, hi))
}
fn mix(a: Vec3, b: Vec3, t: f64) -> Vec3 {
    std::array::from_fn(|i| a[i] + (b[i] - a[i]) * t)
}
fn distance(a: Vec3, b: Vec3) -> f64 {
    ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2) + ((a[2] - b[2]) * 4.).powi(2)).sqrt()
}
fn cell(point: Vec3, level: i32) -> Pos {
    Pos::new(
        (point[0].floor() as i32).clamp(0, 127),
        (point[1].floor() as i32).clamp(0, 95),
        level.clamp(-2, 5),
    )
}
fn center(pos: Pos, altitude: f64) -> Vec3 {
    [
        pos.x as f64 + 0.5,
        pos.y as f64 + 0.5,
        pos.level as f64 + altitude,
    ]
}
fn point_to_cell_distance(point: Vec3, pos: Pos) -> f64 {
    distance(point, center(pos, 0.5))
}
fn airborne(unit: &Unit) -> bool {
    unit.altitude > 0.25 && unit.flightState != "landed"
}
fn can_fire(unit: &Unit, def: &catalog::UnitDef) -> bool {
    def.category != "air"
        || (unit.flightState == "cruising" && unit.altitude >= 1.5 && unit.fuel > 0.)
}
pub fn modifier(unit: &Unit, name: &str) -> f64 {
    unit.plugins
        .iter()
        .filter_map(|p| catalog::plugin_ref(p))
        .filter(|p| p.modifier == name)
        .map(|p| p.magnitude)
        .sum()
}
// Exact values, not a hash or tick revision: direct editor/load/test mutations
// must invalidate geometry too. Dynamic reservoirs and positive HP changes do
// not change geometry; crossing alive/progress thresholds does.
#[derive(Clone, PartialEq, Eq)]
struct GeometryStamp {
    buildings: Vec<(u64, u32, Rect, u64, u8)>,
    rooms: Vec<(u64, u32, Rect, bool)>,
    walls: Vec<(u64, u32, Pos)>,
    entrances: Vec<(u64, u32, Pos, i32, u32, u8, u8, bool, bool, bool)>,
    links: Vec<(u64, u32, Vec<Pos>)>,
    terrain: Vec<u8>,
    excavated: Vec<Pos>,
}
#[derive(Clone, Default)]
pub(crate) struct CollisionCache {
    stamp: Option<GeometryStamp>,
    fixed: Arc<Vec<Body>>,
    spatial: Arc<Spatial>,
}
struct Bodies {
    fixed: Arc<Vec<Body>>,
    moving: Vec<Body>,
    aims: HashMap<u64, Vec3>,
    // Launch/skill validation is read-only. Projectile resolution may remove a
    // prior target between rounds, so its fallback must be resolved live.
    snapshot_aims: bool,
}
impl Bodies {
    #[cfg(test)]
    fn iter(&self) -> impl Iterator<Item = &Body> {
        self.fixed.iter().chain(self.moving.iter())
    }
}
impl Index<usize> for Bodies {
    type Output = Body;
    fn index(&self, index: usize) -> &Body {
        if index < self.fixed.len() {
            &self.fixed[index]
        } else {
            &self.moving[index - self.fixed.len()]
        }
    }
}
#[derive(Default)]
struct Spatial {
    fixed: Option<Arc<Spatial>>,
    buckets: HashMap<(i32, i32, i32), Vec<usize>>,
    surfaces: HashMap<SurfaceKey, Vec<usize>>,
    entities: HashMap<u64, Vec<usize>>,
    entity_bounds: HashMap<u64, (Vec3, Vec3)>,
}
impl Spatial {
    fn new(bodies: &[Body]) -> Self {
        Self::overlay(bodies, 0, None)
    }
    fn overlay(bodies: &[Body], offset: usize, fixed: Option<Arc<Spatial>>) -> Self {
        let mut result = Self {
            fixed,
            buckets: HashMap::new(),
            surfaces: HashMap::new(),
            entities: HashMap::new(),
            entity_bounds: HashMap::new(),
        };
        for (i, b) in bodies.iter().enumerate() {
            let i = i + offset;
            result.surfaces.entry(b.key).or_default().push(i);
            if b.id > 0 {
                result.entities.entry(b.id).or_default().push(i);
                let bounds = result.entity_bounds.entry(b.id).or_insert((b.min, b.max));
                for axis in 0..3 {
                    bounds.0[axis] = bounds.0[axis].min(b.min[axis]);
                    bounds.1[axis] = bounds.1[axis].max(b.max[axis]);
                }
            }
            for z in b.min[2].floor() as i32..=b.max[2].floor() as i32 {
                for y in (b.min[1] / 8.).floor() as i32..=(b.max[1] / 8.).floor() as i32 {
                    for x in (b.min[0] / 8.).floor() as i32..=(b.max[0] / 8.).floor() as i32 {
                        result.buckets.entry((x, y, z)).or_default().push(i);
                    }
                }
            }
        }
        result
    }
    fn entity(&self, id: u64) -> impl Iterator<Item = &usize> {
        self.fixed
            .iter()
            .filter_map(move |s| s.entities.get(&id))
            .flatten()
            .chain(self.entities.get(&id).into_iter().flatten())
    }
    fn surface(&self, key: &SurfaceKey) -> impl Iterator<Item = &usize> {
        let key = *key;
        self.fixed
            .iter()
            .filter_map(move |s| s.surfaces.get(&key))
            .flatten()
            .chain(self.surfaces.get(&key).into_iter().flatten())
    }
    fn entity_may_be_in_range(&self, id: u64, origin: Vec3, radius: f64) -> bool {
        let bounds = self
            .fixed
            .as_ref()
            .and_then(|s| s.entity_bounds.get(&id))
            .or_else(|| self.entity_bounds.get(&id));
        let Some((min, max)) = bounds else {
            return true;
        }; // Preserve existing no-collider fallback aims.
        let bound = radius + EPS;
        let mut squared = 0.;
        for axis in 0..3 {
            let offset = (min[axis] - origin[axis])
                .max(0.)
                .max(origin[axis] - max[axis])
                * if axis == 2 { 4. } else { 1. };
            if offset > bound {
                return false;
            }
            squared += offset * offset;
        }
        squared <= bound * bound
    }
    fn collect_bucket(&self, at: (i32, i32, i32), result: &mut Vec<usize>) {
        if let Some(items) = self.fixed.as_ref().and_then(|s| s.buckets.get(&at)) {
            result.extend(items);
        }
        if let Some(items) = self.buckets.get(&at) {
            result.extend(items);
        }
    }
    fn segment(&self, a: Vec3, b: Vec3) -> Vec<usize> {
        let mut result = Vec::new();
        // Traverse every bucket touched by the segment. Fixed-distance samples can
        // miss a very short crossing near a bucket corner, even at high frequency.
        let scale = [8., 8., 1.];
        let start: Vec3 = std::array::from_fn(|i| a[i] / scale[i]);
        let end: Vec3 = std::array::from_fn(|i| b[i] / scale[i]);
        let delta: Vec3 = std::array::from_fn(|i| end[i] - start[i]);
        let mut bucket: [i32; 3] = std::array::from_fn(|i| start[i].floor() as i32);
        let step: [i32; 3] = std::array::from_fn(|i| delta[i].signum() as i32);
        let stride: Vec3 = std::array::from_fn(|i| {
            if delta[i].abs() > EPS {
                1. / delta[i].abs()
            } else {
                f64::INFINITY
            }
        });
        let mut next: Vec3 = std::array::from_fn(|i| {
            if delta[i].abs() <= EPS {
                f64::INFINITY
            } else if step[i] > 0 {
                (bucket[i] as f64 + 1. - start[i]) / delta[i]
            } else {
                (bucket[i] as f64 - start[i]) / delta[i]
            }
        });
        let mut collect = |at: [i32; 3]| {
            self.collect_bucket((at[0], at[1], at[2]), &mut result);
        };
        collect(bucket);
        loop {
            let t = next.into_iter().fold(f64::INFINITY, f64::min);
            if !t.is_finite() || t > 1. + EPS {
                break;
            }
            let mut axes = [0; 3];
            let mut count = 0;
            for i in 0..3 {
                if (next[i] - t).abs() <= EPS {
                    axes[count] = i;
                    count += 1;
                }
            }
            let axes = &axes[..count];
            // Include edge/corner neighbors before advancing all tied axes.
            for mask in 1..1 << axes.len() {
                let mut touched = bucket;
                for (bit, &axis) in axes.iter().enumerate() {
                    if mask & (1 << bit) != 0 {
                        touched[axis] += step[axis];
                    }
                }
                collect(touched);
            }
            for &axis in axes {
                bucket[axis] += step[axis];
                next[axis] += stride[axis];
            }
        }
        // Preserve the old BTreeSet's ascending candidate order, including
        // equal-distance collision ties, without allocating a tree per ray.
        result.sort_unstable();
        result.dedup();
        result
    }
    fn sphere(&self, p: Vec3, radius: f64) -> Vec<usize> {
        let mut result = Vec::new();
        for z in (p[2] - radius / 4.).floor() as i32..=(p[2] + radius / 4.).floor() as i32 {
            for y in ((p[1] - radius) / 8.).floor() as i32..=((p[1] + radius) / 8.).floor() as i32 {
                for x in
                    ((p[0] - radius) / 8.).floor() as i32..=((p[0] + radius) / 8.).floor() as i32
                {
                    self.collect_bucket((x, y, z), &mut result);
                }
            }
        }
        result.sort_unstable();
        result.dedup();
        result
    }
}
#[derive(Clone)]
struct Impact {
    projectile: Projectile,
    point: Vec3,
    target: Option<u64>,
    level: i32,
    splash: bool,
}
#[derive(Clone, Copy)]
struct WeaponTarget {
    id: u64,
    pos: Pos,
    owner: u32,
    air: bool,
}
impl WeaponTarget {
    fn permitted(self, def: &catalog::UnitDef) -> bool {
        if self.air {
            def.target_air
        } else {
            def.target_ground
        }
    }
}

impl Game {
    fn geometry_stamp(&self) -> GeometryStamp {
        let _observe = CombatObservation::new("combat/static-stamp");
        GeometryStamp {
            buildings: self
                .state
                .buildings
                .iter()
                .filter(|b| b.hp > 0.)
                .map(|b| {
                    (
                        b.id,
                        b.owner,
                        b.rect,
                        b.progress.clamp(0., 1.).to_bits(),
                        if b.kind == "shell" {
                            1
                        } else if matches!(b.kind.as_str(), "airstrip" | "launch-pad") {
                            2
                        } else {
                            0
                        },
                    )
                })
                .collect(),
            rooms: self
                .state
                .rooms
                .iter()
                .filter(|r| r.hp > 0. && r.progress >= 0.5 && r.kind != "corridor")
                .map(|r| (r.id, r.owner, r.rect, r.kind == "column"))
                .collect(),
            walls: self
                .state
                .walls
                .iter()
                .filter(|w| w.hp > 0.)
                .map(|w| (w.id, w.owner, w.pos))
                .collect(),
            entrances: self
                .state
                .entrances
                .iter()
                .map(|e| {
                    (
                        e.id,
                        e.owner,
                        e.pos,
                        e.toLevel,
                        e.width,
                        match e.axis.as_str() {
                            "x" => 1,
                            "y" => 2,
                            _ => 0,
                        },
                        match e.kind.as_str() {
                            "door" => 1,
                            "window" => 2,
                            "elevator" => 3,
                            _ => 0,
                        },
                        e.open,
                        e.hp > 0.,
                        e.reserves_space(),
                    )
                })
                .collect(),
            links: self
                .state
                .links
                .iter()
                .filter(|l| l.hp > 0.)
                .map(|l| (l.id, l.owner, l.path.clone()))
                .collect(),
            terrain: self.state.terrain.clone(),
            excavated: self.state.excavated.iter().copied().collect(),
        }
    }
    fn static_collision(&self) -> (Arc<Vec<Body>>, Arc<Spatial>) {
        let _observe = CombatObservation::new("combat/static-cache");
        let stamp = self.geometry_stamp();
        let mut cache = self.collision_cache.borrow_mut();
        if cache.stamp.as_ref() != Some(&stamp) {
            let fixed = self.static_collision_bodies();
            let index_observe = CombatObservation::new("combat/static-index-rebuild");
            cache.spatial = Arc::new(Spatial::new(&fixed));
            drop(index_observe);
            cache.fixed = Arc::new(fixed);
            cache.stamp = Some(stamp);
        }
        (Arc::clone(&cache.fixed), Arc::clone(&cache.spatial))
    }
    fn collision_scene(&self) -> (Bodies, Spatial) {
        let _observe = CombatObservation::new("combat/collision-scene");
        let (fixed, index) = self.static_collision();
        let (moving, aims) = self.moving_collision_bodies();
        let dynamic_index_observe = CombatObservation::new("combat/dynamic-index");
        let spatial = Spatial::overlay(&moving, fixed.len(), Some(index));
        drop(dynamic_index_observe);
        (
            Bodies {
                fixed,
                moving,
                aims,
                snapshot_aims: true,
            },
            spatial,
        )
    }
    #[cfg(test)]
    fn collision_bodies(&self) -> Vec<Body> {
        self.collision_scene().0.iter().cloned().collect()
    }
    pub(crate) fn point_line_clear(&self, from: [f64; 3], to: [f64; 3]) -> bool {
        let _observe = CombatObservation::new("combat/point-los");
        if !from.into_iter().chain(to).all(f64::is_finite) {
            return false;
        }
        let (bodies, spatial) = self.static_collision();
        !spatial.segment(from, to).into_iter().any(|i| {
            let body = &bodies[i];
            body.opaque()
                && hit(from, to, body).is_some_and(|(enter, exit)| enter < 1. - EPS && exit > EPS)
        })
    }
    /// Same-floor skill sightline through the same walls, doors, terrain and
    /// floor geometry used by projectile collision. A target surface itself is
    /// reachable; intervening friendly structures still obstruct the effect.
    pub(crate) fn skill_line_clear(&self, origin: Pos, target: Pos, _owner: u32) -> bool {
        let _observe = CombatObservation::new("combat/skill-los");
        if !origin.valid() || !target.valid() || origin.level != target.level {
            return false;
        }
        let a = center(origin, 0.5);
        let b = center(target, 0.5);
        let (bodies, spatial) = self.static_collision();
        !spatial.segment(a, b).into_iter().any(|i| {
            let body = &bodies[i];
            body.opaque()
                && !body.contains(b)
                && hit(a, b, body).is_some_and(|(enter, exit)| enter < 1. - EPS && exit > EPS)
        })
    }
    pub(crate) fn skill_weapon_target_aim(
        &self,
        source: &Unit,
        target: u64,
        pos: Pos,
        max_range: f64,
    ) -> Option<Vec3> {
        let Some(def) = catalog::unit_ref(&source.kind) else {
            return None;
        };
        if !max_range.is_finite()
            || max_range < 0.
            || !pos.valid()
            || !self.visible_to(source.owner, pos)
            || !self.valid_weapon_target(def, target)
            || self
                .target_info(target)
                .is_none_or(|(_, owner)| owner == source.owner)
        {
            return None;
        }
        let origin = [source.x, source.y, source.elevation() + 0.5];
        let (bodies, spatial) = self.collision_scene();
        let aim = self.target_point_from(target, pos, origin, &bodies, &spatial);
        if distance(origin, aim) > max_range {
            return None;
        }
        let mut previous = if def.trajectory == "orbital" {
            [aim[0], aim[1], 30.]
        } else {
            origin
        };
        let launch = previous;
        let steps = if def.trajectory == "arc" { 24 } else { 1 };
        for step in 1..=steps {
            let t = step as f64 / steps as f64;
            let mut next = mix(launch, aim, t);
            if def.trajectory == "arc" {
                next[2] += 4. * t * (1. - t) * (distance(launch, aim) / 3.).min(8.);
            }
            if spatial.segment(previous, next).into_iter().any(|i| {
                let body = &bodies[i];
                body.opaque()
                    && body.id != source.id
                    && body.id != target
                    && hit(previous, next, body)
                        .is_some_and(|(enter, exit)| enter < 1. - EPS && exit > EPS)
            }) {
                return None;
            }
            previous = next;
        }
        Some(aim)
    }
    fn valid_weapon_target(&self, d: &catalog::UnitDef, id: u64) -> bool {
        if let Some(unit) = self.state.units.iter().find(|u| u.id == id) {
            if airborne(unit) {
                d.target_air
            } else {
                d.target_ground
            }
        } else if let Some(shipment) = self.state.shipments.iter().find(|s| s.id == id) {
            if shipment.mode == "air" && shipment.altitude > 0.25 {
                d.target_air
            } else {
                d.target_ground
            }
        } else {
            d.target_ground
        }
    }
    fn target_point(&self, id: u64, fallback: Pos) -> Vec3 {
        if let Some(u) = self.state.units.iter().find(|u| u.id == id) {
            return [u.x, u.y, u.elevation() + 0.45];
        }
        if let Some(s) = self.state.shipments.iter().find(|s| s.id == id) {
            let height = s.altitude + 0.35;
            let a = center(s.pos, height);
            return s
                .route
                .first()
                .map(|next| mix(a, center(*next, height), s.progress.clamp(0., 1.)))
                .unwrap_or(a);
        }
        if let Some(b) = self.state.buildings.iter().find(|b| b.id == id) {
            if matches!(b.kind.as_str(), "airstrip" | "launch-pad") {
                return center(fallback, 0.04);
            }
            return center(fallback, 0.08 + 0.42 * b.progress.clamp(0., 1.));
        }
        center(fallback, 0.45)
    }
    fn target_point_from(
        &self,
        id: u64,
        fallback: Pos,
        origin: Vec3,
        bodies: &Bodies,
        spatial: &Spatial,
    ) -> Vec3 {
        if bodies.snapshot_aims {
            if let Some(point) = bodies.aims.get(&id) {
                return *point;
            }
        } else if spatial
            .entity(id)
            .next()
            .is_some_and(|i| bodies[*i].kind == BodyKind::Unit)
        {
            return self.target_point(id, fallback);
        }
        // A 24-cell facade is targetable at the visible surface near the gun;
        // requiring its distant center to be in range would make large shells immune at close range.
        spatial
            .entity(id)
            .map(|i| bodies[*i].nearest(origin))
            .min_by(|a, b| distance(origin, *a).total_cmp(&distance(origin, *b)))
            .unwrap_or_else(|| self.target_point(id, fallback))
    }
    fn pay_weapon(&mut self, index: usize, d: &catalog::UnitDef) -> bool {
        let _observe = CombatObservation::new("combat/weapon-payment");
        let u = self.state.units[index].clone();
        let payload = (1. - modifier(&u, "payload-efficiency")).clamp(0.15, 1.);
        let compute = d.compute_per_attack
            * payload
            * (1. - modifier(&u, "compute-efficiency")).clamp(0.15, 1.)
            * if u.statuses.contains_key("compute-efficiency") {
                0.8
            } else {
                1.
            };
        let ammo = d.ammo_per_shot * payload;
        let energy = d.energy_per_attack.max(0.) * payload;
        // Validate every local reservoir before debiting any of them.
        if u.ammo + EPS < ammo || u.energy + EPS < energy {
            // Payment is attempted at most once per ready unit per step, after
            // acquiring an in-range target or interception. These are unit
            // seconds, not wall seconds or a counterfactual damage estimate.
            if let Some(player) = self.player_mut(u.owner) {
                if u.ammo + EPS < ammo {
                    *player
                        .totals
                        .entry("ammo-starved-firing-unit-seconds".into())
                        .or_default() += DT;
                }
                if u.energy + EPS < energy {
                    *player
                        .totals
                        .entry("energy-starved-firing-unit-seconds".into())
                        .or_default() += DT;
                }
            }
            return false;
        }
        if compute > 0. && !self.pay_attack(index, compute) {
            if let Some(player) = self.player_mut(u.owner) {
                *player
                    .totals
                    .entry("compute-starved-firing-unit-seconds".into())
                    .or_default() += DT;
            }
            return false;
        }
        self.state.units[index].ammo = (self.state.units[index].ammo - ammo).max(0.);
        self.state.units[index].energy = (self.state.units[index].energy - energy).max(0.);
        if compute > 0. {
            self.event("compute-spent", u.pos, u.owner, compute, u.id);
        }
        if ammo > 0. {
            self.event("ammo-spent", u.pos, u.owner, ammo, u.id);
        }
        if energy > 0. {
            self.event("energy-spent", u.pos, u.owner, energy, u.id);
        }
        true
    }
    fn launch(
        &mut self,
        u: &Unit,
        d: &catalog::UnitDef,
        id: u64,
        pos: Pos,
        scale: f64,
        bodies: &Bodies,
        spatial: &Spatial,
    ) {
        let _observe = CombatObservation::new("combat/launch");
        let orbital = d.trajectory == "orbital";
        let mut aim =
            self.target_point_from(id, pos, [u.x, u.y, u.elevation() + 0.5], bodies, spatial);
        let launch = if orbital {
            [aim[0], aim[1], 30.]
        } else {
            [u.x, u.y, u.elevation() + 0.5]
        };
        let travel =
            (distance(launch, aim) / if d.trajectory == "arc" { 12. } else { 28. }).max(0.1);
        let targeting = self.facility_bonus_for_unit(u, "targeting-array");
        let prediction = modifier(u, "prediction");
        // Predictive fire is a branch capability; ordinary unguided shots retain their launch-time aim.
        if u.branch == "algorithm"
            && !matches!(d.trajectory.as_str(), "guided" | "orbital" | "beam")
        {
            if let Some(target) = self.state.units.iter().find(|target| target.id == id) {
                if let (Some(next), Some(td)) =
                    (target.route.first(), catalog::unit_ref(&target.kind))
                {
                    let dx = next.x as f64 + 0.5 - target.x;
                    let dy = next.y as f64 + 0.5 - target.y;
                    let length = (dx * dx + dy * dy).sqrt();
                    if length > EPS {
                        let lead = (td.speed * travel).min(4. * (1. + prediction + targeting));
                        aim[0] = (aim[0] + dx / length * lead).clamp(0.05, 127.95);
                        aim[1] = (aim[1] + dy / length * lead).clamp(0.05, 95.95);
                    }
                }
            }
        }
        if d.trajectory == "direct" {
            // Aim once, including any branch prediction, then fly the same
            // non-homing ray at a fixed speed to its finite weapon range.
            // The old target position is not an airburst fuse: collision or
            // the range endpoint resolves the existing payload and splash.
            let direction_length = distance(launch, aim);
            if direction_length > EPS {
                aim = mix(launch, aim, d.range / direction_length);
            }
        }
        let marked = self
            .state
            .units
            .iter()
            .find(|v| v.id == id)
            .is_some_and(|v| v.statuses.contains_key("marked"));
        let damage = d.damage
            * 1.4f64.powi(u.tier.saturating_sub(d.tier) as i32)
            * scale
            * if marked {
                1.2 + modifier(u, "mark-damage") + targeting * 0.25
            } else {
                1.
            }
            * if u.statuses.contains_key("support-boost") {
                1.2
            } else {
                1.
            }
            * if u.statuses.contains_key("charged-shot") {
                u.chargedShotMultiplier
            } else {
                1.
            };
        let destination = cell(aim, pos.level);
        let pid = self.id();
        self.state.projectiles.push(Projectile {
            id: pid,
            owner: u.owner,
            source: u.id,
            target: Some(id),
            origin: if orbital { destination } else { u.pos },
            destination,
            x: launch[0],
            y: launch[1],
            z: launch[2],
            age: 0.,
            duration: if orbital {
                4.
            } else if d.trajectory == "beam" {
                0.04
            } else if d.trajectory == "guided" {
                travel * 2. + 0.2
            } else if d.trajectory == "direct" {
                d.range / 28.
            } else {
                travel
            },
            damage,
            radius: d.radius,
            kind: d.trajectory.clone(),
            damageType: d.damage_type.clone(),
            penetration: d.breach_depth as f64,
            structureMultiplier: d.structure_multiplier,
            sourceAltitude: launch[2]
                - if orbital {
                    pos.level as f64
                } else {
                    u.level as f64
                },
            targetAltitude: aim[2] - pos.level as f64,
            jammed: false,
            launchPosition: Some(launch),
            aimPosition: Some(aim),
            passedSurfaces: vec![],
            onHitStatus: if u.kind == "deepseek" {
                Some(HitStatus {
                    kind: "marked".into(),
                    duration: 3.,
                })
            } else if u.kind == "minimax" && (u.attackCount + 1) % 4 == 0 {
                Some(HitStatus {
                    kind: "anti-heal".into(),
                    duration: 3.,
                })
            } else {
                None
            },
            movingTargetMultiplier: 1. + prediction * 0.5,
            targetMultiplier: if u.statuses.contains_key(&format!("target-lock:{id}")) {
                1.25
            } else {
                1.
            },
        });
        if orbital {
            self.event("telegraph", destination, u.owner, d.radius, u.id);
        }
    }
    pub(crate) fn advance_combat(&mut self) {
        let _observe = CombatObservation::new("combat/total");
        let (bodies, spatial) = self.collision_scene();
        let firing_observe = CombatObservation::new("combat/firing-total");
        // No target position, ownership or visibility changes until projectile
        // resolution below. Resolve each viewer's target surfaces once instead
        // of rescanning units/buildings/entrances for every gun and chain jump.
        let mut target_lists: HashMap<u32, Vec<WeaponTarget>> = HashMap::new();
        for field in &mut self.state.defenseFields {
            field.remaining -= DT;
        }
        self.state
            .defenseFields
            .retain(|f| f.remaining > 0. && f.hp > 0.);
        for i in 0..self.state.units.len() {
            let u = self.state.units[i].clone();
            let Some(d) = catalog::unit_ref(&u.kind) else {
                continue;
            };
            if u.hp <= 0.
                || u.cooldown > 0.
                || u.statuses.contains_key("silence")
                || !can_fire(&u, d)
            {
                continue;
            }
            if d.category == "orbital"
                && (!self.state.rooms.iter().any(|r| {
                    r.owner == u.owner
                        && r.kind == "orbital-control"
                        && r.hp > 0.
                        && r.online
                        && r.powered
                        && r.connected
                        && r.progress >= 1.
                }) || !self.state.buildings.iter().any(|b| {
                    b.owner == u.owner
                        && b.kind == "launch-pad"
                        && b.hp > 0.
                        && b.progress >= 1.
                        && b.powered
                }))
            {
                continue;
            }
            let target_observe = CombatObservation::new("combat/target-selection");
            if matches!(d.chassis.as_str(), "anti-orbital" | "aa-launcher") {
                let origin = [u.x, u.y, u.elevation() + 0.5];
                if let Some(index) = self.state.projectiles.iter().position(|p| {
                    p.owner != u.owner
                        && p.damage > 0.
                        && matches!(p.kind.as_str(), "guided" | "arc" | "orbital")
                        && (p.kind != "orbital" || d.chassis == "anti-orbital")
                        && distance(origin, [p.x, p.y, p.z]) <= d.range
                        && self.visible_to(u.owner, cell([p.x, p.y, p.z], p.destination.level))
                        && !spatial
                            .segment(origin, [p.x, p.y, p.z])
                            .into_iter()
                            .any(|i| {
                                let b = &bodies[i];
                                b.opaque()
                                    && b.id != u.id
                                    && hit(origin, [p.x, p.y, p.z], b)
                                        .is_some_and(|(enter, exit)| enter < 1. - EPS && exit > EPS)
                            })
                }) {
                    if self.pay_weapon(i, d) {
                        let absorbed = self.state.projectiles[index].damage.min(
                            d.damage
                                * (1. + modifier(&u, "interception"))
                                * if u.statuses.contains_key("charged-shot") {
                                    u.chargedShotMultiplier
                                } else {
                                    1.
                                },
                        );
                        self.state.projectiles[index].damage -= absorbed;
                        self.state.units[i].cooldown = d.period;
                        self.state.units[i].lastAttackTick = Some(self.state.tick);
                        self.state.units[i].statuses.remove("charged-shot");
                        self.state.units[i].chargedShotMultiplier = 1.;
                        self.event("intercept", u.pos, u.owner, absorbed, u.id);
                    }
                    continue;
                }
            }
            let locked = u
                .statuses
                .keys()
                .filter_map(|key| key.strip_prefix("target-lock:")?.parse::<u64>().ok())
                .filter_map(|id| {
                    self.target_info_for(id, u.owner)
                        .map(|(p, owner)| (id, p, owner))
                })
                .find(|(id, p, owner)| {
                    *owner != u.owner
                        && self.valid_weapon_target(d, *id)
                        && self.visible_to(u.owner, *p)
                        && distance(
                            [u.x, u.y, u.elevation() + 0.5],
                            self.target_point_from(
                                *id,
                                *p,
                                [u.x, u.y, u.elevation() + 0.5],
                                &bodies,
                                &spatial,
                            ),
                        ) <= d.range
                });
            let mut target = locked.or_else(|| {
                u.target
                    .and_then(|id| self.target_info_for(id, u.owner).map(|(p, o)| (id, p, o)))
                    .filter(|(id, _, owner)| *owner != u.owner && self.valid_weapon_target(d, *id))
            });
            if target.is_none() {
                let origin = [u.x, u.y, u.elevation() + 0.5];
                let candidates = target_lists
                    .entry(u.owner)
                    .or_insert_with(|| self.weapon_targets(u.owner));
                target = self.nearest_weapon_target(candidates, origin, d, &bodies, &spatial);
            }
            let Some((id, pos, _)) = target else {
                continue;
            };
            if !self.visible_to(u.owner, pos) {
                continue;
            }
            let target_point =
                self.target_point_from(id, pos, [u.x, u.y, u.elevation() + 0.5], &bodies, &spatial);
            if distance([u.x, u.y, u.elevation() + 0.5], target_point) > d.range {
                if d.speed > 0.
                    && !u.wired
                    && u.route.is_empty()
                    && self.state.tick % 60 == 0
                    && d.category != "air"
                {
                    let _routing_observe = CombatObservation::new("combat/chase-route");
                    let mut points = [
                        Pos::new(pos.x - 3, pos.y, pos.level),
                        Pos::new(pos.x + 3, pos.y, pos.level),
                        Pos::new(pos.x, pos.y - 3, pos.level),
                        Pos::new(pos.x, pos.y + 3, pos.level),
                    ];
                    points.sort_by(|a, b| u.pos.distance(*a).total_cmp(&u.pos.distance(*b)));
                    for point in points {
                        if let Some(route) = self.route(u.pos, point, &d.category) {
                            self.state.units[i].route = route;
                            break;
                        }
                    }
                }
                continue;
            }
            drop(target_observe);
            if !self.pay_weapon(i, d) {
                continue;
            }
            let (dx, dy) = (target_point[0] - u.x, target_point[1] - u.y);
            if dx.abs() + dy.abs() > EPS {
                self.state.units[i].facing = (((dy.atan2(dx) - std::f64::consts::FRAC_PI_4)
                    / std::f64::consts::FRAC_PI_4)
                    .round() as i32)
                    .rem_euclid(8) as u32;
            }
            match d.attack_pattern.as_str() {
                "chain" => {
                    self.launch(&u, d, id, pos, 1., &bodies, &spatial);
                    let mut others = target_lists
                        .entry(u.owner)
                        .or_insert_with(|| self.weapon_targets(u.owner))
                        .iter()
                        .copied()
                        .filter_map(|v| {
                            let (other, p, owner) = (v.id, v.pos, v.owner);
                            let near =
                                self.target_point_from(other, p, target_point, &bodies, &spatial);
                            let separation = distance(target_point, near);
                            (other != id
                                && p.level == pos.level
                                && separation <= 4.
                                && v.permitted(d))
                            .then_some((other, p, owner, separation))
                        })
                        .collect::<Vec<_>>();
                    others.sort_by(|a, b| a.3.total_cmp(&b.3));
                    for (index, (other, p, _, _)) in others.into_iter().take(2).enumerate() {
                        self.launch(
                            &u,
                            d,
                            other,
                            p,
                            0.5f64.powi(index as i32 + 1),
                            &bodies,
                            &spatial,
                        );
                    }
                }
                "dual-beam" => {
                    self.launch(&u, d, id, pos, 0.5, &bodies, &spatial);
                    self.launch(&u, d, id, pos, 0.5, &bodies, &spatial);
                }
                "burst" => {
                    for _ in 0..3 {
                        self.launch(&u, d, id, pos, 1. / 3., &bodies, &spatial);
                    }
                }
                "cone" => {
                    self.launch(&u, d, id, pos, 1., &bodies, &spatial);
                    for v in target_lists
                        .entry(u.owner)
                        .or_insert_with(|| self.weapon_targets(u.owner))
                        .iter()
                        .copied()
                    {
                        let (other, p) = (v.id, v.pos);
                        if other == id {
                            continue;
                        }
                        let point = self.target_point_from(
                            other,
                            p,
                            [u.x, u.y, u.elevation() + 0.5],
                            &bodies,
                            &spatial,
                        );
                        if cone(
                            u.pos,
                            cell(target_point, pos.level),
                            cell(point, p.level),
                            d.range,
                            60.,
                        ) && v.permitted(d)
                        {
                            self.launch(&u, d, other, p, 0.55, &bodies, &spatial);
                        }
                    }
                }
                _ => self.launch(&u, d, id, pos, 1., &bodies, &spatial),
            }
            self.state.units[i].cooldown = d.period
                / (1. + modifier(&u, "attack-rate"))
                / if u.statuses.contains_key("haste") {
                    1.35
                } else {
                    1.
                };
            self.state.units[i].attackCount += 1;
            if u.kind == "gemini" && self.state.units[i].attackCount % 3 == 0 {
                self.state.units[i]
                    .statuses
                    .insert("gemini-ready".into(), 20.);
            }
            self.state.units[i].lastAttackTick = Some(self.state.tick);
            self.state.units[i].statuses.remove("charged-shot");
            self.state.units[i].chargedShotMultiplier = 1.;
            self.event("fire", u.pos, u.owner, 0., u.id);
        }
        drop(firing_observe);
        self.advance_projectiles(bodies, spatial);
    }
    fn advance_projectiles(&mut self, mut bodies: Bodies, spatial: Spatial) {
        let _observe = CombatObservation::new("combat/projectiles-total");
        bodies.snapshot_aims = false;
        let mut moving = std::mem::take(&mut self.state.projectiles);
        for mut projectile in moving.drain(..) {
            if projectile.damage <= 0. {
                continue;
            }
            let motion_observe = CombatObservation::new("combat/projectile-motion");
            let before = [projectile.x, projectile.y, projectile.z];
            let launch = *projectile.launchPosition.get_or_insert(before);
            if projectile.aimPosition.is_none() {
                projectile.aimPosition = Some(if let Some(id) = projectile.target {
                    self.target_point_from(id, projectile.destination, before, &bodies, &spatial)
                } else {
                    center(
                        projectile.destination,
                        if projectile.kind == "delayed-area" {
                            0.02
                        } else {
                            projectile.targetAltitude
                        },
                    )
                });
            }
            projectile
                .passedSurfaces
                .retain(|key| spatial.surface(key).any(|i| bodies[*i].contains(before)));
            if projectile.kind == "guided" && !projectile.jammed {
                if let Some((id, (pos, _))) = projectile
                    .target
                    .and_then(|id| self.target_info(id).map(|v| (id, v)))
                {
                    let aim = self.target_point_from(id, pos, before, &bodies, &spatial);
                    projectile.destination = cell(aim, pos.level);
                    projectile.targetAltitude = aim[2] - pos.level as f64;
                    projectile.aimPosition = Some(aim);
                }
                self.jam_projectile(&mut projectile);
            }
            projectile.age += DT;
            let aim = projectile.aimPosition.unwrap();
            let t = (projectile.age / projectile.duration.max(DT)).min(1.);
            let mut arrived = false;
            let mut next = if projectile.kind == "guided" {
                let travel = distance(before, aim);
                let amount = 28. * DT;
                if travel <= amount {
                    arrived = true;
                    aim
                } else {
                    mix(before, aim, amount / travel)
                }
            } else {
                mix(launch, aim, t)
            };
            if projectile.kind == "arc" {
                next[2] += 4. * t * (1. - t) * (distance(launch, aim) / 3.).min(8.);
            }
            if t >= 1. {
                arrived = true;
            }
            if self.intercept_payload(&mut projectile, before, next) {
                continue;
            }
            drop(motion_observe);
            let sweep_observe = CombatObservation::new("combat/projectile-sweep");
            let mut collisions = spatial
                .segment(before, next)
                .into_iter()
                .filter_map(|index| {
                    let body = &bodies[index];
                    if body.id > 0 && body.id == projectile.source
                        || body.owner == projectile.owner && body.kind == BodyKind::Unit
                        || body.owner == projectile.owner
                            && body.kind == BodyKind::Equipment
                            && body.contains(launch)
                        || projectile.passedSurfaces.contains(&body.key)
                    {
                        return None;
                    }
                    hit(before, next, body).map(|(enter, exit)| (index, enter, exit))
                })
                .collect::<Vec<_>>();
            collisions.sort_by(|a, b| {
                a.1.total_cmp(&b.1)
                    .then_with(|| bodies[a.0].key.cmp(&bodies[b.0].key))
            });
            drop(sweep_observe);
            let mut blocked = false;
            let mut last: Option<(u64, f64)> = None;
            for (index, enter, _) in collisions {
                let body = &bodies[index];
                if body.id > 0 && self.target_info(body.id).is_none() {
                    continue;
                }
                if projectile.passedSurfaces.contains(&body.key) {
                    continue;
                }
                let point = mix(before, next, enter);
                // Adjacent pieces of one panel must not double-charge at a shared edge.
                if last.is_some_and(|(id, t)| id == body.id && (t - enter).abs() < EPS) {
                    projectile.passedSurfaces.push(body.key);
                    continue;
                }
                last = Some((body.id, enter));
                let through = projectile.penetration + EPS >= body.resistance
                    && body.resistance > 0.
                    && projectile.penetration > 0.
                    // The intended target receives the remaining payload. Only
                    // intervening cover consumes a fragment of it before arrival.
                    && Some(body.id) != projectile.target;
                if through {
                    let mut fragment = projectile.clone();
                    fragment.damage *= 0.35;
                    fragment.radius = 0.;
                    self.resolve_impact(
                        Impact {
                            projectile: fragment,
                            point,
                            target: if body.id == 0 { None } else { Some(body.id) },
                            level: body.level,
                            splash: false,
                        },
                        &bodies,
                        &spatial,
                    );
                    projectile.damage *= 0.65;
                    projectile.penetration = (projectile.penetration - body.resistance).max(0.);
                    projectile.passedSurfaces.push(body.key);
                } else {
                    projectile.x = point[0];
                    projectile.y = point[1];
                    projectile.z = point[2];
                    self.resolve_impact(
                        Impact {
                            projectile: projectile.clone(),
                            point,
                            target: if body.id == 0 { None } else { Some(body.id) },
                            level: body.level,
                            splash: true,
                        },
                        &bodies,
                        &spatial,
                    );
                    blocked = true;
                    break;
                }
            }
            if blocked || projectile.damage <= 0. {
                continue;
            }
            projectile.x = next[0];
            projectile.y = next[1];
            projectile.z = next[2];
            if arrived {
                self.resolve_impact(
                    Impact {
                        projectile,
                        point: next,
                        target: None,
                        level: aim[2].floor().clamp(-2., 5.) as i32,
                        splash: true,
                    },
                    &bodies,
                    &spatial,
                );
            } else if (-1. ..129.).contains(&next[0])
                && (-1. ..97.).contains(&next[1])
                && (-3. ..=32.).contains(&next[2])
            {
                self.state.projectiles.push(projectile);
            }
        }
    }
    fn intercept_payload(&mut self, p: &mut Projectile, from: Vec3, to: Vec3) -> bool {
        if p.kind == "beam" {
            return false;
        }
        let mut receipts = Vec::new();
        for field in self
            .state
            .defenseFields
            .iter_mut()
            .filter(|f| f.owner != p.owner && f.hp > 0.)
        {
            let crosses = (0..=4).any(|i| {
                let q = mix(from, to, i as f64 / 4.);
                cone(
                    field.pos,
                    field.direction,
                    cell(q, field.pos.level),
                    field.radius,
                    field.angle,
                ) && (q[2] - (field.pos.level as f64 + 0.5)).abs() <= 0.95
            });
            if crosses {
                let amount = field.hp.min(p.damage);
                field.hp -= amount;
                p.damage -= amount;
                receipts.push((field.owner, field.id, amount));
            }
            if p.damage <= 0. {
                break;
            }
        }
        for (owner, id, amount) in receipts {
            self.event(
                "shield-absorb",
                cell(to, p.destination.level),
                owner,
                amount,
                id,
            );
        }
        p.damage <= 0.
    }
    fn jam_projectile(&mut self, p: &mut Projectile) {
        let current = [p.x, p.y, p.z];
        let guard = self.state.rooms.iter().position(|r| {
            r.owner != p.owner
                && r.kind == "network-defense"
                && r.hp > 0.
                && r.progress >= 1.
                && r.connected
                && r.inventory >= 8.
                && point_to_cell_distance(current, r.rect.center()) <= 12.
        });
        if let Some(index) = guard {
            let (owner, id) = (self.state.rooms[index].owner, self.state.rooms[index].id);
            self.state.rooms[index].inventory -= 8.;
            p.jammed = true;
            let mut aim = p
                .aimPosition
                .unwrap_or_else(|| center(p.destination, p.targetAltitude));
            let origin = p.launchPosition.unwrap_or(current);
            let dx = aim[0] - origin[0];
            let dy = aim[1] - origin[1];
            let length = (dx * dx + dy * dy).sqrt().max(0.1);
            let resistance = self
                .state
                .units
                .iter()
                .find(|u| u.id == p.source)
                .map(|u| {
                    modifier(u, "jam-resistance") + self.facility_bonus_for_unit(u, "secure-relay")
                })
                .unwrap_or(0.)
                .clamp(0., 0.75);
            let offset = 2.5 * (1. - resistance) * if p.id % 2 == 0 { 1. } else { -1. };
            aim[0] = (aim[0] - dy / length * offset).clamp(0.05, 127.95);
            aim[1] = (aim[1] + dx / length * offset).clamp(0.05, 95.95);
            p.aimPosition = Some(aim);
            p.destination = cell(aim, p.destination.level);
            p.damage *= 0.85;
            self.event(
                "guidance-jammed",
                cell(current, p.destination.level),
                owner,
                8.,
                id,
            );
        }
    }
    fn resolve_impact(&mut self, impact: Impact, bodies: &Bodies, spatial: &Spatial) {
        let _observe = CombatObservation::new("combat/impact-total");
        let p = &impact.projectile;
        let point = impact.point;
        let where_hit = cell(point, impact.level);
        let mut measured_hp_damage = 0.;
        self.event("projectile-impact", where_hit, p.owner, p.radius, p.id);
        if let Some(event) = self.state.events.last_mut() {
            event.subjectKind = p.kind.clone();
            event.presentationPosition = Some(point);
        }
        if let Some(id) = impact.target {
            let structure = spatial
                .entity(id)
                .next()
                .map(|i| &bodies[*i])
                .is_some_and(Body::structural);
            measured_hp_damage += self.apply_projectile_damage(
                p,
                id,
                p.damage * if structure { p.structureMultiplier } else { 1. },
                where_hit,
            );
        }
        if impact.splash && p.radius > 0. {
            let _splash_observe = CombatObservation::new("combat/splash-total");
            let mut nearby: BTreeMap<u64, (f64, Vec3, usize)> = BTreeMap::new();
            for index in spatial.sphere(point, p.radius) {
                let body = &bodies[index];
                if body.id == 0
                    || body.owner == p.owner
                    || Some(body.id) == impact.target
                    || self.target_info(body.id).is_none()
                {
                    continue;
                }
                let target = body.nearest(point);
                let range = distance(point, target);
                if range >= p.radius {
                    continue;
                }
                let entry = nearby.entry(body.id).or_insert((range, target, index));
                if range < entry.0 {
                    *entry = (range, target, index);
                }
            }
            for (id, (range, target, index)) in nearby {
                let body = &bodies[index];
                let mut attenuation = 1.;
                let mut penetration = p.penetration;
                let mut used = BTreeSet::new();
                let covers_observe = CombatObservation::new("combat/splash-cover");
                let mut covers = spatial
                    .segment(point, target)
                    .into_iter()
                    .filter_map(|blocker| {
                        let cover = &bodies[blocker];
                        if cover.id == id
                            || !cover.opaque()
                            || cover.id > 0 && self.target_info(cover.id).is_none()
                        {
                            return None;
                        }
                        let (enter, exit) = hit(point, target, cover)?;
                        (enter < 1. - EPS && exit > EPS).then_some((enter, blocker))
                    })
                    .collect::<Vec<_>>();
                covers.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));
                for (_, blocker) in covers {
                    let cover = &bodies[blocker];
                    if !used.insert(cover.key) {
                        continue;
                    }
                    if penetration + EPS >= cover.resistance && penetration > 0. {
                        penetration -= cover.resistance;
                        attenuation *= 0.55;
                    } else if cover.floor() {
                        attenuation = 0.;
                        break;
                    } else {
                        attenuation *= 0.25;
                    }
                }
                drop(covers_observe);
                if attenuation <= 0. {
                    continue;
                }
                let damage = p.damage
                    * (1. - range / p.radius)
                    * 0.8
                    * attenuation
                    * if body.structural() {
                        p.structureMultiplier
                    } else {
                        1.
                    };
                measured_hp_damage +=
                    self.apply_projectile_damage(p, id, damage, cell(target, body.level));
            }
        }
        if p.kind == "delayed-area" {
            if let Some(player) = self.player_mut(p.owner) {
                *player
                    .totals
                    .entry("delayed-area-impacts".into())
                    .or_default() += 1.;
                *player
                    .totals
                    .entry("delayed-area-hp-damage".into())
                    .or_default() += measured_hp_damage;
                if measured_hp_damage > 0. {
                    *player
                        .totals
                        .entry("delayed-area-hp-damaging-impacts".into())
                        .or_default() += 1.;
                }
            }
        }
        // A ground splash or friendly obstruction is still visible, but is not recorded as HP damage.
        if impact.target.is_none()
            || impact
                .target
                .and_then(|id| self.target_info(id))
                .is_some_and(|(_, o)| o == p.owner)
        {
            self.event(
                if p.damageType == "energy" {
                    "energy-impact"
                } else if p.damageType == "explosive" {
                    "explosive-impact"
                } else {
                    "impact"
                },
                where_hit,
                p.owner,
                0.,
                p.id,
            );
        }
    }
    fn apply_projectile_damage(
        &mut self,
        projectile: &Projectile,
        id: u64,
        amount: f64,
        point: Pos,
    ) -> f64 {
        let _observe = CombatObservation::new("combat/damage-total");
        let before = self
            .state
            .units
            .iter()
            .find(|u| u.id == id)
            .map(|u| u.hp)
            .or_else(|| {
                self.state
                    .buildings
                    .iter()
                    .find(|b| b.id == id)
                    .map(|b| b.hp)
            })
            .or_else(|| self.state.rooms.iter().find(|r| r.id == id).map(|r| r.hp))
            .or_else(|| self.state.walls.iter().find(|w| w.id == id).map(|w| w.hp))
            .or_else(|| {
                self.state
                    .shipments
                    .iter()
                    .find(|s| s.id == id)
                    .map(|s| s.hp)
            })
            .or_else(|| {
                self.state
                    .entrances
                    .iter()
                    .find(|e| e.id == id)
                    .map(|e| e.hp)
            });
        let moving = self
            .state
            .units
            .iter()
            .any(|u| u.id == id && (u.moving || u.transitProgress > 0.));
        let damage_observe = CombatObservation::new("combat/damage-at");
        self.damage_at(
            id,
            amount
                * if projectile.target == Some(id) {
                    projectile.targetMultiplier
                        * if moving {
                            projectile.movingTargetMultiplier
                        } else {
                            1.
                        }
                } else {
                    1.
                },
            &projectile.damageType,
            projectile.owner,
            Some(point),
        );
        drop(damage_observe);
        // Delayed-area is currently Gemini's warning strike. Count actual HP
        // removed from targets, including lethal hits after entity cleanup;
        // shield-only impacts are deliberately not described as empty casts.
        let measured_hp_damage = if projectile.kind == "delayed-area" {
            let after = self
                .skill_target_hp(id)
                .or_else(|| {
                    self.state
                        .shipments
                        .iter()
                        .find(|s| s.id == id)
                        .map(|s| s.hp)
                })
                .or_else(|| {
                    self.state
                        .entrances
                        .iter()
                        .find(|e| e.id == id)
                        .map(|e| e.hp)
                })
                .unwrap_or(0.)
                .max(0.);
            before
                .map(|hp| (hp.max(0.) - after).clamp(0., hp.max(0.)))
                .unwrap_or(0.)
        } else {
            0.
        };
        if let (Some(hp), Some(status)) = (before, &projectile.onHitStatus) {
            if let Some(unit) = self
                .state
                .units
                .iter_mut()
                .find(|u| u.id == id && u.owner != projectile.owner && u.hp > 0. && u.hp < hp)
            {
                unit.statuses
                    .entry(status.kind.clone())
                    .and_modify(|v| *v = v.max(status.duration))
                    .or_insert(status.duration);
            }
            if status.kind == "anti-heal" {
                if let Some(b) =
                    self.state.buildings.iter_mut().find(|b| {
                        b.id == id && b.hp > 0. && b.hp < hp && b.owner != projectile.owner
                    })
                {
                    b.antiHeal = b.antiHeal.max(status.duration);
                }
                if let Some(r) =
                    self.state.rooms.iter_mut().find(|r| {
                        r.id == id && r.hp > 0. && r.hp < hp && r.owner != projectile.owner
                    })
                {
                    r.antiHeal = r.antiHeal.max(status.duration);
                }
                if let Some(w) =
                    self.state.walls.iter_mut().find(|w| {
                        w.id == id && w.hp > 0. && w.hp < hp && w.owner != projectile.owner
                    })
                {
                    w.antiHeal = w.antiHeal.max(status.duration);
                }
            }
        }
        measured_hp_damage
    }
    fn weapon_targets(&self, owner: u32) -> Vec<WeaponTarget> {
        let _observe = CombatObservation::new("combat/resolve-target-list");
        self.targets(owner)
            .into_iter()
            .filter_map(|(id, _, target_owner)| {
                let (pos, _) = self.target_info_for(id, owner)?;
                if !self.visible_to(owner, pos) {
                    return None;
                }
                let air = self
                    .state
                    .units
                    .iter()
                    .find(|u| u.id == id)
                    .map(airborne)
                    .or_else(|| {
                        self.state
                            .shipments
                            .iter()
                            .find(|s| s.id == id)
                            .map(|s| s.mode == "air" && s.altitude > 0.25)
                    })
                    .unwrap_or(false);
                Some(WeaponTarget {
                    id,
                    pos,
                    owner: target_owner,
                    air,
                })
            })
            .collect()
    }
    fn nearest_weapon_target(
        &self,
        candidates: &[WeaponTarget],
        origin: Vec3,
        def: &catalog::UnitDef,
        bodies: &Bodies,
        spatial: &Spatial,
    ) -> Option<(u64, Pos, u32)> {
        // This is a conservative broad phase only. The union AABB bounds every
        // body of the entity; its distance cannot exceed the true nearest-body
        // distance. Cache it instead of collecting and sorting pieces per gun.
        // Keep entities
        // without a collider (unfinished equipment, corridors, open entries)
        // because the existing aim fallback still makes them legal targets.
        // Small lists cost less to evaluate directly than to index/filter.
        candidates
            .iter()
            .copied()
            .filter(|v| v.permitted(def))
            .filter(|v| {
                candidates.len() <= 32 || spatial.entity_may_be_in_range(v.id, origin, def.range)
            })
            .filter_map(|v| {
                let range = distance(
                    origin,
                    self.target_point_from(v.id, v.pos, origin, bodies, spatial),
                );
                (range <= def.range).then_some((v, range))
            })
            // Iterate the original visible-target list, never a hash table, so
            // equal-distance tie behavior and replay determinism stay intact.
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(v, _)| (v.id, v.pos, v.owner))
    }
    fn moving_collision_bodies(&self) -> (Vec<Body>, HashMap<u64, Vec3>) {
        let _observe = CombatObservation::new("combat/dynamic-bodies");
        let mut bodies = Vec::with_capacity(self.state.units.len() + self.state.shipments.len());
        let mut aims = HashMap::with_capacity(self.state.units.len() + self.state.shipments.len());
        for unit in self.state.units.iter().filter(|u| u.hp > 0.) {
            let z = unit.elevation();
            aims.insert(unit.id, [unit.x, unit.y, z + 0.45]);
            bodies.push(Body {
                id: unit.id,
                owner: unit.owner,
                key: [unit.id, 0],
                min: [unit.x - 0.45, unit.y - 0.45, z + 0.05],
                max: [unit.x + 0.45, unit.y + 0.45, z + 0.85],
                level: unit.level,
                kind: BodyKind::Unit,
                resistance: 0.35,
            });
        }
        for shipment in self.state.shipments.iter().filter(|s| s.hp > 0.) {
            let p = self.target_point(shipment.id, shipment.pos);
            aims.insert(shipment.id, p);
            let (rx, ry) = if shipment.mode == "air" {
                (1.2, 1.)
            } else {
                (0.65, 0.4)
            };
            bodies.push(Body {
                id: shipment.id,
                owner: shipment.owner,
                key: [shipment.id, 0],
                min: [p[0] - rx, p[1] - ry, p[2] - 0.3],
                max: [p[0] + rx, p[1] + ry, p[2] + 0.35],
                level: shipment.pos.level,
                kind: BodyKind::Unit,
                resistance: 0.5,
            });
        }
        (bodies, aims)
    }
    fn static_collision_bodies(&self) -> Vec<Body> {
        let _observe = CombatObservation::new("combat/static-rebuild");
        let mut bodies = Vec::new();
        let shaft = |x: i32, y: i32, a: i32, b: i32| {
            self.state.entrances.iter().find(|e| {
                e.hp > 0.
                    && !matches!(e.kind.as_str(), "door" | "window")
                    && e.serves(a)
                    && e.serves(b)
                    && e.footprint_contains(x, y)
            })
        };
        for building in self.state.buildings.iter().filter(|b| b.hp > 0.) {
            let r = building.rect;
            let z = r.level as f64;
            let progress = building.progress.clamp(0., 1.);
            if building.kind != "shell" {
                let height = if matches!(building.kind.as_str(), "airstrip" | "launch-pad") {
                    0.08
                } else {
                    0.15 + 0.8 * progress
                };
                bodies.push(Body {
                    id: building.id,
                    owner: building.owner,
                    key: [building.id, 0],
                    min: [r.x as f64, r.y as f64, z],
                    max: [(r.x + r.width) as f64, (r.y + r.height) as f64, z + height],
                    level: r.level,
                    kind: BodyKind::Solid,
                    resistance: 1.,
                });
                continue;
            }
            // Hollow shells: individual wall runs and real floor plates, never a filled room-sized cube.
            for side in 0..4 {
                let length = if side < 2 { r.height } else { r.width };
                for n in 0..length {
                    let (x, y) = match side {
                        0 => (r.x, r.y + n),
                        1 => (r.x + r.width - 1, r.y + n),
                        2 => (r.x + n, r.y),
                        _ => (r.x + n, r.y + r.height - 1),
                    };
                    if self.state.entrances.iter().any(|e| {
                        e.owner == building.owner
                            && e.kind == "door"
                            && e.reserves_space()
                            && e.covers(Pos::new(x, y, r.level))
                    }) {
                        continue;
                    }
                    let gate = self.state.entrances.iter().find(|e| {
                        e.owner == building.owner
                            && e.hp > 0.
                            && e.kind == "window"
                            && e.covers(Pos::new(x, y, r.level))
                    });
                    if gate.is_some_and(|e| e.open) {
                        continue;
                    }
                    let (id, owner, key) = gate
                        .map(|e| (e.id, e.owner, [e.id, 20 + side as u64]))
                        .unwrap_or((building.id, building.owner, [building.id, 2 + side as u64]));
                    let (min, max) = match side {
                        0 => (
                            [r.x as f64, y as f64, z + 0.04],
                            [r.x as f64 + 0.22, y as f64 + 1., z + 0.15 + 0.8 * progress],
                        ),
                        1 => (
                            [(r.x + r.width) as f64 - 0.22, y as f64, z + 0.04],
                            [
                                (r.x + r.width) as f64,
                                y as f64 + 1.,
                                z + 0.15 + 0.8 * progress,
                            ],
                        ),
                        2 => (
                            [x as f64, r.y as f64, z + 0.04],
                            [x as f64 + 1., r.y as f64 + 0.22, z + 0.15 + 0.8 * progress],
                        ),
                        _ => (
                            [x as f64, (r.y + r.height) as f64 - 0.22, z + 0.04],
                            [
                                x as f64 + 1.,
                                (r.y + r.height) as f64,
                                z + 0.15 + 0.8 * progress,
                            ],
                        ),
                    };
                    bodies.push(Body {
                        id,
                        owner,
                        key,
                        min,
                        max,
                        level: r.level,
                        kind: BodyKind::Wall,
                        resistance: 0.8 * (0.3 + 0.7 * progress),
                    });
                }
            }
            for ceiling in [false, true] {
                if ceiling && progress < 0.6 {
                    continue;
                }
                let plane = z + if ceiling { 0.98 } else { 0.02 };
                for y in r.y..r.y + r.height {
                    let mut x = r.x;
                    while x < r.x + r.width {
                        let skip = |xx: i32| {
                            if ceiling
                                && self.state.buildings.iter().any(|above| {
                                    above.kind == "shell"
                                        && above.hp > 0.
                                        && above.rect.contains(Pos::new(xx, y, r.level + 1))
                                })
                            {
                                return true;
                            }
                            shaft(
                                xx,
                                y,
                                r.level,
                                if ceiling { r.level + 1 } else { r.level - 1 },
                            )
                            .is_some_and(|e| e.open)
                        };
                        if skip(x) {
                            x += 1;
                            continue;
                        }
                        let start = x;
                        x += 1;
                        while x < r.x + r.width && !skip(x) {
                            x += 1;
                        }
                        let body = Body {
                            id: building.id,
                            owner: building.owner,
                            key: [building.id, if ceiling { 1 } else { 0 }],
                            min: [start as f64, y as f64, plane - 0.07],
                            max: [x as f64, y as f64 + 1., plane + 0.07],
                            level: r.level,
                            kind: BodyKind::Floor,
                            resistance: 1.,
                        };
                        // Merge uniform rows so large floors do not create one collider per microcell.
                        if let Some(last) = bodies.last_mut() {
                            if last.key == body.key
                                && last.kind == body.kind
                                && last.min[0] == body.min[0]
                                && last.max[0] == body.max[0]
                                && last.max[1] == body.min[1]
                                && last.min[2] == body.min[2]
                            {
                                last.max[1] = body.max[1];
                                continue;
                            }
                        }
                        bodies.push(body);
                    }
                }
            }
        }
        for room in self
            .state
            .rooms
            .iter()
            .filter(|r| r.hp > 0. && r.progress >= 0.5 && r.kind != "corridor")
        {
            let r = room.rect;
            let (cx, cy) = (
                r.x as f64 + r.width as f64 / 2.,
                r.y as f64 + r.height as f64 / 2.,
            );
            let radius = if room.kind == "column" {
                0.3
            } else {
                (r.width.min(r.height) as f64 * 0.3).clamp(0.45, 1.2)
            };
            bodies.push(Body {
                id: room.id,
                owner: room.owner,
                key: [room.id, 0],
                min: [cx - radius, cy - radius, r.level as f64 + 0.08],
                max: [
                    cx + radius,
                    cy + radius,
                    r.level as f64 + if room.kind == "column" { 0.95 } else { 0.75 },
                ],
                level: r.level,
                kind: if room.kind == "column" {
                    BodyKind::Solid
                } else {
                    BodyKind::Equipment
                },
                resistance: 0.6,
            });
        }
        for wall in self.state.walls.iter().filter(|w| w.hp > 0.) {
            let id = wall.id;
            let p = wall.pos;
            bodies.push(Body {
                id,
                owner: wall.owner,
                key: [id, 0],
                min: [p.x as f64, p.y as f64, p.level as f64],
                max: [p.x as f64 + 1., p.y as f64 + 1., p.level as f64 + 0.85],
                level: p.level,
                kind: BodyKind::Wall,
                resistance: 0.8,
            });
        }
        // Door openings belong to the shell layout. A live closed leaf is its
        // own whole-cell object, including gates in a freestanding perimeter.
        // Breaking it leaves an opening instead of regenerating the old wall.
        for door in self
            .state
            .entrances
            .iter()
            .filter(|e| e.kind == "door" && e.hp > 0. && !e.open)
        {
            let (w, h) = if door.axis == "y" {
                (1., door.width as f64)
            } else {
                (door.width as f64, 1.)
            };
            bodies.push(Body {
                id: door.id,
                owner: door.owner,
                key: [door.id, 20],
                min: [
                    door.pos.x as f64,
                    door.pos.y as f64,
                    door.pos.level as f64 + 0.04,
                ],
                max: [
                    door.pos.x as f64 + w,
                    door.pos.y as f64 + h,
                    door.pos.level as f64 + 0.95,
                ],
                level: door.pos.level,
                kind: BodyKind::Wall,
                resistance: 0.8,
            });
        }
        for link in self.state.links.iter().filter(|l| l.hp > 0.) {
            for p in &link.path {
                bodies.push(Body {
                    id: link.id,
                    owner: link.owner,
                    key: [link.id, ((p.level + 2) * 128 * 96 + p.y * 128 + p.x) as u64],
                    min: [p.x as f64 + 0.4, p.y as f64 + 0.4, p.level as f64 + 0.02],
                    max: [p.x as f64 + 0.6, p.y as f64 + 0.6, p.level as f64 + 0.18],
                    level: p.level,
                    kind: BodyKind::Cable,
                    resistance: 0.1,
                });
            }
        }
        // Closed vertical hatches cover their shaft. Open shafts remain real holes in floor plates.
        for entry in self
            .state
            .entrances
            .iter()
            .filter(|e| e.hp > 0. && !e.open && !matches!(e.kind.as_str(), "door" | "window"))
        {
            for level in entry.pos.level.min(entry.toLevel)..=entry.pos.level.max(entry.toLevel) {
                let (w, h) = if entry.axis == "x" {
                    (entry.width as f64, 1.)
                } else {
                    (1., entry.width as f64)
                };
                bodies.push(Body {
                    id: entry.id,
                    owner: entry.owner,
                    key: [entry.id, 100 + (level + 2) as u64],
                    min: [entry.pos.x as f64, entry.pos.y as f64, level as f64 - 0.07],
                    max: [
                        entry.pos.x as f64 + w,
                        entry.pos.y as f64 + h,
                        level as f64 + 0.1,
                    ],
                    level,
                    kind: BodyKind::Floor,
                    resistance: 1.,
                });
            }
        }
        for (i, kind) in self.state.terrain.iter().enumerate() {
            if *kind != 1 {
                continue;
            }
            let (x, y) = ((i % 128) as f64, (i / 128) as f64);
            bodies.push(Body {
                id: 0,
                owner: 0,
                key: [0, 200_000 + i as u64],
                min: [x, y, 0.],
                max: [x + 1., y + 1., 0.9],
                level: 0,
                kind: BodyKind::Terrain,
                resistance: 1.2,
            });
        }
        for p in &self.state.excavated {
            if p.level >= 0 {
                continue;
            }
            let upper = p.level + 1;
            let roof_present = self.state.buildings.iter().any(|b| {
                b.hp > 0.
                    && b.kind == "shell"
                    && ((b.progress >= 0.6 && b.rect.contains(*p))
                        || b.rect.contains(Pos::new(p.x, p.y, upper)))
            });
            if roof_present || shaft(p.x, p.y, p.level, upper).is_some_and(|e| e.open) {
                continue;
            }
            bodies.push(Body {
                id: 0,
                owner: 0,
                key: [
                    0,
                    300_000 + ((upper + 2) * 128 * 96 + p.y * 128 + p.x) as u64,
                ],
                min: [p.x as f64, p.y as f64, upper as f64 - 0.08],
                max: [p.x as f64 + 1., p.y as f64 + 1., upper as f64 + 0.08],
                level: upper,
                kind: BodyKind::Terrain,
                resistance: 1.,
            });
        }
        bodies
    }
}

#[cfg(test)]
mod geometry_tests {
    use super::*;
    #[test]
    fn ranged_target_prefilter_matches_brute_force_and_keeps_missing_colliders() {
        // These candidates represent the already validated visible-target list.
        // Compare the optimized selector to its original exhaustive distance
        // rule on >32 targets so the broad-phase path is actually exercised.
        let game = Game::new(96, false);
        let mut raw = vec![Body {
            id: 1000,
            owner: 2,
            key: [1000, 0],
            min: [20., 20., 0.],
            max: [44., 44., 1.],
            level: 0,
            kind: BodyKind::Solid,
            resistance: 1.,
        }];
        let mut candidates = vec![WeaponTarget {
            id: 1000,
            pos: Pos::new(32, 32, 0),
            owner: 2,
            air: false,
        }];
        for n in 1..81 {
            let x = (80 + n * 17 % 45) as f64;
            let y = (60 + n * 11 % 34) as f64;
            let z = (n % 6) as f64;
            raw.push(Body {
                id: 1000 + n as u64,
                owner: 2,
                key: [1000 + n as u64, 0],
                min: [x, y, z],
                max: [x + 1.5, y + 1.25, z + 0.85],
                level: z as i32,
                kind: BodyKind::Solid,
                resistance: 1.,
            });
            candidates.push(WeaponTarget {
                id: 1000 + n as u64,
                pos: Pos::new(x as i32, y as i32, z as i32),
                owner: 2,
                air: n % 7 == 0,
            });
        }
        let spatial = Spatial::new(&raw);
        let bodies = Bodies {
            fixed: Arc::new(raw),
            moving: vec![],
            aims: HashMap::new(),
            snapshot_aims: true,
        };
        let mut def = catalog::unit_ref("vscode").unwrap().clone();
        def.range = 3.5;
        assert_eq!(
            game.nearest_weapon_target(&candidates, [16.5, 22.5, 0.5], &def, &bodies, &spatial)
                .map(|v| v.0),
            Some(1000),
            "visible near facade remains targetable when its center is far away"
        );
        candidates.push(WeaponTarget {
            id: 999,
            pos: Pos::new(17, 22, 0),
            owner: 2,
            air: false,
        });
        assert_eq!(
            game.nearest_weapon_target(&candidates, [16.5, 22.5, 0.5], &def, &bodies, &spatial)
                .map(|v| v.0),
            Some(999),
            "an existing no-collider aim fallback must survive the spatial filter"
        );
        for n in 0..400 {
            let origin = [
                ((n * 23) % 128) as f64 + 0.49,
                ((n * 17) % 96) as f64 + 0.51,
                (n % 8) as f64 - 1.5,
            ];
            def.range = [0., 1., 4., 8., 12., 32., 64.][n % 7];
            let expected = candidates
                .iter()
                .copied()
                .filter(|v| v.permitted(&def))
                .filter_map(|v| {
                    let range = distance(
                        origin,
                        game.target_point_from(v.id, v.pos, origin, &bodies, &spatial),
                    );
                    (range <= def.range).then_some((v, range))
                })
                .min_by(|a, b| a.1.total_cmp(&b.1))
                .map(|(v, _)| (v.id, v.pos, v.owner));
            assert_eq!(
                game.nearest_weapon_target(&candidates, origin, &def, &bodies, &spatial),
                expected,
                "query {n}"
            );
        }
    }
    #[test]
    fn cached_sightlines_follow_terrain_excavation_hatches_and_wall_edits() {
        let mut g = Game::new(77, false);
        g.state.terrain.fill(0);
        g.state.buildings.clear();
        g.state.rooms.clear();
        g.state.walls.clear();
        g.state.entrances.clear();
        g.state.links.clear();
        g.state.excavated.clear();
        let ray = ([10.5, 10.5, 0.5], [14.5, 10.5, 0.5]);
        assert!(g.point_line_clear(ray.0, ray.1));
        let fixed = Arc::clone(&g.collision_cache.borrow().fixed);
        g.state.players[0].credits += 100.;
        assert!(g.point_line_clear(ray.0, ray.1));
        assert!(
            Arc::ptr_eq(&fixed, &g.collision_cache.borrow().fixed),
            "resource changes do not rebuild collision geometry"
        );
        g.state.terrain[10 * 128 + 12] = 1;
        assert!(!g.point_line_clear(ray.0, ray.1));
        g.state.terrain[10 * 128 + 12] = 0;
        assert!(g.point_line_clear(ray.0, ray.1));
        let vertical = ([12.5, 10.5, -1.5], [12.5, 10.5, 0.5]);
        assert!(g.point_line_clear(vertical.0, vertical.1));
        g.state.excavated.insert(Pos::new(12, 10, -1));
        assert!(
            !g.point_line_clear(vertical.0, vertical.1),
            "new basement earth roof blocks the ray"
        );
        g.state.entrances.push(Entrance {
            id: 501,
            owner: 1,
            pos: Pos::new(12, 10, -1),
            toLevel: 0,
            kind: "stairs".into(),
            hp: 250.,
            open: true,
            powered: false,
            width: 1,
            axis: "x".into(),
        });
        assert!(
            g.point_line_clear(vertical.0, vertical.1),
            "open shaft is a real roof hole"
        );
        g.state.entrances[0].open = false;
        assert!(
            !g.point_line_clear(vertical.0, vertical.1),
            "closed hatch restores physical cover"
        );
        g.state.entrances[0].hp = 0.;
        assert!(
            !g.point_line_clear(vertical.0, vertical.1),
            "destroyed shaft no longer cuts an earth opening"
        );
    }
    #[test]
    fn cached_construction_height_tracks_progress_but_not_positive_hp() {
        let mut g = Game::new(78, false);
        g.state.terrain.fill(0);
        g.state.rooms.clear();
        g.state.links.clear();
        let b = &mut g.state.buildings[0];
        b.kind = "wind-power".into();
        b.rect = Rect {
            x: 12,
            y: 10,
            level: 0,
            width: 2,
            height: 2,
        };
        b.progress = 0.1;
        let ray = ([10.5, 10.5, 0.7], [16.5, 10.5, 0.7]);
        assert!(g.point_line_clear(ray.0, ray.1));
        let fixed = Arc::clone(&g.collision_cache.borrow().fixed);
        g.state.buildings[0].hp -= 1.;
        assert!(g.point_line_clear(ray.0, ray.1));
        assert!(Arc::ptr_eq(&fixed, &g.collision_cache.borrow().fixed));
        g.state.buildings[0].progress = 1.;
        assert!(!g.point_line_clear(ray.0, ray.1));
        g.state.buildings[0].hp = 0.;
        assert!(g.point_line_clear(ray.0, ray.1));
    }
    #[test]
    fn overlay_broad_phase_matches_full_sweep_with_deterministic_candidate_order() {
        let mut bodies = Vec::new();
        for i in 0..120 {
            let x = ((i * 37) % 127) as f64;
            let y = ((i * 19) % 95) as f64;
            let z = (i % 8) as f64 - 2.;
            bodies.push(Body {
                id: i as u64 + 1,
                owner: 1,
                key: [i as u64 + 1, 0],
                min: [x, y, z],
                max: [x + 1.5, y + 1.25, z + 0.85],
                level: z as i32,
                kind: BodyKind::Solid,
                resistance: 1.,
            });
        }
        let base = Arc::new(Spatial::new(&bodies[..80]));
        let overlay = Spatial::overlay(&bodies[80..], 80, Some(base));
        for n in 0..400 {
            let a = [
                ((n * 23) % 129) as f64 - 0.5,
                ((n * 17) % 97) as f64 - 0.5,
                (n % 10) as f64 - 2.5,
            ];
            let b = [
                ((n * 71 + 5) % 129) as f64 - 0.5,
                ((n * 43 + 3) % 97) as f64 - 0.5,
                ((n * 3 + 1) % 10) as f64 - 2.5,
            ];
            let candidates = overlay.segment(a, b);
            assert!(candidates.windows(2).all(|w| w[0] < w[1]));
            let actual: Vec<_> = candidates
                .into_iter()
                .filter(|i| hit(a, b, &bodies[*i]).is_some())
                .collect();
            let expected: Vec<_> = bodies
                .iter()
                .enumerate()
                .filter_map(|(i, body)| hit(a, b, body).map(|_| i))
                .collect();
            assert_eq!(actual, expected, "ray {n}");
            let radius = 6.;
            let nearby = overlay.sphere(a, radius);
            for (i, body) in bodies.iter().enumerate() {
                if distance(a, body.nearest(a)) <= radius {
                    assert!(nearby.contains(&i));
                }
            }
        }
    }
    #[test]
    #[ignore = "Explicit component benchmark against the unchanged saved full pressure fixture, not full tick/GPU acceptance"]
    fn collision_cache_full_fixture_component_benchmark() {
        use std::{hint::black_box, time::Instant};
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../Logs/v6/pressure/pressure-save.json");
        let save: Save = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
        let g = Game::load(save).unwrap();
        assert_eq!(
            g.state
                .buildings
                .iter()
                .filter(|b| b.kind == "shell")
                .count(),
            128
        );
        assert_eq!(g.state.rooms.len(), 512);
        assert_eq!(g.state.units.len(), 200);
        assert_eq!(g.state.projectiles.len(), 600);
        let fresh = g.static_collision_bodies();
        let (cached, _) = g.collision_scene();
        assert_eq!(cached.fixed.as_ref(), &fresh);
        let begin = Instant::now();
        for _ in 0..20 {
            let bodies = g.static_collision_bodies();
            let fixed_index = Arc::new(Spatial::new(&bodies));
            let (moving, aims) = g.moving_collision_bodies();
            black_box(Spatial::overlay(&moving, bodies.len(), Some(fixed_index)));
            black_box(Bodies {
                fixed: Arc::new(bodies),
                moving,
                aims,
                snapshot_aims: true,
            });
        }
        let cold = begin.elapsed().as_secs_f64() * 1000.;
        let begin = Instant::now();
        for _ in 0..20 {
            black_box(g.collision_scene());
        }
        let warm = begin.elapsed().as_secs_f64() * 1000.;
        println!(
            "{}",
            serde_json::json!({"component":"collision-geometry-and-index","iterations":20,"shells":128,"rooms":512,"units":200,"projectiles":600,"coldMilliseconds":cold,"cachedMilliseconds":warm,"speedup":cold/warm,"fullTickAcceptance":false})
        );
    }
    #[test]
    #[ignore = "Exact-target equivalence and selection-only timing on the original full pressure save; not full tick acceptance"]
    fn ranged_target_full_fixture_component_benchmark() {
        use std::{hint::black_box, time::Instant};
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../Logs/v6/pressure/pressure-save.json");
        let save: Save = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
        let g = Game::load(save).unwrap();
        assert_eq!(g.state.rooms.len(), 512);
        assert_eq!(g.state.units.len(), 200);
        assert_eq!(g.state.projectiles.len(), 600);
        let (bodies, spatial) = g.collision_scene();
        let lists: HashMap<_, _> = [1, 2]
            .into_iter()
            .map(|owner| (owner, g.weapon_targets(owner)))
            .collect();
        let original = || {
            g.state
                .units
                .iter()
                .map(|u| {
                    let origin = [u.x, u.y, u.elevation() + 0.5];
                    let def = catalog::unit_ref(&u.kind).unwrap();
                    lists[&u.owner]
                        .iter()
                        .copied()
                        .filter(|v| v.permitted(def))
                        .filter_map(|v| {
                            let range = distance(
                                origin,
                                g.target_point_from(v.id, v.pos, origin, &bodies, &spatial),
                            );
                            (range <= def.range).then_some((v, range))
                        })
                        .min_by(|a, b| a.1.total_cmp(&b.1))
                        .map(|(v, _)| (v.id, v.pos, v.owner))
                })
                .collect::<Vec<_>>()
        };
        let indexed = || {
            g.state
                .units
                .iter()
                .map(|u| {
                    g.nearest_weapon_target(
                        &lists[&u.owner],
                        [u.x, u.y, u.elevation() + 0.5],
                        catalog::unit_ref(&u.kind).unwrap(),
                        &bodies,
                        &spatial,
                    )
                })
                .collect::<Vec<_>>()
        };
        assert_eq!(original(), indexed());
        let before = Instant::now();
        for _ in 0..3 {
            black_box(original());
        }
        let exhaustive = before.elapsed().as_secs_f64() * 1000.;
        let before = Instant::now();
        for _ in 0..3 {
            black_box(indexed());
        }
        let filtered = before.elapsed().as_secs_f64() * 1000.;
        println!(
            "{}",
            serde_json::json!({"component":"visible-target-selection","queries":600,"units":200,"rooms":512,"projectiles":600,"candidateCounts":[lists[&1].len(),lists[&2].len()],"allTargetsIdentical":true,"exhaustiveMilliseconds":exhaustive,"prefilteredMilliseconds":filtered,"speedup":exhaustive/filtered,"fullTickAcceptance":false})
        );
    }
    #[test]
    fn slab_sweep_catches_fast_rounds_and_rejects_parallel_misses() {
        let b = Body {
            id: 1,
            owner: 2,
            key: [1, 0],
            min: [5., 4., 0.],
            max: [5.2, 5., 1.],
            level: 0,
            kind: BodyKind::Wall,
            resistance: 1.,
        };
        let range = hit([0., 4.5, 0.5], [12., 4.5, 0.5], &b).unwrap();
        assert!((range.0 - 5. / 12.).abs() < 1e-8);
        assert!(range.1 > range.0);
        assert!(hit([0., 5.2, 0.5], [12., 5.2, 0.5], &b).is_none());
    }
    #[test]
    fn broad_phase_visits_short_bucket_corner_crossings_in_both_directions() {
        let body = Body {
            id: 1,
            owner: 2,
            key: [1, 0],
            min: [7.94, 8.03, 0.],
            max: [7.98, 8.11, 1.],
            level: 0,
            kind: BodyKind::Wall,
            resistance: 1.,
        };
        let a = [7.7, 7.6, 0.5];
        let b = [8.7, 9.4, 0.5];
        assert!(hit(a, b, &body).is_some());
        let spatial = Spatial::new(&[body]);
        assert!(spatial.segment(a, b).contains(&0));
        assert!(spatial.segment(b, a).contains(&0));
    }
    #[test]
    fn broad_phase_visits_exact_three_axis_corner_neighbors() {
        let body = Body {
            id: 1,
            owner: 2,
            key: [1, 0],
            min: [8., 7.5, 0.5],
            max: [8.2, 8., 1.],
            level: 0,
            kind: BodyKind::Wall,
            resistance: 1.,
        };
        let a = [7., 7., 0.];
        let b = [9., 9., 2.];
        assert!(hit(a, b, &body).is_some());
        assert!(Spatial::new(&[body]).segment(a, b).contains(&0));
    }
    #[test]
    fn cargo_aircraft_use_actual_altitude_for_weapon_permissions_and_impacts() {
        // Drive only the physics subsystem here; physical logistics is tested
        // separately with its own valid airport/stock/flight scenarios.
        let mut game = Game::new(82, false);
        game.state.units.clear();
        game.state.rooms.clear();
        game.state.links.clear();
        game.state.projectiles.clear();
        game.state.walls.clear();
        game.state.terrain.fill(0);
        let shipment: Shipment = serde_json::from_value(serde_json::json!({
            "id":501,"owner":2,"from":0,"to":0,"pos":{"x":30,"y":45,"z":0},
            "route":[],"amount":40.,"hp":260.,"progress":0.,"cargo":"ammo",
            "mode":"air","altitude":2.,"flightState":"cruising","fuel":30.,"fuelMax":40.,"flightTimer":0.
        })).unwrap();
        game.state.shipments.push(shipment);
        let tank = catalog::units()
            .into_iter()
            .find(|d| d.chassis == "tank")
            .unwrap();
        let aa = catalog::units()
            .into_iter()
            .find(|d| d.chassis == "aa-launcher")
            .unwrap();
        assert!(!game.valid_weapon_target(&tank, 501));
        assert!(game.valid_weapon_target(&aa, 501));
        game.state.shipments[0].altitude = 0.;
        assert!(game.valid_weapon_target(&tank, 501));
        game.state.shipments[0].altitude = 2.;
        let body = game
            .collision_bodies()
            .into_iter()
            .find(|b| b.id == 501)
            .unwrap();
        assert!(hit([20.5, 45.5, 0.5], [32.5, 45.5, 0.5], &body).is_none());
        assert!(hit([20.5, 45.5, 2.35], [32.5, 45.5, 2.35], &body).is_some());
        game.state.projectiles.push(serde_json::from_value(serde_json::json!({
            "id":502,"owner":1,"source":500,"target":501,
            "origin":{"x":20,"y":45,"z":0},"destination":{"x":30,"y":45,"z":0},
            "x":20.5,"y":45.5,"z":2.35,"age":0.,"duration":1.,"damage":100.,"radius":0.,
            "kind":"direct","damageType":"kinetic","penetration":0.,"launchPosition":[20.5,45.5,2.35],"aimPosition":[30.5,45.5,2.35]
        })).unwrap());
        for _ in 0..70 {
            game.advance_combat();
        }
        assert!((game.state.shipments[0].hp - 160.).abs() < 0.001);
    }
}
