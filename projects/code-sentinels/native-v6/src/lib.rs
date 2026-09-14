pub mod catalog;
pub mod profiling;
mod combat;
mod construction;
mod network;
mod logistics;
mod skills;
mod traits;
mod vision;
mod topology;
mod entrances;
mod ballistics;
mod replay;
mod validation;
mod destruction;
mod aviation;
mod navigation;
pub use navigation::supercover;
pub use replay::{apply_replay_orders,ReplayController};
mod themes;
mod bot;
pub mod types;
mod world;
use std::collections::BTreeMap;
pub use types::*;
pub const RULES_VERSION:&str="v6.2";
pub const RULES_FINGERPRINT:&str=env!("SENTINELS_V6_RULES_FINGERPRINT");
#[derive(Clone)]
pub struct Game {
    pub(crate) navigation:std::cell::RefCell<navigation::NavCache>,
    pub(crate) fog_cache:std::cell::RefCell<vision::FogCache>,
    pub(crate) collision_cache:std::cell::RefCell<ballistics::CollisionCache>,
    pub(crate) network_topology:std::cell::RefCell<network::NetworkCache>,
    pub administrative_events:Vec<AdministrativeEvent>,
    pub state: Snapshot,
    pub orders: Vec<LoggedOrder>,
    pub next_id: u64,
    pub sequences: [u64; 2],
    pub shield_auto: [bool; 2],
    pub initial_ai: bool,
    pub receipts: BTreeMap<(u32, u64), (String, Receipt)>,
}
impl Game {
    pub fn player(&self, owner: u32) -> Option<&Player> {
        self.state.players.iter().find(|p| p.owner == owner)
    }
    pub fn player_mut(&mut self, owner: u32) -> Option<&mut Player> {
        self.state.players.iter_mut().find(|p| p.owner == owner)
    }
    pub fn id(&mut self) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        id
    }
    pub fn event(&mut self, kind: &str, pos: Pos, owner: u32, magnitude: f64, subject: u64) {
        let _timing=profiling::observe("world/events");
        if let Some(unit)=self.state.units.iter_mut().find(|u|u.id==subject){if kind=="fire"{unit.lastAttackTick=Some(self.state.tick);}if kind.starts_with("skill-"){unit.lastCastTick=Some(self.state.tick);}if magnitude>0.&&matches!(kind,"impact"|"energy-impact"|"explosive-impact"){unit.lastHitTick=Some(self.state.tick);}}
        let accounting=match kind{"fire"=>Some(("fire",1.)),"ore-delivered"|"ammo-spent"|"compute-spent"|"energy-spent"|"transport-spent"=>Some((kind,magnitude)),"impact"|"energy-impact"|"explosive-impact"=>Some(("damage-dealt",magnitude)),_=>None};
        if let Some((key,value))=accounting{if let Some(player)=self.player_mut(owner){*player.totals.entry(key.into()).or_default()+=value;}}
        let id = self.id();
        let visual=self.state.units.iter().find(|u|u.id==subject).map(|u|(u.kind.clone(),[u.x,u.y,u.elevation()+0.1],u.facing));
        self.state.events.push(Event {
            id,
            tick: self.state.tick,
            kind: kind.into(),
            pos,
            owner,
            magnitude,
            subject,
            direction:None,
            subjectKind:visual.as_ref().map(|v|v.0.clone()).unwrap_or_default(),
            rect:None,
            presentationPosition:visual.as_ref().map(|v|v.1),
            facing:visual.map(|v|v.2),
        });
        if self.state.events.len() > 4096 {
            self.state.events.remove(0);
        }
    }
    pub fn spend(&mut self, owner: u32, credits: f64, compute: f64) -> Result<(), String> {
        let p = self.player_mut(owner).ok_or("玩家不存在")?;
        if !credits.is_finite() || !compute.is_finite() || credits < 0. || compute < 0. {
            return Err("资源参数无效".into());
        }
        if p.credits + 0.001 < credits {
            return Err("金币不足".into());
        }
        if p.compute + 0.001 < compute {
            return Err("算力不足".into());
        }
        p.credits = (p.credits - credits).max(0.);
        p.compute = (p.compute - compute).max(0.);
        Ok(())
    }
    pub fn tech(&self, owner: u32, branch: &str) -> u32 {
        self.player(owner)
            .map(|p| {
                if branch.is_empty() {
                    p.branches.values().copied().max().unwrap_or(1)
                } else {
                    p.branches.get(branch).copied().unwrap_or(0)
                }
            })
            .unwrap_or(0)
    }
    pub fn order(&mut self, order: Order) -> Receipt {
        let key = (order.owner, order.sequence);
        let payload = serde_json::to_string(&order.command).unwrap_or_default();
        if let Some((old, r)) = self.receipts.get(&key) {
            return if old == &payload {
                r.clone()
            } else {
                Receipt {
                    accepted: false,
                    sequence: order.sequence,
                    tick: self.state.tick,
                    reason: "相同序号不能修改指令".into(),
                }
            };
        }
        let valid = (1..=2).contains(&order.owner)
            && order.sequence
                == self.sequences[(order.owner.saturating_sub(1).min(1)) as usize] + 1;
        let result = if !valid {
            Err("指令序号无效".into())
        } else if self.state.winner.is_some() {
            Err("战斗已经结束".into())
        } else {
            self.execute(order.owner, order.command.clone())
        };
        let receipt = Receipt {
            accepted: result.is_ok(),
            sequence: order.sequence,
            tick: self.state.tick,
            reason: result.err().unwrap_or_else(|| "指令已执行".into()),
        };
        if valid {
            self.sequences[(order.owner - 1) as usize] = order.sequence;
            self.receipts.insert(key, (payload, receipt.clone()));
            self.orders.push(LoggedOrder {
                tick: self.state.tick,
                order,
                receipt: receipt.clone(),
            });
            self.state.revision += 1;
        }
        receipt
    }
    pub fn save(&self) -> Save {
        let mut snapshot=self.state.clone();snapshot.shieldAuto=self.shield_auto;
        for room in &mut snapshot.rooms {
            crate::construction::fill_room_capacity_metadata(room, &snapshot.entrances);
        }
        Save {
            rulesVersion:RULES_VERSION.into(),
            rulesFingerprint:RULES_FINGERPRINT.into(),
            administrativeEvents:self.administrative_events.clone(),
            snapshot,
            orders: self.orders.clone(),
            nextId: self.next_id,
            sequences: self.sequences,
            shieldAuto: self.shield_auto,
            initialAi: self.initial_ai,
        }
    }
    pub fn load(mut save: Save) -> Result<Self, String> {
        save.snapshot.shieldAuto=save.shieldAuto;
        save.validate()?;
        if save.snapshot.version != 6
            || save.snapshot.terrain.len() != 128 * 96
            || save.snapshot.players.len() != 2
        {
            return Err("存档版本或数据不完整".into());
        }
        let receipts = save
            .orders
            .iter()
            .map(|l| {
                (
                    (l.order.owner, l.order.sequence),
                    (
                        serde_json::to_string(&l.order.command).unwrap_or_default(),
                        l.receipt.clone(),
                    ),
                )
            })
            .collect();
        let mut game = Self {
            navigation:navigation::cache(),
            fog_cache:Default::default(),
            collision_cache:Default::default(),
            network_topology:network::cache(),
            administrative_events:save.administrativeEvents,
            state: save.snapshot,
            orders: save.orders,
            next_id: save.nextId,
            sequences: save.sequences,
            shield_auto: save.shieldAuto,
            initial_ai: save.initialAi,
            receipts,
        };
        game.refresh_room_capacities();
        Ok(game)
    }
    pub fn forfeit(&mut self, owner: u32, reason: String) -> Result<(), String> {
        if !(1..=2).contains(&owner) {
            return Err("玩家无效".into());
        }
        if self.state.winner.is_none() {
            self.administrative_events.push(AdministrativeEvent{tick:self.state.tick,owner,reason:reason.clone(),orderIndex:self.orders.len()});
            self.state.winner = Some(3 - owner);
            self.state.winReason = reason;
            self.state.revision += 1;
        }
        Ok(())
    }
    pub fn replay(save:&Save)->Result<bool,String>{save.validate()?;let mut g=Game::new_theme(save.snapshot.seed,save.initialAi,&save.snapshot.theme);let(mut index,mut admin)=(0,0);loop{apply_replay_orders(&mut g,save,&mut index,&mut admin);if g.state.tick==save.snapshot.tick{break;}if g.state.winner.is_some()||g.state.tick>save.snapshot.tick{return Ok(false);}g.step();}Ok(serde_json::to_value(&g.state).map_err(|e|e.to_string())?==serde_json::to_value(&save.snapshot).map_err(|e|e.to_string())?)}
}


