// @vitest-environment node
import { existsSync, readFileSync } from 'node:fs';
import { resolve } from 'node:path';
import { describe, expect, it } from 'vitest';
import { BUGS, CELL_COUNT, GPUS, INITIAL_V2, LEVELS, OPERATORS, commandDeploy, commandGpu,
  commandGpuSell, commandGpuUpgrade, commandLevel, commandNextLevel, commandNextWave,
  commandPause, commandRestart, commandSell, commandSkill, commandSpeed, commandTarget,
  commandUpgrade, decodeV2 } from '@/lib/sentinelsV2';

type Published = { name: string; transform: { translation: number[]; scale: number[] } };
const published = (name: string, values: number[]): Published => ({ name,
  transform: { translation: values.slice(0, 3), scale: values.slice(3, 6) } });

/** Independent observed-protocol fixture; values intentionally distinguish each publisher field. */
function snapshot(): Published[] {
  const entities = [
    published('CS_State', [73.5, 12, 3, 1, 61, 2]),
    published('CS_Economy', [415, 52, 900, 123.5, 75, 2]),
    published('CS_StateAux', [7, 19, 5, 3, 0, 17]),
    published('CS_Campaign', [2, 4, 3, 1, 17, 169]),
    published('CS_Meta', [23, 90, 103.75, 2, 2, 0.45]),
  ];
  for (let slot = 0; slot < 24; slot++) {
    entities.push(published(`CS_Unit${slot}`, [0, 0, -1, 0, 0, 0]),
      published(`CS_UnitAux${slot}`, [0, 0, 0, 0, 0, 0]),
      published(`CS_UnitCost${slot}`, [0, 0, 0, 0, 0, 0]));
  }
  for (let slot = 0; slot < 8; slot++) entities.push(published(`CS_GPU${slot}`, [0, 0, 0, 0, 0, 0]));
  for (let row = 0; row < 14; row++) entities.push(published(`CS_MapRow${row}`, [0, 0, 0, 0, 17, 2]));
  set(entities, 'CS_Unit6', [3, 2, 149, 112, 7.25, 90]);
  set(entities, 'CS_UnitAux6', [5.9, 6.1, 2, 34, 4, 1]);
  set(entities, 'CS_UnitCost6', [116, 182, 260, 2, 1.3, 0.2]);
  set(entities, 'CS_GPU0', [2, 1, 20, 220, 154, 154]);
  set(entities, 'CS_GPU7', [3, 2, 32, 350, 315, 416]);
  // 0..7 repeated three times, encoded as four independent 18-bit words.
  set(entities, 'CS_MapRow0', [181896, 107070, 36780, 256794, 17, 2]);
  // Last row: six 7s, six 0s, six 1s, six 4s (catches row/group ordering).
  set(entities, 'CS_MapRow13', [262143, 0, 37449, 149796, 17, 2]);
  return entities;
}
function set(entities: Published[], name: string, values: number[]) {
  entities[entities.findIndex((entity) => entity.name === name)] = published(name, values);
}

const project = resolve(__dirname, '../../../projects/code-sentinels');
const publicAssets = resolve(__dirname, '../public/games/code-sentinels');
const json = (path: string) => JSON.parse(readFileSync(path, 'utf8').replace(/^\uFEFF/, ''));

describe('Code Sentinels V2 native publication decoder', () => {
  it('accepts a captured real native DLL publication after GPU purchase, deployment and ticking', () => {
    const captured = json(resolve(__dirname, 'fixtures/sentinels-v2-native.json'));
    const state = decodeV2(captured.entities);
    expect(state).not.toBeNull();
    expect(state).toMatchObject({ credits: 365, production: 12, gpuCount: 1, level: 1, wave: 0 });
    expect(state!.energy).toBeCloseTo(24, 3);
    expect(state!.units[0]).toMatchObject({ kind: 1, tier: 1, cell: 159, hp: 130 });
    expect(state!.gpus[0]).toMatchObject({ model: 1, tier: 1, rate: 12 });
    expect(state!.terrain).toHaveLength(336);
    expect(state!.terrain[169]).toBe(5);
  });
  it('reads economy, campaign, unit costs and GPU slots from their actual native publishers', () => {
    const state = decodeV2(snapshot().reverse());
    expect(state).toMatchObject({ energy: 73.5, hp: 12, wave: 3, phase: 1, kills: 61, level: 2,
      credits: 415, production: 52, capacity: 900, spentAttack: 123.5, spentSkill: 75, gpuCount: 2,
      spawned: 7, total: 19, enemies: 5, speed: 3, mapRevision: 17, terrainStage: 1, unlocked: 3,
      feedback: 23, lastCost: 90, elapsed: 103.75, combo: 2, bossPhase: 2, bossHealth: 0.45 });
    expect(state!.units).toHaveLength(24);
    expect(state!.gpus).toHaveLength(8);
    expect(state!.units[6]).toEqual({ slot: 6, kind: 3, tier: 2, cell: 149, hp: 112,
      skillCooldown: 7.25, skillCost: 90, attackCost: 5.9, range: 6.1, targetMode: 2,
      attacks: 34, casts: 4, starved: true, upgradeCost: 116, sellRefund: 182, jam: 1.3 });
    expect(state!.units[0]).toMatchObject({ slot: 0, kind: 0, cell: -1 });
    expect(state!.gpus[7]).toEqual({ slot: 7, model: 3, tier: 2, rate: 32, upgradeCost: 315, sellRefund: 416 });
    expect(INITIAL_V2.units[6].kind).toBe(0);
  });

  it('decodes six three-bit cells per word across all 336 cells, including the final group', () => {
    const terrain = decodeV2(snapshot())!.terrain;
    expect(terrain).toHaveLength(CELL_COUNT);
    expect(terrain.slice(0, 24)).toEqual([0,1,2,3,4,5,6,7,0,1,2,3,4,5,6,7,0,1,2,3,4,5,6,7]);
    expect(terrain.slice(24, 48)).toEqual(Array(24).fill(0));
    expect(terrain.slice(-24)).toEqual([...Array(6).fill(7), ...Array(6).fill(0), ...Array(6).fill(1), ...Array(6).fill(4)]);
  });

  it('rejects absent, partial, malformed, duplicate or mixed-revision state without inventing zeros', () => {
    expect(decodeV2([])).toBeNull();
    expect(decodeV2(snapshot().filter((e) => e.name !== 'CS_State'))).toBeNull();
    expect(decodeV2([published('CS_State', [0, 20, 0, 0, 0, 1])])).toBeNull();
    for (const missing of ['CS_Economy', 'CS_UnitCost6', 'CS_GPU7', 'CS_MapRow13']) {
      expect(decodeV2(snapshot().filter((e) => e.name !== missing))).toBeNull();
    }
    const invalid = snapshot();
    set(invalid, 'CS_State', [Number.NaN, 12, 3, 1, 61, 2]);
    expect(decodeV2(invalid)).toBeNull();
    const mixed = snapshot();
    set(mixed, 'CS_MapRow13', [0, 0, 0, 0, 16, 2]);
    expect(decodeV2(mixed)).toBeNull();
    const overflow = snapshot();
    set(overflow, 'CS_MapRow0', [262144, 0, 0, 0, 17, 2]);
    expect(decodeV2(overflow)).toBeNull();
    const duplicate = snapshot(); duplicate.push(duplicate[0]);
    expect(decodeV2(duplicate)).toBeNull();
  });
});

describe('V2 commands preserve the native integer transport contract', () => {
  it('encodes boundary purchases, targeted skills and every GPU socket without collisions', () => {
    expect(commandDeploy(1, 0)).toBe(1_001_000);
    expect(commandDeploy(4, 335)).toBe(1_004_335);
    expect(commandSkill(23, 335)).toBe(3_023_335);
    expect(commandGpu(0, 1)).toBe(4_000_001);
    expect(commandGpu(7, 7)).toBe(4_000_077);
    expect(commandTarget(23, 2)).toBe(3_100_232);
    expect(commandUpgrade(23)).toBe(2_000_023);
    expect(commandSell(23)).toBe(2_100_023);
    expect(commandGpuUpgrade(7)).toBe(4_100_007);
    expect(commandGpuSell(7)).toBe(4_200_007);
    const codes = [commandNextWave(), commandLevel(3), commandNextLevel(), commandPause(), commandSpeed(), commandRestart()];
    expect(codes).toEqual([5_000_000, 5_100_003, 5_200_000, 6_000_000, 7_000_000, 8_000_000]);
    const gpuCodes = Array.from({ length: 8 }, (_, slot) => Array.from({ length: 7 }, (_, model) => commandGpu(slot, model + 1))).flat();
    expect(new Set(gpuCodes).size).toBe(56);
    for (const code of [...codes, ...gpuCodes, commandDeploy(4, 335), commandSkill(23, 335)]) expect(Math.fround(code)).toBe(code);
  });
  it('does not silently round invalid cells, slots, models or non-finite input into another command', () => {
    for (const cell of [-1, 336, 1.5, Number.NaN, Number.POSITIVE_INFINITY]) expect(() => commandDeploy(1, cell)).toThrow(RangeError);
    expect(() => commandDeploy(0, 10)).toThrow(RangeError);
    expect(() => commandSkill(24, 0)).toThrow(RangeError);
    expect(() => commandGpu(8, 1)).toThrow(RangeError);
    expect(() => commandGpu(0, 8)).toThrow(RangeError);
    expect(() => commandTarget(0, 3)).toThrow(RangeError);
    expect(() => commandLevel(4)).toThrow(RangeError);
  });
});

describe('V2 encyclopedia and balance metadata', () => {
  it('keeps real GPU imagery and VRAM distinct from the native fictional economy table', () => {
    const economy = json(resolve(project, 'Content/Data/economy-v2.json'));
    const sources = json(resolve(project, 'references/sources-gpu.json'));
    expect(GPUS).toHaveLength(7);
    for (const [i, gpu] of GPUS.entries()) {
      expect(gpu).toMatchObject({ id: economy.gpus[i].id, cost: economy.gpus[i].credits,
        rate: economy.gpus[i].energyPerSecond, capacity: economy.gpus[i].capacity,
        vram: sources.gpus[i].memoryGB, memory: sources.gpus[i].memoryType,
        image: sources.gpus[i].image, sourceUrl: sources.gpus[i].sourcePage });
      expect(existsSync(resolve(publicAssets, gpu.image))).toBe(true);
      expect(readFileSync(resolve(publicAssets, gpu.image)))
        .toEqual(readFileSync(resolve(project, 'references', sources.gpus[i].image)));
      expect(gpu.caption.length).toBeGreaterThan(4);
    }
    expect(LEVELS).toEqual(economy.levels);
    expect(OPERATORS).toHaveLength(4);
    for (const [i, operator] of OPERATORS.entries()) {
      const u = economy.units[i];
      expect(operator).toMatchObject({ id: u.id, cost: u.credits, attackCost: u.attackEnergy,
        skillCost: u.skillEnergy, cooldown: u.skillCooldown, skillName: u.skill, range: u.range });
      expect(existsSync(resolve(publicAssets, operator.image))).toBe(true);
      expect(operator.description.length).toBeGreaterThan(10);
      expect(operator.skillDescription.length).toBeGreaterThan(10);
    }
    expect(BUGS.map((bug) => bug.id)).toEqual([0,1,2,3,4,5,6,7]);
    expect(BUGS.map((bug) => bug.cwe)).toEqual(['CWE-476','CWE-401','CWE-362','CWE-833','CWE-674','CWE-122','CWE-833','CWE-674']);
    for (const bug of BUGS) {
      expect(bug.image).toMatch(/^enemies\/.+\.png$/);
      expect([...readFileSync(resolve(publicAssets, bug.image)).subarray(0, 8)])
        .toEqual([137, 80, 78, 71, 13, 10, 26, 10]);
      expect(bug.description.length).toBeGreaterThan(15);
      expect(bug.english.length).toBeGreaterThan(3);
    }
  });
});
