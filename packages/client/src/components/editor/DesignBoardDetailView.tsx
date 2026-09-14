import { useEffect, useRef, useState } from 'react';
import type { DragEvent as ReactDragEvent, PointerEvent as ReactPointerEvent } from 'react';
import { ArrowLeft, Plus, X } from 'lucide-react';
import { cn } from '@/lib/cn';
import {
  useDesignBoardStore,
  type BoardAnchor,
  type BoardAssetRef,
  type BoardNode,
} from '@/lib/designBoardStore';
import type { AssetItem } from '@/lib/assetStore';
import { useCanvasViewport, type WorldRect } from '@/lib/useCanvasViewport';
import { TONE_META, kindIcon } from './DesignBoardNode';
import BoardAssetPicker from './BoardAssetPicker';
import BoardEdgeLabel from './BoardEdgeLabel';
import CanvasHud from './CanvasHud';
import Thumb, { typeIcon } from './assetThumb';

/**
 * 实例详情画布(画板下钻;openNodeId 指向的实体独占一面无限画布):
 * 中心 = 实例主卡(名称/描述可编辑 + 特性清单);左列 = 挂载素材卡(缩略图/备注/
 * 可拖动,坐标持久化在 BoardAssetRef.pos),细线汇入中心卡;右列 = 交互幽灵卡——
 * 主画板上凡涉及本实体的连线,一条边一张对端实体卡,箭头保留方向、线上标签沿用
 * BoardEdgeLabel(可编辑/删除),点幽灵卡跳到对端实体的详情画布。
 * 画布空白处接受 Assets 面板拖放(forge/asset-*),落点即素材卡位置。
 * 视口独立持久化(forge:designBoardDetail:<nodeId>);组件按 nodeId 重挂载。
 */

const toolBtn =
  'flex items-center gap-1 rounded-md border border-edge-strong bg-shell-panel px-2 py-0.5 text-2xs text-fg-2 transition-colors hover:bg-shell-hover disabled:opacity-40 disabled:hover:bg-shell-panel';

// ---------- 几何(渲染高度与连线锚点共用同一套常量) ----------

const CENTER_W = 240;
const CENTER_BAR_H = 3;
const CENTER_TITLE_H = 28;
const CENTER_DESC_H = 64;
const CENTER_SECTION_H = 20;
const CENTER_FEAT_H = 20;
const CENTER_PAD_B = 6;

/** 中心实例卡总高(特性 0 条时保留一行「无特性」占位) */
function centerCardH(node: BoardNode): number {
  const rows = Math.max(node.features.length, 1);
  return (
    CENTER_BAR_H + CENTER_TITLE_H + CENTER_DESC_H + CENTER_SECTION_H + rows * CENTER_FEAT_H + CENTER_PAD_B
  );
}

export const ASSET_W = 190;
const ASSET_THUMB_H = 84;
const ASSET_ROW_H = 22;
/** 素材卡总高 = 缩略图 + 文件行 + 备注行 */
export const ASSET_H = ASSET_THUMB_H + ASSET_ROW_H * 2;

const GHOST_W = 176;
const GHOST_H = 50;
/** 幽灵卡列(中心卡右侧)与纵向步距 */
const GHOST_X = CENTER_W + 180;
const GHOST_STEP = 116;

/** 水平贝塞尔(与主画板同参:横向控制柄 ≥48px;右→左时控制柄镜像) */
function bezierH(x1: number, y1: number, x2: number, y2: number): string {
  const dx = Math.max(48, Math.abs(x2 - x1) / 2);
  const s = x2 >= x1 ? 1 : -1;
  return `M ${x1} ${y1} C ${x1 + s * dx} ${y1}, ${x2 - s * dx} ${y2}, ${x2} ${y2}`;
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
  dx: number;
  dy: number;
  pos: [number, number];
}

// ---------- 素材卡 ----------

interface AssetCardProps {
  nodeId: string;
  asset: BoardAssetRef;
  pos: [number, number];
  dragging: boolean;
  onDragStart: (e: ReactPointerEvent<HTMLDivElement>, asset: BoardAssetRef) => void;
  onDragMove: (e: ReactPointerEvent<HTMLDivElement>, asset: BoardAssetRef) => void;
  onDragEnd: (asset: BoardAssetRef) => void;
}

function AssetCard({ nodeId, asset, pos, dragging, onDragStart, onDragMove, onDragEnd }: AssetCardProps) {
  const detachAsset = useDesignBoardStore((s) => s.detachAsset);
  const setAssetNote = useDesignBoardStore((s) => s.setAssetNote);
  const [editing, setEditing] = useState(false);
  const [draft, setDraft] = useState(asset.note);
  const base = asset.path.split('/').pop() ?? asset.path;

  const commit = () => {
    setEditing(false);
    const v = draft.trim();
    if (v !== asset.note) setAssetNote(nodeId, asset.id, v);
  };

  return (
    <div
      data-detail-card
      data-detail-asset={asset.id}
      data-testid={`detail-asset-${asset.id}`}
      className={cn(
        'group absolute select-none rounded-md border bg-shell-panel shadow-composer',
        dragging ? 'cursor-grabbing border-fg-4' : 'cursor-grab border-edge-strong',
      )}
      style={{ left: pos[0], top: pos[1], width: ASSET_W }}
      onPointerDown={(e) => onDragStart(e, asset)}
      onPointerMove={(e) => onDragMove(e, asset)}
      onPointerUp={() => onDragEnd(asset)}
    >
      <div className="w-full overflow-hidden rounded-t-md" style={{ height: ASSET_THUMB_H }}>
        <Thumb item={{ path: asset.path, guid: asset.guid, type: asset.type, size: 0 } as AssetItem} size={28} />
      </div>
      <div className="flex items-center gap-1 border-t border-edge px-1.5" style={{ height: ASSET_ROW_H }}>
        <span className="shrink-0 text-fg-3">{typeIcon(asset.type, 10)}</span>
        <span title={asset.path} className="min-w-0 flex-1 truncate text-[10px] text-fg">
          {base}
        </span>
        <span className="shrink-0 text-[9px] text-fg-4">{asset.type}</span>
        <button
          type="button"
          data-no-drag
          data-testid={`detail-asset-remove-${asset.id}`}
          title="移除素材(仅解除与该实例的挂载,不删除资产文件)"
          onClick={() => detachAsset(nodeId, asset.id)}
          className="shrink-0 rounded p-px text-fg-4 opacity-0 transition-opacity hover:text-danger group-hover:opacity-100"
        >
          <X size={9} strokeWidth={2} />
        </button>
      </div>
      <div className="flex items-center border-t border-edge px-1.5" style={{ height: ASSET_ROW_H }}>
        {editing ? (
          <input
            autoFocus
            data-no-drag
            data-testid={`detail-asset-note-input-${asset.id}`}
            value={draft}
            onChange={(e) => setDraft(e.target.value)}
            onBlur={commit}
            onKeyDown={(e) => {
              if (e.key === 'Enter') commit();
              if (e.key === 'Escape') {
                setDraft(asset.note);
                setEditing(false);
              }
            }}
            placeholder="用途备注,如:主体贴图"
            className="w-full bg-transparent text-[10px] text-fg-2 outline-none placeholder:text-fg-4"
          />
        ) : (
          <button
            type="button"
            data-no-drag
            data-testid={`detail-asset-note-${asset.id}`}
            title="点击填写该素材在此实例中的用途"
            onClick={() => {
              setDraft(asset.note);
              setEditing(true);
            }}
            className={cn(
              'w-full truncate text-left text-[10px]',
              asset.note !== '' ? 'text-fg-3' : 'italic text-fg-4',
            )}
          >
            {asset.note !== '' ? asset.note : '用途备注…'}
          </button>
        )}
      </div>
    </div>
  );
}

// ---------- 主视图 ----------

export default function DesignBoardDetailView({ nodeId }: { nodeId: string }) {
  const node = useDesignBoardStore((s) => s.nodes.find((n) => n.id === nodeId));
  const nodes = useDesignBoardStore((s) => s.nodes);
  const kinds = useDesignBoardStore((s) => s.kinds);
  const edges = useDesignBoardStore((s) => s.edges);
  const closeNode = useDesignBoardStore((s) => s.closeNode);
  const openNode = useDesignBoardStore((s) => s.openNode);
  const renameNode = useDesignBoardStore((s) => s.renameNode);
  const setNodeDesc = useDesignBoardStore((s) => s.setNodeDesc);
  const attachAsset = useDesignBoardStore((s) => s.attachAsset);
  const moveAssetRef = useDesignBoardStore((s) => s.moveAssetRef);

  const vp = useCanvasViewport({
    storageKey: `forge:designBoardDetail:${nodeId}`,
    panExclude: '[data-detail-card],[data-board-edge-label]',
  });
  const [drag, setDrag] = useState<DragState | null>(null);
  const [pickerOpen, setPickerOpen] = useState(false);
  const [editingName, setEditingName] = useState(false);
  const [nameDraft, setNameDraft] = useState('');

  // ---- 素材卡拖拽(拖拽中本地态,松手一次性提交 store) ----

  const posOfAsset = (a: BoardAssetRef): [number, number] => (drag?.id === a.id ? drag.pos : a.pos);

  const onAssetDragStart = (e: ReactPointerEvent<HTMLDivElement>, a: BoardAssetRef) => {
    if (e.button !== 0) return;
    if ((e.target as HTMLElement).closest('[data-no-drag]')) return;
    capturePointer(e);
    const [x, y] = vp.toWorld(e);
    const pos = posOfAsset(a);
    setDrag({ id: a.id, dx: x - pos[0], dy: y - pos[1], pos });
  };

  const onAssetDragMove = (e: ReactPointerEvent<HTMLDivElement>, a: BoardAssetRef) => {
    if (!drag || drag.id !== a.id) return;
    const [x, y] = vp.toWorld(e);
    setDrag({ ...drag, pos: [x - drag.dx, y - drag.dy] });
  };

  const onAssetDragEnd = (a: BoardAssetRef) => {
    if (!drag || drag.id !== a.id) return;
    moveAssetRef(nodeId, a.id, drag.pos);
    setDrag(null);
  };

  // ---- Assets 面板拖放:落点世界坐标即素材卡位置 ----

  const onCanvasDrop = (e: ReactDragEvent<HTMLDivElement>) => {
    if (!e.dataTransfer.types.includes('forge/asset-guid')) return;
    e.preventDefault();
    const [x, y] = vp.toWorld(e);
    attachAsset(
      nodeId,
      {
        guid: e.dataTransfer.getData('forge/asset-guid'),
        path: e.dataTransfer.getData('forge/asset-path'),
        type: e.dataTransfer.getData('forge/asset-type'),
      },
      [x - ASSET_W / 2, y - ASSET_H / 2],
    );
  };

  // ---- 几何:中心卡锚点 / 交互幽灵卡 / 包围盒 ----

  const kind = kinds.find((k) => k.id === node?.kindId);

  /** 中心卡上的连线锚点纵坐标:锚在特性 → 对应特性行中线,否则标题行中线 */
  const centerAnchorY = (featureId?: string): number => {
    const titleMid = CENTER_BAR_H + CENTER_TITLE_H / 2;
    if (!node || featureId === undefined) return titleMid;
    const i = node.features.findIndex((f) => f.id === featureId);
    if (i < 0) return titleMid;
    return (
      CENTER_BAR_H + CENTER_TITLE_H + CENTER_DESC_H + CENTER_SECTION_H + i * CENTER_FEAT_H + CENTER_FEAT_H / 2
    );
  };

  const nameOf = (anchor: BoardAnchor): string => {
    const n = nodes.find((x) => x.id === anchor.node);
    if (!n) return '?';
    const f = anchor.feature ? n.features.find((x) => x.id === anchor.feature) : undefined;
    return f ? `${n.name}.${f.type}` : n.name;
  };

  const related = edges.filter((e) => e.from.node === nodeId || e.to.node === nodeId);

  interface GhostGeom {
    edge: (typeof related)[number];
    other: BoardNode;
    outgoing: boolean;
    x: number;
    y: number;
    d: string;
    mx: number;
    my: number;
    title: string;
  }

  const ghosts: GhostGeom[] = [];
  related.forEach((e, i) => {
    const outgoing = e.from.node === nodeId;
    const otherId = outgoing ? e.to.node : e.from.node;
    const other = nodes.find((n) => n.id === otherId);
    if (!other) return; // store 级联删除下不应出现,如实跳过
    const gx = GHOST_X;
    const gy = i * GHOST_STEP;
    const anchorY = centerAnchorY(outgoing ? e.from.feature : e.to.feature);
    const gMid = gy + GHOST_H / 2;
    // 出边:中心卡右缘 → 幽灵卡左缘;入边:幽灵卡左缘 → 中心卡右缘(箭头方向如实)
    const d = outgoing ? bezierH(CENTER_W, anchorY, gx, gMid) : bezierH(gx, gMid, CENTER_W, anchorY);
    ghosts.push({
      edge: e,
      other,
      outgoing,
      x: gx,
      y: gy,
      d,
      mx: (CENTER_W + gx) / 2,
      my: (anchorY + gMid) / 2,
      title: `${nameOf(e.from)} → ${nameOf(e.to)}`,
    });
  });

  // 包围盒:中心卡 + 素材卡 + 幽灵卡(适应内容 / 读档找回视野共用)
  const rects: WorldRect[] = node
    ? [
        { x: 0, y: 0, w: CENTER_W, h: centerCardH(node) },
        ...node.assets.map((a) => {
          const [x, y] = posOfAsset(a);
          return { x, y, w: ASSET_W, h: ASSET_H };
        }),
        ...ghosts.map((g) => ({ x: g.x, y: g.y, w: GHOST_W, h: GHOST_H })),
      ]
    : [];
  const rectsRef = useRef(rects);
  rectsRef.current = rects;

  // 读档回来若视口停在空白处,把内容找回来;只在挂载后判一次
  const checkedRef = useRef(false);
  useEffect(() => {
    if (checkedRef.current || rectsRef.current.length === 0) return;
    checkedRef.current = true;
    vp.ensureContentVisible(rectsRef.current);
  }, [vp.ensureContentVisible]);

  if (!node || !kind) return null; // 悬空 nodeId 由 DesignBoardView 侧挡,这里如实不渲染

  const tone = TONE_META[kind.tone];
  const Icon = kindIcon(kind);

  const commitName = () => {
    setEditingName(false);
    renameNode(node.id, nameDraft); // store 内 trim,空名忽略
  };

  return (
    <div className="flex min-h-0 flex-1 flex-col bg-shell-sunk" aria-label="DesignBoardDetail">
      {/* 顶栏:返回 + 实体身份 + 计数 + 添加素材 */}
      <div className="relative flex shrink-0 flex-wrap items-center gap-1.5 border-b border-edge bg-shell-panel px-2 py-1">
        <button type="button" data-testid="detail-back" title="返回主画板" onClick={closeNode} className={toolBtn}>
          <ArrowLeft size={11} strokeWidth={1.8} />
          返回画板
        </button>
        <Icon size={12} strokeWidth={1.8} className={cn('shrink-0', tone.text)} />
        <span data-testid="detail-title" className="max-w-[200px] truncate text-2xs font-medium text-fg">
          {node.name}
        </span>
        <span className={cn('shrink-0 text-[10px]', tone.text)}>{kind.label}</span>
        <span className="text-2xs text-fg-4">
          {node.assets.length} 素材 · {related.length} 交互
        </span>
        <span className="flex-1" />
        <button
          type="button"
          data-testid="detail-add-asset"
          title="挂载素材(图片 / 建模 / 纹理等资产;也可从 Assets 面板拖入画布)"
          onClick={() => setPickerOpen((v) => !v)}
          className={toolBtn}
        >
          <Plus size={11} strokeWidth={1.8} />
          素材
        </button>
        {pickerOpen && (
          <BoardAssetPicker
            nodeId={nodeId}
            onClose={() => setPickerOpen(false)}
            className="absolute right-2 top-full mt-0.5"
          />
        )}
      </div>

      {/* 无限画布:网格容器承接平移/缩放与拖放,世界层放 SVG 连线与 HTML 卡片 */}
      <div
        ref={vp.ref}
        data-testid="detail-canvas"
        className={cn(
          'relative min-h-0 flex-1 touch-none overflow-hidden',
          vp.panning ? 'cursor-grabbing' : 'cursor-grab',
        )}
        style={vp.gridStyle}
        onPointerDown={vp.onPointerDown}
        onDragOver={(e) => {
          if (e.dataTransfer.types.includes('forge/asset-guid')) e.preventDefault();
        }}
        onDrop={onCanvasDrop}
      >
        <div data-testid="detail-world" className="absolute left-0 top-0 h-0 w-0" style={vp.worldStyle}>
          <svg
            width="1"
            height="1"
            aria-hidden
            className="pointer-events-none absolute left-0 top-0 overflow-visible"
          >
            <defs>
              <marker
                id="detail-arrow"
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
            {/* 素材连线:素材卡右缘中点 → 中心卡左缘(细信息线,与交互边区分) */}
            {node.assets.map((a) => {
              const [x, y] = posOfAsset(a);
              return (
                <path
                  key={a.id}
                  data-detail-asset-link={a.id}
                  d={bezierH(x + ASSET_W, y + ASSET_H / 2, 0, CENTER_BAR_H + CENTER_TITLE_H / 2)}
                  fill="none"
                  stroke="var(--info)"
                  strokeWidth="1"
                  opacity="0.7"
                />
              );
            })}
            {/* 交互边:方向如实(出边中心→幽灵,入边幽灵→中心) */}
            {ghosts.map((g) => (
              <path
                key={g.edge.id}
                data-detail-edge={g.edge.id}
                d={g.d}
                fill="none"
                stroke="var(--warn)"
                strokeWidth="1.5"
                markerEnd="url(#detail-arrow)"
              />
            ))}
          </svg>

          {ghosts.map((g) => (
            <BoardEdgeLabel key={g.edge.id} edge={g.edge} x={g.mx} y={g.my} title={g.title} />
          ))}

          {/* 中心实例卡 */}
          <div
            data-detail-card
            data-testid="detail-center"
            className="absolute select-none rounded-md border border-edge-strong bg-shell-panel shadow-composer"
            style={{ left: 0, top: 0, width: CENTER_W }}
          >
            <div className={cn('rounded-t-md', tone.bar)} style={{ height: CENTER_BAR_H }} />
            <div className="flex items-center gap-1 px-2" style={{ height: CENTER_TITLE_H }}>
              <Icon size={12} strokeWidth={1.8} className={cn('shrink-0', tone.text)} />
              {editingName ? (
                <input
                  autoFocus
                  data-no-drag
                  data-testid="detail-name-input"
                  value={nameDraft}
                  onChange={(e) => setNameDraft(e.target.value)}
                  onBlur={commitName}
                  onKeyDown={(e) => {
                    if (e.key === 'Enter') commitName();
                    if (e.key === 'Escape') setEditingName(false);
                  }}
                  className="w-full min-w-0 rounded border border-edge-strong bg-shell-panel px-1 py-px text-2xs text-fg outline-none focus:border-fg-4"
                />
              ) : (
                <span
                  role="button"
                  tabIndex={0}
                  data-no-drag
                  data-testid="detail-name"
                  title="点击改名"
                  onClick={() => {
                    setNameDraft(node.name);
                    setEditingName(true);
                  }}
                  onKeyDown={(e) => {
                    if (e.key === 'Enter') {
                      setNameDraft(node.name);
                      setEditingName(true);
                    }
                  }}
                  className="min-w-0 flex-1 cursor-text truncate text-xs font-medium text-fg"
                >
                  {node.name}
                </span>
              )}
              <span className={cn('shrink-0 text-[10px]', tone.text)}>{kind.label}</span>
            </div>
            <textarea
              data-no-drag
              data-testid="detail-desc"
              value={node.desc}
              onChange={(e) => setNodeDesc(node.id, e.target.value)}
              placeholder={`${kind.label}描述:外观 / 布局 / 行为…`}
              style={{ height: CENTER_DESC_H }}
              className="block w-full resize-none border-t border-edge bg-transparent px-2 py-1 text-2xs text-fg-2 outline-none placeholder:text-fg-4"
            />
            <div
              className="flex items-center border-t border-edge px-2 text-[10px] text-fg-4"
              style={{ height: CENTER_SECTION_H }}
            >
              特性(ECS 组件)· 交互可锚在特性行
            </div>
            {node.features.length === 0 ? (
              <div className="flex items-center px-2 text-[10px] italic text-fg-4" style={{ height: CENTER_FEAT_H }}>
                (无特性)
              </div>
            ) : (
              node.features.map((f) => (
                <div
                  key={f.id}
                  data-testid={`detail-feat-${f.id}`}
                  className="flex items-center gap-1 px-2"
                  style={{ height: CENTER_FEAT_H }}
                >
                  <span
                    aria-hidden
                    className={cn('h-1.5 w-1.5 shrink-0 rounded-full', f.custom ? 'bg-warn' : 'bg-fg-4')}
                  />
                  <span className="shrink-0 font-mono text-[10px] text-fg">{f.type}</span>
                  {f.note !== '' && <span className="min-w-0 truncate text-[10px] text-fg-4">{f.note}</span>}
                </div>
              ))
            )}
            <div style={{ height: CENTER_PAD_B }} />
          </div>

          {/* 素材卡:可拖动,坐标持久化 */}
          {node.assets.map((a) => (
            <AssetCard
              key={a.id}
              nodeId={nodeId}
              asset={a}
              pos={posOfAsset(a)}
              dragging={drag?.id === a.id}
              onDragStart={onAssetDragStart}
              onDragMove={onAssetDragMove}
              onDragEnd={onAssetDragEnd}
            />
          ))}

          {/* 交互幽灵卡:一条边一张对端实体卡,点击跳其详情 */}
          {ghosts.map((g) => {
            const gKind = kinds.find((k) => k.id === g.other.kindId);
            const gTone = gKind ? TONE_META[gKind.tone] : TONE_META.info;
            const GIcon = gKind ? kindIcon(gKind) : Icon;
            return (
              <button
                key={g.edge.id}
                type="button"
                data-detail-card
                data-testid={`detail-ghost-${g.edge.id}`}
                title={`${g.title};点击进入「${g.other.name}」的详情画布`}
                onClick={() => openNode(g.other.id)}
                className="absolute flex flex-col items-start justify-center gap-0.5 rounded-md border border-dashed border-edge-strong bg-shell-panel px-2 text-left shadow-sm transition-colors hover:border-fg-4 hover:bg-shell-hover"
                style={{ left: g.x, top: g.y, width: GHOST_W, height: GHOST_H }}
              >
                <span className="flex w-full items-center gap-1">
                  <GIcon size={11} strokeWidth={1.8} className={cn('shrink-0', gTone.text)} />
                  <span className="min-w-0 flex-1 truncate text-2xs font-medium text-fg">{g.other.name}</span>
                  {gKind && <span className={cn('shrink-0 text-[10px]', gTone.text)}>{gKind.label}</span>}
                </span>
                <span className="text-[9px] text-fg-4">
                  {g.outgoing ? '本实体 → 对方' : '对方 → 本实体'} · 点击进入详情
                </span>
              </button>
            );
          })}
        </div>

        {node.assets.length === 0 && related.length === 0 && (
          <div className="pointer-events-none absolute inset-0 flex items-center justify-center">
            <p className="max-w-[460px] px-4 text-center text-xs leading-5 text-fg-4">
              「{node.name}」还没有素材与交互:点右上「+ 素材」挂载图片 / 建模 / 纹理等资产,
              或从 Assets 面板直接拖进画布;回主画板给它拉交互连线,这里会同步可视化
              (箭头 = 方向,线上标签可编辑)。
              <br />
              画布无限:空白处拖拽或滚轮平移,Ctrl + 滚轮缩放,右下角可回 100% / 适应内容。
            </p>
          </div>
        )}

        <CanvasHud vp={vp} prefix="detail" onFit={rects.length > 0 ? () => vp.fitTo(rects) : undefined} />
      </div>
    </div>
  );
}
