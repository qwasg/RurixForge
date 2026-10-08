//! Code Sentinels V2 — native open-terrain GPU-economy campaign.
//! Hardware names identify game buildings; all economy values are fictional.
//! Browser UI reads published state; it never simulates combat or pathfinding.
use std::sync::Mutex;

const W: usize = 24;
const H: usize = 14;
const CELLS: usize = W * H;
const BASE: usize = 7 * W + 1;
const UNITS: usize = 24;
const ENEMIES: usize = 64;
const SHOTS: usize = 32;
const FX: usize = 64;
const GPU_COST: [f32; 8] = [0., 130., 220., 350., 500., 760., 980., 1300.];
const GPU_RATE: [f32; 8] = [0., 12., 20., 32., 49., 72., 94., 128.];
const GPU_CAP: [f32; 8] = [0., 300., 420., 600., 800., 1000., 1300., 1700.];
const COST: [f32; 5] = [0., 65., 90., 110., 100.];
const ATTACK_ENERGY: [f32; 5] = [0., 3., 6., 5., 4.];
const SKILL_ENERGY: [f32; 5] = [0., 55., 75., 90., 110.];
const SKILL_CD: [f32; 5] = [0., 13., 17., 20., 24.];
const DAMAGE: [f32; 5] = [0., 27., 24., 15., 19.];
const RATE: [f32; 5] = [1., 0.72, 1.32, 1.24, 1.05];
const RANGE: [f32; 5] = [0., 3.8, 3.4, 4.5, 4.1];
const INF: f32 = 1_000_000.;

#[derive(Clone, Copy, Debug)]
struct Unit {
    kind: usize,
    level: u32,
    cell: usize,
    hp: f32,
    cool: f32,
    skill: f32,
    flash: f32,
    target: u32,
    attacks: u32,
    casts: u32,
    starved: bool,
    jam: f32,
    invested: f32,
}
impl Default for Unit {
    fn default() -> Self {
        Self {
            kind: 0,
            level: 0,
            cell: CELLS,
            hp: 0.,
            cool: 0.,
            skill: 0.,
            flash: 0.,
            target: 0,
            attacks: 0,
            casts: 0,
            starved: false,
            jam: 0.,
            invested: 0.,
        }
    }
}
#[derive(Clone, Copy, Debug, Default)]
struct Gpu {
    model: usize,
    tier: u32,
    invested: f32,
    generated: f32,
}
#[derive(Clone, Copy, Debug, Default)]
struct Enemy {
    active: bool,
    kind: usize,
    cell: usize,
    next: usize,
    x: f32,
    y: f32,
    hp: f32,
    max_hp: f32,
    speed: f32,
    slow: f32,
    marked: f32,
    hit: f32,
    age: f32,
    special: f32,
    generation: u32,
    reward: f32,
    attacked: u32,
    route_revision: u32,
}
#[derive(Clone, Copy, Debug, Default)]
struct Shot {
    life: f32,
    x: f32,
    y: f32,
    tx: f32,
    ty: f32,
    kind: usize,
}
#[derive(Clone, Copy, Debug, Default)]
struct Effect {
    x: f32,
    y: f32,
    age: f32,
    life: f32,
    kind: usize,
    scale: f32,
    overlay: bool,
}
#[derive(Debug)]
struct Game {
    energy: f32,
    credits: f32,
    hp: f32,
    level: u32,
    unlocked: u32,
    wave: u32,
    phase: u32,
    kills: u32,
    elapsed: f32,
    wave_time: f32,
    spawn_clock: f32,
    spawned: u32,
    total: u32,
    paused: bool,
    speed: f32,
    feedback: u32,
    feedback_time: f32,
    last_cost: f32,
    spent_attack: f32,
    spent_skill: f32,
    terrain: [u8; CELLS],
    dist: [f32; CELLS],
    terrain_stage: u32,
    map_revision: u32,
    boss_phase: u32,
    boss_hp: f32,
    boss_killed: bool,
    route_changes: u32,
    units: [Unit; UNITS],
    gpus: [Gpu; 8],
    enemies: [Enemy; ENEMIES],
    shots: [Shot; SHOTS],
    effects: [Effect; FX],
    fx_cursor: usize,
}
fn xy(cell: usize) -> (f32, f32) {
    ((cell % W) as f32 - 11.5, 6.5 - (cell / W) as f32)
}
fn cell_at(x: f32, y: f32) -> usize {
    let c = (x + 12.).floor() as i32;
    let r = (7. - y).floor() as i32;
    if c < 0 || r < 0 || c >= W as i32 || r >= H as i32 {
        CELLS
    } else {
        r as usize * W + c as usize
    }
}
fn neighbors(cell: usize) -> [usize; 4] {
    let r = cell / W;
    let c = cell % W;
    [
        if c > 0 { cell - 1 } else { CELLS },
        if r > 0 { cell - W } else { CELLS },
        if c + 1 < W { cell + 1 } else { CELLS },
        if r + 1 < H { cell + W } else { CELLS },
    ]
}
fn distance(a: (f32, f32), b: (f32, f32)) -> f32 {
    ((a.0 - b.0).powi(2) + (a.1 - b.1).powi(2)).sqrt()
}
fn passable(t: u8) -> bool {
    t != 1 && t != 2
}

fn progress_path() -> Option<std::path::PathBuf> {
    if let Some(dir) = std::env::var_os("FORGE_GAME_SAVE_DIR") {
        return Some(std::path::PathBuf::from(dir).join("code-sentinels-v2.txt"));
    }
    std::env::var_os("FORGE_PROJECT_ROOT")
        .map(|root| std::path::PathBuf::from(root).join(".forge/save/code-sentinels-v2.txt"))
}
fn load_progress() -> u32 {
    progress_path()
        .and_then(|path| std::fs::read_to_string(path).ok())
        .and_then(|text| text.trim().parse::<u32>().ok())
        .unwrap_or(1)
        .clamp(1, 3)
}
fn save_progress(unlocked: u32) {
    if let Some(path) = progress_path() {
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let _ = std::fs::write(path, unlocked.clamp(1, 3).to_string());
    }
}
fn tile_cost(t: u8) -> f32 {
    match t {
        3 | 7 => 0.76,
        4 => 1.3,
        6 => 1.6,
        _ => 1.,
    }
}
fn spawn_cells(level: u32) -> Vec<usize> {
    match level {
        1 => vec![3 * W + 23, 10 * W + 23, 16],
        2 => vec![11, 6 * W + 23, 13 * W + 15],
        _ => vec![2 * W + 23, 11 * W + 23, 9, 13 * W + 8],
    }
}
fn terrain_for(level: u32, stage: u32) -> [u8; CELLS] {
    let mut t = [0u8; CELLS];
    for r in 0..H {
        for c in 0..W {
            let i = r * W + c;
            match level {
                1 => {
                    if r == 6 || r == 10 {
                        t[i] = 3;
                    }
                    if (c == 10 && (2..=11).contains(&r) && r != 6 && r != 10)
                        || (c == 17 && (1..=4).contains(&r))
                    {
                        t[i] = 1;
                    }
                    if r == 8 && (7..=21).contains(&c) {
                        t[i] = 2;
                    }
                    if r == 8 && (c == 13 || c == 20) {
                        t[i] = 7;
                    }
                    if (4..=6).contains(&c) && (2..=4).contains(&r) {
                        t[i] = 4;
                    }
                    if stage >= 1 && r == 8 && c == 13 {
                        t[i] = 2;
                    }
                    if stage >= 1 && r == 10 && c == 10 {
                        t[i] = 1;
                    }
                    if stage >= 2 && c == 10 && (3..=5).contains(&r) {
                        t[i] = 3;
                    }
                }
                2 => {
                    if r == 2 || r == 11 {
                        t[i] = 3;
                    }
                    if (7..=14).contains(&c) && (3..=10).contains(&r) {
                        t[i] = 2;
                    }
                    if r == 6 && (7..=14).contains(&c) {
                        t[i] = 7;
                    }
                    if c == 5 && (1..=4).contains(&r) {
                        t[i] = 1;
                    }
                    if (16..=20).contains(&c) && (1..=4).contains(&r) {
                        t[i] = 4;
                    }
                    if stage >= 1 && r == 6 && (7..=14).contains(&c) {
                        t[i] = 2;
                    }
                    if stage >= 2 && r == 7 && (7..=14).contains(&c) {
                        t[i] = 7;
                    }
                }
                _ => {
                    if (10..=20).contains(&c) && (3..=11).contains(&r) {
                        t[i] = 4;
                    }
                    if c == 8 && r <= 12 && r != 2 && r != 10 {
                        t[i] = 1;
                    }
                    if r == 2 || r == 10 {
                        t[i] = 3;
                    }
                    if c == 17 && (4..=9).contains(&r) {
                        t[i] = 2;
                    }
                    if r == 6 && c == 17 {
                        t[i] = 7;
                    }
                    if (c == 5 && r == 4) || (c == 4 && r == 10) || (c == 20 && r == 6) {
                        t[i] = 1;
                    }
                    if stage >= 1 && c == 8 && (5..=7).contains(&r) {
                        t[i] = 3;
                    }
                    if stage >= 1 && c == 8 && r == 10 {
                        t[i] = 1;
                    }
                    if stage >= 1 && (12..=14).contains(&c) && (5..=7).contains(&r) {
                        t[i] = 6;
                    }
                    if stage >= 2 && c == 17 && (5..=8).contains(&r) {
                        t[i] = 7;
                    }
                }
            }
            if c <= 3 && (5..=8).contains(&r) {
                t[i] = 5;
            }
        }
    }
    t[BASE] = 5;
    for s in spawn_cells(level) {
        t[s] = 3;
    }
    t
}
impl Game {
    fn new(level: u32, unlocked: u32) -> Self {
        let mut g = Self {
            energy: 0.,
            credits: 560.,
            hp: 20.,
            level,
            unlocked: unlocked.max(level),
            wave: 0,
            phase: 0,
            kills: 0,
            elapsed: 0.,
            wave_time: 0.,
            spawn_clock: 0.,
            spawned: 0,
            total: 0,
            paused: false,
            speed: 1.,
            feedback: 0,
            feedback_time: 0.,
            last_cost: 0.,
            spent_attack: 0.,
            spent_skill: 0.,
            terrain: terrain_for(level, 0),
            dist: [INF; CELLS],
            terrain_stage: 0,
            map_revision: 1,
            boss_phase: 0,
            boss_hp: 0.,
            boss_killed: false,
            route_changes: 0,
            units: [Unit::default(); UNITS],
            gpus: [Gpu::default(); 8],
            enemies: [Enemy::default(); ENEMIES],
            shots: [Shot::default(); SHOTS],
            effects: [Effect::default(); FX],
            fx_cursor: 0,
        };
        g.routes();
        g
    }
    fn feedback(&mut self, n: u32, cost: f32) {
        self.feedback = n;
        self.feedback_time = 3.;
        self.last_cost = cost;
    }
    fn gpu_count(&self) -> usize {
        self.gpus.iter().filter(|g| g.model > 0).count()
    }
    fn gpu_rate(g: Gpu) -> f32 {
        GPU_RATE[g.model] * (1. + g.tier.saturating_sub(1) as f32 * 0.55)
    }
    fn gpu_cap(g: Gpu) -> f32 {
        GPU_CAP[g.model] * (1. + g.tier.saturating_sub(1) as f32 * 0.3)
    }
    fn rate(&self) -> f32 {
        self.gpus.iter().map(|g| Self::gpu_rate(*g)).sum()
    }
    fn cap(&self) -> f32 {
        self.gpus.iter().map(|g| Self::gpu_cap(*g)).sum()
    }
    fn occupied(&self, cell: usize) -> bool {
        self.units.iter().any(|u| u.kind > 0 && u.cell == cell)
    }
    fn routes(&mut self) {
        self.dist = [INF; CELLS];
        self.dist[BASE] = 0.;
        let mut done = [false; CELLS];
        let mut blocked = [false; CELLS];
        for u in &self.units {
            if u.kind > 0 && u.cell < CELLS {
                blocked[u.cell] = true;
            }
        }
        for _ in 0..CELLS {
            let mut best = CELLS;
            let mut value = INF;
            for i in 0..CELLS {
                if !done[i] && self.dist[i] < value {
                    value = self.dist[i];
                    best = i;
                }
            }
            if best == CELLS {
                break;
            }
            done[best] = true;
            for n in neighbors(best) {
                if n < CELLS && !done[n] && passable(self.terrain[n]) && !blocked[n] {
                    let d = value + tile_cost(self.terrain[best]);
                    if d < self.dist[n] {
                        self.dist[n] = d;
                    }
                }
            }
        }
    }
    fn route_open(&self) -> bool {
        spawn_cells(self.level).iter().all(|i| self.dist[*i] < INF)
    }
    fn next_cell(&self, cell: usize, variant: usize) -> usize {
        if cell >= CELLS {
            return BASE;
        }
        let ns = neighbors(cell);
        let mut next = cell;
        let mut val = self.dist[cell];
        for shift in 0..4 {
            let n = ns[(shift + variant) % 4];
            if n < CELLS && self.dist[n] < val - 0.001 {
                val = self.dist[n];
                next = n;
            }
        }
        next
    }
    fn closest_open(&self, from: usize) -> usize {
        let origin = xy(from.min(CELLS - 1));
        (0..CELLS)
            .filter(|c| self.dist[*c] < INF && passable(self.terrain[*c]) && !self.occupied(*c))
            .min_by(|a, b| distance(xy(*a), origin).total_cmp(&distance(xy(*b), origin)))
            .unwrap_or(BASE)
    }
    fn terrain_change(&mut self, stage: u32) {
        if stage <= self.terrain_stage {
            return;
        }
        self.terrain_stage = stage;
        self.terrain = terrain_for(self.level, stage);
        self.map_revision += 1;
        self.routes();
        for i in 0..UNITS {
            if self.units[i].kind > 0 && !passable(self.terrain[self.units[i].cell]) {
                let cell = self.closest_open(self.units[i].cell);
                self.units[i].cell = cell;
                self.units[i].hp -= 20.;
            }
        }
        self.routes();
        for i in 0..ENEMIES {
            if self.enemies[i].active {
                let cell = self.enemies[i].cell;
                if self.dist[cell] >= INF {
                    let n = self.closest_open(cell);
                    let (x, y) = xy(n);
                    self.enemies[i].cell = n;
                    self.enemies[i].x = x;
                    self.enemies[i].y = y;
                }
                self.enemies[i].next = self.enemies[i].cell;
                self.enemies[i].route_revision = self.map_revision;
            }
        }
        self.route_changes += 1;
        self.feedback(if stage == 1 { 40 } else { 41 }, 0.);
    }
    fn combo(&self, index: usize) -> u32 {
        let u = self.units[index];
        if u.kind == 0 {
            return 0;
        }
        let origin = xy(u.cell);
        let mut mask = 0u32;
        for other in &self.units {
            if other.kind > 0 && distance(origin, xy(other.cell)) <= 4.5 {
                mask |= 1 << (other.kind - 1);
            }
        }
        if mask == 15 {
            3
        } else if mask & 3 == 3 && mask & 12 != 0 {
            2
        } else if mask & 3 == 3 || mask & 12 == 12 {
            1
        } else {
            0
        }
    }
    fn unit_range(&self, i: usize) -> f32 {
        let u = self.units[i];
        if u.kind == 0 {
            return 0.;
        }
        RANGE[u.kind]
            + u.level.saturating_sub(1) as f32 * 0.35
            + if self.terrain[u.cell] == 4 { 1.25 } else { 0. }
    }
    fn attack_cost(&self, i: usize) -> f32 {
        let u = self.units[i];
        ATTACK_ENERGY[u.kind] * (1. + u.level.saturating_sub(1) as f32 * 0.18)
    }
    fn emit(&mut self, x: f32, y: f32, kind: usize, life: f32, scale: f32, overlay: bool) {
        let i = self.fx_cursor % FX;
        self.fx_cursor += 1;
        self.effects[i] = Effect {
            x,
            y,
            kind,
            life,
            scale,
            overlay,
            age: 0.,
        };
    }
    fn los(&self, cell: usize, ex: f32, ey: f32) -> bool {
        if self.terrain[cell] == 4 {
            return true;
        }
        let (a, b) = xy(cell);
        let d = distance((a, b), (ex, ey));
        let steps = (d * 5.).ceil() as usize;
        for n in 1..steps {
            let t = n as f32 / steps as f32;
            let c = cell_at(a + (ex - a) * t, b + (ey - b) * t);
            if c < CELLS && self.terrain[c] == 1 {
                return false;
            }
        }
        true
    }
    fn command(&mut self, raw: f32) -> f32 {
        if !raw.is_finite() || raw.fract() != 0. {
            self.feedback(8, 0.);
            return 0.;
        }
        let code = raw as i32;
        if code == 0 {
            return 0.;
        }
        if code == 8_000_000 {
            let (level, unlocked) = (self.level, self.unlocked);
            *self = Self::new(level, unlocked);
            return 1.;
        }
        if code == 6_000_000 {
            self.paused = !self.paused;
            return 1.;
        }
        if code == 7_000_000 {
            self.speed = if self.speed == 1. {
                2.
            } else if self.speed == 2. {
                3.
            } else {
                1.
            };
            return 1.;
        }
        if (5_100_001..=5_100_003).contains(&code) {
            let level = (code - 5_100_000) as u32;
            if level > self.unlocked {
                self.feedback(32, 0.);
                return 0.;
            }
            *self = Self::new(level, self.unlocked);
            return 1.;
        }
        if code == 5_200_000 {
            if self.phase != 2 || self.level >= 3 {
                self.feedback(33, 0.);
                return 0.;
            }
            let mut next = Self::new(self.level + 1, self.unlocked);
            next.gpus = self.gpus;
            next.energy = self.energy;
            next.credits =
                self.credits + 200. + self.units.iter().map(|u| u.invested * 0.7).sum::<f32>();
            *self = next;
            return 1.;
        }
        if self.phase >= 2 {
            self.feedback(9, 0.);
            return 0.;
        }
        if (1_001_000..=1_004_335).contains(&code) {
            let kind = ((code - 1_000_000) / 1000) as usize;
            let cell = ((code - 1_000_000) % 1000) as usize;
            if kind < 1 || kind > 4 || cell >= CELLS {
                self.feedback(8, 0.);
                return 0.;
            }
            if !matches!(self.terrain[cell], 0 | 3 | 4 | 7) || cell % W < 4 {
                self.feedback(34, 0.);
                return 0.;
            }
            if self.occupied(cell) {
                self.feedback(2, 0.);
                return 0.;
            }
            if self
                .enemies
                .iter()
                .any(|e| e.active && distance((e.x, e.y), xy(cell)) < 0.8)
            {
                self.feedback(35, 0.);
                return 0.;
            }
            if self.credits < COST[kind] {
                self.feedback(1, COST[kind]);
                return 0.;
            }
            if self.gpu_count() == 0 && self.credits - COST[kind] < GPU_COST[1] {
                self.feedback(31, GPU_COST[1]);
                return 0.;
            }
            let Some(slot) = self.units.iter().position(|u| u.kind == 0) else {
                self.feedback(36, 0.);
                return 0.;
            };
            self.units[slot] = Unit {
                kind,
                level: 1,
                cell,
                hp: 130.,
                cool: 0.1,
                invested: COST[kind],
                ..Unit::default()
            };
            self.routes();
            if !self.route_open() {
                self.units[slot] = Unit::default();
                self.routes();
                self.feedback(37, 0.);
                return 0.;
            }
            self.credits -= COST[kind];
            self.map_revision += 1;
            self.feedback(10, COST[kind]);
            return 1.;
        }
        if (2_000_000..2_000_024).contains(&code) {
            let i = (code - 2_000_000) as usize;
            let u = self.units[i];
            if u.kind == 0 {
                self.feedback(3, 0.);
                return 0.;
            }
            if u.level >= 3 {
                self.feedback(4, 0.);
                return 0.;
            }
            let price = (COST[u.kind] * (0.65 + u.level as f32 * 0.2)).round();
            if self.credits < price {
                self.feedback(1, price);
                return 0.;
            }
            self.credits -= price;
            self.units[i].level += 1;
            self.units[i].hp += 45.;
            self.units[i].invested += price;
            self.feedback(11, price);
            return 1.;
        }
        if (2_100_000..2_100_024).contains(&code) {
            let i = (code - 2_100_000) as usize;
            let u = self.units[i];
            if u.kind == 0 {
                self.feedback(3, 0.);
                return 0.;
            }
            self.credits += (u.invested * 0.7).round();
            self.units[i] = Unit::default();
            self.map_revision += 1;
            self.routes();
            self.feedback(12, 0.);
            return 1.;
        }
        if (3_000_000..=3_023_335).contains(&code) {
            let slot = ((code - 3_000_000) / 1000) as usize;
            let cell = ((code - 3_000_000) % 1000) as usize;
            return self.skill(slot, cell);
        }
        if (3_100_000..=3_100_232).contains(&code) {
            let slot = ((code - 3_100_000) / 10) as usize;
            let mode = ((code - 3_100_000) % 10) as u32;
            if slot >= UNITS || mode > 2 || self.units[slot].kind == 0 {
                self.feedback(8, 0.);
                return 0.;
            }
            self.units[slot].target = mode;
            self.feedback(18, 0.);
            return 1.;
        }
        if (4_000_001..=4_000_077).contains(&code) {
            let slot = ((code - 4_000_000) / 10) as usize;
            let model = ((code - 4_000_000) % 10) as usize;
            if slot >= 8 || model < 1 || model > 7 {
                self.feedback(8, 0.);
                return 0.;
            }
            if self.gpus[slot].model > 0 {
                self.feedback(2, 0.);
                return 0.;
            }
            let price = GPU_COST[model];
            if self.credits < price {
                self.feedback(1, price);
                return 0.;
            }
            self.credits -= price;
            self.gpus[slot] = Gpu {
                model,
                tier: 1,
                invested: price,
                generated: 0.,
            };
            self.feedback(20, price);
            return 1.;
        }
        if (4_100_000..4_100_008).contains(&code) {
            let i = (code - 4_100_000) as usize;
            let g = self.gpus[i];
            if g.model == 0 {
                self.feedback(3, 0.);
                return 0.;
            }
            if g.tier >= 3 {
                self.feedback(4, 0.);
                return 0.;
            }
            let price = (GPU_COST[g.model] * (0.5 + g.tier as f32 * 0.2)).round();
            if self.credits < price {
                self.feedback(1, price);
                return 0.;
            }
            self.credits -= price;
            self.gpus[i].tier += 1;
            self.gpus[i].invested += price;
            self.feedback(21, price);
            return 1.;
        }
        if (4_200_000..4_200_008).contains(&code) {
            let i = (code - 4_200_000) as usize;
            if self.gpus[i].model == 0 {
                self.feedback(3, 0.);
                return 0.;
            }
            if self.gpu_count() == 1 {
                self.feedback(38, 0.);
                return 0.;
            }
            self.credits += (self.gpus[i].invested * 0.7).round();
            self.gpus[i] = Gpu::default();
            self.energy = self.energy.min(self.cap());
            self.feedback(22, 0.);
            return 1.;
        }
        if code == 5_000_000 {
            if self.gpu_count() == 0 {
                self.feedback(30, 130.);
                return 0.;
            }
            if self.phase != 0 {
                self.feedback(7, 0.);
                return 0.;
            }
            self.wave += 1;
            self.phase = 1;
            self.spawned = 0;
            self.total = 8 + self.wave * 2 + (self.level - 1) * 3;
            self.spawn_clock = 0.4;
            self.wave_time = 0.;
            self.feedback(14, 0.);
            return 1.;
        }
        self.feedback(8, 0.);
        0.
    }
    fn skill(&mut self, slot: usize, cell: usize) -> f32 {
        if slot >= UNITS || cell >= CELLS || self.units[slot].kind == 0 {
            self.feedback(3, 0.);
            return 0.;
        }
        let u = self.units[slot];
        if u.skill > 0. {
            self.feedback(5, u.skill);
            return 0.;
        }
        let price = SKILL_ENERGY[u.kind];
        if self.energy < price {
            self.feedback(23, price);
            return 0.;
        }
        let center = xy(cell);
        let targets: Vec<usize> = self
            .enemies
            .iter()
            .enumerate()
            .filter(|(_, e)| {
                e.active && distance((e.x, e.y), center) < if u.kind == 1 { 1.8 } else { 3.1 }
            })
            .map(|(i, _)| i)
            .collect();
        if targets.is_empty() && u.kind != 4 {
            self.feedback(6, 0.);
            return 0.;
        }
        self.energy -= price;
        self.spent_skill += price;
        self.units[slot].skill = SKILL_CD[u.kind];
        self.units[slot].casts += 1;
        self.units[slot].flash = 0.65;
        self.emit(center.0, center.1, u.kind, 2., 2.8, true);
        if u.kind == 4 {
            self.hp = (self.hp + 5.).min(20.);
            for ally in &mut self.units {
                if ally.kind > 0 && distance(xy(ally.cell), center) < 3.2 {
                    ally.hp = (ally.hp + 55.).min(130. + 45. * ally.level.saturating_sub(1) as f32);
                    ally.jam = 0.;
                }
            }
        }
        for i in targets {
            match u.kind {
                1 => {
                    self.enemies[i].marked = 8.;
                    self.enemies[i].slow = 2.;
                    self.hurt(i, 145. + u.level as f32 * 28., true);
                }
                2 => {
                    self.enemies[i].marked = 8.;
                    self.enemies[i].special = 8.;
                    self.hurt(
                        i,
                        if self.enemies[i].kind == 1 {
                            190.
                        } else {
                            100.
                        },
                        true,
                    );
                }
                3 => {
                    self.enemies[i].slow = 7.;
                    self.enemies[i].marked = 7.;
                    self.hurt(i, 60., true);
                }
                _ => {
                    self.enemies[i].slow = 3.;
                    self.hurt(i, 55., true);
                }
            }
        }
        self.feedback(13, price);
        1.
    }
    fn spawn_enemy(&mut self, kind: usize, cell: usize, generation: u32, scaled: f32) -> bool {
        let Some(i) = self.enemies.iter().position(|e| !e.active) else {
            return false;
        };
        let (x, y) = xy(cell);
        let hp = (38. + self.wave as f32 * 10. + self.level as f32 * 16.)
            * [1., 1.35, 0.72, 1.85, 1.1, 10., 12., 15.][kind]
            * scaled;
        self.enemies[i] = Enemy {
            active: true,
            kind,
            cell,
            next: cell,
            x,
            y,
            hp,
            max_hp: hp,
            speed: [0.82, 0.6, 1.35, 0.58, 0.76, 0.36, 0.32, 0.3][kind] + self.level as f32 * 0.025,
            special: 2.5,
            generation,
            reward: if kind >= 5 {
                180.
            } else {
                10. + kind as f32 * 2.
            },
            route_revision: self.map_revision,
            ..Enemy::default()
        };
        true
    }
    fn spawn(&mut self) {
        let spawns = spawn_cells(self.level);
        let cell = spawns[(self.spawned as usize + self.wave as usize) % spawns.len()];
        let kind = if self.wave == 4 && self.spawned + 1 == self.total {
            self.level as usize + 4
        } else {
            let choices = if self.wave == 1 && self.level == 1 {
                2
            } else {
                5
            };
            ((self.spawned + self.wave + self.level) % choices) as usize
        };
        if self.spawn_enemy(kind, cell, 0, 1.) {
            self.spawned += 1;
            if kind >= 5 {
                self.boss_phase = 1;
                self.boss_hp = 1.;
            }
        }
    }
    fn hurt(&mut self, i: usize, damage: f32, true_damage: bool) {
        if !self.enemies[i].active {
            return;
        }
        self.enemies[i].attacked += 1;
        let e = self.enemies[i];
        if !true_damage && e.kind == 2 && e.marked <= 0. && e.attacked % 3 == 0 {
            return;
        }
        let armor = if !true_damage && (e.kind == 3 || e.kind == 6) && e.marked <= 0. {
            0.55
        } else {
            1.
        };
        self.enemies[i].hp -= damage * armor * if e.marked > 0. { 1.25 } else { 1. };
        self.enemies[i].hit = 0.15;
        if e.kind >= 5 {
            self.boss_hp = (self.enemies[i].hp / e.max_hp).max(0.);
            if self.boss_hp < 0.5 && self.boss_phase == 1 {
                self.boss_phase = 2;
                self.terrain_change(1);
            }
        }
        if self.enemies[i].hp <= 0. {
            self.enemies[i].active = false;
            self.kills += 1;
            self.credits += e.reward;
            self.emit(e.x, e.y, 2, 0.5, 0.7, false);
            if e.kind == 4 && e.generation == 0 {
                let cell = self.closest_open(e.cell);
                self.spawn_enemy(4, cell, 1, 0.35);
                self.spawn_enemy(4, cell, 1, 0.35);
            }
            if e.kind >= 5 {
                self.boss_phase = 3;
                self.boss_killed = true;
                self.terrain_change(2);
            }
        }
    }
    fn tick(&mut self, dt: f32) {
        if !dt.is_finite() || dt <= 0. {
            return;
        }
        let mut remaining = dt.min(0.25) * self.speed;
        while remaining > 0.000001 {
            let step = remaining.min(1. / 60.);
            self.step(step);
            remaining -= step;
        }
    }
    fn step(&mut self, dt: f32) {
        if self.paused || self.phase >= 2 {
            return;
        }
        self.elapsed += dt;
        self.feedback_time -= dt;
        if self.feedback_time <= 0. {
            self.feedback = 0;
        }
        let mut produced = 0.;
        for g in &mut self.gpus {
            let n = Self::gpu_rate(*g) * dt;
            g.generated += n;
            produced += n;
        }
        self.energy = (self.energy + produced).min(self.cap());
        for u in &mut self.units {
            u.cool = (u.cool - dt).max(0.);
            u.skill = (u.skill - dt).max(0.);
            u.flash = (u.flash - dt).max(0.);
            u.jam = (u.jam - dt).max(0.);
        }
        for s in &mut self.shots {
            s.life = (s.life - dt).max(0.);
        }
        for e in &mut self.effects {
            e.age += dt;
        }
        if self.phase != 1 {
            return;
        }
        self.wave_time += dt;
        self.spawn_clock -= dt;
        if self.spawned < self.total && self.spawn_clock <= 0. {
            self.spawn();
            self.spawn_clock = (1.7 - self.level as f32 * 0.15 - self.wave as f32 * 0.07).max(0.75);
        }
        for i in 0..ENEMIES {
            if !self.enemies[i].active {
                continue;
            }
            let old = self.enemies[i];
            self.enemies[i].age += dt;
            self.enemies[i].slow = (old.slow - dt).max(0.);
            self.enemies[i].marked = (old.marked - dt).max(0.);
            self.enemies[i].hit = (old.hit - dt).max(0.);
            self.enemies[i].special -= dt;
            if old.kind == 1 && old.marked <= 0. {
                self.energy = (self.energy - dt * 1.7).max(0.);
                self.enemies[i].hp = (self.enemies[i].hp + dt * 2.).min(old.max_hp);
            }
            if (old.kind == 3 || old.kind == 6) && old.special <= 0. {
                let mut targets = 0;
                for u in &mut self.units {
                    if u.kind > 0 && distance(xy(u.cell), (old.x, old.y)) < 3.3 {
                        u.jam = 2.2;
                        targets += 1;
                        if targets >= 2 {
                            break;
                        }
                    }
                }
                self.enemies[i].special = 5.;
                if targets > 0 {
                    self.emit(old.x, old.y, 2, 0.8, 1.2, false);
                }
            }
            if old.kind == 5 && old.special <= 0. {
                for u in &mut self.units {
                    if u.kind > 0 && distance(xy(u.cell), (old.x, old.y)) < 3.5 {
                        u.hp -= 12.;
                    }
                }
                self.enemies[i].special = 5.;
                self.energy = (self.energy - 14.).max(0.);
            }
            if old.kind == 7 && old.special <= 0. {
                let cell = self.closest_open(old.cell);
                self.spawn_enemy(4, cell, 1, 0.38);
                self.enemies[i].special = 6.;
            }
            if old.kind == 0 && old.special <= 0. {
                let a = self.next_cell(old.cell, 0);
                let b = self.next_cell(a, 0);
                let (x, y) = xy(b);
                self.enemies[i].cell = b;
                self.enemies[i].next = b;
                self.enemies[i].x = x;
                self.enemies[i].y = y;
                self.enemies[i].special = 6.;
                self.emit(x, y, 4, 0.35, 0.6, false);
            }
            let e = self.enemies[i];
            let target = if e.next == e.cell || distance((e.x, e.y), xy(e.next)) < 0.035 {
                self.next_cell(e.cell, if e.kind == 2 { i % 4 } else { 0 })
            } else {
                e.next
            };
            self.enemies[i].next = target;
            let (tx, ty) = xy(target);
            let d = distance((e.x, e.y), (tx, ty));
            let sprint = if e.kind == 2 && (e.age as i32 % 4) < 2 {
                1.35
            } else {
                1.
            };
            let amount = e.speed * dt * sprint * if e.slow > 0. { 0.4 } else { 1. }
                / tile_cost(self.terrain[e.cell]).sqrt();
            if d <= amount || d < 0.035 {
                self.enemies[i].x = tx;
                self.enemies[i].y = ty;
                self.enemies[i].cell = target;
            } else {
                self.enemies[i].x += (tx - e.x) / d * amount;
                self.enemies[i].y += (ty - e.y) / d * amount;
            }
            if target == BASE && distance((self.enemies[i].x, self.enemies[i].y), xy(BASE)) < 0.2 {
                self.hp = (self.hp
                    - if e.kind >= 5 {
                        20.
                    } else if e.kind == 3 {
                        2.
                    } else {
                        1.
                    })
                .max(0.);
                self.enemies[i].active = false;
            }
        }
        for slot in 0..UNITS {
            if self.units[slot].kind > 0 && self.units[slot].hp <= 0. {
                self.units[slot] = Unit::default();
                self.map_revision += 1;
                self.routes();
                continue;
            }
            let u = self.units[slot];
            if u.kind == 0 || u.cool > 0. || u.jam > 0. {
                continue;
            }
            let origin = xy(u.cell);
            let range = self.unit_range(slot);
            let mut target = None;
            let mut priority = INF;
            for (i, e) in self.enemies.iter().enumerate() {
                if !e.active || distance(origin, (e.x, e.y)) > range || !self.los(u.cell, e.x, e.y)
                {
                    continue;
                }
                let score = match u.target {
                    1 => e.hp,
                    2 => {
                        if e.kind >= 5 {
                            -1000. + e.hp / e.max_hp
                        } else {
                            self.dist[e.cell]
                        }
                    }
                    _ => self.dist[e.cell],
                };
                if score < priority {
                    priority = score;
                    target = Some(i);
                }
            }
            let Some(target) = target else {
                self.units[slot].starved = false;
                continue;
            };
            let price = self.attack_cost(slot);
            if self.energy + 0.0001 < price {
                self.units[slot].starved = true;
                continue;
            }
            self.energy = (self.energy - price).max(0.);
            self.spent_attack += price;
            self.units[slot].starved = false;
            self.units[slot].attacks += 1;
            let combo = self.combo(slot);
            let t = self.enemies[target];
            let mult = 1. + u.level.saturating_sub(1) as f32 * 0.65 + combo as f32 * 0.13;
            self.units[slot].cool =
                RATE[u.kind] / (1. + u.level.saturating_sub(1) as f32 * 0.1 + combo as f32 * 0.04);
            self.units[slot].flash = 0.2;
            let si = self
                .shots
                .iter()
                .position(|s| s.life <= 0.)
                .unwrap_or(slot % SHOTS);
            self.shots[si] = Shot {
                life: 0.18,
                x: origin.0,
                y: origin.1 + 0.25,
                tx: t.x,
                ty: t.y + 0.2,
                kind: u.kind,
            };
            self.emit(t.x, t.y, u.kind, 0.4, 0.45, false);
            if u.kind == 3 {
                self.enemies[target].slow = 2.3;
                self.enemies[target].marked = 2.5;
            }
            let high_penalty =
                if self.terrain[u.cell] != 4 && self.terrain[t.cell] == 4 && t.marked <= 0. {
                    0.8
                } else {
                    1.
                };
            self.hurt(
                target,
                DAMAGE[u.kind] * mult * high_penalty,
                u.kind == 2 && combo > 0,
            );
            if u.kind == 2 {
                for other in 0..ENEMIES {
                    if other != target
                        && self.enemies[other].active
                        && distance((self.enemies[other].x, self.enemies[other].y), (t.x, t.y))
                            < 1.25
                    {
                        self.hurt(other, DAMAGE[u.kind] * mult * 0.6, combo > 0);
                    }
                }
            }
        }
        if self.hp <= 0. {
            self.phase = 3;
            self.feedback(16, 0.);
            return;
        }
        if self.spawned >= self.total && !self.enemies.iter().any(|e| e.active) {
            self.credits += 90. + self.level as f32 * 30. + self.wave as f32 * 10.;
            if self.wave == 4 {
                if self.boss_killed {
                    self.phase = 2;
                    self.unlocked = self.unlocked.max((self.level + 1).min(3));
                    save_progress(self.unlocked);
                    self.feedback(15, 0.);
                } else {
                    self.phase = 3;
                    self.feedback(16, 0.);
                }
            } else {
                self.phase = 0;
                self.feedback(17, 0.);
            }
        }
    }
    fn read(&self, key: i32) -> f32 {
        match key {
            0 => self.energy,
            1 => self.hp,
            2 => self.wave as f32,
            3 => self.phase as f32,
            4 => self.kills as f32,
            5 => self.level as f32,
            6 => self.credits,
            7 => self.rate(),
            8 => self.cap(),
            9 => self.spawned as f32,
            10 => self.total as f32,
            11 => self.enemies.iter().filter(|e| e.active).count() as f32,
            12 => self.speed,
            13 => self.paused as u8 as f32,
            14 => self.map_revision as f32,
            15 => self.feedback as f32,
            16 => self.elapsed,
            17 => (0..UNITS).map(|i| self.combo(i)).max().unwrap_or(0) as f32,
            18 => self.boss_phase as f32,
            19 => self.boss_hp,
            20 => self.terrain_stage as f32,
            21 => self.unlocked as f32,
            22 => self.spent_attack,
            23 => self.spent_skill,
            24 => self.gpu_count() as f32,
            25 => self.last_cost,
            26 => 4.,
            27 => BASE as f32,
            28 => self.route_changes as f32,
            1000..=1479 => {
                let i = ((key - 1000) / 20) as usize;
                let f = (key - 1000) % 20;
                let u = self.units[i];
                match f {
                    0 => u.kind as f32,
                    1 => u.level as f32,
                    2 => {
                        if u.kind == 0 {
                            -1.
                        } else {
                            u.cell as f32
                        }
                    }
                    3 => u.hp,
                    4 => u.skill,
                    5 => SKILL_ENERGY[u.kind],
                    6 => self.attack_cost(i),
                    7 => self.unit_range(i),
                    8 => u.target as f32,
                    9 => u.attacks as f32,
                    10 => u.casts as f32,
                    11 => u.starved as u8 as f32,
                    12 => u.invested,
                    13 => u.jam,
                    14 => u.flash,
                    15 => {
                        if u.kind > 0 {
                            xy(u.cell).0
                        } else {
                            -100.
                        }
                    }
                    16 => {
                        if u.kind > 0 {
                            xy(u.cell).1
                        } else {
                            -100.
                        }
                    }
                    17 => (u.invested * 0.7).round(),
                    18 => {
                        if u.kind > 0 && u.level < 3 {
                            (COST[u.kind] * (0.65 + u.level as f32 * 0.2)).round()
                        } else {
                            0.
                        }
                    }
                    19 => {
                        if u.kind > 0 {
                            130. + 45. * u.level.saturating_sub(1) as f32
                        } else {
                            0.
                        }
                    }
                    _ => 0.,
                }
            }
            2000..=2079 => {
                let i = ((key - 2000) / 10) as usize;
                let f = (key - 2000) % 10;
                let g = self.gpus[i];
                match f {
                    0 => g.model as f32,
                    1 => g.tier as f32,
                    2 => Self::gpu_rate(g),
                    3 => GPU_COST[g.model],
                    4 => {
                        if g.tier < 3 {
                            (GPU_COST[g.model] * (0.5 + g.tier as f32 * 0.2)).round()
                        } else {
                            0.
                        }
                    }
                    5 => (g.invested * 0.7).round(),
                    6 => Self::gpu_cap(g),
                    7 => g.generated,
                    _ => 0.,
                }
            }
            3000..=3139 => {
                let row = ((key - 3000) / 10) as usize;
                let f = (key - 3000) % 10;
                if f < 4 {
                    let mut packed = 0u32;
                    for c in 0..6 {
                        packed |= (self.terrain[row * W + f as usize * 6 + c] as u32) << (c * 3);
                    }
                    packed as f32
                } else if f == 4 {
                    self.map_revision as f32
                } else {
                    self.level as f32
                }
            }
            4000..=4639 => {
                let e = self.enemies[((key - 4000) / 10) as usize];
                match (key - 4000) % 10 {
                    0 => {
                        if e.active {
                            e.x
                        } else {
                            -100.
                        }
                    }
                    1 => {
                        if e.active {
                            e.y
                        } else {
                            -100.
                        }
                    }
                    2 => e.kind as f32,
                    3 => (e.hp / e.max_hp.max(1.)).max(0.),
                    4 => e.active as u8 as f32,
                    5 => e.route_revision as f32,
                    6 => e.slow,
                    7 => e.marked,
                    8 => e.generation as f32,
                    _ => e.age,
                }
            }
            5000..=5319 => {
                let s = self.shots[((key - 5000) / 10) as usize];
                let t = (1. - s.life / 0.18).clamp(0., 1.);
                match (key - 5000) % 10 {
                    0 => {
                        if s.life > 0. {
                            s.x + (s.tx - s.x) * t
                        } else {
                            -100.
                        }
                    }
                    1 => {
                        if s.life > 0. {
                            s.y + (s.ty - s.y) * t
                        } else {
                            -100.
                        }
                    }
                    2 => s.kind as f32,
                    3 => s.life,
                    _ => 0.,
                }
            }
            6000..=6639 => {
                let e = self.effects[((key - 6000) / 10) as usize];
                let active = e.age < e.life && e.life > 0.;
                match (key - 6000) % 10 {
                    0 => {
                        if active {
                            e.x
                        } else {
                            -100.
                        }
                    }
                    1 => {
                        if active {
                            e.y
                        } else {
                            -100.
                        }
                    }
                    2 => e.age,
                    3 => e.life,
                    4 => e.kind as f32,
                    5 => active as u8 as f32,
                    6 => e.overlay as u8 as f32,
                    7 => e.scale,
                    _ => 0.,
                }
            }
            _ => 0.,
        }
    }
}
static GAME: Mutex<Option<Game>> = Mutex::new(None);
fn game<T>(f: impl FnOnce(&mut Game) -> T) -> T {
    let mut g = GAME.lock().unwrap_or_else(|p| p.into_inner());
    f(g.get_or_insert_with(|| Game::new(1, load_progress())))
}
#[no_mangle]
pub extern "C" fn cs_reset() -> f32 {
    game(|g| {
        *g = Game::new(1, load_progress());
        1.
    })
}
#[no_mangle]
pub extern "C" fn cs_tick(dt: f32) -> f32 {
    game(|g| {
        g.tick(dt);
        g.phase as f32
    })
}
#[no_mangle]
pub extern "C" fn cs_input(command: f32) -> f32 {
    game(|g| g.command(command))
}
#[no_mangle]
pub extern "C" fn cs_get(key: f32) -> f32 {
    game(|g| g.read(key as i32))
}
#[no_mangle]
pub extern "C" fn cs_map_template(code: f32) -> f32 {
    let n = code as usize;
    let l = n / 10000;
    let stage = (n % 10000) / 1000;
    let c = n % 1000;
    if !(1..=3).contains(&l) || stage > 2 || c >= CELLS {
        return -1.;
    }
    terrain_for(l as u32, stage as u32)[c] as f32
}
#[no_mangle]
pub extern "C" fn cs_spawn_cell(code: f32) -> f32 {
    let n = code as u32;
    let level = n / 10;
    let index = (n % 10) as usize;
    if !(1..=3).contains(&level) {
        return -1.;
    }
    spawn_cells(level)
        .get(index)
        .map(|v| *v as f32)
        .unwrap_or(-1.)
}
fn visual(g: &Game, id: i32) -> ([f32; 3], [f32; 3], i32, i32) {
    let mut p = [-100., -100., 0.];
    let mut s = [1., 1., 1.];
    let mut frame = -1;
    let mut attack = 0;
    match id {
        0..=95 => {
            let u = g.units[id as usize / 4];
            if u.kind == id as usize % 4 + 1 {
                let (x, y) = xy(u.cell);
                p = [x, y, 0.];
                attack = (u.flash > 0.1) as i32;
            }
            let size =
                1. + u.level.saturating_sub(1) as f32 * 0.05 + if u.flash > 0. { 0.04 } else { 0. };
            s = [size, size, 1.];
        }
        100..=163 => {
            let e = g.enemies[(id - 100) as usize];
            if e.active {
                p = [e.x, e.y, 0.];
            }
            let size = if e.kind >= 5 { 1.7 } else { 0.78 }
                * if e.generation > 0 { 0.58 } else { 1. }
                * if e.hit > 0. { 1.06 } else { 1. };
            s = [size, size, 1.];
            frame = (e.kind * 2 + ((e.age * 5.) as usize % 2)) as i32;
        }
        200..=231 => {
            p = [
                g.read(5000 + (id - 200) * 10),
                g.read(5001 + (id - 200) * 10),
                0.,
            ];
            frame = g.shots[(id - 200) as usize].kind.saturating_sub(1) as i32;
        }
        300..=307 => {
            if g.gpus[(id - 300) as usize].model > 0 {
                p = [
                    -11.15 + ((id - 300) % 4) as f32 * 0.78,
                    0.95 - ((id - 300) / 4) as f32 * 1.05,
                    0.,
                ];
            }
            frame = g.gpus[(id - 300) as usize].model.saturating_sub(1) as i32;
        }
        400..=408 => {
            if id - 400 == ((g.level - 1) * 3 + g.terrain_stage) as i32 {
                p = [0., 0., 0.];
            }
        }
        500..=563 => {
            let e = g.effects[(id - 500) as usize];
            if e.overlay && e.age < e.life {
                p = [e.x, e.y, 0.];
            }
            s = [e.scale, e.scale, 1.];
            let base = match e.kind {
                3 => 0,
                4 => 48,
                _ => 96,
            };
            frame = (base + ((e.age * 24.) as usize).min(47)) as i32;
        }
        _ => {}
    }
    (p, s, frame, attack)
}
#[no_mangle]
pub extern "C" fn cs_visual_x(id: f32) -> f32 {
    game(|g| visual(g, id as i32).0[0])
}
#[no_mangle]
pub extern "C" fn cs_visual_y(id: f32) -> f32 {
    game(|g| visual(g, id as i32).0[1])
}
#[no_mangle]
pub extern "C" fn cs_visual_scale(id: f32) -> f32 {
    game(|g| visual(g, id as i32).1[0])
}
#[no_mangle]
pub extern "C" fn cs_visual_frame(id: f32) -> f32 {
    game(|g| visual(g, id as i32).2 as f32)
}
#[no_mangle]
pub extern "C" fn cs_visual_attacking(id: f32) -> bool {
    game(|g| visual(g, id as i32).3 != 0)
}

/// forge-logic native frame ABI v1. Layout is tested on both sides of the DLL.
#[repr(C)]
pub struct NativeBinding {
    pub entity_id: u64,
    pub kind: u32,
    pub data: [i32; 6],
}
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct NativeUpdate {
    pub entity_id: u64,
    pub translation: [f32; 3],
    pub scale: [f32; 3],
    pub frame: i32,
    pub animator_bool: i32,
}
#[no_mangle]
pub extern "C" fn cs_frame(
    abi: u32,
    dt: f32,
    bindings: *const NativeBinding,
    count: u32,
    updates: *mut NativeUpdate,
    capacity: u32,
) -> u32 {
    if abi != 1 || count > capacity || count > 4096 || bindings.is_null() || updates.is_null() {
        return 0;
    }
    let inputs = unsafe { std::slice::from_raw_parts(bindings, count as usize) };
    let output = unsafe { std::slice::from_raw_parts_mut(updates, count as usize) };
    game(|g| {
        g.tick(dt);
        for (i, b) in inputs.iter().enumerate() {
            let mut u = NativeUpdate {
                entity_id: b.entity_id,
                translation: [0.; 3],
                scale: [1.; 3],
                frame: -1,
                animator_bool: -1,
            };
            match b.kind {
                0 => {
                    u.translation = [g.read(b.data[0]), g.read(b.data[1]), g.read(b.data[2])];
                    u.scale = [g.read(b.data[3]), g.read(b.data[4]), g.read(b.data[5])];
                }
                1 | 2 | 3 => {
                    let (p, s, f, a) = visual(g, b.data[0]);
                    u.translation = p;
                    u.scale = s;
                    if b.kind == 2 {
                        u.frame = f;
                    }
                    if b.kind == 3 {
                        u.animator_bool = a;
                    }
                }
                4 => {
                    u.translation = [g.read(b.data[0]), g.read(b.data[1]), 0.];
                    u.scale = [g.read(b.data[2]), g.read(b.data[3]), g.read(b.data[4])];
                }
                _ => return 0,
            }
            output[i] = u;
        }
        count
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    fn seconds(g: &mut Game, s: f32) {
        for _ in 0..(s * 60.) as usize {
            g.tick(1. / 60.);
        }
    }
    fn buy(g: &mut Game, slot: usize, model: usize) -> f32 {
        g.command((4_000_000 + slot * 10 + model) as f32)
    }
    fn deploy(g: &mut Game, kind: usize, cell: usize) -> f32 {
        g.command((1_000_000 + kind * 1000 + cell) as f32)
    }
    fn path(g: &Game, start: usize) -> Vec<usize> {
        let mut out = vec![start];
        let mut c = start;
        for _ in 0..CELLS {
            if c == BASE {
                break;
            }
            let n = g.next_cell(c, 0);
            if n == c {
                break;
            }
            out.push(n);
            c = n;
        }
        out
    }
    #[test]
    fn gpu_is_a_required_player_action_and_only_energy_source() {
        let mut g = Game::new(1, 1);
        seconds(&mut g, 20.);
        assert_eq!(g.energy, 0.);
        assert_eq!(g.gpu_count(), 0);
        assert_eq!(g.command(5_000_000.), 0.);
        assert_eq!(g.feedback, 30);
        assert_eq!(buy(&mut g, 0, 1), 1.);
        assert_eq!(g.credits, 430.);
        seconds(&mut g, 5.);
        assert!((g.energy - 60.).abs() < 0.01);
        let e = g.energy;
        g.spawn_enemy(0, 4 * W + 7, 0, 1.);
        g.hurt(0, 9999., true);
        assert_eq!(g.energy, e, "kills must only award credits");
        assert!(g.credits > 430.);
        assert_eq!(g.command(4_200_000.), 0., "last GPU cannot be sold");
    }
    #[test]
    fn hardware_models_have_distinct_fictional_production() {
        for model in 1..8 {
            let mut g = Game::new(1, 1);
            g.credits = 5000.;
            buy(&mut g, 0, model);
            seconds(&mut g, 1.);
            assert!((g.energy - GPU_RATE[model]).abs() < 0.02);
        }
    }
    #[test]
    fn every_auto_attack_pays_energy_and_starvation_prevents_damage() {
        let mut g = Game::new(1, 1);
        buy(&mut g, 0, 1);
        deploy(&mut g, 1, 6 * W + 5);
        g.units[0].cool = 0.;
        g.phase = 1;
        g.total = 1;
        g.spawned = 1;
        g.spawn_clock = 99.;
        g.spawn_enemy(0, 6 * W + 7, 0, 1.);
        let hp = g.enemies[0].hp;
        g.tick(1. / 60.);
        assert_eq!(g.enemies[0].hp, hp);
        assert!(g.units[0].starved);
        g.energy = 10.;
        g.tick(1. / 60.);
        assert_eq!(g.units[0].attacks, 1);
        assert_eq!(g.spent_attack, 3.);
        assert!((g.energy - 7.2).abs() < 0.001);
        assert!(g.enemies[0].hp < hp);
    }
    #[test]
    fn initial_spending_reserves_the_cost_of_a_gpu() {
        let mut g = Game::new(1, 1);
        for cell in 0..CELLS {
            deploy(&mut g, 4, cell);
        }
        assert_eq!(g.gpu_count(), 0);
        assert!(g.credits >= 130.);
        assert_eq!(buy(&mut g, 0, 1), 1.);
    }
    #[test]
    fn impassable_tiles_and_route_sealing_deployments_are_rejected_atomically() {
        let mut g = Game::new(1, 1);
        let money = g.credits;
        assert_eq!(deploy(&mut g, 1, 8 * W + 8), 0.);
        assert_eq!(g.credits, money);
        g.terrain = [1; CELLS];
        for c in 0..W {
            g.terrain[7 * W + c] = 0;
        }
        for r in 0..=7 {
            g.terrain[r * W + 16] = 0;
        }
        for r in 3..=10 {
            g.terrain[r * W + 23] = 0;
        }
        g.terrain[BASE] = 5;
        g.routes();
        assert!(g.route_open());
        assert_eq!(deploy(&mut g, 1, 7 * W + 10), 0.);
        assert_eq!(g.feedback, 37);
        assert_eq!(g.credits, money);
        assert!(g.route_open());
    }
    #[test]
    fn each_boss_mutates_real_terrain_and_recomputes_an_observably_different_route() {
        for level in 1..=3 {
            let mut g = Game::new(level, level);
            assert!(g.route_open());
            let start = if level == 1 {
                10 * W + 23
            } else if level == 2 {
                6 * W + 23
            } else {
                11 * W + 23
            };
            let original = g.terrain;
            let before = path(&g, start);
            g.spawn_enemy(level as usize + 4, start, 0, 1.);
            g.boss_phase = 1;
            let hp = g.enemies[0].hp;
            g.hurt(0, hp * 0.55, true);
            assert_eq!(g.terrain_stage, 1);
            assert_ne!(g.terrain, original);
            assert!(g.route_open());
            assert_ne!(path(&g, start), before, "level {level} route must change");
            assert_eq!(g.enemies[0].route_revision, g.map_revision);
            g.hurt(0, 99999., true);
            assert_eq!(g.terrain_stage, 2);
            assert!(g.route_open());
            assert_eq!(g.route_changes, 2);
        }
    }
    #[test]
    fn all_four_targeted_skills_have_individual_cost_cooldown_and_effects() {
        for kind in 1..5 {
            let mut g = Game::new(1, 1);
            deploy(&mut g, kind, 6 * W + 5);
            g.energy = 200.;
            g.hp = 10.;
            g.spawn_enemy(1, 6 * W + 7, 0, 8.);
            let hp = g.enemies[0].hp;
            assert_eq!(g.skill(0, 6 * W + 7), 1.);
            assert_eq!(g.energy, 200. - SKILL_ENERGY[kind]);
            assert_eq!(g.spent_skill, SKILL_ENERGY[kind]);
            assert_eq!(g.units[0].skill, SKILL_CD[kind]);
            assert_eq!(g.units[0].casts, 1);
            assert!(g.enemies[0].hp < hp);
            assert_eq!(g.skill(0, 6 * W + 7), 0.);
            if kind == 3 {
                assert_eq!(g.enemies[0].slow, 7.);
            }
            if kind == 4 {
                assert_eq!(g.hp, 15.);
            }
            assert!(g.effects.iter().any(|e| e.overlay && e.kind == kind));
        }
    }
    #[test]
    fn high_ground_changes_range_and_obstruction_changes_line_of_sight() {
        let mut g = Game::new(1, 1);
        deploy(&mut g, 1, 3 * W + 5);
        assert!(g.unit_range(0) > RANGE[1] + 1.);
        g.terrain[g.units[0].cell] = 0;
        let cell = g.units[0].cell;
        g.terrain[cell + 1] = 1;
        let (x, y) = xy(cell + 2);
        assert!(!g.los(cell, x, y));
        g.terrain[cell] = 4;
        assert!(g.los(cell, x, y));
    }
    #[test]
    fn bug_archetypes_have_distinct_mechanics() {
        let mut g = Game::new(1, 1);
        g.phase = 1;
        g.total = 10;
        g.spawned = 10;
        g.spawn_clock = 99.;
        g.energy = 100.;
        g.spawn_enemy(1, 6 * W + 15, 0, 1.);
        g.enemies[0].hp -= 10.;
        let hp = g.enemies[0].hp;
        g.tick(0.1);
        assert!(g.energy < 100.);
        assert!(g.enemies[0].hp > hp);
        g.spawn_enemy(2, 6 * W + 16, 0, 1.);
        let hp = g.enemies[1].hp;
        for _ in 0..3 {
            g.hurt(1, 1., false);
        }
        assert_eq!(g.enemies[1].hp, hp - 2.);
        g.spawn_enemy(4, 6 * W + 17, 0, 1.);
        g.hurt(2, 9999., true);
        assert_eq!(
            g.enemies
                .iter()
                .filter(|e| e.active && e.kind == 4 && e.generation == 1)
                .count(),
            2
        );
    }
    #[test]
    fn target_selection_pause_and_campaign_restart_are_real_state() {
        let mut g = Game::new(1, 1);
        buy(&mut g, 0, 1);
        deploy(&mut g, 1, 6 * W + 5);
        assert_eq!(g.command(3_100_002.), 1.);
        assert_eq!(g.units[0].target, 2);
        seconds(&mut g, 2.);
        g.command(6_000_000.);
        let energy = g.energy;
        seconds(&mut g, 3.);
        assert_eq!(g.energy, energy);
        g.command(8_000_000.);
        assert_eq!(g.gpu_count(), 0);
        assert_eq!(g.credits, 560.);
        assert_eq!(g.energy, 0.);
        assert_eq!(g.command(5_100_003.), 0.);
        assert_eq!(g.level, 1);
    }
    fn candidate(g: &Game, kind: usize) -> Option<usize> {
        let paths: Vec<_> = spawn_cells(g.level)
            .iter()
            .flat_map(|s| path(g, *s))
            .collect();
        let mut list = Vec::new();
        for cell in 0..CELLS {
            if cell % W < 4
                || cell % W > 10
                || !matches!(g.terrain[cell], 0 | 3 | 4 | 7)
                || g.occupied(cell)
            {
                continue;
            }
            let pos = xy(cell);
            let range = RANGE[kind] + if g.terrain[cell] == 4 { 1.25 } else { 0. };
            let coverage = paths
                .iter()
                .filter(|c| distance(pos, xy(**c)) < range && g.los(cell, xy(**c).0, xy(**c).1))
                .count() as f32;
            let crowded = g
                .units
                .iter()
                .filter(|u| u.kind > 0 && distance(pos, xy(u.cell)) < 1.5)
                .count() as f32;
            list.push((coverage - crowded * 4. - (cell % W) as f32 * 0.35, cell));
        }
        list.sort_by(|a, b| b.0.total_cmp(&a.0));
        list.first().map(|(_, c)| *c)
    }
    fn invest(g: &mut Game) {
        if g.gpu_count() == 0 {
            buy(g, 0, 1);
        }
        let mut need = g
            .units
            .iter()
            .filter(|u| u.kind > 0)
            .map(|u| ATTACK_ENERGY[u.kind] / RATE[u.kind])
            .sum::<f32>()
            * 1.45
            + 5.;
        if g.rate() < need {
            for i in 0..8 {
                if g.gpus[i].model == 0 && g.credits >= 130. {
                    buy(g, i, 1);
                    break;
                }
                if g.gpus[i].model > 0 && g.gpus[i].tier < 3 {
                    let price =
                        (GPU_COST[g.gpus[i].model] * (0.5 + g.gpus[i].tier as f32 * 0.2)).round();
                    if g.credits >= price {
                        g.command((4_100_000 + i) as f32);
                        break;
                    }
                }
            }
        }
        let count = g.units.iter().filter(|u| u.kind > 0).count();
        let kind = [1, 1, 3, 2, 4, 1, 2, 3, 4, 1, 2, 3][count % 12];
        if count < 18 && g.credits >= COST[kind] + if g.rate() < need { 130. } else { 0. } {
            if let Some(c) = candidate(g, kind) {
                deploy(g, kind, c);
            }
        }
        need = g
            .units
            .iter()
            .filter(|u| u.kind > 0)
            .map(|u| ATTACK_ENERGY[u.kind] / RATE[u.kind])
            .sum::<f32>();
        if g.rate() > need * 1.2 {
            for i in 0..UNITS {
                let u = g.units[i];
                let price = (COST[u.kind] * (0.65 + u.level as f32 * 0.2)).round();
                if u.kind > 0 && u.level < 3 && g.credits > price + 100. {
                    g.command((2_000_000 + i) as f32);
                    break;
                }
            }
        }
    }
    #[test]
    fn normal_economy_can_complete_all_three_open_map_levels() {
        let mut g = Game::new(1, 1);
        let mut completed = 0;
        for tick in 0..180_000 {
            if tick % 90 == 0 {
                invest(&mut g);
                if g.phase == 0
                    && g.units.iter().filter(|u| u.kind > 0).count() >= 2
                    && g.energy > g.cap() * 0.7
                {
                    g.command(5_000_000.);
                }
                if g.phase == 1 {
                    for i in 0..UNITS {
                        let u = g.units[i];
                        if u.kind == 0 || u.skill > 0. || g.energy < SKILL_ENERGY[u.kind] + 30. {
                            continue;
                        }
                        if let Some(e) = g
                            .enemies
                            .iter()
                            .filter(|e| e.active && (e.kind >= 5 || g.dist[e.cell] < 8.))
                            .min_by(|a, b| g.dist[a.cell].total_cmp(&g.dist[b.cell]))
                            .copied()
                        {
                            g.skill(i, e.cell);
                            break;
                        }
                    }
                }
            }
            g.tick(1. / 60.);
            if g.phase == 3 {
                panic!(
                    "level{} wave{} failed hp{} kills{} energy{} credits{} units{} rate{}",
                    g.level,
                    g.wave,
                    g.hp,
                    g.kills,
                    g.energy,
                    g.credits,
                    g.units.iter().filter(|u| u.kind > 0).count(),
                    g.rate()
                );
            }
            if g.phase == 2 {
                completed += 1;
                assert_eq!(g.terrain_stage, 2);
                assert!(g.boss_killed);
                if g.level == 3 {
                    break;
                }
                g.command(5_200_000.);
            }
        }
        assert_eq!(
            completed,
            3,
            "campaign did not complete: level{} wave{} phase{} units{} money{} energy{}",
            g.level,
            g.wave,
            g.phase,
            g.units.iter().filter(|u| u.kind > 0).count(),
            g.credits,
            g.energy
        );
        assert!(g.spent_attack > 0.);
        assert!(g.spent_skill > 0.);
        assert!(g.hp > 0.);
    }
    #[test]
    fn powered_but_undefended_base_still_loses() {
        let mut g = Game::new(1, 1);
        buy(&mut g, 0, 1);
        for _ in 0..30000 {
            if g.phase == 0 {
                g.command(5_000_000.);
            }
            g.tick(1. / 60.);
            if g.phase == 3 {
                break;
            }
        }
        assert_eq!(g.phase, 3);
        assert_eq!(g.hp, 0.);
    }
}
