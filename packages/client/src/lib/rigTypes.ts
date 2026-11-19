/**
 * .rxrig v1 文档的 TS 镜像(F-GAME-5 / D-050;与 crates/forge-rig2d/src/doc.rs serde 形态逐字段对齐)
 * + rig_patch 操作联合(patch.rs PatchOp,tag = "op",snake_case)+ MCP 工具返回形态
 * + 轨道采样纯函数(与 eval.rs sample_track / clip_time 同语义,时间轴预览与测试共用)。
 *
 * 坐标约定(doc.rs 文件头):画布像素 Y 向下;mesh.vertices = 画布像素,mesh.uvs = 图集像素;
 * 骨骼 position/rotation 为绝对静止姿态(像素 / 度,Y 向下 → 正角视觉顺时针);
 * keyform / clip 对骨骼的修改为相对静止姿态的局部增量。
 * keyform 组 forms 行主序:最后一个参数轴变化最快 idx = ((i0*n1)+i1)*n2+i2。
 */

export type Vec2 = [number, number];

export interface RigCanvas {
  size: Vec2;
  origin: Vec2;
}

export interface RigParam {
  id: string;
  min: number;
  max: number;
  default: number;
  /** 循环参数:越界取模而非夹取(缺省 false,序列化时省略)。 */
  repeat?: boolean;
}

export interface RigBone {
  id: string;
  parent?: string;
  /** 绝对静止关节位置(画布像素)。 */
  position: Vec2;
  /** 绝对静止朝向(度)。 */
  rotation: number;
  /** 骨长(像素;0 = 点骨)。 */
  length: number;
}

export type RigDeformer =
  | { id: string; parent?: string; type: 'warp'; rect: [number, number, number, number]; grid: [number, number] }
  | { id: string; parent?: string; type: 'rotation'; origin: Vec2 };

export interface RigMesh {
  vertices: Vec2[];
  uvs: Vec2[];
  triangles: [number, number, number][];
}

/** rig_get includeMesh=false 时的网格摘要形态。 */
export interface RigMeshSummary {
  vertexCount: number;
  triangleCount: number;
  uvBounds: [number, number, number, number];
}

export type PartBlend = 'alpha' | 'additive' | 'multiply';

/** 单顶点影响:[骨骼 id, 权重](serde 元组 → JSON 二元数组)。 */
export type Influence = [string, number];

export interface RigPart {
  id: string;
  drawOrder: number;
  mesh: RigMesh;
  /** 每顶点 ≤4 个影响;与 bone 互斥。 */
  weights?: Influence[][];
  /** 刚性绑定骨骼。 */
  bone?: string;
  deformer?: string;
  /** 缺省 1(序列化时省略)。 */
  opacity?: number;
  /** 缺省 alpha(序列化时省略)。 */
  blend?: PartBlend;
  /** 缺省 true(序列化时省略;只有 false 才落盘)。 */
  visible?: boolean;
  /** v1 不支持(非空 → RIG_FEATURE_UNSUPPORTED)。 */
  mask?: string[];
}

export interface Keyform {
  offsets?: Vec2[];
  opacity?: number;
  drawOrder?: number;
  position?: Vec2;
  rotation?: number;
  scale?: Vec2;
}

export interface KeyformGroup {
  /** "part:<id>" | "deformer:<id>" | "bone:<id>" */
  target: string;
  params: string[];
  keys: number[][];
  forms: Keyform[];
}

export interface PhysicsInput {
  param: string;
  weight?: number;
  kind: 'x' | 'angle';
}

export interface PhysicsSegment {
  length: number;
  damping?: number;
  stiffness?: number;
}

export interface PhysicsOutput {
  param: string;
  segment?: number;
  scale?: number;
}

export interface PhysicsRig {
  id: string;
  inputs: PhysicsInput[];
  segments: PhysicsSegment[];
  outputs: PhysicsOutput[];
  gravity?: number;
  inputScale?: number;
}

export interface AutoBlink {
  params: string[];
  intervalSec?: Vec2;
  closeSec?: number;
  holdSec?: number;
  openSec?: number;
}

export interface AutoBreath {
  param: string;
  periodSec?: number;
  amplitude?: number;
}

export interface RigAuto {
  blink?: AutoBlink;
  breath?: AutoBreath;
}

export type Interp = 'linear' | 'step' | 'smooth';

export interface RigTrack {
  /** "param:<id>" | "bone:<id>.rotation|x|y|sx|sy" | "part:<id>.opacity" */
  target: string;
  /** [[时间秒, 值], ...] 按时间升序。 */
  keys: Vec2[];
  /** 缺省 linear(序列化时省略)。 */
  interp?: Interp;
}

export interface RigClip {
  duration: number;
  loop: boolean;
  tracks: RigTrack[];
}

export type AnimatorCond = { param: string; eq: boolean } | { trigger: string };

export interface AnimatorTransition {
  from: string | string[];
  to: string;
  when?: AnimatorCond[];
  hasExitTime?: boolean;
}

export interface RigAnimator {
  defaultState: string;
  parameters?: Record<string, string>;
  states: Record<string, { clip: string }>;
  transitions?: AnimatorTransition[];
}

export interface RigDoc {
  version: number;
  texture: string;
  pixelsPerUnit: number;
  canvas: RigCanvas;
  parameters: RigParam[];
  bones: RigBone[];
  deformers: RigDeformer[];
  parts: RigPart[];
  keyforms: KeyformGroup[];
  physics: PhysicsRig[];
  auto?: RigAuto;
  clips: Record<string, RigClip>;
  animator?: RigAnimator;
}

/** RigDoc::stats()(MCP 返回 camelCase)。 */
export interface RigStats {
  parts: number;
  vertices: number;
  triangles: number;
  bones: number;
  deformers: number;
  parameters: number;
  keyformGroups: number;
  physics: number;
  clips: number;
  hasAnimator: boolean;
}

/** rig_evaluate 返回。positions 为画布像素,按 drawOrder 排序。 */
export interface RigEvalPart {
  id: string;
  drawOrder: number;
  opacity: number;
  positions: Vec2[];
}

export interface RigEvalResult {
  bbox: [number, number, number, number];
  parts: RigEvalPart[];
  bones: Array<{ id: string; head: Vec2; tail: Vec2 }>;
  unknown: string[];
}

/** rig_patch 操作(patch.rs PatchOp;tag "op" snake_case,deny_unknown_fields)。 */
export type PatchOp =
  | { op: 'add_part'; part: RigPart }
  | { op: 'set_part'; id: string; fields: Record<string, unknown> }
  | { op: 'remove_part'; id: string }
  | { op: 'add_bone'; bone: RigBone }
  | { op: 'set_bone'; id: string; fields: Record<string, unknown> }
  | { op: 'remove_bone'; id: string }
  | { op: 'set_weights'; part: string; weights?: Influence[][]; bone?: string }
  | { op: 'add_parameter'; parameter: RigParam }
  | { op: 'set_parameter'; id: string; fields: Record<string, unknown> }
  | { op: 'remove_parameter'; id: string }
  | { op: 'add_deformer'; deformer: RigDeformer }
  | { op: 'set_deformer'; id: string; fields: Record<string, unknown> }
  | { op: 'remove_deformer'; id: string }
  | {
      op: 'set_keyform';
      target: string;
      params: string[];
      keys?: number[][];
      at: Record<string, number>;
      form: Keyform;
    }
  | { op: 'set_keyform_group'; group: KeyformGroup }
  | { op: 'remove_keyform_group'; target: string; params: string[] }
  | { op: 'set_physics'; physics: PhysicsRig }
  | { op: 'remove_physics'; id: string }
  | { op: 'set_auto'; auto: RigAuto }
  | { op: 'set_clip'; name: string; clip: RigClip }
  | { op: 'remove_clip'; name: string }
  | { op: 'set_animator'; animator: RigAnimator | null }
  | { op: 'set_canvas'; size?: Vec2; origin?: Vec2; pixelsPerUnit?: number }
  | { op: 'merge_scaffold'; template: string; overwrite?: boolean };

// ───────────────────────── 目标串 ─────────────────────────

export type TargetKind = 'part' | 'deformer' | 'bone' | 'param';

/** 解析 "kind:id[.channel]"(与 doc.rs split_target 同规则:仅 bone/part 带通道,参数 id 可含 '.')。 */
export function splitTarget(t: string): { kind: TargetKind; id: string; channel: string | null } | null {
  const i = t.indexOf(':');
  if (i < 0) return null;
  const k = t.slice(0, i);
  const rest = t.slice(i + 1);
  if (k !== 'part' && k !== 'deformer' && k !== 'bone' && k !== 'param') return null;
  if (rest === '') return null;
  if (k === 'bone' || k === 'part') {
    const j = rest.lastIndexOf('.');
    if (j > 0 && j < rest.length - 1) return { kind: k, id: rest.slice(0, j), channel: rest.slice(j + 1) };
  }
  return { kind: k, id: rest, channel: null };
}

// ───────────────────────── 轨道采样(eval.rs 镜像) ─────────────────────────

function lerp(a: number, b: number, t: number): number {
  return a + (b - a) * t;
}

/** Rust f32::rem_euclid 语义(结果恒非负)。 */
function remEuclid(a: number, b: number): number {
  const r = a % b;
  return r < 0 ? r + Math.abs(b) : r;
}

/** clip 时间归一化(循环取模;非循环夹取)。duration ≤ 0 时返回 0(Rust 端会得 NaN,前端兜底)。 */
export function clipTime(clip: Pick<RigClip, 'duration' | 'loop'>, t: number): number {
  if (!(clip.duration > 0)) return 0;
  return clip.loop ? remEuclid(t, clip.duration) : Math.min(clip.duration, Math.max(0, t));
}

/**
 * 轨道在时间 t 的值(t 已归一化)。与 eval.rs sample_track 逐分支对齐:
 * 空轨 0;t ≤ 首键 → 首值;t ≥ 末键 → 末值;区间 i = partition_point(key.t ≤ t) − 1;
 * span ≤ 1e-9 → 右值;linear u / step 0 / smooth u²(3−2u)。
 */
export function sampleTrack(track: Pick<RigTrack, 'keys' | 'interp'>, t: number): number {
  const k = track.keys;
  if (k.length === 0) return 0;
  if (t <= k[0][0]) return k[0][1];
  const last = k[k.length - 1];
  if (t >= last[0]) return last[1];
  let pp = 0;
  while (pp < k.length && k[pp][0] <= t) pp += 1;
  const i = Math.max(0, pp - 1);
  const a = k[i];
  const b = k[Math.min(i + 1, k.length - 1)];
  const span = b[0] - a[0];
  if (span <= 1e-9) return b[1];
  let u = (t - a[0]) / span;
  const interp = track.interp ?? 'linear';
  if (interp === 'step') u = 0;
  else if (interp === 'smooth') u = u * u * (3 - 2 * u);
  return lerp(a[1], b[1], u);
}

/** 时间相同(±1e-4)的键视为同一键。 */
const KEY_EPS = 1e-4;

/** 插入/覆盖关键帧(按时间升序;同时刻覆盖值)。返回新数组。 */
export function upsertKey(keys: Vec2[], time: number, value: number): Vec2[] {
  const out = keys.filter((k) => Math.abs(k[0] - time) > KEY_EPS);
  out.push([time, value]);
  out.sort((a, b) => a[0] - b[0]);
  return out;
}

/** 移动第 idx 个关键帧到新时间(碰撞同刻键时覆盖之),返回 { keys, index }(移动后的新下标)。 */
export function moveKey(keys: Vec2[], idx: number, time: number): { keys: Vec2[]; index: number } {
  const k = keys[idx];
  if (!k) return { keys, index: idx };
  const moved: Vec2 = [time, k[1]];
  const rest = keys.filter((_, i) => i !== idx && Math.abs(keys[i][0] - time) > KEY_EPS);
  rest.push(moved);
  rest.sort((a, b) => a[0] - b[0]);
  return { keys: rest, index: rest.indexOf(moved) };
}

export function deleteKey(keys: Vec2[], idx: number): Vec2[] {
  return keys.filter((_, i) => i !== idx);
}

/** 骨骼尾端(画布像素):head + length·(cos r, sin r),r 为度。 */
export function boneTail(b: Pick<RigBone, 'position' | 'rotation' | 'length'>): Vec2 {
  const r = (b.rotation * Math.PI) / 180;
  return [b.position[0] + Math.cos(r) * b.length, b.position[1] + Math.sin(r) * b.length];
}

/** 参数夹取(与 RigParam::clamp 同规则:非有限 → default;repeat 取模)。 */
export function clampParam(p: RigParam, v: number): number {
  if (!Number.isFinite(v)) return p.default;
  if (p.repeat) {
    const span = p.max - p.min;
    if (span <= 0) return p.min;
    return p.min + remEuclid(v - p.min, span);
  }
  return Math.min(p.max, Math.max(p.min, v));
}

/** keyform 组内「参数值坐标」→ forms 下标(行主序,最后一轴最快);任一轴不在关键值上 → null。 */
export function keyformIndex(group: Pick<KeyformGroup, 'params' | 'keys'>, at: Record<string, number>): number | null {
  let idx = 0;
  for (let ai = 0; ai < group.params.length; ai += 1) {
    const v = at[group.params[ai]];
    const ks = group.keys[ai] ?? [];
    if (v === undefined) return null;
    const ki = ks.findIndex((k) => Math.abs(k - v) < 1e-3);
    if (ki < 0) return null;
    idx = idx * ks.length + ki;
  }
  return idx;
}
