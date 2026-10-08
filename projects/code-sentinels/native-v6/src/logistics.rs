//! Flattened economy: extractors credit owners directly; repair-bay aura; plant upkeep.
use crate::{catalog, Game, Pos};
const DT: f64 = 1. / 60.;

impl Game {
    pub fn logistics_second(&mut self) {
        self.mine_extractors();
        self.plant_credit_upkeep();
        self.repair_bay_aura();
    }

    fn mine_extractors(&mut self) {
        let extractors: Vec<_> = self
            .state
            .buildings
            .iter()
            .filter(|b| {
                b.kind == "extractor" && b.hp > 0. && b.progress >= 1. && b.powered
            })
            .map(|b| (b.id, b.owner, b.rect))
            .collect();
        for (id, owner, rect) in extractors {
            let center = rect.center();
            let Some(resource) = self.state.resources.iter_mut().find(|r| {
                r.remaining > 0.
                    && matches!(r.kind.as_str(), "ore" | "coal")
                    && rect.contains(r.pos)
            }) else {
                continue;
            };
            let rate = if resource.kind == "coal" { 4.0 } else { 6.0 };
            let amount = (rate as f64).min(resource.remaining);
            if amount <= 0. {
                continue;
            }
            resource.remaining -= amount;
            if let Some(player) = self.player_mut(owner) {
                player.credits += amount;
                *player.totals.entry("ore-mined".into()).or_default() += amount;
            }
            self.event("ore-mined", center, owner, amount, id);
        }
    }

    fn plant_credit_upkeep(&mut self) {
        for owner in 1..=2u32 {
            let plants: Vec<_> = self
                .state
                .buildings
                .iter()
                .filter(|b| {
                    b.owner == owner
                        && b.hp > 0.
                        && b.progress >= 1.
                        && matches!(b.kind.as_str(), "coal-power" | "nuclear-power")
                })
                .map(|b| (b.id, b.kind.clone(), b.rect.center()))
                .collect();
            for (id, kind, pos) in plants {
                let upkeep = if kind == "nuclear-power" {
                    catalog::NUCLEAR_UPKEEP
                } else {
                    catalog::COAL_UPKEEP
                };
                let credits = self.player(owner).map(|p| p.credits).unwrap_or(0.);
                let powered = credits + 0.001 >= upkeep;
                if powered {
                    if let Some(player) = self.player_mut(owner) {
                        player.credits = (player.credits - upkeep).max(0.);
                    }
                }
                if let Some(building) = self.state.buildings.iter_mut().find(|b| b.id == id) {
                    building.powered = powered;
                }
                let _ = pos;
            }
        }
    }

    fn repair_bay_aura(&mut self) {
        let bays: Vec<_> = self
            .state
            .rooms
            .iter()
            .filter(|r| {
                r.kind == "repair-bay"
                    && r.hp > 0.
                    && r.progress >= 1.
                    && r.powered
                    && r.connected
                    && r.online
            })
            .map(|r| (r.owner, r.rect.center(), r.equipmentShare))
            .collect();
        if bays.is_empty() {
            return;
        }
        let rapid_centers: Vec<_> = self
            .state
            .rooms
            .iter()
            .filter(|r| {
                r.kind == "rapid-logistics"
                    && r.hp > 0.
                    && r.progress >= 1.
                    && r.powered
                    && r.connected
                    && r.online
            })
            .map(|r| (r.owner, r.rect.center()))
            .collect();
        for i in 0..self.state.units.len() {
            let unit = &self.state.units[i];
            if unit.hp <= 0. || unit.hp >= unit.maxHp {
                continue;
            }
            let owner = unit.owner;
            let pos = unit.pos;
            let mut rate = 0.0_f64;
            for (bay_owner, center, share) in &bays {
                if *bay_owner != owner {
                    continue;
                }
                if center.distance(pos) <= catalog::REPAIR_AURA_RADIUS {
                    rate = rate.max(catalog::REPAIR_BAY_HEAL_PER_SEC * share);
                }
            }
            if rate <= 0. {
                continue;
            }
            let boosted = rapid_centers.iter().any(|(o, c)| {
                *o == owner && c.distance(pos) <= catalog::REPAIR_AURA_RADIUS
            });
            if boosted {
                rate *= 1.0 + catalog::RAPID_READINESS_BONUS;
            }
            let heal = rate * DT;
            let u = &mut self.state.units[i];
            u.hp = (u.hp + heal).min(u.maxHp);
        }
    }

    /// Readiness restore rate for a landed aircraft at `pos` owned by `owner`.
    pub fn readiness_restore_rate(&self, owner: u32, pos: Pos) -> f64 {
        let mut rate = catalog::READINESS_RESTORE_PER_SEC;
        if self.state.rooms.iter().any(|r| {
            r.owner == owner
                && r.kind == "rapid-logistics"
                && r.hp > 0.
                && r.progress >= 1.
                && r.powered
                && r.connected
                && r.online
                && r.rect.center().distance(pos) <= catalog::REPAIR_AURA_RADIUS
        }) {
            rate *= 1.0 + catalog::RAPID_READINESS_BONUS;
        }
        rate
    }
}
