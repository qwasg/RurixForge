use crate::{ballistics::modifier, catalog, types::*, Game};
use std::collections::BTreeSet;
const DT: f64 = 1. / 60.;
impl Game {
    pub fn target_info(&self, id: u64) -> Option<(Pos, u32)> {
        self.state
            .units
            .iter()
            .find(|x| x.id == id && x.hp > 0.)
            .map(|x| (x.pos, x.owner))
            .or_else(|| {
                self.state
                    .buildings
                    .iter()
                    .find(|x| x.id == id && x.hp > 0.)
                    .map(|x| (x.rect.center(), x.owner))
            })
            .or_else(|| {
                self.state
                    .rooms
                    .iter()
                    .find(|x| x.id == id && x.hp > 0.)
                    .map(|x| (x.rect.center(), x.owner))
            })
            .or_else(|| {
                self.state
                    .walls
                    .iter()
                    .find(|x| x.id == id && x.hp > 0.)
                    .map(|x| (x.pos, x.owner))
            })
            .or_else(|| {
                self.state
                    .shipments
                    .iter()
                    .find(|x| x.id == id && x.hp > 0.)
                    .map(|x| (x.pos, x.owner))
            })
            .or_else(|| {
                self.state
                    .links
                    .iter()
                    .find(|x| x.id == id && x.hp > 0.)
                    .and_then(|x| x.path.first().map(|p| (*p, x.owner)))
            })
            .or_else(|| {
                self.state
                    .entrances
                    .iter()
                    .find(|e| e.id == id && e.hp > 0.)
                    .map(|e| (e.pos, e.owner))
            })
    }
    pub fn target_info_for(&self, id: u64, viewer: u32) -> Option<(Pos, u32)> {
        if let Some(entry) = self.state.entrances.iter().find(|e| e.id == id) {
            return if entry.hp <= 0. {
                None
            } else if entry.owner == viewer {
                Some((entry.pos, viewer))
            } else {
                entry
                    .cells()
                    .into_iter()
                    .find(|p| self.visible_to(viewer, *p))
                    .map(|p| (p, entry.owner))
            };
        }
        if let Some(building) = self
            .state
            .buildings
            .iter()
            .find(|b| b.id == id && b.hp > 0.)
        {
            return if building.owner == viewer {
                Some((building.rect.center(), viewer))
            } else {
                self.visible_building_edge(viewer, building.rect)
                    .map(|p| (p, building.owner))
            };
        }
        if let Some(link) = self.state.links.iter().find(|l| l.id == id && l.hp > 0.) {
            return link
                .path
                .iter()
                .find(|p| self.visible_to(viewer, **p))
                .map(|p| (*p, link.owner));
        }
        self.target_info(id)
    }
    pub(crate) fn targets(&self, owner: u32) -> Vec<(u64, Pos, u32)> {
        self.state
            .units
            .iter()
            .filter(|x| x.owner != owner && x.hp > 0.)
            .map(|x| (x.id, x.pos, x.owner))
            .chain(
                self.state
                    .buildings
                    .iter()
                    .filter(|x| x.owner != owner && x.hp > 0.)
                    .map(|x| (x.id, x.rect.center(), x.owner)),
            )
            .chain(
                self.state
                    .rooms
                    .iter()
                    .filter(|x| x.owner != owner && x.hp > 0.)
                    .map(|x| (x.id, x.rect.center(), x.owner)),
            )
            .chain(
                self.state
                    .walls
                    .iter()
                    .filter(|x| x.owner != owner && x.hp > 0.)
                    .map(|x| (x.id, x.pos, x.owner)),
            )
            .chain(
                self.state
                    .shipments
                    .iter()
                    .filter(|x| x.owner != owner && x.hp > 0.)
                    .map(|x| (x.id, x.pos, x.owner)),
            )
            .chain(
                self.state
                    .entrances
                    .iter()
                    .filter(|e| e.owner != owner && e.hp > 0. && !e.open)
                    .map(|e| (e.id, e.pos, e.owner)),
            )
            .chain(
                self.state
                    .links
                    .iter()
                    .filter(|l| l.owner != owner && l.hp > 0.)
                    .filter_map(|l| {
                        l.path
                            .iter()
                            .find(|p| self.visible_to(owner, **p))
                            .map(|p| (l.id, *p, l.owner))
                    }),
            )
            .collect()
    }
    pub fn damage(&mut self, id: u64, raw: f64, kind: &str, attacker: u32) {
        self.damage_at(id, raw, kind, attacker, None);
    }
    pub fn damage_at(&mut self, id: u64, raw: f64, kind: &str, attacker: u32, impact: Option<Pos>) {
        let Some((default_pos, owner)) = self.target_info(id) else {
            return;
        };
        let pos = impact.unwrap_or(default_pos);
        let mut amount = raw.max(0.);
        let personal_target=self.state.units.iter().any(|u|u.id==id)||self.state.shipments.iter().any(|s|s.id==id);
        let allied_armor=if raw<1e10&&personal_target{self.ally_armor_at(owner,pos,id).clamp(0.,0.15)}else{0.};
        if attacker != 0 && attacker == owner {
            return;
        }
        if raw < 1e10 {
            if let Some(region) = self.state.shieldRegions.iter_mut().find(|r| {
                r.owner == owner
                    && (r.cells.contains(&pos)
                        || [
                            Pos::new(pos.x + 1, pos.y, pos.level),
                            Pos::new(pos.x - 1, pos.y, pos.level),
                            Pos::new(pos.x, pos.y + 1, pos.level),
                            Pos::new(pos.x, pos.y - 1, pos.level),
                        ]
                        .iter()
                        .any(|p| r.cells.contains(p)))
            }) {
                let taken = amount.min(region.current);
                region.current -= taken;
                amount -= taken;
            }
            // A projectile always applies residual physical damage, even against mismatched defenses.
            let guards: Vec<(u64, String, f64, f64)> = self
                .state
                .rooms
                .iter()
                .filter(|r| {
                    r.owner == owner
                        && r.hp > 0.
                        && r.rect.level == pos.level
                        && r.rect.center().distance(pos) < 12. * r.equipmentShare.sqrt()
                        && ((r.kind == "network-defense" && r.connected)
                            || (r.kind == "energy-defense" && r.powered))
                        && self.skill_line_clear(r.rect.center(), pos, owner)
                })
                .map(|r| (r.id, r.kind.clone(), r.inventory, r.equipmentShare))
                .collect();
            for (gid, gkind, charge, share) in guards {
                let base: f64 = match (gkind.as_str(), kind) {
                    ("network-defense", "network") => 0.85,
                    ("network-defense", _) => 0.25,
                    ("energy-defense", "energy") => 0.8,
                    ("energy-defense", _) => 0.45,
                    _ => 0.,
                };
                let efficiency = 1. - (1. - base).powf(share);
                let absorb = (amount * efficiency).min(charge);
                if let Some(r) = self.state.rooms.iter_mut().find(|r| r.id == gid) {
                    r.inventory -= absorb;
                }
                amount -= absorb;
            }
            if let Some(w) = self.state.walls.iter_mut().find(|w| w.id == id) {
                let scale = match (w.kind.as_str(), kind) {
                    ("physical", "kinetic") => 0.65,
                    ("physical", "explosive") => 1.3,
                    ("physical", "network") => 0.3,
                    ("cuda", "network") => 0.5,
                    ("cuda", "kinetic") => 0.85,
                    _ => 1.,
                };
                amount *= scale;
            }
        }
        if let Some(u) = self.state.units.iter_mut().find(|u| u.id == id) {
            amount*=1.-allied_armor;
            if raw<1e10&&kind=="network" {amount*=(1.-0.5*modifier(u,"jam-resistance")).clamp(0.,1.);}
            if u.statuses.get("fortify").copied().unwrap_or(0.) > 0. {
                amount *= 0.6;
            }
            u.hp -= amount;
        }
        if let Some(b) = self.state.buildings.iter_mut().find(|b| b.id == id) {
            b.hp -= amount;
        }
        if let Some(r) = self.state.rooms.iter_mut().find(|r| r.id == id) {
            r.hp -= amount;
        }
        if let Some(w) = self.state.walls.iter_mut().find(|w| w.id == id) {
            w.hp -= amount;
        }
        if let Some(s) = self.state.shipments.iter_mut().find(|s| s.id == id) {
            amount*=1.-allied_armor;
            s.hp -= amount;
        }
        if let Some(l) = self.state.links.iter_mut().find(|l| l.id == id) {
            l.hp -= amount;
        }
        if let Some(e) = self.state.entrances.iter_mut().find(|e| e.id == id) {
            e.hp -= amount;
        }
        if self.state.walls.iter().any(|w| w.id == id && w.hp <= 0.)
            || self
                .state
                .entrances
                .iter()
                .any(|e| e.id == id && e.hp <= 0.)
        {
            self.state.shieldRegions.retain(|r| {
                r.owner != owner
                    || ![
                        Pos::new(pos.x + 1, pos.y, pos.level),
                        Pos::new(pos.x - 1, pos.y, pos.level),
                        Pos::new(pos.x, pos.y + 1, pos.level),
                        Pos::new(pos.x, pos.y - 1, pos.level),
                    ]
                    .iter()
                    .any(|p| r.cells.contains(p))
            });
        }
        self.event(
            if kind == "energy" {
                "energy-impact"
            } else if kind == "explosive" {
                "explosive-impact"
            } else {
                "impact"
            },
            pos,
            attacker,
            amount,
            id,
        );
        self.cleanup_dead();
    }
    pub(crate) fn pay_attack(&mut self, index: usize, cost: f64) -> bool {
        self.pay_unit_compute(index, cost)
    }
    pub fn step(&mut self) {
        if self.state.winner.is_some() {
            return;
        }
        if catalog::sandbox_opening() {
            for player in &mut self.state.players {
                catalog::apply_sandbox_stock(player);
            }
        }
        self.state.tick += 1;
        self.state.revision += 1;
        crate::profiling::begin(self.state.tick);
        // Public velocity describes completed physical movement, never facing
        // or a future route. IDs survive removal/reordering during damage.
        let mut motion_before:Vec<_>=self.state.units.iter().map(|u|(u.id,u.x,u.y,u.level)).collect();
        motion_before.sort_unstable_by_key(|row|row.0);
        crate::profiling::mark("motion-observation-capture");
        self.construction_tick();
        crate::profiling::mark("construction");
        for b in &mut self.state.buildings {
            b.jam = (b.jam - DT).max(0.);
            b.antiHeal = (b.antiHeal - DT).max(0.);
        }
        for r in &mut self.state.rooms {
            r.cooldown = (r.cooldown - DT).max(0.);
            r.antiHeal = (r.antiHeal - DT).max(0.);
        }
        for w in &mut self.state.walls {
            w.antiHeal = (w.antiHeal - DT).max(0.);
        }
        crate::profiling::mark("building-statuses");
        if self.state.tick % 15 == 0 {
            self.networks();
        }
        crate::profiling::mark("network-rebuild");
        self.produce_compute(DT);
        self.maintain_rooms(DT);
        crate::profiling::mark("compute-and-room-maintenance");
        self.aviation_tick();
        self.recharge_energy(DT);
        crate::profiling::mark("aviation-and-energy");
        for owner in 1..=2 {
            let income = 0.35
                + self
                    .state
                    .resources
                    .iter()
                    .filter(|r| r.owner == owner && r.kind == "node")
                    .count() as f64
                    * 1.5;
            let p = self.player_mut(owner).unwrap();
            p.income = income;
            p.credits += income * DT;
        }
        for owner in 1..=2 {
            let tasks = self.player(owner).unwrap().researches.clone();
            let mut next = Vec::new();
            for mut task in tasks {
                if self.classic() {
                    // Research runs at the command core and only costs credits,
                    // so it advances as long as the core survives.
                    if let Some(core) = self
                        .state
                        .buildings
                        .iter()
                        .find(|b| b.owner == owner && b.kind == "core" && b.hp > 0.)
                    {
                        task.lab = core.id;
                        task.progress = (task.progress + DT / task.duration).min(1.);
                    }
                } else {
                    let lab = self
                        .state
                        .rooms
                        .iter()
                        .find(|r| {
                            r.id == task.lab && r.powered && r.connected && r.hp > 0. && r.online
                        })
                        .or_else(|| {
                            self.state.rooms.iter().find(|r| {
                                r.owner == owner
                                    && r.kind == "research-lab"
                                    && r.branch.as_deref() == Some(task.branch.as_str())
                                    && r.online
                            })
                        })
                        .cloned();
                    if let Some(lab) = lab {
                        task.lab = lab.id;
                        if self.consume_compute(
                            owner,
                            lab.rect.center(),
                            catalog::LAB_COMPUTE_UPKEEP[(task.target - 1) as usize] * DT,
                        ) {
                            task.progress = (task.progress + DT / task.duration).min(1.);
                        }
                    }
                }
                if task.progress >= 1. {
                    self.player_mut(owner)
                        .unwrap()
                        .branches
                        .insert(task.branch.clone(), task.target);
                    let pos = self
                        .state
                        .rooms
                        .iter()
                        .find(|r| r.id == task.lab)
                        .map(|r| r.rect.center())
                        .or_else(|| {
                            self.state
                                .buildings
                                .iter()
                                .find(|b| b.id == task.lab)
                                .map(|b| b.rect.center())
                        })
                        .unwrap_or_default();
                    self.event("research", pos, owner, task.target as f64, task.lab);
                } else {
                    next.push(task);
                }
            }
            let p = self.player_mut(owner).unwrap();
            p.research = next.first().cloned();
            p.researches = next;
        }
        crate::profiling::mark("income-and-research");
        let unit_count = self.state.units.len();
        for i in 0..unit_count {
            let owner = self.state.units[i].owner;
            let def = catalog::unit_ref(&self.state.units[i].kind).unwrap();
            let ready_for_goal = def.category != "air"
                || self.state.units[i].flightState == "cruising"
                || self.state.units[i].flightState == "landed"
                    && self.state.units[i].fuel >= self.state.units[i].fuelMax * 0.95
                    && self.state.units[i].energy >= self.state.units[i].energyMax * 0.95;
            if ready_for_goal
                && self.state.units[i].goal.is_none()
                && self.state.units[i].route.is_empty()
                && self.state.units[i].sortieTarget.is_none()
                && !self.state.units[i].queuedGoals.is_empty()
            {
                let next = self.state.units[i].queuedGoals.remove(0);
                self.state.units[i].goal = Some(next);
                if let Some(route) = self.route(self.state.units[i].pos, next, &def.category) {
                    self.state.units[i].route = route;
                }
                if def.category == "air" && self.state.units[i].flightState == "landed" {
                    self.state.units[i].flightState = "taking-off".into();
                }
            }
            let current = self.state.units[i].pos;
            let mut movement_blocked = false;
            if let Some(next) = self.state.units[i].route.first().copied() {
                let step = next;
                if !self.can_step(current, step, &def.category) {
                    movement_blocked = true;
                    if next.level == current.level || self.state.units[i].transitProgress <= 0. {
                        self.state.units[i].route.clear();
                        self.state.units[i].transitProgress = 0.;
                    }
                }
            }
            if self.state.units[i].route.is_empty()
                && !self.state.units[i].wired
                && (self.state.tick + self.state.units[i].id) % 60 == 0
            {
                if let Some(goal) = self.state.units[i].goal {
                    if goal != current {
                        if let Some(route) = self.route(current, goal, &def.category) {
                            self.state.units[i].route = route;
                        }
                    } else {
                        self.state.units[i].goal = None;
                    }
                }
            }
            let pos = self.state.units[i].pos;
            let charge = if self.state.units[i].covered {
                (12. * DT).min(self.state.units[i].batteryMax - self.state.units[i].battery)
            } else {
                0.
            };
            if charge > 0. && self.consume_unit_compute(owner, pos, charge) {
                self.state.units[i].battery += charge;
            }
            let u = &mut self.state.units[i];
            u.cooldown = (u.cooldown - DT).max(0.);
            u.skillCooldown = (u.skillCooldown - DT).max(0.);
            for t in u.statuses.values_mut() {
                *t -= DT;
            }
            u.statuses.retain(|_, t| *t > 0.);
            u.moving = false;
            if !u.route.is_empty()
                && !movement_blocked
                && !u.wired
                && (def.category != "air" || u.fuel > 0.)
                && (def.category != "air"
                    || !matches!(
                        u.flightState.as_str(),
                        "taking-off" | "landing" | "landed" | "emergency"
                    ))
            {
                let next = u.route[0];
                let dx = next.x as f64 + 0.5 - u.x;
                let dy = next.y as f64 + 0.5 - u.y;
                if dx.abs() + dy.abs() > 0.001 {
                    u.facing = (((dy.atan2(dx) - std::f64::consts::FRAC_PI_4)
                        / std::f64::consts::FRAC_PI_4)
                        .round() as i32)
                        .rem_euclid(8) as u32;
                }
                let dist = ((next.x as f64 + 0.5 - u.x).powi(2)
                    + (next.y as f64 + 0.5 - u.y).powi(2))
                .sqrt();
                let speed = def.speed
                    * if u.statuses.contains_key("slow") {
                        0.5
                    } else {
                        1.
                    }
                    * crate::traits::movement_multiplier(u).clamp(0.1,3.);
                let move_step = speed
                    * DT
                    * if u.dash.is_some() { 3. } else { 1. }
                    * if u.statuses.contains_key("haste") {
                        1.2
                    } else {
                        1.
                    };
                let crossing = next.level != u.level;
                if crossing {
                    u.transitProgress = (u.transitProgress + move_step / 4.).min(1.);
                    if u.transitProgress < 1. {
                        u.moving = true;
                        if def.category == "vehicle" {
                            u.fuel = (u.fuel - DT * 0.12).max(0.);
                        }
                        continue;
                    }
                }
                if dist <= move_step || crossing {
                    u.x = next.x as f64 + 0.5;
                    u.y = next.y as f64 + 0.5;
                    u.pos = next;
                    u.level = next.level;
                    u.transitProgress = 0.;
                    u.route.remove(0);
                    if u.route.is_empty() && u.goal == Some(u.pos) {
                        u.goal = None;
                    }
                } else {
                    u.x += (next.x as f64 + 0.5 - u.x) / dist * move_step;
                    u.y += (next.y as f64 + 0.5 - u.y) / dist * move_step;
                    u.pos = Pos::new(u.x.floor() as i32, u.y.floor() as i32, u.level);
                }
                u.moving = true;
                if def.category == "air" {
                    u.fuel = (u.fuel - DT * 0.12).max(0.);
                }
            }
        }
        // Dynamic obstructions invalidate movement rather than letting units tunnel through a new wall.
        let blocked: Vec<usize> = self
            .state
            .units
            .iter()
            .enumerate()
            .filter_map(|(i, u)| {
                u.route
                    .first()
                    .filter(|p| !self.walkable(**p, &catalog::unit_ref(&u.kind).unwrap().category))
                    .map(|_| i)
            })
            .collect();
        for i in blocked {
            self.state.units[i].route.clear();
            self.state.units[i].transitProgress = 0.;
        }
        crate::profiling::mark("navigation-and-movement");
        if self.state.tick % 30 == 0 {
            self.refresh_fog();
        }
        crate::profiling::mark("fog");
        self.advance_skills();
        crate::profiling::mark("skills");
        self.passives_tick();
        crate::profiling::mark("traits");
        self.advance_combat();
        crate::profiling::mark("combat");
        
        crate::profiling::mark("logistics-movement");
        if self.state.tick % 60 == 0 {
            self.economy_second();
            crate::profiling::mark("logistics-and-economy");
            self.cleanup_dead();
            
            self.capture_nodes();
            crate::profiling::mark("cleanup-supports-and-capture");
        }
        for unit in &mut self.state.units {
            let previous=motion_before.binary_search_by_key(&unit.id,|row|row.0).ok().map(|i|motion_before[i]);
            (unit.velocityX,unit.velocityY)=match previous {
                Some((_,x,y,level)) if level==unit.level=>((unit.x-x)/DT,(unit.y-y)/DT),
                _=>(0.,0.),
            };
        }
        crate::profiling::mark("motion-observation-update");
        if self.initial_ai && self.state.tick % 180 == 0 {
            self.bot();
        }
        crate::profiling::mark("bot");
        self.settle();
        crate::profiling::mark("settlement");
        crate::profiling::end();
    }
    fn economy_second(&mut self) {
        self.logistics_second();
        for owner in 1..=2 {
            let ai = self
                .state
                .units
                .iter()
                .filter(|u| {
                    u.owner == owner && catalog::unit_ref(&u.kind).unwrap().category == "ai"
                })
                .count() as f64;
            let nodes = self
                .state
                .resources
                .iter()
                .filter(|r| r.owner == owner && r.kind == "node" && !r.contested)
                .count() as f64;
            let p = self.player_mut(owner).unwrap();
            p.credits = (p.credits - ai * ai * catalog::AI_UPKEEP_QUADRATIC).max(0.);
            p.science += nodes * 0.6;
            let rooms: Vec<Room> = self
                .state
                .rooms
                .iter()
                .filter(|r| r.owner == owner && r.progress >= 1. && r.hp > 0.)
                .cloned()
                .collect();
            for room in rooms {
                if room.kind == "network-defense" && room.connected {
                    let charge = (300. * room.equipmentShare - room.inventory)
                        .min(12. * room.equipmentShare)
                        .max(0.);
                    if self.consume_compute(owner, room.rect.center(), charge) {
                        self.state
                            .rooms
                            .iter_mut()
                            .find(|r| r.id == room.id)
                            .unwrap()
                            .inventory += charge;
                    }
                }
                if room.kind == "energy-defense" && room.powered {
                    let r = self
                        .state
                        .rooms
                        .iter_mut()
                        .find(|r| r.id == room.id)
                        .unwrap();
                    r.inventory =
                        (r.inventory + 15. * r.equipmentShare).min(400. * r.equipmentShare);
                }
                if room.kind == "data-synthesis"
                    && room.powered
                    && room.connected
                    && self.consume_compute(owner, room.rect.center(), 5. * room.equipmentShare)
                {
                    self.player_mut(owner).unwrap().science += 0.04 * room.equipmentShare;
                }
            }
        }
        self.topology_charge();
        self.summarize_compute();
    }
    fn capture_nodes(&mut self) {
        for r in self.state.resources.iter_mut().filter(|r| r.kind == "node") {
            let owners: BTreeSet<u32> = self
                .state
                .units
                .iter()
                .filter(|u| {
                    u.hp > 0.
                        && u.level == r.pos.level
                        && u.altitude < 0.5
                        && u.pos.distance(r.pos) < 9.
                        && catalog::unit_ref(&u.kind).unwrap().speed > 0.
                })
                .map(|u| u.owner)
                .collect();
            if owners.len() == 1 {
                let owner = *owners.iter().next().unwrap();
                if owner != r.owner {
                    if r.capturer != owner {
                        r.capture = 0.;
                        r.capturer = owner;
                    }
                    r.capture += 1.;
                    if r.capture >= 20. {
                        r.owner = owner;
                        r.capture = 0.;
                        r.capturer = 0;
                    }
                } else {
                    r.capture = 0.;
                    r.capturer = 0;
                }
            } else if owners.is_empty() {
                r.capture = (r.capture - 1.).max(0.);
                if r.capture <= 0. {
                    r.capturer = 0;
                }
            }
            r.contested = owners.len() > 1 || r.owner > 0 && owners.iter().any(|o| *o != r.owner);
        }
        if self.state.tick >= 18 * 60 * 60 {
            for owner in 1..=2 {
                let nodes = self
                    .state
                    .resources
                    .iter()
                    .filter(|r| r.kind == "node" && r.owner == owner)
                    .count();
                let contested = self
                    .state
                    .resources
                    .iter()
                    .any(|r| r.kind == "node" && r.owner == owner && r.contested);
                let p = self.player_mut(owner).unwrap();
                if nodes >= 2 {
                    if !contested {
                        p.dominance += 1.;
                    }
                } else {
                    p.dominance = (p.dominance - 2.).max(0.);
                }
            }
        }
    }
    fn settle(&mut self) {
        if self.state.winner.is_some() {
            return;
        }
        for p in &self.state.players {
            if p.dominance >= 360. {
                self.state.winner = Some(p.owner);
                self.state.winReason = "节点压制达成".into();
            }
        }
    }
}
