import { create } from 'zustand';
import { callAssetTool } from './forgeApi';
import { useAssetStore } from './assetStore';
import { useWorkbenchStore } from './workbenchStore';
import {
  boneTail,
  clampParam,
  clipTime,
  deleteKey,
  keyformIndex,
  moveKey,
  splitTarget,
  upsertKey,
  type Influence,
  type Interp,
  type KeyformGroup,
  type PatchOp,
  type RigBone,
  type RigClip,
  type RigDoc,
  type RigEvalResult,
  type RigPart,
  type RigStats,
  type Vec2,
} from './rigTypes';
import { applyBrush, bindingToWeights, type BrushParams } from '@/components/rig/RigWeightBrush';

/**
 * rigStore(F-GAME-5 W5 / D-050):绑骨编辑器状态——.rxrig 文档本地可变副本(rig_get includeMesh)
 * + 编辑动作(网格顶点 / 骨骼 / 权重笔刷 / 显隐与 drawOrder / keyform 记录 / clip 轨道)
 * + 撤销/重做(doc 快照,手势 coalesce 键合并)+ rig_evaluate 节流(rAF 合帧、在途 ≤ 1、最新者胜)
 * + 保存(diff → rig_patch ops;diff 不可表达或过大 → rig_set 整文档)。
 *
 * 求值单一事实源 = 后端 rig_evaluate(方案 A):它读的是**已落盘**文档,所以网格/骨骼/权重模式
 * 直接画本地静止网格(编辑即见);参数/动画模式画求值结果,本地有未保存改动时视图如实提示
 * 「预览基于已保存版本」。自动网格/权重在服务端改盘:先保存本地改动,再调工具,再整文档重载
 * (重载前的本地文档进撤销栈,撤销后再保存即可回滚)。
 */

export type RigMode = 'mesh' | 'bones' | 'weights' | 'params' | 'animate';

export type RigSelection = { kind: 'part' | 'bone' | 'deformer'; id: string } | null;

/** 工具级错误信封(asset-pipeline 以 200 + {error,message} 返回)。 */
interface ToolErr {
  error?: string;
  message?: string;
}

const UNDO_MAX = 50;
/** ops 数超过此值改走 rig_set(整文档更省且原子)。 */
export const PATCH_OPS_LIMIT = 200;
/** ops JSON 体积超过文档体积此比例时改走 rig_set。 */
const PATCH_SIZE_RATIO = 0.8;

function cloneDoc<T>(doc: T): T {
  return JSON.parse(JSON.stringify(doc)) as T;
}

function round2(v: number): number {
  return Math.round(v * 100) / 100;
}

function sortKeys<T>(rec: Record<string, T>): Record<string, T> {
  const out: Record<string, T> = {};
  for (const k of Object.keys(rec).sort()) out[k] = rec[k];
  return out;
}

/** 后端可能省略的缺省段补齐(serde default 镜像),保证编辑代码可直接读写。 */
export function normalizeRigDoc(raw: RigDoc): RigDoc {
  const d = cloneDoc(raw);
  d.parameters ??= [];
  d.bones ??= [];
  d.deformers ??= [];
  d.parts ??= [];
  d.keyforms ??= [];
  d.physics ??= [];
  d.clips = sortKeys(d.clips ?? {});
  d.pixelsPerUnit ??= 100;
  return d;
}

/** 键序无关的确定性序列化(diff 等值判定用;undefined 字段视为缺失)。 */
export function stableStringify(v: unknown): string {
  if (v === undefined) return 'null';
  if (v === null || typeof v !== 'object') return JSON.stringify(v) ?? 'null';
  if (Array.isArray(v)) return `[${v.map((x) => stableStringify(x)).join(',')}]`;
  const o = v as Record<string, unknown>;
  const keys = Object.keys(o)
    .filter((k) => o[k] !== undefined)
    .sort();
  return `{${keys.map((k) => `${JSON.stringify(k)}:${stableStringify(o[k])}`).join(',')}}`;
}

function eq(a: unknown, b: unknown): boolean {
  return stableStringify(a) === stableStringify(b);
}

/** RFC 7386 merge-patch 字段差:b 有且不同 → 取 b;a 有 b 无 → null(删除)。 */
function fieldsDiff(a: object, b: object, skip: string[] = []): Record<string, unknown> | null {
  const ao = a as Record<string, unknown>;
  const bo = b as Record<string, unknown>;
  const out: Record<string, unknown> = {};
  const keys = new Set([...Object.keys(ao), ...Object.keys(bo)]);
  for (const k of keys) {
    if (skip.includes(k) || k === 'id') continue;
    const av = ao[k];
    const bv = bo[k];
    if (bv === undefined) {
      if (av !== undefined) out[k] = null;
    } else if (!eq(av, bv)) {
      out[k] = bv;
    }
  }
  return Object.keys(out).length > 0 ? out : null;
}

/**
 * 有序 id 列表差:返回 removed/added/common;若公共项相对顺序变了、或新增项没全部排在末尾
 * (add_* 只会 push 到末尾,无法表达插入/重排)→ null,调用方改走 rig_set。
 */
function listDiff(aIds: string[], bIds: string[]): { removed: string[]; added: string[]; common: string[] } | null {
  const aSet = new Set(aIds);
  const bSet = new Set(bIds);
  const removed = aIds.filter((id) => !bSet.has(id));
  const commonA = aIds.filter((id) => bSet.has(id));
  const commonB = bIds.filter((id) => aSet.has(id));
  if (JSON.stringify(commonA) !== JSON.stringify(commonB)) return null;
  const firstAdded = bIds.findIndex((id) => !aSet.has(id));
  if (firstAdded >= 0 && bIds.slice(firstAdded).some((id) => aSet.has(id))) return null;
  return { removed, added: bIds.filter((id) => !aSet.has(id)), common: commonA };
}

const groupKey = (g: Pick<KeyformGroup, 'target' | 'params'>) => `${g.target}|${g.params.join(',')}`;

/** forms 下标 → 各轴关键值坐标(行主序反解)。 */
function decodeAt(g: KeyformGroup, idx: number): Record<string, number> {
  const at: Record<string, number> = {};
  let rem = idx;
  for (let ai = g.params.length - 1; ai >= 0; ai -= 1) {
    const ks = g.keys[ai] ?? [];
    const len = Math.max(1, ks.length);
    at[g.params[ai]] = ks[rem % len] ?? 0;
    rem = Math.floor(rem / len);
  }
  return at;
}

function isEmptyForm(f: object | undefined): boolean {
  return !f || Object.values(f).every((v) => v === undefined);
}

/** 单组 keyform 改动 → ops(只改了 ≤3 个形态时用细粒度 set_keyform,否则整组替换)。 */
function keyformOps(a: KeyformGroup | undefined, b: KeyformGroup): PatchOp[] {
  const fine = 3;
  if (a && eq(a.keys, b.keys) && a.forms.length === b.forms.length) {
    const diffIdx = b.forms.map((f, i) => (eq(a.forms[i], f) ? -1 : i)).filter((i) => i >= 0);
    if (diffIdx.length > 0 && diffIdx.length <= fine) {
      return diffIdx.map((i) => ({
        op: 'set_keyform' as const,
        target: b.target,
        params: b.params,
        at: decodeAt(b, i),
        form: b.forms[i] ?? {},
      }));
    }
  }
  if (!a) {
    const nonEmpty = b.forms.map((f, i) => (isEmptyForm(f) ? -1 : i)).filter((i) => i >= 0);
    if (nonEmpty.length > 0 && nonEmpty.length <= fine) {
      return nonEmpty.map((i, n) => ({
        op: 'set_keyform' as const,
        target: b.target,
        params: b.params,
        ...(n === 0 ? { keys: b.keys } : {}),
        at: decodeAt(b, i),
        form: b.forms[i] ?? {},
      }));
    }
  }
  return [{ op: 'set_keyform_group', group: b }];
}

/**
 * 文档 diff → rig_patch ops(a = 已保存,b = 本地)。返回 null = 无法用 ops 表达
 * (texture/version 变化、列表重排),调用方走 rig_set。
 *
 * 顺序:先删依附项(keyform 组 / clip / 物理),再删实体(部件 / 形变器 / 骨骼 / 参数;
 * 后端删除会级联清悬空引用,先删依附项可避免「组已被级联删掉再删一次」报错);
 * 随后 canvas → 参数 → 骨骼 → 形变器 → 部件(+ set_weights)→ keyform → 物理 → auto → clip → animator。
 * set_* 一律携带 b 的完整字段值(非增量),级联改过的项最终仍收敛到 b。
 */
export function diffRigDocs(a: RigDoc, b: RigDoc): PatchOp[] | null {
  if (a.version !== b.version || a.texture !== b.texture) return null;
  const ops: PatchOp[] = [];
  const removes: PatchOp[] = [];

  // keyform 组
  const kd = listDiff(a.keyforms.map(groupKey), b.keyforms.map(groupKey));
  if (!kd) return null;
  for (const k of kd.removed) {
    const g = a.keyforms.find((x) => groupKey(x) === k);
    if (g) removes.push({ op: 'remove_keyform_group', target: g.target, params: g.params });
  }
  // clip
  for (const name of Object.keys(a.clips)) {
    if (!(name in b.clips)) removes.push({ op: 'remove_clip', name });
  }
  // 物理
  const phd = listDiff(a.physics.map((p) => p.id), b.physics.map((p) => p.id));
  if (!phd) return null;
  for (const id of phd.removed) removes.push({ op: 'remove_physics', id });

  const pd = listDiff(a.parts.map((p) => p.id), b.parts.map((p) => p.id));
  const dd = listDiff(a.deformers.map((d) => d.id), b.deformers.map((d) => d.id));
  const bd = listDiff(a.bones.map((x) => x.id), b.bones.map((x) => x.id));
  const prd = listDiff(a.parameters.map((p) => p.id), b.parameters.map((p) => p.id));
  if (!pd || !dd || !bd || !prd) return null;
  for (const id of pd.removed) removes.push({ op: 'remove_part', id });
  for (const id of dd.removed) removes.push({ op: 'remove_deformer', id });
  for (const id of bd.removed) removes.push({ op: 'remove_bone', id });
  for (const id of prd.removed) removes.push({ op: 'remove_parameter', id });
  ops.push(...removes);

  // canvas / ppu
  if (!eq(a.canvas, b.canvas) || a.pixelsPerUnit !== b.pixelsPerUnit) {
    const op: PatchOp = { op: 'set_canvas' };
    if (!eq(a.canvas.size, b.canvas.size)) op.size = b.canvas.size;
    if (!eq(a.canvas.origin, b.canvas.origin)) op.origin = b.canvas.origin;
    if (a.pixelsPerUnit !== b.pixelsPerUnit) op.pixelsPerUnit = b.pixelsPerUnit;
    ops.push(op);
  }

  // 参数
  for (const p of b.parameters) {
    const old = a.parameters.find((x) => x.id === p.id);
    if (!old) ops.push({ op: 'add_parameter', parameter: p });
    else {
      const f = fieldsDiff(old, p);
      if (f) ops.push({ op: 'set_parameter', id: p.id, fields: f });
    }
  }
  // 骨骼
  for (const bone of b.bones) {
    const old = a.bones.find((x) => x.id === bone.id);
    if (!old) ops.push({ op: 'add_bone', bone });
    else {
      const f = fieldsDiff(old, bone);
      if (f) ops.push({ op: 'set_bone', id: bone.id, fields: f });
    }
  }
  // 形变器
  for (const d of b.deformers) {
    const old = a.deformers.find((x) => x.id === d.id);
    if (!old) ops.push({ op: 'add_deformer', deformer: d });
    else {
      const f = fieldsDiff(old, d);
      if (f) ops.push({ op: 'set_deformer', id: d.id, fields: f });
    }
  }
  // 部件(绑定改动单列 set_weights)
  for (const p of b.parts) {
    const old = a.parts.find((x) => x.id === p.id);
    if (!old) {
      ops.push({ op: 'add_part', part: p });
      continue;
    }
    const f = fieldsDiff(old, p, ['weights', 'bone']);
    if (f) ops.push({ op: 'set_part', id: p.id, fields: f });
    if (!eq(old.weights, p.weights) || (old.bone ?? null) !== (p.bone ?? null)) {
      const sw: PatchOp = { op: 'set_weights', part: p.id };
      if (p.weights) sw.weights = p.weights;
      else if (p.bone) sw.bone = p.bone;
      ops.push(sw);
    }
  }
  // keyform 组
  for (const g of b.keyforms) {
    const old = a.keyforms.find((x) => groupKey(x) === groupKey(g));
    if (old && eq(old, g)) continue;
    ops.push(...keyformOps(old, g));
  }
  // 物理
  for (const ph of b.physics) {
    const old = a.physics.find((x) => x.id === ph.id);
    if (!old || !eq(old, ph)) ops.push({ op: 'set_physics', physics: ph });
  }
  // auto
  if (!eq(a.auto ?? {}, b.auto ?? {})) ops.push({ op: 'set_auto', auto: b.auto ?? {} });
  // clip
  for (const [name, clip] of Object.entries(b.clips)) {
    if (!eq(a.clips[name], clip)) ops.push({ op: 'set_clip', name, clip });
  }
  // animator
  if (!eq(a.animator ?? null, b.animator ?? null)) ops.push({ op: 'set_animator', animator: b.animator ?? null });
  return ops;
}

// ───────────────────────── rig_evaluate 节流 ─────────────────────────

type Scheduler = (cb: () => void) => void;

const defaultScheduler: Scheduler = (cb) => {
  if (typeof requestAnimationFrame === 'function') requestAnimationFrame(() => cb());
  else setTimeout(cb, 16);
};

let scheduler: Scheduler = defaultScheduler;
let evalScheduled = false;
let evalInFlight = false;
let evalAgain = false;

/** 测试注入:替换合帧调度器(null = 恢复 rAF),并清空节流内部态。 */
export function __setRigScheduler(s: Scheduler | null): void {
  scheduler = s ?? defaultScheduler;
  evalScheduled = false;
  evalInFlight = false;
  evalAgain = false;
}

function modeNeedsEval(mode: RigMode): boolean {
  return mode === 'params' || mode === 'animate';
}

/** 参数轴 keyform 记录的一次性入参。 */
export interface KeyformRecordSpec {
  partId: string;
  /** 1..3 个参数轴。 */
  axes: string[];
  /** 每轴关键值(升序;组已存在时须与现有一致)。 */
  keys: number[][];
}

interface RigState {
  assetPath: string | null;
  guid: string | null;
  doc: RigDoc | null;
  /** 已落盘文档(diff 基线)。 */
  savedDoc: RigDoc | null;
  savedJson: string | null;
  dirty: boolean;
  loading: boolean;
  saving: boolean;
  /** 服务端长操作进行中(自动网格 / 自动权重)。 */
  busy: 'mesh' | 'weights' | null;
  error: string | null;
  /** 非错误提示(如自动网格清掉了哪些部件的权重)。 */
  notice: string | null;
  stats: RigStats | null;

  texPath: string | null;
  texDataUrl: string | null;
  texW: number;
  texH: number;

  mode: RigMode;
  selection: RigSelection;
  selectedVertex: number | null;

  /** 当前参数值(编辑器摆姿势;不进文档、不进撤销栈)。 */
  params: Record<string, number>;
  clip: string | null;
  /** 播放头(clip 内归一化时间,秒)。 */
  time: number;
  playing: boolean;

  evaluated: RigEvalResult | null;
  evalBusy: boolean;
  evalError: string | null;
  /** 参数模式下拖动的顶点(显示坐标,画布像素);记录 keyform 时与求值结果做差。 */
  poseEdits: Record<string, Record<number, Vec2>>;

  brush: BrushParams;

  undoStack: string[];
  redoStack: string[];
  undoCoalesce: string | null;

  openRig: (assetPath: string) => Promise<void>;
  /** 整文档重载(服务端改盘后);pushUndo = 重载前本地文档进撤销栈。 */
  reload: (opts?: { pushUndo?: boolean }) => Promise<void>;
  setTextureSize: (w: number, h: number) => void;

  setMode: (m: RigMode) => void;
  select: (sel: RigSelection) => void;
  selectVertex: (idx: number | null) => void;

  setParam: (id: string, v: number) => void;
  resetParams: () => void;
  requestEvaluate: () => void;

  moveVertex: (partId: string, idx: number, pos: Vec2, coalesce?: string) => void;
  moveBoneHead: (id: string, pos: Vec2, opts?: { withChildren?: boolean; coalesce?: string }) => void;
  setBoneTail: (id: string, tail: Vec2, coalesce?: string) => void;
  addBone: (head: Vec2, tail: Vec2, parent: string | null) => string | null;
  removeBone: (id: string) => void;
  setPartVisible: (id: string, visible: boolean) => void;
  moveDrawOrder: (id: string, dir: 1 | -1) => void;
  setBrush: (patch: Partial<BrushParams>) => void;
  brushAt: (partId: string, center: Vec2, coalesce?: string) => void;

  setPoseVertex: (partId: string, idx: number, pos: Vec2) => void;
  clearPoseEdits: (partId?: string) => void;
  recordKeyform: (spec: KeyformRecordSpec) => boolean;

  selectClip: (name: string | null) => void;
  addClip: (name: string) => void;
  removeClip: (name: string) => void;
  setClipMeta: (name: string, patch: { duration?: number; loop?: boolean }) => void;
  addTrack: (clip: string, target: string) => void;
  removeTrack: (clip: string, trackIdx: number) => void;
  setTrackInterp: (clip: string, trackIdx: number, interp: Interp) => void;
  addKey: (clip: string, trackIdx: number, time: number, value: number) => void;
  moveKeyTo: (clip: string, trackIdx: number, keyIdx: number, time: number, coalesce?: string) => number;
  setKeyValue: (clip: string, trackIdx: number, keyIdx: number, value: number, coalesce?: string) => void;
  deleteKeyAt: (clip: string, trackIdx: number, keyIdx: number) => void;
  setTime: (t: number) => void;
  play: () => void;
  pause: () => void;
  tick: (dt: number) => void;

  undo: () => void;
  redo: () => void;
  save: () => Promise<boolean>;
  autoMesh: () => Promise<void>;
  autoWeights: () => Promise<void>;
  reset: () => void;
}

const DEFAULT_BRUSH: BrushParams = { mode: 'add', radius: 40, strength: 0.5, bone: null };

const FRESH = {
  assetPath: null as string | null,
  guid: null as string | null,
  doc: null as RigDoc | null,
  savedDoc: null as RigDoc | null,
  savedJson: null as string | null,
  dirty: false,
  loading: false,
  saving: false,
  busy: null as 'mesh' | 'weights' | null,
  error: null as string | null,
  notice: null as string | null,
  stats: null as RigStats | null,
  texPath: null as string | null,
  texDataUrl: null as string | null,
  texW: 0,
  texH: 0,
  selection: null as RigSelection,
  selectedVertex: null as number | null,
  params: {} as Record<string, number>,
  clip: null as string | null,
  time: 0,
  playing: false,
  evaluated: null as RigEvalResult | null,
  evalBusy: false,
  evalError: null as string | null,
  poseEdits: {} as Record<string, Record<number, Vec2>>,
  undoStack: [] as string[],
  redoStack: [] as string[],
  undoCoalesce: null as string | null,
};

function defaultParams(doc: RigDoc): Record<string, number> {
  const out: Record<string, number> = {};
  for (const p of doc.parameters) out[p.id] = p.default;
  return out;
}

function selectionValid(doc: RigDoc, sel: RigSelection): boolean {
  if (!sel) return true;
  if (sel.kind === 'part') return doc.parts.some((p) => p.id === sel.id);
  if (sel.kind === 'bone') return doc.bones.some((b) => b.id === sel.id);
  return doc.deformers.some((d) => d.id === sel.id);
}

/** 本地删骨(镜像 patch.rs RemoveBone 级联:子骨挂到父、刚性绑定改父、权重剔除、目标引用清理)。 */
function removeBoneLocal(doc: RigDoc, id: string): void {
  const pos = doc.bones.findIndex((b) => b.id === id);
  if (pos < 0) return;
  const removed = doc.bones.splice(pos, 1)[0];
  for (const b of doc.bones) {
    if (b.parent === id) {
      if (removed.parent) b.parent = removed.parent;
      else delete b.parent;
    }
  }
  for (const p of doc.parts) {
    if (p.bone === id) {
      if (removed.parent) p.bone = removed.parent;
      else delete p.bone;
    }
    if (p.weights) p.weights = p.weights.map((inf) => inf.filter(([b]) => b !== id));
  }
  const hits = (t: string) => {
    const s = splitTarget(t);
    return s !== null && s.kind === 'bone' && s.id === id;
  };
  doc.keyforms = doc.keyforms.filter((g) => !hits(g.target));
  for (const c of Object.values(doc.clips)) c.tracks = c.tracks.filter((t) => !hits(t.target));
}

export const useRigStore = create<RigState>((set, get) => {
  /**
   * 变更文档:先推撤销快照(coalesce 键与上次相同则合并,同一拖拽手势一档一条),
   * 清重做栈,应用变更并重算 dirty;失效选中回落。
   */
  const mutate = (fn: (doc: RigDoc) => void, coalesce?: string) => {
    const { doc, undoStack, undoCoalesce, savedJson, selection } = get();
    if (!doc) return;
    const key = coalesce ?? null;
    const stack =
      key !== null && key === undoCoalesce ? undoStack : [...undoStack, JSON.stringify(doc)].slice(-UNDO_MAX);
    const next = cloneDoc(doc);
    fn(next);
    const json = JSON.stringify(next);
    set({
      doc: next,
      undoStack: stack,
      redoStack: [],
      undoCoalesce: key,
      dirty: json !== savedJson,
      selection: selectionValid(next, selection) ? selection : null,
    });
  };

  /** 贴图 GUID → 资产路径(经 assetStore 列表;未加载先拉)。 */
  const resolveTexPath = async (textureGuid: string): Promise<string | null> => {
    const assets = useAssetStore.getState();
    if (assets.items.length === 0) await assets.load();
    return useAssetStore.getState().items.find((i) => i.guid === textureGuid)?.path ?? null;
  };

  const fetchDoc = async (assetPath: string): Promise<{ doc: RigDoc; guid: string | null; stats: RigStats | null }> => {
    const r = await callAssetTool<ToolErr & { guid?: string; doc?: RigDoc; stats?: RigStats }>('rig_get', {
      assetPath,
      includeMesh: true,
    });
    if (r.error || !r.doc) throw new Error(r.message ?? r.error ?? 'rig_get 空响应');
    return { doc: normalizeRigDoc(r.doc), guid: r.guid ?? null, stats: r.stats ?? null };
  };

  const runEvaluate = async () => {
    const s = get();
    if (!s.assetPath || !s.doc) return;
    const reqPath = s.assetPath;
    const args: Record<string, unknown> = { assetPath: reqPath, params: { ...s.params } };
    if (s.mode === 'animate' && s.clip) {
      args.clip = s.clip;
      args.time = s.time;
    }
    evalInFlight = true;
    set({ evalBusy: true });
    try {
      const r = await callAssetTool<ToolErr & Partial<RigEvalResult>>('rig_evaluate', args);
      if (get().assetPath === reqPath) {
        if (r.error || !r.parts) set({ evalError: r.message ?? r.error ?? 'rig_evaluate 空响应' });
        else
          set({
            evaluated: {
              bbox: r.bbox ?? [0, 0, 0, 0],
              parts: r.parts,
              bones: r.bones ?? [],
              unknown: r.unknown ?? [],
            },
            evalError: null,
          });
      }
    } catch (err) {
      if (get().assetPath === reqPath) set({ evalError: (err as Error).message });
    } finally {
      evalInFlight = false;
      set({ evalBusy: false });
      if (evalAgain) {
        evalAgain = false;
        get().requestEvaluate();
      }
    }
  };

  const loadTexture = async (textureGuid: string) => {
    const texPath = await resolveTexPath(textureGuid);
    if (texPath === null) {
      set({ error: `图集贴图 GUID 未在资产列表中解析到路径:${textureGuid}` });
      return;
    }
    set({ texPath });
    try {
      const t = await callAssetTool<ToolErr & { dataUrl?: string; width?: number; height?: number }>(
        'asset_thumbnail',
        { assetPath: texPath },
      );
      if (t.error || !t.dataUrl) {
        set({ error: `图集加载失败:${t.message ?? t.error ?? '空响应'}` });
        return;
      }
      set({ texDataUrl: t.dataUrl });
    } catch (err) {
      set({ error: `图集加载失败:${(err as Error).message}` });
    }
  };

  return {
    ...FRESH,
    mode: 'mesh' as RigMode,
    brush: { ...DEFAULT_BRUSH },

    openRig: async (assetPath) => {
      const mode = get().mode;
      set({ ...FRESH, assetPath, loading: true, mode, brush: { ...DEFAULT_BRUSH } });
      try {
        const { doc, guid, stats } = await fetchDoc(assetPath);
        if (get().assetPath !== assetPath) return;
        set({
          guid,
          doc,
          savedDoc: cloneDoc(doc),
          savedJson: JSON.stringify(doc),
          stats,
          dirty: false,
          params: defaultParams(doc),
          clip: Object.keys(doc.clips)[0] ?? null,
          selection: doc.parts[0] ? { kind: 'part', id: doc.parts[0].id } : null,
          brush: { ...DEFAULT_BRUSH, bone: doc.bones[0]?.id ?? null },
        });
        await loadTexture(doc.texture);
        set({ loading: false });
        if (modeNeedsEval(get().mode)) get().requestEvaluate();
      } catch (err) {
        set({ loading: false, error: (err as Error).message });
      }
    },

    reload: async (opts = {}) => {
      const { assetPath, doc: before, undoStack, selection, params } = get();
      if (!assetPath) return;
      try {
        const { doc, guid, stats } = await fetchDoc(assetPath);
        const nextParams = defaultParams(doc);
        for (const k of Object.keys(nextParams)) if (k in params) nextParams[k] = params[k];
        set({
          guid: guid ?? get().guid,
          doc,
          savedDoc: cloneDoc(doc),
          savedJson: JSON.stringify(doc),
          stats: stats ?? get().stats,
          dirty: false,
          params: nextParams,
          poseEdits: {},
          undoStack: opts.pushUndo && before ? [...undoStack, JSON.stringify(before)].slice(-UNDO_MAX) : undoStack,
          redoStack: [],
          undoCoalesce: null,
          selection: selectionValid(doc, selection) ? selection : null,
        });
        if (modeNeedsEval(get().mode)) get().requestEvaluate();
      } catch (err) {
        set({ error: (err as Error).message });
      }
    },

    setTextureSize: (w, h) => set({ texW: w, texH: h }),

    setMode: (m) => {
      set({ mode: m, playing: false, selectedVertex: null });
      if (modeNeedsEval(m)) get().requestEvaluate();
    },

    select: (sel) => {
      set({ selection: sel, selectedVertex: null });
      if (sel?.kind === 'bone') set({ brush: { ...get().brush, bone: sel.id } });
    },

    selectVertex: (idx) => set({ selectedVertex: idx }),

    setParam: (id, v) => {
      const { doc, params } = get();
      const p = doc?.parameters.find((x) => x.id === id);
      if (!p) return;
      // 姿势改变后,参数模式下拖过的顶点不再对应当前姿势 → 清掉(避免记录到错误关键点)。
      set({ params: { ...params, [id]: clampParam(p, v) }, poseEdits: {} });
      if (modeNeedsEval(get().mode)) get().requestEvaluate();
    },

    resetParams: () => {
      const { doc } = get();
      if (!doc) return;
      set({ params: defaultParams(doc), poseEdits: {} });
      if (modeNeedsEval(get().mode)) get().requestEvaluate();
    },

    requestEvaluate: () => {
      const { assetPath, doc } = get();
      if (!assetPath || !doc) return;
      if (evalInFlight) {
        evalAgain = true;
        return;
      }
      if (evalScheduled) return;
      evalScheduled = true;
      scheduler(() => {
        evalScheduled = false;
        void runEvaluate();
      });
    },

    moveVertex: (partId, idx, pos, coalesce) => {
      mutate((d) => {
        const p = d.parts.find((x) => x.id === partId);
        if (!p || idx < 0 || idx >= p.mesh.vertices.length) return;
        p.mesh.vertices[idx] = [round2(pos[0]), round2(pos[1])];
      }, coalesce);
    },

    moveBoneHead: (id, pos, opts = {}) => {
      mutate((d) => {
        const b = d.bones.find((x) => x.id === id);
        if (!b) return;
        const dx = pos[0] - b.position[0];
        const dy = pos[1] - b.position[1];
        b.position = [round2(pos[0]), round2(pos[1])];
        if (opts.withChildren) {
          // 静止姿态为绝对坐标:后代整体平移,保持骨架形状。
          const moved = new Set([id]);
          let grew = true;
          while (grew) {
            grew = false;
            for (const c of d.bones) {
              if (c.parent && moved.has(c.parent) && !moved.has(c.id)) {
                moved.add(c.id);
                c.position = [round2(c.position[0] + dx), round2(c.position[1] + dy)];
                grew = true;
              }
            }
          }
        }
      }, opts.coalesce);
    },

    setBoneTail: (id, tail, coalesce) => {
      mutate((d) => {
        const b = d.bones.find((x) => x.id === id);
        if (!b) return;
        const dx = tail[0] - b.position[0];
        const dy = tail[1] - b.position[1];
        b.length = round2(Math.hypot(dx, dy));
        if (b.length > 0) b.rotation = round2((Math.atan2(dy, dx) * 180) / Math.PI);
      }, coalesce);
    },

    addBone: (head, tail, parent) => {
      const { doc } = get();
      if (!doc) return null;
      let n = doc.bones.length;
      while (doc.bones.some((b) => b.id === `bone_${n}`)) n += 1;
      const id = `bone_${n}`;
      const dx = tail[0] - head[0];
      const dy = tail[1] - head[1];
      const bone: RigBone = {
        id,
        position: [round2(head[0]), round2(head[1])],
        rotation: round2((Math.atan2(dy, dx) * 180) / Math.PI),
        length: round2(Math.hypot(dx, dy)),
      };
      if (parent && doc.bones.some((b) => b.id === parent)) bone.parent = parent;
      mutate((d) => {
        d.bones.push(bone);
      });
      get().select({ kind: 'bone', id });
      return id;
    },

    removeBone: (id) => {
      mutate((d) => removeBoneLocal(d, id));
    },

    setPartVisible: (id, visible) => {
      mutate((d) => {
        const p = d.parts.find((x) => x.id === id);
        if (!p) return;
        if (visible) delete p.visible;
        else p.visible = false;
      });
    },

    moveDrawOrder: (id, dir) => {
      mutate((d) => {
        const sorted = [...d.parts].sort((x, y) => x.drawOrder - y.drawOrder);
        const i = sorted.findIndex((p) => p.id === id);
        const j = i + dir;
        if (i < 0 || j < 0 || j >= sorted.length) return;
        const me = d.parts.find((p) => p.id === id) as RigPart;
        const other = d.parts.find((p) => p.id === sorted[j].id) as RigPart;
        if (me.drawOrder === other.drawOrder) me.drawOrder += dir;
        else [me.drawOrder, other.drawOrder] = [other.drawOrder, me.drawOrder];
      });
    },

    setBrush: (patch) => set({ brush: { ...get().brush, ...patch } }),

    brushAt: (partId, center, coalesce) => {
      const { doc, brush } = get();
      const part = doc?.parts.find((p) => p.id === partId);
      if (!part) return;
      const cur = bindingToWeights(part);
      const r = applyBrush(cur, part.mesh.vertices, part.mesh.triangles, center, brush);
      if (r.changed.length === 0) return;
      mutate((d) => {
        const p = d.parts.find((x) => x.id === partId);
        if (!p) return;
        p.weights = r.weights;
        delete p.bone;
      }, coalesce);
    },

    setPoseVertex: (partId, idx, pos) => {
      const { poseEdits } = get();
      set({
        poseEdits: {
          ...poseEdits,
          [partId]: { ...(poseEdits[partId] ?? {}), [idx]: [round2(pos[0]), round2(pos[1])] },
        },
      });
    },

    clearPoseEdits: (partId) => {
      if (partId === undefined) set({ poseEdits: {} });
      else {
        const next = { ...get().poseEdits };
        delete next[partId];
        set({ poseEdits: next });
      }
    },

    recordKeyform: ({ partId, axes, keys }) => {
      const { doc, evaluated, poseEdits, params, dirty } = get();
      if (!doc) return false;
      const fail = (msg: string) => {
        set({ error: msg });
        return false;
      };
      if (dirty) return fail('有未保存改动:求值基于已保存文档,请先保存再记录 keyform');
      const part = doc.parts.find((p) => p.id === partId);
      if (!part) return fail(`部件不存在:${partId}`);
      if (axes.length < 1 || axes.length > 3) return fail('keyform 须选 1–3 个参数轴');
      if (keys.length !== axes.length || keys.some((k) => k.length === 0)) return fail('每个参数轴须至少一个关键值');
      const target = `part:${partId}`;
      const existing = doc.keyforms.find((g) => g.target === target && JSON.stringify(g.params) === JSON.stringify(axes));
      const groupKeys = existing ? existing.keys : keys.map((k) => [...k].sort((x, y) => x - y));
      const at: Record<string, number> = {};
      for (const pid of axes) at[pid] = params[pid];
      const idx = keyformIndex({ params: axes, keys: groupKeys }, at);
      if (idx === null) return fail('当前参数值不在关键值上:先用「吸附」按钮把每个轴对齐到关键值');
      const edits = poseEdits[partId];
      if (!edits || Object.keys(edits).length === 0) return fail('该部件在当前姿势下没有拖动过的顶点');
      const ev = evaluated?.parts.find((p) => p.id === partId);
      const n = part.mesh.vertices.length;
      if (!ev || ev.positions.length !== n) return fail('求值结果与本地网格顶点数不一致(先保存并等待求值刷新)');
      const old = existing?.forms[idx]?.offsets;
      // 偏移 = 现有形态偏移 + (拖动后显示位置 − 当前姿势求值位置);keyform 作用于蒙皮/形变器之前,
      // 姿势含骨骼旋转或形变器缩放时为一阶近似(屏幕位移按未变换偏移写入)。
      const offsets: Vec2[] = Array.from({ length: n }, (_, i) => {
        const base: Vec2 = old?.[i] ?? [0, 0];
        const e = edits[i];
        if (!e) return [round2(base[0]), round2(base[1])];
        return [round2(base[0] + e[0] - ev.positions[i][0]), round2(base[1] + e[1] - ev.positions[i][1])];
      });
      mutate((d) => {
        let g = d.keyforms.find((x) => x.target === target && JSON.stringify(x.params) === JSON.stringify(axes));
        if (!g) {
          const total = groupKeys.reduce((s, k) => s * k.length, 1);
          g = { target, params: [...axes], keys: groupKeys, forms: Array.from({ length: total }, () => ({})) };
          d.keyforms.push(g);
        }
        g.forms[idx] = { ...(g.forms[idx] ?? {}), offsets };
      });
      get().clearPoseEdits(partId);
      set({ error: null, notice: `已记录 keyform:${target} @ ${axes.map((a) => `${a}=${at[a]}`).join(', ')}` });
      return true;
    },

    selectClip: (name) => set({ clip: name, time: 0, playing: false }),

    addClip: (name) => {
      const trimmed = name.trim();
      const { doc } = get();
      if (!doc || trimmed === '') return;
      if (doc.clips[trimmed]) {
        set({ error: `clip 已存在:${trimmed}` });
        return;
      }
      mutate((d) => {
        d.clips = sortKeys({ ...d.clips, [trimmed]: { duration: 1, loop: true, tracks: [] } });
      });
      set({ clip: trimmed, time: 0, playing: false });
    },

    removeClip: (name) => {
      mutate((d) => {
        delete d.clips[name];
      });
      if (get().clip === name) set({ clip: Object.keys(get().doc?.clips ?? {})[0] ?? null, time: 0, playing: false });
    },

    setClipMeta: (name, patch) => {
      mutate(
        (d) => {
          const c = d.clips[name];
          if (!c) return;
          if (patch.duration !== undefined && patch.duration > 0) c.duration = round2(patch.duration * 50) / 50;
          if (patch.loop !== undefined) c.loop = patch.loop;
        },
        `clipmeta:${name}:${Object.keys(patch).join(',')}`,
      );
    },

    addTrack: (clip, target) => {
      mutate((d) => {
        const c = d.clips[clip];
        if (!c || c.tracks.some((t) => t.target === target)) return;
        c.tracks.push({ target, keys: [] });
      });
    },

    removeTrack: (clip, trackIdx) => {
      mutate((d) => {
        const c = d.clips[clip];
        if (c) c.tracks = c.tracks.filter((_, i) => i !== trackIdx);
      });
    },

    setTrackInterp: (clip, trackIdx, interp) => {
      mutate((d) => {
        const t = d.clips[clip]?.tracks[trackIdx];
        if (!t) return;
        if (interp === 'linear') delete t.interp;
        else t.interp = interp;
      });
    },

    addKey: (clip, trackIdx, time, value) => {
      mutate((d) => {
        const c = d.clips[clip];
        const t = c?.tracks[trackIdx];
        if (!c || !t) return;
        const tt = Math.min(c.duration, Math.max(0, Math.round(time * 1000) / 1000));
        t.keys = upsertKey(t.keys, tt, Math.round(value * 1000) / 1000);
      });
    },

    moveKeyTo: (clip, trackIdx, keyIdx, time, coalesce) => {
      let index = keyIdx;
      mutate((d) => {
        const c = d.clips[clip];
        const t = c?.tracks[trackIdx];
        if (!c || !t) return;
        const tt = Math.min(c.duration, Math.max(0, Math.round(time * 1000) / 1000));
        const r = moveKey(t.keys, keyIdx, tt);
        t.keys = r.keys;
        index = r.index;
      }, coalesce);
      return index;
    },

    setKeyValue: (clip, trackIdx, keyIdx, value, coalesce) => {
      mutate((d) => {
        const k = d.clips[clip]?.tracks[trackIdx]?.keys[keyIdx];
        if (k) k[1] = Math.round(value * 1000) / 1000;
      }, coalesce);
    },

    deleteKeyAt: (clip, trackIdx, keyIdx) => {
      mutate((d) => {
        const t = d.clips[clip]?.tracks[trackIdx];
        if (t) t.keys = deleteKey(t.keys, keyIdx);
      });
    },

    setTime: (t) => {
      const { doc, clip } = get();
      const c: RigClip | undefined = clip ? doc?.clips[clip] : undefined;
      set({ time: c ? clipTime({ duration: c.duration, loop: false }, t) : Math.max(0, t) });
      if (get().mode === 'animate') get().requestEvaluate();
    },

    play: () => {
      const { doc, clip, time } = get();
      const c = clip ? doc?.clips[clip] : undefined;
      if (!c) return;
      // 非循环停在末尾时再按播放 = 从头。
      set({ playing: true, ...(!c.loop && time >= c.duration ? { time: 0 } : {}) });
    },

    pause: () => set({ playing: false }),

    tick: (dt) => {
      const { doc, clip, time, playing } = get();
      const c = clip ? doc?.clips[clip] : undefined;
      if (!playing || !c) return;
      const raw = time + dt;
      if (!c.loop && raw >= c.duration) set({ time: c.duration, playing: false });
      else set({ time: clipTime(c, raw) });
      if (get().mode === 'animate') get().requestEvaluate();
    },

    undo: () => {
      const { undoStack, redoStack, doc, savedJson, selection } = get();
      const last = undoStack[undoStack.length - 1];
      if (last === undefined || !doc) return;
      const prev = JSON.parse(last) as RigDoc;
      set({
        doc: prev,
        undoStack: undoStack.slice(0, -1),
        redoStack: [...redoStack, JSON.stringify(doc)].slice(-UNDO_MAX),
        undoCoalesce: null,
        dirty: JSON.stringify(prev) !== savedJson,
        selection: selectionValid(prev, selection) ? selection : null,
      });
    },

    redo: () => {
      const { undoStack, redoStack, doc, savedJson, selection } = get();
      const last = redoStack[redoStack.length - 1];
      if (last === undefined || !doc) return;
      const next = JSON.parse(last) as RigDoc;
      set({
        doc: next,
        redoStack: redoStack.slice(0, -1),
        undoStack: [...undoStack, JSON.stringify(doc)].slice(-UNDO_MAX),
        undoCoalesce: null,
        dirty: JSON.stringify(next) !== savedJson,
        selection: selectionValid(next, selection) ? selection : null,
      });
    },

    save: async () => {
      const { assetPath, doc, savedDoc, saving } = get();
      if (!assetPath || !doc || saving) return false;
      const ops = savedDoc ? diffRigDocs(savedDoc, doc) : null;
      if (ops !== null && ops.length === 0) {
        set({ dirty: false, savedJson: JSON.stringify(doc) });
        return true;
      }
      const docJson = JSON.stringify(doc);
      const useSet =
        ops === null || ops.length > PATCH_OPS_LIMIT || JSON.stringify(ops).length > docJson.length * PATCH_SIZE_RATIO;
      set({ saving: true, error: null });
      try {
        const r = useSet
          ? await callAssetTool<ToolErr & { stats?: RigStats }>('rig_set', {
              assetPath,
              doc: doc as unknown as Record<string, unknown>,
            })
          : await callAssetTool<ToolErr & { stats?: RigStats }>('rig_patch', { assetPath, ops });
        if (r.error) {
          set({ saving: false, error: r.message ?? r.error });
          return false;
        }
        // 保存期间可能又有编辑:基线取发起保存时的快照,dirty 按当前文档重算。
        set({
          saving: false,
          savedDoc: cloneDoc(doc),
          savedJson: docJson,
          dirty: JSON.stringify(get().doc) !== docJson,
          stats: r.stats ?? get().stats,
        });
        if (modeNeedsEval(get().mode)) get().requestEvaluate();
        return true;
      } catch (err) {
        set({ saving: false, error: (err as Error).message });
        return false;
      }
    },

    autoMesh: async () => {
      const { assetPath, busy, selection } = get();
      if (!assetPath || busy) return;
      if (get().dirty && !(await get().save())) return;
      set({ busy: 'mesh', error: null, notice: null });
      try {
        const args: Record<string, unknown> = { assetPath };
        if (selection?.kind === 'part') args.parts = [selection.id];
        const r = await callAssetTool<
          ToolErr & { parts?: Array<{ id: string; vertices: number; triangles: number }>; weightsCleared?: string[] }
        >('rig_mesh_auto', args);
        if (r.error) {
          set({ busy: null, error: r.message ?? r.error });
          return;
        }
        await get().reload({ pushUndo: true });
        const summary = (r.parts ?? []).map((p) => `${p.id}:${p.vertices}v/${p.triangles}t`).join(' ');
        const cleared = r.weightsCleared?.length ? ` · 已清空权重:${r.weightsCleared.join('、')}` : '';
        set({ busy: null, notice: `自动网格完成 ${summary}${cleared}` });
      } catch (err) {
        set({ busy: null, error: (err as Error).message });
      }
    },

    autoWeights: async () => {
      const { assetPath, busy, selection } = get();
      if (!assetPath || busy) return;
      if (get().dirty && !(await get().save())) return;
      set({ busy: 'weights', error: null, notice: null });
      try {
        const args: Record<string, unknown> = { assetPath };
        if (selection?.kind === 'part') args.parts = [selection.id];
        const r = await callAssetTool<ToolErr & { parts?: Array<{ id: string; dominantBones?: Record<string, number> }> }>(
          'rig_weights_auto',
          args,
        );
        if (r.error) {
          set({ busy: null, error: r.message ?? r.error });
          return;
        }
        await get().reload({ pushUndo: true });
        set({ busy: null, notice: `自动权重完成:${(r.parts ?? []).map((p) => p.id).join('、') || '(无部件)'}` });
      } catch (err) {
        set({ busy: null, error: (err as Error).message });
      }
    },

    reset: () => set({ ...FRESH, mode: 'mesh', brush: { ...DEFAULT_BRUSH } }),
  };
});

/** 打开绑骨编辑器页签并载入资产(资产面板双击 / 右键 / 检视器按钮共用)。 */
export function openRigEditor(assetPath: string): void {
  void useRigStore.getState().openRig(assetPath);
  useWorkbenchStore.getState().openTab('rig-editor');
}

/** 部件当前显示用顶点:参数/动画模式取求值结果(顶点数一致时),否则静止网格;参数模式叠加拖动编辑。 */
export function displayPositions(
  part: RigPart,
  mode: RigMode,
  evaluated: RigEvalResult | null,
  poseEdits: Record<string, Record<number, Vec2>>,
): Vec2[] {
  if (!modeNeedsEval(mode)) return part.mesh.vertices;
  const ev = evaluated?.parts.find((p) => p.id === part.id);
  const base = ev && ev.positions.length === part.mesh.vertices.length ? ev.positions : part.mesh.vertices;
  const edits = mode === 'params' ? poseEdits[part.id] : undefined;
  if (!edits) return base;
  return base.map((v, i) => edits[i] ?? v);
}

/** 骨骼显示线段:静止姿态(网格/骨骼/权重模式);参数/动画模式优先用求值结果。 */
export function displayBones(
  doc: RigDoc,
  mode: RigMode,
  evaluated: RigEvalResult | null,
): Array<{ id: string; head: Vec2; tail: Vec2 }> {
  if (modeNeedsEval(mode) && evaluated && evaluated.bones.length > 0) return evaluated.bones;
  return doc.bones.map((b) => ({ id: b.id, head: b.position, tail: boneTail(b) }));
}

export type { Influence };
