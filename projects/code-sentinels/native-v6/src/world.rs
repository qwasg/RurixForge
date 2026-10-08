use crate::{catalog, types::*, Game};
use std::collections::{BTreeMap, BTreeSet};
impl Game {
    pub(crate) fn base_new(seed: u64, ai: bool) -> Self {
        let mut terrain = vec![0; 128 * 96];
        for y in 0..96 {
            for x in 0..128 {
                let n =
                    ((x as u64 * 73856093) ^ (y as u64 * 19349663) ^ seed.wrapping_mul(83492791))
                        % 101;
                terrain[y * 128 + x] = if (61..=65).contains(&x) && !(42..=53).contains(&y) {
                    2
                } else if n < 4 {
                    1
                } else if n < 12 {
                    4
                } else {
                    0
                };
            }
        }
        for (y, x) in [(48, 10), (48, 114)] {
            for yy in y - 10..y + 11 {
                for xx in x - 8..x + 9 {
                    terrain[yy * 128 + xx] = 0;
                }
            }
        }
        let players = (1..=2)
            .map(|owner| Player {
                owner,
                credits: 2000.,
                compute: 0.,
                computeCapacity: 0.,
                power: 0.,
                demand: 0.,
                income: 0.,
                production: 0.,
                branches: BTreeMap::new(),
                research: None,
                researches: vec![],
                science: 0.,
                dominance: 0.,
                ai: owner == 2 && ai,
                lostValue: 0.,
                totals: BTreeMap::new(),
            })
            .map(|mut player| {
                catalog::apply_sandbox_stock(&mut player);
                player
            })
            .collect();
        let mut g = Game {
            navigation: crate::navigation::cache(),
            fog_cache:Default::default(),
            collision_cache:Default::default(),
            network_topology:crate::network::cache(),
            administrative_events: vec![],
            state: Snapshot {
                version: 6,
                revision: 1,
                tick: 0,
                seed,
                theme: "river".into(),
                ruleset: crate::types::RULESET_FULL.into(),
                width: 128,
                height: 96,
                minLevel: 0,
                maxLevel: 0,
                players,
                terrain,
                excavated: BTreeSet::new(),
                excavationOwners: BTreeMap::new(),
                buildings: vec![],
                rooms: vec![],
                entrances: vec![],
                units: vec![],
                links: vec![],
                walls: vec![],
                resources: vec![],
                shipments: vec![],
                projectiles: vec![],
                events: Default::default(),
                winner: None,
                winReason: String::new(),
                shieldAuto: [true, true],
                explored: vec![BTreeSet::new(), BTreeSet::new()],
                visible: vec![BTreeSet::new(), BTreeSet::new()],
                jobs: vec![],
                rubble: vec![],
                networkStores: vec![],
                defenseFields: vec![],
                shieldRegions: vec![],
                powerGrids: vec![],
                playback: None,
            },
            orders: vec![],
            next_id: 1,
            sequences: [0; 2],
            shield_auto: [true; 2],
            initial_ai: ai,
            receipts: BTreeMap::new(),
        };
        for (owner, x) in [(1, 8), (2, 116)] {
            let id = g.id();
            g.state.buildings.push(Building {
                id,
                owner,
                rect: Rect {
                    x,
                    y: 46,
                    level: 0,
                    width: 4,
                    height: 4,
                },
                kind: "core".into(),
                tier: 1,
                hp: 6000.,
                antiHeal: 0.,
                maxHp: 6000.,
                progress: 1.,
                buildTime: 0.,
                powered: true,
                connected: false,
                power: 0.,
                demand: 0.,
                capacity: 0,
                branch: None,
                inventory: 500.,
                invested: 0.,
                jam: 0.,
                shield: 0.,
                born: 0,
                collapseWarning: 0.,
                supportRatio: 1.,
                stock: BTreeMap::new(),
            });
        }
        for (x, y, kind) in [
            (18, 40, "ore"),
            (110, 56, "ore"),
            (30, 64, "coal"),
            (44, 34, "ore"),
            (98, 32, "coal"),
            (84, 62, "ore"),
            (64, 24, "node"),
            (64, 48, "node"),
            (64, 72, "node"),
        ] {
            let id = g.id();
            g.state.resources.push(Resource {
                id,
                pos: Pos::new(x, y, 0),
                kind: kind.into(),
                remaining: if kind == "node" {
                    1e9
                } else if kind == "coal" {
                    8000.
                } else if x == 18 || x == 110 {
                    4000.
                } else {
                    16000.
                },
                owner: 0,
                capture: 0.,
                contested: false,
                capturer: 0,
            });
            g.state.terrain[y as usize * 128 + x as usize] = if kind == "coal" {
                6
            } else if kind == "ore" {
                5
            } else {
                3
            };
        }
        for b in &mut g.state.buildings {
            if b.kind == "core" {
                b.stock = BTreeMap::from([
                    ("ammo".into(), 180.),
                    ("fuel".into(), 180.),
                    ("repair".into(), 80.),
                ]);
            }
        }
        g.refresh_fog();
        g
    }
    pub fn terrain(&self, p: Pos) -> u8 {
        if !p.valid() {
            1
        } else {
            self.state.terrain[(p.y * 128 + p.x) as usize]
        }
    }
    fn owned_shell(&self, id: u64, owner: u32) -> Result<&Building, String> {
        self.state
            .buildings
            .iter()
            .find(|b| b.id == id && b.owner == owner && b.kind == "shell" && b.hp > 0.)
            .ok_or("没有可用的己方毛坯建筑".into())
    }
    pub fn complete_lab(&self, owner: u32) -> bool {
        self.classic()
            || self.state.rooms.iter().any(|r| {
                r.owner == owner && r.kind == "research-lab" && r.progress >= 1. && r.hp > 0.
            })
    }
    fn supported(&self, owner: u32, rect: Rect) -> bool {
        rect.level <= 0
            || rect.cells().iter().all(|p| {
                self.state.buildings.iter().any(|b| {
                    b.owner == owner
                        && b.kind == "shell"
                        && b.hp > 0.
                        && b.progress >= 1.
                        && b.rect.contains(Pos::new(p.x, p.y, p.level - 1))
                })
            })
    }
    fn free_ground(&self, rect: Rect) -> bool {
        rect.valid()
            && rect.cells().iter().all(|p| {
                if p.level < 0 {
                    self.state.excavated.contains(p)
                } else if p.level == 0 {
                    !matches!(self.terrain(*p), 1 | 2)
                } else {
                    true
                }
            })
            && !self.state.buildings.iter().any(|b| {
                b.hp > 0.
                    && b.rect.level == rect.level
                    && rect.cells().iter().any(|p| b.rect.contains(*p))
            })
    }
    /// Classic placement anchor: the command core or any completed owned building.
    pub(crate) fn classic_deploy_anchor(&self, owner: u32, pos: Pos) -> Option<u64> {
        self.state
            .buildings
            .iter()
            .filter(|b| {
                b.owner == owner && b.hp > 0. && (b.kind == "core" || b.progress >= 1.)
            })
            .filter(|b| b.rect.center().distance(pos) <= catalog::CLASSIC_DEPLOY_RADIUS)
            .min_by(|a, b| {
                let key = |x: &Building| (x.kind != "core", x.rect.center().distance(pos));
                let (a, b) = (key(a), key(b));
                a.0.cmp(&b.0).then_with(|| a.1.total_cmp(&b.1))
            })
            .map(|b| b.id)
    }
    fn classic_core(&self, owner: u32) -> Result<&Building, String> {
        self.state
            .buildings
            .iter()
            .find(|b| b.owner == owner && b.kind == "core" && b.hp > 0.)
            .ok_or("指挥核心已被摧毁".into())
    }
    /// Commands that only exist for the full base-building ruleset.
    fn classic_rejects(command: &Command) -> bool {
        matches!(
            command,
            Command::Shell { .. }
                | Command::ExpandShell { .. }
                | Command::Room { .. }
                | Command::SplitRoom { .. }
                | Command::MergeRooms { .. }
                | Command::ConvertRoom { .. }
                | Command::InstallGpu { .. }
                | Command::RemoveGpu { .. }
                | Command::Wire { .. }
                | Command::Shield { .. }
        ) || matches!(command, Command::Wall { kind, .. } if kind != "physical")
    }
    pub fn execute(&mut self, owner: u32, command: Command) -> Result<(), String> {
        if !(1..=2).contains(&owner) {
            return Err("玩家不存在".into());
        }
        if self.classic() && Self::classic_rejects(&command) {
            return Err("单层塔防模式不支持该操作".into());
        }
        self.invalidate_navigation();
        self.preflight_site(owner, &command)?;
        match command {
            
            Command::Shell { rect } => {
                if rect.width < 4 || rect.height < 4 || rect.level != 0 || !rect.valid() {
                    return Err("毛坯最小4x4，且必须在地面层".into());
                }
                if !self.free_ground(rect) || !self.supported(owner, rect) {
                    return Err("范围被占用、地形不适合或下层承重未完成".into());
                }
                let cost = Self::shell_cost(rect);
                self.spend(owner, cost, 0.)?;
                let id = self.id();
                let hp = Self::shell_hp(rect);
                self.state.buildings.push(Building {
                    id,
                    owner,
                    rect,
                    kind: "shell".into(),
                    tier: 1,
                    hp: hp * 0.1,
                    antiHeal: 0.,
                    maxHp: hp,
                    progress: 0.,
                    buildTime: 6. + rect.area() as f64 * 0.15,
                    powered: false,
                    connected: false,
                    power: 0.,
                    demand: 0.,
                    capacity: rect.area() as u32,
                    branch: None,
                    inventory: 0.,
                    invested: cost,
                    jam: 0.,
                    shield: 0.,
                    born: self.state.tick,
                    collapseWarning: 0.,
                    supportRatio: 1.,
                    stock: BTreeMap::new(),
                });
                self.event("build", rect.center(), owner, 0., id);
                self.enqueue_construction(
                    owner,
                    id,
                    rect,
                    "shell",
                    6. + rect.area() as f64 * 0.15,
                    cost,
                );
            }
            Command::Room {
                shell,
                rect,
                kind,
                branch,
            } => {
                let b = self.owned_shell(shell, owner)?;
                if b.progress < 1.
                    || !rect.valid()
                    || rect.cells().iter().any(|p| !b.rect.contains(*p))
                    || self
                        .state
                        .rooms
                        .iter()
                        .any(|r| r.hp > 0. && r.rect.cells().iter().any(|p| rect.contains(*p)))
                {
                    return Err("房间须在已完成毛坯内且不得重叠".into());
                }
                let d = catalog::facilities()
                    .into_iter()
                    .find(|d| d.id == kind)
                    .ok_or("未知房间类型")?;
                if !d.branch.is_empty() && self.tech(owner, &d.branch) < d.tier {
                    return Err("对应分支科技不足".into());
                }
                if kind == "research-lab"
                    && branch
                        .as_ref()
                        .map(|b| catalog::BRANCHES.contains(&b.as_str()))
                        != Some(true)
                {
                    return Err("研究所必须选择一个科技分支".into());
                }
                if self.room_net_area(rect) < (d.min_area as i32) {
                    return Err("此功能房间的可用面积不足".into());
                }
                self.spend(owner, d.cost + rect.area() as f64 * 3., 0.)?;
                let id = self.id();
                let max_hp = Self::room_hp(rect);
                let capacity = self.room_capacity(&kind, rect);
                self.state.rooms.push(Room {
                    id,
                    shell,
                    owner,
                    rect,
                    kind: kind.clone(),
                    equipmentShare: 1.,
                    branch: branch.clone(),
                    tier: 1,
                    hp: max_hp * 0.1,
                    antiHeal: 0.,
                    maxHp: max_hp,
                    powered: false,
                    connected: false,
                    online: false,
                    maintenance: 0.,
                    capacity,
                    capacityBudget: Some(capacity),
                    potentialCapacity: Some(capacity),
                    gpus: vec![],
                    inventory: 0.,
                    progress: 0.,
                    buildTime: 8.,
                    cooldown: 0.,
                    stock: BTreeMap::new(),
                    invested: d.cost + rect.area() as f64 * 3.,
                });
                self.event("fitout", rect.center(), owner, 0., id);
                self.enqueue_construction(
                    owner,
                    id,
                    rect,
                    "room",
                    8.,
                    d.cost + rect.area() as f64 * 3.,
                );
            }
            Command::Build { pos, kind } => {
                if pos.level != 0 || !pos.valid() {
                    return Err("露天设施需要地表".into());
                }
                let def = catalog::outdoor_building(&kind).ok_or("未知露天设施")?;
                if self.classic() && !catalog::CLASSIC_BUILDINGS.contains(&kind.as_str()) {
                    return Err("单层塔防模式只能建造采集器与露天跑道".into());
                }
                if self.tech(owner, "") < def.tier {
                    return Err("此露天设施的科技尚未解锁".into());
                }
                if kind == "hydro-power" && !self.near_terrain(pos, 2, 5) {
                    return Err("水电需要河岸".into());
                }
                if kind == "nuclear-power" && !self.near_terrain(pos, 2, 12) {
                    return Err("核电需要冷却水".into());
                }
                if kind == "extractor"
                    && !self
                        .state
                        .resources
                        .iter()
                        .any(|r| matches!(r.kind.as_str(),"ore"|"coal")&&r.remaining>0.&&r.pos.distance(pos) <= 5.)
                {
                    return Err("采集器须靠近矿点".into());
                }
                let cost = def.cost;
                let power = if matches!(
                    kind.as_str(),
                    "wind-power" | "hydro-power" | "coal-power" | "nuclear-power"
                ) {
                    def.power
                } else {
                    0.
                } * if kind == "wind-power" && self.terrain(pos) == 4 {
                    1.5
                } else {
                    1.
                };
                let rect = Rect {
                    x: pos.x,
                    y: pos.y,
                    level: 0,
                    width: def.width as i32,
                    height: def.height as i32,
                };
                if !self.free_ground(rect) {
                    return Err("地形不适合或位置已占用".into());
                }
                self.spend(owner, cost, 0.)?;
                let id = self.id();
                self.state.buildings.push(Building {
                    id,
                    owner,
                    rect,
                    kind: kind.clone(),
                    tier: 1,
                    hp: 70.,
                    antiHeal: 0.,
                    maxHp: 700.,
                    progress: 0.,
                    buildTime: 5.,
                    powered: power > 0.,
                    connected: false,
                    power,
                    demand: if matches!(kind.as_str(), "airstrip" | "launch-pad" | "mobile-relay") {
                        def.power
                    } else {
                        0.
                    },
                    capacity: 0,
                    branch: None,
                    inventory: 0.,
                    invested: cost,
                    jam: 0.,
                    shield: 0.,
                    born: self.state.tick,
                    collapseWarning: 0.,
                    supportRatio: 1.,
                    stock: BTreeMap::new(),
                });
                self.event("build", pos, owner, 0., id);
                self.enqueue_construction(owner, id, rect, "build", 5., cost);
            }
            Command::Wire {
                kind,
                path,
                unit_endpoints,
            } => {
                if !matches!(kind.as_str(), "power" | "compute")
                    || path.len() < 2
                    || path.len() > 512
                    || path.iter().any(|p| !p.valid() || p.level != 0)
                    || path.windows(2).any(|p| {
                        (p[0].x - p[1].x).abs() + (p[0].y - p[1].y).abs() != 1
                            || p[0].level != p[1].level
                    })
                {
                    return Err("线路必须逐格相连".into());
                }
                if unit_endpoints.len() > 2
                    || unit_endpoints
                        .iter()
                        .collect::<std::collections::BTreeSet<_>>()
                        .len()
                        != unit_endpoints.len()
                    || (!unit_endpoints.is_empty() && kind != "compute")
                    || unit_endpoints.iter().any(|id| {
                        self.state.units.iter().find(|u| u.id == *id).is_none_or(|u| {
                            u.owner != owner
                                || u.hp <= 0.
                                || catalog::unit_ref(&u.kind)
                                    .is_none_or(|d| d.category != "ai")
                                || !(path.first() == Some(&u.pos) || path.last() == Some(&u.pos))
                        })
                    })
                {
                    return Err("驻守接入只能绑定线路首尾的己方存活AI".into());
                }
                if path.iter().any(|p| self.terrain(*p) == 2) {
                    return Err("线路经过水域或未挖掘空间".into());
                }
                self.spend(
                    owner,
                    path.len() as f64 * if kind == "power" { 2. } else { 3. },
                    0.,
                )?;
                let id = self.id();
                let invested = path.len() as f64 * if kind == "power" { 2. } else { 3. };
                self.state.links.push(Link {
                    id,
                    owner,
                    kind,
                    path,
                    hp: 150.,
                    active: false,
                    unitEndpoints: unit_endpoints,
                    invested,
                });
            }
            Command::Wall { kind, path } => {
                if path.iter().any(|p| {
                    self.state
                        .entrances
                        .iter()
                        .any(|e| e.kind == "door" && e.hp > 0. && e.covers(*p))
                }) {
                    return Err("先回收门再填墙".into());
                }
                if !matches!(kind.as_str(), "physical" | "cuda" | "moat")
                    || path.is_empty()
                    || path.len() > 256
                    || path.iter().any(|p| !p.valid())
                    || path.windows(2).any(|p| {
                        (p[0].x - p[1].x).abs()
                            + (p[0].y - p[1].y).abs()
                            + (p[0].level - p[1].level).abs()
                            > 1
                    })
                    || path.iter().any(|p| {
                        !p.valid()
                            || p.level == 0 && matches!(self.terrain(*p), 1 | 2)
                            || p.level < 0 && !self.state.excavated.contains(p)
                            || p.level > 0
                                && !self.state.buildings.iter().any(|b| {
                                    b.owner == owner
                                        && b.kind == "shell"
                                        && b.hp > 0.
                                        && b.progress >= 1.
                                        && b.rect.contains(*p)
                                })
                            || self.state.walls.iter().any(|w| {
                                w.pos == *p && w.hp > 0. && (w.owner != owner || w.kind != kind)
                            })
                    })
                {
                    return Err("墙体位置无效".into());
                }
                let mut seen = std::collections::BTreeSet::new();
                let path: Vec<_> = path
                    .into_iter()
                    .filter(|p| {
                        seen.insert(*p)
                            && !self.state.walls.iter().any(|w| {
                                w.pos == *p && w.hp > 0. && w.owner == owner && w.kind == kind
                            })
                    })
                    .collect();
                if path.is_empty() {
                    return Ok(());
                }
                let cost = if kind == "physical" { 10. } else { 18. };
                self.spend(owner, cost * path.len() as f64, 0.)?;
                for pos in path {
                    let id = self.id();
                    self.state.walls.push(Wall {
                        id,
                        owner,
                        pos,
                        kind: kind.clone(),
                        hp: if kind == "physical" { 260. } else { 160. },
                        antiHeal: 0.,
                        maxHp: if kind == "physical" { 260. } else { 160. },
                        shield: 0.,
                        invested: cost,
                    });
                }
            }
            Command::InstallGpu { room, model } => {
                let d = catalog::gpus()
                    .into_iter()
                    .find(|d| d.id == model)
                    .ok_or("未知显卡")?;
                let r = self
                    .state
                    .rooms
                    .iter()
                    .find(|r| {
                        r.id == room
                            && r.owner == owner
                            && r.kind == "data-center"
                            && r.progress >= 1.
                    })
                    .ok_or("选择已完成的数据中心房间")?;
                let usable = r.capacity.min(r.capacity_budget()).min(self.room_capacity(&r.kind, r.rect));
                if r.gpus.len() >= usable as usize || self.tech(owner, "") < d.tier {
                    return Err("机架已满或科技不足".into());
                }
                self.spend(owner, d.cost, 0.)?;
                self.state
                    .rooms
                    .iter_mut()
                    .find(|r| r.id == room)
                    .unwrap()
                    .gpus
                    .push(model);
            }
            Command::Research { room, branch } => {
                // Classic researches at the command core: credits only, no lab,
                // no compute network and no science data.
                let (lab, lab_pos) = if self.classic() {
                    if !catalog::BRANCHES.contains(&branch.as_str()) {
                        return Err("未知科技分支".into());
                    }
                    let core = self.classic_core(owner)?;
                    (core.id, core.rect.center())
                } else {
                    let r = self
                        .state
                        .rooms
                        .iter()
                        .find(|r| {
                            r.id == room
                                && r.owner == owner
                                && r.kind == "research-lab"
                                && r.progress >= 1.
                        })
                        .ok_or("研究所未完成")?;
                    if r.branch.as_deref() != Some(&branch) || !r.powered || !r.connected {
                        return Err("研究所分支不符或尚未接入电力与算力")?;
                    }
                    (r.id, r.rect.center())
                };
                let t = self.tech(owner, &branch);
                if t >= 5 {
                    return Err("此分支已达到T5".into());
                }
                if self
                    .player(owner)
                    .unwrap()
                    .researches
                    .iter()
                    .any(|r| !self.classic() && r.lab == lab || r.branch == branch)
                {
                    return Err("该研究所或分支正在研究".into());
                }
                let multiplier=catalog::research_multiplier(&self.player(owner).unwrap().branches,&branch);
                let data = if self.classic() {
                    0.
                } else {
                    catalog::RESEARCH_DATA[t as usize] * multiplier
                };
                if self.player(owner).unwrap().science < data {
                    return Err("科研数据不足：争夺节点或开启低效本地合成".into());
                }
                self.spend_at(
                    owner,
                    catalog::RESEARCH_CREDITS[t as usize] * multiplier,
                    if self.classic() {
                        0.
                    } else {
                        catalog::RESEARCH_COMPUTE[t as usize]
                    },
                    lab_pos,
                )?;
                self.player_mut(owner).unwrap().science -= data;
                self.player_mut(owner).unwrap().researches.push(Research {
                    lab,
                    branch,
                    target: t + 1,
                    progress: 0.,
                    duration: catalog::RESEARCH_SECONDS[t as usize] * multiplier,
                });
            }
            Command::Deploy { room, kind, pos } => {
                let d = catalog::unit(&kind).ok_or("未知单位")?;
                let classic = self.classic();
                if classic {
                    if d.category == "orbital" {
                        return Err("单层塔防模式不支持天基单位".into());
                    }
                    if self.tech(owner, &d.branch) < d.tier {
                        return Err("对应分支科技等级不足".into());
                    }
                } else if !self.complete_lab(owner) || self.tech(owner, &d.branch) < d.tier {
                    return Err("需要初始研究所及对应分支等级")?;
                }
                if !classic
                    && !d.branch.is_empty()
                    && !self.state.rooms.iter().any(|r| {
                        r.owner == owner
                            && r.kind == "research-lab"
                            && r.branch.as_deref() == Some(d.branch.as_str())
                            && r.hp > 0.
                            && r.progress >= 1.
                            && r.powered
                            && r.connected
                            && r.online
                    })
                {
                    return Err(
                        "对应分支研究所离线：检查供电和维护算力，重建后沿用原科技等级".into(),
                    );
                }
                // The classic ruleset has no producing rooms: any completed owned
                // structure within the deploy radius anchors the placement.
                let (facility_id, facility_rect, facility_powered, facility_shell) = if classic {
                    let anchor = self
                        .classic_deploy_anchor(owner, pos)
                        .ok_or("部署位置须在指挥核心或己方已完工建筑12格内")?;
                    let b = self
                        .state
                        .buildings
                        .iter()
                        .find(|b| b.id == anchor)
                        .ok_or("部署锚点不存在")?;
                    (anchor, b.rect, true, None)
                } else {
                    let facility = self
                        .state
                        .rooms
                        .iter()
                        .find(|r| r.id == room && r.owner == owner && r.progress >= 1. && r.hp > 0.)
                        .ok_or("生产房间不存在")?;
                    if d.category == "ai"
                        && (facility.kind != "research-lab"
                            || facility.branch.as_deref() != Some(d.branch.as_str()))
                        || d.category == "air" && facility.kind != "airfield"
                        || matches!(d.category.as_str(), "vehicle" | "orbital")
                            && !matches!(
                                facility.kind.as_str(),
                                "factory" | "orbital-control" | "missile-silo"
                            )
                    {
                        return Err("该房间不能生产此单位".into());
                    }
                    if d.category == "orbital"
                        && (facility.kind != "orbital-control"
                            || !facility.powered
                            || !facility.connected
                            || !facility.online
                            || !self.state.buildings.iter().any(|b| {
                                b.owner == owner
                                    && b.kind == "launch-pad"
                                    && b.hp > 0.
                                    && b.progress >= 1.
                                    && b.powered
                            }))
                    {
                        return Err("天基单位需要在线轨道控制室和已供电的发射平台".into());
                    }
                    (
                        facility.id,
                        facility.rect,
                        facility.powered,
                        Some(facility.shell),
                    )
                };
                let facility_center = facility_rect.center();
                if !pos.valid()
                    || pos.distance(facility_center) > catalog::CLASSIC_DEPLOY_RADIUS
                    || !self.walkable(pos, &d.category)
                {
                    return Err("部署位置不可达或距离生产建筑过远".into());
                }
                let count = self
                    .state
                    .units
                    .iter()
                    .filter(|u| {
                        u.owner == owner
                            && catalog::unit(&u.kind)
                                .map(|d| d.category == "ai")
                                .unwrap_or(false)
                    })
                    .count();
                let cost = d.cost
                    * if d.category == "ai" {
                        1. + catalog::AI_PURCHASE_GROWTH * (count * (count + 1)) as f64
                    } else {
                        1.
                    };
                let production_pos = facility_center;
                if matches!(d.category.as_str(), "vehicle" | "air" | "orbital" | "ai")
                    && !facility_powered
                {
                    return Err("生产设施没有稳定供电".into());
                }
                let source_facility = facility_id;
                                if d.category == "air"
                    && !self.state.buildings.iter().any(|b| {
                        b.owner == owner
                            && b.kind == "airstrip"
                            && b.progress >= 1.
                            && b.powered
                            && b.rect.center().distance(pos) < 14.
                    })
                {
                    return Err("飞行器需要附近已完成的露天跑道".into());
                }
                let rally = pos;
                let mut vehicle_route = None;
                let spawn = if d.category == "vehicle" && classic {
                    // No factory shell exists: the vehicle occupies its target cell.
                    if !self.vehicle_footprint_clear(pos) {
                        return Err("车辆需要2×2的空闲部署位置".into());
                    }
                    pos
                } else if d.category == "vehicle" {
                    let shell_rect = self
                        .state
                        .buildings
                        .iter()
                        .find(|b| Some(b.id) == facility_shell && b.kind == "shell")
                        .map(|b| b.rect)
                        .unwrap_or(facility_rect);
                    let mut candidates = Vec::new();
                    for y in shell_rect.y - 2..=shell_rect.y + shell_rect.height {
                        for x in shell_rect.x - 2..=shell_rect.x + shell_rect.width {
                            let p = Pos::new(x, y, 0);
                            let on_ring = x == shell_rect.x - 2
                                || x == shell_rect.x + shell_rect.width
                                || y == shell_rect.y - 2
                                || y == shell_rect.y + shell_rect.height
                                || x == shell_rect.x - 1
                                || x == shell_rect.x + shell_rect.width - 1 + 1
                                || y == shell_rect.y - 1
                                || y == shell_rect.y + shell_rect.height - 1 + 1;
                            // Prefer the immediate exterior ring of the solid shell.
                            let exterior = x < shell_rect.x
                                || x + 1 >= shell_rect.x + shell_rect.width
                                || y < shell_rect.y
                                || y + 1 >= shell_rect.y + shell_rect.height;
                            if !exterior || !p.valid() {
                                continue;
                            }
                            let _ = on_ring;
                            if self.vehicle_footprint_clear(p) {
                                candidates.push(p);
                            }
                        }
                    }
                    candidates.sort_by(|a, b| {
                        a.distance(shell_rect.center())
                            .total_cmp(&b.distance(shell_rect.center()))
                            .then_with(|| a.distance(rally).total_cmp(&b.distance(rally)))
                    });
                    let (start, route) = candidates
                        .into_iter()
                        .filter(|p| {
                            !self.state.units.iter().any(|u| {
                                if u.hp <= 0. || u.altitude >= 0.5 || u.level != p.level {
                                    return false;
                                }
                                let span = if catalog::unit_ref(&u.kind)
                                    .is_some_and(|d| d.category == "vehicle")
                                {
                                    2
                                } else {
                                    1
                                };
                                p.x < u.pos.x + span
                                    && p.x + 2 > u.pos.x
                                    && p.y < u.pos.y + span
                                    && p.y + 2 > u.pos.y
                            })
                        })
                        .find_map(|p| self.route(p, rally, "vehicle").map(|route| (p, route)))
                        .ok_or("车厂需要毛坯外沿空闲的2×2出厂位置，以及可达集结点")?;
                    vehicle_route = Some(route);
                    start
                } else if d.category == "air" {
                    self.state
                        .buildings
                        .iter()
                        .filter(|b| {
                            b.owner == owner
                                && b.kind == "airstrip"
                                && b.progress >= 1.
                                && b.powered
                        })
                        .min_by(|a, b| {
                            a.rect
                                .center()
                                .distance(pos)
                                .total_cmp(&b.rect.center().distance(pos))
                        })
                        .map(|b| b.rect.center())
                        .ok_or("没有可用的起飞跑道")?
                } else {
                    pos
                };
                let initial_route = if let Some(route) = vehicle_route {
                    route
                } else if d.category == "air" {
                    self.route(spawn, rally, "air")
                        .ok_or("起飞航线被高楼阻断")?
                } else {
                    vec![]
                };
                if self
                    .state
                    .units
                    .iter()
                    .any(|u| u.hp > 0. && u.pos == spawn && u.altitude < 0.5)
                {
                    return Err("部署格或跑道正被占用".into());
                }
                self.spend_at(
                    owner,
                    cost,
                    if d.category == "ai" && !classic {
                        catalog::AI_DEPLOY_COMPUTE
                    } else {
                        0.
                    },
                    production_pos,
                )?;
                let id = self.id();
                self.state.units.push(Unit {
                    id,
                    owner,
                    kind,
                    pos: spawn,
                    x: spawn.x as f64 + 0.5,
                    y: spawn.y as f64 + 0.5,
                    velocityX:0.,
                    velocityY:0.,
                    level: spawn.level,
                    tier: d.tier,
                    hp: d.hp,
                    maxHp: d.hp,
                    battery: if d.category != "ai" {
                        0.
                    } else if classic {
                        // Nothing drains the cache in classic; start it full so
                        // readiness checks are not permanently short.
                        crate::traits::initial_compute_capacity(&d)
                    } else {
                        120.
                    },
                    batteryMax: crate::traits::initial_compute_capacity(&d),
                    covered: false,
                    wired: false,
                    ammo: d.ammo_capacity,
                    route: initial_route,
                    target: None,
                    cooldown: 0.,
                    skillCooldown: 0.,
                    plugins: vec![],
                    pluginDiscount:0.,
                    chargedShotMultiplier:1.,
                    statuses: BTreeMap::new(),
                    invested: cost,
                    moving: false,
                    attackCount: 0,
                    fuel: d.fuel_capacity,
                    fuelMax: d.fuel_capacity,
                    ammoMax: d.ammo_capacity,
                    branch: d.branch.clone(),
                    facing: 0,
                    energy: d.energy_capacity,
                    energyMax: d.energy_capacity,
                    altitude: 0.,
                    flightState: if d.category == "air" {
                        "taking-off"
                    } else {
                        "ground"
                    }
                    .into(),
                    sourceFacility: source_facility,
                    sortieTarget: None,
                    goal: if matches!(d.category.as_str(), "air" | "vehicle") {
                        Some(rally)
                    } else {
                        None
                    },
                    queuedGoals: vec![],
                    transitProgress: 0.,
                    lastAttackTick: None,
                    lastCastTick: None,
                    lastHitTick: None,
                    dash: None,
                });
                self.event("deploy", spawn, owner, 0., id);
            }
            Command::RemoveGpu { room, bay } => {
                let r = self
                    .state
                    .rooms
                    .iter()
                    .find(|r| {
                        r.id == room && r.owner == owner && r.hp > 0. && r.kind == "data-center"
                    })
                    .ok_or("选择己方数据中心")?;
                let model = r.gpus.get(bay).ok_or("机架没有显卡")?.clone();
                let refund = catalog::gpus()
                    .iter()
                    .find(|g| g.id == model)
                    .map(|g| g.cost * 0.6)
                    .unwrap_or(0.);
                self.state
                    .rooms
                    .iter_mut()
                    .find(|r| r.id == room)
                    .unwrap()
                    .gpus
                    .remove(bay);
                self.player_mut(owner).unwrap().credits += refund;
            }
            Command::QueueMove { ids, pos } => {
                if ids.is_empty()
                    || ids.len() > 128
                    || !pos.valid()
                    || ids.iter().copied().collect::<BTreeSet<_>>().len() != ids.len()
                {
                    return Err("移动队列参数无效".into());
                }
                let mut changes = vec![];
                for id in ids {
                    let u = self
                        .state
                        .units
                        .iter()
                        .find(|u| u.id == id && u.owner == owner && u.hp > 0.)
                        .ok_or("只能操作己方单位")?;
                    let def = catalog::unit_ref(&u.kind).ok_or("单位型号无效")?;
                    if def.speed <= 0. || u.wired || u.queuedGoals.len() >= 16 {
                        return Err("固定/有线单位不能排队，或队列已满16项".into());
                    }
                    let start = u
                        .queuedGoals
                        .last()
                        .copied()
                        .or(u.sortieTarget)
                        .or(u.goal)
                        .or_else(|| u.route.last().copied())
                        .unwrap_or(u.pos);
                    if start == pos {
                        continue;
                    }
                    let route = self
                        .route(start, pos, &def.category)
                        .ok_or("队列下一目的地没有可达路线")?;
                    changes.push((
                        id,
                        route,
                        u.goal.is_none()
                            && u.route.is_empty()
                            && u.sortieTarget.is_none()
                            && u.queuedGoals.is_empty(),
                    ));
                }
                for (id, route, start) in changes {
                    let u = self.state.units.iter_mut().find(|u| u.id == id).unwrap();
                    if start {
                        u.goal = Some(pos);
                        u.route = route;
                        if u.flightState == "landed" {
                            u.flightState = "taking-off".into();
                        }
                    } else {
                        u.queuedGoals.push(pos);
                    }
                }
            }
            Command::Move { ids, pos } => {
                if ids.is_empty() || ids.len() > 128 {
                    return Err("选择单位无效".into());
                }
                let mut routes = Vec::new();
                for id in ids {
                    let u = self
                        .state
                        .units
                        .iter()
                        .find(|u| u.id == id && u.owner == owner)
                        .ok_or("只能操作己方单位")?;
                    let d = catalog::unit(&u.kind).unwrap();
                    if d.speed <= 0. || u.wired {
                        return Err("固定炮塔或有线角色不可移动".into());
                    }
                    routes.push((
                        id,
                        self.route(u.pos, pos, &d.category)
                            .ok_or("找不到跨层可达路径")?,
                    ));
                }
                for (id, path) in routes {
                    let u = self.state.units.iter_mut().find(|u| u.id == id).unwrap();
                    u.dash = None;
                    u.transitProgress = 0.;
                    u.queuedGoals.clear();
                    if catalog::unit_ref(&u.kind).is_some_and(|d| d.category == "air") {
                        if matches!(u.flightState.as_str(), "returning" | "landing")
                            || u.fuel < u.fuelMax * 0.2
                            || u.ammoMax > 0. && u.ammo < u.ammoMax * 0.2
                            || u.energyMax > 0. && u.energy < u.energyMax * 0.2
                        {
                            u.sortieTarget = Some(pos);
                            continue;
                        }
                        if u.flightState == "landed" {
                            u.flightState = "taking-off".into();
                        }
                    }
                    u.route = path;
                    u.goal = Some(pos);
                    u.target = None;
                }
            }
            Command::Attack { ids, target } => {
                if ids.is_empty() || ids.len() > 128 {
                    return Err("选择单位无效".into());
                }
                let (pos, target_owner) =
                    self.target_info_for(target, owner).ok_or("目标不存在")?;
                if target_owner == owner {
                    return Err("不能攻击己方".into());
                }
                for id in &ids {
                    let u = self
                        .state
                        .units
                        .iter()
                        .find(|u| u.id == *id && u.owner == owner)
                        .ok_or("只能操作己方单位")?;
                    if !self.visible_to(owner, pos) {
                        return Err("目标不在可见范围".into());
                    }
                    if u.hp <= 0. {
                        return Err("单位已损毁".into());
                    }
                }
                for id in ids {
                    let u = self.state.units.iter_mut().find(|u| u.id == id).unwrap();
                    u.dash = None;
                    u.queuedGoals.clear();
                    u.target = Some(target);
                    u.transitProgress = 0.;
                }
            }
            Command::Stop { ids } => {
                if ids.is_empty()
                    || ids.len() > 128
                    || ids.iter().any(|id| {
                        !self
                            .state
                            .units
                            .iter()
                            .any(|u| u.id == *id && u.owner == owner)
                    })
                {
                    return Err("只能操作己方单位".into());
                }
                for id in ids {
                    let u = self
                        .state
                        .units
                        .iter_mut()
                        .find(|u| u.id == id && u.owner == owner)
                        .ok_or("只能操作己方单位")?;
                    u.route.clear();
                    u.dash = None;
                    u.sortieTarget = None;
                    u.transitProgress = 0.;
                    u.queuedGoals.clear();
                    u.goal = None;
                    u.target = None;
                }
            }
            Command::Skill { id, pos, direction } => return self.cast(owner, id, pos, direction),
            Command::Plugin { id, plugin } => {
                let meta = catalog::plugin(&plugin).ok_or("未知插件")?;
                let u = self
                    .state
                    .units
                    .iter()
                    .find(|u| u.id == id && u.owner == owner)
                    .ok_or("单位不存在")?;
                let def = catalog::unit(&u.kind).unwrap();
                if def.branch != meta.branch
                    || self.tech(owner, &meta.branch) < meta.tier
                    || u.tier < meta.tier
                    || u.plugins
                        .iter()
                        .filter_map(|p| catalog::plugin(p))
                        .any(|p| p.category == meta.category)
                {
                    return Err("插件分支、类别或等级不兼容".into());
                }
                let index = self.state.units.iter().position(|u| u.id == id).unwrap();
                let discount=self.plugin_discount(&self.state.units[index]);
                self.spend_unit(index, meta.cost*(1.-discount), meta.compute_cost)?;
                let u = self.state.units.iter_mut().find(|u| u.id == id).unwrap();
                u.plugins.push(plugin);
                if meta.modifier == "battery-capacity" {
                    if u.batteryMax>0. {u.batteryMax*=1.+meta.magnitude;}else{u.batteryMax=120.;}
                    if u.energyMax>0. {u.energyMax*=1.+meta.magnitude;}
                }
                if meta.modifier == "armor" {
                    u.maxHp *= 1. + meta.magnitude;
                    u.hp *= 1. + meta.magnitude;
                }
            }
            Command::Upgrade { id } => {
                if let Some(u) = self
                    .state
                    .units
                    .iter()
                    .find(|u| u.id == id && u.owner == owner)
                {
                    let d = catalog::unit(&u.kind).unwrap();
                    let t = u.tier;
                    if t >= 5 || self.tech(owner, &d.branch) <= t {
                        return Err("下一等级科技未解锁".into());
                    }
                    let cost = u.invested * 0.45;
                    let index = self.state.units.iter().position(|u| u.id == id).unwrap();
                    self.spend_unit(index, cost, 60.)?;
                    let u = self.state.units.iter_mut().find(|u| u.id == id).unwrap();
                    u.tier += 1;
                    u.maxHp *= 1.4;
                    u.hp = u.maxHp;
                    u.invested += cost;
                } else {
                    let b = self
                        .state
                        .buildings
                        .iter()
                        .find(|b| b.id == id && b.owner == owner)
                        .ok_or("建筑不存在")?;
                    if b.tier >= 5 || b.tier >= self.tech(owner, "") {
                        return Err("已满级".into());
                    }
                    let cost = b.invested * 0.5;
                    self.spend(owner, cost, 0.)?;
                    let b = self
                        .state
                        .buildings
                        .iter_mut()
                        .find(|b| b.id == id)
                        .unwrap();
                    b.tier += 1;
                    b.maxHp *= 1.3;
                    b.hp = b.maxHp;
                    b.power *= 1.35;
                    b.invested += cost;
                }
            }
            Command::Repair { id } => self.queue_repair(owner, id)?,
            Command::Recycle { id } => {
                let (pos, o) = self.target_info(id).ok_or("目标不存在")?;
                if o != owner
                    || self
                        .state
                        .buildings
                        .iter()
                        .any(|b| b.id == id && b.kind == "core")
                {
                    return Err("无法回收此目标".into());
                }
                if self.state.jobs.iter().any(|j| j.target == id) {
                    return Err("施工中请使用取消任务".into());
                }
                if self.state.shipments.iter().any(|s| s.id == id) {
                    return Err("运输车辆不能兑换金币；完成运送后会自动回收".into());
                }
                let refund = if let Some(b) = self.state.buildings.iter().find(|b| b.id == id) {
                    b.invested * 0.5
                } else if let Some(u) = self.state.units.iter().find(|u| u.id == id) {
                    u.invested * 0.4
                } else if let Some(room) = self.state.rooms.iter().find(|r| r.id == id) {
                    room.invested * 0.5
                        + room
                            .gpus
                            .iter()
                            .filter_map(|model| {
                                catalog::gpus().into_iter().find(|g| &g.id == model)
                            })
                            .map(|g| g.cost * 0.6)
                            .sum::<f64>()
                        + room.stock.values().sum::<f64>() * 0.1
                } else if let Some(w) = self.state.walls.iter().find(|w| w.id == id) {
                    w.invested * 0.5
                } else if let Some(l) = self.state.links.iter().find(|l| l.id == id) {
                    l.invested * 0.5
                } else {
                    0.
                };
                self.player_mut(owner).unwrap().credits += refund;
                if let Some(b) = self.state.buildings.iter_mut().find(|b| b.id == id) {
                    b.invested = 0.;
                }
                if let Some(u) = self.state.units.iter_mut().find(|u| u.id == id) {
                    u.invested = 0.;
                }
                self.damage(id, 1e12, "kinetic", 0);
                self.event("recycle", pos, owner, refund, id);
            }
            Command::Shield { enabled } => {
                self.shield_auto[(owner - 1) as usize] = enabled;
                self.state.shieldAuto = self.shield_auto;
            }
            
            other => {
                self.extra_construction(owner, other)?;
            }
        }
        self.invalidate_navigation();
        self.networks();
        Ok(())
    }
    fn near_terrain(&self, pos: Pos, terrain: u8, radius: i32) -> bool {
        (pos.y - radius..=pos.y + radius).any(|y| {
            (pos.x - radius..=pos.x + radius).any(|x| {
                self.terrain(Pos::new(x, y, 0)) == terrain
                    && Pos::new(x, y, 0).distance(pos) <= radius as f64
            })
        })
    }
}
