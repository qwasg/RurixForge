import { useEffect, useState } from 'react';
import { Link2, X } from 'lucide-react';
import { cn } from '@/lib/cn';
import { useStudioStore, type StudioEdge } from '@/lib/studioStore';

/**
 * 素材创作连线标签:引用说明(如「按大纲出原画」)行内编辑,悬浮出删除;
 * 刚拉出的新边(pendingEdgeId)自动进编辑态(与画板 BoardEdgeLabel 同交互,store 不同)。
 */
export default function StudioEdgeLabel({
  edge,
  x,
  y,
  title,
}: {
  edge: StudioEdge;
  x: number;
  y: number;
  title: string;
}) {
  const setEdgeLabel = useStudioStore((s) => s.setEdgeLabel);
  const removeEdge = useStudioStore((s) => s.removeEdge);
  const pendingEdgeId = useStudioStore((s) => s.pendingEdgeId);
  const clearPendingEdge = useStudioStore((s) => s.clearPendingEdge);
  const [editing, setEditing] = useState(false);
  const [draft, setDraft] = useState(edge.label);

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
      data-studio-edge-label={edge.id}
      title={title}
      className="group absolute z-10 flex -translate-x-1/2 -translate-y-1/2 items-center gap-1 rounded-full border border-acc/40 bg-shell-panel px-1.5 py-0.5 shadow-sm"
      style={{ left: x, top: y }}
    >
      <Link2 size={10} strokeWidth={2} className="shrink-0 text-acc" />
      {editing ? (
        <input
          autoFocus
          data-testid={`studio-edge-input-${edge.id}`}
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
          placeholder="引用说明,如:按大纲出原画"
          className="w-[150px] bg-transparent text-2xs text-fg outline-none placeholder:text-fg-4"
        />
      ) : (
        <button
          type="button"
          data-testid={`studio-edge-label-${edge.id}`}
          title="点击编辑引用说明(上游产物在生成时拼进上下文)"
          onClick={() => {
            setDraft(edge.label);
            setEditing(true);
          }}
          className={cn(
            'max-w-[180px] truncate text-2xs',
            edge.label !== '' ? 'text-fg-2' : 'italic text-fg-4',
          )}
        >
          {edge.label !== '' ? edge.label : '引用说明…'}
        </button>
      )}
      <button
        type="button"
        data-testid={`studio-edge-remove-${edge.id}`}
        title="删除连线"
        onClick={() => removeEdge(edge.id)}
        className="hidden shrink-0 text-fg-4 hover:text-danger group-hover:block"
      >
        <X size={10} strokeWidth={2} />
      </button>
    </div>
  );
}
