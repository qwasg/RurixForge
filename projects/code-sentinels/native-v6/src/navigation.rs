//! Single-layer navigation shared by pathfinding and physical movement.
use crate::{Game, Pos, Snapshot};
use std::{cell::RefCell, cmp::Reverse, collections::BinaryHeap, sync::Arc};
const CELLS: usize = 128 * 96;
fn index(p: Pos) -> usize {
    (p.y * 128 + p.x) as usize
}
fn pos(i: usize) -> Pos {
    Pos::new((i % 128) as i32, (i / 128) as i32, 0)
}
#[derive(Clone, Default)]
pub(crate) struct NavCache {
    stamp: Option<(u64, u64)>,
    fingerprint: u64,
    grid: Option<Arc<NavGrid>>,
}
pub(crate) struct NavGrid {
    /// Walkable for infantry / drones on open ground (not inside shells).
    ground: Vec<bool>,
    /// Always true for air — aircraft fly over structures.
    air: Vec<bool>,
    /// Completed shell / outdoor building occupancy (blocks ground & vehicle).
    blocked: Vec<bool>,
}
fn fingerprint(s: &Snapshot) -> u64 {
    let mut hash = 0xcbf29ce484222325u64;
    let mut mix = |v: u64| {
        hash = (hash ^ v).wrapping_mul(0x100000001b3);
    };
    for t in &s.terrain {
        mix(*t as u64);
    }
    mix(0x11);
    for b in &s.buildings {
        if b.hp <= 0. {
            continue;
        }
        mix(b.id);
        for n in [b.rect.x, b.rect.y, b.rect.width, b.rect.height] {
            mix(n as u64);
        }
        mix((b.progress >= 1.) as u64);
        for c in b.kind.bytes() {
            mix(c as u64);
        }
    }
    mix(0x22);
    for w in s.walls.iter().filter(|w| w.hp > 0.) {
        mix(index(w.pos) as u64);
    }
    mix(0x33);
    for r in &s.rubble {
        for n in [r.rect.x, r.rect.y, r.rect.width, r.rect.height] {
            mix(n as u64);
        }
    }
    hash
}
impl NavGrid {
    fn build(s: &Snapshot) -> Self {
        let mut g = Self {
            ground: vec![true; CELLS],
            air: vec![true; 128 * 96],
            blocked: vec![false; CELLS],
        };
        for y in 0..96 {
            for x in 0..128 {
                let t = s.terrain[(y * 128 + x) as usize];
                // Water (2) and rock (1) are not walkable for ground units.
                if t == 1 || t == 2 {
                    g.ground[index(Pos::new(x, y, 0))] = false;
                }
            }
        }
        for b in s.buildings.iter().filter(|b| b.hp > 0. && b.progress >= 1.) {
            for cell in b.rect.cells() {
                if cell.valid() {
                    let i = index(cell);
                    g.blocked[i] = true;
                    g.ground[i] = false;
                }
            }
        }
        for r in &s.rubble {
            for cell in r.rect.cells() {
                if cell.valid() {
                    g.ground[index(cell)] = false;
                }
            }
        }
        for w in s.walls.iter().filter(|w| w.hp > 0.) {
            if w.pos.valid() {
                g.ground[index(w.pos)] = false;
            }
        }
        g
    }
    fn walkable(&self, p: Pos, category: &str) -> bool {
        if !p.valid() {
            return false;
        }
        let i = index(p);
        match category {
            "air" | "orbital" => self.air.get(i % (128 * 96)).copied().unwrap_or(true),
            "vehicle" => self.ground[i] && !self.blocked[i],
            _ => self.ground[i],
        }
    }
}
pub(crate) fn cache() -> RefCell<NavCache> {
    RefCell::new(NavCache::default())
}
impl Game {
    pub fn invalidate_navigation(&self) {
        self.navigation.borrow_mut().stamp = None;
        self.navigation.borrow_mut().grid = None;
    }
    fn nav_grid(&self) -> Arc<NavGrid> {
        let mut cache = self.navigation.borrow_mut();
        let stamp = (self.state.tick, self.state.revision);
        let fp = fingerprint(&self.state);
        if cache.stamp == Some(stamp) && cache.fingerprint == fp {
            if let Some(g) = &cache.grid {
                return Arc::clone(g);
            }
        }
        let grid = Arc::new(NavGrid::build(&self.state));
        cache.stamp = Some(stamp);
        cache.fingerprint = fp;
        cache.grid = Some(Arc::clone(&grid));
        grid
    }
    pub fn walkable(&self, p: Pos, category: &str) -> bool {
        self.nav_grid().walkable(p, category)
    }
    pub fn can_step(&self, from: Pos, to: Pos, category: &str) -> bool {
        if from.level != 0 || to.level != 0 {
            return false;
        }
        if (from.x - to.x).abs() + (from.y - to.y).abs() != 1 {
            return false;
        }
        self.walkable(to, category)
            && (category != "vehicle" || self.vehicle_footprint_clear(to))
    }
    pub(crate) fn vehicle_footprint_clear(&self, p: Pos) -> bool {
        for dy in 0..2 {
            for dx in 0..2 {
                let cell = Pos::new(p.x + dx, p.y + dy, 0);
                if !self.walkable(cell, "vehicle") {
                    return false;
                }
            }
        }
        true
    }
    pub fn route(&self, start: Pos, goal: Pos, category: &str) -> Option<Vec<Pos>> {
        if start.level != 0 || goal.level != 0 {
            return None;
        }
        if start == goal {
            return Some(vec![]);
        }
        if !self.walkable(goal, category)
            || (category == "vehicle" && !self.vehicle_footprint_clear(goal))
        {
            return None;
        }
        let grid = self.nav_grid();
        let start_i = index(start);
        let goal_i = index(goal);
        let mut came = vec![usize::MAX; CELLS];
        let mut g_score = vec![f64::INFINITY; CELLS];
        let mut open = BinaryHeap::new();
        g_score[start_i] = 0.;
        open.push(Reverse((
            ordered(start.distance(goal)),
            ordered(0.),
            start_i,
        )));
        while let Some(Reverse((_, _, current))) = open.pop() {
            if current == goal_i {
                let mut path = Vec::new();
                let mut c = goal_i;
                while c != start_i {
                    path.push(pos(c));
                    c = came[c];
                    if c == usize::MAX {
                        return None;
                    }
                }
                path.reverse();
                return Some(path);
            }
            let cur = pos(current);
            for (dx, dy) in [(1, 0), (-1, 0), (0, 1), (0, -1)] {
                let next = Pos::new(cur.x + dx, cur.y + dy, 0);
                if !next.valid() {
                    continue;
                }
                if !grid.walkable(next, category) {
                    continue;
                }
                if category == "vehicle" && !self.vehicle_footprint_clear(next) {
                    continue;
                }
                let ni = index(next);
                let tentative = g_score[current] + 1.;
                if tentative < g_score[ni] {
                    came[ni] = current;
                    g_score[ni] = tentative;
                    let f = tentative + next.distance(goal);
                    open.push(Reverse((ordered(f), ordered(tentative), ni)));
                }
            }
        }
        None
    }
}
fn ordered(v: f64) -> u64 {
    v.to_bits()
}

/// Supercover line for air / LOS including both corner-touch cells.
pub fn supercover(a: Pos, b: Pos) -> Vec<Pos> {
    let mut cells = Vec::new();
    let (x0, y0, x1, y1) = (a.x, a.y, b.x, b.y);
    let dx = (x1 - x0).abs();
    let dy = (y1 - y0).abs();
    let sx = if x0 < x1 { 1 } else { -1 };
    let sy = if y0 < y1 { 1 } else { -1 };
    let mut err = dx - dy;
    let mut x = x0;
    let mut y = y0;
    loop {
        cells.push(Pos::new(x, y, 0));
        if x == x1 && y == y1 {
            break;
        }
        let e2 = 2 * err;
        if e2 > -dy {
            err -= dy;
            x += sx;
        }
        if e2 < dx {
            err += dx;
            y += sy;
        }
    }
    cells
}
