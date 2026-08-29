import { cleanup, fireEvent, render, screen } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import DesignBoardView from '@/components/editor/DesignBoardView';
import { ASSET_H, ASSET_W } from '@/components/editor/DesignBoardDetailView';
import { NODE_W, collapsedPortY } from '@/components/editor/DesignBoardNode';
import EditorView from '@/views/EditorView';
import {
  BUILTIN_KINDS,
  DUPLICATE_OFFSET,
  assetDefaultPos,
  loadBoard,
  useDesignBoardStore,
} from '@/lib/designBoardStore';
import { useAssetStore } from '@/lib/assetStore';
import { useComposerPrefillStore } from '@/lib/composerStore';
import { useEditorStore, type ComponentTypeInfo } from '@/lib/editorStore';
import { useToastStore } from '@/lib/toastStore';
import { mockForgeBackend } from './forgeMock';

/**
 * 画板波 v2/v3:节点绑定 ECS 特性 + 挂载素材与实例下钻详情画布。
 * store 单测(实体类型 / 特性 / 素材 / 锚点边 / 选中与面板 / 复制 / v1、v2 迁移 /
 * buildPrompt)+ 视图集成(特性子节点、特性下拉、自定义类型表单、特性端点连线、
 * 交给 AI 预填、素材选择器挂载、下钻详情画布:素材卡拖动与备注 / 交互幽灵卡跳转 /
 * 拖放挂载、画布 / 实体 / 特性三套右键菜单与等效键盘快捷键)。
 */

const BOARD_KEY = 'forge:designBoard';

/** component_list_types 实返形态的子集(名称 + 字段简表) */
const COMPONENT_TYPES: ComponentTypeInfo[] = [
  { name: 'MeshRenderer', fields: [{ name: 'mesh', type: 'string' }, { name: 'material', type: 'string' }] },
  { name: 'RigidBody', fields: [{ name: 'kind', type: 'enum:static|dynamic|kinematic' }] },
  { name: 'Script', fields: [{ name: 'module', type: 'string' }, { name: 'graphRef', type: 'string' }] },
  { name: 'Trigger', fields: [{ name: 'kind', type: 'enum:box' }] },
];

const initialBoard = useDesignBoardStore.getState();
const initialEditor = useEditorStore.getState();
const initialPrefill = useComposerPrefillStore.getState();
const initialAssets = useAssetStore.getState();

const board = () => useDesignBoardStore.getState();

beforeEach(() => {
  globalThis.localStorage.clear();
  useDesignBoardStore.setState(
    { ...initialBoard, ...loadBoard(), pendingEdgeId: null, selectedNodeId: null, panel: null },
    true,
  );
  useEditorStore.setState({ ...initialEditor, componentTypes: COMPONENT_TYPES }, true);
  useComposerPrefillStore.setState(initialPrefill, true);
  useToastStore.setState({ items: [] });
});

afterEach(() => {
  cleanup();
  vi.unstubAllGlobals();
});

/**
 * 指针事件:jsdom 无 PointerEvent 构造器,用同名类型的 MouseEvent 承载
 * (clientX/clientY/button 齐全,React 按事件名派发,行为与真机一致)。
 */
function pointer(type: string, init: MouseEventInit = {}): MouseEvent {
  return new MouseEvent(type, { bubbles: true, cancelable: true, ...init });
}

/**
 * 拖放事件:jsdom 无 DragEvent 构造器,用 MouseEvent 承载并挂 dataTransfer
 * (React 从原生事件读 dataTransfer;types/getData 形态与 AssetsPanel dragstart 写入一致)。
 */
function assetDrop(
  data: Record<string, string>,
  init: MouseEventInit = {},
): MouseEvent {
  const e = new MouseEvent('drop', { bubbles: true, cancelable: true, ...init });
  Object.defineProperty(e, 'dataTransfer', {
    value: {
      types: Object.keys(data),
      getData: (k: string) => data[k] ?? '',
    },
  });
  return e;
}

/** 取某实体上指定组件名的特性(取不到直接抛,避免断言里连串非空断言) */
function featureOf(nodeId: string, type: string) {
  const f = board()
    .nodes.find((n) => n.id === nodeId)
    ?.features.find((x) => x.type === type);
  if (!f) throw new Error(`实体 ${nodeId} 上没有特性 ${type}`);
  return f;
}

describe('designBoardStore v3 — 实体类型', () => {
  it('内置角色/地图就位,自带默认特性', () => {
    const kinds = board().kinds;
    expect(kinds.map((k) => k.id)).toEqual(['role', 'map']);
    expect(kinds.every((k) => k.builtin)).toBe(true);
    expect(kinds[0]).toMatchObject({ label: '角色', category: 'role' });
    expect(kinds[0].defaultFeatures).toEqual(BUILTIN_KINDS[0].defaultFeatures);
    expect(kinds[0].defaultFeatures.length).toBeGreaterThan(0);
  });

  it('addKind:新建自定义类型并落盘(只存自定义项);空名 / 撞名拒绝', () => {
    const id = board().addKind({
      label: '敌人',
      tone: 'danger',
      category: 'role',
      defaultFeatures: ['MeshRenderer', 'RigidBody'],
    });
    expect(id).not.toBeNull();
    expect(board().kinds.find((k) => k.id === id)).toMatchObject({
      label: '敌人',
      builtin: false,
      tone: 'danger',
      category: 'role',
    });

    expect(board().addKind({ label: '   ', tone: 'warn', category: 'map', defaultFeatures: [] })).toBeNull();
    expect(board().addKind({ label: '敌人', tone: 'warn', category: 'map', defaultFeatures: [] })).toBeNull();
    expect(board().addKind({ label: '角色', tone: 'warn', category: 'map', defaultFeatures: [] })).toBeNull();
    expect(board().kinds).toHaveLength(3);

    const raw = JSON.parse(globalThis.localStorage.getItem(BOARD_KEY)!) as {
      version: number;
      customKinds: Array<{ id: string; builtin: boolean }>;
    };
    expect(raw.version).toBe(3);
    expect(raw.customKinds).toHaveLength(1);
    expect(raw.customKinds[0].id).toBe(id);
  });

  it('removeKind:内置拒删,占用中拒删并给出原因,空闲可删', () => {
    expect(board().removeKind('role')).toBe('内置类型不可删除');
    expect(board().removeKind('nope')).toBe('类型不存在');

    const id = board().addKind({ label: '道具', tone: 'acc', category: 'interaction', defaultFeatures: [] })!;
    const n = board().addNode(id)!;
    expect(board().removeKind(id)).toContain('道具');
    expect(board().kinds.some((k) => k.id === id)).toBe(true);

    board().removeNode(n);
    expect(board().removeKind(id)).toBeNull();
    expect(board().kinds.some((k) => k.id === id)).toBe(false);
  });

  it('updateKind:改名/配色/分类生效,内置与撞名忽略', () => {
    const id = board().addKind({ label: '道具', tone: 'acc', category: 'map', defaultFeatures: [] })!;
    board().updateKind(id, { label: '可拾取物', tone: 'warn', category: 'interaction' });
    expect(board().kinds.find((k) => k.id === id)).toMatchObject({
      label: '可拾取物',
      tone: 'warn',
      category: 'interaction',
    });

    board().updateKind(id, { label: '角色' }); // 与内置撞名 → 整笔忽略
    expect(board().kinds.find((k) => k.id === id)?.label).toBe('可拾取物');

    board().updateKind('role', { label: '主角' }); // 内置 → 忽略
    expect(board().kinds.find((k) => k.id === 'role')?.label).toBe('角色');
  });
});

describe('designBoardStore v3 — 实体与特性', () => {
  it('addNode:按类型默认特性铺开子节点(有几个特性就有几个),未知类型返 null', () => {
    const id = board().addNode('role')!;
    const n = board().nodes.find((x) => x.id === id)!;
    const roleKind = board().kinds.find((k) => k.id === 'role')!;

    expect(n.name).toBe('角色 1');
    expect(n.features.map((f) => f.type)).toEqual(roleKind.defaultFeatures);
    expect(n.features.every((f) => !f.custom)).toBe(true);
    // 特性 id 与实体 id 共用 seq,互不相撞
    expect(new Set([n.id, ...n.features.map((f) => f.id)]).size).toBe(n.features.length + 1);

    expect(board().addNode('nope')).toBeNull();
  });

  it('自定义类型的默认特性里含未注册名时,自动标记为自定义特性', () => {
    const id = board().addKind({
      label: '敌人',
      tone: 'danger',
      category: 'role',
      defaultFeatures: ['MeshRenderer', '仇恨列表'],
    })!;
    const n = board().addNode(id)!;
    const feats = board().nodes.find((x) => x.id === n)!.features;
    expect(feats.find((f) => f.type === 'MeshRenderer')!.custom).toBe(false);
    expect(feats.find((f) => f.type === '仇恨列表')!.custom).toBe(true);
  });

  it('addFeature:已注册 / 自定义都能加,同名去重,空名与悬空实体拒绝', () => {
    const id = board().addNode('map')!; // 默认只有 MeshRenderer
    expect(board().addFeature(id, 'Trigger', false)).not.toBeNull();
    expect(board().addFeature(id, '寻路网格', true)).not.toBeNull();
    expect(board().addFeature(id, 'Trigger', false)).toBeNull();
    expect(board().addFeature(id, '  ', false)).toBeNull();
    expect(board().addFeature('nope', 'Trigger', false)).toBeNull();

    const feats = board().nodes.find((n) => n.id === id)!.features;
    expect(feats.map((f) => f.type)).toEqual(['MeshRenderer', 'Trigger', '寻路网格']);
    expect(feats.find((f) => f.type === '寻路网格')!.custom).toBe(true);
  });

  it('removeFeature 级联删除挂该特性的连线,实体本身与其它边不受影响', () => {
    const a = board().addNode('role')!;
    const b = board().addNode('map')!;
    board().addFeature(a, 'Script', false);
    const script = featureOf(a, 'Script');

    board().addEdge({ node: a, feature: script.id }, { node: b }); // 特性端点边
    board().addEdge({ node: a }, { node: b }); // 实体端点边
    expect(board().edges).toHaveLength(2);

    board().removeFeature(a, script.id);
    expect(board().edges).toHaveLength(1);
    expect(board().edges[0].from.feature).toBeUndefined();
    expect(board().nodes.find((n) => n.id === a)!.features.some((f) => f.type === 'Script')).toBe(false);
  });

  it('setFeatureNote 写入说明', () => {
    const id = board().addNode('map')!;
    const f = board().nodes.find((n) => n.id === id)!.features[0];
    board().setFeatureNote(id, f.id, '石墙材质');
    expect(board().nodes.find((n) => n.id === id)!.features[0].note).toBe('石墙材质');
  });
});

describe('designBoardStore v3 — 连线与持久化', () => {
  it('addEdge:特性端点可连,同实体自环(含实体↔自身特性)与悬空锚拒绝', () => {
    const a = board().addNode('role')!;
    const b = board().addNode('map')!;
    const fa = board().nodes.find((n) => n.id === a)!.features[0];

    const e = board().addEdge({ node: a, feature: fa.id }, { node: b });
    expect(e).not.toBeNull();
    expect(board().edges[0].from).toMatchObject({ node: a, feature: fa.id });
    expect(board().edges[0].to.feature).toBeUndefined();
    expect(board().pendingEdgeId).toBe(e);

    expect(board().addEdge({ node: a }, { node: a })).toBeNull();
    expect(board().addEdge({ node: a, feature: fa.id }, { node: a })).toBeNull();
    expect(board().addEdge({ node: a }, { node: 'nope' })).toBeNull();
    expect(board().addEdge({ node: a, feature: 'nope' }, { node: b })).toBeNull();
    expect(board().edges).toHaveLength(1);
  });

  it('removeNode 级联删除以该实体或其特性为端点的连线', () => {
    const a = board().addNode('role')!;
    const b = board().addNode('map')!;
    const fa = board().nodes.find((n) => n.id === a)!.features[0];
    board().addEdge({ node: a, feature: fa.id }, { node: b });
    board().addEdge({ node: b }, { node: a });

    board().removeNode(a);
    expect(board().nodes.map((n) => n.id)).toEqual([b]);
    expect(board().edges).toHaveLength(0);
  });

  it('clearBoard 清实体与连线,自定义类型保留且 seq 不回卷(防 id 重复)', () => {
    const kindId = board().addKind({ label: '敌人', tone: 'danger', category: 'role', defaultFeatures: [] })!;
    board().addNode(kindId);
    board().clearBoard();

    expect(board().nodes).toHaveLength(0);
    expect(board().edges).toHaveLength(0);
    expect(board().kinds.some((k) => k.id === kindId)).toBe(true);

    const second = board().addKind({ label: '道具', tone: 'acc', category: 'map', defaultFeatures: [] })!;
    expect(second).not.toBe(kindId);
    expect(new Set(board().kinds.map((k) => k.id)).size).toBe(board().kinds.length);
  });

  it('loadBoard:坏档回空板;v2 旧档迁移回读(节点补空素材)并丢弃冒名内置 / 孤儿节点 / 悬空与失效锚', () => {
    globalThis.localStorage.setItem(BOARD_KEY, '{bad json');
    const bad = loadBoard();
    expect(bad.nodes).toEqual([]);
    expect(bad.kinds.map((k) => k.id)).toEqual(['role', 'map']);

    globalThis.localStorage.setItem(
      BOARD_KEY,
      JSON.stringify({
        version: 2,
        seq: 9,
        customKinds: [
          { id: 'k5', label: '敌人', builtin: false, tone: 'danger', category: 'role', defaultFeatures: [] },
          { id: 'role', label: '冒名内置', builtin: false, tone: 'warn', category: 'map', defaultFeatures: [] },
        ],
        nodes: [
          {
            id: 'n1',
            kindId: 'k5',
            name: '哥布林',
            desc: '',
            pos: [10, 20],
            features: [{ id: 'f1', type: 'Script', custom: false, note: '' }],
          },
          { id: 'n2', kindId: '不存在的类型', name: '孤儿', desc: '', pos: [0, 0], features: [] },
        ],
        edges: [
          { id: 'e1', from: { node: 'n1' }, to: { node: 'nX' }, label: '悬空' },
          { id: 'e2', from: { node: 'n1' }, to: { node: 'n1' }, label: '自环' },
          { id: 'e3', from: { node: 'n1', feature: 'fX' }, to: { node: 'n1' }, label: '失效特性锚' },
        ],
      }),
    );
    const doc = loadBoard();
    expect(doc.seq).toBe(9);
    expect(doc.kinds.map((k) => k.id)).toEqual(['role', 'map', 'k5']); // 冒名内置项丢弃
    expect(doc.kinds.find((k) => k.id === 'role')!.label).toBe('角色');
    expect(doc.nodes.map((n) => n.id)).toEqual(['n1']); // 类型缺失的孤儿节点丢弃
    expect(doc.nodes[0].features).toHaveLength(1);
    expect(doc.nodes[0].assets).toEqual([]); // v2 → v3 迁移补空素材清单
    expect(doc.edges).toHaveLength(0);
  });

  it('loadBoard:v1 旧档迁移(kind → kindId、补空特性、字符串端点 → 锚点)', () => {
    globalThis.localStorage.setItem(
      BOARD_KEY,
      JSON.stringify({
        version: 1,
        seq: 4,
        nodes: [
          { id: 'n1', kind: 'role', name: '玩家', desc: '小球', pos: [40, 48] },
          { id: 'n2', kind: 'map', name: '迷宫', desc: '', pos: [250, 48] },
        ],
        edges: [{ id: 'e3', from: 'n1', to: 'n2', label: '碰墙停止' }],
      }),
    );
    const doc = loadBoard();
    expect(doc.nodes.map((n) => n.kindId)).toEqual(['role', 'map']);
    expect(doc.nodes.every((n) => Array.isArray(n.features) && n.features.length === 0)).toBe(true);
    expect(doc.nodes.every((n) => Array.isArray(n.assets) && n.assets.length === 0)).toBe(true);
    expect(doc.edges).toHaveLength(1);
    expect(doc.edges[0].from).toEqual({ node: 'n1' });
    expect(doc.edges[0].to).toEqual({ node: 'n2' });
    expect(doc.edges[0].label).toBe('碰墙停止');
  });
});

describe('designBoardStore v3 — 素材与下钻', () => {
  it('attachAsset:挂载 / 按 guid 去重 / 默认落位阶梯;显式 pos 生效;空 guid 与悬空实体拒绝', () => {
    const id = board().addNode('role')!;
    const a1 = board().attachAsset(id, { guid: 'g-1', path: 'textures/a.png', type: 'texture' });
    expect(a1).not.toBeNull();
    const a2 = board().attachAsset(id, { guid: 'g-2', path: 'meshes/b.glb', type: 'mesh' }, [12, 34]);
    expect(a2).not.toBeNull();

    expect(board().attachAsset(id, { guid: 'g-1', path: 'textures/dup.png', type: 'texture' })).toBeNull();
    expect(board().attachAsset(id, { guid: '  ', path: 'x.png', type: 'texture' })).toBeNull();
    expect(board().attachAsset('nope', { guid: 'g-3', path: 'x.png', type: 'texture' })).toBeNull();
    expect(board().attachAsset(id, { guid: 'g-4', path: 'x.png', type: 'texture' }, [NaN, 0])).toBeNull();

    const assets = board().nodes.find((n) => n.id === id)!.assets;
    expect(assets.map((a) => a.guid)).toEqual(['g-1', 'g-2']);
    expect(assets[0].pos).toEqual(assetDefaultPos(0));
    expect(assets[1].pos).toEqual([12, 34]);
    // 素材 id 与实体/特性共用 seq,互不相撞
    expect(new Set(assets.map((a) => a.id)).size).toBe(2);
  });

  it('setAssetNote / moveAssetRef(脏值拒绝)/ detachAsset', () => {
    const id = board().addNode('map')!;
    const a = board().attachAsset(id, { guid: 'g-1', path: 'textures/wall.png', type: 'texture' })!;

    board().setAssetNote(id, a, '石墙贴图');
    board().moveAssetRef(id, a, [-50, 60]);
    let asset = board().nodes.find((n) => n.id === id)!.assets[0];
    expect(asset.note).toBe('石墙贴图');
    expect(asset.pos).toEqual([-50, 60]);

    board().moveAssetRef(id, a, [Number.NaN, 0]);
    asset = board().nodes.find((n) => n.id === id)!.assets[0];
    expect(asset.pos).toEqual([-50, 60]);

    board().detachAsset(id, a);
    expect(board().nodes.find((n) => n.id === id)!.assets).toHaveLength(0);
  });

  it('素材随 v3 落盘并回读(guid/path/type/note/pos 全量)', () => {
    const id = board().addNode('role')!;
    const a = board().attachAsset(id, { guid: 'g-9', path: 'meshes/rock.glb', type: 'mesh' }, [5, 6])!;
    board().setAssetNote(id, a, '主体网格');

    const raw = JSON.parse(globalThis.localStorage.getItem(BOARD_KEY)!) as { version: number };
    expect(raw.version).toBe(3);

    const doc = loadBoard();
    expect(doc.nodes[0].assets).toHaveLength(1);
    expect(doc.nodes[0].assets[0]).toMatchObject({
      guid: 'g-9',
      path: 'meshes/rock.glb',
      type: 'mesh',
      note: '主体网格',
      pos: [5, 6],
    });
  });

  it('openNode/closeNode:悬空 id 忽略;removeNode / clearBoard 收敛 openNodeId', () => {
    const id = board().addNode('role')!;
    board().openNode('nope');
    expect(board().openNodeId).toBeNull();

    board().openNode(id);
    expect(board().openNodeId).toBe(id);
    board().closeNode();
    expect(board().openNodeId).toBeNull();

    board().openNode(id);
    board().removeNode(id);
    expect(board().openNodeId).toBeNull();

    const id2 = board().addNode('map')!;
    board().openNode(id2);
    board().clearBoard();
    expect(board().openNodeId).toBeNull();
  });
});

describe('designBoardStore v3 — buildPrompt / handoff', () => {
  it('提示词含类型定义、特性清单、特性端点连线与未注册特性警示', () => {
    const enemy = board().addKind({
      label: '敌人',
      tone: 'danger',
      category: 'role',
      defaultFeatures: ['MeshRenderer', 'Script'],
    })!;
    const p = board().addNode('role')!;
    const m = board().addNode('map')!;
    board().addNode(enemy);

    board().renameNode(p, '玩家');
    board().setNodeDesc(p, '第三人称小球,蓝色金属材质');
    board().renameNode(m, '迷宫');

    board().addFeature(p, 'Script', false);
    const script = featureOf(p, 'Script');
    board().setFeatureNote(p, script.id, '移动控制');
    board().addFeature(p, '体力条', true);

    const e = board().addEdge({ node: p, feature: script.id }, { node: m })!;
    board().setEdgeLabel(e, '碰到墙壁停止移动');

    const text = board().buildPrompt();
    expect(text).toContain('【实体类型定义】');
    expect(text).toContain('- 角色(内置,后端分类 角色)');
    expect(text).toContain('- 敌人(自定义,后端分类 角色):默认特性 MeshRenderer / Script');
    expect(text).toContain('1. 玩家 [角色]:第三人称小球,蓝色金属材质');
    expect(text).toContain('Script(备注:移动控制)');
    expect(text).toContain('体力条[自定义]');
    expect(text).toContain('迷宫 [地图]:(未填写描述)');
    // 端点落在特性上时写成「实体.特性」
    expect(text).toContain('1. 玩家.Script →(碰到墙壁停止移动)→ 迷宫');
    // 未注册特性如实警示,并禁止直接 component_add
    expect(text).toContain('未注册特性 体力条');
    expect(text).toContain('不在引擎组件注册表');
    expect(text).toContain('component_add');
  });

  it('无自定义特性时不追加警示条', () => {
    board().addNode('map');
    const text = board().buildPrompt();
    expect(text).not.toContain('未注册特性');
    expect(text).toContain('4. 未填写描述之处');
  });

  it('handoffToAgent 预填 Composer(build 模式)', () => {
    board().addNode('role');
    board().handoffToAgent();
    const pf = useComposerPrefillStore.getState();
    expect(pf.draft).toContain('【实体类型定义】');
    expect(pf.draft).toContain('角色 1');
    expect(pf.mode).toBe('build');
    expect(pf.token).toBe(1);
  });

  it('已挂载素材写进实体清单(path/type/GUID/备注),要求条款改为优先引用', () => {
    const p = board().addNode('role')!;
    const a = board().attachAsset(p, { guid: 'g-hero', path: 'textures/hero.png', type: 'texture' })!;
    board().setAssetNote(p, a, '主体贴图');
    board().attachAsset(p, { guid: 'g-mesh', path: 'meshes/hero.glb', type: 'mesh' });

    const text = board().buildPrompt();
    expect(text).toContain('素材:textures/hero.png [texture, GUID g-hero](备注:主体贴图) / meshes/hero.glb [mesh, GUID g-mesh]');
    expect(text).toContain('已挂载的资产优先直接引用');

    // 无素材实体不输出「素材」行
    board().detachAsset(p, a);
    board().nodes.find((n) => n.id === p)!.assets.forEach((x) => board().detachAsset(p, x.id));
    expect(board().buildPrompt()).not.toContain('素材:');
  });
});

describe('<DesignBoardView /> — 特性子节点', () => {
  it('空态引导;添加实体后特性子节点数 = 特性数,并画出从属 spine', () => {
    const { container } = render(<DesignBoardView />);
    expect(screen.getByText(/画板为空/)).toBeInTheDocument();

    fireEvent.click(screen.getByTestId('board-add-role'));
    const roleFeatureCount = board().nodes[0].features.length;
    expect(container.querySelectorAll('[data-board-node]')).toHaveLength(1);
    expect(container.querySelectorAll('[data-board-feature]')).toHaveLength(roleFeatureCount);
    expect(container.querySelectorAll('[data-board-spine]')).toHaveLength(1);
    expect(screen.getByTestId(`board-feature-type-${board().nodes[0].features[0].id}`)).toHaveTextContent(
      'MeshRenderer',
    );

    fireEvent.click(screen.getByTestId('board-add-map'));
    const total = board().nodes.reduce((s, n) => s + n.features.length, 0);
    expect(container.textContent).toContain(`2 实体 · ${total} 特性 · 0 连线`);
    expect(screen.queryByText(/画板为空/)).not.toBeInTheDocument();
  });

  it('特性下拉:列出后端组件清单,已有项禁用,可加已注册与自定义特性', () => {
    render(<DesignBoardView />);
    fireEvent.click(screen.getByTestId('board-add-map'));
    const nodeId = board().nodes[0].id;

    fireEvent.click(screen.getByTestId(`board-add-feature-${nodeId}`));
    expect(screen.getByTestId(`board-feature-menu-${nodeId}`)).toBeInTheDocument();
    // 地图默认已有 MeshRenderer → 该项禁用;其余可选
    expect(screen.getByTestId(`board-feature-opt-${nodeId}-MeshRenderer`)).toBeDisabled();
    expect(screen.getByTestId(`board-feature-opt-${nodeId}-Trigger`)).toBeEnabled();

    fireEvent.click(screen.getByTestId(`board-feature-opt-${nodeId}-Trigger`));
    expect(board().nodes[0].features.map((f) => f.type)).toEqual(['MeshRenderer', 'Trigger']);

    fireEvent.click(screen.getByTestId(`board-add-feature-${nodeId}`));
    const input = screen.getByTestId(`board-feature-custom-${nodeId}`);
    fireEvent.change(input, { target: { value: '寻路网格' } });
    fireEvent.keyDown(input, { key: 'Enter' });
    expect(featureOf(nodeId, '寻路网格').custom).toBe(true);
  });

  it('组件清单为空(后端离线)时如实提示,仍可加自定义特性', () => {
    useEditorStore.setState({ componentTypes: [] });
    render(<DesignBoardView />);
    fireEvent.click(screen.getByTestId('board-add-map'));
    const nodeId = board().nodes[0].id;

    fireEvent.click(screen.getByTestId(`board-add-feature-${nodeId}`));
    expect(screen.getByText(/组件清单未加载/)).toBeInTheDocument();
    expect(screen.getByTestId(`board-feature-custom-${nodeId}`)).toBeInTheDocument();
  });

  it('特性说明可内联编辑;删除特性移除子节点', () => {
    render(<DesignBoardView />);
    fireEvent.click(screen.getByTestId('board-add-map'));
    const nodeId = board().nodes[0].id;
    const f = board().nodes[0].features[0];

    fireEvent.click(screen.getByTestId(`board-feature-note-${f.id}`));
    const input = screen.getByTestId(`board-feature-note-input-${f.id}`);
    fireEvent.change(input, { target: { value: '石墙材质' } });
    fireEvent.keyDown(input, { key: 'Enter' });
    expect(board().nodes[0].features[0].note).toBe('石墙材质');

    fireEvent.click(screen.getByTestId(`board-feature-remove-${f.id}`));
    expect(board().nodes.find((n) => n.id === nodeId)!.features).toHaveLength(0);
    expect(screen.queryByTestId(`board-feature-type-${f.id}`)).not.toBeInTheDocument();
  });
});

describe('<DesignBoardView /> — 自定义类型与连线', () => {
  it('新类型表单:保存后工具栏出现该类型按钮,建出的实体带默认特性', () => {
    render(<DesignBoardView />);
    fireEvent.click(screen.getByTestId('board-kind-new'));

    fireEvent.change(screen.getByTestId('board-kind-label'), { target: { value: '敌人' } });
    fireEvent.click(screen.getByTestId('board-kind-tone-danger'));
    fireEvent.change(screen.getByTestId('board-kind-category'), { target: { value: 'role' } });
    fireEvent.click(screen.getByTestId('board-kind-feat-MeshRenderer'));
    fireEvent.click(screen.getByTestId('board-kind-feat-Script'));
    fireEvent.click(screen.getByTestId('board-kind-save'));

    const kind = board().kinds.find((k) => k.label === '敌人')!;
    expect(kind).toMatchObject({ builtin: false, tone: 'danger', category: 'role' });
    expect(kind.defaultFeatures).toEqual(['MeshRenderer', 'Script']);
    expect(screen.queryByTestId('board-kind-form')).not.toBeInTheDocument();

    fireEvent.click(screen.getByTestId(`board-add-${kind.id}`));
    expect(board().nodes[0].name).toBe('敌人 1');
    expect(board().nodes[0].features.map((f) => f.type)).toEqual(['MeshRenderer', 'Script']);
  });

  it('新类型表单:撞名如实报错且不落库', () => {
    render(<DesignBoardView />);
    fireEvent.click(screen.getByTestId('board-kind-new'));
    fireEvent.change(screen.getByTestId('board-kind-label'), { target: { value: '角色' } });
    fireEvent.click(screen.getByTestId('board-kind-save'));

    expect(screen.getByTestId('board-kind-error')).toBeInTheDocument();
    expect(board().kinds).toHaveLength(2);
  });

  it('自定义类型占用中不可删,如实给出原因', () => {
    const id = board().addKind({ label: '道具', tone: 'acc', category: 'interaction', defaultFeatures: [] })!;
    render(<DesignBoardView />);
    fireEvent.click(screen.getByTestId(`board-add-${id}`));

    fireEvent.click(screen.getByTestId(`board-kind-edit-${id}`));
    fireEvent.click(screen.getByTestId('board-kind-delete'));
    expect(screen.getByTestId('board-kind-error')).toHaveTextContent('道具');
    expect(board().kinds.some((k) => k.id === id)).toBe(true);
  });

  it('特性端点连线:边渲染 + 标签自动进编辑,提交后写入 store', () => {
    const a = board().addNode('role')!;
    const b = board().addNode('map')!;
    board().addFeature(a, 'Script', false);
    const script = featureOf(a, 'Script');
    const e = board().addEdge({ node: a, feature: script.id }, { node: b })!;

    const { container } = render(<DesignBoardView />);
    expect(container.querySelectorAll('[data-board-edge]')).toHaveLength(1);

    const input = screen.getByTestId(`board-edge-input-${e}`);
    fireEvent.change(input, { target: { value: '碰到墙壁停止' } });
    fireEvent.keyDown(input, { key: 'Enter' });
    expect(board().edges[0].label).toBe('碰到墙壁停止');

    // 悬浮标题标明端点落在哪个特性上
    expect(container.querySelector(`[data-board-edge-label="${e}"]`)).toHaveAttribute(
      'title',
      '角色 1.Script → 地图 1',
    );
  });

  it('交给 AI:空板禁用,点击后预填含特性清单 + toast', () => {
    render(<DesignBoardView />);
    expect(screen.getByTestId('board-handoff')).toBeDisabled();

    fireEvent.click(screen.getByTestId('board-add-role'));
    fireEvent.click(screen.getByTestId('board-handoff'));

    const pf = useComposerPrefillStore.getState();
    expect(pf.draft).toContain('角色 1');
    expect(pf.draft).toContain('MeshRenderer');
    expect(pf.mode).toBe('build');
    expect(useToastStore.getState().items.some((t) => t.title.includes('已预填'))).toBe(true);
  });
});

describe('<DesignBoardView /> — 无限画布', () => {
  /** jsdom 无布局面(getBoundingClientRect 全 0),世界层 transform 即视口真值 */
  const world = () => screen.getByTestId('board-world');

  it('空白处拖拽平移世界层,松手后位移保留', () => {
    render(<DesignBoardView />);
    expect(world().style.transform).toBe('translate(0px, 0px) scale(1)');

    fireEvent(screen.getByTestId('board-canvas'), pointer('pointerdown', { button: 0, clientX: 100, clientY: 80 }));
    fireEvent(window, pointer('pointermove', { clientX: 160, clientY: 120 }));
    fireEvent(window, pointer('pointerup'));

    expect(world().style.transform).toBe('translate(60px, 40px) scale(1)');
  });

  it('实体卡上按下不平移画布,拖拽可越过原点到负坐标(四向无界)', () => {
    board().addNode('role');
    const { container } = render(<DesignBoardView />);
    const card = container.querySelector('[data-board-node]')!;
    expect(board().nodes[0].pos).toEqual([40, 48]);

    fireEvent(card, pointer('pointerdown', { button: 0, clientX: 100, clientY: 100 }));
    fireEvent(card, pointer('pointermove', { clientX: 20, clientY: 20 }));
    fireEvent(card, pointer('pointerup'));

    expect(board().nodes[0].pos).toEqual([-40, -32]);
    expect(world().style.transform).toBe('translate(0px, 0px) scale(1)');
  });

  it('HUD:放大改倍率与世界层缩放,点百分比回 100%', () => {
    render(<DesignBoardView />);
    fireEvent.click(screen.getByTestId('board-zoom-in'));

    expect(screen.getByTestId('board-zoom-reset')).toHaveTextContent('120%');
    expect(world().style.transform).toContain('scale(1.2)');

    fireEvent.click(screen.getByTestId('board-zoom-reset'));
    expect(world().style.transform).toBe('translate(0px, 0px) scale(1)');
    expect(screen.getByTestId('board-zoom-reset')).toHaveTextContent('100%');
  });

  it('缩放后拖拽按世界坐标换算(指针不与卡片脱手)', () => {
    board().addNode('role');
    const { container } = render(<DesignBoardView />);
    fireEvent.click(screen.getByTestId('board-zoom-in')); // k = 1.2

    const card = container.querySelector('[data-board-node]')!;
    fireEvent(card, pointer('pointerdown', { button: 0, clientX: 120, clientY: 120 })); // 世界 (100,100)
    fireEvent(card, pointer('pointermove', { clientX: 60, clientY: 60 })); // 世界 (50,50)
    fireEvent(card, pointer('pointerup'));

    const [x, y] = board().nodes[0].pos;
    expect(x).toBeCloseTo(-10, 6);
    expect(y).toBeCloseTo(-2, 6);
  });

  it('空板时「适应内容」置灰(没有内容可收)', () => {
    render(<DesignBoardView />);
    expect(screen.getByTestId('board-fit')).toBeDisabled();

    fireEvent.click(screen.getByTestId('board-add-map'));
    expect(screen.getByTestId('board-fit')).toBeEnabled();
  });
});

describe('<DesignBoardView /> — 素材挂载与实例下钻', () => {
  beforeEach(() => {
    useAssetStore.setState(
      { ...initialAssets, items: [], thumbs: {}, loading: false, error: null },
      true,
    );
  });

  it('双击实体卡下钻详情画布,「返回画板」回主画板;标题行「打开」按钮同效', () => {
    board().addNode('role');
    const { container } = render(<DesignBoardView />);
    const nodeId = board().nodes[0].id;

    fireEvent.doubleClick(container.querySelector(`[data-board-node="${nodeId}"]`)!);
    expect(board().openNodeId).toBe(nodeId);
    expect(screen.getByLabelText('DesignBoardDetail')).toBeInTheDocument();
    expect(screen.getByTestId('detail-title')).toHaveTextContent('角色 1');
    expect(screen.getByTestId('detail-center')).toBeInTheDocument();

    fireEvent.click(screen.getByTestId('detail-back'));
    expect(board().openNodeId).toBeNull();
    expect(screen.getByLabelText('DesignBoard')).toBeInTheDocument();

    fireEvent.click(screen.getByTestId(`board-node-open-${nodeId}`));
    expect(screen.getByLabelText('DesignBoardDetail')).toBeInTheDocument();
  });

  it('实体卡素材行:计数如实;「+ 素材」选择器列出资产、点击挂载、已挂载置灰去重', async () => {
    vi.stubGlobal(
      'fetch',
      mockForgeBackend({
        asset_list: {
          assets: [
            { path: 'meshes/rock.glb', guid: 'g-rock', type: 'mesh', size: 10 },
            { path: 'materials/stone.mat', guid: 'g-stone', type: 'material', size: 5 },
          ],
        },
        asset_build_status: { items: [] },
      }),
    );
    const id = board().addNode('map')!;
    render(<DesignBoardView />);
    expect(screen.getByTestId(`board-node-assets-${id}`)).toHaveTextContent('无素材');

    fireEvent.click(screen.getByTestId(`board-add-asset-${id}`));
    const opt = await screen.findByTestId(`board-asset-opt-${id}-g-rock`);
    fireEvent.click(opt);

    expect(board().nodes[0].assets.map((a) => a.guid)).toEqual(['g-rock']);
    expect(screen.getByTestId(`board-asset-opt-${id}-g-rock`)).toBeDisabled();
    expect(screen.getByTestId(`board-asset-opt-${id}-g-stone`)).toBeEnabled();
    expect(screen.getByTestId(`board-node-assets-${id}`)).toHaveTextContent('1 素材');
  });

  it('实体卡接受 Assets 面板拖放直接挂载(forge/asset-* 三键)', () => {
    const id = board().addNode('role')!;
    const { container } = render(<DesignBoardView />);

    fireEvent(
      container.querySelector(`[data-board-node="${id}"]`)!,
      assetDrop({
        'forge/asset-guid': 'g-7',
        'forge/asset-type': 'mesh',
        'forge/asset-path': 'meshes/crate.glb',
      }),
    );
    expect(board().nodes[0].assets).toHaveLength(1);
    expect(board().nodes[0].assets[0]).toMatchObject({
      guid: 'g-7',
      type: 'mesh',
      path: 'meshes/crate.glb',
    });
  });

  it('详情画布:素材卡渲染并连线中心卡,备注行内编辑,移除后卡与连线消失', () => {
    const id = board().addNode('role')!;
    const aid = board().attachAsset(id, { guid: 'g-1', path: 'meshes/rock.glb', type: 'mesh' })!;
    board().openNode(id);
    const { container } = render(<DesignBoardView />);

    expect(screen.getByTestId(`detail-asset-${aid}`)).toBeInTheDocument();
    expect(container.querySelectorAll('[data-detail-asset-link]')).toHaveLength(1);

    fireEvent.click(screen.getByTestId(`detail-asset-note-${aid}`));
    const input = screen.getByTestId(`detail-asset-note-input-${aid}`);
    fireEvent.change(input, { target: { value: '主体网格' } });
    fireEvent.keyDown(input, { key: 'Enter' });
    expect(board().nodes[0].assets[0].note).toBe('主体网格');

    fireEvent.click(screen.getByTestId(`detail-asset-remove-${aid}`));
    expect(board().nodes[0].assets).toHaveLength(0);
    expect(screen.queryByTestId(`detail-asset-${aid}`)).not.toBeInTheDocument();
    expect(container.querySelectorAll('[data-detail-asset-link]')).toHaveLength(0);
  });

  it('详情画布:素材卡可拖动,松手按世界坐标写回 store(pos 持久化)', () => {
    const id = board().addNode('role')!;
    const aid = board().attachAsset(id, { guid: 'g-1', path: 'meshes/rock.glb', type: 'mesh' }, [0, 0])!;
    board().openNode(id);
    render(<DesignBoardView />);

    const card = screen.getByTestId(`detail-asset-${aid}`);
    fireEvent(card, pointer('pointerdown', { button: 0, clientX: 100, clientY: 100 }));
    fireEvent(card, pointer('pointermove', { clientX: 140, clientY: 60 }));
    fireEvent(card, pointer('pointerup'));
    expect(board().nodes[0].assets[0].pos).toEqual([40, -40]);
  });

  it('详情画布空白处接受拖放:落点世界坐标即素材卡位置(居中落点)', () => {
    const id = board().addNode('role')!;
    board().openNode(id);
    render(<DesignBoardView />);

    fireEvent(
      screen.getByTestId('detail-canvas'),
      assetDrop(
        {
          'forge/asset-guid': 'g-8',
          'forge/asset-type': 'mesh',
          'forge/asset-path': 'meshes/skin.glb',
        },
        { clientX: 200, clientY: 150 },
      ),
    );
    const a = board().nodes[0].assets[0];
    expect(a.guid).toBe('g-8');
    expect(a.pos).toEqual([200 - ASSET_W / 2, 150 - ASSET_H / 2]);
  });

  it('详情画布:交互边渲染为幽灵卡(方向如实),线上标签可读,点幽灵卡跳对端详情', () => {
    const a = board().addNode('role')!;
    const b = board().addNode('map')!;
    const e1 = board().addEdge({ node: a }, { node: b })!;
    board().setEdgeLabel(e1, '踩到机关');
    board().clearPendingEdge(); // 消费掉新边自动编辑态,标签以只读态渲染
    board().openNode(a);
    const { container } = render(<DesignBoardView />);

    expect(container.querySelectorAll('[data-detail-edge]')).toHaveLength(1);
    const ghost = screen.getByTestId(`detail-ghost-${e1}`);
    expect(ghost).toHaveTextContent('地图 1');
    expect(ghost).toHaveTextContent('本实体 → 对方');
    expect(screen.getByTestId(`board-edge-label-${e1}`)).toHaveTextContent('踩到机关');

    fireEvent.click(ghost);
    expect(board().openNodeId).toBe(b);
    expect(screen.getByTestId('detail-title')).toHaveTextContent('地图 1');
    // 对端视角:同一条边如实反向
    expect(screen.getByTestId(`detail-ghost-${e1}`)).toHaveTextContent('对方 → 本实体');
  });
});

describe('EditorView 画板页签', () => {
  it('第三页签「画板」切换后渲染画板,并拉取组件注册表', async () => {
    const fetchMock = mockForgeBackend({
      entity_list: { entities: [] },
      scene_summary: {
        name: 'Demo',
        entityCount: 0,
        playState: 'edit',
        render: { frames: 0, lastTris: 0, lastNonZeroPixels: 0 },
      },
      play_state: { state: 'edit' },
      scene_load: { ok: true },
      component_list_types: COMPONENT_TYPES,
      viewport_get_camera: { target: [0, 0.5, 0], yaw: 35, pitch: 28, dist: 9, fovY: 50 },
      viewport_frame: {
        width: 16,
        height: 16,
        format: 'rgba8',
        pixelsB64: btoa(String.fromCharCode(...new Array(16 * 16 * 4).fill(0))),
        deviceName: 'mock-gpu',
        draws: 1,
        frames: 1,
        nonZeroPixels: 0,
        truncated: false,
      },
      asset_list: { assets: [] },
      asset_build_status: { items: [] },
    });
    vi.stubGlobal('fetch', fetchMock);
    useEditorStore.setState({ componentTypes: [] }); // 逼出真实拉取路径

    render(<EditorView />);
    fireEvent.click(screen.getByRole('button', { name: '画板' }));

    expect(useEditorStore.getState().centerTab).toBe('design');
    expect(screen.getByLabelText('DesignBoard')).toBeInTheDocument();
    expect(screen.getByTestId('board-add-role')).toBeInTheDocument();
    expect(screen.getByTestId('board-kind-new')).toBeInTheDocument();

    // 挂载即懒加载组件注册表(特性下拉的数据源)
    await screen.findByTestId('board-handoff');
    const tools = fetchMock.mock.calls.map(
      (c) => (JSON.parse((c[1] as { body: string }).body) as { tool: string }).tool,
    );
    expect(tools).toContain('mcp__engine-scene__component_list_types');
  });
});

describe('designBoardStore v3 — 特性收起', () => {
  it('toggleCollapse 落盘并回读;老档缺 collapsed 按展开读', () => {
    const id = board().addNode('role')!;
    expect(board().nodes[0].collapsed).toBe(false);

    board().toggleCollapse(id);
    expect(board().nodes[0].collapsed).toBe(true);
    expect(loadBoard().nodes[0].collapsed).toBe(true);

    board().toggleCollapse(id);
    expect(loadBoard().nodes[0].collapsed).toBe(false);

    // collapsed 是 v3 中途加的视图字段:老档没有它照收节点,按展开读
    globalThis.localStorage.setItem(
      BOARD_KEY,
      JSON.stringify({
        version: 3,
        seq: 5,
        nodes: [{ id: 'n1', kindId: 'role', name: '玩家', desc: '', pos: [0, 0], features: [], assets: [] }],
        edges: [],
      }),
    );
    const doc = loadBoard();
    expect(doc.nodes.map((n) => n.id)).toEqual(['n1']);
    expect(doc.nodes[0].collapsed).toBe(false);
  });

  it('收起只换形态:特性与连线原样留在档里', () => {
    const a = board().addNode('role')!;
    const b = board().addNode('map')!;
    const feats = board().nodes.find((n) => n.id === a)!.features;
    board().addEdge({ node: a, feature: feats[0].id }, { node: b });

    board().toggleCollapse(a);
    expect(board().nodes.find((n) => n.id === a)!.features).toEqual(feats);
    expect(board().edges[0].from).toMatchObject({ node: a, feature: feats[0].id });
  });
});

describe('<DesignBoardView /> — 双向端口与特性收起', () => {
  /** jsdom 没有命中测试:拉线落点由 elementFromPoint 桩给出 */
  function withDropTarget(el: Element, run: () => void): void {
    const doc = document as unknown as { elementFromPoint?: (x: number, y: number) => Element | null };
    const orig = doc.elementFromPoint;
    doc.elementFromPoint = () => el;
    try {
      run();
    } finally {
      doc.elementFromPoint = orig;
    }
  }

  /** 从某个 port 按下,拖到 target 上松手 */
  function dragLink(port: Element, target: Element): void {
    withDropTarget(target, () => {
      fireEvent(port, pointer('pointerdown', { button: 0, clientX: 10, clientY: 10 }));
      fireEvent(port, pointer('pointermove', { clientX: 200, clientY: 60 }));
      fireEvent(port, pointer('pointerup', { clientX: 200, clientY: 60 }));
    });
  }

  it('出线端起手:本节点 → 落点', () => {
    const a = board().addNode('role')!;
    const b = board().addNode('map')!;
    const { container } = render(<DesignBoardView />);

    dragLink(
      container.querySelector(`[data-board-port="${a}"]`)!,
      container.querySelector(`[data-board-node="${b}"]`)!,
    );

    expect(board().edges).toHaveLength(1);
    expect(board().edges[0].from).toEqual({ node: a });
    expect(board().edges[0].to).toEqual({ node: b });
  });

  it('入线端起手:同一个特性反向连,落点 → 本特性', () => {
    const a = board().addNode('role')!;
    const b = board().addNode('map')!;
    const f = board().nodes.find((n) => n.id === a)!.features[0];
    const { container } = render(<DesignBoardView />);

    dragLink(
      container.querySelector(`[data-board-port-in="${a}:${f.id}"]`)!,
      container.querySelector(`[data-board-node="${b}"]`)!,
    );

    expect(board().edges).toHaveLength(1);
    expect(board().edges[0].from).toEqual({ node: b });
    expect(board().edges[0].to).toEqual({ node: a, feature: f.id });
  });

  it('收起:子节点与 spine 折进卡片右缘端口列,再点还原', () => {
    const { container } = render(<DesignBoardView />);
    fireEvent.click(screen.getByTestId('board-add-role'));
    const { id, features } = board().nodes[0];
    expect(container.querySelectorAll('[data-board-feature]')).toHaveLength(features.length);

    fireEvent.click(screen.getByTestId(`board-collapse-${id}`));
    expect(board().nodes[0].collapsed).toBe(true);
    expect(container.querySelectorAll('[data-board-spine]')).toHaveLength(0);
    expect(screen.queryByTestId(`board-feature-type-${features[0].id}`)).not.toBeInTheDocument();
    // 端口列仍是一特性一端点(落点命中照旧靠 data-board-feature)
    expect(container.querySelectorAll('[data-board-feature]')).toHaveLength(features.length);
    expect(screen.getByTestId(`board-feature-port-${features[0].id}`)).toBeInTheDocument();
    expect(container.textContent).toContain('已收起');

    fireEvent.click(screen.getByTestId(`board-collapse-${id}`));
    expect(board().nodes[0].collapsed).toBe(false);
    expect(screen.getByTestId(`board-feature-type-${features[0].id}`)).toBeInTheDocument();
    expect(container.querySelectorAll('[data-board-spine]')).toHaveLength(1);
  });

  it('收起后连线不断:出线锚吸卡右缘端口,入线锚吸卡左缘同高', () => {
    const a = board().addNode('role')!;
    const b = board().addNode('map')!;
    const fa = board().nodes.find((n) => n.id === a)!.features[0];
    const fb = board().nodes.find((n) => n.id === b)!.features[0];
    board().addEdge({ node: a, feature: fa.id }, { node: b, feature: fb.id });
    board().toggleCollapse(a);
    board().toggleCollapse(b);

    const { container } = render(<DesignBoardView />);
    const d = container.querySelector('[data-board-edge]')!.getAttribute('d')!;
    const [ax, ay] = board().nodes.find((n) => n.id === a)!.pos;
    const [bx, by] = board().nodes.find((n) => n.id === b)!.pos;
    expect(d).toContain(`M ${ax + NODE_W} ${ay + collapsedPortY(0)} `);
    expect(d.endsWith(` ${bx} ${by + collapsedPortY(0)}`)).toBe(true);
  });

  it('收起态端口仍能起手连线', () => {
    const a = board().addNode('role')!;
    const b = board().addNode('map')!;
    const f = board().nodes.find((n) => n.id === a)!.features[0];
    board().toggleCollapse(a);
    const { container } = render(<DesignBoardView />);

    dragLink(
      screen.getByTestId(`board-feature-port-${f.id}`),
      container.querySelector(`[data-board-node="${b}"]`)!,
    );

    expect(board().edges[0].from).toEqual({ node: a, feature: f.id });
    expect(board().edges[0].to).toEqual({ node: b });
  });
});

describe('designBoardStore v3 — 选中 / 面板 / 复制', () => {
  it('addNode 带落点:卡落在给定世界坐标并选中,脏坐标拒绝', () => {
    const id = board().addNode('role', [-120, 340]);
    expect(board().nodes[0].pos).toEqual([-120, 340]);
    expect(board().selectedNodeId).toBe(id);
    expect(board().addNode('role', [Number.NaN, 0])).toBeNull();
    expect(board().nodes).toHaveLength(1);
  });

  it('duplicateNode:描述 / 特性 / 素材一并复制(id 全新),连线不复制,选中切到副本', () => {
    const a = board().addNode('role')!;
    const b = board().addNode('map')!;
    board().setNodeDesc(a, '会跑会跳');
    board().attachAsset(a, { guid: 'g1', path: 'assets/hero.png', type: 'texture' });
    board().addEdge({ node: a }, { node: b });

    const copy = board().duplicateNode(a)!;
    const src = board().nodes.find((n) => n.id === a)!;
    const dup = board().nodes.find((n) => n.id === copy)!;

    expect(dup.kindId).toBe(src.kindId);
    expect(dup.desc).toBe('会跑会跳');
    expect(dup.pos).toEqual([src.pos[0] + DUPLICATE_OFFSET, src.pos[1] + DUPLICATE_OFFSET]);
    expect(dup.features.map((f) => f.type)).toEqual(src.features.map((f) => f.type));
    expect(dup.features.map((f) => f.id)).not.toEqual(src.features.map((f) => f.id));
    expect(dup.assets.map((x) => x.guid)).toEqual(['g1']);
    expect(dup.assets[0].id).not.toBe(src.assets[0].id);
    expect(board().edges).toHaveLength(1); // 交互是两端的约定,复制会凭空造语义
    expect(board().selectedNodeId).toBe(copy);
    expect(board().duplicateNode('nope')).toBeNull();
  });

  it('副本重名顺延序号', () => {
    const a = board().addNode('role')!;
    const first = board().duplicateNode(a)!;
    const second = board().duplicateNode(a)!;
    expect(board().nodes.find((n) => n.id === first)!.name).toBe('角色 1 副本');
    expect(board().nodes.find((n) => n.id === second)!.name).toBe('角色 1 副本 2');
  });

  it('选中与面板:openPanel 顺带选中;取消选中 / 删特性 / 删实体 / 清空都收敛', () => {
    const a = board().addNode('role')!;
    const f = board().nodes[0].features[0];

    board().openPanel({ kind: 'rename', node: a });
    expect(board().selectedNodeId).toBe(a);
    board().selectNode(null);
    expect(board().panel).toBeNull();

    board().openPanel({ kind: 'feature-note', node: a, feature: f.id });
    board().removeFeature(a, f.id);
    expect(board().panel).toBeNull();

    board().openPanel({ kind: 'asset-picker', node: a });
    board().removeNode(a);
    expect(board().selectedNodeId).toBeNull();
    expect(board().panel).toBeNull();

    const b = board().addNode('map')!;
    board().openPanel({ kind: 'rename', node: b });
    board().clearBoard();
    expect(board().selectedNodeId).toBeNull();
    expect(board().panel).toBeNull();
  });

  it('选中与面板是会话态,不进 localStorage', () => {
    const a = board().addNode('role')!;
    board().openPanel({ kind: 'rename', node: a });
    const raw = globalThis.localStorage.getItem(BOARD_KEY) ?? '';
    expect(raw).not.toContain('selectedNodeId');
    expect(raw).not.toContain('panel');
  });
});

describe('<DesignBoardView /> — 画布 / 实体 / 特性三套右键菜单', () => {
  /** jsdom 下视口 rect 全 0、缩放 1,client 坐标即世界坐标 */
  function rightClick(el: Element, x = 120, y = 80): void {
    fireEvent.contextMenu(el, { clientX: x, clientY: y });
  }

  it('空白右键 = 画布菜单,卡身右键 = 实体菜单,两者不会同时出', () => {
    const id = board().addNode('role')!;
    const { container } = render(<DesignBoardView />);
    const card = container.querySelector(`[data-board-node="${id}"]`)!;

    rightClick(screen.getByTestId('board-canvas'));
    expect(screen.getByTestId('board-menu-canvas')).toHaveTextContent('在此新建角色');
    expect(screen.queryByTestId('board-menu-node')).not.toBeInTheDocument();

    rightClick(card);
    const nodeMenu = screen.getByTestId('board-menu-node');
    expect(nodeMenu).toHaveTextContent('角色 1'); // 标题行认到具体实体
    expect(nodeMenu).toHaveTextContent('打开详情画布');
    expect(screen.queryByTestId('board-menu-canvas')).not.toBeInTheDocument();
    expect(board().selectedNodeId).toBe(id);
  });

  it('特性子节点右键 = 特性菜单(标题为组件名),删除只落在这条特性上', () => {
    const id = board().addNode('role')!;
    const f = featureOf(id, 'RigidBody');
    const { container } = render(<DesignBoardView />);

    rightClick(container.querySelector(`[data-board-feature="${f.id}"]`)!);
    const menu = screen.getByTestId('board-menu-feature');
    expect(menu).toHaveTextContent('RigidBody');
    expect(screen.queryByTestId('board-menu-node')).not.toBeInTheDocument();

    fireEvent.click(screen.getByTestId('board-menu-feature-remove'));
    expect(board().nodes[0].features.map((x) => x.type)).toEqual(['MeshRenderer']);
    expect(screen.queryByTestId('board-menu-feature')).not.toBeInTheDocument();
  });

  it('收起态的特性端口右键也走特性菜单(不误弹实体菜单)', () => {
    const id = board().addNode('role')!;
    const f = featureOf(id, 'MeshRenderer');
    board().toggleCollapse(id);
    render(<DesignBoardView />);

    rightClick(screen.getByTestId(`board-feature-port-${f.id}`));
    expect(screen.getByTestId('board-menu-feature')).toHaveTextContent('MeshRenderer');
    expect(screen.queryByTestId('board-menu-node')).not.toBeInTheDocument();
  });

  it('画布菜单「在此新建」把卡放在右键落点', () => {
    render(<DesignBoardView />);
    rightClick(screen.getByTestId('board-canvas'), 260, 140);
    fireEvent.click(screen.getByTestId('board-menu-canvas-add-map'));
    expect(board().nodes).toHaveLength(1);
    expect(board().nodes[0]).toMatchObject({ kindId: 'map', pos: [260, 140] });
  });

  it('实体菜单:复制 / 改名 / 加特性 / 删除都落到右键那张卡上', () => {
    const id = board().addNode('role')!;
    const { container } = render(<DesignBoardView />);
    const card = () => container.querySelector(`[data-board-node="${id}"]`)!;

    rightClick(card());
    fireEvent.click(screen.getByTestId('board-menu-node-rename'));
    expect(screen.getByTestId(`board-node-name-input-${id}`)).toBeInTheDocument();

    rightClick(card());
    fireEvent.click(screen.getByTestId('board-menu-node-add-feature'));
    expect(screen.getByTestId(`board-feature-menu-${id}`)).toBeInTheDocument();

    rightClick(card());
    fireEvent.click(screen.getByTestId('board-menu-node-duplicate'));
    expect(board().nodes).toHaveLength(2);

    rightClick(card());
    fireEvent.click(screen.getByTestId('board-menu-node-remove'));
    expect(board().nodes.map((n) => n.id)).not.toContain(id);
  });

  it('菜单项标注等效快捷键;Esc 与点外部都能关掉', () => {
    const id = board().addNode('role')!;
    const { container } = render(<DesignBoardView />);

    rightClick(container.querySelector(`[data-board-node="${id}"]`)!);
    expect(screen.getByTestId('board-menu-node-remove')).toHaveTextContent('Del');
    expect(screen.getByTestId('board-menu-node-duplicate')).toHaveTextContent('Ctrl+D');
    fireEvent.keyDown(document, { key: 'Escape' });
    expect(screen.queryByTestId('board-menu-node')).not.toBeInTheDocument();

    rightClick(screen.getByTestId('board-canvas'));
    expect(screen.getByTestId('board-menu-canvas-new-kind')).toHaveTextContent('N');
    fireEvent.mouseDown(document.body);
    expect(screen.queryByTestId('board-menu-canvas')).not.toBeInTheDocument();
  });
});

describe('<DesignBoardView /> — 画布快捷键', () => {
  it('数字键按类型放实体,N 开新类型表单', () => {
    render(<DesignBoardView />);
    const canvas = screen.getByTestId('board-canvas');

    fireEvent.keyDown(canvas, { key: '2' });
    expect(board().nodes.map((n) => n.kindId)).toEqual(['map']);

    fireEvent.keyDown(canvas, { key: 'n' });
    expect(screen.getByTestId('board-kind-form')).toBeInTheDocument();
  });

  it('选中实体后 F2 改名 / A 加特性 / Del 删除;Esc 取消选中后不再作用', () => {
    const id = board().addNode('role')!;
    const { container } = render(<DesignBoardView />);
    const canvas = screen.getByTestId('board-canvas');
    const card = container.querySelector(`[data-board-node="${id}"]`)!;

    fireEvent(card, pointer('pointerdown', { button: 0 }));
    expect(board().selectedNodeId).toBe(id);

    fireEvent.keyDown(canvas, { key: 'F2' });
    expect(screen.getByTestId(`board-node-name-input-${id}`)).toBeInTheDocument();
    fireEvent.keyDown(canvas, { key: 'Escape' });

    fireEvent(card, pointer('pointerdown', { button: 0 }));
    fireEvent.keyDown(canvas, { key: 'a' });
    expect(screen.getByTestId(`board-feature-menu-${id}`)).toBeInTheDocument();

    fireEvent.keyDown(canvas, { key: 'Escape' }); // 取消选中,顺带收面板
    expect(board().selectedNodeId).toBeNull();
    fireEvent.keyDown(canvas, { key: 'Delete' });
    expect(board().nodes).toHaveLength(1); // 没有作用对象就不删

    fireEvent(card, pointer('pointerdown', { button: 0 }));
    fireEvent.keyDown(canvas, { key: 'Delete' });
    expect(board().nodes).toHaveLength(0);
  });

  it('Ctrl+D 复制选中实体', () => {
    const id = board().addNode('role')!;
    const { container } = render(<DesignBoardView />);
    fireEvent(
      container.querySelector(`[data-board-node="${id}"]`)!,
      pointer('pointerdown', { button: 0 }),
    );

    fireEvent.keyDown(screen.getByTestId('board-canvas'), { key: 'd', ctrlKey: true });
    expect(board().nodes).toHaveLength(2);
    expect(board().nodes[1].name).toBe('角色 1 副本');
  });

  it('卡上输入框里打字不当快捷键(键归输入框)', () => {
    const id = board().addNode('role')!;
    render(<DesignBoardView />);

    fireEvent.keyDown(screen.getByTestId(`board-node-desc-${id}`), { key: 'Delete' });
    expect(board().nodes).toHaveLength(1);
  });

  it('点空白清选中,卡框着重色随选中切换', () => {
    const id = board().addNode('role')!;
    const { container } = render(<DesignBoardView />);
    const card = container.querySelector(`[data-board-node="${id}"]`)!;

    fireEvent(card, pointer('pointerdown', { button: 0 }));
    expect(card.getAttribute('data-board-node-selected')).toBe('');

    fireEvent(screen.getByTestId('board-canvas'), pointer('pointerdown', { button: 0 }));
    expect(board().selectedNodeId).toBeNull();
    expect(card.hasAttribute('data-board-node-selected')).toBe(false);
  });
});
