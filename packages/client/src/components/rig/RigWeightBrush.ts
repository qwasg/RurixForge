import type { Influence, RigPart, Vec2 } from '@/lib/rigTypes';

/**
 * 权重笔刷(F-GAME-5 W5):加 / 减 / 平滑,半径与强度;纯函数,输出整部件新权重数组,
 * 由 rigStore 写回 part.weights(并清 part.bone),保存时 diff 成 rig_patch `set_weights`。
 *
 * 规则(与 forge-rig2d 校验对齐):每顶点 ≤ 4 个影响、权重 ≥ 0;笔刷后逐顶点归一化到和为 1
 * (单一影响即 1);< 1e-4 的影响剔除;4 位小数。
 * - add:v' = v + f·(1−v),其余骨骼按比例让出 (1−v');
 * - subtract:v' = v·(1−f),减掉的量按比例分给其余骨骼(只有这一根骨骼时无处可分 → 不变,如实);
 * - smooth:与一环邻居(三角形邻接)的平均权重按 f 插值。
 * 衰减 f = strength·(1 − d/r)²(d < r)。
 */

export type BrushMode = 'add' | 'subtract' | 'smooth';

export interface BrushParams {
  mode: BrushMode;
  /** 半径(画布像素)。 */
  radius: number;
  /** 0..1 */
  strength: number;
  /** add/subtract 作用的骨骼(smooth 忽略)。 */
  bone: string | null;
}

const MAX_INF = 4;
const EPS = 1e-4;

function round4(v: number): number {
  return Math.round(v * 10000) / 10000;
}

/** 部件当前绑定 → 逐顶点影响(刚性 bone → [[bone,1]];未绑定 → 空)。 */
export function bindingToWeights(part: Pick<RigPart, 'weights' | 'bone' | 'mesh'>): Influence[][] {
  const n = part.mesh.vertices.length;
  if (part.weights && part.weights.length === n) return part.weights.map((inf) => inf.map(([b, w]) => [b, w]));
  if (part.bone) return Array.from({ length: n }, () => [[part.bone as string, 1]] as Influence[]);
  return Array.from({ length: n }, () => [] as Influence[]);
}

/** 顶点对某骨骼的归一化权重(热力图显示用)。 */
export function weightOf(inf: Influence[] | undefined, bone: string): number {
  if (!inf || inf.length === 0) return 0;
  let sum = 0;
  let mine = 0;
  for (const [b, w] of inf) {
    sum += w;
    if (b === bone) mine += w;
  }
  return sum > 0 ? mine / sum : 0;
}

/** 剔除极小影响 → 保留最大 4 个 → 归一化 → 4 位小数(按骨骼名排序,输出确定)。 */
export function normalizeInfluences(inf: Influence[]): Influence[] {
  const merged = new Map<string, number>();
  for (const [b, w] of inf) {
    if (!(w > 0) || !Number.isFinite(w)) continue;
    merged.set(b, (merged.get(b) ?? 0) + w);
  }
  let list = [...merged.entries()].filter(([, w]) => w >= EPS);
  list.sort((a, b) => b[1] - a[1]);
  list = list.slice(0, MAX_INF);
  const sum = list.reduce((s, [, w]) => s + w, 0);
  if (sum <= 0) return [];
  const out = list.map(([b, w]) => [b, round4(w / sum)] as Influence).filter(([, w]) => w >= EPS);
  out.sort((a, b) => (a[0] < b[0] ? -1 : a[0] > b[0] ? 1 : 0));
  return out;
}

/** 三角形一环邻接表。 */
export function buildAdjacency(triangles: [number, number, number][], n: number): number[][] {
  const sets = Array.from({ length: n }, () => new Set<number>());
  for (const [a, b, c] of triangles) {
    if (a >= n || b >= n || c >= n) continue;
    sets[a].add(b).add(c);
    sets[b].add(a).add(c);
    sets[c].add(a).add(b);
  }
  return sets.map((s) => [...s]);
}

function falloff(d: number, r: number, strength: number): number {
  if (r <= 0 || d >= r) return 0;
  const t = 1 - d / r;
  return Math.max(0, Math.min(1, strength)) * t * t;
}

function addTo(inf: Influence[], bone: string, f: number): Influence[] {
  const total = inf.reduce((s, [, w]) => s + w, 0);
  const norm = total > 0 ? inf.map(([b, w]) => [b, w / total] as Influence) : [];
  const v = norm.filter(([b]) => b === bone).reduce((s, [, w]) => s + w, 0);
  const v2 = v + f * (1 - v);
  const others = norm.filter(([b]) => b !== bone);
  const oSum = others.reduce((s, [, w]) => s + w, 0);
  const scaled = oSum > 0 ? others.map(([b, w]) => [b, (w / oSum) * (1 - v2)] as Influence) : [];
  return normalizeInfluences([...scaled, [bone, v2]]);
}

function subtractFrom(inf: Influence[], bone: string, f: number): Influence[] {
  const total = inf.reduce((s, [, w]) => s + w, 0);
  if (total <= 0) return inf;
  const norm = inf.map(([b, w]) => [b, w / total] as Influence);
  const v = norm.filter(([b]) => b === bone).reduce((s, [, w]) => s + w, 0);
  const others = norm.filter(([b]) => b !== bone);
  const oSum = others.reduce((s, [, w]) => s + w, 0);
  if (v <= 0 || oSum <= 0) return normalizeInfluences(norm);
  const v2 = v * (1 - f);
  const scaled = others.map(([b, w]) => [b, (w / oSum) * (1 - v2)] as Influence);
  return normalizeInfluences([...scaled, [bone, v2]]);
}

function lerpInfluences(a: Influence[], b: Influence[], t: number): Influence[] {
  const m = new Map<string, number>();
  const sa = a.reduce((s, [, w]) => s + w, 0) || 1;
  const sb = b.reduce((s, [, w]) => s + w, 0) || 1;
  for (const [k, w] of a) m.set(k, (m.get(k) ?? 0) + (w / sa) * (1 - t));
  for (const [k, w] of b) m.set(k, (m.get(k) ?? 0) + (w / sb) * t);
  return normalizeInfluences([...m.entries()]);
}

/**
 * 在 center(画布像素)处落一笔。vertices = 用于命中的顶点位置(静止网格);
 * 返回新权重数组与受影响顶点下标(无顶点受影响时 changed 为空,调用方可跳过写回)。
 */
export function applyBrush(
  weights: Influence[][],
  vertices: Vec2[],
  triangles: [number, number, number][],
  center: Vec2,
  brush: BrushParams,
): { weights: Influence[][]; changed: number[] } {
  const n = vertices.length;
  const out = weights.slice(0, n);
  while (out.length < n) out.push([]);
  const changed: number[] = [];
  const factors: number[] = new Array(n).fill(0);
  for (let i = 0; i < n; i += 1) {
    const d = Math.hypot(vertices[i][0] - center[0], vertices[i][1] - center[1]);
    factors[i] = falloff(d, brush.radius, brush.strength);
    if (factors[i] > 0) changed.push(i);
  }
  if (changed.length === 0) return { weights: out, changed };
  if (brush.mode === 'smooth') {
    const adj = buildAdjacency(triangles, n);
    const src = out.map((x) => x);
    for (const i of changed) {
      const nb = adj[i];
      if (nb.length === 0) continue;
      const avg = new Map<string, number>();
      for (const j of nb) {
        const inf = src[j];
        const s = inf.reduce((acc, [, w]) => acc + w, 0);
        if (s <= 0) continue;
        for (const [b, w] of inf) avg.set(b, (avg.get(b) ?? 0) + w / s / nb.length);
      }
      if (avg.size === 0) continue;
      out[i] = lerpInfluences(src[i], [...avg.entries()], factors[i]);
    }
    return { weights: out, changed };
  }
  const bone = brush.bone;
  if (!bone) return { weights: out, changed: [] };
  for (const i of changed) {
    out[i] = brush.mode === 'add' ? addTo(out[i], bone, factors[i]) : subtractFrom(out[i], bone, factors[i]);
  }
  return { weights: out, changed };
}

/** 热力图配色:0 → 深蓝,0.5 → 绿,1 → 红(alpha 固定)。 */
export function heatColor(w: number, alpha = 0.85): string {
  const t = Math.max(0, Math.min(1, w));
  const r = Math.round(255 * Math.min(1, Math.max(0, 2 * t - 0.5) * 1.4));
  const g = Math.round(255 * (1 - Math.abs(t - 0.5) * 2) * 0.9 + 20);
  const b = Math.round(255 * Math.max(0, 1 - 2 * t));
  return `rgba(${r},${Math.min(255, g)},${b},${alpha})`;
}
