import { create } from 'zustand';
import type { EntityCategory } from './entityCategory';
import { CATEGORY_META } from './entityCategory';
import { useComposerPrefillStore } from './composerStore';

/**
 * 画板设计 store v3(素材波):实体卡 + 特性子节点 + 挂载素材(图片/建模/纹理等资产引用)。
 * 实体类型 = 内置 role/map + 用户自定义(映射到后端三分类);特性来自 component_list_types,
 * 自定义特性仅作给 AI 的自然语言描述(后端 REGISTRY 是编译期闭集,validate_props 拒未注册类型)。
 * 连线端点可落在实体本身或其某个特性上,进出两向都可起手;交互描述仍写在线上。
 * collapsed = 特性子节点收进卡片右缘端口列(连线不断,只换锚点);v3 老档缺该字段按展开读。
 * 素材 = 资产引用快照({guid, path, type} + 用途备注 + 详情画布坐标),按 guid 去重;
 * openNodeId = 下钻中的实体详情画布,selectedNodeId = 快捷键作用对象,panel = 卡上
 * 展开中的编辑/浮层(三者皆会话态,不持久化)。
 * 持久化:localStorage(forge:designBoard, version 3);v1/v2 旧档自动迁移。
 */

export type KindTone = 'info' | 'sage' | 'warn' | 'acc' | 'danger';

export interface EntityKindDef {
  id: string;
  label: string;
  builtin: boolean;
  tone: KindTone;
  category: EntityCategory;
  defaultFeatures: string[];
}

export interface BoardFeature {
  id: string;
  type: string;
  custom: boolean;
  note: string;
}

/** 实体挂载的素材引用(资产快照:资产面离线时也能显示 path/type;pos = 详情画布坐标)。 */
export interface BoardAssetRef {
  id: string;
  guid: string;
  path: string;
  type: string;
  note: string;
  pos: [number, number];
}

export interface BoardNode {
  id: string;
  kindId: string;
  name: string;
  desc: string;
  pos: [number, number];
  features: BoardFeature[];
  assets: BoardAssetRef[];
  /** 特性子节点是否收进卡片右缘端口列 */
  collapsed: boolean;
}

/** 详情画布素材卡默认落位:中心实例卡(原点)左侧一列,按已有素材数向下排。 */
export function assetDefaultPos(i: number): [number, number] {
  return [-300, i * 148];
}

/** 复制出的实体相对原件的错位(世界坐标),两张卡不重叠才看得出复制成功。 */
export const DUPLICATE_OFFSET = 28;

export interface BoardAnchor {
  node: string;
  feature?: string;
}

/**
 * 卡上的内联编辑 / 浮层(同一时刻只开一个)。
 * 右键菜单与键盘快捷键都落到这里,卡片自身不再各存一份开合态,
 * 否则「菜单点了改名」与「卡上点了标题」会各开各的。
 */
export type BoardPanel =
  | { kind: 'rename'; node: string }
  | { kind: 'feature-menu'; node: string }
  | { kind: 'asset-picker'; node: string }
  | { kind: 'feature-note'; node: string; feature: string };

export interface BoardEdge {
  id: string;
  from: BoardAnchor;
  to: BoardAnchor;
  label: string;
}

export const REGISTERED_COMPONENTS = [
  'MeshRenderer',
  'RigidBody',
  'Light',
  'Camera',
  'Script',
  'Tag',
  'Trigger',
  'Category',
] as const;

export const BUILTIN_KINDS: EntityKindDef[] = [
  {
    id: 'role',
    label: '角色',
    builtin: true,
    tone: 'info',
    category: 'role',
    defaultFeatures: ['MeshRenderer', 'RigidBody'],
  },
  {
    id: 'map',
    label: '地图',
    builtin: true,
    tone: 'sage',
    category: 'map',
    defaultFeatures: ['MeshRenderer'],
  },
];

export const TONE_BAR: Record<KindTone, string> = {
  info: 'bg-info',
  sage: 'bg-sage',
  warn: 'bg-warn',
  acc: 'bg-acc',
  danger: 'bg-danger',
};

const BOARD_KEY = 'forge:designBoard';

/** v2 节点形态(无 assets / collapsed;读档迁移时补空数组与展开态) */
type BoardNodeV2 = Omit<BoardNode, 'assets' | 'collapsed'>;

interface PersistedBoardV3 {
  version: 3;
  seq: number;
  customKinds?: EntityKindDef[];
  nodes: BoardNode[];
  edges: BoardEdge[];
}

interface PersistedBoardV2 {
  version: 2;
  seq: number;
  /** 首选字段;旧草稿可能用 kinds */
  customKinds?: EntityKindDef[];
  kinds?: EntityKindDef[];
  nodes: BoardNodeV2[];
  edges: BoardEdge[];
}

interface PersistedBoardV1 {
  version?: 1;
  seq?: number;
  nodes?: Array<{
    id: string;
    kind?: 'role' | 'map';
    kindId?: string;
    name: string;
    desc: string;
    pos: [number, number];
    features?: BoardFeature[];
  }>;
  edges?: Array<{
    id: string;
    from: string | BoardAnchor;
    to: string | BoardAnchor;
    label: string;
  }>;
}

function isFeature(v: unknown): v is BoardFeature {
  if (typeof v !== 'object' || v === null) return false;
  const f = v as Partial<BoardFeature>;
  return (
    typeof f.id === 'string' &&
    typeof f.type === 'string' &&
    typeof f.custom === 'boolean' &&
    typeof f.note === 'string'
  );
}

function isAssetRef(v: unknown): v is BoardAssetRef {
  if (typeof v !== 'object' || v === null) return false;
  const a = v as Partial<BoardAssetRef>;
  return (
    typeof a.id === 'string' &&
    typeof a.guid === 'string' &&
    typeof a.path === 'string' &&
    typeof a.type === 'string' &&
    typeof a.note === 'string' &&
    Array.isArray(a.pos) &&
    a.pos.length === 2 &&
    a.pos.every((x) => typeof x === 'number' && Number.isFinite(x))
  );
}

function isNodeV2(v: unknown): v is BoardNodeV2 {
  if (typeof v !== 'object' || v === null) return false;
  const n = v as Partial<BoardNode>;
  return (
    typeof n.id === 'string' &&
    typeof n.kindId === 'string' &&
    typeof n.name === 'string' &&
    typeof n.desc === 'string' &&
    Array.isArray(n.pos) &&
    n.pos.length === 2 &&
    n.pos.every((x) => typeof x === 'number' && Number.isFinite(x)) &&
    Array.isArray(n.features) &&
    n.features.every(isFeature)
  );
}

/** collapsed 是 v3 中途加的视图字段:老档缺它照收,读回来一律按展开 */
function isNodeV3(v: unknown): v is Omit<BoardNode, 'collapsed'> {
  if (!isNodeV2(v)) return false;
  const assets = (v as Partial<BoardNode>).assets;
  return Array.isArray(assets) && assets.every(isAssetRef);
}

function withCollapsed(n: Omit<BoardNode, 'collapsed'>): BoardNode {
  return { ...n, collapsed: (n as Partial<BoardNode>).collapsed === true };
}

function isKindDef(v: unknown): v is EntityKindDef {
  if (typeof v !== 'object' || v === null) return false;
  const k = v as Partial<EntityKindDef>;
  return (
    typeof k.id === 'string' &&
    k.id !== '' &&
    typeof k.label === 'string' &&
    typeof k.builtin === 'boolean' &&
    (k.tone === 'info' ||
      k.tone === 'sage' ||
      k.tone === 'warn' ||
      k.tone === 'acc' ||
      k.tone === 'danger') &&
    (k.category === 'role' || k.category === 'map' || k.category === 'interaction') &&
    Array.isArray(k.defaultFeatures) &&
    k.defaultFeatures.every((x) => typeof x === 'string')
  );
}

function parseAnchor(v: unknown): BoardAnchor | null {
  if (typeof v === 'string') return { node: v };
  if (typeof v !== 'object' || v === null) return null;
  const a = v as Partial<BoardAnchor>;
  if (typeof a.node !== 'string') return null;
  if (a.feature !== undefined && typeof a.feature !== 'string') return null;
  return { node: a.node, feature: a.feature };
}

function isEdgeV2(v: unknown): v is BoardEdge {
  if (typeof v !== 'object' || v === null) return false;
  const e = v as Partial<BoardEdge>;
  const from = parseAnchor(e.from);
  const to = parseAnchor(e.to);
  return typeof e.id === 'string' && from !== null && to !== null && typeof e.label === 'string';
}

function migrateV1Node(
  n: NonNullable<PersistedBoardV1['nodes']>[number],
): BoardNode | null {
  const kindId = n.kindId ?? n.kind;
  if (kindId !== 'role' && kindId !== 'map') return null;
  if (
    typeof n.id !== 'string' ||
    typeof n.name !== 'string' ||
    typeof n.desc !== 'string' ||
    !Array.isArray(n.pos) ||
    n.pos.length !== 2
  ) {
    return null;
  }
  return {
    id: n.id,
    kindId,
    name: n.name,
    desc: n.desc,
    pos: n.pos,
    features: Array.isArray(n.features) ? n.features.filter(isFeature) : [],
    assets: [],
    collapsed: false,
  };
}

function anchorValid(anchor: BoardAnchor, nodes: BoardNode[]): boolean {
  const node = nodes.find((n) => n.id === anchor.node);
  if (!node) return false;
  if (!anchor.feature) return true;
  return node.features.some((f) => f.id === anchor.feature);
}

function sameEntity(a: BoardAnchor, b: BoardAnchor): boolean {
  return a.node === b.node;
}

function anchorKey(a: BoardAnchor): string {
  return a.feature ? `${a.node}:${a.feature}` : a.node;
}

export interface LoadedBoard {
  seq: number;
  kinds: EntityKindDef[];
  nodes: BoardNode[];
  edges: BoardEdge[];
}

export function loadBoard(): LoadedBoard {
  const empty: LoadedBoard = { seq: 1, kinds: [...BUILTIN_KINDS], nodes: [], edges: [] };
  try {
    const raw = globalThis.localStorage?.getItem(BOARD_KEY);
    if (!raw) return empty;
    const p = JSON.parse(raw) as PersistedBoardV1 | PersistedBoardV2 | PersistedBoardV3;

    let customKinds: EntityKindDef[] = [];
    let nodes: BoardNode[] = [];
    let edges: BoardEdge[] = [];
    let seq = 1;

    if (p.version === 3) {
      const v3 = p as PersistedBoardV3;
      const rawKinds = Array.isArray(v3.customKinds) ? v3.customKinds : [];
      customKinds = rawKinds
        .filter(isKindDef)
        .filter((k) => !k.builtin && !BUILTIN_KINDS.some((b) => b.id === k.id));
      nodes = Array.isArray(v3.nodes) ? v3.nodes.filter(isNodeV3).map(withCollapsed) : [];
      edges = Array.isArray(v3.edges) ? v3.edges.filter(isEdgeV2) : [];
      seq =
        typeof v3.seq === 'number' && Number.isFinite(v3.seq) && v3.seq >= 1
          ? Math.floor(v3.seq)
          : nodes.length + edges.length + 1;
    } else if (p.version === 2) {
      const v2 = p as PersistedBoardV2;
      const rawKinds = Array.isArray(v2.customKinds)
        ? v2.customKinds
        : Array.isArray(v2.kinds)
          ? v2.kinds
          : [];
      customKinds = rawKinds
        .filter(isKindDef)
        .filter((k) => !k.builtin && !BUILTIN_KINDS.some((b) => b.id === k.id));
      // v2 → v3:节点补空素材清单与展开态
      nodes = Array.isArray(v2.nodes)
        ? v2.nodes.filter(isNodeV2).map((n) => ({ ...n, assets: [], collapsed: false }))
        : [];
      edges = Array.isArray(v2.edges) ? v2.edges.filter(isEdgeV2) : [];
      seq =
        typeof v2.seq === 'number' && Number.isFinite(v2.seq) && v2.seq >= 1
          ? Math.floor(v2.seq)
          : nodes.length + edges.length + 1;
    } else {
      const v1 = p as PersistedBoardV1;
      nodes = Array.isArray(v1.nodes)
        ? v1.nodes.map(migrateV1Node).filter((n): n is BoardNode => n !== null)
        : [];
      edges = Array.isArray(v1.edges)
        ? v1.edges
            .map((e) => {
              const from = parseAnchor(e.from);
              const to = parseAnchor(e.to);
              if (!from || !to || typeof e.id !== 'string' || typeof e.label !== 'string') return null;
              return { id: e.id, from, to, label: e.label };
            })
            .filter((e): e is BoardEdge => e !== null)
        : [];
      seq =
        typeof v1.seq === 'number' && Number.isFinite(v1.seq) && v1.seq >= 1
          ? Math.floor(v1.seq)
          : nodes.length + edges.length + 1;
    }

    const kinds = [...BUILTIN_KINDS, ...customKinds];
    const kindIds = new Set(kinds.map((k) => k.id));
    nodes = nodes.filter((n) => kindIds.has(n.kindId));

    edges = edges.filter(
      (e) =>
        anchorValid(e.from, nodes) &&
        anchorValid(e.to, nodes) &&
        !sameEntity(e.from, e.to) &&
        anchorKey(e.from) !== anchorKey(e.to),
    );

    return { seq, kinds, nodes, edges };
  } catch {
    return empty;
  }
}

function persistBoard(doc: {
  seq: number;
  kinds: EntityKindDef[];
  nodes: BoardNode[];
  edges: BoardEdge[];
}): void {
  try {
    globalThis.localStorage?.setItem(
      BOARD_KEY,
      JSON.stringify({
        version: 3,
        seq: doc.seq,
        customKinds: doc.kinds.filter((k) => !k.builtin),
        nodes: doc.nodes,
        edges: doc.edges,
      } satisfies PersistedBoardV3),
    );
  } catch {
    // 写不进静默
  }
}

export type KindDraft = Omit<EntityKindDef, 'id' | 'builtin'>;

interface DesignBoardState {
  kinds: EntityKindDef[];
  nodes: BoardNode[];
  edges: BoardEdge[];
  seq: number;
  pendingEdgeId: string | null;
  /** 下钻中的实体详情画布(null = 主画板;会话态,不持久化) */
  openNodeId: string | null;
  /** 选中的实体:键盘快捷键的作用对象(会话态,不持久化) */
  selectedNodeId: string | null;
  /** 展开中的卡上编辑 / 浮层(会话态,不持久化) */
  panel: BoardPanel | null;

  /** pos 缺省时按已有实体数排成网格;右键「在此新建」传画布落点 */
  addNode: (kindId: string, pos?: [number, number]) => string | null;
  renameNode: (id: string, name: string) => void;
  setNodeDesc: (id: string, desc: string) => void;
  moveNode: (id: string, pos: [number, number]) => void;
  removeNode: (id: string) => void;
  /** 连特性与素材一起复制一个新实体;连线不复制——交互是两端的约定,复制会凭空造出语义 */
  duplicateNode: (id: string) => string | null;
  /** 特性子节点在「铺开」与「收进卡片右缘端口列」之间切换(连线端点不变) */
  toggleCollapse: (id: string) => void;

  selectNode: (id: string | null) => void;
  openPanel: (panel: BoardPanel) => void;
  closePanel: () => void;

  openNode: (id: string) => void;
  closeNode: () => void;

  addKind: (draft: KindDraft) => string | null;
  updateKind: (id: string, patch: Partial<KindDraft>) => void;
  removeKind: (id: string) => string | null;

  addFeature: (nodeId: string, type: string, custom: boolean) => string | null;
  removeFeature: (nodeId: string, featureId: string) => void;
  setFeatureNote: (nodeId: string, featureId: string, note: string) => void;

  /** 挂载素材(按 guid 去重;pos 缺省按已有素材数排在详情画布左列) */
  attachAsset: (
    nodeId: string,
    ref: { guid: string; path: string; type: string },
    pos?: [number, number],
  ) => string | null;
  detachAsset: (nodeId: string, assetId: string) => void;
  moveAssetRef: (nodeId: string, assetId: string, pos: [number, number]) => void;
  setAssetNote: (nodeId: string, assetId: string, note: string) => void;

  addEdge: (from: BoardAnchor, to: BoardAnchor) => string | null;
  setEdgeLabel: (id: string, label: string) => void;
  removeEdge: (id: string) => void;
  clearBoard: () => void;
  clearPendingEdge: () => void;

  buildPrompt: () => string;
  handoffToAgent: () => void;
}

function makeFeatures(types: string[], seqStart: number): { features: BoardFeature[]; nextSeq: number } {
  let seq = seqStart;
  const features: BoardFeature[] = [];
  for (const type of types) {
    const custom = !REGISTERED_COMPONENTS.includes(type as (typeof REGISTERED_COMPONENTS)[number]);
    features.push({ id: `f${seq}`, type, custom, note: '' });
    seq += 1;
  }
  return { features, nextSeq: seq };
}

function kindLabelTaken(kinds: EntityKindDef[], label: string, exceptId?: string): boolean {
  const t = label.trim();
  return kinds.some((k) => k.id !== exceptId && k.label === t);
}

function edgeTouchesNodeOrFeature(e: BoardEdge, nodeId: string, featureId?: string): boolean {
  if (featureId) {
    return (
      (e.from.node === nodeId && e.from.feature === featureId) ||
      (e.to.node === nodeId && e.to.feature === featureId)
    );
  }
  return e.from.node === nodeId || e.to.node === nodeId;
}

export const useDesignBoardStore = create<DesignBoardState>((set, get) => {
  const commit = (
    patch: Partial<Pick<DesignBoardState, 'kinds' | 'nodes' | 'edges' | 'seq'>>,
  ): void => {
    set(patch);
    const { seq, kinds, nodes, edges } = get();
    persistBoard({ seq, kinds, nodes, edges });
  };

  const resolveAnchorLabel = (anchor: BoardAnchor, nodes: BoardNode[]): string => {
    const node = nodes.find((n) => n.id === anchor.node);
    if (!node) return '?';
    if (!anchor.feature) return node.name;
    const feat = node.features.find((f) => f.id === anchor.feature);
    return feat ? `${node.name}.${feat.type}` : node.name;
  };

  return {
    ...loadBoard(),
    pendingEdgeId: null,
    openNodeId: null,
    selectedNodeId: null,
    panel: null,

    addNode: (kindId, pos) => {
      const { nodes, kinds, seq } = get();
      const kind = kinds.find((k) => k.id === kindId);
      if (!kind) return null;
      if (pos !== undefined && !pos.every((v) => Number.isFinite(v))) return null;
      const id = `n${seq}`;
      let i = nodes.filter((n) => n.kindId === kindId).length + 1;
      while (nodes.some((n) => n.name === `${kind.label} ${i}`)) i += 1;
      const idx = nodes.length;
      const { features, nextSeq } = makeFeatures(kind.defaultFeatures, seq + 1);
      const node: BoardNode = {
        id,
        kindId,
        name: `${kind.label} ${i}`,
        desc: '',
        pos: pos ?? [40 + (idx % 4) * 240, 48 + Math.floor(idx / 4) * 220],
        features,
        assets: [],
        collapsed: false,
      };
      commit({ nodes: [...nodes, node], seq: nextSeq });
      set({ selectedNodeId: id });
      return id;
    },

    renameNode: (id, name) => {
      const t = name.trim();
      if (t === '') return;
      commit({ nodes: get().nodes.map((n) => (n.id === id ? { ...n, name: t } : n)) });
    },

    setNodeDesc: (id, desc) =>
      commit({ nodes: get().nodes.map((n) => (n.id === id ? { ...n, desc } : n)) }),

    // 无限画布:坐标四向无界(可为负),只挡 NaN/Infinity 之类的脏值
    moveNode: (id, pos) => {
      if (!pos.every((v) => Number.isFinite(v))) return;
      commit({
        nodes: get().nodes.map((n) => (n.id === id ? { ...n, pos: [pos[0], pos[1]] } : n)),
      });
    },

    removeNode: (id) => {
      // 正下钻在该实体时收敛回主画板(不留悬空详情页)
      if (get().openNodeId === id) set({ openNodeId: null });
      if (get().selectedNodeId === id) set({ selectedNodeId: null });
      if (get().panel?.node === id) set({ panel: null });
      commit({
        nodes: get().nodes.filter((n) => n.id !== id),
        edges: get().edges.filter((e) => !edgeTouchesNodeOrFeature(e, id)),
      });
    },

    duplicateNode: (id) => {
      const { nodes, seq } = get();
      const src = nodes.find((n) => n.id === id);
      if (!src) return null;
      let next = seq;
      const newId = `n${next}`;
      next += 1;
      const features = src.features.map((f) => {
        const copy = { ...f, id: `f${next}` };
        next += 1;
        return copy;
      });
      const assets = src.assets.map((a) => {
        const copy: BoardAssetRef = { ...a, id: `a${next}`, pos: [a.pos[0], a.pos[1]] };
        next += 1;
        return copy;
      });
      let name = `${src.name} 副本`;
      let i = 2;
      while (nodes.some((n) => n.name === name)) {
        name = `${src.name} 副本 ${i}`;
        i += 1;
      }
      const node: BoardNode = {
        id: newId,
        kindId: src.kindId,
        name,
        desc: src.desc,
        // 错开一格落在原卡右下,不完全盖住原件
        pos: [src.pos[0] + DUPLICATE_OFFSET, src.pos[1] + DUPLICATE_OFFSET],
        features,
        assets,
        collapsed: src.collapsed,
      };
      commit({ nodes: [...nodes, node], seq: next });
      set({ selectedNodeId: newId });
      return newId;
    },

    toggleCollapse: (id) =>
      commit({
        nodes: get().nodes.map((n) => (n.id === id ? { ...n, collapsed: !n.collapsed } : n)),
      }),

    selectNode: (id) => set({ selectedNodeId: id, ...(id === null ? { panel: null } : {}) }),

    openPanel: (panel) => set({ panel, selectedNodeId: panel.node }),

    closePanel: () => set({ panel: null }),

    openNode: (id) => {
      if (!get().nodes.some((n) => n.id === id)) return;
      set({ openNodeId: id });
    },

    closeNode: () => set({ openNodeId: null }),

    addKind: (draft) => {
      const label = draft.label.trim();
      if (label === '' || kindLabelTaken(get().kinds, label)) return null;
      const { kinds, seq } = get();
      const id = `k${seq}`;
      const kind: EntityKindDef = {
        id,
        label,
        builtin: false,
        tone: draft.tone,
        category: draft.category,
        defaultFeatures: [...draft.defaultFeatures],
      };
      commit({ kinds: [...kinds, kind], seq: seq + 1 });
      return id;
    },

    updateKind: (id, patch) => {
      const kinds = get().kinds;
      const target = kinds.find((k) => k.id === id);
      if (!target || target.builtin) return;
      if (patch.label !== undefined) {
        const next = patch.label.trim();
        if (next === '' || kindLabelTaken(kinds, next, id)) return;
      }
      commit({
        kinds: kinds.map((k) => {
          if (k.id !== id || k.builtin) return k;
          return {
            ...k,
            ...(patch.label !== undefined ? { label: patch.label.trim() } : {}),
            ...(patch.tone !== undefined ? { tone: patch.tone } : {}),
            ...(patch.category !== undefined ? { category: patch.category } : {}),
            ...(patch.defaultFeatures !== undefined
              ? { defaultFeatures: [...patch.defaultFeatures] }
              : {}),
          };
        }),
      });
    },

    removeKind: (id) => {
      const kind = get().kinds.find((k) => k.id === id);
      if (!kind) return '类型不存在';
      if (kind.builtin) return '内置类型不可删除';
      if (get().nodes.some((n) => n.kindId === id)) {
        return `仍有节点使用「${kind.label}」,请先删除相关节点`;
      }
      commit({ kinds: get().kinds.filter((k) => k.id !== id) });
      return null;
    },

    addFeature: (nodeId, type, custom) => {
      const t = type.trim();
      if (t === '') return null;
      const { nodes, seq } = get();
      const node = nodes.find((n) => n.id === nodeId);
      if (!node) return null;
      if (node.features.some((f) => f.type === t && f.custom === custom)) return null;
      const id = `f${seq}`;
      const feature: BoardFeature = { id, type: t, custom, note: '' };
      commit({
        nodes: nodes.map((n) => (n.id === nodeId ? { ...n, features: [...n.features, feature] } : n)),
        seq: seq + 1,
      });
      return id;
    },

    removeFeature: (nodeId, featureId) => {
      const panel = get().panel;
      if (panel?.kind === 'feature-note' && panel.node === nodeId && panel.feature === featureId) {
        set({ panel: null });
      }
      commit({
        nodes: get().nodes.map((n) =>
          n.id === nodeId ? { ...n, features: n.features.filter((f) => f.id !== featureId) } : n,
        ),
        edges: get().edges.filter((e) => !edgeTouchesNodeOrFeature(e, nodeId, featureId)),
      });
    },

    setFeatureNote: (nodeId, featureId, note) =>
      commit({
        nodes: get().nodes.map((n) =>
          n.id === nodeId
            ? { ...n, features: n.features.map((f) => (f.id === featureId ? { ...f, note } : f)) }
            : n,
        ),
      }),

    attachAsset: (nodeId, ref, pos) => {
      const guid = ref.guid.trim();
      const path = ref.path.trim();
      if (guid === '' || path === '') return null;
      if (pos !== undefined && !pos.every((v) => Number.isFinite(v))) return null;
      const { nodes, seq } = get();
      const node = nodes.find((n) => n.id === nodeId);
      if (!node) return null;
      if (node.assets.some((a) => a.guid === guid)) return null; // 同资产去重
      const id = `a${seq}`;
      const type = ref.type.trim();
      const asset: BoardAssetRef = {
        id,
        guid,
        path,
        type: type !== '' ? type : 'unknown',
        note: '',
        pos: pos ?? assetDefaultPos(node.assets.length),
      };
      commit({
        nodes: nodes.map((n) => (n.id === nodeId ? { ...n, assets: [...n.assets, asset] } : n)),
        seq: seq + 1,
      });
      return id;
    },

    detachAsset: (nodeId, assetId) =>
      commit({
        nodes: get().nodes.map((n) =>
          n.id === nodeId ? { ...n, assets: n.assets.filter((a) => a.id !== assetId) } : n,
        ),
      }),

    moveAssetRef: (nodeId, assetId, pos) => {
      if (!pos.every((v) => Number.isFinite(v))) return;
      commit({
        nodes: get().nodes.map((n) =>
          n.id === nodeId
            ? {
                ...n,
                assets: n.assets.map((a) => (a.id === assetId ? { ...a, pos: [pos[0], pos[1]] } : a)),
              }
            : n,
        ),
      });
    },

    setAssetNote: (nodeId, assetId, note) =>
      commit({
        nodes: get().nodes.map((n) =>
          n.id === nodeId
            ? { ...n, assets: n.assets.map((a) => (a.id === assetId ? { ...a, note } : a)) }
            : n,
        ),
      }),

    addEdge: (from, to) => {
      const { nodes, edges, seq } = get();
      if (!anchorValid(from, nodes) || !anchorValid(to, nodes)) return null;
      if (sameEntity(from, to) || anchorKey(from) === anchorKey(to)) return null;
      const id = `e${seq}`;
      commit({ edges: [...edges, { id, from, to, label: '' }], seq: seq + 1 });
      set({ pendingEdgeId: id });
      return id;
    },

    setEdgeLabel: (id, label) =>
      commit({ edges: get().edges.map((e) => (e.id === id ? { ...e, label } : e)) }),

    removeEdge: (id) => commit({ edges: get().edges.filter((e) => e.id !== id) }),

    // seq 不回卷:自定义类型仍在档,重置会让后续 addKind 造出与之重复的 id
    clearBoard: () => {
      set({ openNodeId: null, selectedNodeId: null, panel: null });
      commit({ nodes: [], edges: [] });
    },

    clearPendingEdge: () => set({ pendingEdgeId: null }),

    buildPrompt: () => {
      const { kinds, nodes, edges } = get();
      const kindById = new Map(kinds.map((k) => [k.id, k]));
      const usedKindIds = [...new Set(nodes.map((n) => n.kindId))];

      const kindLines =
        usedKindIds.length === 0
          ? '(无)'
          : usedKindIds
              .map((kid) => {
                const k = kindById.get(kid);
                if (!k) return `- ${kid}:未知类型`;
                const cat = CATEGORY_META[k.category].label;
                const tag = k.builtin ? '内置' : '自定义';
                const defs =
                  k.defaultFeatures.length > 0 ? k.defaultFeatures.join(' / ') : '(无默认特性)';
                return `- ${k.label}(${tag},后端分类 ${cat}):默认特性 ${defs}`;
              })
              .join('\n');

      const entityLines =
        nodes.length === 0
          ? '(无)'
          : nodes
              .map((n, i) => {
                const k = kindById.get(n.kindId);
                const kindLabel = k?.label ?? n.kindId;
                const desc = n.desc.trim() !== '' ? n.desc.trim() : '(未填写描述)';
                const featStr =
                  n.features.length === 0
                    ? '   特性:(无)'
                    : `   特性:${n.features
                        .map((f) => {
                          const tag = f.custom ? '[自定义]' : '';
                          const note = f.note.trim() !== '' ? `(备注:${f.note.trim()})` : '';
                          return `${f.type}${tag}${note}`;
                        })
                        .join(' / ')}`;
                const parts = [`${i + 1}. ${n.name} [${kindLabel}]:${desc}`, featStr];
                if (n.assets.length > 0) {
                  parts.push(
                    `   素材:${n.assets
                      .map((a) => {
                        const note = a.note.trim() !== '' ? `(备注:${a.note.trim()})` : '';
                        return `${a.path} [${a.type}, GUID ${a.guid}]${note}`;
                      })
                      .join(' / ')}`,
                  );
                }
                return parts.join('\n');
              })
              .join('\n');

      const edgeLines =
        edges.length === 0
          ? '(无)'
          : edges
              .map((e, i) => {
                const from = resolveAnchorLabel(e.from, nodes);
                const to = resolveAnchorLabel(e.to, nodes);
                const label = e.label.trim() !== '' ? e.label.trim() : '未填写交互描述';
                return `${i + 1}. ${from} →(${label})→ ${to}`;
              })
              .join('\n');

      const customTypes = [
        ...new Set(nodes.flatMap((n) => n.features.filter((f) => f.custom).map((f) => f.type))),
      ];

      const lines = [
        '请按照下面的「画板」流程图设计,为当前工程制作素材与逻辑代码。',
        '',
        '【实体类型定义】',
        kindLines,
        '',
        '【实体】',
        entityLines,
        '',
        '【交互连线】(交互描述写在连线上,方向 = 流程方向;端点可落在实体或其特性上)',
        edgeLines,
        '',
        '要求:',
        '1. 为每个实体节点生成或绑定所需素材(网格 / 材质 / 贴图),命名与节点名对应;实体「素材」行已挂载的资产优先直接引用(GUID 已给出),缺口部分再生成;',
        '2. 为每条交互连线生成对应节点图(.rxgraph)并挂到相关实体的 Script 组件(graphRef);',
        '3. 在场景中创建并摆放对应实体(分类按实体类型的后端映射:角色=role / 地图=map / 交互=interaction),完成后保存场景;',
        '4. 未填写描述之处按工程现状合理补全,先说明设定再执行;完成后汇报素材 / 节点图 / 实体清单。',
      ];

      if (customTypes.length > 0) {
        lines.push(
          `5. 未注册特性 ${customTypes.join(' / ')} 不在引擎组件注册表(${REGISTERED_COMPONENTS.join(' / ')})内,请用已注册组件加脚本或节点图实现等价能力,或说明需要扩展引擎;不要直接调用 component_add 传未注册类型。`,
        );
      }

      return lines.join('\n');
    },

    handoffToAgent: () => {
      useComposerPrefillStore.getState().prefill(get().buildPrompt(), 'build');
    },
  };
});
