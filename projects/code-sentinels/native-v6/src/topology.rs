use crate::{types::*, Game};
use std::collections::{BTreeSet, VecDeque};
impl Game {
    pub fn topology_charge(&mut self) {
        let old = std::mem::take(&mut self.state.shieldRegions);
        for owner in 1..=2 {
            let mut protected = BTreeSet::new();
            let mut protected_walls = BTreeSet::new();
            for z in -2..=5 {
                let mut barriers: BTreeSet<Pos> = self
                    .state
                    .walls
                    .iter()
                    .filter(|w| {
                        w.owner == owner && w.hp > 0. && w.pos.level == z && w.kind != "physical"
                    })
                    .map(|w| w.pos)
                    .collect();
                for door in self.state.entrances.iter().filter(|e| {
                    e.owner == owner && e.hp > 0. && !e.open && e.kind == "door" && e.pos.level == z
                }) {
                    for i in 0..door.width {
                        let p = if door.axis == "y" {
                            Pos::new(door.pos.x, door.pos.y + i as i32, z)
                        } else {
                            Pos::new(door.pos.x + i as i32, door.pos.y, z)
                        };
                        barriers.insert(p);
                    }
                }
                if barriers.len() < 8 {
                    continue;
                }
                let minx = barriers.iter().map(|p| p.x).min().unwrap();
                let maxx = barriers.iter().map(|p| p.x).max().unwrap();
                let miny = barriers.iter().map(|p| p.y).min().unwrap();
                let maxy = barriers.iter().map(|p| p.y).max().unwrap();
                let mut seen = BTreeSet::new();
                for y in miny..=maxy {
                    for x in minx..=maxx {
                        let start = Pos::new(x, y, z);
                        if barriers.contains(&start) || !seen.insert(start) {
                            continue;
                        }
                        let mut queue = VecDeque::from([start]);
                        let mut region = BTreeSet::from([start]);
                        let mut open = false;
                        while let Some(p) = queue.pop_front() {
                            if p.x == minx || p.x == maxx || p.y == miny || p.y == maxy {
                                open = true;
                            }
                            for n in [
                                Pos::new(p.x + 1, p.y, z),
                                Pos::new(p.x - 1, p.y, z),
                                Pos::new(p.x, p.y + 1, z),
                                Pos::new(p.x, p.y - 1, z),
                            ] {
                                if n.x >= minx
                                    && n.x <= maxx
                                    && n.y >= miny
                                    && n.y <= maxy
                                    && !barriers.contains(&n)
                                    && seen.insert(n)
                                {
                                    region.insert(n);
                                    queue.push_back(n);
                                }
                            }
                        }
                        if open
                            || self.state.entrances.iter().any(|e| {
                                e.hp > 0.
                                    && e.open
                                    && e.toLevel != e.pos.level
                                    && e.serves(z)
                                    && region.iter().any(|p| e.covers(*p))
                            })
                        {
                            continue;
                        }
                        let center = self
                            .state
                            .rooms
                            .iter()
                            .find(|r| {
                                r.owner == owner
                                    && r.kind == "data-center"
                                    && r.powered
                                    && r.connected
                                    && region.contains(&r.rect.center())
                            })
                            .map(|r| r.rect.center());
                        let Some(center) = center else {
                            continue;
                        };
                        let Some(network) = self.compute_network(owner, center) else {
                            continue;
                        };
                        let capacity = (region.len() as f64 * 12.).min(6000.);
                        let anchor = *region.iter().next().unwrap();
                        let mut current = old
                            .iter()
                            .filter(|r| r.owner == owner && region.contains(&r.anchor))
                            .map(|r| r.current)
                            .sum::<f64>()
                            .min(capacity);
                        let available = if self.shield_auto[(owner - 1) as usize] {
                            self.state.networkStores[network]
                                .compute
                                .min(20.)
                                .min((capacity - current) / 1.5)
                        } else {
                            0.
                        };
                        self.state.networkStores[network].compute -= available;
                        current += available * 1.5;
                        self.state.shieldRegions.push(ShieldRegion {
                            owner,
                            anchor,
                            cells: region.clone(),
                            current,
                            capacity,
                            network: self.state.networkStores[network].anchor,
                        });
                        let buildings: Vec<u64> = self
                            .state
                            .buildings
                            .iter()
                            .filter(|b| b.owner == owner && region.contains(&b.rect.center()))
                            .map(|b| b.id)
                            .collect();
                        let walls: Vec<u64> = self
                            .state
                            .walls
                            .iter()
                            .filter(|w| {
                                w.owner == owner
                                    && w.pos.level == z
                                    && w.kind != "physical"
                                    && [
                                        Pos::new(w.pos.x + 1, w.pos.y, z),
                                        Pos::new(w.pos.x - 1, w.pos.y, z),
                                        Pos::new(w.pos.x, w.pos.y + 1, z),
                                        Pos::new(w.pos.x, w.pos.y - 1, z),
                                    ]
                                    .iter()
                                    .any(|p| region.contains(p))
                            })
                            .map(|w| w.id)
                            .collect();
                        for b in self
                            .state
                            .buildings
                            .iter_mut()
                            .filter(|b| buildings.contains(&b.id))
                        {
                            b.shield = current;
                            protected.insert(b.id);
                        }
                        for w in self
                            .state
                            .walls
                            .iter_mut()
                            .filter(|w| walls.contains(&w.id))
                        {
                            w.shield = current;
                            protected_walls.insert(w.id);
                        }
                    }
                }
            }
            for b in self
                .state
                .buildings
                .iter_mut()
                .filter(|b| b.owner == owner && !protected.contains(&b.id))
            {
                b.shield = 0.;
            }
            for w in self
                .state
                .walls
                .iter_mut()
                .filter(|w| w.owner == owner && !protected_walls.contains(&w.id))
            {
                w.shield = 0.;
            }
        }
    }
}
