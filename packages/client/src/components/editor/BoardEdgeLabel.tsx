import { useEffect, useState } from 'react';
import { X, Zap } from 'lucide-react';
import { cn } from '@/lib/cn';
import { useDesignBoardStore, type BoardEdge } from '@/lib/designBoardStore';

/**
 * 线上交互标签(主画板与实例详情画布共用):
 * 显示 / 行内编辑交互描述,悬浮出删除按钮;刚拉出的新边(pendingEdgeId)自动进编辑态。
 */
export default function BoardEdgeLabel({
  edge,
  x,
  y,
  title,
}: {
  edge: BoardEdge;
  x: number;
  y: number;
  title: string;
}) {
  const setEdgeLabel = useDesignBoardStore((s) => s.setEdgeLabel);
  const removeEdge = useDesignBoardStore((s) => s.removeEdge);
  const pendingEdgeId = useDesignBoardStore((s) => s.pendingEdgeId);
  const clearPendingEdge = useDesignBoardStore((s) => s.clearPendingEdge);
  const [editing, setEditing] = useState(false);
  const [draft, setDraft] = useState(edge.label);

  // 刚拉出的新边:标签直接进入编辑态(一次性消费 pendingEdgeId)
  useEffect(() => {
    if (pendingEdgeId !== edge.id) return;
    setDraft(edge.label);
    setEditing(true);
    clearPendingEdge();
  }, [pendingEdgeId, edge.id, edge.label, clearPendingEdge]);

  const commit = () => {
    setEditing(false);
    const v = draft.trim();
    if (v !== edge.label) setEdgeLabel(edge.id, v);
  };

  return (
    <div
      data-board-edge-label={edge.id}
      title={title}
      className="group absolute z-10 flex -translate-x-1/2 -translate-y-1/2 items-center gap-1 rounded-full border border-warn/40 bg-shell-panel px-1.5 py-0.5 shadow-sm"
      style={{ left: x, top: y }}
    >
      <Zap size={10} strokeWidth={2} className="shrink-0 text-warn" />
      {editing ? (
        <input
          autoFocus
          data-testid={`board-edge-input-${edge.id}`}
          value={draft}
          onChange={(e) => setDraft(e.target.value)}
          onBlur={commit}
          onKeyDown={(e) => {
            if (e.key === 'Enter') commit();
            if (e.key === 'Escape') {
              setDraft(edge.label);
              setEditing(false);
            }
          }}
          placeholder="交互描述,如:碰到后开门"
          className="w-[150px] bg-transparent text-2xs text-fg outline-none placeholder:text-fg-4"
        />
      ) : (
        <button
          type="button"
          data-testid={`board-edge-label-${edge.id}`}
          title="点击编辑交互描述"
          onClick={() => {
            setDraft(edge.label);
            setEditing(true);
          }}
          className={cn(
            'max-w-[180px] truncate text-2xs',
            edge.label !== '' ? 'text-fg-2' : 'italic text-fg-4',
          )}
        >
          {edge.label !== '' ? edge.label : '点击填写交互'}
        </button>
      )}
      <button
        type="button"
        data-testid={`board-edge-remove-${edge.id}`}
        title="删除连线"
        onClick={() => removeEdge(edge.id)}
        className="hidden shrink-0 text-fg-4 hover:text-danger group-hover:block"
      >
        <X size={10} strokeWidth={2} />
      </button>
    </div>
  );
}
