use crate::{catalog, types::*, Game};
impl Game {
    pub fn cleanup_dead(&mut self) {
        let _timing=crate::profiling::observe("world/cleanup");
        // Most impacts are nonlethal. Preserve immediate death order while avoiding
        // allocation, retained collections and navigation work when nothing changed.
        let needs_cleanup=self.state.buildings.iter().any(|b|b.hp<=0.)
            ||self.state.units.iter().any(|u|u.hp<=0.)
            ||self.state.rooms.iter().any(|r|r.hp<=0.)
            ||self.state.walls.iter().any(|w|w.hp<=0.)
            ||self.state.links.iter().any(|l|l.hp<=0.||l.unitEndpoints.iter().any(|id|!self.state.units.iter().any(|u|u.id==*id&&u.hp>0.)))
            ||self.state.entrances.iter().any(|e|e.hp<=0.&&(e.kind!="door"||e.hp!=0.||!e.open||e.powered));
        if !needs_cleanup{return;}
        let previous_entrance_count = self.state.entrances.len();
        let dead: Vec<Building> = self
            .state
            .buildings
            .iter()
            .filter(|b| b.hp <= 0.)
            .cloned()
            .collect();
        let mut changed = !dead.is_empty();
        for b in dead {
            if b.kind == "core" {
                self.state.winner = Some(3 - b.owner);
                self.state.winReason = "指挥核心被摧毁".into();
            }
            let children: Vec<u64> = self
                .state
                .rooms
                .iter()
                .filter(|r| r.shell == b.id)
                .map(|r| r.id)
                .collect();
            let gpu_value = self
                .state
                .rooms
                .iter()
                .filter(|r| r.shell == b.id)
                .flat_map(|r| &r.gpus)
                .filter_map(|id| catalog::gpus().into_iter().find(|g| &g.id == id))
                .map(|g| g.cost)
                .sum::<f64>();
            self.player_mut(b.owner).unwrap().lostValue += b.invested + gpu_value;
            self.event("destroy", b.rect.center(), b.owner, b.invested, b.id);
            self.state.events.last_mut().unwrap().subjectKind = b.kind.clone();
            self.state.events.last_mut().unwrap().rect = Some(b.rect);
            if b.kind != "core" {
                let id = self.id();
                self.state.rubble.push(Rubble {
                    id,
                    owner: b.owner,
                    rect: b.rect,
                    salvage: b.invested * 0.2 + gpu_value * 0.1,
                });
            }
            self.state
                .jobs
                .retain(|j| j.target != b.id && !children.contains(&j.target));
            self.state.rooms.retain(|r| r.shell != b.id);
            self.state
                .links
                .retain(|l| !l.path.iter().any(|p| b.rect.contains(*p)));
            self.state.entrances.retain(|e| !b.rect.contains(e.pos));
            self.state.buildings.retain(|s| s.id != b.id);
        }
        let units: Vec<Unit> = self
            .state
            .units
            .iter()
            .filter(|u| u.hp <= 0.)
            .cloned()
            .collect();
        for u in units {
            self.player_mut(u.owner).unwrap().lostValue += u.invested;
            self.event(
                &format!("unit-death:{}", u.kind),
                u.pos,
                u.owner,
                u.invested,
                u.id,
            );
            let (dx, dy) = [
                (1, 1),
                (0, 1),
                (-1, 1),
                (-1, 0),
                (-1, -1),
                (0, -1),
                (1, -1),
                (1, 0),
            ][u.facing as usize % 8];
            self.state.events.last_mut().unwrap().direction =
                Some(Pos::new(u.pos.x + dx, u.pos.y + dy, u.level));
            self.state.events.last_mut().unwrap().presentationPosition =
                Some([u.x, u.y, u.elevation() + 0.1]);
            self.state.events.last_mut().unwrap().facing = Some(u.facing);
            self.state.jobs.retain(|j| j.target != u.id);
            self.state.units.retain(|s| s.id != u.id);
        }
        let room_ids: Vec<u64> = self
            .state
            .rooms
            .iter()
            .filter(|r| r.hp <= 0.)
            .map(|r| r.id)
            .collect();
        for id in &room_ids {
            self.state.jobs.retain(|j| j.target != *id);
        }
        changed |= !room_ids.is_empty()
            || self.state.walls.iter().any(|w| w.hp <= 0.)
            || self
                .state
                .entrances
                .iter()
                .any(|e| e.hp <= 0. && (e.kind != "door" || e.hp != 0. || !e.open))
            || self.state.links.iter().any(|l| l.hp <= 0.);
        self.state.rooms.retain(|r| r.hp > 0.);
        self.state.walls.retain(|w| w.hp > 0.);
        self.state.links.retain(|l| l.hp > 0.);
        let living_units:std::collections::BTreeSet<_>=self.state.units.iter().filter(|u|u.hp>0.).map(|u|u.id).collect();
        for link in &mut self.state.links{link.unitEndpoints.retain(|id|living_units.contains(id));}
        let dead_shafts: Vec<_> = self
            .state
            .entrances
            .iter()
            .filter(|e| e.hp <= 0. && matches!(e.kind.as_str(), "stairs" | "elevator" | "ramp"))
            .cloned()
            .collect();
        self.state.links.retain(|l| {
            !l.path.windows(2).any(|edge| {
                edge[0].level != edge[1].level
                    && dead_shafts
                        .iter()
                        .any(|e| e.covers(edge[0]) && e.covers(edge[1]))
            })
        });
        for door in self
            .state
            .entrances
            .iter_mut()
            .filter(|e| e.kind == "door" && e.hp <= 0.)
        {
            door.hp = 0.;
            door.open = true;
            door.powered = false;
        }
        self.state
            .entrances
            .retain(|e| e.hp > 0. || e.kind == "door");
        if self.state.entrances.len() != previous_entrance_count {
            self.refresh_room_capacities();
        }
        if changed {
            self.invalidate_navigation();
            self.networks();
        }
    }
    pub fn update_supports(&mut self) {
        let mut collapse = Vec::new();
        for z in 1..=5 {
            let candidates: Vec<Building> = self
                .state
                .buildings
                .iter()
                .filter(|b| b.kind == "shell" && b.rect.level == z && b.progress >= 1.)
                .cloned()
                .collect();
            for b in candidates {
                let supported = b
                    .rect
                    .cells()
                    .iter()
                    .map(|p| {
                        self.state
                            .buildings
                            .iter()
                            .filter(|s| {
                                s.kind == "shell"
                                    && s.hp > 0.
                                    && s.rect.contains(Pos::new(p.x, p.y, z - 1))
                            })
                            .map(|s| {
                                (s.hp / s.maxHp).clamp(0., 1.)
                                    * if s.rect.level > 0 { s.supportRatio } else { 1. }
                            })
                            .fold(0., f64::max)
                    })
                    .sum::<f64>()
                    / b.rect.area() as f64;
                let columns = self
                    .state
                    .rooms
                    .iter()
                    .filter(|r| {
                        r.kind == "column"
                            && r.hp > 0.
                            && r.rect.level == z - 1
                            && b.rect
                                .contains(Pos::new(r.rect.center().x, r.rect.center().y, z))
                    })
                    .map(|r| r.equipmentShare * (r.hp / r.maxHp).clamp(0., 1.) * r.progress)
                    .sum::<f64>();
                let ratio = (supported + columns * 0.07).min(1.);
                let building = self
                    .state
                    .buildings
                    .iter_mut()
                    .find(|s| s.id == b.id)
                    .unwrap();
                building.supportRatio = ratio;
                if ratio < 0.65 {
                    if building.collapseWarning <= 0. {
                        building.collapseWarning = 3.;
                        self.event("support-warning", b.rect.center(), b.owner, 3., b.id);
                    } else {
                        building.collapseWarning -= 1.;
                        if building.collapseWarning <= 0. {
                            collapse.push(b.id);
                        }
                    }
                } else {
                    building.collapseWarning = 0.;
                }
            }
        }
        let falling: Vec<Building> = self
            .state
            .buildings
            .iter()
            .filter(|b| collapse.contains(&b.id))
            .cloned()
            .collect();
        for b in &falling {
            self.event(
                "collapse",
                b.rect.center(),
                b.owner,
                b.rect.area() as f64,
                b.id,
            );
            self.state.events.last_mut().unwrap().rect = Some(b.rect);
            self.state.events.last_mut().unwrap().subjectKind = b.kind.clone();
            if let Some(live) = self.state.buildings.iter_mut().find(|s| s.id == b.id) {
                live.hp = 0.;
            }
        }
        // Choose landing floors only after every failed support has left the
        // collision set, so a six-floor collapse cannot strand a unit on F5.
        if !falling.is_empty() {
            self.invalidate_navigation();
        }
        let falls: Vec<(u64, Option<Pos>, i32)> = self
            .state
            .units
            .iter()
            .filter(|u| u.altitude < 0.5 && falling.iter().any(|b| b.rect.contains(u.pos)))
            .map(|u| {
                let landing = (-2..u.level)
                    .rev()
                    .map(|z| Pos::new(u.pos.x, u.pos.y, z))
                    .find(|p| {
                        self.walkable(
                            *p,
                            &catalog::unit_ref(&u.kind)
                                .map(|d| d.category.as_str())
                                .unwrap_or("ai"),
                        )
                    });
                (u.id, landing, u.level)
            })
            .collect();
        for (id, landing, old_level) in falls {
            if let Some(u) = self.state.units.iter_mut().find(|u| u.id == id) {
                if let Some(p) = landing {
                    u.hp -= u.maxHp * (0.62 + (old_level - p.level) as f64 * 0.18).min(1.);
                    u.pos = p;
                    u.level = p.level;
                } else {
                    u.hp = 0.;
                }
                u.route.clear();
                u.goal = None;
                u.queuedGoals.clear();
                u.transitProgress = 0.;
                u.dash = None;
            }
        }
        self.cleanup_dead();
    }
}
