//! Skills validate the complete effect before paying; scans share projectile geometry.
use crate::{ballistics::modifier, catalog, types::*, Game};
const DT: f64 = 1. / 60.;
const EPS: f64 = 1e-7;
pub fn cone(origin: Pos, direction: Pos, target: Pos, range: f64, angle: f64) -> bool {
    if origin.level != target.level || origin.distance(target) > range {
        return false;
    }
    let a = (
        (direction.x - origin.x) as f64,
        (direction.y - origin.y) as f64,
    );
    let b = ((target.x - origin.x) as f64, (target.y - origin.y) as f64);
    let lengths = (a.0 * a.0 + a.1 * a.1).sqrt() * (b.0 * b.0 + b.1 * b.1).sqrt();
    lengths > 0. && (a.0 * b.0 + a.1 * b.1) / lengths >= (angle * 0.5).to_radians().cos()
}
fn segment_distance_xy(a: [f64; 2], b: [f64; 2], p: [f64; 2]) -> f64 {
    let v = [b[0] - a[0], b[1] - a[1]];
    let w = [p[0] - a[0], p[1] - a[1]];
    let length = v[0] * v[0] + v[1] * v[1];
    let t = if length > EPS {
        ((v[0] * w[0] + v[1] * w[1]) / length).clamp(0., 1.)
    } else {
        0.
    };
    ((w[0] - t * v[0]).powi(2) + (w[1] - t * v[1]).powi(2)).sqrt()
}
fn in_line(origin: Pos, aim: Pos, target: Pos, range: f64, width: f64) -> bool {
    if target.level != origin.level {
        return false;
    }
    let v = [(aim.x - origin.x) as f64, (aim.y - origin.y) as f64];
    let length = (v[0] * v[0] + v[1] * v[1]).sqrt();
    if length <= EPS {
        return false;
    }
    let w = [(target.x - origin.x) as f64, (target.y - origin.y) as f64];
    let forward = (v[0] * w[0] + v[1] * w[1]) / length;
    forward >= 0.
        && forward <= range.min(length)
        && (v[0] * w[1] - v[1] * w[0]).abs() / length <= width * 0.5
}
impl Game {
    pub(crate) fn active_skill_cost(&self, unit: &Unit) -> f64 {
        let Some(def) = catalog::unit_ref(&unit.kind) else {
            return f64::INFINITY;
        };
        let ready = def.passive.as_ref().is_some_and(|p| p.id == "triad-ready")
            && unit.statuses.get("gemini-ready").copied().unwrap_or(0.) > 0.;
        def.skill_cost
            * (1. + modifier(unit, "skill-power"))
            * if ready { 0.9 } else { 1. }
            * (1. - modifier(unit, "compute-efficiency")).clamp(0.15, 1.)
            * if unit.statuses.contains_key("compute-efficiency") {
                0.8
            } else {
                1.
            }
    }
    /// All healing paths share one timed debuff, including construction and cargo.
    pub fn repair_multiplier(&self, target: u64) -> f64 {
        let cut = self
            .state
            .units
            .iter()
            .find(|u| u.id == target)
            .is_some_and(|u| u.statuses.get("anti-heal").copied().unwrap_or(0.) > 0.)
            || self
                .state
                .buildings
                .iter()
                .find(|b| b.id == target)
                .is_some_and(|b| b.antiHeal > 0.)
            || self
                .state
                .rooms
                .iter()
                .find(|r| r.id == target)
                .is_some_and(|r| r.antiHeal > 0.)
            || self
                .state
                .walls
                .iter()
                .find(|w| w.id == target)
                .is_some_and(|w| w.antiHeal > 0.);
        if cut {
            0.35
        } else {
            1.
        }
    }
    fn set_heal_cut(&mut self, id: u64, duration: f64) {
        self.apply_hit_status(id, "anti-heal", duration);
    }
    pub(crate) fn apply_hit_status(&mut self, id: u64, kind: &str, duration: f64) {
        if !duration.is_finite()
            || duration <= 0.
            || !matches!(kind, "marked" | "anti-heal")
            || !self.skill_target_hp(id).is_some_and(|hp| hp > 0.)
        {
            return;
        }
        if let Some(u) = self.state.units.iter_mut().find(|u| u.id == id) {
            let entry = u.statuses.entry(kind.into()).or_default();
            *entry = entry.max(duration);
        }
        if kind != "anti-heal" {
            return;
        }
        if let Some(b) = self.state.buildings.iter_mut().find(|b| b.id == id) {
            b.antiHeal = b.antiHeal.max(duration);
        }
        if let Some(r) = self.state.rooms.iter_mut().find(|r| r.id == id) {
            r.antiHeal = r.antiHeal.max(duration);
        }
        if let Some(w) = self.state.walls.iter_mut().find(|w| w.id == id) {
            w.antiHeal = w.antiHeal.max(duration);
        }
    }
    pub(crate) fn skill_target_hp(&self, id: u64) -> Option<f64> {
        self.state
            .units
            .iter()
            .find(|u| u.id == id)
            .map(|u| u.hp)
            .or_else(|| {
                self.state
                    .buildings
                    .iter()
                    .find(|b| b.id == id)
                    .map(|b| b.hp)
            })
            .or_else(|| self.state.rooms.iter().find(|r| r.id == id).map(|r| r.hp))
            .or_else(|| self.state.walls.iter().find(|w| w.id == id).map(|w| w.hp))
    }
    fn heal_skill_target(&mut self, id: u64, unit_hp: f64, structure_hp: f64, fortify: bool) {
        let factor = self.repair_multiplier(id);
        if let Some(u) = self
            .state
            .units
            .iter_mut()
            .find(|u| u.id == id && u.hp > 0.)
        {
            u.hp = (u.hp + unit_hp * factor).min(u.maxHp);
            if fortify {
                u.statuses.insert("fortify".into(), 10.);
            }
        }
        if let Some(b) = self
            .state
            .buildings
            .iter_mut()
            .find(|b| b.id == id && b.hp > 0.)
        {
            b.hp = (b.hp + structure_hp * factor).min(b.maxHp);
        }
        if let Some(r) = self
            .state
            .rooms
            .iter_mut()
            .find(|r| r.id == id && r.hp > 0.)
        {
            r.hp = (r.hp + structure_hp * factor).min(r.maxHp);
        }
        if let Some(w) = self
            .state
            .walls
            .iter_mut()
            .find(|w| w.id == id && w.hp > 0.)
        {
            w.hp = (w.hp + structure_hp * factor).min(w.maxHp);
        }
    }
    fn grounded_skill_target(&self, id: u64) -> bool {
        self.state
            .units
            .iter()
            .find(|u| u.id == id)
            .is_none_or(|u| u.altitude <= 0.25)
    }
    pub(crate) fn reachable_skill_target(
        &self,
        owner: u32,
        origin: Pos,
        id: u64,
        target: Pos,
    ) -> bool {
        target.level == origin.level
            && self.grounded_skill_target(id)
            && self.visible_to(owner, target)
            && self.skill_line_clear(origin, target, owner)
    }
    pub(crate) fn friendly_skill_targets(&self, owner: u32, center: Pos, radius: f64) -> Vec<u64> {
        self.state
            .units
            .iter()
            .filter(|u| u.owner == owner && u.hp > 0.)
            .map(|u| (u.id, u.pos))
            .chain(
                self.state
                    .buildings
                    .iter()
                    .filter(|b| b.owner == owner && b.hp > 0.)
                    .map(|b| (b.id, b.rect.center())),
            )
            .chain(
                self.state
                    .rooms
                    .iter()
                    .filter(|r| r.owner == owner && r.hp > 0.)
                    .map(|r| (r.id, r.rect.center())),
            )
            .chain(
                self.state
                    .walls
                    .iter()
                    .filter(|w| w.owner == owner && w.hp > 0.)
                    .map(|w| (w.id, w.pos)),
            )
            .filter(|(id, p)| {
                p.distance(center) <= radius && self.reachable_skill_target(owner, center, *id, *p)
            })
            .map(|(id, _)| id)
            .collect()
    }
    pub(crate) fn skill_target_missing_hp(&self, id: u64) -> f64 {
        self.state
            .units
            .iter()
            .find(|u| u.id == id && u.hp > 0.)
            .map(|u| (u.maxHp - u.hp).max(0.))
            .or_else(|| {
                self.state
                    .buildings
                    .iter()
                    .find(|b| b.id == id && b.hp > 0.)
                    .map(|b| (b.maxHp - b.hp).max(0.))
            })
            .or_else(|| {
                self.state
                    .rooms
                    .iter()
                    .find(|r| r.id == id && r.hp > 0.)
                    .map(|r| (r.maxHp - r.hp).max(0.))
            })
            .or_else(|| {
                self.state
                    .walls
                    .iter()
                    .find(|w| w.id == id && w.hp > 0.)
                    .map(|w| (w.maxHp - w.hp).max(0.))
            })
            .unwrap_or(0.)
    }
    /// Damage only along continuous segments actually traversed by ordinary movement.
    pub fn advance_skills(&mut self) {
        let active: Vec<Unit> = self
            .state
            .units
            .iter()
            .filter(|u| u.dash.is_some())
            .cloned()
            .collect();
        for u in active {
            let mut dash = u.dash.clone().unwrap();
            let current = [u.x, u.y];
            if (current[0] - dash.previous[0]).abs() + (current[1] - dash.previous[1]).abs() > EPS {
                let mut hits = Vec::new();
                for (target, p, _) in self.targets(u.owner) {
                    let point = self
                        .state
                        .units
                        .iter()
                        .find(|t| t.id == target)
                        .map(|t| [t.x, t.y])
                        .unwrap_or([p.x as f64 + 0.5, p.y as f64 + 0.5]);
                    if !dash.hitTargets.contains(&target)
                        && p.level == u.level
                        && segment_distance_xy(dash.previous, current, point) <= dash.width * 0.5
                        && self.reachable_skill_target(u.owner, u.pos, target, p)
                    {
                        hits.push((target, p));
                    }
                }
                // Cleanup can reorder units; every subsequent lookup uses IDs.
                for (target, p) in hits {
                    dash.hitTargets.push(target);
                    self.damage_at(target, dash.damage, "kinetic", u.owner, Some(p));
                }
            }
            dash.previous = current;
            dash.remaining = (dash.remaining - DT).max(0.);
            if let Some(unit) = self.state.units.iter_mut().find(|t| t.id == u.id) {
                if unit.route.is_empty()
                    || dash.remaining <= 0.
                    || unit.wired
                    || unit.level != u.level
                {
                    unit.dash = None;
                    unit.route.clear();
                    unit.goal = None;
                } else {
                    unit.dash = Some(dash);
                }
            }
        }
    }
    fn field_recharge_plan(&self, u: &Unit) -> Vec<(u64, &'static str, f64)> {
        if catalog::unit_ref(&u.kind).is_some_and(|d| d.category == "air")
            && u.flightState != "landed"
        {
            return vec![];
        }
        let mut result = Vec::new();
        for (cargo, max, current, fraction) in [
            ("ammo", u.ammoMax, u.ammo, 0.25),
            ("fuel", u.fuelMax, u.fuel, 0.20),
        ] {
            let incoming: f64 = self
                .state
                .shipments
                .iter()
                .filter(|s| s.to == u.id && s.cargo == cargo && s.hp > 0.)
                .map(|s| s.amount)
                .sum();
            let mut need = (max - current - incoming).max(0.).min(max * fraction);
            let mut sources: Vec<_> = self
                .state
                .buildings
                .iter()
                .filter(|b| b.owner == u.owner && b.hp > 0. && b.progress >= 1.)
                .map(|b| {
                    (
                        b.id,
                        b.rect.center(),
                        b.stock.get(cargo).copied().unwrap_or(0.),
                        4.,
                    )
                })
                .chain(
                    self.state
                        .rooms
                        .iter()
                        .filter(|r| r.owner == u.owner && r.hp > 0. && r.progress >= 1.)
                        .map(|r| {
                            (
                                r.id,
                                r.rect.center(),
                                r.stock.get(cargo).copied().unwrap_or(0.),
                                4. * r.equipmentShare.max(0.).sqrt(),
                            )
                        }),
                )
                .filter(|(_, p, amount, radius)| {
                    *amount > 0.
                        && p.level == u.level
                        && p.distance(u.pos) <= *radius
                        && self.skill_line_clear(u.pos, *p, u.owner)
                })
                .collect();
            sources.sort_by_key(|(id, _, _, _)| *id);
            for (id, _, amount, _) in sources {
                let take = need.min(amount);
                if take > EPS {
                    result.push((id, cargo, take));
                    need -= take;
                }
                if need <= EPS {
                    break;
                }
            }
        }
        result
    }
    fn weapon_skill_target(
        &self,
        source: &Unit,
        click: Pos,
        range: f64,
    ) -> Option<(u64, Pos, [f64; 3])> {
        let mut candidates = Vec::new();
        for (id, _, _) in self.targets(source.owner) {
            let (point, priority, area) =
                if let Some(building) = self.state.buildings.iter().find(|b| b.id == id) {
                    // The representative visible edge need not be the clicked facade.
                    if self
                        .visible_building_edge(source.owner, building.rect)
                        .is_none()
                    {
                        continue;
                    }
                    (
                        building
                            .rect
                            .cells()
                            .into_iter()
                            .filter(|p| self.visible_to(source.owner, *p))
                            .min_by(|a, b| a.distance(click).total_cmp(&b.distance(click))),
                        2,
                        building.rect.area(),
                    )
                } else if let Some(room) = self.state.rooms.iter().find(|r| r.id == id) {
                    // Match owner-filtered room snapshots: a visible facade must not
                    // expose the equipment room hidden behind it.
                    if !self.visible_to(source.owner, room.rect.center()) {
                        continue;
                    }
                    (
                        room.rect
                            .cells()
                            .into_iter()
                            .filter(|p| self.visible_to(source.owner, *p))
                            .min_by(|a, b| a.distance(click).total_cmp(&b.distance(click))),
                        1,
                        room.rect.area(),
                    )
                } else {
                    (
                        self.target_info_for(id, source.owner)
                            .map(|(p, _)| p)
                            .filter(|p| self.visible_to(source.owner, *p)),
                        0,
                        1,
                    )
                };
            if let Some(point) =
                point.filter(|p| p.level == click.level && p.distance(click) <= 1.5)
            {
                candidates.push((id, point, point.distance(click), priority, area));
            }
        }
        candidates.sort_by(|a, b| {
            a.2.total_cmp(&b.2)
                .then(a.3.cmp(&b.3))
                .then(a.4.cmp(&b.4))
                .then(a.0.cmp(&b.0))
        });
        candidates.into_iter().find_map(|(id, point, _, _, _)| {
            self.skill_weapon_target_aim(source, id, point, range)
                .map(|aim| (id, point, aim))
        })
    }
    pub fn cast(
        &mut self,
        owner: u32,
        id: u64,
        pos: Pos,
        direction: Option<Pos>,
    ) -> Result<(), String> {
        let i = self
            .state
            .units
            .iter()
            .position(|u| u.id == id && u.owner == owner && u.hp > 0.)
            .ok_or("单位不存在")?;
        let u = self.state.units[i].clone();
        let d = catalog::unit_ref(&u.kind).ok_or("技能目录不存在")?;
        let weapon_target = matches!(d.skill.as_str(), "precision" | "target-lock");
        if !pos.valid()
            || u.transitProgress > 0.
            || u.skillCooldown > 0.
            || u.statuses.contains_key("silence")
            || pos.level != u.level
            || !weapon_target && u.pos.distance(pos) > d.skill_range
            || !self.visible_to(owner, pos)
        {
            return Err("正在换层，或冷却、沉默、楼层、施法距离及视野条件不满足".into());
        }
        let aim = direction.unwrap_or(pos);
        if !aim.valid() || aim.level != u.level {
            return Err("技能方向须在同层有效地图内".into());
        }
        if matches!(d.skill_shape.as_str(), "direction" | "cone") && aim == u.pos {
            return Err("定向技能须指明方向".into());
        }
        if d.category == "air" && !matches!(u.flightState.as_str(), "landed" | "cruising") {
            return Err("起降或应急状态无法施放技能".into());
        }
        if d.skill_shape != "self" && d.skill != "target-lock" && u.altitude > 0.25 {
            return Err("该定点技能需要地面施法位置".into());
        }
        if matches!(
            d.skill_shape.as_str(),
            "circle-ally" | "circle-mixed" | "target-ally"
        ) && !self.skill_line_clear(u.pos, pos, owner)
        {
            return Err("施法落点被墙体或楼板阻挡".into());
        }
        let ally = if d.skill == "targeted-support" {
            Some(
                self.state
                    .units
                    .iter()
                    .filter(|f| {
                        f.owner == owner
                            && f.hp > 0.
                            && f.pos.distance(pos) <= 1.5
                            && u.pos.distance(f.pos) <= d.skill_range
                            && self.reachable_skill_target(owner, u.pos, f.id, f.pos)
                    })
                    .min_by(|a, b| a.pos.distance(pos).total_cmp(&b.pos.distance(pos)))
                    .map(|f| f.id)
                    .ok_or("请选择施法距离内可接触的己方单位")?,
            )
        } else {
            None
        };
        let target = if weapon_target {
            Some(
                self.weapon_skill_target(&u, pos, d.skill_range)
                    .ok_or("请选择施法距离内可见且无遮挡的敌方目标")?,
            )
        } else {
            None
        };
        let dash = if d.skill == "dash-strike" {
            if u.wired || u.dash.is_some() || pos == u.pos {
                return Err("突进需要解除有线驻守，并指定新的终点".into());
            }
            let route = self
                .route(u.pos, pos, "ai")
                .ok_or("突进路线被墙或楼体阻挡")?;
            let mut previous = u.pos;
            let mut distance = 0.;
            for p in &route {
                if p.level != u.level {
                    return Err("突进不能跨层或借竖井绕行".into());
                }
                distance += previous.distance(*p);
                previous = *p;
            }
            if route.is_empty() || distance > d.skill_range + EPS {
                return Err("突进实际路线超出技能距离".into());
            }
            Some(route)
        } else {
            None
        };
        let resupply = if d.skill == "field-recharge" {
            let plan = self.field_recharge_plan(&u);
            if plan.is_empty() {
                return Err("附近没有可装载库存，或单位已满载/尚未降落".into());
            }
            plan
        } else {
            vec![]
        };
        if !matches!(
            d.skill.as_str(),
            "dash-strike"
                | "intercept-barrier"
                | "repair-armor"
                | "piercing-mark"
                | "telegraphed-bombardment"
                | "repair-heal-cut"
                | "targeted-support"
                | "precision"
                | "debug-cone"
                | "overclock"
                | "guard"
                | "target-lock"
                | "charged-shot"
                | "field-recharge"
        ) {
            return Err("未实现的技能类型".into());
        }
        let power_multiplier = 1. + modifier(&u, "skill-power");
        let gemini_ready = d.passive.as_ref().is_some_and(|p| p.id == "triad-ready")
            && u.statuses.get("gemini-ready").copied().unwrap_or(0.) > 0.;
        let cost = self.active_skill_cost(&u);
        if !self.pay_attack(i, cost) {
            return Err("本机缓存和已接入网络算力不足".into());
        }
        self.state.units[i].skillCooldown =
            d.skill_cooldown * (1. - modifier(&u, "skill-cooldown")).clamp(0.25, 1.);
        self.event("compute-spent", u.pos, owner, cost, id);
        let scale = 1.4_f64.powi(u.tier.saturating_sub(d.tier) as i32) * power_multiplier;
        let radius = d.skill_radius;
        match d.skill.as_str() {
            "dash-strike" => {
                let f = self.state.units.iter_mut().find(|f| f.id == id).unwrap();
                f.route = dash.unwrap();
                f.goal = Some(pos);
                f.dash = Some(DashState {
                    previous: [u.x, u.y],
                    hitTargets: vec![],
                    remaining: 2.,
                    damage: 125. * scale,
                    width: d.skill_width,
                });
            }
            "intercept-barrier" => {
                let field = self.id();
                self.state.defenseFields.push(DefenseField {
                    id: field,
                    owner,
                    pos: u.pos,
                    direction: aim,
                    radius,
                    angle: d.skill_angle,
                    hp: 360. * scale * (1. + modifier(&u, "interception")),
                    remaining: 10.,
                    kind: "interception-cone".into(),
                });
            }
            "repair-armor" => {
                for target in self.friendly_skill_targets(owner, pos, radius) {
                    self.heal_skill_target(target, 160. * scale, 120. * scale, true);
                }
            }
            "piercing-mark" | "debug-cone" => {
                let targets: Vec<_> = self
                    .targets(owner)
                    .into_iter()
                    .filter_map(|(id, _, _)| self.target_info_for(id, owner).map(|(p, _)| (id, p)))
                    .filter(|(id, p)| {
                        self.reachable_skill_target(owner, u.pos, *id, *p)
                            && if d.skill == "piercing-mark" {
                                in_line(u.pos, aim, *p, d.skill_range, d.skill_width)
                            } else {
                                cone(
                                    u.pos,
                                    aim,
                                    *p,
                                    if radius > 0. { radius } else { d.skill_range },
                                    d.skill_angle,
                                )
                            }
                    })
                    .collect();
                for (target, p) in targets {
                    let before_hp = self
                        .state
                        .units
                        .iter()
                        .find(|f| f.id == target)
                        .map(|f| f.hp);
                    self.damage_at(
                        target,
                        if d.skill == "piercing-mark" {
                            130. * scale
                        } else {
                            100. * scale
                        },
                        "network",
                        owner,
                        Some(p),
                    );
                    if d.skill == "piercing-mark"
                        && self
                            .skill_target_hp(target)
                            .zip(before_hp)
                            .is_some_and(|(after, before)| after > 0. && after < before)
                    {
                        self.apply_hit_status(target, "marked", 9.);
                    }
                }
            }
            "telegraphed-bombardment" => {
                let pid = self.id();
                let launch = [
                    pos.x as f64 + 0.5,
                    pos.y as f64 + 0.5,
                    pos.level as f64 + 4.,
                ];
                let destination = [launch[0], launch[1], pos.level as f64 + 0.02];
                self.state.projectiles.push(Projectile {
                    id: pid,
                    owner,
                    source: id,
                    target: None,
                    origin: pos,
                    destination: pos,
                    x: launch[0],
                    y: launch[1],
                    z: launch[2],
                    age: 0.,
                    duration: catalog::GEMINI_TELEGRAPH_SECONDS,
                    damage: 180. * scale,
                    radius,
                    kind: "delayed-area".into(),
                    damageType: "energy".into(),
                    penetration: 0.,
                    structureMultiplier: 1.,
                    sourceAltitude: 4.,
                    targetAltitude: 0.02,
                    jammed: false,
                    launchPosition: Some(launch),
                    aimPosition: Some(destination),
                    passedSurfaces: vec![],
                    onHitStatus: None,
                    targetMultiplier: 1.,
                    movingTargetMultiplier: 1.,
                });
                self.event("telegraph", pos, owner, radius, id);
            }
            "repair-heal-cut" => {
                let targets: Vec<_> = self
                    .targets(owner)
                    .into_iter()
                    .filter_map(|(id, _, _)| self.target_info_for(id, owner).map(|(p, _)| (id, p)))
                    .filter(|(id, p)| {
                        p.distance(pos) <= radius
                            && self.reachable_skill_target(owner, pos, *id, *p)
                    })
                    .collect();
                for target in self.friendly_skill_targets(owner, pos, radius) {
                    self.heal_skill_target(target, 100. * scale, 100. * scale, false);
                }
                for (target, p) in targets {
                    let before_hp = self.skill_target_hp(target);
                    self.damage_at(target, 65. * scale, "energy", owner, Some(p));
                    if self
                        .skill_target_hp(target)
                        .zip(before_hp)
                        .is_some_and(|(after, before)| after > 0. && after < before)
                    {
                        self.set_heal_cut(target, 10.);
                    }
                }
            }
            "targeted-support" => {
                let f = self
                    .state
                    .units
                    .iter_mut()
                    .find(|f| Some(f.id) == ally)
                    .unwrap();
                f.statuses.insert("support-boost".into(), 12.);
                f.statuses.insert("compute-efficiency".into(), 12.);
            }
            "precision" => {
                let (target, p, destination) = target.unwrap();
                let launch = [u.x, u.y, u.elevation() + 0.5];
                let impact_cell = Pos::new(
                    destination[0].floor().clamp(0., 127.) as i32,
                    destination[1].floor().clamp(0., 95.) as i32,
                    p.level,
                );
                let distance = ((destination[0] - launch[0]).powi(2)
                    + (destination[1] - launch[1]).powi(2)
                    + ((destination[2] - launch[2]) * 4.).powi(2))
                .sqrt();
                let pid = self.id();
                self.state.projectiles.push(Projectile {
                    id: pid,
                    owner,
                    source: id,
                    target: Some(target),
                    origin: u.pos,
                    destination: impact_cell,
                    x: launch[0],
                    y: launch[1],
                    z: launch[2],
                    age: 0.,
                    duration: (distance / 28.).max(0.10),
                    damage: 140. * scale,
                    radius: 0.,
                    kind: "direct".into(),
                    damageType: "kinetic".into(),
                    penetration: 0.,
                    structureMultiplier: 1.,
                    sourceAltitude: launch[2] - u.level as f64,
                    targetAltitude: destination[2] - p.level as f64,
                    jammed: false,
                    launchPosition: Some(launch),
                    aimPosition: Some(destination),
                    passedSurfaces: vec![],
                    onHitStatus: None,
                    targetMultiplier: 1.,
                    movingTargetMultiplier: 1.,
                });
            }
            "overclock" => {
                self.state.units[i].statuses.insert("haste".into(), 10.);
            }
            "guard" => {
                self.state.units[i].statuses.insert("fortify".into(), 10.);
            }
            "target-lock" => {
                let target = target.unwrap().0;
                self.state.units[i].target = Some(target);
                self.state.units[i]
                    .statuses
                    .retain(|key, _| !key.starts_with("target-lock:"));
                self.state.units[i]
                    .statuses
                    .insert(format!("target-lock:{target}"), 10.);
            }
            "charged-shot" => {
                self.state.units[i].chargedShotMultiplier = 1.6 * power_multiplier;
                self.state.units[i]
                    .statuses
                    .insert("charged-shot".into(), 10.);
            }
            "field-recharge" => {
                for (source, cargo, amount) in resupply {
                    if let Some(b) = self.state.buildings.iter_mut().find(|b| b.id == source) {
                        *b.stock.get_mut(cargo).unwrap() -= amount;
                        b.inventory = b.stock.values().sum();
                    }
                    if let Some(r) = self.state.rooms.iter_mut().find(|r| r.id == source) {
                        *r.stock.get_mut(cargo).unwrap() -= amount;
                        r.inventory = r.stock.values().sum();
                    }
                    if cargo == "ammo" {
                        self.state.units[i].ammo += amount;
                    } else {
                        self.state.units[i].fuel += amount;
                    }
                    self.event("field-recharge", u.pos, owner, amount, id);
                }
            }
            _ => unreachable!(),
        }
        if gemini_ready {
            if let Some(unit) = self.state.units.iter_mut().find(|f| f.id == id) {
                unit.statuses.remove("gemini-ready");
            }
        }
        // The event publishes lastCastTick only after a successful paid effect.
        self.event(
            &format!("skill-{}", d.skill),
            if matches!(d.skill_shape.as_str(), "direction" | "cone" | "self") {
                u.pos
            } else {
                pos
            },
            owner,
            radius,
            id,
        );
        if let Some(event) = self.state.events.last_mut() {
            event.direction = Some(aim);
        }
        Ok(())
    }
}
