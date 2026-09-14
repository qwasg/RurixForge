//! Physical inventory and transport. Credits are awarded only after ore arrives.
use crate::{types::*, Game};
use std::collections::{BTreeMap, BTreeSet, VecDeque};
const DT: f64 = 1. / 60.;
const MAX_SHIPMENTS: usize = 256;
const EXTRACTOR_STOCK_CAPACITY: f64 = 800.;
#[derive(Clone, Copy)]
enum SupplyLocation {
    Building(usize),
    Room(usize),
}
#[derive(Clone, Copy)]
struct SupplySource {
    id: u64,
    pos: Pos,
    location: SupplyLocation,
    reserve: f64,
}
type AutomaticSources = [BTreeMap<&'static str, Vec<SupplySource>>; 2];
struct LogisticsObservation(std::time::Instant);
impl Drop for LogisticsObservation {
    fn drop(&mut self) {
        crate::profiling::record_substage(
            "logistics/total",
            self.0.elapsed().as_secs_f64() * 1000.,
        );
    }
}

impl Game {
    pub(crate) fn extractor_can_sustain_income(building: &Building) -> bool {
        building.stock.values().sum::<f64>() + 1e-7 < EXTRACTOR_STOCK_CAPACITY
            || building.stock.get("ore").copied().unwrap_or(0.)
                > crate::catalog::TRANSPORT_DISPATCH_COST
    }
    fn room_stock_limit(room: &Room) -> f64 {
        let Some(def) = crate::catalog::facility_ref(&room.kind) else {
            return 0.;
        };
        let net_area = if def.capacity_per_area > 0. {
            (room.capacity as f64 / def.capacity_per_area).min(room.rect.area() as f64)
        } else {
            room.rect.area() as f64
        };
        (net_area * def.stock_per_area).floor().max(0.)
    }
    // Manufacturing buffers share the existing physical capacity. These are
    // production ceilings, never export reserves or a reason to discard cargo.
    fn workshop_stock_target(room: &Room, cargo: &str) -> f64 {
        let capacity = Self::room_stock_limit(room);
        let repair = (capacity / 3.).min((capacity / 4.).max(10.));
        let remaining = capacity - repair;
        let fuel = remaining / 3.;
        match cargo {
            "ammo" => remaining - fuel,
            "fuel" => fuel,
            "repair" => repair,
            _ => 0.,
        }
    }
    fn stock_value(&self, id: u64, cargo: &str) -> f64 {
        self.state
            .buildings
            .iter()
            .find(|b| b.id == id && b.hp > 0.)
            .map(|b| b.stock.get(cargo).copied().unwrap_or(0.))
            .or_else(|| {
                self.state
                    .rooms
                    .iter()
                    .find(|r| r.id == id && r.hp > 0.)
                    .map(|r| r.stock.get(cargo).copied().unwrap_or(0.))
            })
            .unwrap_or(0.)
    }
    fn stock_change(&mut self, id: u64, cargo: &str, change: f64) {
        if let Some(b) = self.state.buildings.iter_mut().find(|b| b.id == id) {
            let value = b.stock.entry(cargo.into()).or_default();
            *value = (*value + change).max(0.);
            b.inventory = b.stock.values().sum();
        } else if let Some(r) = self.state.rooms.iter_mut().find(|r| r.id == id) {
            let value = r.stock.entry(cargo.into()).or_default();
            *value = (*value + change).max(0.);
            r.inventory = r.stock.values().sum();
        }
    }
    fn receiving_space(&self, id: u64, cargo: &str) -> f64 {
        let separate_buffers = self.state.units.iter().any(|u| u.id == id && u.hp > 0.);
        let incoming = self
            .state
            .shipments
            .iter()
            .filter(|s| {
                s.to == id
                    && s.hp > 0.
                    && if separate_buffers {
                        s.cargo == cargo
                    } else {
                        s.cargo != "credits"
                    }
            })
            .map(|s| s.amount + if s.mode == "air" { s.fuel } else { 0. })
            .sum::<f64>();
        let space = if let Some(u) = self.state.units.iter().find(|u| u.id == id && u.hp > 0.) {
            match cargo {
                "ammo" => (u.ammoMax - u.ammo).max(0.),
                "fuel" => (u.fuelMax - u.fuel).max(0.),
                "repair" => {
                    (u.maxHp - u.hp).max(0.)
                        / (4. * self.repair_multiplier(id) * self.material_repair_factor(id))
                }
                _ => 0.,
            }
        } else if let Some(b) = self
            .state
            .buildings
            .iter()
            .find(|b| b.id == id && b.hp > 0.)
        {
            if matches!(cargo, "credits" | "ore") && b.kind == "core" {
                f64::INFINITY
            } else {
                let capacity = match b.kind.as_str() {
                    "core" => 2000.,
                    "coal-power" | "nuclear-power" if cargo == "fuel" => 400.,
                    "mobile-relay" if cargo == "fuel" => 80.,
                    "airstrip" if matches!(cargo, "ammo" | "fuel" | "repair" | "ore") => 320.,
                    "launch-pad" if matches!(cargo, "ammo" | "fuel") => 320.,
                    _ => 0.,
                };
                (capacity - b.stock.values().sum::<f64>()).max(0.)
            }
        } else if let Some(r) = self.state.rooms.iter().find(|r| r.id == id && r.hp > 0.) {
            if matches!(cargo, "credits" | "ore") && r.kind == "depot" {
                f64::INFINITY
            } else {
                (Self::room_stock_limit(r) - r.stock.values().sum::<f64>()).max(0.)
            }
        } else {
            0.
        };
        (space - incoming).max(0.)
    }
    fn cargo_route(&self, from: Pos, to: Pos) -> Option<Vec<Pos>> {
        // One-cell delivery couriers use real doors/stairs/lifts; combat vehicles
        // keep their separate width/ramp requirements in ordinary navigation.
        self.route(from, to, "courier")
    }
    fn cargo_points(&self, id: u64, towards: Pos) -> Vec<Pos> {
        let Some((center, _)) = self.target_info(id) else {
            return vec![];
        };
        let mut outdoor = self
            .state
            .buildings
            .iter()
            .find(|b| b.id == id && b.kind != "shell")
            .map(|b| b.rect);
        if let Some(unit) = self.state.units.iter().find(|u| {
            u.id == id
                && crate::catalog::unit_ref(&u.kind)
                    .map(|d| d.category == "air")
                    .unwrap_or(false)
        }) {
            if unit.flightState != "landed" {
                return vec![];
            }
            outdoor = self
                .state
                .buildings
                .iter()
                .find(|b| {
                    b.owner == unit.owner
                        && b.kind == "airstrip"
                        && b.hp > 0.
                        && b.rect.contains(unit.pos)
                })
                .map(|b| b.rect);
            if outdoor.is_none() {
                return vec![];
            }
        }
        let Some(rect) = outdoor else {
            return if self.walkable(center, "courier") {
                vec![center]
            } else {
                vec![]
            };
        };
        let mut rim = BTreeSet::new();
        for y in rect.y - 1..=rect.y + rect.height {
            for x in rect.x - 1..=rect.x + rect.width {
                if x != rect.x - 1
                    && x != rect.x + rect.width
                    && y != rect.y - 1
                    && y != rect.y + rect.height
                {
                    continue;
                }
                let p = Pos::new(x, y, rect.level);
                if self.walkable(p, "courier")
                    && !self
                        .state
                        .buildings
                        .iter()
                        .any(|b| b.hp > 0. && b.rect.contains(p))
                {
                    rim.insert(p);
                }
            }
        }
        // One representative for each connected exterior dock avoids repeatedly
        // searching the same region when a closed wall divides a loading rim.
        let mut docks = vec![];
        while let Some(start) = rim.pop_first() {
            let mut queue = VecDeque::from([start]);
            let mut best = start;
            while let Some(p) = queue.pop_front() {
                if p.distance(towards) < best.distance(towards) {
                    best = p;
                }
                for n in [
                    Pos::new(p.x - 1, p.y, p.level),
                    Pos::new(p.x + 1, p.y, p.level),
                    Pos::new(p.x, p.y - 1, p.level),
                    Pos::new(p.x, p.y + 1, p.level),
                ] {
                    if rim.remove(&n) {
                        queue.push_back(n);
                    }
                }
            }
            docks.push(best);
        }
        docks.sort_by(|a, b| a.distance(towards).total_cmp(&b.distance(towards)));
        docks
    }
    fn cargo_to_target(&self, from: Pos, to: u64) -> Option<Vec<Pos>> {
        self.cargo_points(to, from)
            .into_iter()
            .find_map(|point| self.cargo_route(from, point))
    }
    fn cargo_at_destination(&self, id: u64, pos: Pos) -> bool {
        let Some((center, _)) = self.target_info(id) else {
            return false;
        };
        let mut outdoor = self
            .state
            .buildings
            .iter()
            .find(|b| b.id == id && b.kind != "shell")
            .map(|b| b.rect);
        if let Some(unit) = self.state.units.iter().find(|u| {
            u.id == id
                && crate::catalog::unit_ref(&u.kind)
                    .map(|d| d.category == "air")
                    .unwrap_or(false)
        }) {
            if unit.flightState != "landed" {
                return false;
            }
            outdoor = self
                .state
                .buildings
                .iter()
                .find(|b| {
                    b.owner == unit.owner
                        && b.kind == "airstrip"
                        && b.hp > 0.
                        && b.rect.contains(unit.pos)
                })
                .map(|b| b.rect);
            if outdoor.is_none() {
                return false;
            }
        }
        if let Some(r) = outdoor {
            pos.level == r.level
                && pos.x >= r.x - 1
                && pos.x <= r.x + r.width
                && pos.y >= r.y - 1
                && pos.y <= r.y + r.height
                && !r.contains(pos)
                && self.walkable(pos, "courier")
                && !self
                    .state
                    .buildings
                    .iter()
                    .any(|b| b.hp > 0. && b.rect.contains(pos))
        } else {
            pos == center
        }
    }
    fn air_cell(&self, pos: Pos) -> bool {
        pos.valid()
            && pos.level == 0
            && !self.state.buildings.iter().any(|b| {
                b.hp > 0.
                    && b.rect.level >= 1
                    && b.rect.contains(Pos::new(pos.x, pos.y, b.rect.level))
            })
    }
    fn cargo_airport(&self, id: u64, owner: u32) -> Option<Pos> {
        self.state
            .buildings
            .iter()
            .find(|b| {
                b.id == id
                    && b.owner == owner
                    && b.kind == "airstrip"
                    && b.hp > 0.
                    && b.powered
                    && b.progress >= 1.
                    && b.rect.level == 0
            })
            .map(|b| b.rect.center())
            .filter(|p| self.air_cell(*p))
    }
    fn air_leg(&self, from: Pos, to: Pos) -> Option<Vec<Pos>> {
        if !from.valid() || from.level != 0 || !self.air_cell(to) {
            return None;
        }
        if from == to {
            return Some(vec![]);
        }
        let mut queue = VecDeque::from([from]);
        let mut previous = BTreeMap::from([(from, from)]);
        while let Some(p) = queue.pop_front() {
            let mut neighbors = [
                Pos::new(p.x - 1, p.y, 0),
                Pos::new(p.x + 1, p.y, 0),
                Pos::new(p.x, p.y - 1, 0),
                Pos::new(p.x, p.y + 1, 0),
            ];
            neighbors.sort_by(|a, b| a.distance(to).total_cmp(&b.distance(to)));
            for next in neighbors {
                if !self.air_cell(next) || previous.contains_key(&next) {
                    continue;
                }
                previous.insert(next, p);
                if next == to {
                    let mut route = vec![];
                    let mut current = to;
                    while current != from {
                        route.push(current);
                        current = previous[&current];
                    }
                    route.reverse();
                    return Some(route);
                }
                queue.push_back(next);
            }
        }
        None
    }
    fn cargo_route_length(from: Pos, route: &[Pos]) -> f64 {
        let mut p = from;
        let mut result = 0.;
        for &next in route {
            result += p.distance(next);
            p = next;
        }
        result
    }
    fn air_fuel_needed(shipment: &Shipment, route: &[Pos]) -> f64 {
        let covered = route
            .first()
            .map(|next| shipment.pos.distance(*next) * shipment.progress)
            .unwrap_or(0.);
        let distance = (Self::cargo_route_length(shipment.pos, route) - covered).max(0.);
        distance * crate::catalog::AIR_TRANSPORT_FUEL_PER_CELL
            + crate::catalog::AIR_TRANSPORT_PHASE_SECONDS
            + if shipment.flightState == "taking-off" {
                (crate::catalog::AIR_TRANSPORT_PHASE_SECONDS - shipment.flightTimer).max(0.)
            } else {
                0.
            }
    }
    /// Preserve the edge already in progress. Route changes start at its far
    /// end, so changing orders never resets the rendered/physical position.
    fn shipment_route(&self, shipment: &Shipment, waypoints: &[Pos]) -> Option<Vec<Pos>> {
        let mut route = Vec::new();
        let mut current = shipment.pos;
        if shipment.progress > 1e-7 {
            let next = *shipment.route.first()?;
            if if shipment.mode == "air" {
                !self.air_cell(next)
            } else {
                !self.can_step(current, next, "courier")
            } {
                return None;
            }
            route.push(next);
            current = next;
        }
        for &waypoint in waypoints {
            route.extend(if shipment.mode == "air" {
                self.air_leg(current, waypoint)?
            } else {
                self.cargo_route(current, waypoint)?
            });
            current = waypoint;
            if route.len() > 16_384 {
                return None;
            }
        }
        route.extend(if shipment.mode == "air" {
            self.air_leg(current, self.cargo_airport(shipment.to, shipment.owner)?)?
        } else {
            self.cargo_to_target(current, shipment.to)?
        });
        if route.len() > 16_384 {
            None
        } else {
            Some(route)
        }
    }
    pub fn reroute_shipment(
        &mut self,
        owner: u32,
        id: u64,
        waypoints: Vec<Pos>,
    ) -> Result<(), String> {
        if waypoints.len() > 16
            || waypoints
                .iter()
                .any(|p| !p.valid() || !self.visible_to(owner, *p))
        {
            return Err("运输航点限16个，且必须在已侦察的有效地图内".into());
        }
        let index = self
            .state
            .shipments
            .iter()
            .position(|s| s.id == id && s.owner == owner && s.hp > 0.)
            .ok_or("只能改线己方仍存活的运输载具")?;
        let current = self.state.shipments[index].clone();
        if current.mode == "air"
            && matches!(
                current.flightState.as_str(),
                "landing" | "landed" | "emergency"
            )
        {
            return Err("降落或迫降中的运输机无法改线".into());
        }
        if self
            .target_info(current.to)
            .is_none_or(|(_, target_owner)| target_owner != owner)
        {
            return Err("收货目标已失效".into());
        }
        let manual = !waypoints.is_empty();
        let mut normalized = Vec::new();
        for p in waypoints {
            if normalized.last() == Some(&p)
                || normalized.is_empty() && current.progress <= 1e-7 && p == current.pos
            {
                continue;
            }
            normalized.push(p);
        }
        if current.manualRoute == manual && current.waypoints == normalized {
            return Ok(());
        }
        let route = self
            .shipment_route(&current, &normalized)
            .ok_or("当前路段、航点或最终装卸位置不可通行")?;
        if current.mode == "air" && current.fuel + 1e-7 < Self::air_fuel_needed(&current, &route) {
            return Err("剩余机载燃油不足以飞完航点并安全降落".into());
        }
        self.spend(owner, crate::catalog::TRANSPORT_REROUTE_COST, 0.)?;
        let s = &mut self.state.shipments[index];
        s.route = route;
        s.waypoints = normalized;
        s.manualRoute = manual;
        let count = s.waypoints.len();
        self.event("shipment-reroute", current.pos, owner, count as f64, id);
        self.event(
            "transport-spent",
            current.pos,
            owner,
            crate::catalog::TRANSPORT_REROUTE_COST,
            id,
        );
        Ok(())
    }
    pub fn dispatch_supply(
        &mut self,
        owner: u32,
        from: u64,
        to: u64,
        amount: f64,
        cargo: String,
    ) -> Result<(), String> {
        self.dispatch_supply_mode(owner, from, to, amount, cargo, "ground".into())
    }
    pub fn dispatch_supply_mode(
        &mut self,
        owner: u32,
        from: u64,
        to: u64,
        amount: f64,
        cargo: String,
        mode: String,
    ) -> Result<(), String> {
        if !matches!(mode.as_str(), "ground" | "air") {
            return Err("运输模式无效".into());
        }
        let cargo = if cargo == "credits" {
            "ore".into()
        } else {
            cargo
        };
        if from == to
            || !amount.is_finite()
            || amount < crate::catalog::MIN_SHIPMENT_AMOUNT
            || amount > 1000.
            || !matches!(
                cargo.as_str(),
                "credits" | "ore" | "ammo" | "fuel" | "repair"
            )
        {
            return Err("补给参数无效".into());
        }
        if self.state.shipments.len() >= MAX_SHIPMENTS {
            return Err("运输队列已满，请等待货物送达".into());
        }
        let (_a, o) = self.target_info(from).ok_or("补给起点不存在")?;
        let (b, p) = self.target_info(to).ok_or("补给终点不存在")?;
        if o != owner || p != owner {
            return Err("补给必须连接己方设施或单位".into());
        }
        if self.state.units.iter().any(|u| {
            u.id == to
                && crate::catalog::unit_ref(&u.kind)
                    .map(|d| d.category == "air")
                    .unwrap_or(false)
                && u.flightState != "landed"
        }) {
            return Err("飞行器须降落后才能接收地面补给".into());
        }
        if cargo == "credits"
            && !self
                .state
                .buildings
                .iter()
                .any(|b| b.id == to && b.kind == "core")
            && !self
                .state
                .rooms
                .iter()
                .any(|r| r.id == to && r.kind == "depot")
        {
            return Err("矿物必须送到核心或仓库".into());
        }
        let key = if cargo == "credits" {
            "ore"
        } else {
            cargo.as_str()
        };
        if self.stock_value(from, key) + 0.0001 < amount {
            return Err("起点没有足量实体库存".into());
        }
        if self.receiving_space(to, &cargo) + 0.0001 < amount {
            return Err("收货方容量不足或已被在途货物预留".into());
        }
        let (origin, route, fuel, fee) = if mode == "air" {
            let origin = self
                .cargo_airport(from, owner)
                .ok_or("空运起点须是己方已完工、供电且无高层遮挡的地面机场跑道")?;
            let destination = self
                .cargo_airport(to, owner)
                .ok_or("空运终点须是己方已完工、供电且无高层遮挡的地面机场跑道")?;
            let route = self
                .air_leg(origin, destination)
                .ok_or("机场间没有安全飞行走廊")?;
            let needed = Self::cargo_route_length(origin, &route)
                * crate::catalog::AIR_TRANSPORT_FUEL_PER_CELL
                + 2. * crate::catalog::AIR_TRANSPORT_PHASE_SECONDS;
            let fuel = needed + 4.;
            if fuel > crate::catalog::AIR_TRANSPORT_FUEL_MAX {
                return Err("空运航程超过油箱与安全余量上限".into());
            }
            if self.stock_value(from, "fuel") + 1e-7
                < fuel + if key == "fuel" { amount } else { 0. }
            {
                return Err("起点燃油不足以同时装货并供应本次真实航程".into());
            }
            if self.receiving_space(to, &cargo) + 1e-7 < amount + fuel {
                return Err("终点没有足够容量预留货物及返回的机载燃油".into());
            }
            (
                origin,
                route,
                fuel,
                crate::catalog::AIR_TRANSPORT_DISPATCH_COST,
            )
        } else {
            let (origin, route) = self
                .cargo_points(from, b)
                .into_iter()
                .find_map(|origin| {
                    self.cargo_to_target(origin, to)
                        .map(|route| (origin, route))
                })
                .ok_or("没有可通行的装卸与补给路线")?;
            (origin, route, 0., crate::catalog::TRANSPORT_DISPATCH_COST)
        };
        self.spend(owner, fee, 0.)?;
        self.stock_change(from, key, -amount);
        if fuel > 0. {
            self.stock_change(from, "fuel", -fuel);
        }
        let id = self.id();
        self.state.shipments.push(Shipment {
            unloadProgress: 0.,
            mode: mode.clone(),
            altitude: 0.,
            flightState: if mode == "air" {
                "taking-off"
            } else {
                "ground"
            }
            .into(),
            flightTimer: 0.,
            fuel,
            fuelMax: if mode == "air" {
                crate::catalog::AIR_TRANSPORT_FUEL_MAX
            } else {
                0.
            },
            waypoints: vec![],
            manualRoute: false,
            id,
            owner,
            from,
            to,
            pos: origin,
            route,
            amount,
            hp: if mode == "air" {
                260.
            } else {
                60. + 8. * amount.sqrt()
            },
            progress: 0.,
            cargo,
        });
        self.event("shipment-dispatch", origin, owner, amount, id);
        self.event("transport-spent", origin, owner, fee, id);
        Ok(())
    }
    fn spill_cargo(&mut self, pos: Pos, cargo: &str, amount: f64, owner: u32, recovery: f64) {
        let cargo = if cargo == "credits" { "ore" } else { cargo };
        let amount = amount * recovery;
        if amount <= 0.0001 {
            return;
        }
        if let Some(pile) = self
            .state
            .resources
            .iter_mut()
            .find(|r| r.pos == pos && r.kind == format!("salvage-{cargo}"))
        {
            pile.remaining += amount;
        } else {
            let id = self.id();
            self.state.resources.push(Resource {
                id,
                pos,
                kind: format!("salvage-{cargo}"),
                remaining: amount,
                owner: 0,
                capture: 0.,
                capturer: 0,
                contested: false,
            });
        }
        self.event("cargo-wreck", pos, owner, amount, 0);
    }
    fn finish_delivery(&mut self, shipment: Shipment) {
        let mut remainder = shipment.amount;
        let mut settled_ore = 0.;
        let mut delivered_to_unit = 0.;
        let repair_factor =
            self.repair_multiplier(shipment.to) * self.material_repair_factor(shipment.to);
        let target = self.target_info(shipment.to);
        if target.map(|(_, o)| o) == Some(shipment.owner) {
            if matches!(shipment.cargo.as_str(), "credits" | "ore")
                && self
                    .state
                    .buildings
                    .iter()
                    .any(|b| b.id == shipment.to && b.kind == "core")
                || matches!(shipment.cargo.as_str(), "credits" | "ore")
                    && self
                        .state
                        .rooms
                        .iter()
                        .any(|r| r.id == shipment.to && r.kind == "depot")
            {
                self.player_mut(shipment.owner).unwrap().credits += shipment.amount;
                settled_ore = shipment.amount;
                remainder = 0.;
            } else if let Some(unit) = self.state.units.iter_mut().find(|u| u.id == shipment.to) {
                let before = remainder;
                match shipment.cargo.as_str() {
                    "ammo" => {
                        let amount = remainder.min((unit.ammoMax - unit.ammo).max(0.));
                        unit.ammo += amount;
                        remainder -= amount;
                    }
                    "fuel" => {
                        let amount = remainder.min((unit.fuelMax - unit.fuel).max(0.));
                        unit.fuel += amount;
                        remainder -= amount;
                    }
                    "repair" => {
                        let amount =
                            remainder.min((unit.maxHp - unit.hp).max(0.) / (4. * repair_factor));
                        unit.hp += amount * 4. * repair_factor;
                        remainder -= amount;
                    }
                    _ => {}
                }
                delivered_to_unit = before - remainder;
            } else {
                let amount = remainder.min(self.receiving_space(shipment.to, &shipment.cargo));
                if amount > 0. {
                    self.stock_change(shipment.to, &shipment.cargo, amount);
                    remainder -= amount;
                }
            }
        }
        if delivered_to_unit > 0. {
            let key = match shipment.cargo.as_str() {
                "ammo" => Some("unit-ammo-delivered"),
                "fuel" => Some("unit-fuel-delivered"),
                "repair" => Some("unit-repair-material-delivered"),
                _ => None,
            };
            if let (Some(key), Some(player)) = (key, self.player_mut(shipment.owner)) {
                if let Some(total) = player.totals.get_mut(key) {
                    *total += delivered_to_unit;
                } else {
                    player.totals.insert(key.into(), delivered_to_unit);
                }
            }
        }
        self.event(
            "shipment-arrive",
            shipment.pos,
            shipment.owner,
            shipment.amount - remainder,
            shipment.to,
        );
        if settled_ore > 0. {
            self.event(
                "ore-delivered",
                shipment.pos,
                shipment.owner,
                settled_ore,
                shipment.to,
            );
        }
        if remainder > 0.0001 {
            self.spill_cargo(shipment.pos, &shipment.cargo, remainder, shipment.owner, 1.);
        }
        if shipment.mode == "air" && shipment.fuel > 0. {
            let returned = if self
                .target_info(shipment.to)
                .is_some_and(|(_, owner)| owner == shipment.owner)
            {
                shipment.fuel.min(self.receiving_space(shipment.to, "fuel"))
            } else {
                0.
            };
            if returned > 0. {
                self.stock_change(shipment.to, "fuel", returned);
            }
            if shipment.fuel > returned {
                self.spill_cargo(
                    shipment.pos,
                    "fuel",
                    shipment.fuel - returned,
                    shipment.owner,
                    1.,
                );
            }
        }
    }
    fn advance_unloading(&mut self, index: usize) -> bool {
        let to = self.state.shipments[index].to;
        let rate = self.loading_rate_for_entity(to);
        let shipment = &mut self.state.shipments[index];
        let duration = shipment.amount / 40.;
        if duration <= 0. {
            shipment.unloadProgress = 1.;
        } else {
            shipment.unloadProgress = (shipment.unloadProgress + DT * rate / duration).min(1.);
        }
        shipment.unloadProgress + 1e-7 >= 1.
    }
    fn burn_transport_fuel(&mut self, index: usize, amount: f64) {
        let s = &mut self.state.shipments[index];
        let spent = amount.min(s.fuel).max(0.);
        let owner = s.owner;
        s.fuel -= spent;
        if let Some(player) = self.player_mut(owner) {
            *player
                .totals
                .entry("transport-fuel-spent".into())
                .or_default() += spent;
        }
    }
    fn transport_destroy_event(&mut self, shipment: &Shipment) {
        let next = shipment.route.first().copied().unwrap_or(shipment.pos);
        let dx = (next.x - shipment.pos.x) as f64;
        let dy = (next.y - shipment.pos.y) as f64;
        let progress = shipment.progress.clamp(0., 1.);
        self.event(
            "transport-destroy",
            shipment.pos,
            shipment.owner,
            0.,
            shipment.id,
        );
        if let Some(event) = self.state.events.last_mut() {
            event.subjectKind = if shipment.mode == "air" {
                "cargo-aircraft"
            } else {
                "cargo-truck"
            }
            .into();
            event.presentationPosition = Some([
                shipment.pos.x as f64 + 0.5 + dx * progress,
                shipment.pos.y as f64 + 0.5 + dy * progress,
                shipment.pos.level as f64
                    + (next.level - shipment.pos.level) as f64 * progress
                    + shipment.altitude
                    + 0.1,
            ]);
            event.facing = Some(
                (((dy.atan2(dx) - std::f64::consts::FRAC_PI_4) / std::f64::consts::FRAC_PI_4)
                    .round() as i32)
                    .rem_euclid(8) as u32,
            );
        }
    }
    fn crash_air_shipment(&mut self, index: usize) {
        let lost = self.state.shipments.remove(index);
        self.transport_destroy_event(&lost);
        self.spill_cargo(lost.pos, &lost.cargo, lost.amount, lost.owner, 0.6);
        self.spill_cargo(lost.pos, "fuel", lost.fuel, lost.owner, 0.6);
        self.event(
            "cargo-aircraft-destroyed",
            lost.pos,
            lost.owner,
            lost.amount,
            lost.id,
        );
    }
    fn divert_air_shipment(&mut self, index: usize) -> bool {
        let current = self.state.shipments[index].clone();
        let mut airports: Vec<_> = self
            .state
            .buildings
            .iter()
            .filter_map(|b| self.cargo_airport(b.id, current.owner).map(|p| (b.id, p)))
            .filter(|(id, _)| {
                *id != current.to
                    && self.receiving_space(*id, &current.cargo) + 1e-7
                        >= current.amount + current.fuel
            })
            .collect();
        airports.sort_by(|a, b| {
            a.1.distance(current.pos)
                .total_cmp(&b.1.distance(current.pos))
                .then(a.0.cmp(&b.0))
        });
        for (destination, _) in airports {
            let mut alternative = current.clone();
            alternative.to = destination;
            alternative.flightState = "cruising".into();
            if let Some(route) = self.shipment_route(&alternative, &[]) {
                if Self::air_fuel_needed(&alternative, &route) <= current.fuel + 1e-7 {
                    let s = &mut self.state.shipments[index];
                    s.to = destination;
                    s.route = route;
                    s.waypoints.clear();
                    s.manualRoute = false;
                    s.unloadProgress = 0.;
                    s.flightState = "cruising".into();
                    s.flightTimer = 0.;
                    self.event(
                        "cargo-aircraft-divert",
                        current.pos,
                        current.owner,
                        destination as f64,
                        current.id,
                    );
                    return true;
                }
            }
        }
        false
    }
    /// Returns true only when the air carrier was removed from the live vector.
    fn advance_air_shipment(&mut self, index: usize) -> bool {
        let current = self.state.shipments[index].clone();
        if current.hp <= 0. {
            self.crash_air_shipment(index);
            return true;
        }
        if current.flightState == "landed"
            && self.cargo_airport(current.to, current.owner) == Some(current.pos)
        {
            if self.advance_unloading(index) {
                let delivered = self.state.shipments.remove(index);
                self.finish_delivery(delivered);
                return true;
            }
            return false;
        }
        if current.flightState == "emergency" {
            let s = &mut self.state.shipments[index];
            s.flightTimer += DT;
            s.altitude = (s.altitude - DT).max(0.);
            if s.altitude <= 1e-7 {
                self.crash_air_shipment(index);
                return true;
            }
            return false;
        }
        if current.fuel <= 1e-7 {
            let s = &mut self.state.shipments[index];
            s.flightState = "emergency".into();
            s.flightTimer = 0.;
            return false;
        }
        if self.cargo_airport(current.to, current.owner).is_none() {
            if !self.divert_air_shipment(index) {
                let s = &mut self.state.shipments[index];
                s.flightState = "emergency".into();
                s.flightTimer = 0.;
            }
            return false;
        }
        if current.flightState == "taking-off" {
            if self.cargo_airport(current.from, current.owner).is_none()
                && current.flightTimer <= 1e-7
            {
                return false;
            }
            let delta =
                DT.min((crate::catalog::AIR_TRANSPORT_PHASE_SECONDS - current.flightTimer).max(0.));
            if current.fuel + 1e-7 < delta {
                self.state.shipments[index].flightState = "emergency".into();
                return false;
            }
            self.burn_transport_fuel(index, delta);
            let s = &mut self.state.shipments[index];
            s.flightTimer += delta;
            s.altitude = (s.flightTimer / crate::catalog::AIR_TRANSPORT_PHASE_SECONDS * 2.).min(2.);
            if s.flightTimer + 1e-7 >= crate::catalog::AIR_TRANSPORT_PHASE_SECONDS {
                s.flightState = "cruising".into();
                s.flightTimer = 0.;
            }
            return false;
        }
        if current.flightState == "landing" {
            let delta =
                DT.min((crate::catalog::AIR_TRANSPORT_PHASE_SECONDS - current.flightTimer).max(0.));
            if current.fuel + 1e-7 < delta {
                self.state.shipments[index].flightState = "emergency".into();
                return false;
            }
            self.burn_transport_fuel(index, delta);
            let s = &mut self.state.shipments[index];
            s.flightTimer += delta;
            s.altitude =
                (2. * (1. - s.flightTimer / crate::catalog::AIR_TRANSPORT_PHASE_SECONDS)).max(0.);
            if s.flightTimer + 1e-7 >= crate::catalog::AIR_TRANSPORT_PHASE_SECONDS {
                s.flightState = "landed".into();
                s.altitude = 0.;
            }
            return false;
        }
        if current.flightState != "cruising" {
            self.state.shipments[index].flightState = "emergency".into();
            return false;
        }
        if current.progress <= 1e-7 {
            let s = &mut self.state.shipments[index];
            while s.waypoints.first() == Some(&s.pos) {
                s.waypoints.remove(0);
            }
        }
        let current = self.state.shipments[index].clone();
        if current.waypoints.is_empty()
            && current.progress <= 1e-7
            && self.cargo_airport(current.to, current.owner) == Some(current.pos)
        {
            self.state.shipments[index].flightState = "landing".into();
            self.state.shipments[index].flightTimer = 0.;
            return false;
        }
        let blocked = current
            .route
            .first()
            .is_none_or(|next| !self.air_cell(*next));
        if blocked && (self.state.tick + current.id) % 30 == 0 {
            if let Some(route) = self.shipment_route(&current, &current.waypoints) {
                if Self::air_fuel_needed(&current, &route) <= current.fuel + 1e-7 {
                    self.state.shipments[index].route = route;
                }
            }
        }
        let mut travel_budget = crate::catalog::AIR_TRANSPORT_SPEED * DT;
        let mut moved = false;
        while travel_budget > 1e-7 {
            let s = &self.state.shipments[index];
            let Some(next) = s.route.first().copied().filter(|p| self.air_cell(*p)) else {
                break;
            };
            let distance = s.pos.distance(next).max(1e-7);
            let travelled = travel_budget.min(distance * (1. - s.progress));
            let fraction = travelled / distance;
            let cost = travelled * crate::catalog::AIR_TRANSPORT_FUEL_PER_CELL;
            if cost > s.fuel + 1e-7 {
                self.state.shipments[index].flightState = "emergency".into();
                return false;
            }
            self.burn_transport_fuel(index, cost);
            let s = &mut self.state.shipments[index];
            s.progress += fraction;
            travel_budget -= travelled;
            moved = true;
            if s.progress + 1e-7 >= 1. {
                s.pos = next;
                s.route.remove(0);
                s.progress = 0.;
                while s.waypoints.first() == Some(&s.pos) {
                    s.waypoints.remove(0);
                }
            }
        }
        self.state.shipments[index].altitude = (self.state.shipments[index].altitude + DT).min(2.);
        if !moved {
            // A blocked airborne carrier burns real holding fuel; its remaining
            // manual waypoints are not silently replaced with a shortcut.
            self.burn_transport_fuel(index, DT * 0.06);
        }
        false
    }
    pub(crate) fn advance_shipments(&mut self) {
        let mut index = 0;
        while index < self.state.shipments.len() {
            if self.state.shipments[index].mode == "air" {
                if !self.advance_air_shipment(index) {
                    index += 1;
                }
                continue;
            }
            // Reached waypoints are removed only at an actual cell arrival.
            if self.state.shipments[index].progress <= 1e-7 {
                let s = &mut self.state.shipments[index];
                while s.waypoints.first() == Some(&s.pos) {
                    s.waypoints.remove(0);
                }
            }
            let current = self.state.shipments[index].clone();
            if current.hp <= 0. {
                let lost = self.state.shipments.remove(index);
                self.transport_destroy_event(&lost);
                self.spill_cargo(lost.pos, &lost.cargo, lost.amount, lost.owner, 0.6);
                continue;
            }
            if self
                .target_info(current.to)
                .is_none_or(|(_, owner)| owner != current.owner)
            {
                let lost = self.state.shipments.remove(index);
                self.spill_cargo(lost.pos, &lost.cargo, lost.amount, lost.owner, 0.8);
                continue;
            }
            if self.state.units.iter().any(|u| {
                u.id == current.to
                    && crate::catalog::unit_ref(&u.kind).is_some_and(|d| d.category == "air")
                    && u.flightState != "landed"
            }) {
                // Waiting for a real landing must not rewind a half-traversed edge.
                index += 1;
                continue;
            }
            if current.waypoints.is_empty()
                && current.progress <= 1e-7
                && self.cargo_at_destination(current.to, current.pos)
            {
                if self.advance_unloading(index) {
                    let delivered = self.state.shipments.remove(index);
                    self.finish_delivery(delivered);
                } else {
                    index += 1;
                }
                continue;
            }
            let blocked = current
                .route
                .first()
                .is_none_or(|p| !self.can_step(current.pos, *p, "courier"));
            let update = (self.state.tick + current.id) % 30 == 0;
            let moving_target = current
                .route
                .last()
                .is_none_or(|p| !self.cargo_at_destination(current.to, *p));
            if update && (blocked || moving_target) {
                if let Some(route) = self.shipment_route(&current, &current.waypoints) {
                    self.state.shipments[index].route = route;
                } else if current.progress <= 1e-7 {
                    self.state.shipments[index].route.clear();
                }
                // If blocked midway, preserve both the edge and its progress.
                // Clearing either would teleport the courier back to its anchor.
            }
            let mut arrived = false;
            // Progress is a fraction of this edge. Pos::distance weights a
            // vertical floor as four cells; carry unused *distance*, not a
            // fraction of the old edge, when the next edge has another length.
            let mut travel_budget = DT * 3.;
            while travel_budget > 1e-7 {
                let s = &self.state.shipments[index];
                let Some(next) = s.route.first().copied() else {
                    break;
                };
                if !self.can_step(s.pos, next, "courier") {
                    break;
                }
                let distance = s.pos.distance(next).max(1.);
                let travelled = travel_budget.min(distance * (1. - s.progress));
                let s = &mut self.state.shipments[index];
                s.progress += travelled / distance;
                travel_budget -= travelled;
                if s.progress + 1e-7 >= 1. {
                    s.pos = next;
                    s.route.remove(0);
                    s.progress = 0.;
                    arrived = true;
                    while s.waypoints.first() == Some(&s.pos) {
                        s.waypoints.remove(0);
                    }
                }
            }
            let s = &self.state.shipments[index];
            if arrived && s.waypoints.is_empty() && self.cargo_at_destination(s.to, s.pos) {
                // Movement already used this tick's time; unload on the next tick.
                index += 1;
                continue;
            }
            index += 1;
        }
    }
    fn nearest_receiver(&self, owner: u32, from: Pos, source_id: Option<u64>) -> Option<u64> {
        self.state
            .buildings
            .iter()
            .filter(|b| b.owner == owner && b.kind == "core" && b.hp > 0.)
            .map(|b| (b.id, b.rect.center()))
            .chain(
                self.state
                    .rooms
                    .iter()
                    .filter(|r| {
                        r.owner == owner && r.kind == "depot" && r.progress >= 1. && r.hp > 0.
                    })
                    .map(|r| (r.id, r.rect.center())),
            )
            .filter(|(id, pos)| {
                source_id
                    .map(|source| self.cargo_points(source, *pos))
                    .unwrap_or_else(|| vec![from])
                    .into_iter()
                    .any(|origin| self.cargo_to_target(origin, *id).is_some())
            })
            .min_by(|a, b| a.1.distance(from).total_cmp(&b.1.distance(from)))
            .map(|(id, _)| id)
    }
    fn automatic_stock_target(&self, id: u64, cargo: &str) -> f64 {
        if let Some(room) = self.state.rooms.iter().find(|r| r.id == id) {
            let base = match (room.kind.as_str(), cargo) {
                ("repair-bay", "repair") => 50.,
                ("factory" | "airfield" | "orbital-control", "ammo") => 40.,
                ("factory" | "airfield" | "orbital-control", "fuel") => 120.,
                _ => 0.,
            };
            return base * room.equipmentShare.max(0.);
        }
        self.state
            .buildings
            .iter()
            .find(|b| b.id == id)
            .map(|b| match (b.kind.as_str(), cargo) {
                ("airstrip", "ammo") => 80.,
                ("airstrip", "fuel") => 200.,
                ("coal-power" | "nuclear-power", "fuel") => 80.,
                _ => 0.,
            })
            .unwrap_or(0.)
    }
    fn automatic_sources(&self) -> AutomaticSources {
        let mut result: AutomaticSources = std::array::from_fn(|_| BTreeMap::new());
        for (index, b) in self
            .state
            .buildings
            .iter()
            .enumerate()
            .filter(|(_, b)| (1..=2).contains(&b.owner) && b.hp > 0.)
        {
            for cargo in ["ammo", "fuel", "repair"] {
                if b.stock.get(cargo).copied().unwrap_or(0.) >= 1. {
                    result[(b.owner - 1) as usize]
                        .entry(cargo)
                        .or_default()
                        .push(SupplySource {
                            id: b.id,
                            pos: b.rect.center(),
                            location: SupplyLocation::Building(index),
                            reserve: self.automatic_stock_target(b.id, cargo),
                        });
                }
            }
        }
        for (index, r) in self
            .state
            .rooms
            .iter()
            .enumerate()
            .filter(|(_, r)| (1..=2).contains(&r.owner) && r.hp > 0. && r.progress >= 1.)
        {
            for cargo in ["ammo", "fuel", "repair"] {
                if r.stock.get(cargo).copied().unwrap_or(0.) >= 1. {
                    result[(r.owner - 1) as usize]
                        .entry(cargo)
                        .or_default()
                        .push(SupplySource {
                            id: r.id,
                            pos: r.rect.center(),
                            location: SupplyLocation::Room(index),
                            reserve: self.automatic_stock_target(r.id, cargo),
                        });
                }
            }
        }
        result
    }
    fn automatic_exportable(&self, source: &SupplySource, to_unit: bool, cargo: &str) -> f64 {
        // Service buffers are for combat units. Other facilities may borrow only
        // genuine surplus, or two nearby consumers endlessly steal and replace
        // each other's operating stock while paying a dispatch fee every trip.
        let current = match source.location {
            SupplyLocation::Building(i) => self
                .state
                .buildings
                .get(i)
                .filter(|b| b.id == source.id && b.hp > 0.)
                .and_then(|b| b.stock.get(cargo)),
            SupplyLocation::Room(i) => self
                .state
                .rooms
                .get(i)
                .filter(|r| r.id == source.id && r.hp > 0.)
                .and_then(|r| r.stock.get(cargo)),
        };
        (current.copied().unwrap_or(0.) - if to_unit { 0. } else { source.reserve }).max(0.)
    }
    fn send_available(
        &mut self,
        bank: &AutomaticSources,
        owner: u32,
        to: u64,
        cargo: &str,
        wanted: f64,
    ) {
        if !(1..=2).contains(&owner)
            || wanted <= 0.
            || self
                .state
                .shipments
                .iter()
                .any(|s| s.owner == owner && s.to == to && s.cargo == cargo && s.hp > 0.)
        {
            return;
        }
        let Some(candidates) = bank[(owner - 1) as usize].get(cargo) else {
            return;
        };
        let Some((destination, _)) = self.target_info(to) else {
            return;
        };
        let to_unit = self.state.units.iter().any(|u| u.id == to && u.hp > 0.);
        let mut sources: Vec<_> = candidates
            .iter()
            .copied()
            .filter(|s| s.id != to && self.automatic_exportable(s, to_unit, cargo) >= 1.)
            .collect();
        sources.sort_by(|a, b| {
            a.pos
                .distance(destination)
                .total_cmp(&b.pos.distance(destination))
        });
        for source in sources {
            let amount = wanted
                .min(self.automatic_exportable(&source, to_unit, cargo))
                .min(self.receiving_space(to, cargo))
                .min(100.);
            if amount >= 1.
                && self
                    .dispatch_supply(owner, source.id, to, amount, cargo.into())
                    .is_ok()
            {
                break;
            }
        }
    }
    pub(crate) fn logistics_second(&mut self) {
        let _observation = LogisticsObservation(std::time::Instant::now());
        let mut stage = std::time::Instant::now();
        // Mining depletes finite world resources into the extractor's actual stock.
        let extractors: Vec<_> = self
            .state
            .buildings
            .iter()
            .filter(|b| b.kind == "extractor" && b.progress >= 1. && b.hp > 0.)
            .cloned()
            .collect();
        for b in extractors {
            let active_source = self.state.resources.iter().position(|r| {
                matches!(r.kind.as_str(), "ore" | "coal")
                    && r.remaining > 0.
                    && r.pos.distance(b.rect.center()) < 8.
            });
            if let Some(i) = active_source {
                let home = self
                    .state
                    .buildings
                    .iter()
                    .find(|c| c.owner == b.owner && c.kind == "core")
                    .map(|c| c.rect.center())
                    .unwrap_or(b.rect.center());
                let rate: f64 = if home.distance(b.rect.center()) > 28. {
                    8.
                } else {
                    5.
                };
                let free = (EXTRACTOR_STOCK_CAPACITY - b.stock.values().sum::<f64>()).max(0.);
                let amount = rate.min(self.state.resources[i].remaining).min(free);
                self.state.resources[i].remaining -= amount;
                self.state.resources[i].owner = b.owner;
                // Coal splits into saleable mineral and station fuel, without creating extra mass.
                if self.state.resources[i].kind == "coal" {
                    self.stock_change(b.id, "ore", amount * 0.7);
                    self.stock_change(b.id, "fuel", amount * 0.3);
                } else {
                    self.stock_change(b.id, "ore", amount);
                }
            }
            let stored = self.stock_value(b.id, "ore");
            let total = self
                .state
                .buildings
                .iter()
                .find(|current| current.id == b.id)
                .map(|current| current.stock.values().sum::<f64>())
                .unwrap_or(0.);
            // Mixed coal stock can fill the shared warehouse with 765 fuel +
            // 35 ore. Waiting for the normal 40-unit batch would then prevent
            // both further mining and any dispatch. Flush a profitable real
            // tail batch when full or exhausted; never discard fuel or pay
            // more in automatic courier fees than that tail is worth.
            let tail_batch = stored > crate::catalog::TRANSPORT_DISPATCH_COST
                && (total + 1e-7 >= EXTRACTOR_STOCK_CAPACITY || active_source.is_none());
            let inflight = self
                .state
                .shipments
                .iter()
                .filter(|s| s.from == b.id && s.hp > 0.)
                .count();
            if (stored >= 40. || tail_batch) && inflight < 3 {
                if let Some(to) = self.nearest_receiver(b.owner, b.rect.center(), Some(b.id)) {
                    let _ =
                        self.dispatch_supply(b.owner, b.id, to, stored.min(80.), "credits".into());
                }
            }
        }
        crate::profiling::record_substage(
            "logistics/mining",
            stage.elapsed().as_secs_f64() * 1000.,
        );
        stage = std::time::Instant::now();
        // Workshops buy and manufacture supplies. Depots only store them.
        let workshops: Vec<_> = self
            .state
            .rooms
            .iter()
            .filter(|r| {
                r.kind == "ammunition-workshop" && r.hp > 0. && r.progress >= 1. && r.powered
            })
            .cloned()
            .collect();
        for r in workshops {
            let share = r.equipmentShare.max(0.);
            if share <= 0. {
                continue;
            }
            let cargo = match (self.state.tick / 60 + r.id) % 6 {
                0 => "fuel",
                1 => "repair",
                _ => "ammo",
            };
            let missing = (Self::workshop_stock_target(&r, cargo)
                - self.stock_value(r.id, cargo)).max(0.);
            let amount = (4. * share)
                .min(self.receiving_space(r.id, cargo))
                .min(missing);
            // A paid positive tail can complete repair9.x to10 even when less
            // than one equipment share remains; no unearned material is added.
            if amount <= 0. {
                continue;
            }
            let unit_cost = match cargo {
                "ammo" => 0.35,
                "fuel" => 0.2,
                _ => 0.7,
            };
            if self.spend(r.owner, amount * unit_cost, 0.).is_ok() {
                self.stock_change(r.id, cargo, amount);
            }
        }
        crate::profiling::record_substage(
            "logistics/manufacture",
            stage.elapsed().as_secs_f64() * 1000.,
        );
        stage = std::time::Instant::now();
        // Stable source locations for this economy step. Dispatch changes stock,
        // not the room/building vectors; live amounts are still checked per use.
        let source_bank = self.automatic_sources();
        crate::profiling::record_substage(
            "logistics/source-index",
            stage.elapsed().as_secs_f64() * 1000.,
        );
        stage = std::time::Instant::now();
        let power_stations: Vec<_> = self
            .state
            .buildings
            .iter()
            .filter(|b| {
                matches!(b.kind.as_str(), "coal-power" | "nuclear-power")
                    && b.hp > 0.
                    && b.progress >= 1.
            })
            .cloned()
            .collect();
        for b in power_stations {
            let stock = self.stock_value(b.id, "fuel");
            if b.powered && stock > 0. {
                self.stock_change(b.id, "fuel", -stock.min(0.25));
            }
            if stock < 25. {
                self.send_available(&source_bank, b.owner, b.id, "fuel", 80. - stock);
            }
        }
        let consumers: Vec<_> = self
            .state
            .units
            .iter()
            .filter(|u| u.hp > 0.)
            .cloned()
            .collect();
        for u in consumers {
            if crate::catalog::unit_ref(&u.kind)
                .map(|d| d.category == "air")
                .unwrap_or(false)
                && u.flightState != "landed"
            {
                continue;
            }
            if u.ammoMax > 0. && u.ammo < u.ammoMax * 0.35 {
                self.send_available(&source_bank, u.owner, u.id, "ammo", u.ammoMax - u.ammo);
            }
            if u.fuelMax > 0. && u.fuel < u.fuelMax * 0.35 {
                self.send_available(&source_bank, u.owner, u.id, "fuel", u.fuelMax - u.fuel);
            }
            if u.hp < u.maxHp * 0.7 {
                self.send_available(
                    &source_bank,
                    u.owner,
                    u.id,
                    "repair",
                    (u.maxHp - u.hp)
                        / (4. * self.repair_multiplier(u.id) * self.material_repair_factor(u.id)),
                );
            }
        }
        let facilities: Vec<_> = self
            .state
            .rooms
            .iter()
            .filter(|r| {
                r.hp > 0.
                    && r.progress >= 1.
                    && matches!(
                        r.kind.as_str(),
                        "factory" | "airfield" | "repair-bay" | "orbital-control"
                    )
            })
            .cloned()
            .collect();
        for r in facilities {
            for cargo in if r.kind == "repair-bay" {
                vec!["repair"]
            } else {
                vec!["ammo", "fuel"]
            } {
                let stock = self.stock_value(r.id, cargo);
                let desired = self.automatic_stock_target(r.id, cargo);
                if stock < desired {
                    self.send_available(&source_bank, r.owner, r.id, cargo, desired - stock);
                }
            }
        }
        let airstrips: Vec<_> = self
            .state
            .buildings
            .iter()
            .filter(|b| b.owner > 0 && b.kind == "airstrip" && b.hp > 0. && b.progress >= 1.)
            .cloned()
            .collect();
        for strip in airstrips {
            for (cargo, target) in [("ammo", 80.), ("fuel", 200.)] {
                let stock = self.stock_value(strip.id, cargo);
                if stock < target {
                    self.send_available(&source_bank, strip.owner, strip.id, cargo, target - stock);
                }
            }
        }
        crate::profiling::record_substage(
            "logistics/auto-dispatch",
            stage.elapsed().as_secs_f64() * 1000.,
        );
        // A nearby surviving friendly unit can physically recover cargo wrecks into a new shipment.
        let salvage: Vec<_> = self
            .state
            .resources
            .iter()
            .filter(|r| r.kind.starts_with("salvage-") && r.remaining > 0.5)
            .cloned()
            .collect();
        for pile in salvage {
            if self.state.shipments.len() >= MAX_SHIPMENTS {
                break;
            }
            let owners: BTreeSet<u32> = self
                .state
                .units
                .iter()
                .filter(|u| {
                    u.hp > 0. && (1..=2).contains(&u.owner) && u.pos.distance(pile.pos) < 4.
                })
                .map(|u| u.owner)
                .collect();
            if let Some(resource) = self.state.resources.iter_mut().find(|r| r.id == pile.id) {
                resource.contested = owners.len() > 1;
            }
            if owners.len() != 1 {
                continue;
            }
            if let Some(owner) = owners.iter().next().copied() {
                if let Some(to) = self.nearest_receiver(owner, pile.pos, None) {
                    let cargo = pile.kind.trim_start_matches("salvage-").to_owned();
                    let amount = pile
                        .remaining
                        .min(80.)
                        .min(self.receiving_space(to, &cargo));
                    if amount < crate::catalog::MIN_SHIPMENT_AMOUNT {
                        continue;
                    }
                    if let Some(route) = self.cargo_to_target(pile.pos, to) {
                        if self
                            .spend(owner, crate::catalog::TRANSPORT_DISPATCH_COST, 0.)
                            .is_err()
                        {
                            continue;
                        }
                        let id = self.id();
                        self.state
                            .resources
                            .iter_mut()
                            .find(|r| r.id == pile.id)
                            .unwrap()
                            .remaining -= amount;
                        self.state.shipments.push(Shipment {
                            unloadProgress: 0.,
                            mode: "ground".into(),
                            altitude: 0.,
                            flightState: "ground".into(),
                            flightTimer: 0.,
                            fuel: 0.,
                            fuelMax: 0.,
                            waypoints: vec![],
                            manualRoute: false,
                            id,
                            owner,
                            from: pile.id,
                            to,
                            pos: pile.pos,
                            route,
                            amount,
                            hp: 60. + 8. * amount.sqrt(),
                            progress: 0.,
                            cargo,
                        });
                        self.event("salvage", pile.pos, owner, amount, id);
                        self.event(
                            "transport-spent",
                            pile.pos,
                            owner,
                            crate::catalog::TRANSPORT_DISPATCH_COST,
                            id,
                        );
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Explicit unearned logistics fixtures: isolate manufacturing and physical
    // shipments without claiming a normal opening, AI planner or balance match.
    fn workshop_fixture(capacity: u32, width: i32) -> (Game, u64) {
        let mut g = Game::new(6026, false);
        g.state.terrain.fill(0);
        g.state.resources.clear();
        g.state.units.clear();
        for b in &mut g.state.buildings {
            b.stock.clear();
            b.inventory = 0.;
        }
        g.player_mut(1).unwrap().credits = 10_000.;
        let mut shell = g.state.buildings[0].clone();
        shell.id = 1000;
        shell.kind = "shell".into();
        shell.rect = Rect { x: 2, y: 38, level: 0, width: 8, height: 4 };
        g.state.buildings.push(shell);
        let room: Room = serde_json::from_value(serde_json::json!({
            "id":1002,"shell":1000,"owner":1,"rect":{"x":2,"y":38,"z":0,"w":width,"h":2},
            "kind":"ammunition-workshop","equipmentShare":1.,"branch":null,"tier":1,
            "hp":140.,"maxHp":140.,"powered":true,"connected":false,"online":true,
            "capacity":capacity,"capacityBudget":capacity,"potentialCapacity":width*20,
            "gpus":[],"inventory":0.,"progress":1.,"buildTime":1.,"cooldown":0.,"stock":{},"invested":180.
        })).unwrap();
        g.state.rooms.push(room);
        g.state.entrances.push(Entrance { id: 1001, owner: 1, pos: Pos::new(2,41,0),
            toLevel: 0, kind: "door".into(), hp: 300., open: true, powered: false,
            width: 1, axis: "x".into() });
        g.next_id = 2000;
        g.invalidate_navigation();
        (g, 1002)
    }

    fn workshop_seconds(g: &mut Game, seconds: u32) {
        for _ in 0..seconds*60 {
            g.state.tick += 1;
            if g.state.tick % 60 == 0 { g.logistics_second(); }
            g.advance_shipments();
        }
    }

    // Select one existing production slot for boundary tests, without changing
    // the production dispatcher or treating this fixture time as match evidence.
    fn workshop_slot(g: &mut Game, id: u64, cargo: &str) {
        let phase = match cargo { "fuel" => 0, "repair" => 1, _ => 2 };
        loop {
            g.state.tick += 60;
            if (g.state.tick/60 + id) % 6 == phase { break; }
        }
        g.logistics_second();
    }

    fn workshop_stock(g: &mut Game, id: u64, values: &[(&str, f64)]) {
        let r = g.state.rooms.iter_mut().find(|r|r.id==id).unwrap();
        r.stock = values.iter().map(|(k,v)|(k.to_string(),*v)).collect();
        r.inventory = values.iter().map(|(_,v)|v).sum();
    }

    fn near(actual: f64, expected: f64) {
        assert!((actual-expected).abs()<1e-8,"actual{actual}, expected{expected}");
    }

    #[test]
    fn workshop_normal_and_small_buffers_share_only_actual_capacity() {
        let (mut g,id)=workshop_fixture(40,2);
        let r=g.state.rooms.iter().find(|r|r.id==id).unwrap();
        near(Game::workshop_stock_target(r,"ammo"),20.);
        near(Game::workshop_stock_target(r,"fuel"),10.);
        near(Game::workshop_stock_target(r,"repair"),10.);
        for capacity in [0,1,5,10,20,30,40] {
            let r=g.state.rooms.iter_mut().find(|r|r.id==id).unwrap();
            r.capacity=capacity;r.capacityBudget=Some(capacity);
            let targets=["ammo","fuel","repair"].map(|c|Game::workshop_stock_target(r,c));
            near(targets.iter().sum(),capacity as f64);
            assert!(targets.iter().all(|v|*v>=0.));
            if capacity>0 { assert!(targets.iter().all(|v|*v>0.)); }
        }
    }

    #[test]
    fn workshop_zero_fuel_use_does_not_starve_repeated_paid_ammo_repair_deliveries() {
        let (mut g,id)=workshop_fixture(40,2);
        let core=g.state.buildings.iter().find(|b|b.kind=="core"&&b.owner==1).unwrap().id;
        let initial_credits=g.player(1).unwrap().credits;
        for second in 1..=240 {
            workshop_seconds(&mut g,1);
            if second%12==0 {
                for cargo in ["ammo","repair"] {
                    let amount=g.stock_value(id,cargo);
                    if amount>=4. { g.dispatch_supply(1,id,core,amount,cargo.into()).unwrap(); }
                }
            }
            assert!(g.stock_value(id,"fuel")<=10.+1e-8);
            assert!(g.state.rooms[0].stock.values().sum::<f64>()<=40.+1e-8);
        }
        workshop_seconds(&mut g,30);
        assert!(g.stock_value(core,"ammo")>100.,"ammo must physically arrive repeatedly");
        assert!(g.stock_value(core,"repair")>40.,"repair must physically arrive repeatedly");
        near(g.stock_value(id,"fuel"),10.);
        let manufactured_cost: f64=[("ammo",0.35),("fuel",0.2),("repair",0.7)].iter()
            .map(|(cargo,price)|(g.stock_value(id,cargo)+g.stock_value(core,cargo)
                +g.state.shipments.iter().filter(|s|s.cargo==*cargo).map(|s|s.amount).sum::<f64>())*price).sum();
        let fees=g.state.events.iter().filter(|e|e.kind=="transport-spent").map(|e|e.magnitude).sum::<f64>();
        assert!(fees>0.);
        near(initial_credits-g.player(1).unwrap().credits,manufactured_cost+fees);
    }

    #[test]
    fn workshop_standard_buffer_funds_a_real_ten_material_repair_job() {
        let (mut g,id)=workshop_fixture(40,2);
        workshop_seconds(&mut g,90);
        near(g.stock_value(id,"repair"),10.);
        let core=g.state.buildings.iter_mut().find(|b|b.kind=="core"&&b.owner==1).unwrap();
        core.hp-=100.;let target=core.id;
        let credits=g.player(1).unwrap().credits;
        g.queue_repair(1,target).unwrap();
        near(g.stock_value(id,"repair"),0.);
        near(credits-g.player(1).unwrap().credits,20.);
        assert!(g.state.jobs.iter().any(|j|j.target==target&&j.kind=="repair"));
    }

    #[test]
    fn workshop_positive_fraction_finishes_repair_buffer_at_actual_price() {
        let (mut g,id)=workshop_fixture(40,2);
        workshop_stock(&mut g,id,&[("ammo",20.),("fuel",10.),("repair",9.6)]);
        let credits=g.player(1).unwrap().credits;
        workshop_slot(&mut g,id,"repair");
        near(g.stock_value(id,"repair"),10.);
        near(g.state.rooms[0].inventory,40.);
        near(credits-g.player(1).unwrap().credits,0.4*0.7);
    }

    #[test]
    fn workshop_manufacture_respects_real_inbound_reservations() {
        for reserved in [38.,40.] {
            let (mut g,id)=workshop_fixture(40,2);
            let core=g.state.buildings.iter_mut().find(|b|b.kind=="core"&&b.owner==1).unwrap();
            core.stock.insert("fuel".into(),reserved);let from=core.id;
            g.dispatch_supply(1,from,id,reserved,"fuel".into()).unwrap();
            let credits=g.player(1).unwrap().credits;
            workshop_slot(&mut g,id,"ammo");
            near(g.stock_value(id,"ammo"),40.-reserved);
            near(credits-g.player(1).unwrap().credits,(40.-reserved)*0.35);
            near(g.stock_value(id,"ammo")+g.state.shipments.iter().map(|s|s.amount).sum::<f64>(),40.);
            workshop_seconds(&mut g,30);
            near(g.stock_value(id,"fuel"),reserved);
            assert!(g.state.rooms[0].inventory<=40.+1e-8);
        }
    }

    #[test]
    fn workshop_unpowered_or_unfunded_production_cannot_create_stock() {
        let (mut g,id)=workshop_fixture(40,2);
        g.state.rooms[0].powered=false;
        let credits=g.player(1).unwrap().credits;
        workshop_seconds(&mut g,12);
        near(g.state.rooms[0].inventory,0.);near(g.player(1).unwrap().credits,credits);
        g.state.rooms[0].powered=true;g.player_mut(1).unwrap().credits=0.;
        workshop_seconds(&mut g,12);
        near(g.state.rooms[0].inventory,0.);
        g.player_mut(1).unwrap().credits=1.4;
        workshop_slot(&mut g,id,"ammo");
        near(g.stock_value(id,"ammo"),4.);near(g.player(1).unwrap().credits,0.);
        workshop_slot(&mut g,id,"repair");near(g.stock_value(id,"repair"),0.);
    }

    #[test]
    fn workshop_existing_excess_is_kept_until_real_paid_export() {
        let (mut g,id)=workshop_fixture(40,2);
        workshop_stock(&mut g,id,&[("fuel",40.)]);
        let credits=g.player(1).unwrap().credits;
        workshop_seconds(&mut g,24);
        near(g.stock_value(id,"fuel"),40.);near(g.player(1).unwrap().credits,credits);
        let core=g.state.buildings.iter().find(|b|b.kind=="core"&&b.owner==1).unwrap().id;
        g.dispatch_supply(1,id,core,30.,"fuel".into()).unwrap();
        near(g.stock_value(id,"fuel"),10.);
        workshop_seconds(&mut g,90);
        near(g.stock_value(core,"fuel"),30.);near(g.stock_value(id,"fuel"),10.);
        near(g.stock_value(id,"ammo"),20.);near(g.stock_value(id,"repair"),10.);
    }

    #[test]
    fn workshop_capacity_ten_retains_all_three_cargoes_without_fake_repair_batch() {
        let (mut g,id)=workshop_fixture(10,2);
        workshop_seconds(&mut g,120);
        for cargo in ["ammo","fuel","repair"] { assert!(g.stock_value(id,cargo)>0.); }
        near(g.state.rooms[0].inventory,10.);
        assert!(g.stock_value(id,"repair")<10.);
        let core=g.state.buildings.iter_mut().find(|b|b.kind=="core"&&b.owner==1).unwrap();
        core.hp-=100.;let target=core.id;
        assert!(g.queue_repair(1,target).is_err(),"small buffers cannot fabricate ten local repair materials");
    }

    #[test]
    fn workshop_split_merge_preserves_stock_share_and_six_slot_output() {
        let (mut g,id)=workshop_fixture(80,4);
        workshop_stock(&mut g,id,&[("ammo",4.),("fuel",4.),("repair",4.)]);
        let initial=g.state.rooms[0].clone();
        g.extra_construction(1,Command::SplitRoom{id,axis:"x".into(),offset:2}).unwrap();
        let ids=g.state.rooms.iter().map(|r|r.id).collect::<Vec<_>>();
        assert_eq!(ids.len(),2);
        near(g.state.rooms.iter().map(|r|r.equipmentShare).sum(),initial.equipmentShare);
        near(g.state.rooms.iter().flat_map(|r|r.stock.values()).sum(),12.);
        assert!(g.state.rooms.iter().all(|r|r.capacity==40));
        workshop_seconds(&mut g,6);
        near(g.state.rooms.iter().map(|r|r.stock.values().sum::<f64>()).sum(),36.);
        g.extra_construction(1,Command::MergeRooms{ids}).unwrap();
        assert_eq!(g.state.rooms.len(),1);near(g.state.rooms[0].equipmentShare,1.);
        near(g.state.rooms[0].stock.values().sum(),36.);
        near(g.state.rooms[0].hp,initial.hp);near(g.state.rooms[0].invested,initial.invested);
        workshop_seconds(&mut g,6);
        near(g.state.rooms[0].stock.values().sum(),60.);
    }

    #[test]
    fn workshop_consumed_fuel_refills_through_existing_demand_transport() {
        let (mut g,id)=workshop_fixture(40,2);
        let mut station=g.state.buildings[0].clone();
        station.id=1010;station.kind="coal-power".into();station.powered=true;
        station.rect=Rect{x:14,y:46,level:0,width:2,height:2};
        station.stock.insert("fuel".into(),20.);
        g.state.buildings.push(station);g.invalidate_navigation();
        for _ in 0..240 {
            workshop_seconds(&mut g,1);
            assert!(g.stock_value(id,"fuel")<=10.+1e-8);
        }
        assert!(g.stock_value(1010,"fuel")>0.,"burning station must receive replenishment");
        // The burning station is this fixture's only automatic consumer.
        let fuel_dispatches=g.state.events.iter().filter(|e|e.kind=="transport-spent").count();
        assert!(fuel_dispatches>2,"production target must not become an export reserve");
        near(g.stock_value(id,"ammo"),20.);near(g.stock_value(id,"repair"),10.);
    }

    #[test]
    fn workshop_saved_existing_inventory_is_not_clipped_by_manufacturing_targets() {
        let (mut g,id)=workshop_fixture(40,2);
        workshop_stock(&mut g,id,&[("fuel",40.)]);
        let save=g.save();let mut restored=Game::load(save).unwrap();
        assert_eq!(restored.state.rooms[0].stock,g.state.rooms[0].stock);
        workshop_seconds(&mut restored,18);
        near(restored.stock_value(id,"fuel"),40.);
        near(restored.state.rooms[0].inventory,40.);
    }

    #[test]
    fn inventory_conserved_through_real_dispatch_and_arrival() {
        // A second owned receiver is an explicit rule fixture, not an earned economy claim.
        let mut g = Game::new(52, false);
        let source = g.state.buildings[0].clone();
        let mut receiver = source.clone();
        receiver.id = 900;
        receiver.rect.x = 14;
        receiver.stock.clear();
        receiver.inventory = 0.;
        g.state.buildings.push(receiver);
        let original = g.stock_value(source.id, "ammo");
        g.dispatch_supply(1, source.id, 900, 30., "ammo".into())
            .unwrap();
        assert_eq!(g.stock_value(source.id, "ammo"), original - 30.);
        assert_eq!(g.stock_value(900, "ammo"), 0.);
        assert_eq!(g.state.shipments[0].amount, 30.);
        for _ in 0..600 {
            g.state.tick += 1;
            g.advance_shipments();
        }
        assert!(g.state.shipments.is_empty());
        assert_eq!(g.stock_value(900, "ammo"), 30.);
        assert_eq!(
            g.stock_value(source.id, "ammo") + g.stock_value(900, "ammo"),
            original
        );
    }
    #[test]
    fn ordinary_mining_pays_only_after_physical_delivery() {
        let mut g = Game::new(54, false);
        let before = g.player(1).unwrap().credits;
        let receipt = g.order(Order {
            owner: 1,
            sequence: 1,
            command: Command::Build {
                pos: Pos::new(18, 40, 0),
                kind: "extractor".into(),
            },
        });
        assert!(receipt.accepted, "{}", receipt.reason);
        let after_purchase = g.player(1).unwrap().credits;
        assert!(after_purchase < before);
        let mut mined_before_delivery = false;
        let mut delivered = false;
        for _ in 0..3600 {
            g.step();
            let stock = g
                .state
                .buildings
                .iter()
                .find(|b| b.owner == 1 && b.kind == "extractor")
                .map(|b| b.stock.get("ore").copied().unwrap_or(0.))
                .unwrap_or(0.);
            let arrived = g
                .state
                .events
                .iter()
                .any(|e| e.owner == 1 && e.kind == "ore-delivered");
            if stock > 0. && !arrived {
                mined_before_delivery = true;
                let transport = g
                    .state
                    .events
                    .iter()
                    .filter(|e| e.owner == 1 && e.kind == "transport-spent")
                    .map(|e| e.magnitude)
                    .sum::<f64>();
                assert!(
                    (g.player(1).unwrap().credits - after_purchase + transport
                        - g.state.tick as f64 / 60. * 0.35)
                        .abs()
                        < 0.01,
                    "undelivered ore changed wallet"
                );
            }
            if arrived {
                delivered = true;
                break;
            }
        }
        assert!(mined_before_delivery);
        assert!(delivered, "ordinary extractor never delivered its ore");
    }
    #[test]
    fn stock_rejects_unfunded_and_enemy_transfers() {
        let mut g = Game::new(42, false);
        let c1 = g.state.buildings.iter().find(|b| b.owner == 1).unwrap().id;
        let c2 = g.state.buildings.iter().find(|b| b.owner == 2).unwrap().id;
        let before = g.save();
        assert!(g.dispatch_supply(1, c1, c2, 10., "ammo".into()).is_err());
        assert!(g.dispatch_supply(1, c1, c1, 1., "ammo".into()).is_err());
        assert_eq!(g.state.shipments.len(), 0);
        assert_eq!(
            g.player(1).unwrap().credits,
            before.snapshot.players[0].credits
        );
    }
    #[test]
    fn lost_cargo_becomes_partial_recoverable_wreck() {
        let mut g = Game::new(44, false);
        let core = g.state.buildings[0].clone();
        g.state.shipments.push(Shipment {
            unloadProgress: 0.,
            mode: "ground".into(),
            altitude: 0.,
            flightState: "ground".into(),
            flightTimer: 0.,
            fuel: 0.,
            fuelMax: 0.,
            waypoints: vec![],
            manualRoute: false,
            id: 900,
            owner: 1,
            from: core.id,
            to: core.id,
            pos: Pos::new(20, 48, 0),
            route: vec![core.rect.center()],
            amount: 100.,
            hp: 0.,
            progress: 0.,
            cargo: "ammo".into(),
        });
        g.advance_shipments();
        assert!(g.state.shipments.is_empty());
        let pile = g
            .state
            .resources
            .iter()
            .find(|r| r.kind == "salvage-ammo")
            .unwrap();
        assert_eq!(pile.remaining, 60.);
    }
    #[test]
    fn blocked_shipment_never_teleports_or_awards_credits() {
        let mut g = Game::new(46, false);
        let core = g.state.buildings[0].clone();
        let pos = Pos::new(40, 40, 0);
        let wall = Pos::new(41, 40, 0);
        g.state.walls.push(Wall {
            id: 901,
            owner: 1,
            pos: wall,
            kind: "physical".into(),
            hp: 100.,
            maxHp: 100.,
            shield: 0.,
            invested: 0.,
            antiHeal: 0.,
        });
        g.state.shipments.push(Shipment {
            unloadProgress: 0.,
            mode: "ground".into(),
            altitude: 0.,
            flightState: "ground".into(),
            flightTimer: 0.,
            fuel: 0.,
            fuelMax: 0.,
            waypoints: vec![],
            manualRoute: false,
            id: 902,
            owner: 1,
            from: core.id,
            to: core.id,
            pos,
            route: vec![wall, core.rect.center()],
            amount: 40.,
            hp: 150.,
            progress: 0.99,
            cargo: "ore".into(),
        });
        let money = g.player(1).unwrap().credits;
        g.state.tick = 1;
        g.advance_shipments();
        assert_eq!(g.state.shipments[0].pos, pos);
        assert_eq!(g.player(1).unwrap().credits, money);
    }
}

#[cfg(test)]
#[path = "../tests/support/logistics_contracts.rs"]
mod logistics_contracts;
