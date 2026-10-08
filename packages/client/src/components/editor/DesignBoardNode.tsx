import { useEffect, useRef, useState } from 'react';
import type { DragEvent as ReactDragEvent, PointerEvent as ReactPointerEvent } from 'react';
import { Boxes, ChevronDown, ChevronRight, Maximize2, Plus, X } from 'lucide-react';
import { cn } from '@/lib/cn';
import { CATEGORY_META } from '@/lib/entityCategory';
import { useEditorStore } from '@/lib/editorStore';
import {
  useDesignBoardStore,
  type BoardAnchor,
  type BoardFeature,
  type BoardNode,
  type EntityKindDef,
  type KindTone,
} from '@/lib/designBoardStore';
import type { AssetItem } from '@/lib/assetStore';
import BoardAssetPicker from './BoardAssetPicker';
import Thumb from './assetThumb';

/**
 * 画板实体卡 + 特性子节点(画板波 v3:实体可挂载素材并下钻详情画布)。
 * 一个实体 = 一张卡(类型色条 + 名称 + 描述 + 素材行 + 「+ 特性」),它拥有的每个
 * ECS 组件展开成卡下方一个独立的特性子节点——有几个特性就有几个子节点,每个子节点
 * 左右各一个 port:右 = 出线(本节点 → 对方),左 = 入线(对方 → 本节点),两向都能起手。
 * 特性列可「收起」:子节点折进实体卡右缘的一列端口(与特性同序),连线不断只换锚点,
 * 出线仍吸在右缘端口上,入线吸到卡左缘同一高度。
 * 素材行 = 微缩略图条 + 计数 + 「+ 素材」(选择器挂载);双击卡身 / 标题行「打开」
 * 按钮 = 下钻进该实体的详情画布;卡身接受 Assets 面板拖放(forge/asset-*)直接挂载。
 * 改名 / 特性下拉 / 素材选择器 / 特性说明这四个开合态存在 store(panel),卡上按钮、
 * 右键菜单、键盘快捷键因此是同一个入口的三种点法;选中态(selectedNodeId)= 快捷键
 * 的作用对象,卡框着重色标出。
 * 几何常量在此集中导出:画布按同一套数字算连线锚点与包围盒,渲染侧用显式高度类
 * 锁死实际高度,保证锚点与像素对齐。
 */

// ---------- 几何(渲染高度与画布锚点共用同一套常量) ----------

export const NODE_W = 176;
const BAR_H = 3;
const TITLE_H = 24;
const DESC_H = 52;
const ASSET_ROW_H = 22;
const ADD_ROW_H = 24;
/** 实体卡总高 = 色条 + 标题 + 描述 + 素材行 + 加特性行 */
export const ENTITY_CARD_H = BAR_H + TITLE_H + DESC_H + ASSET_ROW_H + ADD_ROW_H;
/** 实体连线锚点纵坐标(标题行中线) */
export const ANCHOR_Y = BAR_H + TITLE_H / 2;

export const FEATURE_H = 22;
export const FEATURE_GAP = 4;
export const FEATURE_INDENT = 20;
/** 卡底与第一个特性子节点之间的留白 */
export const FEATURE_TOP_GAP = 6;
export const FEATURE_W = NODE_W - FEATURE_INDENT;

/** 第 i 个特性子节点相对实体卡左上角的偏移 */
export function featureOffset(i: number): [number, number] {
  return [FEATURE_INDENT, ENTITY_CARD_H + FEATURE_TOP_GAP + i * (FEATURE_H + FEATURE_GAP)];
}

/** 收起态端口列:自标题行下方起,一个特性一颗端口,纵向步距 */
const COLLAPSED_PORT_TOP = BAR_H + TITLE_H + 6;
export const COLLAPSED_PORT_STEP = 12;

/** 第 i 个收起端口的圆心纵坐标(相对实体卡左上角) */
export function collapsedPortY(i: number): number {
  return COLLAPSED_PORT_TOP + i * COLLAPSED_PORT_STEP + COLLAPSED_PORT_STEP / 2;
}

/** 实体连同特性列的总高(画布包围盒用) */
export function nodeTotalH(node: BoardNode): number {
  if (node.features.length === 0) return ENTITY_CARD_H;
  // 收起态:特性不占卡下空间,但端口列特性多时会漏出卡底,包围盒取两者较高
  if (node.collapsed) {
    return Math.max(ENTITY_CARD_H, collapsedPortY(node.features.length - 1) + COLLAPSED_PORT_STEP / 2);
  }
  return ENTITY_CARD_H + FEATURE_TOP_GAP + node.features.length * (FEATURE_H + FEATURE_GAP);
}

// ---------- 类型配色 ----------

export const TONE_META: Record<KindTone, { bar: string; text: string; dot: string }> = {
  info: { bar: 'bg-info', text: 'text-info', dot: 'bg-info' },
  sage: { bar: 'bg-sage', text: 'text-sage', dot: 'bg-sage' },
  warn: { bar: 'bg-warn', text: 'text-warn', dot: 'bg-warn' },
  acc: { bar: 'bg-acc', text: 'text-acc', dot: 'bg-acc' },
  danger: { bar: 'bg-danger', text: 'text-danger', dot: 'bg-danger' },
};

/** 类型图标:内置沿用三分类图标,自定义统一 Boxes */
export function kindIcon(kind: EntityKindDef) {
  if (kind.id === 'role' || kind.id === 'map') return CATEGORY_META[kind.id].icon;
  return Boxes;
}

// ---------- 特性下拉(已注册组件 + 自定义入口) ----------

function FeatureMenu({ node, onClose }: { node: BoardNode; onClose: () => void }) {
  const componentTypes = useEditorStore((s) => s.componentTypes);
  const addFeature = useDesignBoardStore((s) => s.addFeature);
  const [customDraft, setCustomDraft] = useState('');
  const ref = useRef<HTMLDivElement>(null);

  // 点外部关闭(捕获期,避免与卡身拖拽/其它 port 抢事件)
  useEffect(() => {
    const onDown = (ev: MouseEvent) => {
      if (!ref.current?.contains(ev.target as Node)) onClose();
    };
    document.addEventListener('mousedown', onDown, true);
    return () => document.removeEventListener('mousedown', onDown, true);
  }, [onClose]);

  const owned = new Set(node.features.map((f) => f.type));

  const commitCustom = () => {
    const t = customDraft.trim();
    if (t === '') return;
    addFeature(node.id, t, true);
    setCustomDraft('');
    onClose();
  };

  return (
    <div
      ref={ref}
      data-no-drag
      data-testid={`board-feature-menu-${node.id}`}
      className="absolute left-1.5 top-full z-20 mt-0.5 w-[168px] rounded-md border border-edge-strong bg-shell-float p-1 shadow-pop"
    >
      <p className="px-1 pb-0.5 text-[10px] text-fg-4">引擎组件(已注册)</p>
      {componentTypes.length === 0 ? (
        <p className="px-1 pb-1 text-[10px] text-fg-4">组件清单未加载(后端离线)</p>
      ) : (
        <div className="max-h-[136px] overflow-y-auto">
          {componentTypes.map((t) => (
            <button
              key={t.name}
              type="button"
              data-testid={`board-feature-opt-${node.id}-${t.name}`}
              disabled={owned.has(t.name)}
              title={t.fields.map((f) => `${f.name}: ${f.type}`).join('\n') || t.name}
              onClick={() => {
                addFeature(node.id, t.name, false);
                onClose();
              }}
              className="block w-full truncate rounded px-1 py-0.5 text-left text-2xs text-fg-2 transition-colors hover:bg-shell-hover disabled:opacity-40 disabled:hover:bg-transparent"
            >
              {t.name}
              {owned.has(t.name) && <span className="ml-1 text-fg-4">已有</span>}
            </button>
          ))}
        </div>
      )}
      <div className="mt-1 border-t border-edge pt-1">
        <p className="px-1 pb-0.5 text-[10px] text-fg-4">自定义特性(仅作 AI 描述,引擎未注册)</p>
        <input
          data-testid={`board-feature-custom-${node.id}`}
          value={customDraft}
          onChange={(e) => setCustomDraft(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === 'Enter') commitCustom();
            if (e.key === 'Escape') onClose();
          }}
          placeholder="起个名字,回车添加"
          className="w-full rounded border border-edge-strong bg-shell-panel px-1 py-px text-2xs text-fg outline-none placeholder:text-fg-4 focus:border-fg-4"
        />
      </div>
    </div>
  );
}

// ---------- 连线端口 ----------

/** 起手端:out = 从本锚点连出去,in = 反向,落点才是起点(对方 → 本锚点) */
export type LinkDir = 'out' | 'in';

export interface LinkHandlers {
  onLinkStart: (e: ReactPointerEvent<HTMLElement>, anchor: BoardAnchor, dir: LinkDir) => void;
  onLinkMove: (e: ReactPointerEvent<HTMLElement>) => void;
  onLinkEnd: (e: ReactPointerEvent<HTMLElement>) => void;
}

const OUT_PORT_TITLE = '按住拖到目标完成连线(本节点 → 对方);交互描述写在线上';
const IN_PORT_TITLE = '按住拖到来源完成反向连线(对方 → 本节点)';

// ---------- 特性子节点 ----------

interface FeatureNodeProps extends LinkHandlers {
  node: BoardNode;
  feature: BoardFeature;
  /** 相对画布的绝对位置 */
  pos: [number, number];
}

function FeatureNode({ node, feature, pos, onLinkStart, onLinkMove, onLinkEnd }: FeatureNodeProps) {
  const setFeatureNote = useDesignBoardStore((s) => s.setFeatureNote);
  const removeFeature = useDesignBoardStore((s) => s.removeFeature);
  // 说明编辑态在 store:卡上点击、右键菜单、F2 快捷键都从同一处开合
  const editing = useDesignBoardStore(
    (s) => s.panel?.kind === 'feature-note' && s.panel.node === node.id && s.panel.feature === feature.id,
  );
  const openPanel = useDesignBoardStore((s) => s.openPanel);
  const closePanel = useDesignBoardStore((s) => s.closePanel);
  const [draft, setDraft] = useState(feature.note);
  useEffect(() => setDraft(feature.note), [feature.note]);

  const commit = () => {
    closePanel();
    const v = draft.trim();
    if (v !== feature.note) setFeatureNote(node.id, feature.id, v);
  };

  return (
    <div
      data-board-feature={feature.id}
      data-board-feature-node={node.id}
      title={feature.custom ? `${feature.type}(自定义,引擎未注册)` : `${feature.type}(引擎组件)`}
      className={cn(
        'group/feat absolute flex items-center gap-1 rounded border bg-shell-panel px-1 shadow-sm',
        feature.custom ? 'border-dashed border-warn/60' : 'border-edge-strong',
      )}
      style={{ left: pos[0], top: pos[1], width: FEATURE_W, height: FEATURE_H }}
    >
      <span
        aria-hidden
        className={cn(
          'h-1.5 w-1.5 shrink-0 rounded-full',
          feature.custom ? 'bg-warn' : 'bg-fg-4',
        )}
      />
      <span
        data-testid={`board-feature-type-${feature.id}`}
        className="shrink-0 truncate font-mono text-[10px] text-fg"
      >
        {feature.type}
      </span>
      <AnnotationHandle reference={editorReference('blueprint', { resourceId: 'main', selection: { nodeIds: [node.id], featureIds: [feature.id] } })} label={`${node.name}.${feature.type}`} />
      {editing ? (
        <input
          autoFocus
          data-no-drag
          data-testid={`board-feature-note-input-${feature.id}`}
          value={draft}
          onChange={(e) => setDraft(e.target.value)}
          onBlur={commit}
          onKeyDown={(e) => {
            if (e.key === 'Enter') commit();
            if (e.key === 'Escape') {
              setDraft(feature.note);
              closePanel();
            }
          }}
          placeholder="说明"
          className="min-w-0 flex-1 bg-transparent text-[10px] text-fg-2 outline-none placeholder:text-fg-4"
        />
      ) : (
        <button
          type="button"
          data-no-drag
          data-testid={`board-feature-note-${feature.id}`}
          title="点击填写该特性的说明(右键该特性有更多操作)"
          onClick={() => {
            setDraft(feature.note);
            openPanel({ kind: 'feature-note', node: node.id, feature: feature.id });
          }}
          className={cn(
            'min-w-0 flex-1 truncate text-left text-[10px]',
            feature.note !== '' ? 'text-fg-3' : 'italic text-fg-4',
          )}
        >
          {feature.note !== '' ? feature.note : '说明…'}
        </button>
      )}
      <button
        type="button"
        data-no-drag
        data-testid={`board-feature-remove-${feature.id}`}
        title="删除该特性(相关连线一并删除)"
        onClick={() => removeFeature(node.id, feature.id)}
        className="shrink-0 rounded p-px text-fg-4 opacity-0 transition-opacity hover:text-danger group-hover/feat:opacity-100"
      >
        <X size={9} strokeWidth={2} />
      </button>
      {/* 特性入线 port(左缘):从这里起手 = 对方 → 本特性 */}
      <span
        data-no-drag
        data-board-port-in={`${node.id}:${feature.id}`}
        title={`${IN_PORT_TITLE};交互挂在该特性上`}
        onPointerDown={(e) => onLinkStart(e, { node: node.id, feature: feature.id }, 'in')}
        onPointerMove={onLinkMove}
        onPointerUp={onLinkEnd}
        className="absolute -left-[5px] block h-2.5 w-2.5 cursor-crosshair rounded-full border-2 border-edge-strong bg-shell-panel transition-colors hover:border-warn hover:bg-warn"
        style={{ top: (FEATURE_H - 10) / 2 }}
      />
      {/* 特性出线 port(右缘):交互可直接挂在这个特性上 */}
      <span
        data-no-drag
        data-board-port={`${node.id}:${feature.id}`}
        title={`${OUT_PORT_TITLE};交互挂在该特性上`}
        onPointerDown={(e) => onLinkStart(e, { node: node.id, feature: feature.id }, 'out')}
        onPointerMove={onLinkMove}
        onPointerUp={onLinkEnd}
        className="absolute -right-[5px] block h-2.5 w-2.5 cursor-crosshair rounded-full border-2 border-warn bg-shell-panel transition-colors hover:bg-warn"
        style={{ top: (FEATURE_H - 10) / 2 }}
      />
    </div>
  );
}

// ---------- 实体卡 ----------

export interface BoardNodeCardProps extends LinkHandlers {
  node: BoardNode;
  kind: EntityKindDef;
  /** 有效位置(拖拽中 = 拖拽位,否则 = store 位) */
  pos: [number, number];
  dragging: boolean;
  onDragStart: (e: ReactPointerEvent<HTMLDivElement>, node: BoardNode) => void;
  onDragMove: (e: ReactPointerEvent<HTMLDivElement>, node: BoardNode) => void;
  onDragEnd: (node: BoardNode) => void;
}

export default function BoardNodeCard({
  node,
  kind,
  pos,
  dragging,
  onDragStart,
  onDragMove,
  onDragEnd,
  onLinkStart,
  onLinkMove,
  onLinkEnd,
}: BoardNodeCardProps) {
  const renameNode = useDesignBoardStore((s) => s.renameNode);
  const setNodeDesc = useDesignBoardStore((s) => s.setNodeDesc);
  const removeNode = useDesignBoardStore((s) => s.removeNode);
  const openNode = useDesignBoardStore((s) => s.openNode);
  const attachAsset = useDesignBoardStore((s) => s.attachAsset);
  const toggleCollapse = useDesignBoardStore((s) => s.toggleCollapse);
  const openPanel = useDesignBoardStore((s) => s.openPanel);
  const closePanel = useDesignBoardStore((s) => s.closePanel);
  // 三个开合态都在 store:卡上按钮、右键菜单、快捷键是同一个入口的三种点法
  const editingName = useDesignBoardStore(
    (s) => s.panel?.kind === 'rename' && s.panel.node === node.id,
  );
  const menuOpen = useDesignBoardStore(
    (s) => s.panel?.kind === 'feature-menu' && s.panel.node === node.id,
  );
  const assetPickerOpen = useDesignBoardStore(
    (s) => s.panel?.kind === 'asset-picker' && s.panel.node === node.id,
  );
  const selectedIds = useDesignBoardStore((s) => s.selectedNodeIds);
  const selected = useDesignBoardStore((s) => s.selectedNodeId === node.id || s.selectedNodeIds.includes(node.id));
  const tone = TONE_META[kind.tone];
  const Icon = kindIcon(kind);
  const [nameDraft, setNameDraft] = useState(node.name);
  const [dropActive, setDropActive] = useState(false);

  // 改名可由菜单 / F2 从卡外发起,进编辑态时草稿要对齐当前名字
  useEffect(() => {
    if (editingName) setNameDraft(node.name);
  }, [editingName, node.name]);

  const commitName = () => {
    closePanel();
    renameNode(node.id, nameDraft); // store 内 trim,空名忽略
  };

  /** 同一面板再点一次 = 收起 */
  const togglePanel = (kindOf: 'feature-menu' | 'asset-picker', open: boolean) => {
    if (open) closePanel();
    else openPanel({ kind: kindOf, node: node.id });
  };

  /** Assets 面板拖放挂载(dataTransfer 三键由 AssetsPanel 写入) */
  const onAssetDrop = (e: ReactDragEvent<HTMLDivElement>) => {
    if (!e.dataTransfer.types.includes('forge/asset-guid')) return;
    e.preventDefault();
    setDropActive(false);
    attachAsset(node.id, {
      guid: e.dataTransfer.getData('forge/asset-guid'),
      path: e.dataTransfer.getData('forge/asset-path'),
      type: e.dataTransfer.getData('forge/asset-type'),
    });
  };

  return (
    <>
      <div
        data-board-node={node.id}
        data-board-node-selected={selected ? '' : undefined}
        className={cn(
          'group absolute select-none rounded-md border bg-shell-panel shadow-composer',
          dragging ? 'cursor-grabbing border-fg-4' : 'cursor-grab border-edge-strong',
          // 选中 = 快捷键的作用对象,边框比悬停更重,一眼看出键会打到谁
          selected && !dragging && 'border-acc',
          dropActive && 'ring-1 ring-acc',
        )}
        style={{ left: pos[0], top: pos[1], width: NODE_W }}
        onPointerDown={(e) => onDragStart(e, node)}
        onPointerMove={(e) => onDragMove(e, node)}
        onPointerUp={() => onDragEnd(node)}
        onDoubleClick={(e) => {
          if ((e.target as HTMLElement).closest('[data-no-drag]')) return;
          openNode(node.id);
        }}
        onDragOver={(e) => {
          if (e.dataTransfer.types.includes('forge/asset-guid')) {
            e.preventDefault();
            setDropActive(true);
          }
        }}
        onDragLeave={() => setDropActive(false)}
        onDrop={onAssetDrop}
      >
        <div className={cn('rounded-t-md', tone.bar)} style={{ height: BAR_H }} />
        <div className="flex items-center gap-1 px-1.5" style={{ height: TITLE_H }}>
          <AnnotationHandle reference={editorReference('blueprint', { resourceId: 'main', selection: { nodeIds: selectedIds.includes(node.id) ? selectedIds : [node.id] } })} label={node.name} />
          <Icon size={11} strokeWidth={1.8} className={cn('shrink-0', tone.text)} />
          {editingName ? (
            <input
              autoFocus
              data-no-drag
              data-testid={`board-node-name-input-${node.id}`}
              value={nameDraft}
              onChange={(e) => setNameDraft(e.target.value)}
              onBlur={commitName}
              onKeyDown={(e) => {
                if (e.key === 'Enter') commitName();
                if (e.key === 'Escape') {
                  setNameDraft(node.name);
                  closePanel();
                }
              }}
              className="w-full min-w-0 rounded border border-edge-strong bg-shell-panel px-1 py-px text-2xs text-fg outline-none focus:border-fg-4"
            />
          ) : (
            <span
              role="button"
              tabIndex={0}
              data-no-drag
              data-testid={`board-node-name-${node.id}`}
              title={`${kind.label}(引擎分类 ${kind.category}),点击改名;右键卡身有更多操作`}
              onClick={() => openPanel({ kind: 'rename', node: node.id })}
              onKeyDown={(e) => {
                if (e.key === 'Enter') openPanel({ kind: 'rename', node: node.id });
              }}
              className="min-w-0 flex-1 cursor-text truncate text-2xs font-medium text-fg"
            >
              {node.name}
            </span>
          )}
          <span className={cn('shrink-0 text-[10px]', tone.text)}>{kind.label}</span>
          <button
            type="button"
            data-no-drag
            data-testid={`board-node-open-${node.id}`}
            title="打开实例详情画布(素材与交互;双击卡身同效)"
            onClick={() => openNode(node.id)}
            className="shrink-0 rounded p-0.5 text-fg-4 opacity-0 transition-opacity hover:text-fg-2 group-hover:opacity-100"
          >
            <Maximize2 size={10} strokeWidth={2} />
          </button>
          <button
            type="button"
            data-no-drag
            data-testid={`board-node-remove-${node.id}`}
            title="删除实体(特性与相关连线一并删除)"
            onClick={() => removeNode(node.id)}
            className="shrink-0 rounded p-0.5 text-fg-4 opacity-0 transition-opacity hover:text-danger group-hover:opacity-100"
          >
            <X size={10} strokeWidth={2} />
          </button>
        </div>
        <textarea
          data-no-drag
          data-testid={`board-node-desc-${node.id}`}
          value={node.desc}
          onChange={(e) => setNodeDesc(node.id, e.target.value)}
          placeholder={`${kind.label}描述:外观 / 布局 / 行为…`}
          style={{ height: DESC_H }}
          className="block w-full resize-none border-t border-edge bg-transparent px-1.5 py-1 text-2xs text-fg-2 outline-none placeholder:text-fg-4"
        />
        {/* 素材行:微缩略图条 + 计数 + 选择器入口(完整管理在详情画布) */}
        <div
          data-testid={`board-node-assets-${node.id}`}
          className="flex items-center gap-1 border-t border-edge px-1.5"
          style={{ height: ASSET_ROW_H }}
        >
          {node.assets.slice(0, 3).map((a) => (
            <span
              key={a.id}
              data-testid={`board-node-asset-thumb-${a.id}`}
              title={`${a.path} [${a.type}]`}
              className="block h-3.5 w-3.5 shrink-0 overflow-hidden rounded-sm"
            >
              <Thumb item={{ path: a.path, guid: a.guid, type: a.type, size: 0 } as AssetItem} size={9} />
            </span>
          ))}
          <span className="min-w-0 truncate text-[10px] text-fg-4">
            {node.assets.length > 0 ? `${node.assets.length} 素材` : '无素材'}
          </span>
          <span className="flex-1" />
          <button
            type="button"
            data-no-drag
            data-testid={`board-add-asset-${node.id}`}
            title="挂载素材(图片 / 建模 / 纹理等资产;也可从 Assets 面板拖入)"
            onClick={() => togglePanel('asset-picker', assetPickerOpen)}
            className="flex items-center gap-0.5 rounded border border-edge-strong px-1 py-px text-[10px] text-fg-3 transition-colors hover:bg-shell-hover hover:text-fg-2"
          >
            <Plus size={9} strokeWidth={2} />
            素材
          </button>
        </div>
        <div
          className="flex items-center gap-1 border-t border-edge px-1.5"
          style={{ height: ADD_ROW_H }}
        >
          <button
            type="button"
            data-no-drag
            data-testid={`board-collapse-${node.id}`}
            disabled={node.features.length === 0}
            title={
              node.collapsed
                ? '展开特性:端口列还原成卡下的特性子节点'
                : '收起特性:子节点折进卡片右缘端口列,连线不断'
            }
            onClick={() => toggleCollapse(node.id)}
            className="shrink-0 rounded p-px text-fg-4 transition-colors hover:bg-shell-hover hover:text-fg-2 disabled:opacity-40 disabled:hover:bg-transparent"
          >
            {node.collapsed ? (
              <ChevronRight size={11} strokeWidth={2} />
            ) : (
              <ChevronDown size={11} strokeWidth={2} />
            )}
          </button>
          <span className="text-[10px] text-fg-4">
            {node.features.length} 特性{node.collapsed && node.features.length > 0 ? '(已收起)' : ''}
          </span>
          <span className="flex-1" />
          <button
            type="button"
            data-no-drag
            data-testid={`board-add-feature-${node.id}`}
            title="给该实体添加一个特性(ECS 组件)"
            onClick={() => togglePanel('feature-menu', menuOpen)}
            className="flex items-center gap-0.5 rounded border border-edge-strong px-1 py-px text-[10px] text-fg-3 transition-colors hover:bg-shell-hover hover:text-fg-2"
          >
            <Plus size={9} strokeWidth={2} />
            特性
          </button>
        </div>
        {menuOpen && <FeatureMenu node={node} onClose={closePanel} />}
        {assetPickerOpen && (
          <BoardAssetPicker
            nodeId={node.id}
            onClose={closePanel}
            className="absolute left-1.5 top-full mt-0.5"
          />
        )}
        {/* 实体入线 port(左缘):连线终点吸到这里,也可从这里起手反向连 */}
        <span
          data-no-drag
          data-board-port-in={node.id}
          title={IN_PORT_TITLE}
          onPointerDown={(e) => onLinkStart(e, { node: node.id }, 'in')}
          onPointerMove={onLinkMove}
          onPointerUp={onLinkEnd}
          className="absolute -left-[6px] block h-3 w-3 cursor-crosshair rounded-full border-2 border-edge-strong bg-shell-panel transition-colors hover:border-warn hover:bg-warn"
          style={{ top: ANCHOR_Y - 6 }}
        />
        {/* 实体出线 port(右缘) */}
        <span
          data-no-drag
          data-board-port={node.id}
          title={OUT_PORT_TITLE}
          onPointerDown={(e) => onLinkStart(e, { node: node.id }, 'out')}
          onPointerMove={onLinkMove}
          onPointerUp={onLinkEnd}
          className="absolute -right-[6px] block h-3 w-3 cursor-crosshair rounded-full border-2 border-warn bg-shell-panel transition-colors hover:bg-warn"
          style={{ top: ANCHOR_Y - 6 }}
        />
        {/* 收起态端口列:一个特性一颗端口,悬停出名牌;仍是连线的起手点与落点 */}
        {node.collapsed &&
          node.features.map((f, i) => (
            <span
              key={f.id}
              data-no-drag
              data-board-feature={f.id}
              data-board-feature-node={node.id}
              data-board-port={`${node.id}:${f.id}`}
              data-testid={`board-feature-port-${f.id}`}
              title={`${f.type}${f.custom ? '(自定义,引擎未注册)' : '(引擎组件)'}${
                f.note !== '' ? ` — ${f.note}` : ''
              };${OUT_PORT_TITLE},拖线落到此点 = 对方 → 本特性`}
              onPointerDown={(e) => onLinkStart(e, { node: node.id, feature: f.id }, 'out')}
              onPointerMove={onLinkMove}
              onPointerUp={onLinkEnd}
              className={cn(
                'group/port absolute -right-[5px] block h-2.5 w-2.5 cursor-crosshair rounded-full border-2 border-warn transition-colors hover:bg-warn',
                f.custom ? 'bg-warn/40' : 'bg-shell-panel',
              )}
              style={{ top: collapsedPortY(i) - 5 }}
            >
              <span className="pointer-events-none absolute left-[13px] top-1/2 hidden -translate-y-1/2 whitespace-nowrap rounded border border-edge-strong bg-shell-float px-1 font-mono text-[10px] text-fg-2 shadow-pop group-hover/port:block">
                {f.type}
              </span>
            </span>
          ))}
      </div>

      {/* 特性子节点:有几个特性就有几个,跟随实体卡定位;收起态改由卡右缘端口列代表 */}
      {!node.collapsed &&
        node.features.map((f, i) => {
          const [ox, oy] = featureOffset(i);
          return (
            <FeatureNode
              key={f.id}
              node={node}
              feature={f}
              pos={[pos[0] + ox, pos[1] + oy]}
              onLinkStart={onLinkStart}
              onLinkMove={onLinkMove}
              onLinkEnd={onLinkEnd}
            />
          );
        })}
    </>
  );
}
import AnnotationHandle from './AnnotationHandle';
import { editorReference } from '@/lib/editorReferences';
