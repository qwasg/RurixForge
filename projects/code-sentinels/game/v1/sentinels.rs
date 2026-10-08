//! Code Sentinels: deterministic native tower defence simulation.
//! The engine invokes the scalar C exports through .rxgraph call.call_function.
//! Each play session owns a fresh DLL; no browser simulation or network is used.
use std::sync::Mutex;

const LANES: [f32; 3] = [2.6, 0.0, -2.6];
const COLS: [f32; 4] = [-6.3, -3.5, -0.7, 2.1];
const ENEMIES: usize = 36;
const SHOTS: usize = 40;
const COST: [f32; 5] = [0., 80., 115., 145., 165.];
const DAMAGE: [f32; 5] = [0., 28., 20., 13., 17.];
const RATE: [f32; 5] = [1., 0.8, 1.3, 1.35, 1.15];
const RANGE: [f32; 5] = [0., 4.3, 4.1, 4.9, 4.5];
const TOTAL_WAVES: u32 = 8;

#[derive(Clone, Copy, Default, Debug)]
struct Tower { kind: usize, level: u32, cool: f32, flash: f32, invested: f32, generated: f32 }
#[derive(Clone, Copy, Default, Debug)]
struct Enemy { active: bool, lane: usize, kind: usize, x: f32, hp: f32, max_hp: f32, speed: f32,
    slow: f32, marked: f32, hit: f32, reward: f32, breach: f32, age: f32 }
#[derive(Clone, Copy, Default, Debug)]
struct Shot { life: f32, x: f32, y: f32, tx: f32, ty: f32, kind: usize }

#[derive(Debug)]
struct Game {
    energy: f32, hp: f32, wave: u32, phase: u32, kills: u32, skill: f32,
    time: f32, wave_time: f32, spawn_clock: f32, spawned: u32, total: u32,
    paused: bool, speed: f32, feedback: u32, feedback_time: f32, combo_hits: u32,
    towers: [Tower; 12], enemies: [Enemy; ENEMIES], shots: [Shot; SHOTS],
    skill_lane: i32, skill_flash: f32, earned: f32,
}
impl Default for Game {
    fn default() -> Self { Self {
        energy: 420., hp: 20., wave: 0, phase: 0, kills: 0, skill: 0.,
        time: 0., wave_time: 0., spawn_clock: 0., spawned: 0, total: 0,
        paused: false, speed: 1., feedback: 0, feedback_time: 0., combo_hits: 0,
        towers: [Tower::default(); 12], enemies: [Enemy::default(); ENEMIES],
        shots: [Shot::default(); SHOTS], skill_lane: -1, skill_flash: 0., earned: 0.,
    } }
}

fn lane_y(lane: usize, x: f32) -> f32 {
    // A shared compiled-data bus subtly bends as it passes the central router.
    LANES[lane] - 0.52 + (1.0 - (x / 2.7).abs()).max(0.0) * if lane == 1 { -0.28 } else { 0.28 }
}
fn tower_xy(slot: usize) -> (f32, f32) { (COLS[slot % 4], LANES[slot / 4] + 0.1) }
impl Game {
    fn feedback(&mut self, code: u32) { self.feedback = code; self.feedback_time = 2.5; }
    fn lane_mask(&self, lane: usize) -> u32 {
        self.towers[lane * 4..lane * 4 + 4].iter().fold(0, |mask, t| mask | if t.kind == 0 { 0 } else { 1 << (t.kind - 1) })
    }
    fn lane_combo(&self, lane: usize) -> u32 {
        let m = self.lane_mask(lane);
        if m == 15 { 3 } else if (m & 3 == 3) && (m & 12 != 0) { 2 } else if m & 3 == 3 || m & 12 == 12 { 1 } else { 0 }
    }
    fn command(&mut self, raw: f32) -> f32 {
        if !raw.is_finite() || raw.fract() != 0.0 { self.feedback(8); return 0.; }
        let command = raw as i32;
        if command == 0 { return 0.; }
        if command == 8000 { *self = Game::default(); return 1.; }
        if command == 6000 { self.paused = !self.paused; return 1.; }
        if command == 7000 { self.speed = if self.speed == 1. { 2. } else if self.speed == 2. { 3. } else { 1. }; return 1.; }
        if self.phase >= 2 { self.feedback(9); return 0.; }
        match command {
            1000..=1114 => {
                let slot = ((command - 1000) / 10) as usize;
                let kind = ((command - 1000) % 10) as usize;
                if slot >= 12 || kind == 0 || kind > 4 { self.feedback(8); return 0.; }
                if self.towers[slot].kind != 0 { self.feedback(2); return 0.; }
                if self.energy < COST[kind] { self.feedback(1); return 0.; }
                self.energy -= COST[kind];
                self.towers[slot] = Tower { kind, level: 1, cool: 0.15, invested: COST[kind], ..Tower::default() };
                self.feedback(10); 1.
            }
            2000..=2011 => {
                let slot = (command - 2000) as usize;
                let t = self.towers[slot];
                if t.kind == 0 { self.feedback(3); return 0.; }
                if t.level >= 3 { self.feedback(4); return 0.; }
                let price = (COST[t.kind] * (0.6 + t.level as f32 * 0.25)).round();
                if self.energy < price { self.feedback(1); return 0.; }
                self.energy -= price; self.towers[slot].level += 1;
                self.towers[slot].invested += price; self.feedback(11); 1.
            }
            3000..=3011 => {
                let slot = (command - 3000) as usize;
                if self.towers[slot].kind == 0 { self.feedback(3); return 0.; }
                self.energy += (self.towers[slot].invested * 0.7).round();
                self.towers[slot] = Tower::default(); self.feedback(12); 1.
            }
            4000..=4002 => {
                if self.skill > 0. { self.feedback(5); return 0.; }
                let lane = (command - 4000) as usize;
                if !self.enemies.iter().any(|e| e.active && e.lane == lane) { self.feedback(6); return 0.; }
                self.skill = 25.; self.skill_lane = lane as i32; self.skill_flash = 0.9;
                let damage = 65. + self.wave as f32 * 10. + self.lane_combo(lane) as f32 * 25.;
                for i in 0..ENEMIES { if self.enemies[i].active && self.enemies[i].lane == lane {
                    self.enemies[i].slow = 4.; self.enemies[i].marked = 5.; self.hurt(i, damage, true);
                } }
                self.feedback(13); 1.
            }
            5000 => {
                if self.phase != 0 { self.feedback(7); return 0.; }
                self.wave += 1; self.phase = 1; self.spawned = 0;
                self.total = 7 + self.wave * 3; self.spawn_clock = 0.3; self.wave_time = 0.;
                self.feedback(14); 1.
            }
            _ => { self.feedback(8); 0. }
        }
    }
    fn hurt(&mut self, index: usize, damage: f32, true_damage: bool) {
        let e = &mut self.enemies[index];
        if !e.active { return; }
        let armor = if !true_damage && e.kind == 2 && e.marked <= 0. { 0.52 } else { 1. };
        let marked = if e.marked > 0. { 1.2 } else { 1. };
        e.hp -= damage * armor * marked; e.hit = 0.12;
        if e.hp <= 0. { e.active = false; self.kills += 1; self.energy += e.reward; self.earned += e.reward; }
    }
    fn spawn(&mut self) {
        let Some(index) = self.enemies.iter().position(|e| !e.active) else { return; };
        let n = self.spawned;
        let lane = ((n + self.wave) % 3) as usize;
        let kind = if self.wave == TOTAL_WAVES && n + 1 == self.total { 3 }
            else if self.wave >= 3 && n % 5 == 3 { 2 }
            else if self.wave >= 2 && n % 4 == 2 { 1 } else { 0 };
        let base = 48. + self.wave as f32 * 16.;
        let hp = base * [1., 0.66, 2.15, 13.5][kind];
        self.enemies[index] = Enemy { active: true, lane, kind, x: 10.0, hp, max_hp: hp,
            speed: [0.67, 1.15, 0.49, 0.34][kind] + self.wave as f32 * 0.026,
            reward: [13., 12., 23., 200.][kind], breach: [1., 1., 2., 20.][kind], ..Enemy::default() };
        self.spawned += 1;
    }
    fn emit_shot(&mut self, slot: usize, target: usize, kind: usize) {
        let i = self.shots.iter().position(|s| s.life <= 0.).unwrap_or(slot);
        let (x, y) = tower_xy(slot);
        self.shots[i] = Shot { life: 0.22, x: x + 0.22, y: y + 0.48,
            tx: self.enemies[target].x, ty: lane_y(self.enemies[target].lane, self.enemies[target].x) + 0.32, kind };
    }
    fn tick(&mut self, dt: f32) {
        if !dt.is_finite() || dt <= 0. { return; }
        // 1/60 steps keep combat outcome invariant under render cadence/speed.
        let mut remaining = dt.min(0.25) * self.speed;
        while remaining > 0.000001 { let step = remaining.min(1. / 60.); self.step(step); remaining -= step; }
    }
    fn step(&mut self, dt: f32) {
        if self.paused || self.phase >= 2 { return; }
        self.time += dt;
        self.skill = (self.skill - dt).max(0.); self.skill_flash = (self.skill_flash - dt).max(0.);
        self.feedback_time -= dt; if self.feedback_time <= 0. { self.feedback = 0; }
        for s in &mut self.shots { s.life = (s.life - dt).max(0.); }
        for t in &mut self.towers { t.flash = (t.flash - dt).max(0.); }
        if self.phase != 1 { return; }
        self.wave_time += dt; self.energy = (self.energy + dt * 3.5).min(9999.);
        self.spawn_clock -= dt;
        if self.spawned < self.total && self.spawn_clock <= 0. {
            self.spawn(); self.spawn_clock = (1.35 - self.wave as f32 * 0.07).max(0.64);
        }
        for i in 0..ENEMIES {
            let e = &mut self.enemies[i];
            if !e.active { continue; }
            e.age += dt; e.hit = (e.hit - dt).max(0.); e.marked = (e.marked - dt).max(0.);
            e.slow = (e.slow - dt).max(0.);
            e.x -= e.speed * if e.slow > 0. { 0.48 } else { 1. } * dt;
            if e.x < -9.25 { self.hp = (self.hp - e.breach).max(0.); e.active = false; }
        }
        for slot in 0..12 {
            let t = self.towers[slot]; if t.kind == 0 { continue; }
            let lane = slot / 4; let combo = self.lane_combo(lane);
            self.towers[slot].cool = (t.cool - dt).max(0.);
            if t.kind == 4 {
                self.towers[slot].generated += dt;
                if self.towers[slot].generated >= 4.0 {
                    let amount = 5. + 3. * t.level as f32;
                    self.energy += amount; self.earned += amount; self.towers[slot].generated -= 4.;
                }
            }
            if self.towers[slot].cool > 0. { continue; }
            let (x, _) = tower_xy(slot);
            let range = RANGE[t.kind] + (t.level - 1) as f32 * 0.38;
            let target = self.enemies.iter().enumerate()
                .filter(|(_, e)| e.active && e.lane == lane && (e.x - x).abs() <= range)
                .min_by(|(_, a), (_, b)| a.x.total_cmp(&b.x)).map(|(i, _)| i);
            let Some(target) = target else { continue; };
            let support = self.towers[lane * 4..lane * 4 + 4].iter().any(|o| o.kind == 4);
            let amp = 1. + combo as f32 * 0.16 + if support && t.kind != 4 { 0.18 } else { 0. };
            let damage = DAMAGE[t.kind] * (1. + (t.level - 1) as f32 * 0.68) * amp;
            self.towers[slot].cool = RATE[t.kind] / (1. + (t.level - 1) as f32 * 0.12 + combo as f32 * 0.06);
            self.towers[slot].flash = 0.22; self.emit_shot(slot, target, t.kind);
            if t.kind == 3 { self.enemies[target].slow = 2.8; self.enemies[target].marked = 4.; }
            if combo > 0 { self.combo_hits += 1; }
            let target_x = self.enemies[target].x;
            self.hurt(target, damage, t.kind == 2 && combo >= 1);
            if t.kind == 2 || (t.kind == 3 && combo >= 2) {
                for other in 0..ENEMIES { if other != target && self.enemies[other].active
                    && self.enemies[other].lane == lane && (self.enemies[other].x - target_x).abs() < 1.25 {
                    if t.kind == 3 { self.enemies[other].slow = 2.; self.enemies[other].marked = 3.; }
                    self.hurt(other, damage * 0.7, combo >= 1);
                } }
            }
        }
        if self.hp <= 0. { self.phase = 3; self.feedback(16); return; }
        if self.spawned >= self.total && !self.enemies.iter().any(|e| e.active) {
            self.energy += 65. + self.wave as f32 * 12.;
            if self.wave == TOTAL_WAVES { self.phase = 2; self.feedback(15); }
            else { self.phase = 0; self.feedback(17); }
        }
    }
    fn read(&self, key: i32) -> f32 {
        match key {
            0 => self.energy, 1 => self.hp, 2 => self.wave as f32, 3 => self.phase as f32,
            4 => self.kills as f32, 5 => (0..3).map(|l| self.lane_combo(l)).max().unwrap_or(0) as f32,
            6 => self.skill, 7 => self.spawned as f32, 8 => self.total as f32,
            9 => self.enemies.iter().filter(|e| e.active).count() as f32,
            10 => self.speed, 11 => if self.paused { 1. } else { 0. },
            12 => self.feedback as f32, 13 => self.time, 14 => self.combo_hits as f32,
            15 => self.earned, 16..=18 => self.lane_combo((key - 16) as usize) as f32,
            19 => TOTAL_WAVES as f32,
            100..=219 => {
                let slot = ((key - 100) / 10) as usize; let field = (key - 100) % 10;
                let t = self.towers[slot]; let (x,y) = tower_xy(slot);
                match field { 0 => t.kind as f32, 1 => t.level as f32, 2 => t.cool,
                    3 => x, 4 => y, 5 => t.flash, 6 => t.invested, 7 => self.lane_combo(slot / 4) as f32,
                    8 => if t.kind == 0 || t.level >= 3 { 0. } else { (COST[t.kind] * (0.6 + t.level as f32 * 0.25)).round() },
                    9 => RANGE[t.kind] + t.level.saturating_sub(1) as f32 * 0.38, _ => 0. }
            }
            1000..=1359 => {
                let i = ((key - 1000) / 10) as usize; let field = (key - 1000) % 10; let e = self.enemies[i];
                match field { 0 => if e.active { e.x } else { -100. },
                    1 => if e.active { lane_y(e.lane, e.x) } else { -100. },
                    2 => e.kind as f32, 3 => e.hp / e.max_hp.max(1.), 4 => e.hit,
                    5 => e.slow, 6 => e.marked, 7 => if e.active { 1. } else { 0. },
                    8 => e.lane as f32, _ => e.age }
            }
            2000..=2399 => {
                let i = ((key - 2000) / 10) as usize; let field = (key - 2000) % 10; let s = self.shots[i];
                let t = (1. - s.life / 0.22).clamp(0., 1.);
                match field { 0 => if s.life > 0. { s.x + (s.tx-s.x)*t } else { -100. },
                    1 => if s.life > 0. { s.y + (s.ty-s.y)*t } else { -100. },
                    2 => s.kind as f32, 3 => s.life, _ => 0. }
            }
            3000 => if self.skill_flash > 0. { 0. } else { -100. },
            3001 => if self.skill_flash > 0. { LANES[self.skill_lane.max(0) as usize] - 0.42 } else { -100. },
            3002 => self.skill_flash,
            _ => 0.,
        }
    }
}

static GAME: Mutex<Option<Game>> = Mutex::new(None);
fn with_game<T>(f: impl FnOnce(&mut Game) -> T) -> T {
    let mut guard = GAME.lock().unwrap_or_else(|p| p.into_inner());
    f(guard.get_or_insert_with(Game::default))
}
#[no_mangle]
pub extern "C" fn cs_reset() -> f32 { with_game(|g| { *g = Game::default(); 1. }) }
#[no_mangle]
pub extern "C" fn cs_tick(dt: f32) -> f32 { with_game(|g| { g.tick(dt); g.phase as f32 }) }
#[no_mangle]
pub extern "C" fn cs_input(command: f32) -> f32 { with_game(|g| g.command(command)) }
#[no_mangle]
pub extern "C" fn cs_get(key: f32) -> f32 { with_game(|g| g.read(key as i32)) }
/// Renderer lanes: 0..47 are four tower variants per slot; 100..135 enemies;
/// 200..239 pulses; 300 is the active lane compiler effect.
#[no_mangle]
pub extern "C" fn cs_visual_x(id: f32) -> f32 { with_game(|g| {
    let id = id as i32;
    if (0..48).contains(&id) { let slot = id as usize / 4; if g.towers[slot].kind == id as usize % 4 + 1 { tower_xy(slot).0 } else { -100. } }
    else if (100..136).contains(&id) { g.read(1000 + (id - 100) * 10) }
    else if (200..240).contains(&id) { g.read(2000 + (id - 200) * 10) }
    else { g.read(3000) }
}) }
#[no_mangle]
pub extern "C" fn cs_visual_y(id: f32) -> f32 { with_game(|g| {
    let id = id as i32;
    if (0..48).contains(&id) { let slot = id as usize / 4; if g.towers[slot].kind == id as usize % 4 + 1 { tower_xy(slot).1 } else { -100. } }
    else if (100..136).contains(&id) { g.read(1001 + (id - 100) * 10) }
    else if (200..240).contains(&id) { g.read(2001 + (id - 200) * 10) }
    else { g.read(3001) }
}) }
#[no_mangle]
pub extern "C" fn cs_visual_scale(id: f32) -> f32 { with_game(|g| {
    let id = id as i32;
    if (0..48).contains(&id) { let t = g.towers[id as usize / 4]; 1. + t.level.saturating_sub(1) as f32 * 0.06 + if t.flash > 0. { 0.05 } else { 0. } }
    else if (100..136).contains(&id) { let e = g.enemies[(id - 100) as usize]; [0.7, 0.57, 0.88, 1.5][e.kind] * if e.hit > 0. { 1.08 } else { 1. } }
    else { 1. }
}) }
#[no_mangle]
pub extern "C" fn cs_visual_frame(id: f32) -> f32 { with_game(|g| {
    let id = id as i32;
    if (100..136).contains(&id) { let e=g.enemies[(id-100) as usize]; (e.kind*2 + ((e.age*5.) as usize%2)) as f32 }
    else if (200..240).contains(&id) { g.shots[(id-200) as usize].kind.saturating_sub(1) as f32 }
    else {0.}
}) }
#[no_mangle]
pub extern "C" fn cs_visual_attacking(id: f32) -> bool { with_game(|g| {
    let id=id as i32;
    (0..48).contains(&id) && g.towers[id as usize/4].kind==id as usize%4+1 && g.towers[id as usize/4].flash>0.1
}) }

#[cfg(test)]
mod tests {
    use super::*;
    fn seconds(g: &mut Game, duration: f32) { for _ in 0..(duration * 60.) as usize { g.tick(1. / 60.); } }
    #[test] fn purchase_is_atomic_and_upgrade_sell_conserve_economy() {
        let mut g = Game::default(); assert_eq!(g.command(1001.), 1.);
        let balance = g.energy; assert_eq!(g.command(1002.), 0.); assert_eq!(g.energy, balance);
        assert_eq!(g.command(2000.), 1.); assert_eq!(g.towers[0].level, 2);
        assert_eq!(g.command(3000.), 1.); assert_eq!(g.towers[0].kind, 0);
        assert!(g.energy < 420.); assert_eq!(g.command(1119.), 0.);
    }
    #[test] fn mask_requires_distinct_tools_and_complete_pipeline() {
        let mut g = Game::default(); g.energy = 1000.;
        for (slot, kind) in [(0,1),(1,2),(2,3),(3,4)] { g.command((1000 + slot*10 + kind) as f32); }
        assert_eq!(g.lane_combo(0), 3); assert_eq!(g.lane_combo(1), 0);
        g.command(3001.); assert_eq!(g.lane_combo(0), 1);
    }
    #[test] fn pause_freezes_simulation_and_restart_resets_everything() {
        let mut g = Game::default(); g.command(5000.); seconds(&mut g, 3.);
        g.command(6000.); let before = (g.time, g.energy, g.enemies[0].x);
        seconds(&mut g, 5.); assert_eq!((g.time,g.energy,g.enemies[0].x), before);
        g.command(8000.); assert_eq!(g.wave,0); assert_eq!(g.energy,420.); assert!(!g.paused);
    }
    #[test] fn skill_damages_only_selected_lane_and_obeys_cooldown() {
        let mut g = Game::default(); g.command(5000.); seconds(&mut g, 5.);
        let hp0: f32 = g.enemies.iter().filter(|e|e.active&&e.lane==0).map(|e|e.hp).sum();
        let hp1: f32 = g.enemies.iter().filter(|e|e.active&&e.lane==1).map(|e|e.hp).sum();
        assert_eq!(g.command(4000.),1.); assert_eq!(g.command(4001.),0.);
        assert!(g.enemies.iter().filter(|e|e.active&&e.lane==0).map(|e|e.hp).sum::<f32>() < hp0);
        assert_eq!(g.enemies.iter().filter(|e|e.active&&e.lane==1).map(|e|e.hp).sum::<f32>(),hp1);
    }
    #[test] fn undefended_base_really_loses() {
        let mut g = Game::default();
        for _ in 0..100000 { if g.phase == 0 {g.command(5000.);} g.tick(1./60.); if g.phase == 3 {break;} }
        assert_eq!(g.phase,3); assert_eq!(g.hp,0.);
    }
    #[test] fn normal_economy_strategy_can_win_all_eight_waves() {
        let mut g = Game::default();
        // Spend only earned energy: start one debugger and one tracer per lane,
        // fill pipelines as rewards arrive, then prioritize upgrades.
        for slot in [2,6,10] { assert_eq!(g.command((1000+slot*10+1) as f32),1.); }
        for _ in 0..100000 {
            for lane in 0..3 { for (col,kind) in [(1,3),(3,2),(0,4)] {
                let s=lane*4+col; if g.towers[s].kind==0 && g.energy>=COST[kind] {g.command((1000+s*10+kind) as f32);}
            } }
            for s in 0..12 { if g.towers[s].kind>0 && g.towers[s].level<3 && g.energy>=g.read(108+s as i32*10) { g.command((2000+s) as f32); } }
            if g.phase==0 {g.command(5000.);}
            if g.skill<=0. { if let Some(l)=g.enemies.iter().filter(|e|e.active&&e.x<0.).min_by(|a,b|a.x.total_cmp(&b.x)).map(|e|e.lane) {g.command((4000+l) as f32);} }
            g.tick(1./60.); if g.phase>=2 {break;}
        }
        assert_eq!(g.phase,2,"normal economy strategy failed: {:?}",g);
        assert_eq!(g.wave,8); assert!(g.combo_hits>0); assert!(g.hp>0.);
    }
    #[test] fn malformed_commands_do_not_mutate_economy() {
        let mut g=Game::default(); for command in [f32::NAN,f32::INFINITY,1001.5,-5.,1119.,999999.] {assert_eq!(g.command(command),0.);}
        assert_eq!(g.energy,420.); assert!(g.towers.iter().all(|t|t.kind==0));
    }
}
