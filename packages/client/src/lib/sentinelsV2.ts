/** Native V2 publication protocol. No browser combat/economy simulation.
 * Source of truth: sentinels_v2.rs::read/command and game/build_v2.py.
 * GPU product facts: references/sources-gpu.json. Rates/costs are fictional balance.
 */
export const GRID_WIDTH = 24;
export const GRID_HEIGHT = 14;
export const CELL_COUNT = GRID_WIDTH * GRID_HEIGHT;
export const WAVES_PER_LEVEL = 4;
export const LEVELS = ['断点森林', '泄漏湿地', '递归高地'];
export const TERRAIN_NAMES = ['平地', '阻挡', '水域', '道路', '高地', '基地', '危险区', '桥'];

export type V2Unit = {
  slot: number; kind: number; tier: number; cell: number; hp: number;
  skillCooldown: number; skillCost: number; attackCost: number; range: number;
  targetMode: number; attacks: number; casts: number; starved: boolean;
  upgradeCost: number; sellRefund: number; jam: number;
};
export type V2Gpu = {
  slot: number; model: number; tier: number; rate: number; upgradeCost: number; sellRefund: number;
};
export type V2State = {
  energy: number; hp: number; wave: number; phase: number; kills: number; level: number;
  credits: number; production: number; capacity: number; spentAttack: number; spentSkill: number;
  gpuCount: number; spawned: number; total: number; enemies: number; speed: number;
  mapRevision: number; terrainStage: number; unlocked: number; feedback: number; lastCost: number;
  elapsed: number; combo: number; bossPhase: number; bossHealth: number;
  units: V2Unit[]; gpus: V2Gpu[]; terrain: number[];
};

export const GPUS = [
  {
    "id": 1,
    "name": "GeForce RTX 5060",
    "caption": "MSI VENTUS 2X OC · PCIe",
    "cost": 130,
    "rate": 12,
    "capacity": 300,
    "vram": 8,
    "memory": "GDDR7",
    "image": "gpus/rtx-5060-msi-ventus2x.png",
    "sourceUrl": "https://us-store.msi.com/Graphics-Cards/NVIDIA-GPU/RTX-5060/GeForce-RTX-5060-8G-VENTUS-2X-OC"
  },
  {
    "id": 2,
    "name": "GeForce RTX 5070",
    "caption": "Founders Edition · PCIe",
    "cost": 220,
    "rate": 20,
    "capacity": 420,
    "vram": 12,
    "memory": "GDDR7",
    "image": "gpus/rtx-5070-fe.png",
    "sourceUrl": "https://www.nvidia.com/en-us/geforce/graphics-cards/50-series/rtx-5070-family/"
  },
  {
    "id": 3,
    "name": "GeForce RTX 5080",
    "caption": "Founders Edition · PCIe",
    "cost": 350,
    "rate": 32,
    "capacity": 600,
    "vram": 16,
    "memory": "GDDR7",
    "image": "gpus/rtx-5080-fe.jpg",
    "sourceUrl": "https://www.nvidia.com/en-us/geforce/graphics-cards/50-series/rtx-5080/"
  },
  {
    "id": 4,
    "name": "GeForce RTX 5090",
    "caption": "Founders Edition · PCIe",
    "cost": 500,
    "rate": 49,
    "capacity": 800,
    "vram": 32,
    "memory": "GDDR7",
    "image": "gpus/rtx-5090-fe.jpg",
    "sourceUrl": "https://www.nvidia.com/en-us/geforce/graphics-cards/50-series/rtx-5090/"
  },
  {
    "id": 5,
    "name": "RTX PRO 6000 Blackwell",
    "caption": "Blackwell 工作站版 · PCIe",
    "cost": 760,
    "rate": 72,
    "capacity": 1000,
    "vram": 96,
    "memory": "GDDR7 ECC",
    "image": "gpus/rtx-pro6000-blackwell-workstation-card.jpg",
    "sourceUrl": "https://www.nvidia.com/en-us/products/workstations/professional-desktop-gpus/rtx-pro-6000/"
  },
  {
    "id": 6,
    "name": "NVIDIA A100",
    "caption": "80GB · PCIe 被动散热卡",
    "cost": 980,
    "rate": 94,
    "capacity": 1300,
    "vram": 80,
    "memory": "HBM2e",
    "image": "gpus/a100-80gb-pcie.png",
    "sourceUrl": "https://www.pny.com/nvidia-a100-80gb"
  },
  {
    "id": 7,
    "name": "NVIDIA H200 NVL",
    "caption": "NVL · PCIe 被动散热卡",
    "cost": 1300,
    "rate": 128,
    "capacity": 1700,
    "vram": 141,
    "memory": "HBM3e",
    "image": "gpus/h200-nvl-pcie.png",
    "sourceUrl": "https://www.pny.com/nvidia-h200-nvl?iscommercial=true"
  }
] as const;

export const OPERATORS = [
  { id: 1, name: 'VS Code', role: '精准单点', cost: 65, attackCost: 3, skillCost: 55,
    skillName: '断点狙击', cooldown: 13, range: 3.8, image: 'vscode.svg', color: '#63b6ff',
    description: '低耗能快速射击。依照目标策略拦截进入射程的错误进程。',
    skillDescription: '打击目标格周围 1.8 格内的敌人，穿透护甲，标记 8 秒并减速 2 秒。' },
  { id: 2, name: 'PyCharm', role: '集群调试', cost: 90, attackCost: 6, skillCost: 75,
    skillName: '垃圾回收', cooldown: 17, range: 3.4, image: 'pycharm.svg', color: '#b5e75c',
    description: '攻击带有 1.25 格溅射；激活协同后可穿透护甲。',
    skillDescription: '净化目标格周围 3.1 格内的敌人并标记 8 秒，对内存泄漏造成更高伤害，同时延缓敌方特殊行为。' },
  { id: 3, name: 'DeepSeek 娘', role: '潮汐控制', cost: 110, attackCost: 5, skillCost: 90,
    skillName: '推理潮汐', cooldown: 20, range: 4.5, image: 'deepseek.png', color: '#6cd1f4',
    description: '普通攻击施加减速与易伤标记，帮助附近工具链集中火力。',
    skillDescription: '向目标格周围 3.1 格释放潮汐，造成穿透伤害，并施加 7 秒减速与易伤标记。' },
  { id: 4, name: 'GPT 娘', role: '回滚修复', cost: 100, attackCost: 4, skillCost: 110,
    skillName: '回滚星核', cooldown: 24, range: 4.1, image: 'gpt.png', color: '#c9a7ff',
    description: '以算力驱动星核射击；技能同时修复核心、支援友军并压制敌人。',
    skillDescription: '恢复核心 5 点；治疗目标附近 3.2 格内友军 55 点并解除干扰，同时伤害附近 3.1 格敌人并减速 3 秒。' },
] as const;

/** CWE links explain real bug concepts; monster behavior is their fictional gameplay metaphor. */
export const BUGS = [
  { id: 0, name: '空指针', english: 'Null Pointer', description: '解引用空指针可能让程序崩溃。战场中它会周期性瞬移，快速穿过防线空隙。', cwe: 'CWE-476', image: 'enemies/null-pointer.png' },
  { id: 1, name: '内存泄漏', english: 'Memory Leak', description: '不再使用的内存没有释放。战场中它持续窃取算力并恢复生命；标记可以阻止这项能力。', cwe: 'CWE-401', image: 'enemies/memory-leak.png' },
  { id: 2, name: '竞态条件', english: 'Race Condition', description: '共享资源的并发访问缺少正确同步。战场中它会冲刺并闪避部分普通攻击，标记可克制闪避。', cwe: 'CWE-362', image: 'enemies/race-condition.png' },
  { id: 3, name: '死锁', english: 'Deadlock', description: '执行者互相等待而无法推进。战场中它有护甲，并周期性干扰附近两名守护者。', cwe: 'CWE-833', image: 'enemies/deadlock.png' },
  { id: 4, name: '栈溢出', english: 'Stack Overflow', description: '失控递归可以耗尽调用栈。战场中初代敌人被击破后分裂为两只弱化子体。', cwe: 'CWE-674', image: 'enemies/stack-overflow.png' },
  { id: 5, name: '堆损坏巨兽', english: 'Heap Corruption', description: '堆缓冲区越界可破坏内存。森林首领周期性伤害附近守护者并吞噬算力；阶段变化会改写地形。', cwe: 'CWE-122', image: 'enemies/heap-corruption.png' },
  { id: 6, name: '死锁巨像', english: 'Deadlock Colossus', description: '湿地首领带有护甲，并持续干扰附近守护者。半血和击败时改变通路，迫使双方重新寻路。', cwe: 'CWE-833', image: 'enemies/deadlock-boss.png' },
  { id: 7, name: '递归巨构', english: 'Recursive Titan', description: '高地首领周期性召唤栈溢出子体；它的阶段转换会改变地图，考验产能与防线的持续性。', cwe: 'CWE-674', image: 'enemies/stack-boss.png' },
] as const;

const emptyUnit = (slot: number): V2Unit => ({ slot, kind: 0, tier: 0, cell: -1, hp: 0,
  skillCooldown: 0, skillCost: 0, attackCost: 0, range: 0, targetMode: 0, attacks: 0, casts: 0,
  starved: false, upgradeCost: 0, sellRefund: 0, jam: 0 });
/** Loading placeholder only. decodeV2 never returns this for absent native state. */
export const INITIAL_V2: V2State = {
  energy: 0, hp: 20, wave: 0, phase: 0, kills: 0, level: 1, credits: 560, production: 0,
  capacity: 0, spentAttack: 0, spentSkill: 0, gpuCount: 0, spawned: 0, total: 0, enemies: 0,
  speed: 1, mapRevision: 0, terrainStage: 0, unlocked: 1, feedback: 0, lastCost: 0, elapsed: 0,
  combo: 0, bossPhase: 0, bossHealth: 0, units: Array.from({ length: 24 }, (_, i) => emptyUnit(i)),
  gpus: Array.from({ length: 8 }, (_, slot) => ({ slot, model: 0, tier: 0, rate: 0, upgradeCost: 0, sellRefund: 0 })),
  terrain: Array(CELL_COUNT).fill(0),
};

type Six = [number, number, number, number, number, number];
const integer = (n: number, min: number, max: number) => Number.isInteger(n) && n >= min && n <= max;

/** Reject incomplete/invalid publications rather than fabricating a zeroed successful game. */
export function decodeV2(entities: readonly unknown[]): V2State | null {
  const byName = new Map<string, unknown>();
  const duplicates = new Set<string>();
  for (const value of entities) {
    if (!value || typeof value !== 'object' || !('name' in value) || typeof value.name !== 'string') continue;
    if (byName.has(value.name)) duplicates.add(value.name);
    byName.set(value.name, value);
  }
  const record = (name: string): Six | null => {
    if (duplicates.has(name)) return null;
    const value = byName.get(name);
    if (!value || typeof value !== 'object' || !('transform' in value)) return null;
    const t = value.transform;
    if (!t || typeof t !== 'object' || !('translation' in t) || !('scale' in t)
      || !Array.isArray(t.translation) || !Array.isArray(t.scale)
      || t.translation.length !== 3 || t.scale.length !== 3) return null;
    const fields = [...t.translation, ...t.scale];
    return fields.every((v) => typeof v === 'number' && Number.isFinite(v)) ? fields as Six : null;
  };
  const main = record('CS_State');
  if (!main) return null;
  const economy = record('CS_Economy'), aux = record('CS_StateAux');
  const campaign = record('CS_Campaign'), meta = record('CS_Meta');
  if (!economy || !aux || !campaign || !meta) return null;
  if (!integer(main[3], 0, 3) || !integer(main[2], 0, WAVES_PER_LEVEL)
    || !integer(main[5], 1, 3) || campaign[0] !== main[5] || campaign[1] !== WAVES_PER_LEVEL
    || !integer(campaign[2], main[5], 3) || !integer(campaign[3], 0, 2)
    || !integer(aux[3], 1, 3) || !integer(aux[5], 0, 16_777_215)
    || campaign[4] !== aux[5] || campaign[5] !== 169) return null;
  const units: V2Unit[] = [];
  for (let slot = 0; slot < 24; slot++) {
    const u = record('CS_Unit' + slot), a = record('CS_UnitAux' + slot), cost = record('CS_UnitCost' + slot);
    if (!u || !a || !cost || !integer(u[0], 0, 4) || !integer(u[1], 0, 3)
      || cost[3] !== u[1] || !integer(u[2], -1, CELL_COUNT - 1)
      || !integer(a[2], 0, 2) || !integer(a[5], 0, 1)
      || (u[0] > 0 && (u[1] === 0 || u[2] < 0))) return null;
    units.push({ slot, kind: u[0], tier: u[1], cell: u[2], hp: u[3],
      skillCooldown: u[4], skillCost: u[5], attackCost: a[0], range: a[1], targetMode: a[2],
      attacks: a[3], casts: a[4], starved: a[5] === 1, upgradeCost: cost[0], sellRefund: cost[1], jam: cost[4] });
  }
  const gpus: V2Gpu[] = [];
  for (let slot = 0; slot < 8; slot++) {
    const g = record('CS_GPU' + slot);
    if (!g || !integer(g[0], 0, 7) || !integer(g[1], 0, 3) || (g[0] > 0 && g[1] === 0)) return null;
    gpus.push({ slot, model: g[0], tier: g[1], rate: g[2], upgradeCost: g[4], sellRefund: g[5] });
  }
  const terrain: number[] = [];
  for (let row = 0; row < GRID_HEIGHT; row++) {
    const cells = record('CS_MapRow' + row);
    if (!cells || cells[4] !== aux[5] || cells[5] !== main[5]) return null;
    for (let group = 0; group < 4; group++) {
      const packed = cells[group];
      if (!integer(packed, 0, 262143)) return null;
      for (let cell = 0; cell < 6; cell++) terrain.push((packed >>> (cell * 3)) & 7);
    }
  }
  return { energy: main[0], hp: main[1], wave: main[2], phase: main[3], kills: main[4], level: main[5],
    credits: economy[0], production: economy[1], capacity: economy[2],
    spentAttack: economy[3], spentSkill: economy[4], gpuCount: economy[5],
    spawned: aux[0], total: aux[1], enemies: aux[2], speed: aux[3], mapRevision: aux[5],
    terrainStage: campaign[3], unlocked: campaign[2], feedback: meta[0], lastCost: meta[1],
    elapsed: meta[2], combo: meta[3], bossPhase: meta[4], bossHealth: meta[5], units, gpus, terrain };
}

function argument(name: string, value: number, min: number, max: number): number {
  if (!integer(value, min, max)) throw new RangeError(name + ' must be an integer in ' + min + '..' + max);
  return value;
}
// Every encoding is < 2^24, so it survives the native f32 transport exactly.
export const commandDeploy = (kind: number, cell: number) =>
  1_000_000 + argument('kind', kind, 1, 4) * 1000 + argument('cell', cell, 0, 335);
export const commandUpgrade = (slot: number) => 2_000_000 + argument('slot', slot, 0, 23);
export const commandSell = (slot: number) => 2_100_000 + argument('slot', slot, 0, 23);
export const commandSkill = (slot: number, cell: number) =>
  3_000_000 + argument('slot', slot, 0, 23) * 1000 + argument('cell', cell, 0, 335);
export const commandTarget = (slot: number, mode: number) =>
  3_100_000 + argument('slot', slot, 0, 23) * 10 + argument('mode', mode, 0, 2);
export const commandGpu = (slot: number, model: number) =>
  4_000_000 + argument('slot', slot, 0, 7) * 10 + argument('model', model, 1, 7);
export const commandGpuUpgrade = (slot: number) => 4_100_000 + argument('slot', slot, 0, 7);
export const commandGpuSell = (slot: number) => 4_200_000 + argument('slot', slot, 0, 7);
export const commandNextWave = () => 5_000_000;
export const commandLevel = (level: number) => 5_100_000 + argument('level', level, 1, 3);
export const commandNextLevel = () => 5_200_000;
export const commandPause = () => 6_000_000;
export const commandSpeed = () => 7_000_000;
export const commandRestart = () => 8_000_000;
