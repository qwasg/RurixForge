use crate::{types::*, Game};
use std::{
    collections::{BTreeSet, HashMap, VecDeque},
    sync::Arc,
};
#[derive(Clone, PartialEq, Eq)]
struct FogKey {
    terrain: Vec<u8>,
    shells: Vec<(Rect, bool)>,
    walls: Vec<Pos>,
    entrances: Vec<(Pos, i32, u32, bool, bool, String, String)>,
}

#[cfg(test)]
mod cache_contracts {
    use super::*;
    use serde_json::json;
    fn scene() -> Game {
        let mut g = Game::new(32, false);
        g.state.terrain.fill(0);
        g.next_id = 2000;
        let mut shell = g.state.buildings[0].clone();
        shell.kind = "shell".into();
        shell.rect = Rect {
            x: 20,
            y: 40,
            level: 0,
            width: 12,
            height: 12,
        };
        shell.hp = 1000.;
        shell.maxHp = 1000.;
        shell.progress = 1.;
        shell.id = 100;
        g.state.buildings.push(shell.clone());
        shell.id = 101;
        shell.rect.level = 1;
        g.state.buildings.push(shell);
        g.state.entrances.push(Entrance {
            id: 102,
            owner: 1,
            pos: Pos::new(25, 51, 0),
            toLevel: 0,
            kind: "door".into(),
            hp: 100.,
            open: false,
            powered: false,
            width: 2,
            axis: "x".into(),
        });
        g.state.entrances.push(Entrance {
            id: 103,
            owner: 1,
            pos: Pos::new(25, 45, 0),
            toLevel: 1,
            kind: "elevator".into(),
            hp: 100.,
            open: true,
            powered: true,
            width: 2,
            axis: "x".into(),
        });
        for n in 0..6 {
            let z = n % 2;
            let p = Pos::new(24 + n, 46, z);
            let u:Unit=serde_json::from_value(json!({"id":200+n,"owner":1+n%2,"kind":if n%2==0{"deepseek"}else{"kimi"},"pos":p,"x":p.x as f64+0.5,"y":p.y as f64+0.5,"z":z,"tier":2,"hp":1000.,"maxHp":1000.,"battery":120.,"batteryMax":240.,"covered":false,"wired":false,"ammo":0.,"route":[],"target":null,"cooldown":0.,"skillCooldown":0.,"plugins":[],"statuses":{},"invested":0.,"moving":false,"attackCount":0})).unwrap();
            g.state.units.push(u);
        }
        g
    }
    #[test]
    fn cached_fog_matches_reference_across_movement_doors_refill_and_floor_changes() {
        let mut g = scene();
        for change in 0..12 {
            match change {
                2 => {
                    g.state.units[0].pos.x += 1;
                    g.state.units[0].x += 1.;
                }
                3 => g.state.entrances[0].open = true,
                4 => g.state.walls.push(Wall {
                    id: 104,
                    owner: 1,
                    pos: Pos::new(25, 51, 0),
                    kind: "physical".into(),
                    hp: 100.,
                    maxHp: 100.,
                    shield: 0.,
                    antiHeal: 0.,
                    invested: 0.,
                }),
                5 => g.state.walls.clear(),
                6 => {
                    g.state
                        .buildings
                        .iter_mut()
                        .find(|b| b.id == 100)
                        .unwrap()
                        .hp = 0.
                }
                7 => {
                    g.state
                        .buildings
                        .iter_mut()
                        .find(|b| b.id == 101)
                        .unwrap()
                        .progress = 0.5
                }
                8 => g.state.terrain[47 * 128 + 26] = 1,
                9 => g.state.units[0].plugins.push("algorithm-support".into()),
                10 => g.state.entrances[1].open = false,
                11 => g.state.units[1].altitude = 1.3,
                _ => {}
            }
            let mut reference = g.clone();
            reference.refresh_fog_reference();
            g.refresh_fog();
            assert_eq!(
                g.state.visible, reference.state.visible,
                "visible differs at transition {change}"
            );
            assert_eq!(
                g.state.explored, reference.state.explored,
                "explored differs at transition {change}"
            );
            assert!(g.fog_cache.borrow().indices <= 1_000_000);
        }
    }
}
impl FogKey {
    fn of(s: &Snapshot) -> Self {
        Self {
            terrain: s.terrain.clone(),
            shells: s
                .buildings
                .iter()
                .filter(|b| b.hp > 0. && b.kind == "shell")
                .map(|b| (b.rect, b.progress >= 1.))
                .collect(),
            walls: s
                .walls
                .iter()
                .filter(|w| w.hp > 0.)
                .map(|w| w.pos)
                .collect(),
            entrances: s
                .entrances
                .iter()
                .filter(|e| e.hp > 0. || e.kind == "door")
                .map(|e| {
                    (
                        e.pos,
                        e.toLevel,
                        e.width,
                        e.hp > 0.,
                        e.open,
                        e.kind.clone(),
                        e.axis.clone(),
                    )
                })
                .collect(),
        }
    }
}
struct Occlusion {
    opaque: Vec<bool>,
    plates: Vec<bool>,
    covered: Vec<i32>,
}
type SourceKey = (Pos, u64, u64);
#[derive(Clone, Default)]
pub(crate) struct FogCache {
    key: Option<FogKey>,
    occlusion: Option<Arc<Occlusion>>,
    sources: HashMap<SourceKey, Arc<Vec<usize>>>,
    queue: VecDeque<SourceKey>,
    indices: usize,
}
impl FogCache {
    fn ensure(&mut self, s: &Snapshot) {
        let key = FogKey::of(s);
        if self.key.as_ref() == Some(&key) {
            return;
        }
        let mut opaque = vec![false; 128 * 96 * 8];
        let mut plates = opaque.clone();
        let mut holes = opaque.clone();
        let mut walls = opaque.clone();
        let mut covered = vec![-2; 128 * 96];
        for i in 0..128 * 96 {
            if s.terrain[i] == 1 {
                opaque[2 * 128 * 96 + i] = true;
            }
        }
        for w in s.walls.iter().filter(|w| w.hp > 0.) {
            opaque[index(w.pos)] = true;
            walls[index(w.pos)] = true;
        }
        // Exact same floor-hole rule as the uncached implementation, evaluated once per shaft.
        for e in s.entrances.iter().filter(|e| {
            e.hp > 0. && e.open && matches!(e.kind.as_str(), "stairs" | "elevator" | "ramp")
        }) {
            for p in e.cells() {
                holes[index(p)] = true;
            }
        }
        for b in s
            .buildings
            .iter()
            .filter(|b| b.hp > 0. && b.kind == "shell")
        {
            let r = b.rect;
            for p in r.cells() {
                if r.level > 0 && b.progress >= 1. && !holes[index(p)] {
                    plates[index(p)] = true;
                    covered[(p.y * 128 + p.x) as usize] =
                        covered[(p.y * 128 + p.x) as usize].max(p.level);
                }
                if p.x == r.x || p.y == r.y || p.x == r.x + r.width - 1 || p.y == r.y + r.height - 1
                {
                    opaque[index(p)] = true;
                }
            }
        }
        for e in &s.entrances {
            if e.kind == "door" && e.hp > 0. && !e.open {
                for p in e.cells() {
                    opaque[index(p)] = true;
                }
            }
            if (e.hp > 0. || e.kind == "door")
                && e.open
                && matches!(e.kind.as_str(), "door" | "window")
            {
                for p in e.cells() {
                    if !walls[index(p)]
                        && !(p.level == 0 && s.terrain[(p.y * 128 + p.x) as usize] == 1)
                    {
                        opaque[index(p)] = false;
                    }
                }
            }
        }
        self.key = Some(key);
        self.occlusion = Some(Arc::new(Occlusion {
            opaque,
            plates,
            covered,
        }));
        self.sources.clear();
        self.queue.clear();
        self.indices = 0;
    }
    fn source(&mut self, p: Pos, eye: f64, radius: f64) -> Arc<Vec<usize>> {
        let key = (p, eye.to_bits(), radius.to_bits());
        if let Some(v) = self.sources.get(&key) {
            return v.clone();
        }
        let o = self.occlusion.as_ref().unwrap();
        let reach = radius.ceil() as i32;
        let mut seen = vec![false; 128 * 96 * 8];
        let mut output = vec![];
        let mut planes = vec![p.level];
        if p.level > 0 {
            planes.push(0);
        }
        for z in planes {
            let start = Pos::new(p.x, p.y, z);
            let a = index(start);
            seen[a] = true;
            output.push(a);
            for side in 0..4 {
                for offset in -reach..=reach {
                    let end = match side {
                        0 => Pos::new(p.x - reach, p.y + offset, z),
                        1 => Pos::new(p.x + reach, p.y + offset, z),
                        2 => Pos::new(p.x + offset, p.y - reach, z),
                        _ => Pos::new(p.x + offset, p.y + reach, z),
                    };
                    let steps = (end.x - start.x).abs().max((end.y - start.y).abs());
                    let mut previous_height = eye;
                    for i in 1..=steps {
                        let q = Pos::new(
                            start.x + (end.x - start.x) * i / steps,
                            start.y + (end.y - start.y) * i / steps,
                            z,
                        );
                        if !q.valid()
                            || ((q.x - p.x).pow(2) + (q.y - p.y).pow(2)) as f64 > radius * radius
                        {
                            break;
                        }
                        let ray_height = eye + (z as f64 + 0.5 - eye) * i as f64 / steps as f64;
                        let solid_floor = ((ray_height.floor() as i32 + 1)
                            ..=previous_height.floor() as i32)
                            .any(|floor| {
                                (-2..=5).contains(&floor)
                                    && o.plates[index(Pos::new(q.x, q.y, floor))]
                            });
                        if solid_floor {
                            break;
                        }
                        let k = index(q);
                        if (z == p.level || o.covered[(q.y * 128 + q.x) as usize] <= z) && !seen[k]
                        {
                            seen[k] = true;
                            output.push(k);
                        }
                        let ray_cell = Pos::new(q.x, q.y, (ray_height.floor() as i32).clamp(-2, 5));
                        if o.opaque[index(ray_cell)] {
                            break;
                        }
                        previous_height = ray_height;
                    }
                }
            }
        }
        let output = Arc::new(output);
        const BUDGET: usize = 1_000_000;
        if output.len() <= BUDGET {
            while self.indices + output.len() > BUDGET {
                if let Some(old) = self.queue.pop_front() {
                    if let Some(v) = self.sources.remove(&old) {
                        self.indices -= v.len();
                    }
                } else {
                    break;
                }
            }
            self.indices += output.len();
            self.sources.insert(key, output.clone());
            self.queue.push_back(key);
        }
        output
    }
}
fn index(p: Pos) -> usize {
    ((p.level + 2) * 128 * 96 + p.y * 128 + p.x) as usize
}
impl Game {
    pub fn visible_building_edge(&self, viewer: u32, rect: Rect) -> Option<Pos> {
        (rect.x..rect.x + rect.width)
            .flat_map(|x| {
                [
                    Pos::new(x, rect.y, rect.level),
                    Pos::new(x, rect.y + rect.height - 1, rect.level),
                ]
            })
            .chain((rect.y..rect.y + rect.height).flat_map(|y| {
                [
                    Pos::new(rect.x, y, rect.level),
                    Pos::new(rect.x + rect.width - 1, y, rect.level),
                ]
            }))
            .find(|p| self.visible_to(viewer, *p))
    }
    /// Exact existing construction visibility, shared with ordinary-command planning.
    pub(crate) fn construction_visible_to(&self, owner: u32, p: Pos) -> bool {
            self.visible_to(owner, p)
                || p.level != 0
                    && self.visible_to(owner, Pos::new(p.x, p.y, 0))
                    && self.state.buildings.iter().any(|b| {
                        b.owner == owner
                            && b.hp > 0.
                            && b.rect.contains(Pos::new(
                                p.x,
                                p.y,
                                if p.level > 0 { p.level - 1 } else { 0 },
                            ))
                    })
    }
    pub fn preflight_site(&self, owner: u32, c: &Command) -> Result<(), String> {
        let known = |p: Pos| self.construction_visible_to(owner, p);
        let rect = match c {
            Command::Shell { rect }
            | Command::Excavate { rect }
            | Command::ExpandShell { rect, .. } => Some(*rect),
            Command::Build { pos, kind } => crate::catalog::outdoor_building(kind).map(|b| Rect {
                x: pos.x,
                y: pos.y,
                level: pos.level,
                width: b.width as i32,
                height: b.height as i32,
            }),
            _ => None,
        };
        if let Some(r) = rect {
            if !r.valid() {
                return Err("地图范围无效".into());
            }
            if !r
                .cells()
                .iter()
                .all(|p| known(*p) || p.level < 0 && self.visible_to(owner, Pos::new(p.x, p.y, 0)))
            {
                return Err("需要先侦察此处".into());
            }
        }
        if let Command::Wire { path, .. } | Command::Wall { path, .. } = c {
            if !path.iter().all(|p| known(*p)) {
                return Err("需要先侦察此处".into());
            }
        }
        Ok(())
    }
    pub fn visible_to(&self, owner: u32, p: Pos) -> bool {
        (1..=2).contains(&owner) && self.state.visible[(owner - 1) as usize].contains(&p)
    }
    pub fn refresh_fog(&mut self) {
        let phase = std::time::Instant::now();
        let mut cache = self.fog_cache.borrow_mut();
        cache.ensure(&self.state);
        crate::profiling::record_substage("fog/topology", phase.elapsed().as_secs_f64() * 1000.);
        for owner in 1..=2 {
            let phase = std::time::Instant::now();
            let mut sources = std::collections::BTreeMap::new();
            for (p, base, eye) in self
                .state
                .buildings
                .iter()
                .filter(|b| b.owner == owner && b.hp > 0. && b.progress > 0.)
                .map(|b| {
                    (
                        b.rect.center(),
                        if b.kind == "core" {
                            25.
                        } else if b.progress < 1. {
                            6.
                        } else {
                            14.
                        },
                        b.rect.level as f64 + 0.7,
                    )
                })
                .chain(
                    self.state
                        .units
                        .iter()
                        .filter(|u| u.owner == owner && u.hp > 0.)
                        .map(|u| (u.pos, crate::traits::vision_radius(u), u.elevation() + 0.7)),
                )
                .chain(
                    self.state
                        .jobs
                        .iter()
                        .filter(|j| j.owner == owner && !j.blocked)
                        .map(|j| (j.worker, 6., j.worker.level as f64 + 0.7)),
                )
            {
                sources
                    .entry((p, eye.to_bits()))
                    .and_modify(|r: &mut f64| *r = (*r).max(base))
                    .or_insert(base);
            }
            let mut bits = vec![false; 128 * 96 * 8];
            for ((p, eye), base) in sources {
                let radius =
                    base + p.level.max(0) as f64 * 2. + if self.terrain(p) == 4 { 3. } else { 0. };
                for i in cache.source(p, f64::from_bits(eye), radius).iter() {
                    bits[*i] = true;
                }
            }
            crate::profiling::record_substage(
                "fog/sources-and-traces",
                phase.elapsed().as_secs_f64() * 1000.,
            );
            let phase = std::time::Instant::now();
            // Pos is ordered x,y,z. Feeding that order permits bulk tree construction,
            // rather than sorting/inserting a z,y,x permutation every update.
            let visible: BTreeSet<_> = (0..128)
                .flat_map(|x| (0..96).flat_map(move |y| (-2..=5).map(move |z| Pos::new(x, y, z))))
                .filter(|p| bits[index(*p)])
                .collect();
            let newly_explored: Vec<_> = visible
                .difference(&self.state.explored[(owner - 1) as usize])
                .copied()
                .collect();
            self.state.explored[(owner - 1) as usize].extend(newly_explored);
            self.state.visible[(owner - 1) as usize] = visible;
            crate::profiling::record_substage(
                "fog/visible-and-explored-sets",
                phase.elapsed().as_secs_f64() * 1000.,
            );
        }
    }
    #[cfg(test)]
    fn refresh_fog_reference(&mut self) {
        let mut opaque = vec![false; 128 * 96 * 8];
        let mut plates = vec![false; 128 * 96 * 8];
        let mut covered = vec![-2; 128 * 96];
        for y in 0..96 {
            for x in 0..128 {
                let p = Pos::new(x, y, 0);
                if self.terrain(p) == 1 {
                    opaque[index(p)] = true;
                }
            }
        }
        let solid_walls: BTreeSet<Pos> = self
            .state
            .walls
            .iter()
            .filter(|w| w.hp > 0.)
            .map(|w| w.pos)
            .collect();
        for p in &solid_walls {
            opaque[index(*p)] = true;
        }
        for b in self
            .state
            .buildings
            .iter()
            .filter(|b| b.hp > 0. && b.kind == "shell")
        {
            let r = b.rect;
            for p in r.cells() {
                if r.level > 0
                    && b.progress >= 1.
                    && !self.state.entrances.iter().any(|e| {
                        e.hp > 0.
                            && e.open
                            && matches!(e.kind.as_str(), "stairs" | "elevator" | "ramp")
                            && e.covers(p)
                    })
                {
                    plates[index(p)] = true;
                    covered[(p.y * 128 + p.x) as usize] =
                        covered[(p.y * 128 + p.x) as usize].max(p.level);
                }
                if p.x == r.x || p.y == r.y || p.x == r.x + r.width - 1 || p.y == r.y + r.height - 1
                {
                    opaque[index(p)] = true;
                }
            }
        }
        for e in &self.state.entrances {
            if e.kind == "door" && e.hp > 0. && !e.open {
                for p in e.cells() {
                    opaque[index(p)] = true;
                }
            }
            if (e.hp > 0. || e.kind == "door")
                && e.open
                && matches!(e.kind.as_str(), "door" | "window")
            {
                for offset in 0..e.width {
                    let p = Pos::new(
                        e.pos.x + if e.axis == "y" { 0 } else { offset as i32 },
                        e.pos.y + if e.axis == "y" { offset as i32 } else { 0 },
                        e.pos.level,
                    );
                    if p.valid()
                        && !solid_walls.contains(&p)
                        && !(p.level == 0 && self.terrain(p) == 1)
                    {
                        opaque[index(p)] = false;
                    }
                }
            }
        }
        for owner in 1..=2 {
            let mut bits = vec![false; 128 * 96 * 8];
            let sources: Vec<(Pos, f64, f64)> = self
                .state
                .buildings
                .iter()
                .filter(|b| b.owner == owner && b.hp > 0. && b.progress > 0.)
                .map(|b| {
                    (
                        b.rect.center(),
                        if b.kind == "core" {
                            25.
                        } else if b.progress < 1. {
                            6.
                        } else {
                            14.
                        },
                        b.rect.level as f64 + 0.7,
                    )
                })
                .chain(
                    self.state
                        .units
                        .iter()
                        .filter(|u| u.owner == owner && u.hp > 0.)
                        .map(|u| (u.pos, crate::traits::vision_radius(u), u.elevation() + 0.7)),
                )
                .chain(
                    self.state
                        .jobs
                        .iter()
                        .filter(|j| j.owner == owner && !j.blocked)
                        .map(|j| (j.worker, 6., j.worker.level as f64 + 0.7)),
                )
                .collect();
            let mut unique_sources = std::collections::BTreeMap::new();
            for (p, base, eye) in sources {
                unique_sources
                    .entry((p, eye.to_bits()))
                    .and_modify(|r: &mut f64| *r = (*r).max(base))
                    .or_insert(base);
            }
            for ((p, eye_bits), base) in unique_sources {
                let eye = f64::from_bits(eye_bits);
                let radius =
                    base + p.level.max(0) as f64 * 2. + if self.terrain(p) == 4 { 3. } else { 0. };
                let reach = radius.ceil() as i32;
                let mut planes = vec![p.level];
                if p.level > 0 {
                    planes.push(0);
                }
                for z in planes {
                    let start = Pos::new(p.x, p.y, z);
                    bits[index(start)] = true;
                    for side in 0..4 {
                        for offset in -reach..=reach {
                            let end = match side {
                                0 => Pos::new(p.x - reach, p.y + offset, z),
                                1 => Pos::new(p.x + reach, p.y + offset, z),
                                2 => Pos::new(p.x + offset, p.y - reach, z),
                                _ => Pos::new(p.x + offset, p.y + reach, z),
                            };
                            let steps = (end.x - start.x).abs().max((end.y - start.y).abs());
                            let mut previous_height = eye;
                            for i in 1..=steps {
                                let q = Pos::new(
                                    start.x + (end.x - start.x) * i / steps,
                                    start.y + (end.y - start.y) * i / steps,
                                    z,
                                );
                                if !q.valid()
                                    || ((q.x - p.x).pow(2) + (q.y - p.y).pow(2)) as f64
                                        > radius * radius
                                {
                                    break;
                                }
                                let ray_height =
                                    eye + (z as f64 + 0.5 - eye) * i as f64 / steps as f64;
                                let solid_floor = ((ray_height.floor() as i32 + 1)
                                    ..=previous_height.floor() as i32)
                                    .any(|floor| {
                                        (-2..=5).contains(&floor)
                                            && plates[index(Pos::new(q.x, q.y, floor))]
                                    });
                                if solid_floor {
                                    break;
                                }
                                if z == p.level || covered[(q.y * 128 + q.x) as usize] <= z {
                                    bits[index(q)] = true;
                                }
                                let ray_cell =
                                    Pos::new(q.x, q.y, (ray_height.floor() as i32).clamp(-2, 5));
                                if opaque[index(ray_cell)] {
                                    break;
                                }
                                previous_height = ray_height;
                            }
                        }
                    }
                }
            }
            let mut visible = BTreeSet::new();
            for (z, chunk) in bits.chunks(128 * 96).enumerate() {
                for (i, on) in chunk.iter().enumerate() {
                    if *on {
                        visible.insert(Pos::new((i % 128) as i32, (i / 128) as i32, z as i32 - 2));
                    }
                }
            }
            self.state.explored[(owner - 1) as usize].extend(visible.iter());
            self.state.visible[(owner - 1) as usize] = visible;
        }
    }
    pub fn snapshot(&self, owner: Option<u32>) -> Snapshot {
        let mut s = self.state.clone();
        s.shieldAuto = self.shield_auto;
        for unit in &mut s.units {
            unit.pluginDiscount = if owner.is_none_or(|o| o == unit.owner) {
                self.plugin_discount(unit)
            } else {
                0.
            };
        }
        if let Some(o) = owner.filter(|o| (1..=2).contains(o)) {
            s.shieldAuto[(2 - o) as usize] = false;
            s.buildings
                .retain(|b| b.owner == o || self.visible_building_edge(o, b.rect).is_some());
            s.rooms
                .retain(|r| r.owner == o || self.visible_to(o, r.rect.center()));
            s.units
                .retain(|u| u.owner == o || self.visible_to(o, u.pos));
            s.shipments
                .retain(|u| u.owner == o || self.visible_to(o, u.pos));
            for u in s.units.iter_mut().filter(|u| u.owner != o) {
                // Keep measured velocity from the completed step; hide future intent.
                if u.transitProgress > 0. {
                    u.route.truncate(1);
                } else {
                    u.route.clear();
                }
                u.queuedGoals.clear();
                u.goal = None;
                u.sortieTarget = None;
                u.target = None;
                u.sourceFacility = 0;
                u.pluginDiscount = 0.;
                if let Some(d) = &mut u.dash {
                    d.hitTargets.clear();
                    d.previous = [u.x, u.y];
                }
            }
            for shipment in s.shipments.iter_mut().filter(|s| s.owner != o) {
                // A visible in-progress edge is needed to render the actual
                // position; later destinations and all planned waypoints stay private.
                shipment.route.truncate(1);
                shipment.waypoints.clear();
                shipment.manualRoute = false;
                shipment.from = 0;
                shipment.to = 0;
            }
            s.links
                .retain(|l| l.owner == o || l.path.iter().any(|p| self.visible_to(o, *p)));
            s.walls
                .retain(|w| w.owner == o || self.visible_to(o, w.pos));
            s.entrances
                .retain(|e| e.owner == o || e.cells().iter().any(|p| self.visible_to(o, *p)));
            s.events
                .retain(|e| e.owner == o || self.visible_to(o, e.pos));
            for event in s
                .events
                .iter_mut()
                .filter(|e| e.owner != o && !e.kind.starts_with("unit-death:"))
            {
                if event.direction.is_some_and(|p| !self.visible_to(o, p)) {
                    event.direction = None;
                }
            }
            s.jobs.retain(|j| j.owner == o);
            s.networkStores.retain(|n| n.owner == o);
            s.defenseFields
                .retain(|f| f.owner == o || self.visible_to(o, f.pos));
            s.shieldRegions.retain(|r| r.owner == o);
            s.powerGrids.retain(|r| r.owner == o);
            s.rubble.retain(|r| {
                r.owner == o || self.state.explored[(o - 1) as usize].contains(&r.rect.center())
            });
            s.excavated.retain(|p| {
                self.state
                    .excavationOwners
                    .get(&(((p.level + 2) * 128 * 96 + p.y * 128 + p.x) as u32))
                    == Some(&o)
                    || self.state.explored[(o - 1) as usize].contains(p)
            });
            s.excavationOwners.retain(|key, owner| {
                *owner == o || {
                    let n = *key as i32;
                    self.state.explored[(o - 1) as usize].contains(&Pos::new(
                        n % 128,
                        (n / 128) % 96,
                        n / (128 * 96) - 2,
                    ))
                }
            });
            for link in &mut s.links {
                if link.owner != o {
                    link.unitEndpoints.clear();
                    link.path.retain(|p| self.visible_to(o, *p));
                }
            }
            s.projectiles.retain(|p| {
                p.owner == o
                    || self.visible_to(
                        o,
                        Pos::new(p.x.floor() as i32, p.y.floor() as i32, p.destination.level),
                    )
            });
            for p in &mut s.projectiles {
                if p.owner != o {
                    let current =
                        Pos::new(p.x.floor() as i32, p.y.floor() as i32, p.destination.level);
                    if !self.visible_to(o, p.origin) {
                        p.origin = current;
                        p.launchPosition = Some([p.x, p.y, p.z]);
                        p.source = 0;
                    }
                    if !self.visible_to(o, p.destination) {
                        p.destination = current;
                        p.aimPosition = Some([p.x, p.y, p.z]);
                        p.target = None;
                    }
                }
            }
            s.resources.retain(|r| {
                matches!(r.kind.as_str(), "ore" | "coal" | "node")
                    || r.owner == o
                    || self.visible_to(o, r.pos)
            });
            for resource in &mut s.resources {
                if resource.kind != "node"
                    && resource.owner != o
                    && !self.visible_to(o, resource.pos)
                {
                    resource.owner = 0;
                    resource.remaining = -1.;
                    resource.capture = 0.;
                    resource.contested = false;
                    resource.capturer = 0;
                }
            }
            s.visible[(2 - o) as usize].clear();
            s.explored[(2 - o) as usize].clear();
            for p in &mut s.players {
                if p.owner != o {
                    p.credits = 0.;
                    p.compute = 0.;
                    p.computeCapacity = 0.;
                    p.power = 0.;
                    p.demand = 0.;
                    p.science = 0.;
                    p.production = 0.;
                    p.income = 0.;
                    p.research = None;
                    p.researches.clear();
                    p.branches.clear();
                    p.totals.clear();
                }
            }
        }
        s
    }
}
