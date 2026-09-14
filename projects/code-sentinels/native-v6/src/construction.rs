use crate::{catalog, types::*, Game};
use std::collections::{BTreeMap, BTreeSet, VecDeque};

pub(crate) fn room_net_area_for(rect: Rect, entrances: &[Entrance]) -> i32 {
    let reserved = rect.cells().iter().filter(|p| {
        entrances.iter().any(|entry| entry.reserves_space() && entry.covers(**p))
    }).count() as i32;
    (rect.area() - reserved).max(0)
}

pub(crate) fn room_geometry_capacity(kind: &str, rect: Rect, entrances: &[Entrance]) -> u32 {
    let rate = catalog::facility_ref(kind).map(|f| f.capacity_per_area).unwrap_or(0.);
    (room_net_area_for(rect, entrances) as f64 * rate).floor() as u32
}

pub(crate) fn fill_room_capacity_metadata(room: &mut Room, entrances: &[Entrance]) {
    if room.capacityBudget.is_none() { room.capacityBudget = Some(room.capacity); }
    if room.potentialCapacity.is_none() {
        room.potentialCapacity = Some(room_geometry_capacity(&room.kind, room.rect, entrances));
    }
}

impl Game {
    /// Geometry edits may hide or restore purchased capacity, never mint it.
    pub fn refresh_room_capacities(&mut self) {
        for room in &mut self.state.rooms {
            let budget = room.capacity_budget();
            let potential = room_geometry_capacity(&room.kind, room.rect, &self.state.entrances);
            room.capacityBudget = Some(budget);
            room.potentialCapacity = Some(potential);
            room.capacity = budget.min(potential);
        }
    }
    fn repair_rect(&self, target: u64) -> Option<Rect> {
        self.state
            .buildings
            .iter()
            .find(|b| b.id == target && b.hp > 0.)
            .map(|b| b.rect)
            .or_else(|| {
                self.target_info(target).map(|(p, _)| Rect {
                    x: p.x,
                    y: p.y,
                    level: p.level,
                    width: 1,
                    height: 1,
                })
            })
    }
    fn repair_in_range(&self, target: u64, worker: Pos) -> bool {
        if self
            .state
            .units
            .iter()
            .any(|u| u.id == target && u.altitude > 0.1)
            || self
                .state
                .shipments
                .iter()
                .any(|s| s.id == target && s.altitude > 0.1)
        {
            return false;
        }
        self.repair_rect(target).is_some_and(|r| {
            worker.level == r.level
                && worker.distance(Pos::new(
                    worker.x.clamp(r.x, r.x + r.width - 1),
                    worker.y.clamp(r.y, r.y + r.height - 1),
                    r.level,
                )) <= 1.5
        })
    }
    pub fn construction_route(&self, start: Pos, rect: Rect, kind: &str) -> Option<Vec<Pos>> {
        let mut candidates = BTreeSet::new();
        let layers = if matches!(kind, "room" | "convert" | "repair") {
            vec![rect.level]
        } else if rect.level < 0 {
            vec![rect.level, rect.level + 1]
        } else if rect.level > 0 {
            vec![rect.level - 1]
        } else {
            vec![0]
        };
        for level in layers {
            for y in rect.y - 2..rect.y + rect.height + 2 {
                for x in rect.x - 2..rect.x + rect.width + 2 {
                    let p = Pos::new(x, y, level);
                    let inside = x >= rect.x
                        && x < rect.x + rect.width
                        && y >= rect.y
                        && y < rect.y + rect.height;
                    let eligible = if matches!(kind, "room" | "convert") {
                        inside
                    } else if kind == "repair" {
                        p.distance(Pos::new(
                            x.clamp(rect.x, rect.x + rect.width - 1),
                            y.clamp(rect.y, rect.y + rect.height - 1),
                            rect.level,
                        )) <= 1.5
                    } else {
                        inside
                            || x == rect.x - 1
                            || x == rect.x + rect.width
                            || y == rect.y - 1
                            || y == rect.y + rect.height
                    };
                    if eligible && self.walkable(p, "ai") {
                        candidates.insert(p);
                    }
                }
            }
        }
        if candidates.is_empty() || !start.valid() {
            return None;
        }
        let mut prev = BTreeMap::from([(start, start)]);
        let mut queue = VecDeque::from([start]);
        let mut finish = None;
        while let Some(p) = queue.pop_front() {
            if candidates.contains(&p) {
                finish = Some(p);
                break;
            }
            let mut next = vec![
                Pos::new(p.x + 1, p.y, p.level),
                Pos::new(p.x - 1, p.y, p.level),
                Pos::new(p.x, p.y + 1, p.level),
                Pos::new(p.x, p.y - 1, p.level),
                Pos::new(p.x, p.y, p.level + 1),
                Pos::new(p.x, p.y, p.level - 1),
            ];
            for n in next.drain(..) {
                if !prev.contains_key(&n) && self.can_step(p, n, "ai") {
                    prev.insert(n, p);
                    queue.push_back(n);
                }
            }
        }
        let mut current = finish?;
        let mut route = Vec::new();
        while current != start {
            route.push(current);
            current = prev[&current];
        }
        route.reverse();
        Some(route)
    }
    pub fn queue_repair(&mut self, owner: u32, target: u64) -> Result<(), String> {
        let (pos, o) = self.target_info(target).ok_or("维修目标不存在")?;
        if o != owner || self.state.jobs.iter().any(|j| j.target == target) {
            return Err("只能维修己方空闲对象".into());
        }
        if self.state.shipments.iter().any(|s| s.id == target) {
            return Err("运输载具请使用实际维修补给".into());
        }
        if self
            .state
            .units
            .iter()
            .any(|u| u.id == target && u.altitude > 0.1)
        {
            return Err("飞行器必须先降落再维修".into());
        }
        let repair_rect = self.repair_rect(target).ok_or("维修目标不存在")?;
        let mut sources:Vec<_> = self
            .state
            .buildings
            .iter()
            .filter(|b| {
                b.owner == owner && b.hp > 0. && b.stock.get("repair").copied().unwrap_or(0.) >= 10.
            })
            .map(|b| (b.id, b.rect.center()))
            .chain(
                self.state
                    .rooms
                    .iter()
                    .filter(|r| {
                        r.owner == owner
                            && r.hp > 0.
                            && r.stock.get("repair").copied().unwrap_or(0.) >= 10.
                    })
                    .map(|r| (r.id, r.rect.center())),
            )
            .collect();
        if sources.is_empty(){return Err("没有维修物资，先运输补给".into());}
        // Stable distance order preserves the previous preference among ties;
        // an inaccessible nearby stockpile must not hide a reachable one.
        sources.sort_by(|a,b|a.1.distance(pos).total_cmp(&b.1.distance(pos)));
        let (source,route) = sources.into_iter().find_map(|source|
            self.construction_route(source.1,repair_rect,"repair").map(|route|(source,route)))
            .ok_or("维修无人机无可达路线")?;
        self.spend(owner, 20., 0.)?;
        if let Some(b) = self.state.buildings.iter_mut().find(|b| b.id == source.0) {
            *b.stock.get_mut("repair").unwrap() -= 10.;
        }
        if let Some(r) = self.state.rooms.iter_mut().find(|r| r.id == source.0) {
            *r.stock.get_mut("repair").unwrap() -= 10.;
        }
        let rect = repair_rect;
        self.enqueue_construction(owner, target, rect, "repair", 5., 20.);
        let job = self.state.jobs.last_mut().unwrap();
        job.worker = source.1;
        job.route = route;
        job.blocked = false;
        Ok(())
    }
    pub fn shell_cost(rect: Rect) -> f64 {
        (rect.area() as f64 * 6. + (rect.width + rect.height) as f64 * 4.)
            * (1. + rect.level.max(0) as f64 * 0.22)
    }
    pub fn shell_hp(rect: Rect) -> f64 {
        180. * (rect.area() as f64).powf(0.85)
    }
    pub fn room_hp(rect: Rect) -> f64 {
        140. * (rect.area() as f64 / 4.).powf(0.85)
    }
    pub fn floor_limit(&self, owner: u32) -> (i32, i32) {
        match self.tech(owner, "").clamp(1, 5) {
            1 => (0, 0),
            2 => (1, -1),
            3 => (2, -1),
            4 => (3, -2),
            _ => (5, -2),
        }
    }
    pub fn enqueue_construction(
        &mut self,
        owner: u32,
        target: u64,
        rect: Rect,
        kind: &str,
        duration: f64,
        invested: f64,
    ) {
        self.invalidate_navigation();
        let worker = self
            .state
            .buildings
            .iter()
            .find(|b| b.owner == owner && b.kind == "core")
            .map(|b| b.rect.center())
            .unwrap_or(rect.center());
        let route = self.construction_route(worker, rect, kind);
        let id = self.id();
        self.state.jobs.push(ConstructionJob {
            id,
            owner,
            target,
            rect,
            kind: kind.into(),
            worker,
            route: route.clone().unwrap_or_default(),
            progress: 0.,
            duration,
            invested,
            blocked: route.is_none(),
            beforeBuilding: None,
            beforeRoom: None,
        });
    }
    pub fn construction_tick(&mut self) {
        let dead: Vec<u64> = self
            .state
            .jobs
            .iter()
            .filter(|j| {
                j.target > 0 && j.kind != "clear-rubble" && self.target_info(j.target).is_none()
            })
            .map(|j| j.id)
            .collect();
        self.state.jobs.retain(|j| !dead.contains(&j.id));
        let mut active = [0; 2];
        let mut completed = Vec::new();
        for i in 0..self.state.jobs.len() {
            let owner = self.state.jobs[i].owner;
            if active[(owner - 1) as usize] >= 2 {
                continue;
            }
            if self.state.jobs[i].kind == "repair" {
                let target = self.repair_rect(self.state.jobs[i].target);
                if let Some(target) = target {
                    if self.state.tick % 30 == 0
                        && !self
                            .repair_in_range(self.state.jobs[i].target, self.state.jobs[i].worker)
                    {
                        let worker = self.state.jobs[i].worker;
                        let route = self.construction_route(worker, target, "repair");
                        self.state.jobs[i].route = route.clone().unwrap_or_default();
                        self.state.jobs[i].blocked = route.is_none();
                        self.state.jobs[i].progress = 0.;
                        self.state.jobs[i].rect = target;
                    }
                } else {
                    self.state.jobs[i].blocked = true;
                    continue;
                }
            }
            if self.state.jobs[i].blocked {
                if (self.state.tick + self.state.jobs[i].id) % 120 == 0 {
                    let j = &self.state.jobs[i];
                    if let Some(route) = self.construction_route(j.worker, j.rect, &j.kind) {
                        self.state.jobs[i].route = route;
                        self.state.jobs[i].blocked = false;
                    }
                }
                continue;
            }
            active[(owner - 1) as usize] += 1;
            if !self.state.jobs[i].route.is_empty() {
                if self.state.tick % 6 == 0 {
                    let next = self.state.jobs[i].route[0];
                    if self.can_step(self.state.jobs[i].worker, next, "ai") {
                        self.state.jobs[i].worker = next;
                        self.state.jobs[i].route.remove(0);
                    } else {
                        self.state.jobs[i].blocked = true;
                    }
                }
                continue;
            }
            if self.state.jobs[i].kind == "repair"
                && !self.repair_in_range(self.state.jobs[i].target, self.state.jobs[i].worker)
            {
                continue;
            }
            let j = &mut self.state.jobs[i];
            let old_progress = j.progress;
            j.progress = (j.progress + 1. / 60. / j.duration.max(1.)).min(1.);
            if j.kind != "repair" {
                if let Some(b) = self.state.buildings.iter_mut().find(|b| b.id == j.target) {
                    let added = if j.kind == "expand" {
                        b.maxHp
                            - j.beforeBuilding
                                .as_ref()
                                .map(|old| old.maxHp)
                                .unwrap_or(b.maxHp)
                    } else if matches!(j.kind.as_str(), "shell" | "build") {
                        b.maxHp * 0.9
                    } else {
                        0.
                    };
                    b.hp = (b.hp + added * (j.progress - old_progress)).min(b.maxHp);
                    b.progress = j.progress;
                }
            }
            if j.kind != "repair" {
                if let Some(r) = self.state.rooms.iter_mut().find(|r| r.id == j.target) {
                    if j.kind == "room" {
                        r.hp = (r.hp + r.maxHp * 0.9 * (j.progress - old_progress)).min(r.maxHp);
                    }
                    r.progress = j.progress;
                }
            }
            if j.progress >= 1. {
                completed.push(j.clone());
                self.invalidate_navigation();
            }
        }
        for job in completed {
            if job.kind == "repair" {
                let factor = self.repair_multiplier(job.target)*self.material_repair_factor(job.target);
                if let Some(b) = self.state.buildings.iter_mut().find(|b| b.id == job.target) {
                    b.hp = (b.hp + b.maxHp * 0.35 * factor).min(b.maxHp);
                }
                if let Some(r) = self.state.rooms.iter_mut().find(|r| r.id == job.target) {
                    r.hp = (r.hp + r.maxHp * 0.35 * factor).min(r.maxHp);
                }
                if let Some(u) = self.state.units.iter_mut().find(|u| u.id == job.target) {
                    u.hp = (u.hp + u.maxHp * 0.35 * factor).min(u.maxHp);
                }
                if let Some(w) = self.state.walls.iter_mut().find(|w| w.id == job.target) {
                    w.hp = (w.hp + w.maxHp * 0.35 * factor).min(w.maxHp);
                }
                if let Some(l) = self.state.links.iter_mut().find(|l| l.id == job.target) {
                    l.hp = (l.hp + 150. * 0.35 * factor).min(150.);
                }
                if let Some(e) = self.state.entrances.iter_mut().find(|e| e.id == job.target) {
                    e.hp = (e.hp + 250. * 0.35 * factor).min(250.);
                }
                self.event("repair", job.rect.center(), job.owner, 10., job.target);
            }
            if job.kind == "excavate" {
                self.state.excavated.extend(job.rect.cells());
                for p in job.rect.cells() {
                    self.state
                        .excavationOwners
                        .entry(((p.level + 2) * 128 * 96 + p.y * 128 + p.x) as u32)
                        .or_insert(job.owner);
                }
            }
            if job.kind == "clear-rubble" {
                self.state.rubble.retain(|r| r.id != job.target);
                self.player_mut(job.owner).unwrap().credits += job.invested;
            }
            if let Some(r) = self
                .state
                .rooms
                .iter()
                .find(|r| r.id == job.target && r.kind == "research-lab")
            {
                if let Some(branch) = r.branch.clone() {
                    self.player_mut(job.owner)
                        .unwrap()
                        .branches
                        .entry(branch)
                        .or_insert(1);
                }
            }
            self.event(
                "construction-complete",
                job.rect.center(),
                job.owner,
                0.,
                job.target,
            );
            self.state.jobs.retain(|j| j.id != job.id);
        }
    }
    pub fn extra_construction(&mut self, owner: u32, command: Command) -> Result<(), String> {
        match command {
            Command::ExpandShell { id, rect } => {
                let b = self
                    .state
                    .buildings
                    .iter()
                    .find(|b| {
                        b.id == id && b.kind == "shell" && b.owner == owner && b.progress >= 1.
                    })
                    .cloned()
                    .ok_or("选择已完成的毛坯")?;
                if !rect.valid()
                    || rect.width < 4
                    || rect.height < 4
                    || b.rect.cells().iter().any(|p| !rect.contains(*p))
                    || rect.area() <= b.rect.area()
                {
                    return Err("扩建必须完整包含原楼体并增加相邻面积".into());
                }
                if self.state.buildings.iter().any(|other| {
                    other.id != id
                        && other.hp > 0.
                        && rect.cells().iter().any(|p| other.rect.contains(*p))
                }) {
                    return Err("扩建范围与其他建筑重叠".into());
                }
                if rect
                    .cells()
                    .iter()
                    .any(|p| matches!(self.terrain(*p), 1 | 2))
                    || rect.level > 0
                        && rect.cells().iter().any(|p| {
                            !self.state.buildings.iter().any(|s| {
                                s.owner == owner
                                    && s.kind == "shell"
                                    && s.hp > 0.
                                    && s.progress >= 1.
                                    && s.rect.contains(Pos::new(p.x, p.y, p.level - 1))
                            })
                        })
                {
                    return Err("扩建缺少地形或下层承重".into());
                }
                let cost = Self::shell_cost(rect) - Self::shell_cost(b.rect);
                self.spend(owner, cost, 0.)?;
                let shell = self
                    .state
                    .buildings
                    .iter_mut()
                    .find(|s| s.id == id)
                    .unwrap();
                shell.rect = rect;
                shell.maxHp = Self::shell_hp(rect);
                shell.invested += cost;
                shell.progress = 0.;
                self.enqueue_construction(owner, id, rect, "expand", 8., cost);
                self.state.jobs.last_mut().unwrap().beforeBuilding = Some(b);
            }
            Command::SplitRoom { id, axis, offset } => {
                let old = self
                    .state
                    .rooms
                    .iter()
                    .find(|r| r.id == id && r.owner == owner && r.progress >= 1.)
                    .cloned()
                    .ok_or("选择已完成的房间")?;
                let edge = if axis == "x" {
                    old.rect.width
                } else if axis == "y" {
                    old.rect.height
                } else {
                    return Err("分割方向无效".into());
                };
                if offset < 2 || offset > edge - 2 {
                    return Err("分割偏移越界".into());
                }
                let mut a = old.rect;
                let mut b = old.rect;
                if axis == "x" {
                    a.width = offset;
                    b.x += offset;
                    b.width -= offset;
                } else if axis == "y" {
                    a.height = offset;
                    b.y += offset;
                    b.height -= offset;
                } else {
                    return Err("分割方向无效".into());
                }
                if !a.valid() || !b.valid() {
                    return Err("分割后房间太小".into());
                }
                let min_area = catalog::facility_ref(&old.kind)
                    .map(|d| d.min_area)
                    .unwrap_or(1) as i32;
                if self.room_net_area(a) < min_area || self.room_net_area(b) < min_area {
                    return Err("分割后不足该功能的最小净面积".into());
                }
                if self
                    .player(owner)
                    .unwrap()
                    .researches
                    .iter()
                    .any(|r| r.lab == id)
                {
                    return Err("先完成研究再分割".into());
                }
                let ratio = a.area() as f64 / old.rect.area() as f64;
                let equipment_ratio = self.room_net_area(a) as f64
                    / (self.room_net_area(a) + self.room_net_area(b)) as f64;
                let inventory_ratio =
                    if matches!(old.kind.as_str(), "network-defense" | "energy-defense") {
                        equipment_ratio
                    } else {
                        ratio
                    };
                let apotential = self.room_capacity(&old.kind, a);
                let bpotential = self.room_capacity(&old.kind, b);
                let budget = old.capacity_budget();
                let abudget = (budget as u64 * a.area() as u64 / old.rect.area() as u64) as u32;
                let bbudget = budget - abudget;
                let acap = abudget.min(apotential);
                let bcap = bbudget.min(bpotential);
                if acap + bcap > old.capacity {
                    return Err("分割不能凭空增加设备容量".into());
                }
                if old.gpus.len() > (acap + bcap) as usize {
                    return Err("分割后的净机架容量不足".into());
                }
                let newid = self.id();
                let mut right = old.clone();
                right.id = newid;
                right.rect = b;
                right.capacity = bcap;
                right.capacityBudget = Some(bbudget);
                right.potentialCapacity = Some(bpotential);
                right.hp = old.hp * (1. - ratio);
                right.maxHp = old.maxHp * (1. - ratio);
                right.inventory = old.inventory * (1. - inventory_ratio);
                right.equipmentShare = old.equipmentShare * (1. - equipment_ratio);
                right.invested = old.invested * (1. - ratio);
                for value in right.stock.values_mut() {
                    *value *= 1. - ratio;
                }
                let left = self.state.rooms.iter_mut().find(|r| r.id == id).unwrap();
                left.rect = a;
                left.capacity = acap;
                left.capacityBudget = Some(abudget);
                left.potentialCapacity = Some(apotential);
                left.hp *= ratio;
                left.maxHp *= ratio;
                left.inventory *= inventory_ratio;
                left.equipmentShare *= equipment_ratio;
                left.invested *= ratio;
                for value in left.stock.values_mut() {
                    *value *= ratio;
                }
                right.gpus = left.gpus.split_off(left.gpus.len().min(acap as usize));
                self.state.rooms.push(right);
            }
            Command::MergeRooms { ids } => {
                if ids.len() != 2 || ids[0] == ids[1] {
                    return Err("选择两个不同相邻房间".into());
                }
                let a = self
                    .state
                    .rooms
                    .iter()
                    .find(|r| r.id == ids[0] && r.owner == owner)
                    .cloned()
                    .ok_or("房间不存在")?;
                let b = self
                    .state
                    .rooms
                    .iter()
                    .find(|r| r.id == ids[1] && r.owner == owner)
                    .cloned()
                    .ok_or("房间不存在")?;
                if a.shell != b.shell
                    || a.kind != b.kind
                    || a.branch != b.branch
                    || a.progress < 1.
                    || b.progress < 1.
                    || self
                        .player(owner)
                        .unwrap()
                        .researches
                        .iter()
                        .any(|t| ids.contains(&t.lab))
                {
                    return Err("房间用途、分支、楼体须一致且不在工作中".into());
                }
                let x = a.rect.x.min(b.rect.x);
                let y = a.rect.y.min(b.rect.y);
                let w = (a.rect.x + a.rect.width).max(b.rect.x + b.rect.width) - x;
                let h = (a.rect.y + a.rect.height).max(b.rect.y + b.rect.height) - y;
                if w * h != a.rect.area() + b.rect.area() {
                    return Err("合并必须组成完整矩形".into());
                }
                let rect = Rect {
                    x,
                    y,
                    level: a.rect.level,
                    width: w,
                    height: h,
                };
                let potential = self.room_capacity(&a.kind, rect);
                let budget = a.capacity_budget().checked_add(b.capacity_budget())
                    .ok_or("合并设备预算溢出")?;
                let cap = budget.min(potential);
                if a.gpus.len() + b.gpus.len() > cap as usize {
                    return Err("合并净容量不足".into());
                }
                let r = self.state.rooms.iter_mut().find(|r| r.id == a.id).unwrap();
                r.rect = rect;
                r.hp += b.hp;
                r.maxHp += b.maxHp;
                r.inventory += b.inventory;
                r.equipmentShare += b.equipmentShare;
                r.antiHeal = r.antiHeal.max(b.antiHeal);
                r.invested += b.invested;
                r.capacity = cap;
                r.capacityBudget = Some(budget);
                r.potentialCapacity = Some(potential);
                r.gpus.extend(b.gpus);
                for (k, v) in b.stock {
                    *r.stock.entry(k).or_default() += v;
                }
                for shipment in &mut self.state.shipments {
                    if shipment.to == b.id {
                        shipment.to = a.id;
                    }
                    if shipment.from == b.id {
                        shipment.from = a.id;
                    }
                }
                self.state.rooms.retain(|r| r.id != b.id);
            }
            Command::ConvertRoom { id, kind, branch } => {
                let r = self
                    .state
                    .rooms
                    .iter()
                    .find(|r| r.id == id && r.owner == owner && r.progress >= 1.)
                    .cloned()
                    .ok_or("选择可改造房间")?;
                if !r.gpus.is_empty()
                    || r.stock.values().sum::<f64>() > 0.
                    || self
                        .state
                        .shipments
                        .iter()
                        .any(|s| s.to == id || s.from == id)
                    || self
                        .player(owner)
                        .unwrap()
                        .researches
                        .iter()
                        .any(|t| t.lab == id)
                {
                    return Err("先清空显卡并完成研究".into());
                }
                let def = catalog::facilities()
                    .into_iter()
                    .find(|f| f.id == kind)
                    .ok_or("未知用途")?;
                if self.room_net_area(r.rect) < def.min_area as i32 {
                    return Err("此房间的净面积不足以改为该用途".into());
                }
                if self.tech(owner, &def.branch) < def.tier
                    || kind == "research-lab"
                        && !branch
                            .as_ref()
                            .is_some_and(|b| catalog::BRANCHES.contains(&b.as_str()))
                {
                    return Err("分支或科技条件不满足".into());
                }
                self.spend(owner, def.cost * 0.65, 0.)?;
                let potential = self.room_capacity(&kind, r.rect);
                let budget = if kind == r.kind { r.capacity_budget().max(potential) } else { potential };
                let cap = budget.min(potential);
                let room = self
                    .state
                    .rooms
                    .iter_mut()
                    .find(|room| room.id == id)
                    .unwrap();
                room.kind = kind;
                room.equipmentShare = 1.;
                room.branch = branch;
                room.capacity = cap;
                room.capacityBudget = Some(budget);
                room.potentialCapacity = Some(potential);
                room.progress = 0.;
                room.inventory = 0.;
                room.invested += def.cost * 0.65;
                self.enqueue_construction(owner, id, r.rect, "convert", 8., def.cost * 0.65);
                self.state.jobs.last_mut().unwrap().beforeRoom = Some(r);
            }
            Command::Cancel { id } => {
                let job = self
                    .state
                    .jobs
                    .iter()
                    .find(|j| (j.id == id || j.target == id) && j.owner == owner)
                    .cloned()
                    .ok_or("施工任务不存在")?;
                self.player_mut(owner).unwrap().credits += job.invested * (1. - job.progress) * 0.8;
                self.state.jobs.retain(|j| j.id != job.id);
                if job.kind == "shell" || job.kind == "build" {
                    self.state.buildings.retain(|b| b.id != job.target);
                    self.state.rooms.retain(|r| r.shell != job.target);
                } else if job.kind == "room" {
                    self.state.rooms.retain(|r| r.id != job.target);
                } else if let Some(before) = job.beforeBuilding {
                    if let Some(b) = self.state.buildings.iter_mut().find(|b| b.id == job.target) {
                        let hp = b.hp.min(before.hp);
                        *b = before;
                        b.hp = hp;
                    }
                } else if let Some(before) = job.beforeRoom {
                    if let Some(r) = self.state.rooms.iter_mut().find(|r| r.id == job.target) {
                        let hp = r.hp.min(before.hp);
                        *r = before;
                        r.hp = hp;
                    }
                }
                // Other commands may have changed doorway/shaft space while
                // conversion was pending; do not restore stale usable slots.
                self.refresh_room_capacities();
            }
            Command::ClearRubble { id } => {
                let r = self
                    .state
                    .rubble
                    .iter()
                    .find(|r| r.id == id)
                    .cloned()
                    .ok_or("废墟不存在")?;
                if self.state.jobs.iter().any(|j| j.target == id) {
                    return Err("已在清理".into());
                }
                self.spend(owner, 20., 0.)?;
                self.enqueue_construction(owner, id, r.rect, "clear-rubble", 5., r.salvage);
            }
            _ => return Err("非施工命令".into()),
        }
        Ok(())
    }
    pub fn room_capacity(&self, kind: &str, rect: Rect) -> u32 {
        room_geometry_capacity(kind, rect, &self.state.entrances)
    }
    pub fn room_net_area(&self, rect: Rect) -> i32 {
        room_net_area_for(rect, &self.state.entrances)
    }
}
