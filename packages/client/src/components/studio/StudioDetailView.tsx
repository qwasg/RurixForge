import { useCallback, useEffect, useRef, useState } from 'react';
import type { KeyboardEvent as ReactKeyboardEvent, MouseEvent as ReactMouseEvent } from 'react';
import {
  ArrowLeft,
  Check,
  ClipboardCopy,
  Crosshair,
  Download,
  ExternalLink,
  History,
  Link2Off,
  Maximize2,
  Pencil,
  PencilLine,
  PersonStanding,
  Scissors,
  Sparkles,
  Trash2,
  X,
} from 'lucide-react';
import { cn } from '@/lib/cn';
import { copyText } from '@/lib/clipboard';
import { isTyping } from '@/lib/keyScope';
import {
  canAccept,
  canReslice,
  currentVersion,
  presetOf,
  useStudioStore,
  type StudioNode,
  type StudioVersion,
} from '@/lib/studioStore';
import { useCanvasViewport, type WorldRect } from '@/lib/useCanvasViewport';
import BoardContextMenu, { type BoardMenuItem } from '../editor/BoardContextMenu';
import { TONE_META } from '../editor/DesignBoardNode';
import CanvasHud from '../editor/CanvasHud';
import StudioComposer from './StudioComposer';
import StudioPreview, { KIND_ICON } from './StudioPreview';

/**
 * 创作节点详情画布(素材创作下钻;openNodeId 指向的节点独占一面无限画布):
 * 中央 = 产物节点卡(当前版本;text 可直接编辑,图像/视频/音频给预览,模型给文件面);
 * 左列 = 上游引用卡(主画布连线的来源节点,点击跳对端详情);右列 = 版本历史卡
 * (每次生成一版,点选设为当前;图像/模型可「入库」gen_accept → Content/,入库路径如实标注)。
 * 底部悬浮生成输入条(StudioComposer):prompt + 模型选择 + 参数 chips + 发送。
 * 右键按落点分四套菜单(与主画布共用 BoardContextMenu 外壳)——空白 / 中心产物卡 /
 * 版本卡 / 上游卡;落在输入框或生成输入条上则让位给原生菜单(要能复制粘贴)。
 * 视口独立持久化(forge:studioDetail:<nodeId>);组件按 nodeId 重挂载。
 */

const toolBtn =
  'flex items-center gap-1 rounded-md border border-edge-strong bg-shell-panel px-2 py-0.5 text-2xs text-fg-2 transition-colors hover:bg-shell-hover disabled:opacity-40 disabled:hover:bg-shell-panel';

// ---------- 几何 ----------

const CENTER_W = 340;
const CENTER_BAR_H = 3;
const CENTER_TITLE_H = 28;
const CENTER_META_H = 22;

function centerPreviewH(kind: string): number {
  return kind === 'text' ? 320 : 240;
}

function centerCardH(kind: string): number {
  return CENTER_BAR_H + CENTER_TITLE_H + centerPreviewH(kind) + CENTER_META_H;
}

const VER_W = 190;
const VER_PREVIEW_H = 104;
const VER_INFO_H = 52;
const VER_H = VER_PREVIEW_H + VER_INFO_H;
const VER_X = CENTER_W + 150;
const VER_STEP = VER_H + 24;

const UP_W = 220;
const UP_H = 76;
const UP_X = -(UP_W + 120);
const UP_STEP = UP_H + 16;

/** 右键落在谁身上决定弹哪一套菜单(x/y = client 坐标) */
type MenuState =
  | { kind: 'canvas'; x: number; y: number }
  | { kind: 'center'; x: number; y: number }
  | { kind: 'version'; x: number; y: number; version: string }
  | { kind: 'upstream'; x: number; y: number; node: string };

/** 水平贝塞尔(与画板同参) */
function bezierH(x1: number, y1: number, x2: number, y2: number): string {
  const dx = Math.max(48, Math.abs(x2 - x1) / 2);
  const s = x2 >= x1 ? 1 : -1;
  return `M ${x1} ${y1} C ${x1 + s * dx} ${y1}, ${x2 - s * dx} ${y2}, ${x2} ${y2}`;
}

function timeLabel(ms: number): string {
  const d = new Date(ms);
  const p = (n: number): string => String(n).padStart(2, '0');
  return `${p(d.getMonth() + 1)}-${p(d.getDate())} ${p(d.getHours())}:${p(d.getMinutes())}`;
}

// ---------- 版本历史卡 ----------

function VersionCard({
  node,
  version,
  index,
  isCurrent,
}: {
  node: StudioNode;
  version: StudioVersion;
  index: number;
  isCurrent: boolean;
}) {
  const setCurrentVersion = useStudioStore((s) => s.setCurrentVersion);
  const removeVersion = useStudioStore((s) => s.removeVersion);
  const acceptVersion = useStudioStore((s) => s.acceptVersion);
  const resliceVersion = useStudioStore((s) => s.resliceVersion);
  const openInSpriteEditor = useStudioStore((s) => s.openInSpriteEditor);
  const [acceptBusy, setAcceptBusy] = useState(false);
  const preset = presetOf(node.preset);
  if (!preset) return null;
  const acceptable = canAccept(preset, version);
  const resliceable = canReslice(preset, version);

  const accept = async () => {
    setAcceptBusy(true);
    try {
      await acceptVersion(node.id, version.id);
    } finally {
      setAcceptBusy(false);
    }
  };

  return (
    <div
      data-studio-detail-card
      data-studio-version={version.id}
      data-testid={`studio-version-${version.id}`}
      className={cn(
        'group absolute cursor-pointer select-none rounded-md border bg-shell-panel shadow-composer transition-colors',
        isCurrent ? 'border-acc' : 'border-edge-strong hover:border-fg-4',
      )}
      style={{ left: VER_X, top: index * VER_STEP, width: VER_W }}
      onClick={() => setCurrentVersion(node.id, version.id)}
    >
      <div className="overflow-hidden rounded-t-md bg-shell-sunk" style={{ height: VER_PREVIEW_H }}>
        <StudioPreview kind={preset.kind} version={version} compact className="h-full w-full" />
      </div>
      <div className="px-1.5 py-1" style={{ height: VER_INFO_H }}>
        <div className="flex items-center gap-1">
          <span className="min-w-0 flex-1 truncate font-mono text-[10px] text-fg-3">
            {version.backendId}
          </span>
          {isCurrent && (
            <span className="flex shrink-0 items-center gap-0.5 text-[10px] text-acc">
              <Check size={9} strokeWidth={2} />
              当前
            </span>
          )}
        </div>
        <div className="mt-0.5 flex items-center gap-1">
          <span className="shrink-0 text-[10px] text-fg-4">{timeLabel(version.createdAt)}</span>
          {version.seed !== undefined && (
            <span className="max-w-[64px] truncate font-mono text-[10px] text-fg-4" title={`seed ${version.seed}`}>
              s{version.seed}
            </span>
          )}
          {version.boxes !== undefined && (
            <span className="shrink-0 text-[10px] text-fg-4" title={`图集 ${version.atlas?.width}×${version.atlas?.height}`}>
              {version.boxes.length}帧
            </span>
          )}
          <span className="flex-1" />
          {resliceable && (
            <button
              type="button"
              data-testid={`studio-reslice-${version.id}`}
              title="按当前截帧参数重新切帧(不重新生成视频)"
              onClick={(e) => {
                e.stopPropagation();
                void resliceVersion(node.id, version.id);
              }}
              className="flex shrink-0 items-center gap-0.5 whitespace-nowrap rounded border border-edge-strong px-1 py-px text-[10px] text-fg-3 transition-colors hover:bg-shell-hover hover:text-fg-2"
            >
              <Scissors size={9} strokeWidth={2} />
              重切
            </button>
          )}
          {version.spritePath !== undefined ? (
            <button
              type="button"
              data-testid={`studio-open-sprite-${version.id}`}
              title={`在精灵编辑器打开 ${version.spritePath}`}
              onClick={(e) => {
                e.stopPropagation();
                openInSpriteEditor(node.id, version.id);
              }}
              className="flex shrink-0 items-center gap-0.5 whitespace-nowrap rounded border border-edge-strong px-1 py-px text-[10px] text-sage transition-colors hover:bg-shell-hover"
            >
              <PersonStanding size={9} strokeWidth={2} />
              精灵编辑器
            </button>
          ) : version.assetPath !== undefined ? (
            <span
              className="max-w-[96px] shrink-0 truncate font-mono text-[10px] text-sage"
              title={version.assetPath}
            >
              已入库
            </span>
          ) : acceptable ? (
            <button
              type="button"
              data-testid={`studio-accept-${version.id}`}
              disabled={acceptBusy}
              title={`入库到 Content/${preset.destFolder ?? 'Textures'}/(gen_accept,写 provenance)`}
              onClick={(e) => {
                e.stopPropagation();
                void accept();
              }}
              className="flex shrink-0 items-center gap-0.5 whitespace-nowrap rounded border border-edge-strong px-1 py-px text-[10px] text-fg-3 transition-colors hover:bg-shell-hover hover:text-fg-2 disabled:opacity-40"
            >
              <Download size={9} strokeWidth={2} />
              {acceptBusy ? '入库中…' : '入库'}
            </button>
          ) : null}
          <button
            type="button"
            data-testid={`studio-version-remove-${version.id}`}
            title="删除该版本记录(不删已入库资产)"
            onClick={(e) => {
              e.stopPropagation();
              removeVersion(node.id, version.id);
            }}
            className="rounded p-px text-fg-4 opacity-0 transition-opacity hover:text-danger group-hover:opacity-100"
          >
            <X size={9} strokeWidth={2} />
          </button>
        </div>
      </div>
    </div>
  );
}

// ---------- 上游引用卡 ----------

function UpstreamCard({
  node,
  label,
  index,
}: {
  node: StudioNode;
  label: string;
  index: number;
}) {
  const openNode = useStudioStore((s) => s.openNode);
  const preset = presetOf(node.preset);
  if (!preset) return null;
  const tone = TONE_META[preset.tone];
  const Icon = KIND_ICON[preset.kind];
  const cur = currentVersion(node);
  const summary =
    cur?.text !== undefined
      ? cur.text.slice(0, 60)
      : cur?.assetPath ?? cur?.fileRef ?? (node.prompt.trim() !== '' ? `意图:${node.prompt.trim().slice(0, 40)}` : '(未生成)');

  return (
    <div
      data-studio-detail-card
      data-studio-upstream={node.id}
      data-testid={`studio-upstream-${node.id}`}
      className="absolute select-none rounded-md border border-edge-strong bg-shell-panel shadow-composer"
      style={{ left: UP_X, top: index * UP_STEP, width: UP_W, height: UP_H }}
    >
      <div className="flex items-center gap-1 px-1.5 pt-1">
        <Icon size={11} strokeWidth={1.8} className={cn('shrink-0', tone.text)} />
        <span className="min-w-0 flex-1 truncate text-2xs font-medium text-fg">{node.name}</span>
        <span className={cn('shrink-0 text-[10px]', tone.text)}>{preset.label}</span>
        <button
          type="button"
          data-testid={`studio-upstream-open-${node.id}`}
          title="打开该上游节点的创作画布"
          onClick={() => openNode(node.id)}
          className="shrink-0 rounded p-0.5 text-fg-4 transition-colors hover:text-fg-2"
        >
          <ExternalLink size={10} strokeWidth={2} />
        </button>
      </div>
      {label !== '' && (
        <p className="truncate px-1.5 pt-0.5 text-[10px] text-acc" title={label}>
          {label}
        </p>
      )}
      <p className="truncate px-1.5 pt-0.5 text-[10px] leading-4 text-fg-4" title={summary}>
        {summary}
      </p>
    </div>
  );
}

// ---------- 详情画布 ----------

export default function StudioDetailView({ nodeId }: { nodeId: string }) {
  const nodes = useStudioStore((s) => s.nodes);
  const edges = useStudioStore((s) => s.edges);
  const closeNode = useStudioStore((s) => s.closeNode);
  const openNode = useStudioStore((s) => s.openNode);
  const renameNode = useStudioStore((s) => s.renameNode);
  const writeText = useStudioStore((s) => s.writeText);
  const editCurrentText = useStudioStore((s) => s.editCurrentText);
  const setParam = useStudioStore((s) => s.setParam);
  const setCurrentVersion = useStudioStore((s) => s.setCurrentVersion);
  const removeVersion = useStudioStore((s) => s.removeVersion);
  const removeEdge = useStudioStore((s) => s.removeEdge);
  const generate = useStudioStore((s) => s.generate);
  const acceptVersion = useStudioStore((s) => s.acceptVersion);
  const resliceVersion = useStudioStore((s) => s.resliceVersion);
  const openInSpriteEditor = useStudioStore((s) => s.openInSpriteEditor);
  const busy = useStudioStore((s) => s.busyIds.includes(nodeId));

  const vp = useCanvasViewport({
    storageKey: `forge:studioDetail:${nodeId}`,
    panExclude: '[data-studio-detail-card],[data-studio-composer]',
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
  const [editingName, setEditingName] = useState(false);
  const [nameDraft, setNameDraft] = useState('');
  const [menu, setMenu] = useState<MenuState | null>(null);
  // 稳定引用:菜单内部拿它挂 document / wheel 监听,每帧换新函数会白重绑
  const closeMenu = useCallback(() => setMenu(null), []);

  const node = nodes.find((n) => n.id === nodeId);
  const preset = node !== undefined ? presetOf(node.preset) : undefined;

  const upstream = edges
    .filter((e) => e.to === nodeId)
    .flatMap((e) => {
      const up = nodes.find((n) => n.id === e.from);
      return up !== undefined ? [{ node: up, label: e.label, edge: e.id }] : [];
    });

  // 首帧把内容(中心卡 + 两列)收进视野;只在挂载后判一次
  const centerH = preset !== undefined ? centerCardH(preset.kind) : 240;
  const rects: WorldRect[] = [
    { x: 0, y: 0, w: CENTER_W, h: centerH },
    ...(node?.versions ?? []).map((_, i) => ({ x: VER_X, y: i * VER_STEP, w: VER_W, h: VER_H })),
    ...upstream.map((_, i) => ({ x: UP_X, y: i * UP_STEP, w: UP_W, h: UP_H })),
  ];
  const rectsRef = useRef(rects);
  rectsRef.current = rects;
  const checkedRef = useRef(false);
  useEffect(() => {
    if (checkedRef.current) return;
    checkedRef.current = true;
    vp.ensureContentVisible(rectsRef.current);
  }, [vp.ensureContentVisible]);

  if (node === undefined || preset === undefined) return null; // store 保证不悬空
  const tone = TONE_META[preset.tone];
  const Icon = KIND_ICON[preset.kind];
  const cur = currentVersion(node);
  const versionsDesc = [...node.versions].reverse();

  const commitName = () => {
    setEditingName(false);
    renameNode(node.id, nameDraft);
  };

  const startRename = () => {
    setNameDraft(node.name);
    setEditingName(true);
  };

  const canGenerate = node.prompt.trim() !== '' && !busy;
  const onGenerate = () => {
    if (canGenerate) void generate(node.id);
  };

  // ---- 右键:按落点分派四套菜单 ----

  const onCanvasContextMenu = (e: ReactMouseEvent<HTMLDivElement>) => {
    const el = e.target as HTMLElement | null;
    // 输入框与生成输入条上让位给原生菜单:那里要的是复制粘贴,不是画布动作
    if (isTyping(e.target) || el?.closest?.('[data-studio-composer]')) return;
    e.preventDefault();
    canvasRef.current?.focus();
    const version = el?.closest?.('[data-studio-version]')?.getAttribute('data-studio-version');
    if (version) {
      setMenu({ kind: 'version', x: e.clientX, y: e.clientY, version });
      return;
    }
    const up = el?.closest?.('[data-studio-upstream]')?.getAttribute('data-studio-upstream');
    if (up) {
      setMenu({ kind: 'upstream', x: e.clientX, y: e.clientY, node: up });
      return;
    }
    if (el?.closest?.('[data-studio-center]')) {
      setMenu({ kind: 'center', x: e.clientX, y: e.clientY });
      return;
    }
    setMenu({ kind: 'canvas', x: e.clientX, y: e.clientY });
  };

  /** 画布快捷键:与右键菜单同一批动作(菜单项右侧标的就是这些键) */
  const onCanvasKeyDown = (e: ReactKeyboardEvent<HTMLDivElement>) => {
    if (isTyping(e.target)) return; // 输入条里正在打字,键归输入框(Ctrl+↵ 由它自己接)

    if (e.ctrlKey || e.metaKey) {
      if (e.key === 'Enter') {
        e.preventDefault();
        onGenerate();
      }
      return;
    }
    if (e.altKey || e.shiftKey) return;

    const key = e.key.length === 1 ? e.key.toLowerCase() : e.key;
    if (key === 'Escape') {
      e.preventDefault();
      closeNode();
    } else if (key === 'f') {
      vp.fitTo(rectsRef.current);
    } else if (key === '0') {
      e.preventDefault();
      vp.resetView();
    } else if (key === 'F2') {
      e.preventDefault();
      startRename();
    }
  };

  // ---- 四套菜单的项 ----

  /** 「生成 / 重新生成」在三套菜单里复用同一份定义 */
  const generateItem = (): BoardMenuItem => ({
    key: 'generate',
    label: busy ? '生成中…' : node.versions.length > 0 ? '按当前描述重新生成' : '按当前描述生成',
    keys: 'Ctrl+↵',
    icon: Sparkles,
    disabled: !canGenerate,
    hint:
      node.prompt.trim() === ''
        ? '下方输入条里还没写创作描述'
        : '每次生成追加一版,上游引用拼进上下文',
    onSelect: onGenerate,
  });

  const viewItems = (): BoardMenuItem[] => [
    {
      key: 'fit',
      label: '适应内容',
      keys: 'F',
      icon: Maximize2,
      onSelect: () => vp.fitTo(rectsRef.current),
    },
    { key: 'reset-zoom', label: '缩放回 100%', keys: '0', icon: Crosshair, onSelect: vp.resetView },
  ];

  const canvasMenuItems = (): BoardMenuItem[] => [
    {
      key: 'back',
      label: '返回素材创作主画布',
      keys: 'Esc',
      icon: ArrowLeft,
      onSelect: closeNode,
    },
    generateItem(),
    { sep: true, key: 'sep-view' },
    ...viewItems(),
  ];

  /** 复制项:text 产物复制正文,媒体产物复制它落在哪(入库路径优先于临时 fileRef) */
  const copyItems = (v: StudioVersion | undefined, keyPrefix: string): BoardMenuItem[] => {
    const isText = preset.kind === 'text';
    const value = isText ? (v?.text ?? '') : (v?.assetPath ?? v?.fileRef ?? '');
    const what = isText ? '产物文本' : '产物路径';
    return [
      {
        key: `${keyPrefix}-copy`,
        label: `复制${what}`,
        icon: ClipboardCopy,
        disabled: value === '',
        onSelect: () => void copyText(value, what),
      },
      {
        key: `${keyPrefix}-copy-prompt`,
        label: '复制提示词',
        icon: ClipboardCopy,
        disabled: v === undefined || v.prompt === '',
        hint: '这一版实际发给模型的描述',
        onSelect: () => void copyText(v?.prompt ?? '', '提示词'),
      },
    ];
  };

  const centerMenuItems = (): BoardMenuItem[] => [
    { key: 'rename', label: '重命名', keys: 'F2', icon: Pencil, onSelect: startRename },
    generateItem(),
    {
      key: 'accept',
      label: preset.kind === 'sprite' ? '入库为精灵' : '入库当前版本',
      icon: Download,
      disabled: !canAccept(preset, cur),
      hint: canAccept(preset, cur)
        ? preset.kind === 'sprite'
          ? `图集入 Content/${preset.destFolder ?? 'Textures'}/,并建 Sprites/*.rxsprite`
          : `入库到 Content/${preset.destFolder ?? 'Textures'}/(gen_accept,写 provenance)`
        : preset.kind === 'sprite'
          ? '先截出图集才能入库(只有 mp4 不算引擎资产)'
          : '只有还没入库的图像 / 3D 模型产物可入库',
      onSelect: () => {
        if (cur) void acceptVersion(node.id, cur.id);
      },
    },
    ...(preset.kind === 'sprite'
      ? [
          {
            key: 'reslice',
            label: '重新截帧',
            icon: Scissors,
            disabled: !canReslice(preset, cur),
            hint: '按当前截帧参数重切(不重新生成视频)',
            onSelect: () => {
              if (cur) void resliceVersion(node.id, cur.id);
            },
          },
          {
            key: 'open-sprite',
            label: '打开精灵编辑器',
            icon: PersonStanding,
            disabled: cur?.spritePath === undefined,
            hint: cur?.spritePath ?? '入库后可在精灵编辑器里调帧、编 clip',
            onSelect: () => {
              if (cur) openInSpriteEditor(node.id, cur.id);
            },
          },
        ]
      : []),
    { sep: true, key: 'sep-copy' },
    ...copyItems(cur, 'center'),
    { sep: true, key: 'sep-life' },
    {
      key: 'remove-version',
      label: '删除当前版本',
      keys: 'Del',
      icon: Trash2,
      danger: true,
      disabled: cur === undefined,
      hint: '只删版本记录,已入库的资产不受影响',
      onSelect: () => {
        if (cur) removeVersion(node.id, cur.id);
      },
    },
  ];

  const versionMenuItems = (v: StudioVersion): BoardMenuItem[] => [
    {
      key: 'current',
      label: '设为当前版本',
      icon: Check,
      disabled: node.currentVersionId === v.id,
      onSelect: () => setCurrentVersion(node.id, v.id),
    },
    {
      key: 'accept',
      label: v.assetPath !== undefined ? '已入库' : preset.kind === 'sprite' ? '入库为精灵' : '入库该版本',
      icon: Download,
      disabled: !canAccept(preset, v),
      hint:
        v.assetPath !== undefined
          ? v.assetPath
          : `入库到 Content/${preset.destFolder ?? 'Textures'}/(gen_accept,写 provenance)`,
      onSelect: () => void acceptVersion(node.id, v.id),
    },
    ...(preset.kind === 'sprite'
      ? [
          {
            key: 'reslice',
            label: '重新截帧',
            icon: Scissors,
            disabled: !canReslice(preset, v),
            hint: '按当前截帧参数重切(不重新生成视频)',
            onSelect: () => void resliceVersion(node.id, v.id),
          },
          {
            key: 'open-sprite',
            label: '打开精灵编辑器',
            icon: PersonStanding,
            disabled: v.spritePath === undefined,
            hint: v.spritePath ?? '入库后可在精灵编辑器里调帧、编 clip',
            onSelect: () => openInSpriteEditor(node.id, v.id),
          },
        ]
      : []),
    { sep: true, key: 'sep-copy' },
    ...copyItems(v, 'version'),
    { sep: true, key: 'sep-life' },
    {
      key: 'remove',
      label: '删除该版本',
      icon: Trash2,
      danger: true,
      hint: '只删版本记录,已入库的资产不受影响',
      onSelect: () => removeVersion(node.id, v.id),
    },
  ];

  const upstreamMenuItems = (u: (typeof upstream)[number]): BoardMenuItem[] => {
    const upCur = currentVersion(u.node);
    const text = upCur?.text;
    return [
      {
        key: 'open',
        label: '打开该上游节点',
        icon: ExternalLink,
        onSelect: () => openNode(u.node.id),
      },
      {
        key: 'copy-text',
        label: '复制上游产物文本',
        icon: ClipboardCopy,
        disabled: text === undefined || text === '',
        onSelect: () => void copyText(text ?? '', '上游产物文本'),
      },
      { sep: true, key: 'sep-life' },
      {
        key: 'detach',
        label: '断开该引用',
        icon: Link2Off,
        danger: true,
        hint: '本节点之后生成时不再拼进该上游产物(上游节点本身保留)',
        onSelect: () => removeEdge(u.edge),
      },
    ];
  };

  const renderMenu = () => {
    if (menu === null) return null;
    if (menu.kind === 'canvas') {
      return (
        <BoardContextMenu
          x={menu.x}
          y={menu.y}
          title={node.name}
          scope="创作画布"
          icon={Icon}
          accentClass={tone.text}
          items={canvasMenuItems()}
          onClose={closeMenu}
          testid="studio-detail-menu-canvas"
        />
      );
    }
    if (menu.kind === 'center') {
      return (
        <BoardContextMenu
          x={menu.x}
          y={menu.y}
          title={node.name}
          scope={preset.label}
          icon={Icon}
          accentClass={tone.text}
          items={centerMenuItems()}
          onClose={closeMenu}
          testid="studio-detail-menu-center"
        />
      );
    }
    if (menu.kind === 'version') {
      const v = node.versions.find((x) => x.id === menu.version);
      if (!v) return null;
      return (
        <BoardContextMenu
          x={menu.x}
          y={menu.y}
          title={v.backendId}
          scope={node.currentVersionId === v.id ? '当前版本' : '历史版本'}
          icon={History}
          items={versionMenuItems(v)}
          onClose={closeMenu}
          testid="studio-detail-menu-version"
        />
      );
    }
    const u = upstream.find((x) => x.node.id === menu.node);
    if (!u) return null;
    return (
      <BoardContextMenu
        x={menu.x}
        y={menu.y}
        title={u.node.name}
        scope="上游引用"
        icon={KIND_ICON[presetOf(u.node.preset)?.kind ?? 'text']}
        accentClass="text-acc"
        items={upstreamMenuItems(u)}
        onClose={closeMenu}
        testid="studio-detail-menu-upstream"
      />
    );
  };

  return (
    <div className="flex min-h-0 flex-1 flex-col bg-shell-sunk" aria-label="StudioDetail">
      {/* 顶栏:返回 + 名称 + 类型 + 计数 */}
      <div className="flex shrink-0 flex-wrap items-center gap-1.5 border-b border-edge bg-shell-panel px-2 py-1">
        <button
          type="button"
          data-testid="studio-detail-back"
          title="返回素材创作主画布"
          onClick={closeNode}
          className={toolBtn}
        >
          <ArrowLeft size={11} strokeWidth={1.8} />
          返回
        </button>
        <span className="h-4 w-px bg-edge-strong" />
        <Icon size={12} strokeWidth={1.8} className={tone.text} />
        {editingName ? (
          <input
            autoFocus
            data-testid="studio-detail-name-input"
            value={nameDraft}
            onChange={(e) => setNameDraft(e.target.value)}
            onBlur={commitName}
            onKeyDown={(e) => {
              if (e.key === 'Enter') commitName();
              if (e.key === 'Escape') setEditingName(false);
            }}
            className="w-[180px] rounded border border-edge-strong bg-shell-panel px-1 py-px text-xs text-fg outline-none focus:border-fg-4"
          />
        ) : (
          <button
            type="button"
            data-testid="studio-detail-name"
            title="点击改名"
            onClick={() => {
              setNameDraft(node.name);
              setEditingName(true);
            }}
            className="max-w-[240px] truncate text-xs font-medium text-fg"
          >
            {node.name}
          </button>
        )}
        <span className={cn('text-[10px]', tone.text)}>{preset.label}</span>
        <span className="text-2xs text-fg-4">
          {node.versions.length} 版本 · {upstream.length} 上游引用
        </span>
        {busy && <span className="text-2xs text-warn">生成中…</span>}
        <span className="flex-1" />
      </div>

      {/* 无限画布:中心产物卡 + 左列上游 + 右列版本;底部悬浮生成输入条 */}
      <div
        ref={setCanvasRef}
        data-testid="studio-detail-canvas"
        // 可聚焦:快捷键挂在画布上,不抢工程内其它面板的键
        tabIndex={0}
        aria-label="创作画布(右键出菜单)"
        className={cn(
          'relative min-h-0 flex-1 touch-none overflow-hidden outline-none',
          vp.panning ? 'cursor-grabbing' : 'cursor-grab',
        )}
        style={vp.gridStyle}
        onPointerDown={(e) => {
          if (!isTyping(e.target)) canvasRef.current?.focus(); // 画布拿到焦点,快捷键才生效
          vp.onPointerDown(e);
        }}
        onContextMenu={onCanvasContextMenu}
        onKeyDown={onCanvasKeyDown}
      >
        <div className="absolute left-0 top-0 h-0 w-0" style={vp.worldStyle}>
          <svg
            width="1"
            height="1"
            aria-hidden
            className="pointer-events-none absolute left-0 top-0 overflow-visible"
          >
            {/* 上游卡 → 中心卡(细线;箭头指向中心) */}
            {upstream.map((u, i) => (
              <path
                key={u.node.id}
                d={bezierH(UP_X + UP_W, i * UP_STEP + UP_H / 2, 0, CENTER_BAR_H + CENTER_TITLE_H / 2)}
                fill="none"
                stroke="var(--accent)"
                strokeWidth="1.2"
                opacity="0.7"
              />
            ))}
            {/* 中心卡 → 版本卡(从属细灰线) */}
            {versionsDesc.map((v, i) => (
              <path
                key={v.id}
                d={bezierH(CENTER_W, CENTER_BAR_H + CENTER_TITLE_H / 2, VER_X, i * VER_STEP + VER_PREVIEW_H / 2)}
                fill="none"
                stroke="var(--line-strong)"
                strokeWidth="1"
              />
            ))}
          </svg>

          {/* 中心产物卡 */}
          <div
            data-studio-detail-card
            data-studio-center
            data-testid="studio-detail-center"
            className="absolute select-none rounded-md border border-edge-strong bg-shell-panel shadow-composer"
            style={{ left: 0, top: 0, width: CENTER_W }}
          >
            <div className={cn('rounded-t-md', tone.bar)} style={{ height: CENTER_BAR_H }} />
            <div className="flex items-center gap-1.5 px-2" style={{ height: CENTER_TITLE_H }}>
              <Icon size={12} strokeWidth={1.8} className={cn('shrink-0', tone.text)} />
              <span className="min-w-0 flex-1 truncate text-xs font-medium text-fg">{node.name}</span>
              <span className={cn('shrink-0 text-[10px]', tone.text)}>{preset.label}</span>
            </div>
            <div
              className="overflow-hidden border-t border-edge bg-shell-sunk"
              style={{ height: centerPreviewH(preset.kind) }}
            >
              {preset.kind === 'text' && cur?.text !== undefined ? (
                <textarea
                  data-testid="studio-detail-text"
                  value={cur.text}
                  onChange={(e) => editCurrentText(node.id, e.target.value)}
                  placeholder="(空文本)"
                  className="block h-full w-full resize-none bg-transparent px-2.5 py-2 text-2xs leading-5 text-fg-2 outline-none placeholder:text-fg-4"
                />
              ) : cur !== undefined ? (
                <StudioPreview kind={preset.kind} version={cur} className="h-full w-full" />
              ) : (
                /* 空态:图标 + 引导(text 类给「自己编写内容」+模板快捷;其余引导输入条) */
                <div className="flex h-full flex-col items-center justify-center gap-2 px-4">
                  <Icon size={40} strokeWidth={1.2} className="text-fg-4" />
                  {preset.kind === 'text' ? (
                    <>
                      <p className="text-2xs text-fg-4">试试:</p>
                      <button
                        type="button"
                        data-testid="studio-write-own"
                        onClick={() => writeText(node.id, '')}
                        className={toolBtn}
                      >
                        <PencilLine size={11} strokeWidth={1.8} />
                        自己编写内容
                      </button>
                      {[
                        { id: 'script', label: '剧本生成' },
                        { id: 'plandoc', label: '策划案生成' },
                        { id: 'promptcraft', label: '提示词生成' },
                      ].map((t) => (
                        <button
                          key={t.id}
                          type="button"
                          data-testid={`studio-empty-template-${t.id}`}
                          title="选中该模板,在下方输入需求后发送"
                          onClick={() => setParam(node.id, 'template', t.id)}
                          className={toolBtn}
                        >
                          {t.label}
                        </button>
                      ))}
                    </>
                  ) : (
                    <p className="max-w-[260px] text-center text-2xs leading-5 text-fg-4">
                      {preset.kind === 'image'
                        ? '在下方输入画面描述,选模型与尺寸后生成'
                        : preset.kind === 'model'
                          ? '在下方输入模型描述后生成(需配置 3D 生成后端)'
                          : preset.kind === 'video'
                            ? '在下方输入视频描述后生成(需配置视频生成后端)'
                            : '输入要朗读的文字或切到音乐生成(需配置音频生成后端)'}
                    </p>
                  )}
                </div>
              )}
            </div>
            <div className="flex items-center gap-1.5 border-t border-edge px-2" style={{ height: CENTER_META_H }}>
              {cur !== undefined ? (
                <>
                  <span className="font-mono text-[10px] text-fg-4">{cur.backendId}</span>
                  {cur.assetPath !== undefined && (
                    <span className="min-w-0 flex-1 truncate font-mono text-[10px] text-sage" title={cur.assetPath}>
                      {cur.assetPath}
                    </span>
                  )}
                  {cur.assetPath === undefined && cur.fileRef !== undefined && (
                    <span className="min-w-0 flex-1 truncate font-mono text-[10px] text-fg-4" title={cur.fileRef}>
                      {cur.fileRef}
                    </span>
                  )}
                </>
              ) : (
                <span className="text-[10px] text-fg-4">未生成</span>
              )}
            </div>
          </div>

          {/* 左列:上游引用卡 */}
          {upstream.map((u, i) => (
            <UpstreamCard key={u.node.id} node={u.node} label={u.label} index={i} />
          ))}

          {/* 右列:版本历史卡(新在上) */}
          {versionsDesc.map((v, i) => (
            <VersionCard
              key={v.id}
              node={node}
              version={v}
              index={i}
              isCurrent={node.currentVersionId === v.id}
            />
          ))}
        </div>

        {/* 底部悬浮生成输入条 */}
        <div
          data-studio-composer
          className="pointer-events-none absolute inset-x-0 bottom-3 z-20 flex justify-center"
        >
          <StudioComposer node={node} />
        </div>

        <CanvasHud vp={vp} prefix="studio-detail" onFit={() => vp.fitTo(rectsRef.current)} />
      </div>

      {renderMenu()}
    </div>
  );
}
