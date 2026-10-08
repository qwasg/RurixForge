use crate::{types::*, Game};
use std::collections::BTreeSet;
impl Entrance {
    pub fn serves(&self, level: i32) -> bool {
        if self.kind == "elevator" {
            (self.pos.level.min(self.toLevel)..=self.pos.level.max(self.toLevel)).contains(&level)
        } else {
            level == self.pos.level || level == self.toLevel
        }
    }
    pub fn footprint_contains(&self, x: i32, y: i32) -> bool {
        if self.axis == "y" {
            x == self.pos.x && y >= self.pos.y && y < self.pos.y + self.width as i32
        } else {
            y == self.pos.y && x >= self.pos.x && x < self.pos.x + self.width as i32
        }
    }
    pub fn covers(&self, p: Pos) -> bool {
        self.serves(p.level) && self.footprint_contains(p.x, p.y)
    }
    pub fn reserves_space(&self) -> bool {
        self.hp > 0. || self.kind == "door" && self.hp == 0.
    }
    pub fn cells(&self) -> Vec<Pos> {
        (self.pos.level.min(self.toLevel)..=self.pos.level.max(self.toLevel))
            .filter(|z| self.serves(*z))
            .flat_map(|z| {
                (0..self.width).map(move |i| {
                    Pos::new(
                        self.pos.x + if self.axis == "y" { 0 } else { i as i32 },
                        self.pos.y + if self.axis == "y" { i as i32 } else { 0 },
                        z,
                    )
                })
            })
            .collect()
    }
}
impl Game {
    pub fn build_entrance(
        &mut self,
        owner: u32,
        pos: Pos,
        to: i32,
        kind: String,
        width: u32,
    ) -> Result<(), String> {
        if !(1..=2).contains(&owner)
            || !pos.valid()
            || !(-2..=5).contains(&to)
            || !(1..=4).contains(&width)
            || !matches!(
                kind.as_str(),
                "door" | "window" | "stairs" | "elevator" | "ramp"
            )
        {
            return Err("入口参数无效".into());
        }
        if matches!(kind.as_str(), "door" | "window") && to != pos.level
            || matches!(kind.as_str(), "stairs" | "ramp") && (to - pos.level).abs() != 1
            || kind == "ramp" && width < 3
            || kind == "elevator" && to == pos.level
        {
            return Err("门窗须同层；竖井须连接不同楼层，坡道至少3格宽".into());
        }
        let shell =
            self.state.buildings.iter().find(|b| {
                b.owner == owner && b.hp > 0. && b.kind == "shell" && b.rect.contains(pos)
            });
        let marker = self
            .state
            .entrances
            .iter()
            .find(|e| e.owner == owner && e.kind == "door" && e.hp <= 0. && e.covers(pos));
        let adjoining = self
            .state
            .walls
            .iter()
            .filter(|w| {
                w.owner == owner
                    && w.hp > 0.
                    && w.pos.level == pos.level
                    && w.pos.distance(pos) <= 1.
            })
            .count();
        if shell.is_none() && !(kind == "door" && (adjoining >= 2 || marker.is_some())) {
            return Err("入口须连接己方楼体、围墙或原有门洞".into());
        }
        let axis = if let Some(b) = shell {
            if pos.x == b.rect.x || pos.x == b.rect.x + b.rect.width - 1 {
                "y"
            } else {
                "x"
            }
        } else if let Some(marker) = marker {
            if marker.axis == "y" {
                "y"
            } else {
                "x"
            }
        } else {
            let x = self
                .state
                .walls
                .iter()
                .filter(|w| {
                    w.owner == owner
                        && w.hp > 0.
                        && w.pos.level == pos.level
                        && w.pos.y == pos.y
                        && (w.pos.x - pos.x).abs() == 1
                })
                .count();
            let y = self
                .state
                .walls
                .iter()
                .filter(|w| {
                    w.owner == owner
                        && w.hp > 0.
                        && w.pos.level == pos.level
                        && w.pos.x == pos.x
                        && (w.pos.y - pos.y).abs() == 1
                })
                .count();
            if y > x {
                "y"
            } else {
                "x"
            }
        };
        let entry = Entrance {
            id: 0,
            owner,
            pos,
            toLevel: to,
            kind: kind.clone(),
            hp: 250.,
            open: true,
            powered: kind != "elevator",
            width,
            axis: axis.into(),
        };
        let cells = entry.cells();
        if cells.iter().any(|p| {
            !p.valid() || shell.is_some_and(|b| p.level == pos.level && !b.rect.contains(*p))
        }) {
            return Err("入口宽度超出地图或所属楼体".into());
        }
        if to != pos.level
            && cells.iter().any(|p| {
                !self.state.buildings.iter().any(|b| {
                    b.owner == owner && b.kind == "shell" && b.hp > 0. && b.rect.contains(*p)
                })
            })
        {
            return Err("竖井须在每个经过楼层有完整占地".into());
        }
        let mut replace = BTreeSet::new();
        for old in &self.state.entrances {
            if !cells.iter().any(|p| old.covers(*p)) {
                continue;
            }
            if old.hp > 0. {
                return Err("入口范围与已有入口重叠".into());
            }
            if kind != "door" || old.kind != "door" || !old.cells().iter().all(|p| entry.covers(*p))
            {
                return Err("新门必须完整覆盖旧门洞，不能只重建部分宽度".into());
            }
            replace.insert(old.id);
        }
        if kind == "door"
            && self
                .state
                .walls
                .iter()
                .any(|w| w.hp > 0. && w.owner != owner && entry.covers(w.pos))
        {
            return Err("不能改造敌方墙体".into());
        }
        let prospective_entrances: Vec<_> = self.state.entrances.iter()
            .filter(|old| !replace.contains(&old.id)).cloned()
            .chain(std::iter::once(entry.clone())).collect();
        for room in self
            .state
            .rooms
            .iter()
            .filter(|r| r.owner == owner && r.kind == "data-center")
        {
            let potential = crate::construction::room_geometry_capacity(
                &room.kind, room.rect, &prospective_entrances);
            if room.capacity_budget().min(potential) < room.gpus.len() as u32 {
                return Err("入口占用机架净面积，请先移除显卡".into());
            }
        }
        self.spend(
            owner,
            if kind == "elevator" {
                220.
            } else {
                width as f64 * 30.
            },
            0.,
        )?;
        if kind == "door" {
            self.state
                .walls
                .retain(|w| !(w.owner == owner && entry.covers(w.pos)));
        }
        self.state.entrances.retain(|e| !replace.contains(&e.id));
        let id = self.id();
        self.state.entrances.push(Entrance { id, ..entry });
        self.refresh_room_capacities();
        self.toggle_entrance(owner, id, true)?;
        self.invalidate_navigation();
        self.event("entrance", pos, owner, 0., id);
        Ok(())
    }
    pub fn toggle_entrance(&mut self, owner: u32, id: u64, open: bool) -> Result<(), String> {
        let entry = self
            .state
            .entrances
            .iter()
            .find(|e| e.id == id && e.owner == owner && e.hp > 0.)
            .ok_or("没有己方完好入口；损毁门洞需要重建")?;
        let pos = entry.pos;
        let to = entry.toLevel;
        let width = entry.width;
        if !open
            && (self
                .state
                .units
                .iter()
                .any(|u| u.hp > 0. && u.altitude < 0.5 && entry.covers(u.pos))
                || self.state.shipments.iter().any(|s| {
                    s.hp > 0.
                        && s.altitude < 0.5
                        && (entry.covers(s.pos)
                            || s.progress > 0. && s.route.first().is_some_and(|p| entry.covers(*p)))
                }))
        {
            return Err("入口有单位或运输车，不能关门夹断".into());
        }
        self.state
            .entrances
            .iter_mut()
            .find(|e| e.id == id)
            .unwrap()
            .open = open;
        if open {
            self.state.shieldRegions.retain(|r| {
                r.owner != owner
                    || !r.cells.iter().any(|p| {
                        (p.level == pos.level || p.level == to)
                            && (p.x - pos.x).abs() + (p.y - pos.y).abs() <= width as i32 + 1
                    })
            });
        }
        self.invalidate_navigation();
        Ok(())
    }
}
