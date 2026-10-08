import { useEffect, useState } from 'react';
import { createPortal } from 'react-dom';
import { History, X } from 'lucide-react';
import type { EditorReference } from '@forge/protocol';
import { apiGet, apiPost } from '@/lib/forgeApi';
import { useWorkspaceStore } from '@/lib/workspaceStore';
import { useEditorStore } from '@/lib/editorStore';
import { revealAnnotation, useEditorAnnotationStore } from '@/lib/editorReferences';

interface Change { id: string; status: string; source?: unknown; result?: Record<string, unknown>; lastError?: unknown }
const labels: Record<string, string> = { pending: '等待同步', applied: '已应用', undone: '已撤销', conflict: '冲突', failed: '失败', recoveryRequired: '需要恢复' };
export default function EditorChanges() {
  const [open, setOpen] = useState(false);
  const [changes, setChanges] = useState<Change[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState<string | null>(null);
  const workspaceId = useWorkspaceStore((s) => s.activeWorkspaceId);
  const load = async () => {
    if (!workspaceId) return;
    try {
      const result = await apiGet<{ items?: Change[]; changes?: Change[] }>(`/api/forge/editor/changes?workspaceId=${encodeURIComponent(workspaceId)}`);
      if (useWorkspaceStore.getState().activeWorkspaceId === workspaceId) { setChanges(result.items ?? result.changes ?? []); setError(null); }
    } catch (e) { if (useWorkspaceStore.getState().activeWorkspaceId === workspaceId) setError((e as Error).message); }
  };
  useEffect(() => {
    setChanges([]); setError(null);
    if (!open) return;
    useEditorAnnotationStore.setState({ viewportOverlayOpen: true }); void load();
    const refresh = () => { void load(); };
    window.addEventListener('forge:editor-changes', refresh);
    return () => { window.removeEventListener('forge:editor-changes', refresh); useEditorAnnotationStore.setState({ viewportOverlayOpen: false }); };
  }, [open, workspaceId]);
  const apply = async (change: Change, action: 'undo' | 'redo') => {
    setBusy(change.id); setError(null);
    try { await apiPost(`/api/forge/editor/${action}`, { workspaceId, changeSetId: change.id }); await useEditorStore.getState().refreshSummary(); await load(); }
    catch (e) { setError((e as Error).message); }
    finally { setBusy(null); }
  };
  const references = (change: Change): EditorReference[] => {
    const result = change.result ?? {};
    const candidates = Array.isArray(result.references) ? result.references : result.reference ? [result.reference] : [];
    const explicit = candidates.filter((ref): ref is EditorReference => typeof ref === 'object' && ref !== null && 'kind' in ref && 'workspaceId' in ref);
    if (explicit.length || !workspaceId || typeof result.sceneGuid !== 'string') return explicit;
    const base: EditorReference = { workspaceId, kind: 'scene', sceneGuid: result.sceneGuid, hostEpoch: typeof result.hostEpoch === 'string' ? result.hostEpoch : undefined, revision: typeof result.contentRevision === 'number' ? result.contentRevision : undefined, targetMode: result.targetMode === 'runtime' ? 'runtime' : 'edit' };
    const created = Array.isArray(result.created) ? result.created as Array<{ id?: number; entityGuid?: string }> : [];
    return created.length ? created.map((entity) => ({ ...base, kind: 'entity', entityId: entity.id, entityGuid: entity.entityGuid })) : [base];
  };
  return <><button type="button" title="协作变更记录" aria-label="协作变更记录" className="flex h-6 w-6 items-center justify-center rounded text-fg-3 hover:bg-shell-hover" onClick={() => setOpen(true)}><History size={13} /></button>
    {open && createPortal(<div className="fixed inset-0 z-[1000] flex justify-end bg-black/25" onClick={() => setOpen(false)}>
      <aside role="dialog" aria-label="协作变更记录" className="flex h-full w-[420px] max-w-full flex-col border-l border-edge bg-shell-panel p-4 text-fg shadow-xl" onClick={(e) => e.stopPropagation()}>
        <header className="mb-3 flex items-center justify-between"><strong>协作变更记录</strong><button aria-label="关闭变更记录" onClick={() => setOpen(false)}><X size={16} /></button></header>
        <p className="mb-3 text-xs text-fg-4">场景、蓝图绑定与 Agent 操作的持久记录。撤销会校验当前版本。</p>
        {error && <p role="alert" className="mb-2 text-xs text-danger">{error}</p>}
        <div className="min-h-0 flex-1 space-y-2 overflow-auto">{!changes.length && <p className="text-sm text-fg-4">暂无协作变更</p>}{changes.map((change) => <article key={change.id} className="rounded border border-edge p-3 text-xs">
          <div className="flex justify-between gap-2"><code className="truncate" title={change.id}>{change.id}</code><span className="shrink-0">{labels[change.status] ?? change.status}</span></div>
          {change.source != null && <p className="mt-1 break-words text-fg-4">{typeof change.source === 'string' ? change.source : JSON.stringify(change.source)}</p>}
          {change.lastError != null && <p className="mt-1 break-words text-danger">{typeof change.lastError === 'string' ? change.lastError : JSON.stringify(change.lastError)}</p>}
          <div className="mt-2 flex gap-3">{references(change).map((reference, i) => <button key={i} onClick={() => { setOpen(false); void revealAnnotation({ id: change.id, reference }); }}>定位对象 {i + 1}</button>)}
            {change.status === 'applied' && <button disabled={busy !== null} onClick={() => void apply(change, 'undo')}>撤销</button>}{change.status === 'undone' && <button disabled={busy !== null} onClick={() => void apply(change, 'redo')}>重做</button>}
          </div>
        </article>)}</div>
      </aside>
    </div>, document.body)}
  </>;
}
