use crate::{catalog, types::*, Game};
#[cfg(test)]
use std::collections::VecDeque;
use std::collections::{BTreeMap, BTreeSet};
use std::{cell::RefCell, sync::Arc};

#[derive(Clone, Default)]
pub(crate) struct NetworkCache {
    key: Option<Arc<[u64]>>,
    layout: Option<Arc<NetworkLayout>>,
}
pub(crate) fn cache() -> RefCell<NetworkCache> {
    RefCell::new(NetworkCache::default())
}
struct NetworkLayout {
    owners: Vec<OwnerNetworkLayout>,
}
struct OwnerNetworkLayout {
    power_groups: Vec<BTreeSet<Pos>>,
    power_cells: Vec<BTreeSet<Pos>>,
    power_ports: BTreeMap<u64, usize>,
    compute_groups: Vec<BTreeSet<Pos>>,
    compute_cells: Vec<BTreeSet<Pos>>,
    compute_ports: BTreeMap<u64, usize>,
    compute_rooms: Vec<Vec<usize>>,
    isolated_cells: BTreeMap<usize, BTreeSet<Pos>>,
    isolated_rooms: BTreeMap<usize, Vec<usize>>,
    elevators: Vec<(u64, Vec<usize>)>,
}
// Only physical connectivity belongs in this cache. Fuel, jamming, GPUs,
// equipment share, charge and available capacity are recomputed below.
fn layout_key(s: &Snapshot) -> Vec<u64> {
    let mut key = Vec::new();
    let mut mix = |n: u64| {
        key.push(n);
    };
    for l in &s.links {
        mix(1);
        mix(l.id);
        mix(l.owner as u64);
        mix((l.hp > 0.) as u64);
        mix(l.kind.len() as u64);
        for c in l.kind.bytes() {
            mix(c as u64);
        }
        mix(l.path.len() as u64);
        for p in &l.path {
            mix(p.x as u64);
            mix(p.y as u64);
            mix(p.level as u64);
        }
    }
    for e in s
        .entrances
        .iter()
        .filter(|e| matches!(e.kind.as_str(), "stairs" | "elevator" | "ramp"))
    {
        mix(2);
        mix(e.id);
        mix(e.owner as u64);
        mix((e.hp > 0.) as u64);
        mix(e.kind.len() as u64);
        for c in e.kind.bytes() {
            mix(c as u64);
        }
        for n in [e.pos.x, e.pos.y, e.pos.level, e.toLevel, e.width as i32] {
            mix(n as u64);
        }
        mix(e.axis.len() as u64);
        for c in e.axis.bytes() {
            mix(c as u64);
        }
    }
    for r in &s.rooms {
        mix(3);
        mix(r.id);
        mix(r.owner as u64);
        mix((r.hp > 0.) as u64);
        mix((r.progress >= 1.) as u64);
        for n in [
            r.rect.x,
            r.rect.y,
            r.rect.level,
            r.rect.width,
            r.rect.height,
        ] {
            mix(n as u64);
        }
    }
    for b in s
        .buildings
        .iter()
        .filter(|b| b.kind != "shell" || b.power != 0. || b.demand != 0.)
    {
        mix(4);
        mix(b.id);
        mix(b.owner as u64);
        mix((b.hp > 0.) as u64);
        mix((b.progress >= 1.) as u64);
        mix((b.kind == "shell") as u64);
        for n in [
            b.rect.x,
            b.rect.y,
            b.rect.level,
            b.rect.width,
            b.rect.height,
        ] {
            mix(n as u64);
        }
    }
    key
}
fn group_ports(groups: &[BTreeSet<Pos>], entities: &[(u64, Rect)]) -> BTreeMap<u64, usize> {
    let index: BTreeMap<_, _> = groups
        .iter()
        .enumerate()
        .flat_map(|(i, g)| g.iter().map(move |p| (*p, i)))
        .collect();
    entities
        .iter()
        .filter_map(|(id, rect)| {
            rect.cells()
                .iter()
                .filter_map(|p| index.get(p))
                .min()
                .map(|i| (*id, *i))
        })
        .collect()
}
fn make_layout(s: &Snapshot) -> NetworkLayout {
    let _profile = crate::profiling::observe("network/layout-build");
    let owners =
        (1..=2)
            .map(|owner| {
                let rooms: Vec<_> = s
                    .rooms
                    .iter()
                    .enumerate()
                    .filter(|(_, r)| r.owner == owner && r.hp > 0. && r.progress >= 1.)
                    .collect();
                let mut rects: Vec<_> = rooms.iter().map(|(_, r)| r.rect).collect();
                rects.extend(
                    s.buildings
                        .iter()
                        .filter(|b| {
                            b.owner == owner && b.kind != "shell" && b.hp > 0. && b.progress >= 1.
                        })
                        .map(|b| b.rect),
                );
                let power_groups = components(&s.links, &s.entrances, owner, "power", &rects);
                let compute_groups = components(&s.links, &s.entrances, owner, "compute", &rects);
                let entities: Vec<_> = s
                    .rooms
                    .iter()
                    .filter(|r| r.owner == owner)
                    .map(|r| (r.id, r.rect))
                    .chain(
                        s.buildings
                            .iter()
                            .filter(|b| {
                                b.owner == owner
                                    && (b.kind != "shell" || b.power != 0. || b.demand != 0.)
                            })
                            .map(|b| (b.id, b.rect)),
                    )
                    .collect();
                let power_ports = group_ports(&power_groups, &entities);
                let compute_ports = group_ports(&compute_groups, &entities);
                let mut power_cells = power_groups.clone();
                let mut compute_cells = compute_groups.clone();
                let mut compute_rooms = vec![Vec::new(); compute_groups.len()];
                for (_, r) in &rooms {
                    if let Some(g) = power_ports.get(&r.id) {
                        power_cells[*g].extend(r.rect.cells());
                    }
                }
                for b in s.buildings.iter().filter(|b| {
                    b.owner == owner && b.kind != "shell" && b.hp > 0. && b.progress >= 1.
                }) {
                    if let Some(g) = power_ports.get(&b.id) {
                        power_cells[*g].extend(b.rect.cells());
                    }
                }
                for (i, r) in &rooms {
                    if let Some(g) = compute_ports.get(&r.id) {
                        compute_cells[*g].extend(r.rect.cells());
                        compute_rooms[*g].push(*i);
                    }
                }
                let mut occupied = BTreeMap::<Pos, Vec<usize>>::new();
                for (i, r) in &rooms {
                    for p in r.rect.cells() {
                        occupied.entry(p).or_default().push(*i);
                    }
                }
                let mut isolated_rooms = BTreeMap::new();
                let mut isolated_cells = BTreeMap::new();
                for (i, r) in &rooms {
                    if compute_ports.contains_key(&r.id) {
                        continue;
                    }
                    let bare = r.rect.cells();
                    let attached: BTreeSet<usize> = bare
                        .iter()
                        .filter_map(|p| occupied.get(p))
                        .flatten()
                        .copied()
                        .collect();
                    let mut cells: BTreeSet<Pos> = bare.into_iter().collect();
                    for idx in &attached {
                        cells.extend(s.rooms[*idx].rect.cells());
                    }
                    isolated_rooms.insert(*i, attached.into_iter().collect());
                    isolated_cells.insert(*i, cells);
                }
                let elevators = s
                    .entrances
                    .iter()
                    .filter(|e| e.owner == owner && e.hp > 0. && e.kind == "elevator")
                    .map(|e| {
                        (
                            e.id,
                            power_groups
                                .iter()
                                .enumerate()
                                .filter(|(_, g)| g.iter().any(|p| e.covers(*p)))
                                .map(|(i, _)| i)
                                .collect(),
                        )
                    })
                    .collect();
                OwnerNetworkLayout {
                    power_groups,
                    power_cells,
                    power_ports,
                    compute_groups,
                    compute_cells,
                    compute_ports,
                    compute_rooms,
                    isolated_cells,
                    isolated_rooms,
                    elevators,
                }
            })
            .collect();
    NetworkLayout { owners }
}
fn valid_edge(a: Pos, b: Pos, entrances: &[Entrance], owner: u32) -> bool {
    a.level == b.level
        || entrances.iter().any(|e| {
            e.owner == owner
                && e.hp > 0.
                && matches!(e.kind.as_str(), "stairs" | "elevator" | "ramp")
                && e.covers(a)
                && e.covers(b)
        })
}
fn components(
    links: &[Link],
    entrances: &[Entrance],
    owner: u32,
    kind: &str,
    rects: &[Rect],
) -> Vec<BTreeSet<Pos>> {
    // Index order matches Pos's (x,y,level) ordering, preserving both group
    // ordering and downstream anchor/tie choices from the previous BFS.
    const CELLS: usize = 128 * 96 * 8;
    fn index(p: Pos) -> usize {
        (p.x as usize * 96 + p.y as usize) * 8 + (p.level + 2) as usize
    }
    fn position(i: usize) -> Pos {
        Pos::new(
            (i / (96 * 8)) as i32,
            ((i / 8) % 96) as i32,
            (i % 8) as i32 - 2,
        )
    }
    fn root(parent: &mut [i32], mut n: usize) -> usize {
        let mut r = n;
        while parent[r] as usize != r {
            r = parent[r] as usize;
        }
        while parent[n] as usize != r {
            let next = parent[n] as usize;
            parent[n] = r as i32;
            n = next;
        }
        r
    }
    fn join(parent: &mut [i32], a: usize, b: usize) {
        let a = root(parent, a);
        let b = root(parent, b);
        if a != b {
            parent[a.max(b)] = a.min(b) as i32;
        }
    }
    let mut parent = vec![-1; CELLS];
    let mut active = Vec::new();
    for link in links
        .iter()
        .filter(|l| l.owner == owner && l.kind == kind && l.hp > 0.)
    {
        for edge in link.path.windows(2) {
            if !edge[0].valid()
                || !edge[1].valid()
                || !valid_edge(edge[0], edge[1], entrances, owner)
            {
                continue;
            }
            let a = index(edge[0]);
            let b = index(edge[1]);
            for p in [a, b] {
                if parent[p] < 0 {
                    parent[p] = p as i32;
                    active.push(p);
                }
            }
            join(&mut parent, a, b);
        }
    }
    for rect in rects {
        let mut first = None;
        for x in rect.x..rect.x + rect.width {
            for y in rect.y..rect.y + rect.height {
                let p = Pos::new(x, y, rect.level);
                if !p.valid() {
                    continue;
                }
                let at = index(p);
                // A facility joins cables touching its ports; it must not
                // manufacture a new cable component on otherwise empty cells.
                if parent[at] < 0 {
                    continue;
                }
                if let Some(first) = first {
                    join(&mut parent, first, at);
                } else {
                    first = Some(at);
                }
            }
        }
    }
    active.sort_unstable();
    let mut groups = BTreeMap::<usize, Vec<Pos>>::new();
    for p in active {
        let r = root(&mut parent, p);
        groups.entry(r).or_default().push(position(p));
    }
    groups
        .into_values()
        .map(|cells| cells.into_iter().collect())
        .collect()
}
#[cfg(test)]
fn reference_components(
    links: &[Link],
    entrances: &[Entrance],
    owner: u32,
    kind: &str,
) -> Vec<BTreeSet<Pos>> {
    let mut adj: BTreeMap<Pos, BTreeSet<Pos>> = BTreeMap::new();
    for l in links
        .iter()
        .filter(|l| l.owner == owner && l.kind == kind && l.hp > 0.)
    {
        for p in l.path.windows(2) {
            if !valid_edge(p[0], p[1], entrances, owner) {
                continue;
            }
            adj.entry(p[0]).or_default().insert(p[1]);
            adj.entry(p[1]).or_default().insert(p[0]);
        }
    }
    let mut seen = BTreeSet::new();
    let mut out = vec![];
    for start in adj.keys() {
        if !seen.insert(*start) {
            continue;
        }
        let mut cells = BTreeSet::from([*start]);
        let mut q = VecDeque::from([*start]);
        while let Some(p) = q.pop_front() {
            for n in &adj[&p] {
                if seen.insert(*n) {
                    cells.insert(*n);
                    q.push_back(*n);
                }
            }
        }
        out.push(cells);
    }
    out
}
fn port(groups: &[BTreeSet<Pos>], rect: Rect) -> Option<usize> {
    groups
        .iter()
        .position(|cells| rect.cells().iter().any(|p| cells.contains(p)))
}
#[cfg(test)]
fn reference_merge_ports(groups: &mut Vec<BTreeSet<Pos>>, rects: &[Rect]) {
    for rect in rects {
        let matching: Vec<usize> = groups
            .iter()
            .enumerate()
            .filter(|(_, g)| rect.cells().iter().any(|p| g.contains(p)))
            .map(|(i, _)| i)
            .collect();
        if matching.len() > 1 {
            let first = matching[0];
            for index in matching.into_iter().skip(1).rev() {
                let removed = groups.remove(index);
                groups[first].extend(removed);
            }
        }
    }
}

#[cfg(test)]
mod component_contracts {
    use super::*;
    fn cable(id: u64, owner: u32, kind: &str, path: Vec<Pos>) -> Link {
        Link {
            id,
            owner,
            kind: kind.into(),
            path,
            hp: 150.,
            active: false,
            invested: 1.,
            unitEndpoints: vec![],
        }
    }
    #[test]
    fn disjoint_set_matches_the_original_graph_and_port_merge_exactly() {
        let mut random = 711u64;
        for case in 0..32 {
            let mut links = Vec::new();
            let mut rects = Vec::new();
            for id in 0..64 {
                random = random.wrapping_mul(6364136223846793005).wrapping_add(1);
                let x = 8 + ((random >> 16) % 22) as i32;
                let y = 8 + ((random >> 32) % 22) as i32;
                let z = (id % 8) as i32 - 2;
                let mut path = vec![Pos::new(x, y, z)];
                for n in 1..5 {
                    path.push(Pos::new(x + n, y, z));
                }
                for n in 1..5 {
                    path.push(Pos::new(x + 4, y + n, z));
                }
                let mut link = cable(
                    id + 1,
                    (1 + id % 2) as u32,
                    if id % 3 == 0 { "power" } else { "compute" },
                    path,
                );
                if (id + case) % 9 == 0 {
                    link.hp = 0.;
                }
                links.push(link);
                rects.push(Rect {
                    x,
                    y,
                    level: z,
                    width: 5,
                    height: 5,
                });
            }
            let mut shaft = Entrance {
                id: 200,
                owner: 1,
                kind: "elevator".into(),
                pos: Pos::new(20, 20, -2),
                toLevel: 5,
                hp: 100.,
                width: 1,
                axis: "x".into(),
                open: true,
                powered: false,
            };
            if case % 3 == 0 {
                shaft.hp = 0.;
            }
            links.push(cable(
                300,
                1,
                "compute",
                (-2..=5).map(|z| Pos::new(20, 20, z)).collect(),
            ));
            links.push(cable(301, 2, "power", vec![Pos::new(50, 50, 0)]));
            if case % 2 == 0 {
                links.reverse();
                rects.reverse();
            }
            for owner in [1, 2] {
                for kind in ["power", "compute"] {
                    let mut expected = reference_components(&links, &[shaft.clone()], owner, kind);
                    reference_merge_ports(&mut expected, &rects);
                    assert_eq!(
                        components(&links, &[shaft.clone()], owner, kind, &rects),
                        expected,
                        "case {case}, owner {owner}, {kind}"
                    );
                }
            }
        }
    }
    #[test]
    fn a_port_joins_existing_cables_without_filling_empty_cells() {
        let links = vec![
            cable(
                1,
                1,
                "power",
                vec![Pos::new(10, 10, 0), Pos::new(11, 10, 0)],
            ),
            cable(
                2,
                1,
                "power",
                vec![Pos::new(17, 10, 0), Pos::new(18, 10, 0)],
            ),
        ];
        let rects = vec![Rect {
            x: 10,
            y: 9,
            level: 0,
            width: 9,
            height: 3,
        }];
        let result = components(&links, &[], 1, "power", &rects);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].len(), 4);
        assert!(!result[0].contains(&Pos::new(14, 10, 0)));
    }
}
impl Game {
    fn network_layout(&self) -> Arc<NetworkLayout> {
        let _profile = crate::profiling::observe("network/layout-check");
        let key = layout_key(&self.state);
        let mut cached = self.network_topology.borrow_mut();
        if cached
            .key
            .as_ref()
            .is_some_and(|previous| previous.as_ref() == key.as_slice())
        {
            if let Some(layout) = &cached.layout {
                return Arc::clone(layout);
            }
        }
        let layout = Arc::new(make_layout(&self.state));
        cached.key = Some(key.into());
        cached.layout = Some(Arc::clone(&layout));
        layout
    }
    fn publish_compute_group(
        &mut self,
        owner: u32,
        cells: &BTreeSet<Pos>,
        attached: &[usize],
        sources: &[Option<(Pos, f64, f64)>],
        old: &[NetworkStore],
    ) {
        let mut anchor: Option<Pos> = None;
        let mut capacity = 0.;
        let mut production = 0.;
        for idx in attached {
            if let Some((pos, cap, rate)) = sources[*idx] {
                anchor = Some(anchor.map_or(pos, |a| a.min(pos)));
                capacity += cap;
                production += rate;
            }
        }
        let Some(anchor) = anchor else {
            return;
        };
        for idx in attached {
            self.state.rooms[*idx].connected = true;
        }
        let compute = old
            .iter()
            .filter(|s| s.owner == owner && cells.contains(&s.anchor))
            .map(|s| s.compute)
            .sum::<f64>()
            .min(capacity);
        self.state.networkStores.push(NetworkStore {
            owner,
            anchor,
            cells: cells.clone(),
            compute,
            capacity,
            production,
        });
    }
    /// The actual physical charging connection, shared by recharge and planning.
    /// Movement, remaining capacity and this tick's spare power are checked by
    /// the caller. A hypothetical stationary unit may be used to select a route
    /// endpoint, but proximity alone cannot bypass a service room's walls.
    pub(crate) fn energy_recharge_grid(&self, u: &Unit) -> Option<usize> {
        let air = catalog::unit_ref(&u.kind).is_some_and(|d| d.category == "air");
        let runways: Vec<_> = self
            .state
            .buildings
            .iter()
            .filter(|b| {
                b.owner == u.owner
                    && b.kind == "airstrip"
                    && b.hp > 0.
                    && b.progress >= 1.
                    && b.powered
                    && b.rect.contains(u.pos)
            })
            .map(|b| b.rect.center())
            .collect();
        if air && (u.flightState != "landed" || u.altitude > 0.25 || runways.is_empty()) {
            return None;
        }
        let wired = self.state.links.iter().any(|l| {
            l.owner == u.owner
                && l.kind == "power"
                && l.active
                && (l.path.first() == Some(&u.pos) || l.path.last() == Some(&u.pos))
        });
        let service = if air {
            runways
        } else {
            self.state
                .rooms
                .iter()
                .filter(|r| {
                    r.owner == u.owner
                        && r.hp > 0.
                        && r.progress >= 1.
                        && r.rect.level == u.level
                        && r.powered
                        && matches!(
                            r.kind.as_str(),
                            "factory"
                                | "depot"
                                | "airfield"
                                | "energy-defense"
                                | "repair-bay"
                                | "particle-foundry"
                        )
                        && r.rect.center().distance(u.pos) <= 5. * r.equipmentShare.sqrt()
                        && self.skill_line_clear(u.pos, r.rect.center(), u.owner)
                })
                .map(|r| r.rect.center())
                .collect::<Vec<_>>()
        };
        self.state.powerGrids.iter().position(|g| {
            g.owner == u.owner
                && ((wired && g.cells.contains(&u.pos))
                    || service.iter().any(|p| g.cells.contains(p)))
        })
    }

    pub fn recharge_energy(&mut self, dt: f64) {
        if !dt.is_finite() || dt <= 0. {
            return;
        }
        let mut budgets: Vec<f64> = self
            .state
            .powerGrids
            .iter()
            .map(|g| (g.output - g.load).max(0.) * dt)
            .collect();
        for index in 0..self.state.units.len() {
            let u = self.state.units[index].clone();
            if u.energyMax <= 0. || u.energy >= u.energyMax || u.moving || u.transitProgress > 0. {
                continue;
            }
            let grid = self.energy_recharge_grid(&u);
            if let Some(i) = grid {
                let charge = (u.energyMax - u.energy)
                    .min(35. * (1. + self.facility_bonus_for_unit(&u, "particle-foundry")) * dt)
                    .min(budgets[i]);
                self.state.units[index].energy += charge;
                budgets[i] -= charge;
                if charge > 0. {
                    if let Some(player) = self.player_mut(u.owner) {
                        *player.totals.entry("energy-recharged".into()).or_default() += charge;
                        *player
                            .totals
                            .entry("energy-charging-unit-seconds".into())
                            .or_default() += dt;
                    }
                }
            }
        }
    }
    pub fn spend_unit(&mut self, index: usize, credits: f64, compute: f64) -> Result<(), String> {
        let u = &self.state.units[index];
        let owner = u.owner;
        if self
            .player(owner)
            .map(|p| p.credits < credits)
            .unwrap_or(true)
        {
            return Err("金币不足".into());
        }
        if compute > 0. && !self.pay_attack(index, compute) {
            return Err("角色接入网络或随身算力不足".into());
        }
        self.spend(owner, credits, 0.)
    }
    pub fn networks(&mut self) {
        let _profile = crate::profiling::observe("network/total");
        let topology = self.network_layout();
        let old = std::mem::take(&mut self.state.networkStores);
        self.state.powerGrids.clear();
        for owner in 1..=2 {
            let topology = &topology.owners[(owner - 1) as usize];
            let power_groups = &topology.power_groups;
            let mut supply = vec![0.; power_groups.len()];
            let mut demand = vec![0.; power_groups.len()];
            let mut total = 0.;
            let mut load_total = 0.;
            for b in self.state.buildings.iter_mut().filter(|b| b.owner == owner) {
                let fueled = !matches!(b.kind.as_str(), "coal-power" | "nuclear-power")
                    || b.stock.get("fuel").copied().unwrap_or(0.) > 0.;
                b.powered = b.progress >= 1. && b.hp > 0. && b.jam <= 0. && fueled;
                let value = if b.powered { b.power } else { 0. };
                total += value;
                if let Some(g) = topology.power_ports.get(&b.id).copied() {
                    supply[g] += value;
                    if b.progress >= 1. {
                        demand[g] += b.demand;
                    }
                }
                if b.progress >= 1. {
                    load_total += b.demand;
                }
            }
            let gpu = catalog::gpus();
            for r in self
                .state
                .rooms
                .iter()
                .filter(|r| r.owner == owner && r.hp > 0. && r.progress >= 1.)
            {
                let load = catalog::facility_ref(&r.kind)
                    .map(|f| f.power * r.equipmentShare)
                    .unwrap_or(0.)
                    + r.gpus
                        .iter()
                        .filter_map(|m| gpu.iter().find(|g| &g.id == m))
                        .map(|g| g.power)
                        .sum::<f64>();
                load_total += load;
                if let Some(g) = topology.power_ports.get(&r.id).copied() {
                    demand[g] += load;
                }
            }
            let mut elevator_grids = Vec::new();
            for (id, candidates) in &topology.elevators {
                let grid = candidates.iter().copied().max_by(|a, b| {
                    (supply[*a] - demand[*a])
                        .total_cmp(&(supply[*b] - demand[*b]))
                        .then(b.cmp(a))
                });
                if let Some(i) = grid {
                    demand[i] += catalog::ELEVATOR_POWER_DEMAND;
                }
                load_total += catalog::ELEVATOR_POWER_DEMAND;
                elevator_grids.push((*id, grid));
            }
            for (id, grid) in elevator_grids {
                let powered = grid.is_some_and(|i| supply[i] > 0. && supply[i] + 0.01 >= demand[i]);
                if let Some(lift) = self.state.entrances.iter_mut().find(|e| e.id == id) {
                    lift.powered = powered;
                }
            }
            for (i, cells) in topology.power_cells.iter().enumerate() {
                self.state.powerGrids.push(PowerGrid {
                    owner,
                    cells: cells.clone(),
                    output: supply[i],
                    load: demand[i],
                });
            }
            for r in self.state.rooms.iter_mut().filter(|r| r.owner == owner) {
                r.powered = r.hp > 0.
                    && r.progress >= 1.
                    && topology
                        .power_ports
                        .get(&r.id)
                        .copied()
                        .map(|g| supply[g] > 0. && supply[g] + 0.01 >= demand[g])
                        .unwrap_or(false);
                r.connected = false;
            }
            for b in self
                .state
                .buildings
                .iter_mut()
                .filter(|b| b.owner == owner && b.demand > 0.)
            {
                b.powered = b.hp > 0.
                    && b.progress >= 1.
                    && topology
                        .power_ports
                        .get(&b.id)
                        .copied()
                        .map(|g| supply[g] > 0. && supply[g] + 0.01 >= demand[g])
                        .unwrap_or(false);
            }
            let mut sources = vec![None; self.state.rooms.len()];
            let mut standalone = Vec::new();
            let mut bare_standalone = Vec::new();
            for (i, r) in self.state.rooms.iter().enumerate().filter(|(_, r)| {
                r.owner == owner && r.kind == "data-center" && r.powered && !r.gpus.is_empty()
            }) {
                let mut capacity = 0.;
                let mut production = 0.;
                for model in &r.gpus {
                    if let Some(g) = gpu.iter().find(|g| &g.id == model) {
                        capacity += g.capacity;
                        production += g.rate;
                    }
                }
                sources[i] = Some((r.rect.center(), capacity, production));
                if !topology.compute_ports.contains_key(&r.id)
                    && port(&bare_standalone, r.rect).is_none()
                {
                    standalone.push(i);
                    bare_standalone.push(r.rect.cells().into_iter().collect());
                }
            }
            for i in 0..topology.compute_groups.len() {
                self.publish_compute_group(
                    owner,
                    &topology.compute_cells[i],
                    &topology.compute_rooms[i],
                    &sources,
                    &old,
                );
            }
            for i in standalone {
                self.publish_compute_group(
                    owner,
                    &topology.isolated_cells[&i],
                    &topology.isolated_rooms[&i],
                    &sources,
                    &old,
                );
            }
            let relay_connections: Vec<_> = self
                .state
                .buildings
                .iter()
                .filter(|b| b.owner == owner && b.kind == "mobile-relay")
                .map(|b| (b.id, self.relay_network(b).is_some()))
                .collect();
            for (id, connected) in relay_connections {
                if let Some(b) = self.state.buildings.iter_mut().find(|b| b.id == id) {
                    b.connected = connected;
                }
            }
            for l in self.state.links.iter_mut().filter(|l| l.owner == owner) {
                l.active = l.hp > 0.
                    && l.path
                        .windows(2)
                        .all(|e| valid_edge(e[0], e[1], &self.state.entrances, owner))
                    && if l.kind == "power" {
                        l.path
                            .first()
                            .and_then(|p| power_groups.iter().position(|g| g.contains(p)))
                            .map(|g| supply[g] > 0. && supply[g] >= demand[g])
                            .unwrap_or(false)
                    } else {
                        self.state
                            .networkStores
                            .iter()
                            .any(|s| s.owner == owner && l.path.iter().any(|p| s.cells.contains(p)))
                    };
            }
            let p = self.player_mut(owner).unwrap();
            p.power = total;
            p.demand = load_total;
        }
        for i in 0..self.state.units.len() {
            let owner = self.state.units[i].owner;
            let pos = self.state.units[i].pos;
            let unit_id = self.state.units[i].id;
            let is_ai =
                catalog::unit_ref(&self.state.units[i].kind).is_some_and(|d| d.category == "ai");
            let tether = is_ai
                && self.state.links.iter().any(|l| {
                    l.owner == owner
                        && l.kind == "compute"
                        && l.hp > 0.
                        && l.unitEndpoints.contains(&unit_id)
                        && (l.path.first() == Some(&pos) || l.path.last() == Some(&pos))
                });
            self.state.units[i].wired = tether;
            self.state.units[i].covered = self.unit_network(owner, pos).is_some();
            if tether {
                self.state.units[i].route.clear();
            }
        }
        self.summarize_compute();
        // Elevator power changes can occur after a path query within this tick.
        self.invalidate_navigation();
    }
    pub fn compute_network(&self, owner: u32, pos: Pos) -> Option<usize> {
        if let Some(index) = self
            .state
            .networkStores
            .iter()
            .position(|s| s.owner == owner && s.cells.contains(&pos))
        {
            return Some(index);
        }
        self.wireless_network(owner, pos)
    }
    fn relay_network(&self, relay: &Building) -> Option<usize> {
        if relay.kind != "mobile-relay"
            || relay.hp <= 0.
            || relay.progress < 1.
            || !relay.powered
            || relay.jam > 0.
        {
            return None;
        }
        // An actual cable connection takes precedence over radio backhaul.
        if let Some(index) =
            self.state.networkStores.iter().position(|s| {
                s.owner == relay.owner && s.cells.iter().any(|p| relay.rect.contains(*p))
            })
        {
            return Some(index);
        }
        // Radio attaches to a real live DC on this floor, never to an arbitrary
        // component anchor. A component can contain several far-apart DCs.
        self.state
            .rooms
            .iter()
            .filter(|r| {
                r.owner == relay.owner
                    && r.kind == "data-center"
                    && r.hp > 0.
                    && r.progress >= 1.
                    && r.powered
                    && !r.gpus.is_empty()
                    && r.rect.level == relay.rect.level
                    && r.rect.center().distance(relay.rect.center()) <= 30.
            })
            .filter_map(|r| {
                self.state
                    .networkStores
                    .iter()
                    .position(|s| s.owner == relay.owner && s.cells.contains(&r.rect.center()))
                    .map(|index| (r.rect.center().distance(relay.rect.center()), r.id, index))
            })
            .min_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)))
            .map(|(_, _, index)| index)
    }
    fn wireless_network(&self, owner: u32, pos: Pos) -> Option<usize> {
        let outdoor = self
            .state
            .buildings
            .iter()
            .filter(|b| {
                b.owner == owner
                    && b.kind == "mobile-relay"
                    && b.rect.level == pos.level
                    && b.rect.center().distance(pos) <= 18.
            })
            .filter_map(|b| {
                self.relay_network(b)
                    .map(|network| (b.rect.center().distance(pos), b.id, network))
            })
            .collect::<Vec<_>>();
        let indoor = self
            .state
            .rooms
            .iter()
            .filter(|r| {
                r.owner == owner
                    && r.kind == "wireless-relay"
                    && r.hp > 0.
                    && r.progress >= 1.
                    && r.powered
                    && r.connected
                    && r.online
                    && r.rect.level == pos.level
                    && r.rect.center().distance(pos) <= 12. * r.equipmentShare.sqrt()
            })
            .filter_map(|r| {
                self.state
                    .networkStores
                    .iter()
                    .position(|network| {
                        network.owner == owner && network.cells.contains(&r.rect.center())
                    })
                    .map(|index| (r.rect.center().distance(pos), r.id, index))
            });
        outdoor
            .into_iter()
            .chain(indoor)
            .min_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)))
            .map(|(_, _, index)| index)
    }
    pub fn unit_network(&self, owner: u32, pos: Pos) -> Option<usize> {
        let wired = self.state.links.iter().any(|l| {
            l.owner == owner
                && l.kind == "compute"
                && l.hp > 0.
                && (l.path.first() == Some(&pos) || l.path.last() == Some(&pos))
        });
        if wired {
            if let Some(i) = self
                .state
                .networkStores
                .iter()
                .position(|s| s.owner == owner && s.cells.contains(&pos))
            {
                return Some(i);
            }
        }
        self.wireless_network(owner, pos)
    }
    pub fn consume_unit_compute(&mut self, owner: u32, pos: Pos, amount: f64) -> bool {
        if !amount.is_finite() || amount < 0. {
            return false;
        }
        if let Some(i) = self.unit_network(owner, pos) {
            if self.state.networkStores[i].compute >= amount {
                self.state.networkStores[i].compute -= amount;
                return true;
            }
        }
        false
    }
    pub(crate) fn pay_unit_compute(&mut self, index: usize, amount: f64) -> bool {
        if !amount.is_finite() || amount < 0. {
            return false;
        }
        let Some(unit) = self.state.units.get(index) else {
            return false;
        };
        let network = self.unit_network(unit.owner, unit.pos);
        let available = network
            .map(|i| self.state.networkStores[i].compute)
            .unwrap_or(0.)
            .max(0.);
        if available + unit.battery + 1e-9 < amount {
            return false;
        }
        let supplied = available.min(amount);
        let cached = (amount - supplied).max(0.);
        if let Some(i) = network {
            self.state.networkStores[i].compute -= supplied;
        }
        self.state.units[index].battery = (self.state.units[index].battery - cached).max(0.);
        self.summarize_compute();
        true
    }
    pub fn consume_compute(&mut self, owner: u32, pos: Pos, amount: f64) -> bool {
        if !amount.is_finite() || amount < 0. {
            return false;
        }
        if let Some(i) = self.compute_network(owner, pos) {
            if self.state.networkStores[i].compute + 0.0001 >= amount {
                self.state.networkStores[i].compute =
                    (self.state.networkStores[i].compute - amount).max(0.);
                return true;
            }
        }
        false
    }
    pub fn spend_at(
        &mut self,
        owner: u32,
        credits: f64,
        compute: f64,
        pos: Pos,
    ) -> Result<(), String> {
        if self
            .player(owner)
            .map(|p| p.credits < credits)
            .unwrap_or(true)
        {
            return Err("金币不足".into());
        }
        if compute > 0. && !self.consume_compute(owner, pos, compute) {
            return Err("此接入网络的算力不足".into());
        }
        self.spend(owner, credits, 0.)
    }
    pub fn summarize_compute(&mut self) {
        for owner in 1..=2 {
            let values = self
                .state
                .networkStores
                .iter()
                .filter(|s| s.owner == owner)
                .fold((0., 0., 0.), |(a, b, c), s| {
                    (a + s.compute, b + s.capacity, c + s.production)
                });
            let p = self.player_mut(owner).unwrap();
            p.compute = values.0;
            p.computeCapacity = values.1;
            p.production = values.2;
        }
    }
    pub fn produce_compute(&mut self, dt: f64) {
        for s in &mut self.state.networkStores {
            s.compute = (s.compute + s.production * dt).min(s.capacity);
        }
        self.summarize_compute();
    }
    pub fn maintain_rooms(&mut self, dt: f64) {
        let rooms = self.state.rooms.clone();
        for room in rooms {
            let definition = match catalog::facility_ref(&room.kind) {
                Some(d) => d,
                None => continue,
            };
            let tier = if room.kind == "research-lab" {
                self.tech(room.owner, room.branch.as_deref().unwrap_or(""))
                    .clamp(1, 5)
            } else {
                room.tier
            };
            let maintenance = (if room.kind == "research-lab" {
                catalog::LAB_COMPUTE_UPKEEP[(tier - 1) as usize]
            } else {
                definition.upkeep_compute
            }) * room.equipmentShare;
            let online = room.hp > 0.
                && room.progress >= 1.
                && (definition.power <= 0. || room.powered)
                && (maintenance <= 0.
                    || room.connected
                        && self.consume_compute(room.owner, room.rect.center(), maintenance * dt));
            if let Some(r) = self.state.rooms.iter_mut().find(|r| r.id == room.id) {
                r.online = online;
                r.maintenance = maintenance;
                if r.kind == "research-lab" {
                    r.tier = tier;
                }
            }
            if online && maintenance > 0. {
                *self
                    .player_mut(room.owner)
                    .unwrap()
                    .totals
                    .entry("compute-maintenance".into())
                    .or_default() += maintenance * dt;
            }
        }
        self.summarize_compute();
    }
}
