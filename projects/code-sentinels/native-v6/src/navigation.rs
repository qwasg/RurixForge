//! Immutable navigation layers shared by pathfinding and physical movement.
use crate::{Game, Pos, Snapshot};
use std::{cell::RefCell, cmp::Reverse, collections::BinaryHeap, sync::Arc};
const CELLS: usize = 128 * 96 * 8;
fn index(p: Pos) -> usize {
    ((p.level + 2) * 128 * 96 + p.y * 128 + p.x) as usize
}
fn pos(i: usize) -> Pos {
    Pos::new(
        (i % 128) as i32,
        ((i / 128) % 96) as i32,
        (i / (128 * 96)) as i32 - 2,
    )
}
#[derive(Clone, Default)]
pub(crate) struct NavCache {
    stamp: Option<(u64, u64)>,
    fingerprint: u64,
    grid: Option<Arc<NavGrid>>,
}
pub(crate) struct NavGrid {
    ground: Vec<bool>,
    air: Vec<bool>,
    shell: Vec<u64>,
    door: Vec<bool>,
    portals: Vec<u8>,
}
fn fingerprint(s: &Snapshot) -> u64 {
    let mut hash = 0xcbf29ce484222325u64;
    let mut mix = |v: u64| {
        hash = (hash ^ v).wrapping_mul(0x100000001b3);
    };
    for t in &s.terrain {
        mix(*t as u64);
    }
    for p in &s.excavated {
        mix(index(*p) as u64);
    }
    mix(0x11);
    for b in &s.buildings {
        if b.hp <= 0. {
            continue;
        }
        mix(b.id);
        for n in [
            b.rect.x,
            b.rect.y,
            b.rect.level,
            b.rect.width,
            b.rect.height,
        ] {
            mix(n as u64);
        }
        mix((b.progress >= 1.) as u64);
        for c in b.kind.bytes() {
            mix(c as u64);
        }
    }
    mix(0x22);
    for r in s
        .rooms
        .iter()
        .filter(|r| r.kind == "column" && r.hp > 0. && r.progress >= 1.)
    {
        for n in [
            r.rect.x,
            r.rect.y,
            r.rect.level,
            r.rect.width,
            r.rect.height,
        ] {
            mix(n as u64);
        }
    }
    mix(0x33);
    for w in s.walls.iter().filter(|w| w.hp > 0.) {
        mix(index(w.pos) as u64);
    }
    mix(0x44);
    for e in s.entrances.iter().filter(|e| e.reserves_space()) {
        mix(index(e.pos) as u64);
        mix(e.toLevel as u64);
        mix(e.open as u64);
        mix((e.hp > 0.) as u64);
        mix(e.powered as u64);
        mix(e.width as u64);
        for c in e.kind.bytes().chain(e.axis.bytes()) {
            mix(c as u64);
        }
    }
    hash
}
impl NavGrid {
    fn build(s: &Snapshot) -> Self {
        let mut g = Self {
            ground: vec![false; CELLS],
            air: vec![true; 128 * 96],
            shell: vec![0; CELLS],
            door: vec![false; CELLS],
            portals: vec![0; CELLS],
        };
        for y in 0..96 {
            for x in 0..128 {
                let p = Pos::new(x, y, 0);
                g.ground[index(p)] = !matches!(s.terrain[(y * 128 + x) as usize], 1 | 2);
            }
        }
        for p in &s.excavated {
            g.ground[index(*p)] = true;
        }
        for b in s.buildings.iter().filter(|b| b.hp > 0.) {
            for p in b.rect.cells() {
                let i = index(p);
                if b.kind == "shell" {
                    g.shell[i] = b.id;
                    if p.level > 0 && b.progress >= 1. {
                        g.ground[i] = true;
                    }
                } else if b.kind != "core" {
                    g.ground[i] = false;
                }
                if p.level >= 2 {
                    g.air[(p.y * 128 + p.x) as usize] = false;
                }
            }
        }
        for e in s
            .entrances
            .iter()
            .filter(|e| e.kind == "door" && (e.open || e.hp <= 0.))
        {
            for k in 0..e.width {
                let p = Pos::new(
                    e.pos.x + if e.axis == "y" { 0 } else { k as i32 },
                    e.pos.y + if e.axis == "y" { k as i32 } else { 0 },
                    e.pos.level,
                );
                if p.valid() {
                    g.door[index(p)] = true;
                }
            }
        }
        for w in s.walls.iter().filter(|w| w.hp > 0.) {
            let i = index(w.pos);
            g.ground[i] = false;
        }
        for e in s.entrances.iter().filter(|e| e.hp > 0.) {
            for z in e.pos.level.min(e.toLevel)..=e.pos.level.max(e.toLevel) {
                for k in 0..e.width {
                    let p = Pos::new(
                        e.pos.x + if e.axis == "y" { 0 } else { k as i32 },
                        e.pos.y + if e.axis == "y" { k as i32 } else { 0 },
                        z,
                    );
                    if !p.valid() || !e.serves(z) {
                        continue;
                    }
                    let i = index(p);
                    if e.kind == "door" && !e.open {
                        g.ground[i] = false;
                    }
                    if e.open
                        && !matches!(e.kind.as_str(), "door" | "window")
                        && (e.kind != "elevator" || e.powered)
                    {
                        let vehicle = e.kind == "ramp" && e.width >= 3 && k + 1 < e.width;
                        for (dz, foot, wide) in [(1, 1, 4), (-1, 2, 8)] {
                            if e.serves(z + dz) && (-2..=5).contains(&(z + dz)) {
                                g.portals[i] |= foot;
                                if vehicle {
                                    g.portals[i] |= wide;
                                }
                            }
                        }
                    }
                }
            }
        }
        for r in s
            .rooms
            .iter()
            .filter(|r| r.kind == "column" && r.hp > 0. && r.progress >= 1.)
        {
            for p in r.rect.cells() {
                g.ground[index(p)] = false;
            }
        }
        g
    }
    pub(crate) fn walkable(&self, p: Pos, category: &str) -> bool {
        if !p.valid() {
            return false;
        }
        if matches!(category, "air" | "orbital") {
            return p.level == 0 && self.air[(p.y * 128 + p.x) as usize];
        }
        if category == "vehicle" {
            return [(0, 0), (1, 0), (0, 1), (1, 1)]
                .iter()
                .all(|(x, y)| self.walkable(Pos::new(p.x + x, p.y + y, p.level), "footprint"));
        }
        self.ground[index(p)]
    }
    fn edge(&self, a: Pos, b: Pos) -> bool {
        if !a.valid() || !b.valid() {
            return false;
        }
        let ai = index(a);
        let bi = index(b);
        self.shell[ai] == self.shell[bi]
            || (self.shell[ai] == 0 || self.door[ai]) && (self.shell[bi] == 0 || self.door[bi])
    }
    pub(crate) fn can_step(&self, a: Pos, b: Pos, category: &str) -> bool {
        if !a.valid() || !self.walkable(b, category) {
            return false;
        }
        if a == b {
            return true;
        }
        if matches!(category, "air" | "orbital") {
            return a.level == 0
                && b.level == 0
                && supercover(a, b)
                    .into_iter()
                    .all(|p| self.walkable(p, category));
        }
        if a.level != b.level {
            if a.x != b.x || a.y != b.y || (a.level - b.level).abs() != 1 {
                return false;
            }
            let mask = if category == "vehicle" {
                if b.level > a.level {
                    4
                } else {
                    8
                }
            } else if b.level > a.level {
                1
            } else {
                2
            };
            return self.portals[index(a)] & mask != 0;
        }
        if (a.x - b.x).abs() + (a.y - b.y).abs() != 1 {
            return false;
        }
        self.edge(a, b)
            && (category != "vehicle"
                || [(1, 0), (0, 1), (1, 1)].iter().all(|(x, y)| {
                    self.edge(
                        Pos::new(a.x + x, a.y + y, a.level),
                        Pos::new(b.x + x, b.y + y, b.level),
                    )
                }))
    }
    pub(crate) fn route(&self, a: Pos, b: Pos, category: &str) -> Option<Vec<Pos>> {
        if !a.valid() || !self.walkable(b, category) {
            return None;
        }
        if a == b {
            return Some(vec![]);
        }
        let flying = matches!(category, "air" | "orbital");
        if flying && self.can_step(a, b, category) {
            return Some(vec![b]);
        }
        let start = index(a);
        let target = index(b);
        let mut distance = vec![u32::MAX; CELLS];
        let mut previous = vec![usize::MAX; CELLS];
        let mut heap = BinaryHeap::new();
        let heuristic = |p: Pos| {
            (p.x - b.x).unsigned_abs()
                + (p.y - b.y).unsigned_abs()
                + (p.level - b.level).unsigned_abs()
        };
        distance[start] = 0;
        previous[start] = start;
        heap.push(Reverse((heuristic(a), 0u32, start)));
        let mut expanded = 0;
        while let Some(Reverse((_, cost, i))) = heap.pop() {
            if cost != distance[i] {
                continue;
            }
            if i == target {
                break;
            }
            expanded += 1;
            if expanded > CELLS {
                return None;
            }
            let p = pos(i);
            for next in [
                Pos::new(p.x - 1, p.y, p.level),
                Pos::new(p.x + 1, p.y, p.level),
                Pos::new(p.x, p.y - 1, p.level),
                Pos::new(p.x, p.y + 1, p.level),
                Pos::new(p.x, p.y, p.level - 1),
                Pos::new(p.x, p.y, p.level + 1),
            ] {
                if !self.can_step(p, next, category) {
                    continue;
                }
                let ni = index(next);
                let nc = cost + 1;
                if nc < distance[ni] {
                    distance[ni] = nc;
                    previous[ni] = i;
                    heap.push(Reverse((nc + heuristic(next), nc, ni)));
                }
            }
        }
        if previous[target] == usize::MAX {
            return None;
        }
        let mut route = vec![];
        let mut current = target;
        while current != start {
            route.push(pos(current));
            current = previous[current];
        }
        route.reverse();
        Some(route)
    }
}
/// Includes both cells touching a diagonal corner, so a plane cannot clip the
/// corner of a one-cell tower merely because an endpoint sample missed it.
pub fn supercover(a: Pos, b: Pos) -> Vec<Pos> {
    if !a.valid() || !b.valid() || a.level != b.level {
        return vec![];
    }
    let nx = (b.x - a.x).abs();
    let ny = (b.y - a.y).abs();
    let sx = (b.x - a.x).signum();
    let sy = (b.y - a.y).signum();
    let (mut ix, mut iy) = (0, 0);
    let mut p = a;
    let mut cells = vec![a];
    while ix < nx || iy < ny {
        let decision = (1 + 2 * ix) * ny - (1 + 2 * iy) * nx;
        if decision == 0 {
            cells.push(Pos::new(p.x + sx, p.y, p.level));
            cells.push(Pos::new(p.x, p.y + sy, p.level));
            p.x += sx;
            p.y += sy;
            ix += 1;
            iy += 1;
        } else if decision < 0 {
            p.x += sx;
            ix += 1;
        } else {
            p.y += sy;
            iy += 1;
        }
        cells.push(p);
    }
    cells
}
impl Game {
    pub fn invalidate_navigation(&self) {
        self.navigation.borrow_mut().stamp = None;
    }
    pub(crate) fn navigation_grid(&self) -> Arc<NavGrid> {
        let mut cache = self.navigation.borrow_mut();
        let stamp = (self.state.tick, self.state.revision);
        if cache.stamp != Some(stamp) {
            let fingerprint = fingerprint(&self.state);
            if cache.grid.is_none() || cache.fingerprint != fingerprint {
                cache.grid = Some(Arc::new(NavGrid::build(&self.state)));
                cache.fingerprint = fingerprint;
            }
            cache.stamp = Some(stamp);
        }
        cache.grid.as_ref().unwrap().clone()
    }
}
pub(crate) fn cache() -> RefCell<NavCache> {
    RefCell::new(NavCache::default())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Building, Wall};
    #[test]
    fn cache_reuses_static_layout_and_detects_topology_changes() {
        let mut g = Game::new(3, false);
        g.state.terrain.fill(0);
        let first = g.navigation_grid();
        g.state.tick += 1;
        g.state.buildings[0].hp -= 1.;
        let again = g.navigation_grid();
        assert!(Arc::ptr_eq(&first, &again));
        let id = g.id();
        g.state.walls.push(Wall {
            id,
            owner: 1,
            kind: "physical".into(),
            pos: Pos::new(16, 48, 0),
            hp: 260.,
            maxHp: 260.,
            shield: 0.,
            invested: 10.,
            antiHeal: 0.,
        });
        g.invalidate_navigation();
        let changed = g.navigation_grid();
        assert!(!Arc::ptr_eq(&first, &changed));
        assert!(!g.walkable(Pos::new(16, 48, 0), "ai"));
    }
    #[test]
    fn diagonal_flight_checks_both_corner_cells_and_astar_finds_detour() {
        let mut g = Game::new(4, false);
        g.state.terrain.fill(0);
        let tower:Building=serde_json::from_value(serde_json::json!({"id":700,"owner":2,"kind":"shell","rect":{"x":17,"y":48,"z":2,"w":1,"h":1},"tier":1,"hp":100.,"maxHp":100.,"progress":1.,"buildTime":1.,"powered":false,"connected":false,"power":0.,"demand":0.,"capacity":0,"branch":null,"inventory":0.,"invested":0.,"jam":0.,"shield":0.,"born":0})).unwrap();
        g.state.buildings.push(tower);
        let a = Pos::new(16, 48, 0);
        let b = Pos::new(17, 49, 0);
        let touched = supercover(a, b);
        assert!(touched.contains(&Pos::new(17, 48, 0)));
        assert!(touched.contains(&Pos::new(16, 49, 0)));
        assert!(!g.can_step(a, b, "air"));
        let route = g.route(a, b, "air").unwrap();
        assert!(route.len() > 1);
        let mut previous = a;
        for step in route {
            assert!(g.can_step(previous, step, "air"));
            previous = step;
        }
        assert_eq!(previous, b);
    }
}

#[cfg(test)]
mod boundary_tests {
    use super::*;
    use crate::{Building, Entrance, Link};
    fn floor(id: u64, z: i32) -> Building {
        serde_json::from_value(serde_json::json!({"id":id,"owner":1,"kind":"shell","rect":{"x":18,"y":28,"z":z,"w":8,"h":8},"tier":1,"hp":1000.,"maxHp":1000.,"progress":1.,"buildTime":1.,"powered":false,"connected":false,"power":0.,"demand":0.,"capacity":0,"branch":null,"inventory":0.,"invested":0.,"jam":0.,"shield":0.,"born":0})).unwrap()
    }
    fn entrance(id: u64, kind: &str, width: u32) -> Entrance {
        Entrance {
            id,
            owner: 1,
            pos: Pos::new(20, 30, 0),
            toLevel: 1,
            kind: kind.into(),
            hp: 250.,
            open: true,
            powered: false,
            width,
            axis: "x".into(),
        }
    }
    #[test]
    fn vehicle_requires_its_full_transverse_ramp_width() {
        let mut g = Game::new(6, false);
        g.state.terrain.fill(0);
        g.state.buildings.extend([floor(701, 0), floor(702, 1)]);
        g.state.entrances.push(entrance(703, "ramp", 3));
        assert!(g.can_step(Pos::new(21, 30, 0), Pos::new(21, 30, 1), "vehicle"));
        assert!(!g.can_step(Pos::new(22, 30, 0), Pos::new(22, 30, 1), "vehicle"));
        assert!(g.can_step(Pos::new(22, 30, 0), Pos::new(22, 30, 1), "ai"));
    }
    #[test]
    fn powered_elevator_and_destroyed_shaft_invalidate_same_tick_paths_and_wire() {
        let mut g = Game::new(7, false);
        g.state.terrain.fill(0);
        g.state.buildings.extend([floor(701, 0), floor(702, 1)]);
        g.state.entrances.push(entrance(703, "elevator", 1));
        let a = Pos::new(20, 30, 0);
        let b = Pos::new(20, 30, 1);
        assert!(!g.can_step(a, b, "ai"));
        g.state.entrances[0].powered = true;
        g.invalidate_navigation();
        assert!(g.can_step(a, b, "ai"));
        g.state.links.push(Link {
        unitEndpoints: vec![],
            id: 704,
            owner: 1,
            kind: "power".into(),
            path: vec![a, b],
            hp: 150.,
            active: true,
            invested: 4.,
        });
        g.damage(703, 1e12, "kinetic", 0);
        assert!(!g.can_step(a, b, "ai"));
        assert!(g.state.links.iter().all(|l| l.id != 704));
    }
    #[test]
    fn warmed_navigation_does_not_land_on_a_floor_in_the_same_collapse() {
        let mut g = Game::new(8, false);
        g.state.terrain.fill(0);
        g.state
            .buildings
            .extend([floor(701, 1), floor(702, 2), floor(703, 3)]);
        g.state.units.push(serde_json::from_value(serde_json::json!({"id":704,"owner":1,"kind":"kimi","pos":{"x":21,"y":31,"z":2},"x":21.5,"y":31.5,"z":2,"tier":2,"hp":1000.,"maxHp":1000.,"battery":0.,"batteryMax":240.,"covered":false,"wired":false,"ammo":0.,"route":[],"target":null,"cooldown":0.,"skillCooldown":0.,"plugins":[],"statuses":{},"invested":0.,"moving":false,"attackCount":0})).unwrap());
        let _warm = g.navigation_grid();
        for _ in 0..4 {
            g.update_supports();
        }
        assert!(g
            .state
            .buildings
            .iter()
            .all(|b| ![701, 702, 703].contains(&b.id)));
        let survivor = g.state.units.iter().find(|u| u.id == 704).unwrap();
        assert_eq!(survivor.pos.level, 0);
        assert!(survivor.hp > 0.);
        assert!(!g.walkable(Pos::new(21, 31, 2), "ai"));
        assert!(g.walkable(Pos::new(21, 31, 0), "ai"));
    }
}
