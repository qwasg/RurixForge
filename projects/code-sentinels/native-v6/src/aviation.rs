use crate::{catalog, Game};
const DT: f64 = 1. / 60.;
impl Game {
    /// Runway readiness restores aircraft endurance (fuel) and energy.
    pub fn aviation_tick(&mut self) {
        for i in 0..self.state.units.len() {
            let Some(def) =
                catalog::unit_ref(&self.state.units[i].kind).filter(|d| d.category == "air")
            else {
                continue;
            };
            let old = self.state.units[i].clone();
            let field = self
                .state
                .buildings
                .iter()
                .filter(|b| {
                    b.owner == old.owner
                        && b.kind == "airstrip"
                        && b.hp > 0.
                        && b.progress >= 1.
                        && b.powered
                })
                .min_by(|a, b| {
                    a.rect
                        .center()
                        .distance(old.pos)
                        .total_cmp(&b.rect.center().distance(old.pos))
                })
                .cloned();
            let at_field = field.as_ref().is_some_and(|b| b.rect.contains(old.pos));
            let needs = old.fuel < old.fuelMax * 0.2
                || old.energyMax > 0. && old.energy < old.energyMax * 0.2;
            let serviced = old.fuel >= old.fuelMax * 0.95
                && old.energy >= old.energyMax * 0.95;
            if old.fuel <= 0. && !at_field {
                let u = &mut self.state.units[i];
                u.flightState = "emergency".into();
                u.altitude = (u.altitude - 2. * DT).max(0.);
                u.route.clear();
                if u.altitude <= 0. {
                    u.hp = 0.;
                }
                continue;
            }
            let returning = matches!(old.flightState.as_str(), "returning" | "landing")
                || old.sortieTarget.is_some()
                    && field
                        .as_ref()
                        .is_some_and(|f| old.goal == Some(f.rect.center()));
            if (needs || returning) && !at_field {
                if let Some(field) = &field {
                    let target = field.rect.center();
                    let needs_route = old.goal != Some(target) || old.route.is_empty();
                    if needs_route
                        && (old.flightState != "returning" || (self.state.tick + old.id) % 60 == 0)
                    {
                        if let Some(route) = self.route(old.pos, target, "air") {
                            let u = &mut self.state.units[i];
                            if !returning {
                                u.sortieTarget = old.goal.or_else(|| old.route.last().copied());
                            }
                            u.route = route;
                            u.goal = Some(target);
                            u.target = None;
                            u.flightState = "returning".into();
                        }
                    }
                }
            }
            let current = self.state.units[i].clone();
            let departure_requested = at_field
                && serviced
                && !current.route.is_empty()
                && current.goal.is_some()
                && !returning;
            let land = at_field
                && !departure_requested
                && (needs
                    || matches!(
                        current.flightState.as_str(),
                        "returning" | "landing" | "landed"
                    )
                    || current.route.is_empty());
            if land {
                let u = &mut self.state.units[i];
                if !returning && u.flightState != "landed" && needs {
                    u.sortieTarget = u.goal.or_else(|| u.route.last().copied());
                }
                u.route.clear();
                u.goal = None;
                u.target = None;
                u.altitude = (u.altitude - DT).max(0.);
                u.flightState = if u.altitude <= 0. {
                    "landed"
                } else {
                    "landing"
                }
                .into();
                if u.flightState == "landed" {
                    let rate = self.readiness_restore_rate(old.owner, old.pos) * DT;
                    let u = &mut self.state.units[i];
                    u.fuel = (u.fuel + rate).min(u.fuelMax);
                    if u.energyMax > 0. {
                        u.energy = (u.energy + rate).min(u.energyMax);
                    }
                    if u.fuel >= u.fuelMax * 0.95 && u.energy >= u.energyMax * 0.95 {
                        let target = u.sortieTarget.take();
                        if let Some(target) = target.filter(|p| *p != old.pos) {
                            if let Some(route) = self.route(old.pos, target, "air") {
                                let u = &mut self.state.units[i];
                                u.route = route;
                                u.goal = Some(target);
                                u.flightState = "taking-off".into();
                            } else {
                                self.state.units[i].sortieTarget = Some(target);
                            }
                        }
                    }
                }
            } else if old.altitude <= 0. && !at_field {
                self.state.units[i].flightState = "landed".into();
            } else {
                let u = &mut self.state.units[i];
                let height = if def.tier >= 5 { 4. } else { 2. };
                u.altitude = (u.altitude + DT).min(height);
                if u.altitude < height {
                    u.flightState = "taking-off".into();
                } else {
                    u.flightState = if returning || current.flightState == "returning" {
                        "returning"
                    } else {
                        "cruising"
                    }
                    .into();
                }
            }
            let u = &mut self.state.units[i];
            if u.altitude > 0. {
                u.fuel = (u.fuel - DT * 0.12).max(0.);
            }
        }
    }
}
