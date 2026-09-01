import { useCallback, useEffect, useRef, useState } from 'react';
import type {
  KeyboardEvent as ReactKeyboardEvent,
  MouseEvent as ReactMouseEvent,
  PointerEvent as ReactPointerEvent,
} from 'react';
import {
  ChevronDown,
  ChevronRight,
  Copy,
  Crosshair,
  Eraser,
  ImagePlus,
  LayoutGrid,
  Maximize2,
  Pencil,
  Plus,
  Puzzle,
  Settings2,
  Sparkles,
  Trash2,
} from 'lucide-react';
import { cn } from '@/lib/cn';
import { useEditorStore } from '@/lib/editorStore';
import {
  useDesignBoardStore,
  type BoardAnchor,
  type BoardEdge,
  type BoardNode,
  type EntityKindDef,
} from '@/lib/designBoardStore';
import { isTyping } from '@/lib/keyScope';
import { useToastStore } from '@/lib/toastStore';
import { useCanvasViewport, type WorldRect } from '@/lib/useCanvasViewport';
import BoardContextMenu, { type BoardMenuItem } from './BoardContextMenu';
import BoardNodeCard, {
  ANCHOR_Y,
  FEATURE_H,
  FEATURE_INDENT,
  FEATURE_W,
  NODE_W,
  TONE_META,
  ENTITY_CARD_H,
  collapsedPortY,
  featureOffset,
  kindIcon,
  nodeTotalH,
  type LinkDir,
} from './DesignBoardNode';
import BoardEdgeLabel from './BoardEdgeLabel';
import CanvasHud from './CanvasHud';
import DesignBoardDetailView from './DesignBoardDetailView';
import DesignBoardKindForm from './DesignBoardKindForm';

/**
 * 画板设计页签(画板波 v3;与 Viewport / NodeGraph 同位第三页签)。
 * 实体节点按类型着色(内置 角色/地图 + 用户自定义类型),每个实体挂若干特性
 * (= ECS 组件)子节点与素材(资产引用);连线 = 交互,描述写在线上,端点可落在
 * 实体或某个特性上。双击实体 / 点「打开」= 下钻进该实体的详情画布(openNodeId,
 * 同为无限画布,集中管理素材并直观查看交互)。
 * 画完流程图点「交给 AI 制作」,画板序列化预填进 Composer(build 模式)。
 * 画布 = 无限画布(useCanvasViewport):视口内一层 translate/scale 的世界层,
 * 节点坐标四向无界(可为负),网格由容器 CSS 背景无限延伸;
 * SVG(从属 spine + 交互边,overflow 可见)在底,HTML 卡片 / 线上标签在上。
 * 右键按落点分三套菜单——空白 = 画布(在此新建各类型 / 新类型 / 视口 / 交给 AI / 清空),
 * 卡身 = 该实体(打开 / 改名 / 加特性 / 挂素材 / 收起 / 复制 / 删除),特性子节点或收起态
 * 端口 = 该特性(改说明 / 给宿主加特性 / 删特性);菜单项右侧标的快捷键与 onCanvasKeyDown
 * 走同一批动作,作用对象 = 选中实体(selectedNodeId,卡框着重色标出)。
 */

const toolBtn =
  'flex items-center gap-1 rounded-md border border-edge-strong bg-shell-panel px-2 py-0.5 text-2xs text-fg-2 transition-colors hover:bg-shell-hover disabled:opacity-40 disabled:hover:bg-shell-panel';

/** 贝塞尔边(与 NodeGraphView 数据边同参:横向控制柄 ≥48px) */
function bezierD(x1: number, y1: number, x2: number, y2: number): string {
  const dx = Math.max(48, Math.abs(x2 - x1) / 2);
  return `M ${x1} ${y1} C ${x1 + dx} ${y1}, ${x2 - dx} ${y2}, ${x2} ${y2}`;
}

/** 指针捕获(jsdom / 旧内核无实现则静默退化为元素内拖拽) */
function capturePointer(e: ReactPointerEvent<Element>): void {
  try {
    (e.currentTarget as Element).setPointerCapture?.(e.pointerId);
  } catch {
    // 捕获不可用不致命:拖拽仅在指针停留元素上时跟手
  }
}

interface DragState {
  id: string;
  /** 指针相对卡左上角偏移(画布坐标系) */
  dx: number;
  dy: number;
  pos: [number, number];
}

interface LinkState {
  from: BoardAnchor;
  /** 起手端:in 表示反向拉线,落点才是边的起点 */
  dir: LinkDir;
  x1: number;
  y1: number;
  x2: number;
  y2: number;
}

// ---------- 主视图 ----------

interface EdgeGeom {
  edge: BoardEdge;
  x1: number;
  y1: number;
  x2: number;
  y2: number;
  /** 标签中点(贝塞尔 t=0.5 恰为端点均值;同对实体多边按序号纵向偏移防重叠) */
  mx: number;
  my: number;
  title: string;
}

/** 实体从属 spine:卡底竖线 + 每个特性的横向短接 */
interface SpineGeom {
  key: string;
  x: number;
  y1: number;
  y2: number;
  stubs: Array<{ key: string; y: number; x2: number }>;
}

/** 右键落在谁身上决定弹哪一套菜单(x/y = client 坐标) */
type MenuState =
  | { kind: 'canvas'; x: number; y: number; world: [number, number] }
  | { kind: 'node'; x: number; y: number; node: string }
  | { kind: 'feature'; x: number; y: number; node: string; feature: string };

/** 事件落点所属的实体 / 特性(收起态端口两个属性都带,故特性优先) */
function hitAnchor(t: EventTarget | null): { node: string | null; feature: string | null } {
  const el = t as HTMLElement | null;
  const featEl = el?.closest?.('[data-board-feature]') ?? null;
  if (featEl) {
    return {
      node: featEl.getAttribute('data-board-feature-node'),
      feature: featEl.getAttribute('data-board-feature'),
    };
  }
  return { node: el?.closest?.('[data-board-node]')?.getAttribute('data-board-node') ?? null, feature: null };
}

export default function DesignBoardView() {
  const kinds = useDesignBoardStore((s) => s.kinds);
  const nodes = useDesignBoardStore((s) => s.nodes);
  const edges = useDesignBoardStore((s) => s.edges);
  const openNodeId = useDesignBoardStore((s) => s.openNodeId);
  const selectedNodeId = useDesignBoardStore((s) => s.selectedNodeId);
  const addNode = useDesignBoardStore((s) => s.addNode);
  const moveNode = useDesignBoardStore((s) => s.moveNode);
  const removeNode = useDesignBoardStore((s) => s.removeNode);
  const duplicateNode = useDesignBoardStore((s) => s.duplicateNode);
  const toggleCollapse = useDesignBoardStore((s) => s.toggleCollapse);
  const removeFeature = useDesignBoardStore((s) => s.removeFeature);
  const openNode = useDesignBoardStore((s) => s.openNode);
  const selectNode = useDesignBoardStore((s) => s.selectNode);
  const openPanel = useDesignBoardStore((s) => s.openPanel);
  const addEdge = useDesignBoardStore((s) => s.addEdge);
  const clearBoard = useDesignBoardStore((s) => s.clearBoard);
  const handoffToAgent = useDesignBoardStore((s) => s.handoffToAgent);
  const componentTypes = useEditorStore((s) => s.componentTypes);
  const loadComponentTypes = useEditorStore((s) => s.loadComponentTypes);

  const vp = useCanvasViewport({
    storageKey: 'forge:designBoardView',
    panExclude: '[data-board-node],[data-board-feature],[data-board-edge-label]',
  });
  // 视口容器另存一份:快捷键要靠它拿键盘焦点(vp.ref 只做视口测量)
  const canvasRef = useRef<HTMLDivElement | null>(null);
  const vpRef = vp.ref;
  const setCanvasRef = useCallback(
    (el: HTMLDivElement | null) => {
      canvasRef.current = el;
      vpRef(el);
    },
    [vpRef],
  );
  const [drag, setDrag] = useState<DragState | null>(null);
  const [link, setLink] = useState<LinkState | null>(null);
  const [armClear, setArmClear] = useState(false);
  const [kindForm, setKindForm] = useState<{ editing?: EntityKindDef } | null>(null);
  const [menu, setMenu] = useState<MenuState | null>(null);
  // 稳定引用:菜单内部拿它挂 document / wheel 监听,每帧换新函数会白重绑
  const closeMenu = useCallback(() => setMenu(null), []);

  // 特性清单来自后端组件注册表(component_list_types);离线时清单为空,UI 如实降级
  useEffect(() => {
    if (componentTypes.length === 0) void loadComponentTypes();
  }, [componentTypes.length, loadComponentTypes]);

  // 「清空」二段确认:2.5s 未复点自动撤防(不弹原生对话框)
  useEffect(() => {
    if (!armClear) return;
    const t = setTimeout(() => setArmClear(false), 2500);
    return () => clearTimeout(t);
  }, [armClear]);

  const byId = new Map(nodes.map((n) => [n.id, n]));
  const kindById = new Map(kinds.map((k) => [k.id, k]));
  const posOf = (n: BoardNode): [number, number] => (drag?.id === n.id ? drag.pos : n.pos);

  /** 锚点 → 出线点(右缘)/ 入线点(左缘);特性锚落在对应特性子节点上 */
  const anchorPoint = (a: BoardAnchor, side: 'out' | 'in'): [number, number] | null => {
    const n = byId.get(a.node);
    if (!n) return null;
    const [px, py] = posOf(n);
    if (a.feature !== undefined) {
      const i = n.features.findIndex((f) => f.id === a.feature);
      if (i < 0) return null;
      // 收起态:特性只剩卡右缘一颗端口,出线吸端口、入线吸卡左缘同高度(仍逐特性分层)
      if (n.collapsed) {
        const y = py + collapsedPortY(i);
        return side === 'out' ? [px + NODE_W, y] : [px, y];
      }
      const [ox, oy] = featureOffset(i);
      const y = py + oy + FEATURE_H / 2;
      return side === 'out' ? [px + ox + FEATURE_W, y] : [px + ox, y];
    }
    return side === 'out' ? [px + NODE_W, py + ANCHOR_Y] : [px, py + ANCHOR_Y];
  };

  // ---- 卡身拖拽(拖拽中只走本地态,松手一次性提交 store,防 localStorage 高频写) ----

  const onDragStart = (e: ReactPointerEvent<HTMLDivElement>, node: BoardNode) => {
    if (e.button !== 0) return;
    if ((e.target as HTMLElement).closest('[data-no-drag]')) return;
    capturePointer(e);
    const [x, y] = vp.toWorld(e);
    const pos = posOf(node);
    setDrag({ id: node.id, dx: x - pos[0], dy: y - pos[1], pos });
  };

  const onDragMove = (e: ReactPointerEvent<HTMLDivElement>, node: BoardNode) => {
    if (!drag || drag.id !== node.id) return;
    const [x, y] = vp.toWorld(e);
    // 无限画布:四向无界,不再钳到第一象限
    setDrag({ ...drag, pos: [x - drag.dx, y - drag.dy] });
  };

  const onDragEnd = (node: BoardNode) => {
    if (!drag || drag.id !== node.id) return;
    moveNode(node.id, drag.pos);
    setDrag(null);
  };

  // ---- 拉线(pointerup 用 elementFromPoint 命中,特性优先于实体) ----

  const onLinkStart = (e: ReactPointerEvent<HTMLElement>, anchor: BoardAnchor, dir: LinkDir) => {
    if (e.button !== 0) return;
    e.stopPropagation(); // 不触发卡身拖拽
    capturePointer(e);
    const src = anchorPoint(anchor, dir);
    const [x, y] = vp.toWorld(e);
    setLink({ from: anchor, dir, x1: src?.[0] ?? x, y1: src?.[1] ?? y, x2: x, y2: y });
  };

  const onLinkMove = (e: ReactPointerEvent<HTMLElement>) => {
    if (!link) return;
    const [x, y] = vp.toWorld(e);
    setLink({ ...link, x2: x, y2: y });
  };

  const onLinkEnd = (e: ReactPointerEvent<HTMLElement>) => {
    if (!link) return;
    const el =
      typeof document.elementFromPoint === 'function'
        ? document.elementFromPoint(e.clientX, e.clientY)
        : null;
    const target = el as HTMLElement | null;
    // 落点先看特性子节点,未命中再退实体卡
    const featEl = target?.closest?.('[data-board-feature]') ?? null;
    const to: BoardAnchor | null = featEl
      ? {
          node: featEl.getAttribute('data-board-feature-node') ?? '',
          feature: featEl.getAttribute('data-board-feature') ?? '',
        }
      : (() => {
          const id = target?.closest?.('[data-board-node]')?.getAttribute('data-board-node');
          return id ? { node: id } : null;
        })();
    // 入线端起手 = 反向连,落点才是边的起点;自环 / 悬空由 store 拒绝
    if (to && link.dir === 'out') addEdge(link.from, to);
    else if (to) addEdge(to, link.from);
    setLink(null);
  };

  // ---- 几何:交互边 + 从属 spine ----

  const pairSeen = new Map<string, number>();
  const geoms: EdgeGeom[] = [];
  for (const e of edges) {
    const a = byId.get(e.from.node);
    const b = byId.get(e.to.node);
    if (!a || !b) continue; // 悬空边如实跳过(store 级联删除下不应出现)
    const p1 = anchorPoint(e.from, 'out');
    const p2 = anchorPoint(e.to, 'in');
    if (!p1 || !p2) continue;
    const key = e.from.node < e.to.node ? `${e.from.node}|${e.to.node}` : `${e.to.node}|${e.from.node}`;
    const k = pairSeen.get(key) ?? 0;
    pairSeen.set(key, k + 1);
    const nameOf = (anchor: BoardAnchor, n: BoardNode): string => {
      const f = anchor.feature ? n.features.find((x) => x.id === anchor.feature) : undefined;
      return f ? `${n.name}.${f.type}` : n.name;
    };
    geoms.push({
      edge: e,
      x1: p1[0],
      y1: p1[1],
      x2: p2[0],
      y2: p2[1],
      mx: (p1[0] + p2[0]) / 2,
      my: (p1[1] + p2[1]) / 2 + k * 26,
      title: `${nameOf(e.from, a)} → ${nameOf(e.to, b)}`,
    });
  }

  const spines: SpineGeom[] = nodes
    .filter((n) => n.features.length > 0 && !n.collapsed) // 收起态没有卡下子节点,自然没有 spine
    .map((n) => {
      const [px, py] = posOf(n);
      const x = px + FEATURE_INDENT / 2;
      const stubs = n.features.map((f, i) => {
        const [ox, oy] = featureOffset(i);
        return { key: f.id, y: py + oy + FEATURE_H / 2, x2: px + ox };
      });
      return {
        key: n.id,
        x,
        y1: py + ENTITY_CARD_H,
        y2: stubs[stubs.length - 1].y,
        stubs,
      };
    });

  // 节点包围盒(世界坐标):适应内容 / 新建节点后拉回视野都吃它
  const rects: WorldRect[] = nodes.map((n) => {
    const [x, y] = posOf(n);
    return { x, y, w: NODE_W, h: nodeTotalH(n) };
  });
  const rectsRef = useRef(rects);
  rectsRef.current = rects;

  // 读档回来若视口停在空白处(上次拖远了),把内容找回来;只在挂载后判一次
  const checkedRef = useRef(false);
  useEffect(() => {
    if (checkedRef.current || rectsRef.current.length === 0) return;
    checkedRef.current = true;
    vp.ensureContentVisible(rectsRef.current);
  }, [nodes.length, vp.ensureContentVisible]);

  /** 落点可能在当前视野外(无限画布没有滚动条兜底),拖回来 */
  const revealNode = (id: string | null) => {
    if (id === null) return;
    const n = useDesignBoardStore.getState().nodes.find((x) => x.id === id);
    if (n) vp.reveal({ x: n.pos[0], y: n.pos[1], w: NODE_W, h: nodeTotalH(n) }, 40);
  };

  /** 新建实体;pos = 右键「在此新建」的画布落点,缺省按网格排 */
  const onAddNode = (kindId: string, pos?: [number, number]) => revealNode(addNode(kindId, pos));

  const onDuplicate = (id: string) => revealNode(duplicateNode(id));

  const onHandoff = () => {
    handoffToAgent();
    useToastStore.getState().push('success', '画板设计已预填到对话输入框,确认后发送');
  };

  /** 清空二段确认:工具栏按钮与右键菜单共用同一次「举手」 */
  const onClear = () => {
    if (armClear) {
      clearBoard();
      setArmClear(false);
    } else {
      setArmClear(true);
    }
  };

  const featureCount = nodes.reduce((sum, n) => sum + n.features.length, 0);
  const selected = selectedNodeId !== null ? byId.get(selectedNodeId) : undefined;

  // ---- 右键:按落点分派三套菜单;左键按下顺带定选中(快捷键的作用对象) ----

  const onCanvasPointerDown = (e: ReactPointerEvent<HTMLDivElement>) => {
    if (!isTyping(e.target)) canvasRef.current?.focus(); // 画布拿到焦点,快捷键才生效
    if (e.button === 0) selectNode(hitAnchor(e.target).node);
    vp.onPointerDown(e);
  };

  const onCanvasContextMenu = (e: ReactMouseEvent<HTMLDivElement>) => {
    e.preventDefault(); // 不弹内核原生菜单
    canvasRef.current?.focus();
    const { node, feature } = hitAnchor(e.target);
    if (node !== null && feature !== null) {
      selectNode(node);
      setMenu({ kind: 'feature', x: e.clientX, y: e.clientY, node, feature });
      return;
    }
    if (node !== null) {
      selectNode(node);
      setMenu({ kind: 'node', x: e.clientX, y: e.clientY, node });
      return;
    }
    selectNode(null);
    setMenu({ kind: 'canvas', x: e.clientX, y: e.clientY, world: vp.toWorld(e) });
  };

  /** 画布快捷键:与右键菜单同一批动作(菜单项右侧标的就是这些键) */
  const onCanvasKeyDown = (e: ReactKeyboardEvent<HTMLDivElement>) => {
    if (isTyping(e.target)) return; // 卡上正在打字,键归输入框

    if (e.ctrlKey || e.metaKey) {
      // Ctrl/⌘ 组合只认这两个,K / J / S 等留给 Shell 的全局键位
      if (e.key === 'Enter' && nodes.length > 0) {
        e.preventDefault();
        onHandoff();
      } else if (e.key.toLowerCase() === 'd' && selected) {
        e.preventDefault();
        onDuplicate(selected.id);
      }
      return;
    }
    if (e.altKey || e.shiftKey) return;

    // 单字符按小写比,功能键(F2 / Delete…)保持原名
    const key = e.key.length === 1 ? e.key.toLowerCase() : e.key;

    // 1..9 = 工具栏上第 n 个类型(按网格落位,不跟鼠标)
    const nth = /^[1-9]$/.test(key) ? Number(key) - 1 : -1;
    if (nth >= 0) {
      if (nth < kinds.length) {
        e.preventDefault();
        onAddNode(kinds[nth].id);
      }
      return;
    }

    if (key === '0') {
      e.preventDefault();
      vp.resetView();
      return;
    }
    if (key === 'f') {
      if (rects.length > 0) vp.fitTo(rects);
      return;
    }
    if (key === 'n') {
      e.preventDefault();
      setKindForm({});
      return;
    }
    if (key === 'Escape') {
      selectNode(null);
      return;
    }

    if (!selected) return; // 以下都作用在选中实体上

    if (key === 'Enter') {
      e.preventDefault();
      openNode(selected.id);
    } else if (key === 'F2') {
      e.preventDefault();
      openPanel({ kind: 'rename', node: selected.id });
    } else if (key === 'a') {
      e.preventDefault();
      openPanel({ kind: 'feature-menu', node: selected.id });
    } else if (key === 's') {
      e.preventDefault();
      openPanel({ kind: 'asset-picker', node: selected.id });
    } else if (key === 'e') {
      if (selected.features.length > 0) toggleCollapse(selected.id);
    } else if (key === 'Delete' || key === 'Backspace') {
      e.preventDefault();
      removeNode(selected.id);
    }
  };

  // ---- 三套菜单的项(与上面快捷键一一对应) ----

  const canvasMenuItems = (world: [number, number]): BoardMenuItem[] => [
    ...kinds.map((k, i) => ({
      key: `add-${k.id}`,
      label: `在此新建${k.label}`,
      keys: i < 9 ? `${i + 1}` : undefined,
      icon: kindIcon(k),
      iconClass: TONE_META[k.tone].text,
      hint: `实体落在右键处(引擎分类 ${k.category}${
        k.defaultFeatures.length > 0 ? `;默认特性 ${k.defaultFeatures.join(' / ')}` : ''
      });按数字键则按网格落位`,
      onSelect: () => onAddNode(k.id, world),
    })),
    { sep: true, key: 'sep-kind' },
    {
      key: 'new-kind',
      label: '新建实体类型…',
      keys: 'N',
      icon: Plus,
      hint: '自定义一个由 ECS 组件构成的实体类型',
      onSelect: () => setKindForm({}),
    },
    { sep: true, key: 'sep-view' },
    {
      key: 'fit',
      label: '适应内容',
      keys: 'F',
      icon: Maximize2,
      disabled: rects.length === 0,
      onSelect: () => vp.fitTo(rects),
    },
    { key: 'reset-zoom', label: '缩放回 100%', keys: '0', icon: Crosshair, onSelect: vp.resetView },
    { sep: true, key: 'sep-board' },
    {
      key: 'handoff',
      label: '交给 AI 制作',
      keys: 'Ctrl+↵',
      icon: Sparkles,
      disabled: nodes.length === 0,
      hint: '画板序列化为提示词,预填对话输入框',
      onSelect: onHandoff,
    },
    {
      key: 'clear',
      label: armClear ? '确认清空?' : '清空画板',
      icon: Eraser,
      danger: true,
      disabled: nodes.length === 0 && edges.length === 0,
      hint: '删除全部实体与连线(自定义类型保留);需点两次确认',
      onSelect: onClear,
    },
  ];

  const nodeMenuItems = (n: BoardNode, kind: EntityKindDef): BoardMenuItem[] => [
    {
      key: 'open',
      label: '打开详情画布',
      keys: '↵',
      icon: Maximize2,
      hint: '集中管理该实体的素材与交互(双击卡身同效)',
      onSelect: () => openNode(n.id),
    },
    { key: 'rename', label: '重命名', keys: 'F2', icon: Pencil, onSelect: () => openPanel({ kind: 'rename', node: n.id }) },
    { sep: true, key: 'sep-add' },
    {
      key: 'add-feature',
      label: '添加特性…',
      keys: 'A',
      icon: Puzzle,
      hint: '挂一个 ECS 组件到该实体',
      onSelect: () => openPanel({ kind: 'feature-menu', node: n.id }),
    },
    {
      key: 'add-asset',
      label: '挂载素材…',
      keys: 'S',
      icon: ImagePlus,
      hint: '图片 / 建模 / 纹理等资产;也可从 Assets 面板拖到卡上',
      onSelect: () => openPanel({ kind: 'asset-picker', node: n.id }),
    },
    {
      key: 'collapse',
      label: n.collapsed ? '展开特性列' : '收起特性列',
      keys: 'E',
      icon: n.collapsed ? ChevronRight : ChevronDown,
      disabled: n.features.length === 0,
      hint: '收起后特性折进卡片右缘端口列,连线不断',
      onSelect: () => toggleCollapse(n.id),
    },
    { sep: true, key: 'sep-life' },
    {
      key: 'duplicate',
      label: `复制${kind.label}`,
      keys: 'Ctrl+D',
      icon: Copy,
      hint: '连特性与素材一起复制;连线不复制',
      onSelect: () => onDuplicate(n.id),
    },
    {
      key: 'remove',
      label: '删除实体',
      keys: 'Del',
      icon: Trash2,
      danger: true,
      hint: '特性与相关连线一并删除',
      onSelect: () => removeNode(n.id),
    },
  ];

  const featureMenuItems = (n: BoardNode, featureId: string): BoardMenuItem[] => [
    {
      key: 'note',
      label: '编辑说明',
      keys: 'F2',
      icon: Pencil,
      hint: '这条特性在本实体里做什么(写给 AI 看)',
      onSelect: () => openPanel({ kind: 'feature-note', node: n.id, feature: featureId }),
    },
    {
      key: 'add-feature',
      label: `给「${n.name}」添加特性…`,
      keys: 'A',
      icon: Puzzle,
      onSelect: () => openPanel({ kind: 'feature-menu', node: n.id }),
    },
    {
      key: 'collapse',
      label: n.collapsed ? '展开特性列' : '收起特性列',
      keys: 'E',
      icon: n.collapsed ? ChevronRight : ChevronDown,
      onSelect: () => toggleCollapse(n.id),
    },
    { sep: true, key: 'sep-life' },
    {
      key: 'remove',
      label: '删除该特性',
      keys: 'Del',
      icon: Trash2,
      danger: true,
      hint: '挂在这条特性上的连线一并删除',
      onSelect: () => removeFeature(n.id, featureId),
    },
  ];

  /** 菜单实体可能在菜单开着时被删(如快捷键),取不到就不渲染 */
  const renderMenu = () => {
    if (menu === null) return null;
    if (menu.kind === 'canvas') {
      return (
        <BoardContextMenu
          x={menu.x}
          y={menu.y}
          title="画布"
          scope="空白处"
          icon={LayoutGrid}
          items={canvasMenuItems(menu.world)}
          onClose={closeMenu}
          testid="board-menu-canvas"
        />
      );
    }
    const n = byId.get(menu.node);
    const kind = n ? kindById.get(n.kindId) : undefined;
    if (!n || !kind) return null;
    if (menu.kind === 'node') {
      return (
        <BoardContextMenu
          x={menu.x}
          y={menu.y}
          title={n.name}
          scope={kind.label}
          icon={kindIcon(kind)}
          accentClass={TONE_META[kind.tone].text}
          items={nodeMenuItems(n, kind)}
          onClose={closeMenu}
          testid="board-menu-node"
        />
      );
    }
    const feature = n.features.find((f) => f.id === menu.feature);
    if (!feature) return null;
    return (
      <BoardContextMenu
        x={menu.x}
        y={menu.y}
        title={feature.type}
        scope={feature.custom ? '自定义特性' : '特性'}
        icon={Puzzle}
        accentClass="text-warn"
        items={featureMenuItems(n, feature.id)}
        onClose={closeMenu}
        testid="board-menu-feature"
      />
    );
  };

  // 下钻:实例详情画布整面接管(素材管理 + 交互逻辑;openNodeId 悬空由 store 保证不出现)。
  // key 按实体切:跳转对端实体时强制重挂载,视口(独立 storageKey)与本地态互不串档。
  if (openNodeId !== null && nodes.some((n) => n.id === openNodeId)) {
    return <DesignBoardDetailView key={openNodeId} nodeId={openNodeId} />;
  }

  return (
    <div className="flex min-h-0 flex-1 flex-col bg-shell-sunk" aria-label="DesignBoard">
      {/* 工具栏:实体类型按钮(内置 + 自定义)+ 新类型 + 计数 + 清空 + 交给 AI */}
      <div className="relative flex shrink-0 flex-wrap items-center gap-1.5 border-b border-edge bg-shell-panel px-2 py-1">
        <span className="shrink-0 text-2xs text-fg-4">画板</span>
        {kinds.map((k) => {
          const Icon = kindIcon(k);
          return (
            <span key={k.id} className="flex items-center">
              <button
                type="button"
                data-testid={`board-add-${k.id}`}
                title={`添加${k.label}实体(引擎分类 ${k.category}${
                  k.defaultFeatures.length > 0 ? `;默认特性 ${k.defaultFeatures.join(' / ')}` : ''
                })`}
                onClick={() => onAddNode(k.id)}
                className={toolBtn}
              >
                <Icon size={11} strokeWidth={1.8} className={TONE_META[k.tone].text} />+ {k.label}
              </button>
              {!k.builtin && (
                <button
                  type="button"
                  data-testid={`board-kind-edit-${k.id}`}
                  title={`编辑实体类型「${k.label}」`}
                  onClick={() => setKindForm({ editing: k })}
                  className="ml-px rounded p-0.5 text-fg-4 transition-colors hover:bg-shell-hover hover:text-fg-2"
                >
                  <Settings2 size={10} strokeWidth={1.8} />
                </button>
              )}
            </span>
          );
        })}
        <button
          type="button"
          data-testid="board-kind-new"
          title="自定义一个由 ECS 组件构成的实体类型"
          onClick={() => setKindForm({})}
          className={cn(toolBtn, 'border-dashed')}
        >
          <Plus size={11} strokeWidth={1.8} />
          新类型
        </button>
        <span className="text-2xs text-fg-4">
          {nodes.length} 实体 · {featureCount} 特性 · {edges.length} 连线
        </span>
        <span className="flex-1" />
        <button
          type="button"
          data-testid="board-clear"
          disabled={nodes.length === 0 && edges.length === 0}
          onClick={onClear}
          className={cn(toolBtn, armClear && 'border-danger/50 text-danger')}
        >
          {armClear ? '确认清空?' : '清空'}
        </button>
        <button
          type="button"
          data-testid="board-handoff"
          disabled={nodes.length === 0}
          title="画板序列化为提示词,预填对话输入框(Agent 模式),确认后发送"
          onClick={onHandoff}
          className="flex items-center gap-1 rounded-md bg-acc px-2.5 py-0.5 text-2xs font-medium text-fg-inv transition-opacity hover:opacity-90 disabled:opacity-40"
        >
          <Sparkles size={11} strokeWidth={1.8} />
          交给 AI 制作
        </button>
        {kindForm && (
          <DesignBoardKindForm editing={kindForm.editing} onClose={() => setKindForm(null)} />
        )}
      </div>

      {/* 无限画布:容器画网格 + 承接平移/缩放/右键/快捷键,世界层放 SVG(spine/交互边)与 HTML 卡片 */}
      <div
        ref={setCanvasRef}
        data-testid="board-canvas"
        // 可聚焦:快捷键挂在画布上,不抢工程内其它面板的键
        tabIndex={0}
        aria-label="画板画布(右键出菜单)"
        className={cn(
          'relative min-h-0 flex-1 touch-none overflow-hidden outline-none',
          vp.panning ? 'cursor-grabbing' : 'cursor-grab',
        )}
        style={vp.gridStyle}
        onPointerDown={onCanvasPointerDown}
        onContextMenu={onCanvasContextMenu}
        onKeyDown={onCanvasKeyDown}
      >
        <div data-testid="board-world" className="absolute left-0 top-0 h-0 w-0" style={vp.worldStyle}>
          {/* 世界层尺寸为 0:靠 overflow 可见把负坐标一侧的边也画出来 */}
          <svg
            width="1"
            height="1"
            aria-hidden
            className="pointer-events-none absolute left-0 top-0 overflow-visible"
          >
            <defs>
              <marker
                id="db-arrow"
                viewBox="0 0 8 8"
                refX="7"
                refY="4"
                markerWidth="7"
                markerHeight="7"
                orient="auto-start-reverse"
              >
                <path d="M 0 0 L 8 4 L 0 8 z" fill="var(--warn)" />
              </marker>
            </defs>
            {/* 从属 spine:实体 → 它的特性(细灰线,与交互边区分) */}
            {spines.map((s) => (
              <g key={s.key} data-board-spine={s.key}>
                <line
                  x1={s.x}
                  y1={s.y1}
                  x2={s.x}
                  y2={s.y2}
                  stroke="var(--line-strong)"
                  strokeWidth="1"
                />
                {s.stubs.map((st) => (
                  <line
                    key={st.key}
                    x1={s.x}
                    y1={st.y}
                    x2={st.x2}
                    y2={st.y}
                    stroke="var(--line-strong)"
                    strokeWidth="1"
                  />
                ))}
              </g>
            ))}
            {geoms.map((g) => (
              <path
                key={g.edge.id}
                data-board-edge={g.edge.id}
                d={bezierD(g.x1, g.y1, g.x2, g.y2)}
                fill="none"
                stroke="var(--warn)"
                strokeWidth="1.5"
                markerEnd="url(#db-arrow)"
              />
            ))}
            {/* 拉线中的临时虚线:按起手端定向,箭头指向这条边真正的终点 */}
            {link && (
              <path
                data-board-templink
                data-board-templink-dir={link.dir}
                d={
                  link.dir === 'out'
                    ? bezierD(link.x1, link.y1, link.x2, link.y2)
                    : bezierD(link.x2, link.y2, link.x1, link.y1)
                }
                fill="none"
                stroke="var(--warn)"
                strokeWidth="1.5"
                strokeDasharray="5 4"
                opacity="0.7"
                markerEnd="url(#db-arrow)"
              />
            )}
          </svg>

          {geoms.map((g) => (
            <BoardEdgeLabel key={g.edge.id} edge={g.edge} x={g.mx} y={g.my} title={g.title} />
          ))}

          {nodes.map((n) => {
            const kind = kindById.get(n.kindId);
            if (!kind) return null; // 类型缺失(读档已挡)不渲染,不伪造默认类型
            return (
              <BoardNodeCard
                key={n.id}
                node={n}
                kind={kind}
                pos={posOf(n)}
                dragging={drag?.id === n.id}
                onDragStart={onDragStart}
                onDragMove={onDragMove}
                onDragEnd={onDragEnd}
                onLinkStart={onLinkStart}
                onLinkMove={onLinkMove}
                onLinkEnd={onLinkEnd}
              />
            );
          })}

        </div>

        {nodes.length === 0 && (
          <div className="pointer-events-none absolute inset-0 flex items-center justify-center">
            <p className="max-w-[460px] px-4 text-center text-xs leading-5 text-fg-4">
              画板为空:点上方类型按钮放置实体(自带默认特性,特性 = ECS 组件,有几个特性就有几个子节点),
              也可「新类型」自定义由 ECS 组成的实体;按住实体或特性右缘圆点拖到另一个目标完成连线,
              拖左缘圆点则反向连(对方 → 本节点),交互描述写在线上;特性行左侧箭头把特性子节点
              收进卡片右缘端口列(连线不断);实体可挂素材(「+ 素材」或从 Assets 面板拖入),双击实体进入
              它的详情画布集中管理素材与交互;画好后点「交给 AI 制作」,由 agent 生成素材与代码。
              <br />
              画布无限:空白处拖拽或滚轮平移,Ctrl + 滚轮缩放,右下角可回 100% / 适应内容。
              <br />
              右键分三处:空白 = 画布菜单(在此新建 / 视口 / 交给 AI),卡身 = 该实体的菜单,
              特性子节点 = 该特性的菜单;菜单项右侧标着等效快捷键(1…9 放实体、N 新类型、
              F 适应、0 回 100%,选中实体后 ↵ 打开、F2 改名、A 加特性、S 挂素材、E 收展、
              Ctrl+D 复制、Del 删除)。
            </p>
          </div>
        )}

        <CanvasHud
          vp={vp}
          prefix="board"
          onFit={rects.length > 0 ? () => vp.fitTo(rects) : undefined}
        />
      </div>

      {renderMenu()}
    </div>
  );
}
