#![allow(non_snake_case)]
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

/// Chronological event sequence with the same JSON array format as previous saves.
/// Native ring eviction must not move every large Event payload for every hit.
#[derive(Debug,Clone,Default,Serialize,Deserialize)]
#[serde(transparent)]
pub struct EventLog(std::collections::VecDeque<Event>);
impl EventLog {
    pub fn push(&mut self,event:Event){self.0.push_back(event);}
    pub fn last(&self)->Option<&Event>{self.0.back()}
    pub fn last_mut(&mut self)->Option<&mut Event>{self.0.back_mut()}
    pub fn first(&self)->Option<&Event>{self.0.front()}
    pub fn remove(&mut self,index:usize)->Event{self.0.remove(index).expect("event index out of bounds")}
}
impl std::ops::Deref for EventLog{type Target=std::collections::VecDeque<Event>;fn deref(&self)->&Self::Target{&self.0}}
impl std::ops::DerefMut for EventLog{fn deref_mut(&mut self)->&mut Self::Target{&mut self.0}}
impl From<Vec<Event>> for EventLog{fn from(events:Vec<Event>)->Self{Self(events.into())}}
impl FromIterator<Event> for EventLog{fn from_iter<T:IntoIterator<Item=Event>>(events:T)->Self{Self(events.into_iter().collect())}}
impl Extend<Event> for EventLog{fn extend<T:IntoIterator<Item=Event>>(&mut self,events:T){self.0.extend(events);}}
impl IntoIterator for EventLog{type Item=Event;type IntoIter=std::collections::vec_deque::IntoIter<Event>;fn into_iter(self)->Self::IntoIter{self.0.into_iter()}}
impl<'a> IntoIterator for &'a EventLog{type Item=&'a Event;type IntoIter=std::collections::vec_deque::Iter<'a,Event>;fn into_iter(self)->Self::IntoIter{self.0.iter()}}
impl<'a> IntoIterator for &'a mut EventLog{type Item=&'a mut Event;type IntoIter=std::collections::vec_deque::IterMut<'a,Event>;fn into_iter(self)->Self::IntoIter{self.0.iter_mut()}}

#[derive(
    Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, Hash,
)]
pub struct Pos {
    pub x: i32,
    pub y: i32,
    #[serde(rename = "z")]
    pub level: i32,
}
impl Pos {
    pub fn new(x: i32, y: i32, level: i32) -> Self {
        Self { x, y, level }
    }
    pub fn valid(self) -> bool {
        self.x >= 0 && self.x < 128 && self.y >= 0 && self.y < 96 && self.level == 0
    }
    pub fn distance(self, b: Self) -> f64 {
        (((self.x - b.x).pow(2) + (self.y - b.y).pow(2)) as f64).sqrt()
    }
}
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    #[serde(rename = "z")]
    pub level: i32,
    #[serde(rename = "w")]
    pub width: i32,
    #[serde(rename = "h")]
    pub height: i32,
}
impl Rect {
    pub fn area(self) -> i32 {
        self.width * self.height
    }
    pub fn pos(self) -> Pos {
        Pos::new(self.x, self.y, self.level)
    }
    pub fn center(self) -> Pos {
        Pos::new(
            self.x + self.width / 2,
            self.y + self.height / 2,
            self.level,
        )
    }
    pub fn contains(self, p: Pos) -> bool {
        p.level == self.level
            && p.x >= self.x
            && p.y >= self.y
            && p.x < self.x + self.width
            && p.y < self.y + self.height
    }
    pub fn cells(self) -> Vec<Pos> {
        (self.y..self.y + self.height)
            .flat_map(|y| (self.x..self.x + self.width).map(move |x| Pos::new(x, y, self.level)))
            .collect()
    }
    pub fn valid(self) -> bool {
        self.width >= 1
            && self.height >= 1
            && self.width <= 24
            && self.height <= 24
            && self.level == 0
            && self.pos().valid()
            && Pos::new(
                self.x + self.width - 1,
                self.y + self.height - 1,
                self.level,
            )
            .valid()
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Player {
    pub owner: u32,
    pub credits: f64,
    pub compute: f64,
    pub computeCapacity: f64,
    pub power: f64,
    pub demand: f64,
    pub income: f64,
    pub production: f64,
    pub branches: BTreeMap<String, u32>,
    pub research: Option<Research>,
    #[serde(default)]
    pub researches: Vec<Research>,
    #[serde(default)]
    pub science: f64,
    pub dominance: f64,
    pub ai: bool,
    pub lostValue: f64,
    #[serde(default)]
    pub totals: BTreeMap<String, f64>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Research {
    pub lab: u64,
    pub branch: String,
    pub target: u32,
    pub progress: f64,
    pub duration: f64,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Building {
    pub id: u64,
    pub owner: u32,
    pub rect: Rect,
    pub kind: String,
    pub tier: u32,
    pub hp: f64,
    #[serde(default)]
    pub antiHeal: f64,
    pub maxHp: f64,
    pub progress: f64,
    pub buildTime: f64,
    pub powered: bool,
    pub connected: bool,
    pub power: f64,
    pub demand: f64,
    pub capacity: u32,
    pub branch: Option<String>,
    pub inventory: f64,
    pub invested: f64,
    pub jam: f64,
    pub shield: f64,
    pub born: u64,
    #[serde(default)]
    pub collapseWarning: f64,
    #[serde(default = "one")]
    pub supportRatio: f64,
    #[serde(default)]
    pub stock: BTreeMap<String, f64>,
}
fn one() -> f64 {
    1.
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Room {
    pub id: u64,
    pub shell: u64,
    pub owner: u32,
    pub rect: Rect,
    pub kind: String,
    #[serde(default = "one")]
    pub equipmentShare: f64,
    pub branch: Option<String>,
    pub tier: u32,
    pub hp: f64,
    #[serde(default)]
    pub antiHeal: f64,
    pub maxHp: f64,
    pub powered: bool,
    pub connected: bool,
    #[serde(default)]
    pub online: bool,
    #[serde(default)]
    pub maintenance: f64,
    pub capacity: u32,
    /// Purchased integer capacity survives partitioning and temporary obstructions.
    /// Older explicit fixtures without this metadata are limited to their stored capacity.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub capacityBudget: Option<u32>,
    /// Read-only native cache of the current geometry limit; never a purchase budget.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub potentialCapacity: Option<u32>,
    pub gpus: Vec<String>,
    pub inventory: f64,
    pub progress: f64,
    pub buildTime: f64,
    pub cooldown: f64,
    #[serde(default)]
    pub stock: BTreeMap<String, f64>,
    #[serde(default)]
    pub invested: f64,
}
impl Room {
    pub fn capacity_budget(&self) -> u32 {
        self.capacityBudget.unwrap_or(self.capacity)
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Entrance {
    pub id: u64,
    pub owner: u32,
    pub pos: Pos,
    pub toLevel: i32,
    pub kind: String,
    pub hp: f64,
    #[serde(default = "default_open")]
    pub open: bool,
    #[serde(default)]
    pub powered: bool,
    #[serde(default = "default_width")]
    pub width: u32,
    #[serde(default)]
    pub axis: String,
}
fn default_open() -> bool {
    true
}
fn default_width() -> u32 {
    1
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Unit {
    pub id: u64,
    pub owner: u32,
    pub kind: String,
    pub pos: Pos,
    pub x: f64,
    pub y: f64,
    #[serde(default)] pub velocityX:f64,
    #[serde(default)] pub velocityY:f64,
    #[serde(rename = "z")]
    pub level: i32,
    pub tier: u32,
    pub hp: f64,
    pub maxHp: f64,
    pub battery: f64,
    pub batteryMax: f64,
    pub covered: bool,
    pub wired: bool,
    pub ammo: f64,
    pub route: Vec<Pos>,
    pub target: Option<u64>,
    pub cooldown: f64,
    pub skillCooldown: f64,
    pub plugins: Vec<String>,
    #[serde(default)] pub pluginDiscount:f64,
    #[serde(default = "one")] pub chargedShotMultiplier:f64,
    pub statuses: BTreeMap<String, f64>,
    pub invested: f64,
    pub moving: bool,
    pub attackCount: u64,
    #[serde(default)]
    pub fuel: f64,
    #[serde(default)]
    pub fuelMax: f64,
    #[serde(default)]
    pub ammoMax: f64,
    #[serde(default)]
    pub branch: String,
    #[serde(default)]
    pub facing: u32,
    #[serde(default)]
    pub energy: f64,
    #[serde(default)]
    pub energyMax: f64,
    #[serde(default)]
    pub altitude: f64,
    #[serde(default)]
    pub flightState: String,
    #[serde(default)]
    pub sourceFacility: u64,
    #[serde(default)]
    pub sortieTarget: Option<Pos>,
    #[serde(default)]
    pub goal: Option<Pos>,
    #[serde(default)]
    pub queuedGoals: Vec<Pos>,
    #[serde(default)]
    pub transitProgress: f64,
    #[serde(default)]
    pub lastAttackTick: Option<u64>,
    #[serde(default)]
    pub lastCastTick: Option<u64>,
    #[serde(default)]
    pub lastHitTick: Option<u64>,
    #[serde(default)]
    pub dash: Option<DashState>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DashState {
    pub previous: [f64; 2],
    pub hitTargets: Vec<u64>,
    pub remaining: f64,
    pub damage: f64,
    pub width: f64,
}
impl Unit {
    pub fn elevation(&self) -> f64 {
        self.level as f64 + self.altitude
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Link {
    pub id: u64,
    pub owner: u32,
    pub kind: String,
    pub path: Vec<Pos>,
    pub hp: f64,
    pub active: bool,
    #[serde(default)]
    pub unitEndpoints:Vec<u64>,
    #[serde(default)]
    pub invested: f64,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Wall {
    pub id: u64,
    pub owner: u32,
    pub pos: Pos,
    pub kind: String,
    pub hp: f64,
    #[serde(default)]
    pub antiHeal: f64,
    pub maxHp: f64,
    pub shield: f64,
    #[serde(default)]
    pub invested: f64,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Resource {
    pub id: u64,
    pub pos: Pos,
    pub kind: String,
    pub remaining: f64,
    pub owner: u32,
    pub capture: f64,
    pub contested: bool,
    #[serde(default)]
    pub capturer: u32,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Shipment {
    pub id: u64,
    pub owner: u32,
    pub from: u64,
    pub to: u64,
    pub pos: Pos,
    pub route: Vec<Pos>,
    pub amount: f64,
    pub hp: f64,
    pub progress: f64,
    #[serde(default)] pub unloadProgress:f64,
    pub cargo: String,
    #[serde(default)]
    pub waypoints: Vec<Pos>,
    #[serde(default)]
    pub manualRoute: bool,
    #[serde(default = "default_ground")]
    pub mode: String,
    #[serde(default)]
    pub altitude: f64,
    #[serde(default = "default_ground")]
    pub flightState: String,
    #[serde(default)]
    pub fuel: f64,
    #[serde(default)]
    pub fuelMax: f64,
    #[serde(default)]
    pub flightTimer: f64,
}
fn default_ground() -> String {
    "ground".into()
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Projectile {
    pub id: u64,
    pub owner: u32,
    pub source: u64,
    pub target: Option<u64>,
    pub origin: Pos,
    pub destination: Pos,
    pub x: f64,
    pub y: f64,
    pub z: f64,
    pub age: f64,
    pub duration: f64,
    pub damage: f64,
    pub radius: f64,
    pub kind: String,
    pub damageType: String,
    pub penetration: f64,
    #[serde(default = "one")]
    pub structureMultiplier: f64,
    #[serde(default)]
    pub sourceAltitude: f64,
    #[serde(default)]
    pub targetAltitude: f64,
    #[serde(default)]
    pub jammed: bool,
    #[serde(default)]
    pub launchPosition: Option<[f64; 3]>,
    #[serde(default)]
    pub aimPosition: Option<[f64; 3]>,
    #[serde(default)]
    pub passedSurfaces: Vec<[u64; 2]>,
    #[serde(default)]
    pub onHitStatus: Option<HitStatus>,
    #[serde(default = "one")]
    pub targetMultiplier: f64,
    #[serde(default = "one")] pub movingTargetMultiplier:f64,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HitStatus {
    pub kind: String,
    pub duration: f64,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Event {
    pub id: u64,
    pub tick: u64,
    pub kind: String,
    pub pos: Pos,
    pub owner: u32,
    pub magnitude: f64,
    pub subject: u64,
    #[serde(default)]
    pub direction: Option<Pos>,
    #[serde(default)]
    pub subjectKind: String,
    #[serde(default)]
    pub rect: Option<Rect>,
    #[serde(default)]
    pub presentationPosition: Option<[f64; 3]>,
    #[serde(default)]
    pub facing: Option<u32>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Snapshot {
    pub version: u32,
    pub revision: u64,
    pub tick: u64,
    pub seed: u64,
    #[serde(default = "default_theme")]
    pub theme: String,
    #[serde(default = "default_ruleset")]
    pub ruleset: String,
    pub width: u32,
    pub height: u32,
    pub minLevel: i32,
    pub maxLevel: i32,
    pub players: Vec<Player>,
    pub terrain: Vec<u8>,
    pub excavated: BTreeSet<Pos>,
    #[serde(default)]
    pub excavationOwners: BTreeMap<u32, u32>,
    pub buildings: Vec<Building>,
    pub rooms: Vec<Room>,
    pub entrances: Vec<Entrance>,
    pub units: Vec<Unit>,
    pub links: Vec<Link>,
    pub walls: Vec<Wall>,
    pub resources: Vec<Resource>,
    pub shipments: Vec<Shipment>,
    pub projectiles: Vec<Projectile>,
    pub events: EventLog,
    pub winner: Option<u32>,
    pub winReason: String,
    #[serde(default = "default_shields")]
    pub shieldAuto: [bool; 2],
    pub explored: Vec<BTreeSet<Pos>>,
    pub visible: Vec<BTreeSet<Pos>>,
    #[serde(default)]
    pub jobs: Vec<ConstructionJob>,
    #[serde(default)]
    pub rubble: Vec<Rubble>,
    #[serde(default)]
    pub networkStores: Vec<NetworkStore>,
    #[serde(default)]
    pub defenseFields: Vec<DefenseField>,
    #[serde(default)]
    pub shieldRegions: Vec<ShieldRegion>,
    #[serde(default)]
    pub powerGrids: Vec<PowerGrid>,
    #[serde(default)]
    pub playback: Option<PlaybackStatus>,
}
fn default_theme() -> String {
    "river".into()
}
pub const RULESET_FULL: &str = "full";
pub const RULESET_CLASSIC: &str = "classic";
fn default_ruleset() -> String {
    RULESET_FULL.into()
}
fn default_shields() -> [bool; 2] {
    [true, true]
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DefenseField {
    pub id: u64,
    pub owner: u32,
    pub pos: Pos,
    pub direction: Pos,
    pub radius: f64,
    pub angle: f64,
    pub hp: f64,
    pub remaining: f64,
    pub kind: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShieldRegion {
    pub owner: u32,
    pub anchor: Pos,
    pub cells: BTreeSet<Pos>,
    pub current: f64,
    pub capacity: f64,
    pub network: Pos,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PowerGrid {
    pub owner: u32,
    pub cells: BTreeSet<Pos>,
    pub output: f64,
    pub load: f64,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlaybackStatus {
    pub paused: bool,
    pub speed: f64,
    pub currentTick: u64,
    pub totalTicks: u64,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConstructionJob {
    pub id: u64,
    pub owner: u32,
    pub target: u64,
    pub rect: Rect,
    pub kind: String,
    pub worker: Pos,
    pub route: Vec<Pos>,
    pub progress: f64,
    pub duration: f64,
    pub invested: f64,
    pub blocked: bool,
    #[serde(default)]
    pub beforeBuilding: Option<Building>,
    #[serde(default)]
    pub beforeRoom: Option<Room>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Rubble {
    pub id: u64,
    pub owner: u32,
    pub rect: Rect,
    pub salvage: f64,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NetworkStore {
    pub owner: u32,
    pub anchor: Pos,
    pub cells: BTreeSet<Pos>,
    pub compute: f64,
    pub capacity: f64,
    pub production: f64,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "kebab-case", rename_all_fields = "camelCase")]
pub enum Command {
    Shell {
        rect: Rect,
    },
    Room {
        shell: u64,
        rect: Rect,
        kind: String,
        #[serde(default)]
        branch: Option<String>,
    },
    Build {
        pos: Pos,
        kind: String,
    },
    Wire {
        kind: String,
        path: Vec<Pos>,
        #[serde(default)]
        unit_endpoints:Vec<u64>,
    },
    Wall {
        kind: String,
        path: Vec<Pos>,
    },
    InstallGpu {
        room: u64,
        model: String,
    },
    RemoveGpu {
        room: u64,
        bay: usize,
    },
    Research {
        room: u64,
        branch: String,
    },
    Deploy {
        room: u64,
        kind: String,
        pos: Pos,
    },
    Move {
        ids: Vec<u64>,
        pos: Pos,
    },
    QueueMove {
        ids: Vec<u64>,
        pos: Pos,
    },
    Attack {
        ids: Vec<u64>,
        target: u64,
    },
    Stop {
        ids: Vec<u64>,
    },
    Skill {
        id: u64,
        pos: Pos,
        #[serde(default)]
        direction: Option<Pos>,
    },
    Plugin {
        id: u64,
        plugin: String,
    },
    Upgrade {
        id: u64,
    },
    Repair {
        id: u64,
    },
    Recycle {
        id: u64,
    },
    Shield {
        enabled: bool,
    },
    ExpandShell {
        id: u64,
        rect: Rect,
    },
    SplitRoom {
        id: u64,
        axis: String,
        offset: i32,
    },
    MergeRooms {
        ids: Vec<u64>,
    },
    ConvertRoom {
        id: u64,
        kind: String,
        #[serde(default)]
        branch: Option<String>,
    },
    Cancel {
        id: u64,
    },
    ClearRubble {
        id: u64,
    },
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Order {
    pub owner: u32,
    pub sequence: u64,
    pub command: Command,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Receipt {
    pub accepted: bool,
    pub sequence: u64,
    pub tick: u64,
    pub reason: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoggedOrder {
    pub tick: u64,
    pub order: Order,
    pub receipt: Receipt,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Save {
    #[serde(default)]
    pub rulesVersion:String,
    #[serde(default)]
    pub rulesFingerprint:String,
    pub snapshot: Snapshot,
    pub orders: Vec<LoggedOrder>,
    pub nextId: u64,
    pub sequences: [u64; 2],
    pub shieldAuto: [bool; 2],
    pub initialAi: bool,
    #[serde(default)]
    pub administrativeEvents: Vec<AdministrativeEvent>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AdministrativeEvent {
    pub tick: u64,
    pub owner: u32,
    pub reason: String,
    pub orderIndex: usize,
}
