import { useEffect, useState } from 'react';
import type { PointerEvent as ReactPointerEvent } from 'react';
import { Maximize2, X } from 'lucide-react';
import { cn } from '@/lib/cn';
import {
  currentVersion,
  nodeStatus,
  presetOf,
  useStudioStore,
  type StudioNode,
  type StudioNodeStatus,
} from '@/lib/studioStore';
import { TONE_META } from '../editor/DesignBoardNode';
import StudioPreview, { KIND_ICON } from './StudioPreview';

/**
 * 素材创作主画布的创作卡:类型色条 + 名称(点击改名)+ 状态点 + 产物预览缩略 +
 * 打开(下钻详情画布,双击卡身同效)/ 删除;左右各一个连线 port——
 * 右 = 出线(本节点产物作为对方的上游参考),左 = 入线。
 * 改名态由 store 的 pendingRenameId 也能点起来:右键菜单「重命名」与 F2 走的是
 * 这条路,卡上点标题走本地态,两者最终落到同一个输入框。
 * 几何常量集中导出,画布按同一套数字算连线锚点与包围盒。
 */

export const STUDIO_NODE_W = 200;
const BAR_H = 3;
const TITLE_H = 24;
const PREVIEW_H = 92;
const STATUS_H = 20;
export const STUDIO_NODE_H = BAR_H + TITLE_H + PREVIEW_H + STATUS_H;
/** 连线锚点纵坐标(标题行中线) */
export const STUDIO_ANCHOR_Y = BAR_H + TITLE_H / 2;

const STATUS_META: Record<StudioNodeStatus, { dot: string; label: string }> = {
  empty: { dot: 'bg-dot-idle', label: '草稿' },
  busy: { dot: 'bg-warn animate-pulse', label: '生成中…' },
  done: { dot: 'bg-info', label: '有产物' },
  accepted: { dot: 'bg-dot-done', label: '已入库' },
};

export interface StudioLinkHandlers {
  onLinkStart: (e: ReactPointerEvent<HTMLElement>, nodeId: string, dir: 'out' | 'in') => void;
  onLinkMove: (e: ReactPointerEvent<HTMLElement>) => void;
  onLinkEnd: (e: ReactPointerEvent<HTMLElement>) => void;
}

interface StudioNodeCardProps extends StudioLinkHandlers {
  node: StudioNode;
  pos: [number, number];
  dragging: boolean;
  /** 快捷键的作用对象:着重色描边标出 */
  selected: boolean;
  onDragStart: (e: ReactPointerEvent<HTMLDivElement>, node: StudioNode) => void;
  onDragMove: (e: ReactPointerEvent<HTMLDivElement>, node: StudioNode) => void;
  onDragEnd: (node: StudioNode) => void;
}

export default function StudioNodeCard({
  node,
  pos,
  dragging,
  selected,
  onDragStart,
  onDragMove,
  onDragEnd,
  onLinkStart,
  onLinkMove,
  onLinkEnd,
}: StudioNodeCardProps) {
  const renameNode = useStudioStore((s) => s.renameNode);
  const selectedIds = useStudioStore((s) => s.selectedNodeIds);
  const removeNode = useStudioStore((s) => s.removeNode);
  const openNode = useStudioStore((s) => s.openNode);
  const busyIds = useStudioStore((s) => s.busyIds);
  const pendingRenameId = useStudioStore((s) => s.pendingRenameId);
  const clearPendingRename = useStudioStore((s) => s.clearPendingRename);
  const preset = presetOf(node.preset);
  const [editingName, setEditingName] = useState(false);
  const [nameDraft, setNameDraft] = useState(node.name);

  useEffect(() => {
    if (editingName) setNameDraft(node.name);
  }, [editingName, node.name]);

  // 右键菜单「重命名」/ F2 点起的改名态(与卡上点标题殊途同归)
  useEffect(() => {
    if (pendingRenameId !== node.id) return;
    setNameDraft(node.name);
    setEditingName(true);
    clearPendingRename();
  }, [pendingRenameId, node.id, node.name, clearPendingRename]);

  if (!preset) return null; // 读档已挡未知 preset,不伪造默认类型
  const tone = TONE_META[preset.tone];
  const Icon = KIND_ICON[preset.kind];
  const status = nodeStatus(node, busyIds);
  const cur = currentVersion(node);

  const commitName = () => {
    setEditingName(false);
    renameNode(node.id, nameDraft);
  };

  return (
    <div
      data-studio-node={node.id}
      data-studio-node-selected={selected ? '' : undefined}
      data-testid={`studio-node-${node.id}`}
      className={cn(
        'group absolute select-none rounded-md border bg-shell-panel shadow-composer',
        dragging ? 'cursor-grabbing border-fg-4' : 'cursor-grab border-edge-strong',
        selected && 'border-acc ring-1 ring-acc/40',
      )}
      style={{ left: pos[0], top: pos[1], width: STUDIO_NODE_W }}
      onPointerDown={(e) => onDragStart(e, node)}
      onPointerMove={(e) => onDragMove(e, node)}
      onPointerUp={() => onDragEnd(node)}
      onDoubleClick={(e) => {
        if ((e.target as HTMLElement).closest('[data-no-drag]')) return;
        openNode(node.id);
      }}
    >
      <div className={cn('rounded-t-md', tone.bar)} style={{ height: BAR_H }} />
      <div className="flex items-center gap-1 px-1.5" style={{ height: TITLE_H }}>
        <AnnotationHandle reference={editorReference('studio', { resourceId: 'main', selection: { nodeIds: selectedIds.includes(node.id) ? selectedIds : [node.id], ...(selectedIds.length < 2 ? { versionId: cur?.id } : {}) } })} label={node.name} />
        <Icon size={11} strokeWidth={1.8} className={cn('shrink-0', tone.text)} />
        {editingName ? (
          <input
            autoFocus
            data-no-drag
            data-testid={`studio-node-name-input-${node.id}`}
            value={nameDraft}
            onChange={(e) => setNameDraft(e.target.value)}
            onBlur={commitName}
            onKeyDown={(e) => {
              if (e.key === 'Enter') commitName();
              if (e.key === 'Escape') {
                setNameDraft(node.name);
                setEditingName(false);
              }
            }}
            className="w-full min-w-0 rounded border border-edge-strong bg-shell-panel px-1 py-px text-2xs text-fg outline-none focus:border-fg-4"
          />
        ) : (
          <span
            role="button"
            tabIndex={0}
            data-no-drag
            data-testid={`studio-node-name-${node.id}`}
            title={`${preset.label},点击改名;双击卡身进入创作画布`}
            onClick={() => setEditingName(true)}
            onKeyDown={(e) => {
              if (e.key === 'Enter') setEditingName(true);
            }}
            className="min-w-0 flex-1 cursor-text truncate text-2xs font-medium text-fg"
          >
            {node.name}
          </span>
        )}
        <span className={cn('shrink-0 text-[10px]', tone.text)}>{preset.label}</span>
        <button
          type="button"
          data-no-drag
          data-testid={`studio-node-open-${node.id}`}
          title="打开创作画布(生成/版本管理;双击卡身同效)"
          onClick={() => openNode(node.id)}
          className="shrink-0 rounded p-0.5 text-fg-4 opacity-0 transition-opacity hover:text-fg-2 group-hover:opacity-100"
        >
          <Maximize2 size={10} strokeWidth={2} />
        </button>
        <button
          type="button"
          data-no-drag
          data-testid={`studio-node-remove-${node.id}`}
          title="删除该创作节点(相关连线一并删除)"
          onClick={() => removeNode(node.id)}
          className="shrink-0 rounded p-0.5 text-fg-4 opacity-0 transition-opacity hover:text-danger group-hover:opacity-100"
        >
          <X size={10} strokeWidth={2} />
        </button>
      </div>
      {/* 产物预览(当前版本;空态给类型图标) */}
      <div className="border-t border-edge bg-shell-sunk" style={{ height: PREVIEW_H }}>
        <StudioPreview kind={preset.kind} version={cur} compact className="h-full w-full" />
      </div>
      {/* 状态行:状态点 + 版本计数 */}
      <div className="flex items-center gap-1 border-t border-edge px-1.5" style={{ height: STATUS_H }}>
        <span className={cn('h-1.5 w-1.5 shrink-0 rounded-full', STATUS_META[status].dot)} />
        <span className="text-[10px] text-fg-4">{STATUS_META[status].label}</span>
        <span className="flex-1" />
        <span className="text-[10px] text-fg-4">
          {node.versions.length > 0 ? `${node.versions.length} 版本` : '未生成'}
        </span>
      </div>
      {/* 入线 port(左缘):对方产物 → 本节点上游参考 */}
      <span
        data-no-drag
        data-studio-port-in={node.id}
        title="按住拖到来源节点完成反向连线(对方产物作为本节点的上游参考)"
        onPointerDown={(e) => onLinkStart(e, node.id, 'in')}
        onPointerMove={onLinkMove}
        onPointerUp={onLinkEnd}
        className="absolute -left-[6px] block h-3 w-3 cursor-crosshair rounded-full border-2 border-edge-strong bg-shell-panel transition-colors hover:border-acc hover:bg-acc"
        style={{ top: STUDIO_ANCHOR_Y - 6 }}
      />
      {/* 出线 port(右缘):本节点产物 → 对方上游参考 */}
      <span
        data-no-drag
        data-studio-port={node.id}
        title="按住拖到目标节点完成连线(本节点产物作为对方的上游参考)"
        onPointerDown={(e) => onLinkStart(e, node.id, 'out')}
        onPointerMove={onLinkMove}
        onPointerUp={onLinkEnd}
        className="absolute -right-[6px] block h-3 w-3 cursor-crosshair rounded-full border-2 border-acc bg-shell-panel transition-colors hover:bg-acc"
        style={{ top: STUDIO_ANCHOR_Y - 6 }}
      />
    </div>
  );
}
import AnnotationHandle from '../editor/AnnotationHandle';
import { editorReference } from '@/lib/editorReferences';
