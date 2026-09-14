/** V4 native authority contract. Prices/production are fictional game balance. */
export const GRID_WIDTH = 32;
export const GRID_HEIGHT = 20;
export const WORLD_WIDTH = GRID_HEIGHT * 16 / 9;
export const WORLD_HEIGHT = GRID_HEIGHT;
export const BUILDING_LIMIT = 48;
export const UNIT_LIMIT = 32;
export const GPU_LIMIT = 32;
export const LINK_LIMIT = 64;
export const TERRAIN_NAMES = ['平地', '岩壁', '水域', '道路', '高地', '矿脉', '煤层', '基地候选地'];
export const GPU_TECH = [0, 0, 0, 1, 2, 2, 1, 3];
export const UNIT_TECH = [0, 0, 0, 1, 2];
const ART = '/games/code-sentinels/ui-v4/art/';
export const BUILDINGS = [
  { id: 1, name: '指挥核心', cost: 0, power: 0, tech: 0, description: '预置基地；保护核心并经营前沿网络。', image: ART + 'command-core.png' },
  { id: 2, name: '数据中心', cost: 280, power: -20, tech: 0, description: '接通电网后可装4块GPU，产生算力并提供有线接入。最多8座。', image: ART + 'data-center.png' },
  { id: 3, name: '风力发电站', cost: 160, power: 150, tech: 0, description: '陆地可建，高地产能提高50%；通过电力线供电。', image: ART + 'wind-power.png' },
  { id: 4, name: '水力发电站', cost: 360, power: 420, tech: 1, description: '必须建在距水域2格以内的陆地。', image: ART + 'hydro-power.png' },
  { id: 5, name: '燃煤发电站', cost: 500, power: 800, tech: 1, description: '必须靠近煤层（3格内）；供电稳定但有较高建设成本。', image: ART + 'coal-power.png' },
  { id: 6, name: '核能发电站', cost: 1100, power: 2400, tech: 3, description: '需要科技3，距水域4格内以获得冷却水。', image: ART + 'nuclear-power.png' },
  { id: 7, name: '移动算力基站', cost: 360, power: 0, tech: 1, description: '在数据中心14格回传范围内提供6格无线覆盖，可移动。', image: ART + 'mobile-relay.png' },
  { id: 8, name: '研究院', cost: 220, power: -20, tech: 0, description: '需要电力及算力线接入，研究解锁更高型号GPU与AI。', image: ART + 'research-lab.png' },
  { id: 9, name: '资源采集器', cost: 180, power: 0, tech: 0, description: '建在矿脉或煤层上，持续取得建设经费。', image: ART + 'resource-extractor.png' },
  { id: 10, name: 'cudad 护城河', cost: 8, power: 0, tech: 0, description: '拖线建设城墙。真正闭合的围墙内有供能数据中心时可充算力护盾。', image: ART + 'cudad-wall.png' },
] as const;
export const GPUS = [
  { id: 1, name: 'RTX 5060', cost: 130, rate: 12, capacity: 300, power: 55, tech: 0, image: '/games/code-sentinels/gpus/rtx-5060-msi-ventus2x.png' },
  { id: 2, name: 'RTX 5070', cost: 220, rate: 20, capacity: 420, power: 85, tech: 0, image: '/games/code-sentinels/gpus/rtx-5070-fe.png' },
  { id: 3, name: 'RTX 5080', cost: 350, rate: 32, capacity: 600, power: 120, tech: 1, image: '/games/code-sentinels/gpus/rtx-5080-fe.jpg' },
  { id: 4, name: 'RTX 5090', cost: 500, rate: 49, capacity: 800, power: 180, tech: 2, image: '/games/code-sentinels/gpus/rtx-5090-fe.jpg' },
  { id: 5, name: 'RTX PRO 6000', cost: 760, rate: 72, capacity: 1000, power: 260, tech: 2, image: '/games/code-sentinels/gpus/rtx-pro6000-blackwell-workstation-card.jpg' },
  { id: 6, name: 'A100 80GB PCIe', cost: 980, rate: 94, capacity: 1300, power: 320, tech: 1, image: '/games/code-sentinels/gpus/a100-80gb-pcie.png' },
  { id: 7, name: 'H200 NVL', cost: 1300, rate: 128, capacity: 1700, power: 480, tech: 3, image: '/games/code-sentinels/gpus/h200-nvl-pcie.png' },
] as const;
export const UNITS = [
  { id: 1, name: 'VS Code炮台', cost: 65, tech: 0, mobile: false, skillCost: 55, description: '固定精准炮台，需要算力接入。' },
  { id: 2, name: 'PyCharm炮台', cost: 90, tech: 0, mobile: false, skillCost: 75, description: '固定范围调试炮台，需要算力接入。' },
  { id: 3, name: 'DeepSeek', cost: 110, tech: 1, mobile: true, skillCost: 90, description: '有线接入时固定；无线或电池支持移动与推理潮汐。' },
  { id: 4, name: 'GPT', cost: 100, tech: 2, mobile: true, skillCost: 110, description: '移动支援AI，回滚修复；脱离覆盖后消耗随身算力电池。' },
] as const;

export const COMMAND = {
  build: (kind: number, cell: number) => 1_000_000 + kind * 1000 + cell,
  upgradeBuilding: (slot: number) => 2_000_000 + slot,
  sellBuilding: (slot: number) => 2_100_000 + slot,
  repairBuilding: (slot: number) => 2_200_000 + slot,
  installGpu: (centerSlot: number, bay: number, model: number) => 3_000_000 + centerSlot * 100 + bay * 10 + model,
  upgradeGpu: (slot: number) => 3_100_000 + slot,
  sellGpu: (slot: number) => 3_200_000 + slot,
  deploy: (kind: number, cell: number) => 4_000_000 + kind * 1000 + cell,
  upgradeUnit: (slot: number) => 4_100_000 + slot,
  sellUnit: (slot: number) => 4_200_000 + slot,
  moveUnit: (slot: number, cell: number) => 4_300_000 + slot * 1000 + cell,
  skill: (slot: number, cell: number) => 4_400_000 + slot * 1000 + cell,
  targetMode: (slot: number, mode: number) => 4_500_000 + slot * 10 + mode,
  powerLine: (a: number, b: number) => 5_000_000 + a * 1000 + b,
  deleteLink: (slot: number) => 5_800_000 + slot,
  computeLine: (a: number, b: number) => 6_000_000 + a * 1000 + b,
  wall: (a: number, b: number) => 7_000_000 + a * 1000 + b,
  removeWall: (cell: number) => 7_800_000 + cell,
  moveRelay: (slot: number, cell: number) => 8_000_000 + slot * 1000 + cell,
  research: () => 9_000_001,
  nextWave: () => 10_000_000,
  speed: () => 10_000_001,
  restart: () => 10_000_003,
  nextLevel: () => 10_100_000,
  selectLevel: (level: number) => 10_100_000 + level,
  selectBase: (base: number) => 10_200_000 + base,
  toggleShield: () => 10_300_000,
};

export const FEEDBACK: Record<number, string> = {
  1: '建设经费不足。', 2: '位置或插槽已占用。', 3: '请选择有效目标。', 4: '已达到最高等级。',
  5: '技能仍在冷却。', 6: '目标区域没有敌人。', 7: '当前波次尚未结束。', 8: '无法识别该操作。',
  9: '战斗已结束。', 10: '建筑已建成。', 11: '升级完成。', 12: '回收完成。', 13: '技能已释放。',
  14: '入侵开始。', 15: '战区守住了。', 16: '指挥核心失守。', 17: '本波清空。', 18: '目标策略已更改。',
  20: '显卡已安装。', 21: '显卡升级完成。', 22: '显卡已回收。', 23: '可用算力或随身电池不足。',
  24: '电力线路已铺设。', 25: '算力线路已铺设。', 26: '线路已拆除。', 27: '城墙已建成。', 28: '墙段已拆除。',
  30: '先为数据中心接通电力，并安装可运行的显卡。', 31: '需要保留经营启动经费。', 32: '科技或战区尚未解锁。',
  33: '当前无法进入下一战区。', 34: '此地形无法建设。', 35: '敌人占据目标位置。', 36: '单位或建筑数量达到上限。',
  37: '无法找到有效路线。', 38: '预置核心不能回收。', 39: '发电站不满足附近地形要求。',
  40: 'Boss改写了战场地形，单位正在重新寻路。', 41: 'Boss被击败，新的地形通道出现。',
  42: '当前单位固定或有线接入，无法移动。', 43: '移动命令已接收。', 44: '移动基站正在转移。',
  45: '需要一座同时接通电力和算力的研究院。', 46: '科技研究完成。', 47: '护盾充能设置已更改。',
  48: '需要在矿脉或煤层上建立采集器。', 49: '数据中心最多8座，每座4个GPU插槽。',
  50: '维修完成。', 51: '基地位置已选择。', 52: '没有对应的数据中心或插槽。',
};

export interface V4Building { slot: number; kind: number; cell: number; tier: number; hp: number; powered: boolean; connected: boolean; owner: number; demand: number; supply: number; active: boolean; rate: number; repairCost: number; range: number; upgradeCost: number; sellRefund: number; targetCell: number; x: number; y: number }
export interface V4Unit { slot: number; kind: number; cell: number; tier: number; hp: number; battery: number; batteryMax: number; covered: boolean; wired: boolean; skillCooldown: number; skillCost: number; owner: number; moving: boolean; attackCost: number; range: number; upgradeCost: number; starved: boolean; active: boolean; attacks: number; x: number; y: number; targetCell: number; targetMode: number; casts: number; centerSlot: number }
export interface V4Gpu { slot: number; centerSlot: number; model: number; tier: number; rate: number; demand: number; active: boolean; powered: boolean; capacity: number; invested: number; upgradeCost: number; sellRefund: number; bay: number }
export interface V4Link { slot: number; fromCell: number; toCell: number; kind: number; hp: number; active: boolean; owner: number; powered: boolean; length: number }
export interface V4Enemy { slot: number; kind: number; hp: number; maxHp: number; owner: number; active: boolean; x: number; y: number; cell: number }
export interface V4State {
  credits: number; compute: number; baseHP: number; phase: number; wave: number; level: number;
  powerGenerated: number; powerDemand: number; production: number; capacity: number; incomePerSecond: number; tech: number;
  paused: boolean; speed: number; feedback: number; activeEnemies: number; spawned: number; total: number;
  revision: number; shield: number; shieldCapacity: number; closedArea: number; shieldAuto: boolean; commandSeq: number;
  unlocked: number; baseCell: number; enemyBaseCell: number; spentAttack: number; spentSkill: number; spentShield: number;
  terrain: number[]; wireCells: number[]; wallCells: boolean[]; closedCells: boolean[]; shieldCells: boolean[];
  buildings: V4Building[]; units: V4Unit[]; gpus: V4Gpu[]; links: V4Link[]; enemies: V4Enemy[];
}
export const INITIAL_V4: V4State = { credits: 0, compute: 0, baseHP: 0, phase: 0, wave: 0, level: 1, powerGenerated: 0, powerDemand: 0, production: 0, capacity: 0, incomePerSecond: 0, tech: 0, paused: false, speed: 1, feedback: 0, activeEnemies: 0, spawned: 0, total: 0, revision: 0, shield: 0, shieldCapacity: 0, closedArea: 0, shieldAuto: true, commandSeq: 0, unlocked: 1, baseCell: 323, enemyBaseCell: 348, spentAttack: 0, spentSkill: 0, spentShield: 0, terrain: [], wireCells: [], wallCells: [], closedCells: [], shieldCells: [], buildings: [], units: [], gpus: [], links: [], enemies: [] };
type Entity = { name: string; transform: { translation: number[]; scale: number[] } };
const bool = (n: number) => n > 0.5;
export function decodeV4(input: { entities: Entity[] } | Entity[]): V4State | null {
  const list = Array.isArray(input) ? input : input?.entities;
  if (!Array.isArray(list)) return null;
  const entities = new Map(list.map(e => [e.name, e]));
  const values = (name: string): number[] => {
    const e = entities.get(name); const result = [...(e?.transform?.translation ?? []), ...(e?.transform?.scale ?? [])];
    if (result.length !== 6 || result.some(n => !Number.isFinite(n))) throw new Error('Incomplete native snapshot');
    return result;
  };
  try {
    const s=values('C4_State'), e=values('C4_Economy'), st=values('C4_Status'), n=values('C4_Network'), p=values('C4_Progress');
    const result: V4State = { ...INITIAL_V4, credits:s[0],compute:s[1],baseHP:s[2],phase:s[3],wave:s[4],level:s[5],powerGenerated:e[0],powerDemand:e[1],production:e[2],capacity:e[3],incomePerSecond:e[4],tech:e[5],paused:bool(st[0]),speed:st[1],feedback:st[2],activeEnemies:st[3],spawned:st[4],total:st[5],revision:n[0],shield:n[1],shieldCapacity:n[2],closedArea:n[3],shieldAuto:bool(n[4]),commandSeq:n[5],unlocked:p[0],baseCell:p[1],enemyBaseCell:p[2],spentAttack:p[3],spentSkill:p[4],spentShield:p[5],terrain:[],wireCells:[],wallCells:[],closedCells:[],shieldCells:[],buildings:[],units:[],gpus:[],links:[],enemies:[] };
    for(let i=0;i<48;i++){const a=values(`C4_Building${i}`),b=values(`C4_BuildingMeta${i}`),c=values(`C4_BuildingExtra${i}`);result.buildings.push({slot:i,kind:a[0],cell:a[1],tier:a[2],hp:a[3],powered:bool(a[4]),connected:bool(a[5]),owner:b[0],demand:b[1],supply:b[2],active:bool(b[3]),rate:b[4],repairCost:b[5],range:c[0],upgradeCost:c[1],sellRefund:c[2],targetCell:c[3],x:c[4],y:c[5]});}
    for(let i=0;i<32;i++){const a=values(`C4_Unit${i}`),b=values(`C4_UnitMeta${i}`),c=values(`C4_UnitCombat${i}`),d=values(`C4_UnitPos${i}`);result.units.push({slot:i,kind:a[0],cell:a[1],tier:a[2],hp:a[3],battery:a[4],batteryMax:a[5],covered:bool(b[0]),wired:bool(b[1]),skillCooldown:b[2],skillCost:b[3],owner:b[4],moving:bool(b[5]),attackCost:c[0],range:c[1],upgradeCost:c[2],starved:bool(c[3]),active:bool(c[4]),attacks:c[5],x:d[0],y:d[1],targetCell:d[2],targetMode:d[3],casts:d[4],centerSlot:d[5]});const g=values(`C4_Gpu${i}`),h=values(`C4_GpuMeta${i}`);result.gpus.push({slot:i,centerSlot:g[0],model:g[1],tier:g[2],rate:g[3],demand:g[4],active:bool(g[5]),powered:bool(h[0]),capacity:h[1],invested:h[2],upgradeCost:h[3],sellRefund:h[4],bay:h[5]});}
    for(let i=0;i<64;i++){const a=values(`C4_Link${i}`),b=values(`C4_LinkMeta${i}`);result.links.push({slot:i,fromCell:a[0],toCell:a[1],kind:a[2],hp:a[3],active:bool(a[4]),owner:a[5],powered:bool(b[0]),length:b[1]});const q=values(`C4_EnemyStat${i}`),r=values(`C4_EnemyPos${i}`);result.enemies.push({slot:i,kind:q[0],hp:q[1],maxHp:q[2],owner:q[3],active:bool(q[4]),x:r[0],y:r[1],cell:r[2]});}
    for(let row=0;row<GRID_HEIGHT;row++){const t=values(`C4_MapRow${row}`),w=values(`C4_WireRow${row}`),a=values(`C4_WallRow${row}`);for(let c=0;c<32;c++){result.terrain.push((Math.round(t[Math.floor(c/8)])>>((c%8)*3))&7);result.wireCells.push((Math.round(w[Math.floor(c/8)])>>((c%8)*2))&3);result.wallCells.push(Boolean((Math.round(a[Math.floor(c/16)])>>(c%16))&1));result.closedCells.push(Boolean((Math.round(a[2+Math.floor(c/16)])>>(c%16))&1));result.shieldCells.push(Boolean((Math.round(a[4+Math.floor(c/16)])>>(c%16))&1));}}
    return result;
  } catch { return null; }
}
export function cellFromNormalized(x:number,y:number):number|null { const col=Math.floor(x*WORLD_WIDTH-(WORLD_WIDTH-GRID_WIDTH)/2),row=Math.floor(y*GRID_HEIGHT);return col<0||col>=GRID_WIDTH||row<0||row>=GRID_HEIGHT?null:row*GRID_WIDTH+col; }
export function cellWorld(cell:number):[number,number]{return [cell%GRID_WIDTH-GRID_WIDTH/2+0.5,GRID_HEIGHT/2-0.5-Math.floor(cell/GRID_WIDTH)];}
