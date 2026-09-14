use crate::{catalog, Pos, Rect, Room, Save, Snapshot};
use std::collections::BTreeSet;
fn finite(values: &[f64]) -> bool {
    values.iter().all(|v| v.is_finite() && v.abs() < 1e15)
}
fn owner(value: u32) -> bool {
    value == 1 || value == 2
}
fn rect(r: Rect) -> bool {
    r.valid()
}
fn positions<'a>(values: impl Iterator<Item = &'a Pos>) -> bool {
    values.take(200001).all(|p| p.valid())
}
fn stock(values: &std::collections::BTreeMap<String, f64>) -> bool {
    values.len() <= 4
        && values.iter().all(|(k, v)| {
            matches!(k.as_str(), "ore" | "ammo" | "fuel" | "repair") && finite(&[*v]) && *v >= 0.
        })
}
fn reference(id: u64) -> bool {
    id < 9_007_199_254_740_991
}
fn research(r: &crate::Research) -> bool {
    reference(r.lab)
        && catalog::BRANCHES.contains(&r.branch.as_str())
        && (1..=5).contains(&r.target)
        && finite(&[r.progress, r.duration])
        && r.duration > 0.
        && (0.0..=1.0).contains(&r.progress)
}
fn room_capacity_bounds(r: &Room) -> bool {
    let Some(definition) = catalog::facility_ref(&r.kind) else { return false; };
    let gross_limit = (r.rect.area() as f64 * definition.capacity_per_area).floor() as u32;
    r.capacity <= r.capacity_budget()
        && r.capacity <= gross_limit
        && r.gpus.len() <= r.capacity as usize
        && r.potentialCapacity.is_none_or(|p| r.capacity <= p && p <= gross_limit)
}
impl Snapshot {
    pub fn validate(&self, partial: bool) -> Result<(), String> {
        if self.version != 6
            || self.width != 128
            || self.height != 96
            || self.minLevel != -2
            || self.maxLevel != 5
            || !matches!(self.theme.as_str(), "river" | "mining" | "highland")
            || self.terrain.len() != 128 * 96
            || self.terrain.iter().any(|t| *t > 6)
        {
            return Err("地图版本或尺寸无效".into());
        }
        if self.players.len() != 2
            || self.players.iter().map(|p| p.owner).collect::<Vec<_>>() != vec![1, 2]
            || self.visible.len() != 2
            || self.explored.len() != 2
        {
            return Err("玩家槽位或视野数据无效".into());
        }
        if self.buildings.len() > 1024
            || self.rooms.len() > 4096
            || self.units.len() > 2048
            || self.projectiles.len() > 16384
            || self.events.len() > 4096
            || self.links.len() > 4096
            || self.walls.len() > 98304
            || self.entrances.len() > 4096
            || self.jobs.len() > 8192
            || self.rubble.len() > 8192
            || self.shipments.len() > 8192
            || self.resources.len() > 16384
            || self.networkStores.len() > 4096
            || self.powerGrids.len() > 4096
            || self.shieldRegions.len() > 4096
            || self.defenseFields.len() > 4096
            || self.winner.is_some_and(|p| !owner(p))
            || !reference(self.tick)
            || !reference(self.revision)
        {
            return Err("快照超过本地图实体预算".into());
        }
        let mut ids = BTreeSet::new();
        let mut valid_id = |id: u64| id > 0 && id < 9_007_199_254_740_991 && ids.insert(id);
        for p in &self.players {
            if !finite(&[
                p.credits,
                p.compute,
                p.computeCapacity,
                p.power,
                p.demand,
                p.science,
                p.production,
                p.income,
                p.dominance,
                p.lostValue,
            ]) || p.credits < 0.
                || p.compute < 0.
                || p.science < 0.
                || p.branches
                    .iter()
                    .any(|(b, t)| !catalog::BRANCHES.contains(&b.as_str()) || !(1..=5).contains(t))
                || p.researches.len() > 4096
                || p.researches.iter().any(|r| !research(r))
                || p.research.as_ref().is_some_and(|r| !research(r))
                || p.totals.len() > 64
                || p.totals.values().any(|v| !finite(&[*v]) || *v < 0.)
            {
                return Err("玩家资源或研究数据无效".into());
            }
        }
        for b in &self.buildings {
            if !valid_id(b.id)
                || !owner(b.owner)
                || !rect(b.rect)
                || !finite(&[
                    b.hp,
                    b.antiHeal,
                    b.maxHp,
                    b.progress,
                    b.power,
                    b.demand,
                    b.inventory,
                    b.invested,
                    b.jam,
                    b.shield,
                    b.buildTime,
                    b.supportRatio,
                    b.collapseWarning,
                ])
                || b.maxHp <= 0.
                || !(0.0..=1.0).contains(&b.progress)
                || !stock(&b.stock)
                || b.antiHeal < 0.
                || b.invested < 0.
                || b.born > self.tick
                || !(1..=5).contains(&b.tier)
                || !(b.kind == "core"
                    || b.kind == "shell"
                    || catalog::outdoor_building(&b.kind).is_some())
            {
                return Err("建筑标识、范围或状态无效".into());
            }
        }
        let gpu = catalog::gpus();
        for r in &self.rooms {
            if !valid_id(r.id)
                || !owner(r.owner)
                || !rect(r.rect)
                || catalog::facility(&r.kind).is_none()
                || !finite(&[
                    r.hp,
                    r.antiHeal,
                    r.maintenance,
                    r.equipmentShare,
                    r.invested,
                    r.maxHp,
                    r.progress,
                    r.buildTime,
                    r.inventory,
                    r.cooldown,
                ])
                || r.maxHp <= 0.
                || !(0.0..=1.0).contains(&r.progress)
                || r.gpus.len() > 576
                || !room_capacity_bounds(r)
                || r.gpus.iter().any(|id| !gpu.iter().any(|g| &g.id == id))
                || !stock(&r.stock)
                || r.antiHeal < 0.
                || r.invested < 0.
                || r.maintenance < 0.
                || r.equipmentShare <= 0.
                || r.equipmentShare > 4096.
                || !(1..=5).contains(&r.tier)
                || r.branch
                    .as_ref()
                    .is_some_and(|b| !catalog::BRANCHES.contains(&b.as_str()))
            {
                return Err("房间状态无效".into());
            }
            if !partial {
                let potential = crate::construction::room_geometry_capacity(&r.kind, r.rect, &self.entrances);
                if r.capacity != r.capacity_budget().min(potential)
                    || r.potentialCapacity.is_some_and(|p| p != potential)
                {
                    return Err("房间可用容量与已购预算或当前净面积不一致".into());
                }
            }
            if !partial
                && !self.buildings.iter().any(|b| {
                    b.id == r.shell
                        && b.owner == r.owner
                        && r.rect.cells().iter().all(|p| b.rect.contains(*p))
                })
            {
                return Err("房间没有所属楼体".into());
            }
        }
        for u in &self.units {
            if !valid_id(u.id)
                || !owner(u.owner)
                || !u.pos.valid()
                || !(-2..=5).contains(&u.level)
                || catalog::unit(&u.kind).is_none()
                || u.route.len() > 98304
                || u.queuedGoals.len() > 16
                || !positions(u.queuedGoals.iter())
                || !positions(u.route.iter())
                || !finite(&[
                    u.x,
                    u.y,
                    u.velocityX,
                    u.velocityY,
                    u.hp,
                    u.maxHp,
                    u.ammo,
                    u.fuel,
                    u.ammoMax,
                    u.fuelMax,
                    u.altitude,
                    u.transitProgress,
                    u.invested,
                    u.energy,
                    u.energyMax,
                    u.battery,
                    u.batteryMax,
                    u.cooldown,
                    u.skillCooldown,
                    u.pluginDiscount,
                    u.chargedShotMultiplier,
                ])
                || u.x < 0.
                || u.x >= 128.
                || u.y < 0.
                || u.y >= 96.
                || !(1..=5).contains(&u.tier)
                || u.pos.level != u.level
                || !(0.0..=1.0).contains(&u.transitProgress)
                || u.wired && u.transitProgress > 0.
                || u.transitProgress > 0.
                    && u.route.first().is_none_or(|p| {
                        p.x != u.pos.x || p.y != u.pos.y || (p.level - u.level).abs() != 1
                    })
                || !(0.0..=8.).contains(&u.altitude)
                || u.facing > 7
                || u.maxHp <= 0.
                || [
                    u.ammo,
                    u.ammoMax,
                    u.fuel,
                    u.fuelMax,
                    u.energy,
                    u.energyMax,
                    u.battery,
                    u.batteryMax,
                    u.invested,
                ]
                .iter()
                .any(|n| *n < 0.)
                || u.goal.is_some_and(|p| !p.valid())
                || u.sortieTarget.is_some_and(|p| !p.valid())
                || [u.lastAttackTick, u.lastCastTick, u.lastHitTick]
                    .into_iter()
                    .flatten()
                    .any(|t| t > self.tick)
                || !reference(u.sourceFacility)
                || u.target.is_some_and(|id| !reference(id))
                || !matches!(
                    u.flightState.as_str(),
                    "" | "ground"
                        | "taking-off"
                        | "landing"
                        | "landed"
                        | "cruising"
                        | "returning"
                        | "emergency"
                )
                || u.dash.as_ref().is_some_and(|d| {
                    !finite(&[d.previous[0], d.previous[1], d.remaining, d.damage, d.width])
                        || !(0.0..128.).contains(&d.previous[0])
                        || !(0.0..96.).contains(&d.previous[1])
                        || !(0.0..=2.).contains(&d.remaining)
                        || d.damage < 0.
                        || d.width < 0.
                        || catalog::unit_ref(&u.kind).is_none_or(|def| {
                            def.skill != "dash-strike" || d.width > def.skill_width + 1e-6
                        })
                        || d.hitTargets.len() > 8192
                        || d.hitTargets.iter().any(|id| !reference(*id))
                })
                || u.plugins.len() > 3
                || !(0.0..=0.5).contains(&u.pluginDiscount)
                || !(1.0..=8.0).contains(&u.chargedShotMultiplier)
                || u.statuses.len() > 64
                || u.plugins.iter().any(|p| catalog::plugin(p).is_none())
                || u.statuses.values().any(|v| !v.is_finite())
            {
                return Err("单位范围、型号或数值无效".into());
            }
        }
        for e in &self.entrances {
            if !valid_id(e.id)
                || !owner(e.owner)
                || !e.pos.valid()
                || !(-2..=5).contains(&e.toLevel)
                || !(1..=4).contains(&e.width)
                || !finite(&[e.hp])
                || e.hp <= 0. && (e.kind != "door" || e.hp != 0. || !e.open)
                || !matches!(e.axis.as_str(), "" | "x" | "y")
                || !(if e.axis == "y" {
                    Pos::new(e.pos.x, e.pos.y + e.width as i32 - 1, e.pos.level)
                } else {
                    Pos::new(e.pos.x + e.width as i32 - 1, e.pos.y, e.pos.level)
                })
                .valid()
                || matches!(e.kind.as_str(), "door" | "window") && e.toLevel != e.pos.level
                || matches!(e.kind.as_str(), "stairs" | "ramp")
                    && (e.toLevel - e.pos.level).abs() != 1
                || !matches!(
                    e.kind.as_str(),
                    "door" | "window" | "ramp" | "stairs" | "elevator"
                )
            {
                return Err("入口状态无效".into());
            }
        }
        for l in &self.links {
            if !valid_id(l.id)
                || !owner(l.owner)
                || l.path.is_empty()
                || l.path.len() > 512
                || !positions(l.path.iter())
                || !finite(&[l.hp, l.invested])
                || l.invested < 0.
                || l.unitEndpoints.len()>2
                || l.unitEndpoints.iter().any(|id|*id==0||!reference(*id))
                || l.unitEndpoints.iter().collect::<BTreeSet<_>>().len()!=l.unitEndpoints.len()
                || l.kind!="compute"&&!l.unitEndpoints.is_empty()
                || l.hp > 0. && l.unitEndpoints.iter().any(|id| self.units.iter().any(|u|
                    u.id == *id && u.owner == l.owner && u.hp > 0. && u.transitProgress > 0.
                    && catalog::unit_ref(&u.kind).is_some_and(|d| d.category == "ai")
                    && (l.path.first() == Some(&u.pos) || l.path.last() == Some(&u.pos))))
                || !partial
                    && l.path.windows(2).any(|w| {
                        (w[0].x - w[1].x).abs()
                            + (w[0].y - w[1].y).abs()
                            + (w[0].level - w[1].level).abs()
                            != 1
                    })
                || !matches!(l.kind.as_str(), "power" | "compute")
            {
                return Err("线路状态无效".into());
            }
        }
        for w in &self.walls {
            if !valid_id(w.id)
                || !owner(w.owner)
                || !w.pos.valid()
                || !finite(&[w.hp, w.maxHp, w.shield, w.antiHeal, w.invested])
                || w.maxHp <= 0.
                || w.antiHeal < 0.
                || w.invested < 0.
                || !matches!(w.kind.as_str(), "physical" | "cuda" | "moat")
            {
                return Err("防御墙状态无效".into());
            }
        }
        for r in &self.resources {
            if !valid_id(r.id)
                || r.owner > 2
                || r.capturer > 2
                || !r.pos.valid()
                || !finite(&[r.remaining, r.capture])
                || !matches!(
                    r.kind.as_str(),
                    "ore"
                        | "coal"
                        | "node"
                        | "salvage"
                        | "salvage-ore"
                        | "salvage-ammo"
                        | "salvage-fuel"
                        | "salvage-repair"
                )
                || r.remaining < 0. && !(partial && r.remaining == -1.)
            {
                return Err("资源节点状态无效".into());
            }
        }
        for s in &self.shipments {
            if !valid_id(s.id)
                || !owner(s.owner)
                || !s.pos.valid()
                || s.route.len() > 16384
                || !positions(s.route.iter())
                || !finite(&[s.hp, s.amount, s.progress,s.unloadProgress])
                || !(0.0..=1.0).contains(&s.unloadProgress)
                || s.amount < 0.
                || !(0.0..=1.0).contains(&s.progress)
                || !matches!(s.cargo.as_str(), "ore" | "ammo" | "fuel" | "repair")
                || !reference(s.from)
                || !reference(s.to)
                || s.waypoints.len() > 16
                || !positions(s.waypoints.iter())
                || !matches!(s.mode.as_str(), "ground" | "air")
                || !matches!(
                    s.flightState.as_str(),
                    "ground" | "taking-off" | "cruising" | "landing" | "landed" | "emergency"
                )
                || !finite(&[s.altitude, s.fuel, s.fuelMax, s.flightTimer])
                || !(0.0..=8.).contains(&s.altitude)
                || s.fuel < 0.
                || s.fuelMax < 0.
                || s.fuel > s.fuelMax + 1e-6
                || s.mode == "air"
                    && (s.altitude > 2.000001
                        || (s.fuelMax - catalog::AIR_TRANSPORT_FUEL_MAX).abs() > 1e-6
                        || s.flightTimer > catalog::AIR_TRANSPORT_PHASE_SECONDS + 1e-6
                        || s.pos.level != 0
                        || s.route
                            .iter()
                            .chain(s.waypoints.iter())
                            .any(|p| p.level != 0))
                || !(0.0..=10.).contains(&s.flightTimer)
            {
                return Err("运输状态无效".into());
            }
        }
        for p in &self.projectiles {
            if !valid_id(p.id)
                || !owner(p.owner)
                || !p.origin.valid()
                || !p.destination.valid()
                || !finite(&[
                    p.x,
                    p.y,
                    p.z,
                    p.age,
                    p.duration,
                    p.damage,
                    p.radius,
                    p.sourceAltitude,
                    p.targetAltitude,
                    p.structureMultiplier,
                    p.penetration,
                    p.targetMultiplier,
                    p.movingTargetMultiplier,
                ])
                || p.duration <= 0.
                || !reference(p.source)
                || p.target.is_some_and(|id| !reference(id))
                || p.launchPosition.is_some_and(|p| !finite(&p))
                || p.aimPosition.is_some_and(|p| !finite(&p))
                || p.passedSurfaces.len() > 131072
                || p.passedSurfaces
                    .iter()
                    .any(|p| !reference(p[0]) || p[1] > 1_000_000)
                || p.damage < 0.
                || p.radius < 0.
                || p.radius > 128.
                || p.penetration < 0.
                || p.structureMultiplier < 0.
                || !(1.0..=4.0).contains(&p.targetMultiplier)
                || !(1.0..=4.0).contains(&p.movingTargetMultiplier)
                || p.onHitStatus.as_ref().is_some_and(|s| {
                    !matches!(s.kind.as_str(), "marked" | "anti-heal" | "slow")
                        || !s.duration.is_finite()
                        || !(0.0..=120.).contains(&s.duration)
                })
            {
                return Err("弹道状态无效".into());
            }
        }
        for j in &self.jobs {
            if !valid_id(j.id)
                || !owner(j.owner)
                || !j.worker.valid()
                || !rect(j.rect)
                || !positions(j.route.iter())
                || !finite(&[j.duration, j.progress, j.invested])
                || j.duration <= 0.
                || j.route.len() > 98304
                || !(0.0..=1.0).contains(&j.progress)
                || j.invested < 0.
                || !reference(j.target)
                || j.beforeBuilding.as_ref().is_some_and(|b| {
                    !rect(b.rect)
                        || b.id != j.target
                        || b.owner != j.owner
                        || !finite(&[b.hp, b.maxHp, b.invested, b.progress, b.antiHeal])
                        || b.maxHp <= 0.
                        || b.invested < 0.
                        || !stock(&b.stock)
                })
                || j.beforeRoom.as_ref().is_some_and(|r| {
                    !rect(r.rect)
                        || !room_capacity_bounds(r)
                        || r.id != j.target
                        || r.owner != j.owner
                        || !finite(&[
                            r.hp,
                            r.maxHp,
                            r.invested,
                            r.progress,
                            r.antiHeal,
                            r.equipmentShare,
                        ])
                        || r.maxHp <= 0.
                        || r.invested < 0.
                        || r.equipmentShare <= 0.
                        || r.equipmentShare > 4096.
                        || catalog::facility_ref(&r.kind).is_none()
                        || !stock(&r.stock)
                })
            {
                return Err("施工任务无效".into());
            }
        }
        for e in &self.events {
            if !valid_id(e.id)
                || e.owner > 2
                || !e.pos.valid()
                || e.tick > self.tick
                || !finite(&[e.magnitude])
                || !reference(e.subject)
                || e.direction.is_some_and(|p| {
                    !(-1..=128).contains(&p.x)
                        || !(-1..=96).contains(&p.y)
                        || !(-2..=5).contains(&p.level)
                })
                || e.rect.is_some_and(|r| !rect(r))
                || e.kind.len() > 128
                || e.subjectKind.len() > 128
                || e.presentationPosition.is_some_and(|p| !finite(&p))
                || e.facing.is_some_and(|d| d > 7)
            {
                return Err("事件状态无效".into());
            }
        }
        for f in &self.defenseFields {
            if !valid_id(f.id)
                || !owner(f.owner)
                || !f.pos.valid()
                || !f.direction.valid()
                || !finite(&[f.radius, f.angle, f.hp, f.remaining])
                || f.radius < 0.
                || f.radius > 128.
                || f.remaining < 0.
                || f.kind.len() > 128
            {
                return Err("防御区域无效".into());
            }
        }
        for r in &self.rubble {
            if !valid_id(r.id) || r.owner > 2 || !rect(r.rect) || !finite(&[r.salvage]) {
                return Err("废墟数据无效".into());
            }
        }
        if self
            .visible
            .iter()
            .chain(self.explored.iter())
            .any(|v| v.len() > 98304 || !positions(v.iter()))
            || self.excavated.len() > 24576
            || !positions(self.excavated.iter())
            || self.excavated.iter().any(|p| p.level >= 0)
            || self
                .excavationOwners
                .iter()
                .any(|(cell, o)| *cell >= 98304 || !owner(*o))
        {
            return Err("可见或挖掘格子越界".into());
        }
        if self.networkStores.iter().any(|s| {
            !owner(s.owner)
                || !s.anchor.valid()
                || !positions(s.cells.iter())
                || !finite(&[s.compute, s.capacity, s.production])
                || s.compute < 0.
                || s.cells.len() > 98304
                || s.capacity < 0.
                || s.production < 0.
        }) || self.powerGrids.iter().any(|g| {
            !owner(g.owner)
                || g.cells.len() > 98304
                || !positions(g.cells.iter())
                || !finite(&[g.output, g.load])
                || g.output < 0.
                || g.load < 0.
        }) || self.shieldRegions.iter().any(|r| {
            !owner(r.owner)
                || !r.anchor.valid()
                || !positions(r.cells.iter())
                || !finite(&[r.current, r.capacity])
                || !r.network.valid()
                || r.cells.len() > 98304
                || r.current < 0.
                || r.capacity < 0.
        }) {
            return Err("资源网络状态无效".into());
        }
        if self.playback.as_ref().is_some_and(|p| {
            !p.speed.is_finite()
                || !(0.5..=8.).contains(&p.speed)
                || p.currentTick > p.totalTicks
                || !reference(p.totalTicks)
        }) {
            return Err("回放状态无效".into());
        }
        Ok(())
    }
}
impl Save {
    pub fn validate(&self) -> Result<(), String> {
        if self.rulesVersion.is_empty()||self.rulesFingerprint.is_empty(){return Err("旧开发存档缺少规则指纹，请用创建它的引擎版本打开；原文件保留".into());}
        if self.rulesVersion!=crate::RULES_VERSION||self.rulesFingerprint!=crate::RULES_FINGERPRINT{return Err("存档规则指纹与当前引擎不一致，拒绝加载或回放；原文件保留".into());}
        self.snapshot.validate(false)?;
        if self.snapshot.playback.is_some() {
            return Err("回放显示状态不能载入为权威存档".into());
        }
        if self.orders.len() > 500000
            || self.nextId == 0
            || !reference(self.nextId)
            || self.orders.iter().any(|l| {
                !owner(l.order.owner)
                    || l.tick > self.snapshot.tick
                    || l.receipt.tick != l.tick
                    || l.receipt.sequence != l.order.sequence
            })
            || self.administrativeEvents.iter().any(|e| {
                !owner(e.owner) || e.tick > self.snapshot.tick || e.orderIndex > self.orders.len()
            })
        {
            return Err("存档指令日志无效".into());
        }
        let max_id = self
            .snapshot
            .buildings
            .iter()
            .map(|v| v.id)
            .chain(self.snapshot.rooms.iter().map(|v| v.id))
            .chain(self.snapshot.units.iter().map(|v| v.id))
            .chain(self.snapshot.entrances.iter().map(|v| v.id))
            .chain(self.snapshot.links.iter().map(|v| v.id))
            .chain(self.snapshot.walls.iter().map(|v| v.id))
            .chain(self.snapshot.resources.iter().map(|v| v.id))
            .chain(self.snapshot.shipments.iter().map(|v| v.id))
            .chain(self.snapshot.projectiles.iter().map(|v| v.id))
            .chain(self.snapshot.events.iter().map(|v| v.id))
            .chain(self.snapshot.jobs.iter().map(|v| v.id))
            .chain(self.snapshot.rubble.iter().map(|v| v.id))
            .chain(self.snapshot.defenseFields.iter().map(|v| v.id))
            .max()
            .unwrap_or(0);
        if self.nextId <= max_id {
            return Err("存档下一实体编号重复".into());
        }
        let mut sequences = [0u64; 2];
        let mut last_tick = 0;
        for log in &self.orders {
            let i = (log.order.owner - 1) as usize;
            if log.tick < last_tick || log.order.sequence != sequences[i] + 1 {
                return Err("存档指令顺序损坏".into());
            }
            sequences[i] = log.order.sequence;
            last_tick = log.tick;
        }
        if sequences != self.sequences
            || self
                .administrativeEvents
                .windows(2)
                .any(|v| (v[0].tick, v[0].orderIndex) > (v[1].tick, v[1].orderIndex))
            || self.administrativeEvents.iter().any(|e| {
                self.orders
                    .get(e.orderIndex)
                    .is_some_and(|l| l.tick < e.tick)
                    || e.orderIndex > 0 && self.orders[e.orderIndex - 1].tick > e.tick
            })
        {
            return Err("存档指令游标或管理事件无效".into());
        }
        Ok(())
    }
}
