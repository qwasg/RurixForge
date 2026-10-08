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
}
