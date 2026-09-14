//! Read-only trait queries plus deterministic, paid support passives.
use crate::{ballistics::modifier, catalog, types::*, Game};

pub fn movement_multiplier(unit: &Unit) -> f64 {
    1. + modifier(unit, "move-speed").max(0.)
}
pub fn vision_radius(unit: &Unit) -> f64 {
    let base = if catalog::unit_ref(&unit.kind)
        .and_then(|d| d.passive.as_ref())
        .is_some_and(|p| p.id == "pre-read")
    {
        26.
    } else {
        18.
    };
    base * (1. + modifier(unit, "vision-range").max(0.))
}
pub fn initial_compute_capacity(def: &catalog::UnitDef) -> f64 {
    if def.category != "ai" {
        0.
    } else if def.passive.as_ref().is_some_and(|p| p.id == "light-cache") {
        300.
    } else {
        240.
    }
}
impl Game {
    fn facility_bonus_in_network(
        &self,
        owner: u32,
        pos: Pos,
        kind: &str,
        network: Option<usize>,
    ) -> f64 {
        let Some(network) = network
            .and_then(|i| self.state.networkStores.get(i))
            .filter(|n| n.owner == owner)
        else {
            return 0.;
        };
        let Some(def) = catalog::facility_ref(kind).filter(|d| d.bonus > 0.) else {
            return 0.;
        };
        self.state
            .rooms
            .iter()
            .filter(|r| {
                r.kind == kind
                    && r.owner == owner
                    && r.hp > 0.
                    && r.progress >= 1.
                    && r.powered
                    && r.online
                    && r.connected
                    && r.rect.level == pos.level
                    && r.rect.center().distance(pos)
                        <= def.bonus_radius * r.equipmentShare.max(0.).sqrt()
                    && network.cells.contains(&r.rect.center())
            })
            .map(|_| def.bonus)
            .fold(0., f64::max)
    }
    pub fn facility_bonus_for_unit(&self, unit: &Unit, kind: &str) -> f64 {
        if unit.hp <= 0. || unit.transitProgress > 0. {
            return 0.;
        }
        self.facility_bonus_in_network(
            unit.owner,
            unit.pos,
            kind,
            self.unit_network(unit.owner, unit.pos),
        )
    }
    pub fn facility_bonus_for_entity(&self, id: u64, kind: &str) -> f64 {
        if let Some(unit) = self.state.units.iter().find(|u| u.id == id) {
            return self.facility_bonus_for_unit(unit, kind);
        }
        let entity = self
            .state
            .rooms
            .iter()
            .find(|r| r.id == id && r.hp > 0.)
            .map(|r| (r.owner, r.rect))
            .or_else(|| {
                self.state
                    .buildings
                    .iter()
                    .find(|b| b.id == id && b.hp > 0.)
                    .map(|b| (b.owner, b.rect))
            });
        let Some((owner, rect)) = entity else {
            return 0.;
        };
        let network = self
            .state
            .networkStores
            .iter()
            .position(|s| s.owner == owner && s.cells.iter().any(|p| rect.contains(*p)));
        self.facility_bonus_in_network(owner, rect.center(), kind, network)
    }
    pub fn plugin_discount(&self, unit: &Unit) -> f64 {
        self.facility_bonus_for_unit(unit, "modular-workshop")
            .clamp(0., 0.1)
    }
    /// The returned value is an armor bonus, not a damage multiplier. Multiple
    /// escorts do not stack; a carrier cannot grant its own support aura to itself.
    pub fn ally_armor_at(&self, owner: u32, position: Pos, target_id: u64) -> f64 {
        self.state
            .units
            .iter()
            .filter(|u| {
                u.owner == owner
                    && u.hp > 0.
                    && u.id != target_id
                    && u.transitProgress <= 0.
                    && u.level == position.level
                    && u.pos.distance(position) <= 6.
            })
            .filter_map(|u| {
                let bonus = modifier(u, "ally-armor");
                (bonus > 0.).then_some((u, bonus))
            })
            .filter(|(u, _)| self.skill_line_clear(u.pos, position, owner))
            .map(|(_, bonus)| bonus)
            .fold(0., f64::max)
            .clamp(0., 0.15)
    }
    /// Material conversion only. AI healing must continue to use repair_multiplier.
    pub fn material_repair_factor(&self, id: u64) -> f64 {
        1. + self
            .state
            .units
            .iter()
            .find(|u| u.id == id && u.hp > 0.)
            .map(|u| modifier(u, "repair-efficiency"))
            .unwrap_or(0.)
            .max(0.)
    }
    pub fn loading_rate_for_entity(&self, id: u64) -> f64 {
        1. + self
            .state
            .units
            .iter()
            .find(|u| u.id == id && u.hp > 0.)
            .map(|u| modifier(u, "reload-rate"))
            .unwrap_or(0.)
            .max(0.)
            + self.facility_bonus_for_entity(id, "rapid-logistics")
    }
    pub fn passives_tick(&mut self) {
        let mut actors: Vec<_> = self
            .state
            .units
            .iter()
            .filter(|u| u.hp > 0. && u.transitProgress <= 0. && !u.statuses.contains_key("silence"))
            .filter(|u| {
                catalog::unit_ref(&u.kind)
                    .and_then(|d| d.passive.as_ref())
                    .is_some_and(|p| {
                        matches!(p.id.as_str(), "guardian-intercept" | "maintenance-daemon")
                            && u.statuses
                                .get(&format!("passive-{}-cooldown", p.id))
                                .copied()
                                .unwrap_or(0.)
                                <= 0.
                    })
            })
            .map(|u| u.id)
            .collect();
        actors.sort_unstable();
        for id in actors {
            let Some(index) = self
                .state
                .units
                .iter()
                .position(|u| u.id == id && u.hp > 0.)
            else {
                continue;
            };
            let source = self.state.units[index].clone();
            let Some(passive) = catalog::unit_ref(&source.kind).and_then(|d| d.passive.as_ref())
            else {
                continue;
            };
            let cooldown = format!("passive-{}-cooldown", passive.id);
            if source.statuses.get(&cooldown).copied().unwrap_or(0.) > 0. {
                continue;
            }
            match passive.id.as_str() {
                "guardian-intercept" => {
                    let origin = [source.x, source.y, source.elevation() + 0.5];
                    let candidate = self
                        .state
                        .projectiles
                        .iter()
                        .enumerate()
                        .filter(|(_, p)| {
                            p.owner != source.owner && p.kind == "guided" && p.damage > 0.
                        })
                        .filter(|(_, p)| {
                            p.target.is_some_and(|target| {
                                self.state.units.iter().any(|u| {
                                    u.id == target
                                        && u.owner == source.owner
                                        && u.hp > 0.
                                        && u.level == source.level
                                }) || self.state.shipments.iter().any(|s| {
                                    s.id == target
                                        && s.owner == source.owner
                                        && s.hp > 0.
                                        && s.pos.level == source.level
                                })
                            })
                        })
                        .filter_map(|(i, p)| {
                            let distance = ((p.x - origin[0]).powi(2)
                                + (p.y - origin[1]).powi(2)
                                + ((p.z - origin[2]) * 4.).powi(2))
                            .sqrt();
                            (distance <= 6. && self.point_line_clear(origin, [p.x, p.y, p.z]))
                                .then_some((i, distance, p.id))
                        })
                        .min_by(|a, b| a.1.total_cmp(&b.1).then(a.2.cmp(&b.2)))
                        .map(|(i, _, _)| i);
                    if let Some(projectile) = candidate {
                        if !self.pay_attack(index, 8.) {
                            continue;
                        }
                        let reduction = self.state.projectiles[projectile].damage * 0.2;
                        self.state.projectiles[projectile].damage -= reduction;
                        self.state.units[index].statuses.insert(cooldown, 8.);
                        self.event("compute-spent", source.pos, source.owner, 8., id);
                        self.event(
                            "passive-guardian-intercept",
                            source.pos,
                            source.owner,
                            reduction,
                            id,
                        );
                    }
                }
                "maintenance-daemon" => {
                    if self.unit_network(source.owner, source.pos).is_none() {
                        continue;
                    }
                    let target = self
                        .state
                        .units
                        .iter()
                        .filter(|u| {
                            u.owner == source.owner
                                && u.hp > 0.
                                && u.hp < u.maxHp
                                && u.level == source.level
                                && u.transitProgress <= 0.
                                && u.altitude <= 0.25
                                && u.pos.distance(source.pos) <= 6.
                                && self.skill_line_clear(source.pos, u.pos, source.owner)
                        })
                        .min_by(|a, b| {
                            (a.hp / a.maxHp)
                                .total_cmp(&(b.hp / b.maxHp))
                                .then(a.id.cmp(&b.id))
                        })
                        .map(|u| u.id);
                    if let Some(target) = target {
                        if !self.consume_unit_compute(source.owner, source.pos, 6.) {
                            continue;
                        }
                        let factor = self.repair_multiplier(target);
                        let unit = self
                            .state
                            .units
                            .iter_mut()
                            .find(|u| u.id == target)
                            .unwrap();
                        let amount = (12. * factor).min(unit.maxHp - unit.hp);
                        unit.hp += amount;
                        self.state.units[index].statuses.insert(cooldown, 6.);
                        self.event("compute-spent", source.pos, source.owner, 6., id);
                        self.event(
                            "passive-maintenance-daemon",
                            source.pos,
                            source.owner,
                            amount,
                            id,
                        );
                    }
                }
                _ => {}
            }
        }
    }
}

#[cfg(test)]
#[path = "../tests/support/traits_contracts.rs"]
mod contracts;
