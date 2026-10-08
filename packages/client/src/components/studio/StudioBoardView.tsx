import { useCallback, useEffect, useRef, useState } from 'react';
import type {
  KeyboardEvent as ReactKeyboardEvent,
  MouseEvent as ReactMouseEvent,
  PointerEvent as ReactPointerEvent,
} from 'react';
import {
  ClipboardCopy,
  Copy,
  Crosshair,
  Download,
  Eraser,
  ExternalLink,
  LayoutGrid,
  Link2,
  Maximize2,
  Pencil,
  PersonStanding,
  Scissors,
  Sparkles,
  Trash2,
} from 'lucide-react';
import { cn } from '@/lib/cn';
import { copyText } from '@/lib/clipboard';
import { editorReference, makeAnnotation, useEditorAnnotationStore } from '@/lib/editorReferences';
import { isTyping } from '@/lib/keyScope';
import {
  canAccept,
  canReslice,
  currentVersion,
  presetOf,
  STUDIO_PRESETS,
  useStudioStore,
  type StudioEdge,
  type StudioNode,
  type StudioPresetId,
} from '@/lib/studioStore';
import { useCanvasViewport, type WorldRect } from '@/lib/useCanvasViewport';
import { useWorkspaceStore } from '@/lib/workspaceStore';
import BoardContextMenu, { type BoardMenuItem } from '../editor/BoardContextMenu';
import { TONE_META } from '../editor/DesignBoardNode';
import CanvasHud from '../editor/CanvasHud';
import StudioNodeCard, { STUDIO_ANCHOR_Y, STUDIO_NODE_H, STUDIO_NODE_W } from './StudioNodeCard';
import StudioEdgeLabel from './StudioEdgeLabel';
import StudioDetailView from './StudioDetailView';
import { KIND_ICON } from './StudioPreview';

/**
 * 素材创作主画布(素材创作波;与 Viewport / NodeGraph / 画板同位第四页签)。
 * 无限画布(useCanvasViewport)上放创作节点卡:大纲 / 地图草稿(LLM 文本)、
 * 原画 / 贴图 / UI(图像生成)、3D模型 / 视频 / 音频(媒体生成);
 * 节点间连线 = 引用(上游产物在下游生成时拼进上下文,说明写在线上)。
 * 双击卡片 / 点「打开」下钻进该节点的创作画布(StudioDetailView:产物 + 版本 +
 * 生成输入条),与画板「点击打开子节点」同一分层展现形式。
 * 右键按落点分三套菜单(复用画板的 BoardContextMenu 外壳)——空白 = 画布
 * (在此新建各类型 / 视口 / 清空),卡身与端口 = 该节点(打开 / 改名 / 生成 / 入库 /
 * 复制产物 / 复制节点 / 删除),线上标签 = 该引用(改说明 / 跳两端 / 删连线);
 * 菜单项右侧标的快捷键与 onCanvasKeyDown 走同一批动作,作用对象 = 选中节点
 * (selectedNodeId,卡框着重色标出)。
 */

const toolBtn =
  'flex items-center gap-1 rounded-md border border-edge-strong bg-shell-panel px-2 py-0.5 text-2xs text-fg-2 transition-colors hover:bg-shell-hover disabled:opacity-40 disabled:hover:bg-shell-panel';

/** 贝塞尔边(与画板同参:横向控制柄 ≥48px) */
function bezierD(x1: number, y1: number, x2: number, y2: number): string {
  const dx = Math.max(48, Math.abs(x2 - x1) / 2);
  return `M ${x1} ${y1} C ${x1 + dx} ${y1}, ${x2 - dx} ${y2}, ${x2} ${y2}`;
}

/** 指针捕获(jsdom / 旧内核无实现则静默退化) */
function capturePointer(e: ReactPointerEvent<Element>): void {
  try {
    (e.currentTarget as Element).setPointerCapture?.(e.pointerId);
  } catch {
    // 捕获不可用不致命
  }
}

interface DragState {
  id: string;
  dx: number;
  dy: number;
  pos: [number, number];
}

interface LinkState {
  /** 起手节点与方向(in = 反向:落点才是来源) */
  from: string;
  dir: 'out' | 'in';
  x1: number;
  y1: number;
  x2: number;
  y2: number;
}

/** 右键落在谁身上决定弹哪一套菜单(x/y = client 坐标) */
type MenuState =
  | { kind: 'canvas'; x: number; y: number; world: [number, number] }
  | { kind: 'node'; x: number; y: number; node: string }
  | { kind: 'edge'; x: number; y: number; edge: string };

/** 事件落点所属的节点 / 连线标签(标签浮在卡上方,先判标签;端口是卡的子节点,归卡) */
function hitStudio(t: EventTarget | null): { node: string | null; edge: string | null } {
  const el = t as HTMLElement | null;
  const edge = el?.closest?.('[data-studio-edge-label]')?.getAttribute('data-studio-edge-label') ?? null;
  if (edge !== null) return { node: null, edge };
  return {
    node: el?.closest?.('[data-studio-node]')?.getAttribute('data-studio-node') ?? null,
    edge: null,
  };
}

export default function StudioBoardView() {
  const nodes = useStudioStore((s) => s.nodes);
  const edges = useStudioStore((s) => s.edges);
  const openNodeId = useStudioStore((s) => s.openNodeId);
  const selectedNodeId = useStudioStore((s) => s.selectedNodeId);
  const selectedNodeIds = useStudioStore((s) => s.selectedNodeIds);
  const busyIds = useStudioStore((s) => s.busyIds);
  const addNode = useStudioStore((s) => s.addNode);
  const moveNode = useStudioStore((s) => s.moveNode);
  const removeNode = useStudioStore((s) => s.removeNode);
  const duplicateNode = useStudioStore((s) => s.duplicateNode);
  const selectNode = useStudioStore((s) => s.selectNode);
  const openNode = useStudioStore((s) => s.openNode);
  const editNodeName = useStudioStore((s) => s.editNodeName);
  const addEdge = useStudioStore((s) => s.addEdge);
  const removeEdge = useStudioStore((s) => s.removeEdge);
  const editEdgeLabel = useStudioStore((s) => s.editEdgeLabel);
  const generate = useStudioStore((s) => s.generate);
  const acceptVersion = useStudioStore((s) => s.acceptVersion);
  const resliceVersion = useStudioStore((s) => s.resliceVersion);
  const openInSpriteEditor = useStudioStore((s) => s.openInSpriteEditor);
  const clearBoard = useStudioStore((s) => s.clearBoard);
  const bindWorkspace = useStudioStore((s) => s.bindWorkspace);
  const activeWorkspaceId = useWorkspaceStore((s) => s.activeWorkspaceId);

  useEffect(() => {
    bindWorkspace(activeWorkspaceId);
  }, [activeWorkspaceId, bindWorkspace]);

  const vp = useCanvasViewport({
    storageKey: 'forge:studioBoardView',
    panExclude: '[data-studio-node],[data-studio-edge-label]',
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
  const [menu, setMenu] = useState<MenuState | null>(null);
  // 稳定引用:菜单内部拿它挂 document / wheel 监听,每帧换新函数会白重绑
  const closeMenu = useCallback(() => setMenu(null), []);

  // 「清空」二段确认:2.5s 未复点自动撤防
  useEffect(() => {
    if (!armClear) return;
    const t = setTimeout(() => setArmClear(false), 2500);
    return () => clearTimeout(t);
  }, [armClear]);

  const byId = new Map(nodes.map((n) => [n.id, n]));
  const posOf = (n: StudioNode): [number, number] => (drag?.id === n.id ? drag.pos : n.pos);

  /** 节点 → 出线点(右缘)/入线点(左缘) */
  const anchorPoint = (id: string, side: 'out' | 'in'): [number, number] | null => {
    const n = byId.get(id);
    if (!n) return null;
    const [px, py] = posOf(n);
    return side === 'out' ? [px + STUDIO_NODE_W, py + STUDIO_ANCHOR_Y] : [px, py + STUDIO_ANCHOR_Y];
  };

  // ---- 卡身拖拽(拖拽中本地态,松手一次性提交 store) ----

  const onDragStart = (e: ReactPointerEvent<HTMLDivElement>, node: StudioNode) => {
    if (e.button !== 0) return;
    if ((e.target as HTMLElement).closest('[data-no-drag]')) return;
    capturePointer(e);
    const [x, y] = vp.toWorld(e);
    const pos = posOf(node);
    setDrag({ id: node.id, dx: x - pos[0], dy: y - pos[1], pos });
  };

  const onDragMove = (e: ReactPointerEvent<HTMLDivElement>, node: StudioNode) => {
    if (!drag || drag.id !== node.id) return;
    const [x, y] = vp.toWorld(e);
    setDrag({ ...drag, pos: [x - drag.dx, y - drag.dy] });
  };

  const onDragEnd = (node: StudioNode) => {
    if (!drag || drag.id !== node.id) return;
    moveNode(node.id, drag.pos);
    setDrag(null);
  };

  // ---- 拉线(pointerup 用 elementFromPoint 命中目标卡) ----

  const onLinkStart = (e: ReactPointerEvent<HTMLElement>, nodeId: string, dir: 'out' | 'in') => {
    if (e.button !== 0) return;
    e.stopPropagation();
    capturePointer(e);
    const src = anchorPoint(nodeId, dir === 'out' ? 'out' : 'in');
    const [x, y] = vp.toWorld(e);
    setLink({ from: nodeId, dir, x1: src?.[0] ?? x, y1: src?.[1] ?? y, x2: x, y2: y });
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
    const target = (el as HTMLElement | null)?.closest?.('[data-studio-node]');
    const id = target?.getAttribute('data-studio-node');
    if (id) {
      // in 起手 = 反向连:落点是来源
      if (link.dir === 'out') addEdge(link.from, id);
      else addEdge(id, link.from);
    }
    setLink(null);
  };

  // ---- 边几何 ----

  const pairSeen = new Map<string, number>();
  const geoms = edges.flatMap((e) => {
    const a = byId.get(e.from);
    const b = byId.get(e.to);
    if (!a || !b) return [];
    const p1 = anchorPoint(e.from, 'out');
    const p2 = anchorPoint(e.to, 'in');
    if (!p1 || !p2) return [];
    const key = e.from < e.to ? `${e.from}|${e.to}` : `${e.to}|${e.from}`;
    const k = pairSeen.get(key) ?? 0;
    pairSeen.set(key, k + 1);
    return [
      {
        edge: e,
        x1: p1[0],
        y1: p1[1],
        x2: p2[0],
        y2: p2[1],
        mx: (p1[0] + p2[0]) / 2,
        my: (p1[1] + p2[1]) / 2 + k * 26,
        title: `${a.name} → ${b.name}`,
      },
    ];
  });

  const rects: WorldRect[] = nodes.map((n) => {
    const [x, y] = posOf(n);
    return { x, y, w: STUDIO_NODE_W, h: STUDIO_NODE_H };
  });
  const rectsRef = useRef(rects);
  rectsRef.current = rects;

  // 读档回来若视口停在空白处,把内容找回来;只在挂载后判一次
  const checkedRef = useRef(false);
  useEffect(() => {
    if (checkedRef.current || rectsRef.current.length === 0) return;
    checkedRef.current = true;
    vp.ensureContentVisible(rectsRef.current);
  }, [nodes.length, vp.ensureContentVisible]);

  /** 新建 / 复制出来的节点可能落在视野外,拖回来 */
  const reveal = (id: string | null): void => {
    if (id === null) return;
    const n = useStudioStore.getState().nodes.find((x) => x.id === id);
    if (n) vp.reveal({ x: n.pos[0], y: n.pos[1], w: STUDIO_NODE_W, h: STUDIO_NODE_H }, 40);
  };

  const onAddNode = (presetId: StudioPresetId, pos?: [number, number]) => {
    reveal(addNode(presetId, pos));
  };

  const onClear = () => {
    if (armClear) {
      clearBoard();
      setArmClear(false);
    } else {
      setArmClear(true);
    }
  };

  const selected = selectedNodeId !== null ? byId.get(selectedNodeId) : undefined;

  /** 该节点此刻能否直接跑生成(描述为空 / 正在生成中都不行) */
  const canGenerate = (n: StudioNode): boolean =>
    n.prompt.trim() !== '' && !busyIds.includes(n.id);

  // ---- 右键:按落点分派三套菜单;左键按下顺带定选中(快捷键的作用对象) ----

  const onCanvasPointerDown = (e: ReactPointerEvent<HTMLDivElement>) => {
    if (!isTyping(e.target)) canvasRef.current?.focus(); // 画布拿到焦点,快捷键才生效
    if (e.button === 0) selectNode(hitStudio(e.target).node, e.ctrlKey || e.metaKey);
    vp.onPointerDown(e);
  };

  const onCanvasContextMenu = (e: ReactMouseEvent<HTMLDivElement>) => {
    // 落在输入框(改名 / 引用说明)上让位给原生菜单:那里要的是复制粘贴,不是画布动作
    if (isTyping(e.target)) return;
    e.preventDefault(); // 不弹内核原生菜单
    canvasRef.current?.focus();
    const { node, edge } = hitStudio(e.target);
    if (edge !== null) {
      setMenu({ kind: 'edge', x: e.clientX, y: e.clientY, edge });
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
      // Ctrl/⌘ 组合只认这一个,K / J / S 等留给 Shell 的全局键位
      if (e.key.toLowerCase() === 'd' && selected) {
        e.preventDefault();
        reveal(duplicateNode(selected.id));
      }
      return;
    }
    if (e.altKey || e.shiftKey) return;

    // 单字符按小写比,功能键(F2 / Delete…)保持原名
    const key = e.key.length === 1 ? e.key.toLowerCase() : e.key;

    // 1..8 = 工具栏上第 n 类创作节点(按网格落位,不跟鼠标)
    const nth = /^[1-9]$/.test(key) ? Number(key) - 1 : -1;
    if (nth >= 0) {
      if (nth < STUDIO_PRESETS.length) {
        e.preventDefault();
        onAddNode(STUDIO_PRESETS[nth].id);
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
    if (key === 'Escape') {
      selectNode(null);
      return;
    }

    if (!selected) return; // 以下都作用在选中节点上

    if (key === 'Enter') {
      e.preventDefault();
      openNode(selected.id);
    } else if (key === 'F2') {
      e.preventDefault();
      editNodeName(selected.id);
    } else if (key === 'g') {
      e.preventDefault();
      if (canGenerate(selected)) void generate(selected.id);
    } else if (key === 'Delete' || key === 'Backspace') {
      e.preventDefault();
      removeNode(selected.id);
    }
  };

  // ---- 三套菜单的项(与上面快捷键一一对应) ----

  const canvasMenuItems = (world: [number, number]): BoardMenuItem[] => [
    { key: 'annotate', label: '添加创作画板批注', icon: Sparkles, onSelect: () => useEditorAnnotationStore.getState().add([makeAnnotation(editorReference('studio', { resourceId: 'main' }), '素材创作画板')]) },
    ...STUDIO_PRESETS.map((p, i) => ({
      key: `add-${p.id}`,
      label: `在此新建${p.label}`,
      keys: `${i + 1}`,
      icon: KIND_ICON[p.kind],
      iconClass: TONE_META[p.tone].text,
      hint: `${p.hint}(节点落在右键处;按数字键则按网格落位)`,
      onSelect: () => onAddNode(p.id, world),
    })),
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
      key: 'clear',
      label: armClear ? '确认清空?' : '清空画板',
      icon: Eraser,
      danger: true,
      disabled: nodes.length === 0 && edges.length === 0,
      hint: '删除全部创作节点与连线(已入库的资产不受影响);需点两次确认',
      onSelect: onClear,
    },
  ];

  const nodeMenuItems = (n: StudioNode): BoardMenuItem[] => {
    const preset = presetOf(n.preset);
    const cur = currentVersion(n);
    const busy = busyIds.includes(n.id);
    // text 产物复制正文,媒体产物复制它落在哪(入库路径优先于临时 fileRef)
    const copyable =
      preset?.kind === 'text'
        ? { what: '产物文本', value: cur?.text ?? '' }
        : { what: '产物路径', value: cur?.assetPath ?? cur?.fileRef ?? '' };
    return [
      { key: 'annotate', label: '添加批注到对话', icon: Sparkles, onSelect: () => useEditorAnnotationStore.getState().add([makeAnnotation(editorReference('studio', { resourceId: 'main', selection: { nodeIds: [n.id], versionId: cur?.id } }), n.name)]) },
      {
        key: 'open',
        label: '打开创作画布',
        keys: '↵',
        icon: Maximize2,
        hint: '生成 / 版本管理 / 上游引用都在里面(双击卡身同效)',
        onSelect: () => openNode(n.id),
      },
      { key: 'rename', label: '重命名', keys: 'F2', icon: Pencil, onSelect: () => editNodeName(n.id) },
      { sep: true, key: 'sep-gen' },
      {
        key: 'generate',
        label: busy ? '生成中…' : cur !== undefined ? '按当前描述重新生成' : '按当前描述生成',
        keys: 'G',
        icon: Sparkles,
        disabled: !canGenerate(n),
        hint:
          n.prompt.trim() === ''
            ? '该节点还没写创作描述,先进创作画布写一句'
            : '不下钻直接跑一次生成,上游引用照常拼进上下文',
        onSelect: () => void generate(n.id),
      },
      {
        key: 'accept',
        label: preset?.kind === 'sprite' ? '入库为精灵' : '入库当前版本',
        icon: Download,
        disabled: !canAccept(preset, cur),
        hint: canAccept(preset, cur)
          ? preset?.kind === 'sprite'
            ? `图集入 Content/${preset.destFolder ?? 'Textures'}/,并建 Sprites/*.rxsprite`
            : `入库到 Content/${preset?.destFolder ?? 'Textures'}/(gen_accept,写 provenance)`
          : preset?.kind === 'sprite'
            ? '先截出图集才能入库(只有 mp4 不算引擎资产)'
            : '只有还没入库的图像 / 3D 模型产物可入库',
        onSelect: () => {
          if (cur) void acceptVersion(n.id, cur.id);
        },
      },
      ...(preset?.kind === 'sprite'
        ? [
            {
              key: 'reslice',
              label: '重新截帧',
              icon: Scissors,
              disabled: !canReslice(preset, cur),
              hint: '按当前截帧参数重切(不重新生成视频)',
              onSelect: () => {
                if (cur) void resliceVersion(n.id, cur.id);
              },
            },
            {
              key: 'open-sprite',
              label: '打开精灵编辑器',
              icon: PersonStanding,
              disabled: cur?.spritePath === undefined,
              hint: cur?.spritePath ?? '入库后可在精灵编辑器里调帧、编 clip',
              onSelect: () => {
                if (cur) openInSpriteEditor(n.id, cur.id);
              },
            },
          ]
        : []),
      {
        key: 'copy',
        label: `复制${copyable.what}`,
        icon: ClipboardCopy,
        disabled: copyable.value === '',
        onSelect: () => void copyText(copyable.value, copyable.what),
      },
      { sep: true, key: 'sep-life' },
      {
        key: 'duplicate',
        label: `复制${preset?.label ?? '节点'}`,
        keys: 'Ctrl+D',
        icon: Copy,
        hint: '连描述与参数一起复制成一张新草稿卡;版本历史与连线不复制',
        onSelect: () => reveal(duplicateNode(n.id)),
      },
      {
        key: 'remove',
        label: '删除节点',
        keys: 'Del',
        icon: Trash2,
        danger: true,
        hint: '相关连线一并删除(已入库的资产不受影响)',
        onSelect: () => removeNode(n.id),
      },
    ];
  };

  const edgeMenuItems = (e: StudioEdge): BoardMenuItem[] => {
    const from = byId.get(e.from);
    const to = byId.get(e.to);
    return [
      {
        key: 'edit',
        label: '编辑引用说明',
        keys: 'F2',
        icon: Pencil,
        hint: '这条引用在下游生成时怎么用(写给模型看)',
        onSelect: () => editEdgeLabel(e.id),
      },
      { sep: true, key: 'sep-nav' },
      {
        key: 'open-from',
        label: `打开来源「${from?.name ?? '?'}」`,
        icon: ExternalLink,
        disabled: from === undefined,
        onSelect: () => {
          if (from) openNode(from.id);
        },
      },
      {
        key: 'open-to',
        label: `打开目标「${to?.name ?? '?'}」`,
        icon: ExternalLink,
        disabled: to === undefined,
        onSelect: () => {
          if (to) openNode(to.id);
        },
      },
      { sep: true, key: 'sep-life' },
      {
        key: 'remove',
        label: '删除连线',
        keys: 'Del',
        icon: Trash2,
        danger: true,
        hint: '下游生成时不再拼进该上游产物',
        onSelect: () => removeEdge(e.id),
      },
    ];
  };

  /** 菜单对象可能在菜单开着时被删(如快捷键),取不到就不渲染 */
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
          testid="studio-menu-canvas"
        />
      );
    }
    if (menu.kind === 'edge') {
      const edge = edges.find((e) => e.id === menu.edge);
      if (!edge) return null;
      return (
        <BoardContextMenu
          x={menu.x}
          y={menu.y}
          title={edge.label !== '' ? edge.label : '引用连线'}
          scope="连线"
          icon={Link2}
          accentClass="text-acc"
          items={edgeMenuItems(edge)}
          onClose={closeMenu}
          testid="studio-menu-edge"
        />
      );
    }
    const n = byId.get(menu.node);
    const preset = n ? presetOf(n.preset) : undefined;
    if (!n || !preset) return null;
    return (
      <BoardContextMenu
        x={menu.x}
        y={menu.y}
        title={n.name}
        scope={preset.label}
        icon={KIND_ICON[preset.kind]}
        accentClass={TONE_META[preset.tone].text}
        items={nodeMenuItems(n)}
        onClose={closeMenu}
        testid="studio-menu-node"
      />
    );
  };

  // 下钻:创作节点详情画布整面接管;key 按节点切,视口与本地态互不串档
  if (openNodeId !== null && nodes.some((n) => n.id === openNodeId)) {
    return <StudioDetailView key={openNodeId} nodeId={openNodeId} />;
  }

  return (
    <div className="flex min-h-0 flex-1 flex-col bg-shell-sunk" aria-label="StudioBoard">
      {/* 工具栏:八类创作节点 + 计数 + 清空 */}
      <div className="relative flex shrink-0 flex-wrap items-center gap-1.5 border-b border-edge bg-shell-panel px-2 py-1">
        <span className="shrink-0 text-2xs text-fg-4">素材创作</span>
        {STUDIO_PRESETS.map((p) => {
          const Icon = KIND_ICON[p.kind];
          return (
            <button
              key={p.id}
              type="button"
              data-testid={`studio-add-${p.id}`}
              title={`添加「${p.label}」创作节点(${
                p.kind === 'text'
                  ? 'LLM 文本生成'
                  : p.kind === 'image'
                    ? '图像生成'
                    : p.kind === 'model'
                      ? 'Blender 制作地图／角色，或远程生成模型'
                      : p.kind === 'video'
                        ? '文生视频 / 图生视频'
                        : '音频/音乐生成'
              })`}
              onClick={() => onAddNode(p.id)}
              className={toolBtn}
            >
              <Icon size={11} strokeWidth={1.8} className={TONE_META[p.tone].text} />+ {p.label}
            </button>
          );
        })}
        <span className="text-2xs text-fg-4">
          {nodes.length} 节点 · {edges.length} 连线
        </span>
        <span className="flex-1" />
        <button
          type="button"
          data-testid="studio-clear"
          disabled={nodes.length === 0 && edges.length === 0}
          onClick={onClear}
          className={cn(toolBtn, armClear && 'border-danger/50 text-danger')}
        >
          {armClear ? '确认清空?' : '清空'}
        </button>
      </div>

      {/* 无限画布:容器画网格 + 承接平移/缩放/右键/快捷键,世界层放 SVG 边与 HTML 卡片 */}
      <div
        ref={setCanvasRef}
        data-testid="studio-canvas"
        // 可聚焦:快捷键挂在画布上,不抢工程内其它面板的键
        tabIndex={0}
        aria-label="素材创作画布(右键出菜单)"
        className={cn(
          'relative min-h-0 flex-1 touch-none overflow-hidden outline-none',
          vp.panning ? 'cursor-grabbing' : 'cursor-grab',
        )}
        style={vp.gridStyle}
        onPointerDown={onCanvasPointerDown}
        onContextMenu={onCanvasContextMenu}
        onKeyDown={onCanvasKeyDown}
      >
        <div data-testid="studio-world" className="absolute left-0 top-0 h-0 w-0" style={vp.worldStyle}>
          <svg
            width="1"
            height="1"
            aria-hidden
            className="pointer-events-none absolute left-0 top-0 overflow-visible"
          >
            <defs>
              <marker
                id="studio-arrow"
                viewBox="0 0 8 8"
                refX="7"
                refY="4"
                markerWidth="7"
                markerHeight="7"
                orient="auto-start-reverse"
              >
                <path d="M 0 0 L 8 4 L 0 8 z" fill="var(--accent)" />
              </marker>
            </defs>
            {geoms.map((g) => (
              <path
                key={g.edge.id}
                data-studio-edge={g.edge.id}
                d={bezierD(g.x1, g.y1, g.x2, g.y2)}
                fill="none"
                stroke="var(--accent)"
                strokeWidth="1.5"
                markerEnd="url(#studio-arrow)"
              />
            ))}
            {link && (
              <path
                data-studio-templink
                d={
                  link.dir === 'out'
                    ? bezierD(link.x1, link.y1, link.x2, link.y2)
                    : bezierD(link.x2, link.y2, link.x1, link.y1)
                }
                fill="none"
                stroke="var(--accent)"
                strokeWidth="1.5"
                strokeDasharray="5 4"
                opacity="0.7"
              />
            )}
          </svg>

          {geoms.map((g) => (
            <StudioEdgeLabel key={g.edge.id} edge={g.edge} x={g.mx} y={g.my} title={g.title} />
          ))}

          {nodes.map((n) => (
            <StudioNodeCard
              key={n.id}
              node={n}
              pos={posOf(n)}
              dragging={drag?.id === n.id}
              selected={selectedNodeId === n.id || selectedNodeIds.includes(n.id)}
              onDragStart={onDragStart}
              onDragMove={onDragMove}
              onDragEnd={onDragEnd}
              onLinkStart={onLinkStart}
              onLinkMove={onLinkMove}
              onLinkEnd={onLinkEnd}
            />
          ))}
        </div>

        {nodes.length === 0 && (
          <div className="pointer-events-none absolute inset-0 flex items-center justify-center">
            <p className="max-w-[480px] px-4 text-center text-xs leading-5 text-fg-4">
              素材创作板为空:点上方按钮放置创作节点——大纲 / 地图草稿(LLM 文本),
              原画 / 贴图 / UI(图像生成),3D模型(Blender 制作或远程生成),视频 / 音频(需配置生成后端)。
              双击节点进入它的创作画布:输入描述选模型生成,版本管理,图像与模型可一键入库。
              节点间拖 port 连线 = 上游产物在下游生成时作为参考上下文(如 大纲 → 原画 → 3D模型)。
              <br />
              画布无限:空白处拖拽或滚轮平移,Ctrl + 滚轮缩放,右下角可回 100% / 适应内容;
              空白处右键可就地新建节点,数字键 1..8 同效。
            </p>
          </div>
        )}

        <CanvasHud
          vp={vp}
          prefix="studio"
          onFit={rects.length > 0 ? () => vp.fitTo(rects) : undefined}
        />
      </div>

      {renderMenu()}
    </div>
  );
}
