import { getEditorDocument, putEditorDocument } from './editorApi';
import { BUILTIN_KINDS, useDesignBoardStore } from './designBoardStore';
import { useStudioStore } from './studioStore';
import { useEditorDocumentSync, documentSyncTasks as tasks, documentSyncKey } from './editorDocumentState';
export { useEditorDocumentSync, documentSyncKey, documentRevision, flushEditorDocuments } from './editorDocumentState';

type DocumentData = Record<string, unknown>;
const sanitized = (value: unknown): DocumentData => JSON.parse(JSON.stringify(value, (key, val) => key === 'dataUrl' ? undefined : val));

/** Host owns canonical documents; localStorage remains a recoverable draft, never an overwrite authority. */
export function bindEditorDocuments(workspaceId: string): () => void {
  let live = true;
  const disposers: Array<() => void> = [];
  const connect = (kind: 'blueprint' | 'studio', get: () => DocumentData, apply: (doc: DocumentData) => void, subscribe: (fn: () => void) => () => void) => {
    const key = documentSyncKey(workspaceId, kind);
    const localKey = `forge:editor-document:${key}`;
    let revision = 0, baseline = '', dirty = false, applying = false, initialized = false;
    let preserved: DocumentData = {};
    let latest = get();
    const refreshLocal = () => { latest = { ...preserved, ...get() }; };
    const current = () => latest;
    let saving: Promise<void> | null = null;
    let timer: ReturnType<typeof setTimeout> | null = null;
    const state = (error: string | null = null) => useEditorDocumentSync.setState((s) => ({ status: { ...s.status, [key]: { revision, pending: dirty, error } } }));
    const persist = () => { try { localStorage.setItem(localKey, JSON.stringify({ document: current(), baseRevision: revision, dirty })); } catch { /* memory draft remains */ } };
    const flush = async (): Promise<void> => {
      if (saving) { await saving; if (dirty) return flush(); return; }
      if (!initialized || !dirty) return;
      const snapshot = sanitized(current());
      const encoded = JSON.stringify(snapshot);
      saving = (async () => {
        try {
          const response = await putEditorDocument(workspaceId, kind, 'main', snapshot, revision);
          revision = response.revision; baseline = encoded;
          dirty = JSON.stringify(sanitized(current())) !== encoded;
          state();
          try { localStorage.setItem(localKey, JSON.stringify({ document: dirty ? current() : snapshot, baseRevision: revision, dirty })); } catch { /* draft remains in memory */ }
          localStorage.setItem(`forge:editor-migrated:${kind}`, workspaceId);
        } catch (error) {
          state((error as Error).message); throw error;
        } finally { saving = null; }
      })();
      await saving;
      if (dirty) await flush();
    };
    // Preserve per-workspace local drafts across renderer restarts; legacy board is migrated only once.
    const initial = get();
    if (kind === 'blueprint' && !localStorage.getItem('forge:editor-legacy-blueprint:backup')) {
      const original = localStorage.getItem('forge:designBoard');
      if (original) localStorage.setItem('forge:editor-legacy-blueprint:backup', original);
    }
    let local: DocumentData | null = null;
    let localDirty = false, localRevision = 0;
    try {
      const saved = JSON.parse(localStorage.getItem(localKey) ?? 'null');
      if (saved?.document) { local = saved.document; localDirty = saved.dirty === true; localRevision = Number(saved.baseRevision) || 0; }
      else if (saved) { local = saved; localDirty = true; }
    } catch { /* malformed local draft ignored */ }
    if (kind === 'blueprint') {
      const ownerKey = 'forge:editor-legacy-blueprint:owner';
      const owner = localStorage.getItem(ownerKey) ?? localStorage.getItem('forge:editor-migrated:blueprint');
      if (!owner) localStorage.setItem(ownerKey, workspaceId);
      if (!local && owner && owner !== workspaceId) local = { ...initial, kinds: BUILTIN_KINDS, nodes: [], edges: [], bindings: {}, seq: 1 };
    }
    if (local) { preserved = local; applying = true; apply(local); applying = false; refreshLocal(); }
    const changedDuringLoad = { value: false };
    const off = subscribe(() => {
      if (applying || !live) return;
      refreshLocal();
      changedDuringLoad.value = true;
      dirty = JSON.stringify(sanitized(current())) !== baseline; persist(); state();
      if (timer) clearTimeout(timer);
      timer = setTimeout(() => { void flush().catch(() => {}); }, 450);
    });
    const load = async () => {
      try {
        let result;
        try { result = await getEditorDocument<DocumentData | null>(workspaceId, kind); }
        catch (error) { if ((error as { status?: number }).status !== 404) throw error; result = { document: null, revision: 0 }; }
        revision = result.revision;
        if (!live) {
          // The detached snapshot belongs to the old workspace; finish its pending
          // save without reading or replacing the newly selected workspace's UI.
          dirty = changedDuringLoad.value || localDirty || !result.document;
          if (localDirty && result.document) revision = localRevision;
          initialized = true;
          if (dirty) await flush();
          return;
        }
        if (result.document && !changedDuringLoad.value && !localDirty) {
          preserved = result.document;
          applying = true; apply(result.document); applying = false;
          refreshLocal();
          baseline = JSON.stringify(sanitized(current())); dirty = false; persist();
        } else { dirty = true; if (localDirty && result.document) revision = localRevision; }
        initialized = true; state();
        if (dirty) await flush();
      } catch (error) { state((error as Error).message); }
    };
    const reload = () => {
      if (dirty) { state('远端文档已更新；本地草稿已保留，请保存或重新打开后处理冲突'); return; }
      changedDuringLoad.value = false; void load();
    };
    window.addEventListener(`forge:editor-document:${kind}`, reload);
    const ready = load();
    const task = async () => { await ready; if (!initialized) throw new Error(`${kind} 文档尚未读取，请重试`); await flush(); };
    tasks.set(key, task);
    disposers.push(() => {
      off(); if (timer) clearTimeout(timer);
      void task().catch(() => {}).finally(() => { if (tasks.get(key) === task) tasks.delete(key); });
      window.removeEventListener(`forge:editor-document:${kind}`, reload);
    });
  };
  connect('blueprint', () => {
    const { seq, kinds, nodes, edges, bindings } = useDesignBoardStore.getState(); return { version: 3, seq, kinds, nodes, edges, bindings };
  }, (doc) => {
    if (!Array.isArray(doc.nodes) || !Array.isArray(doc.edges)) throw new Error('蓝图文档格式无效');
    const board = useDesignBoardStore.getState();
    const ids = new Set(doc.nodes.map((n) => n.id));
    useDesignBoardStore.setState({ nodes: doc.nodes, edges: doc.edges, bindings: (doc.bindings ?? {}) as ReturnType<typeof useDesignBoardStore.getState>['bindings'], ...(Array.isArray(doc.kinds) ? { kinds: doc.kinds } : {}), seq: Number(doc.seq) || 1, openNodeId: ids.has(board.openNodeId) ? board.openNodeId : null, selectedNodeId: ids.has(board.selectedNodeId) ? board.selectedNodeId : null, selectedNodeIds: board.selectedNodeIds.filter((id) => ids.has(id)) });
  }, (fn) => useDesignBoardStore.subscribe((next, prev) => { if (next.nodes !== prev.nodes || next.edges !== prev.edges || next.kinds !== prev.kinds || next.bindings !== prev.bindings) fn(); }));
  useStudioStore.getState().bindWorkspace(workspaceId);
  connect('studio', () => {
    const { seq, nodes, edges, readonlyWorkspaceIds, includeLibrary } = useStudioStore.getState(); return { version: 1, seq, nodes, edges, readonlyWorkspaceIds, includeLibrary };
  }, (doc) => {
    if (!Array.isArray(doc.nodes) || !Array.isArray(doc.edges)) throw new Error('创作文档格式无效');
    const studio = useStudioStore.getState();
    const ids = new Set(doc.nodes.map((n) => n.id));
    useStudioStore.setState({ nodes: doc.nodes, edges: doc.edges, seq: Number(doc.seq) || 1, readonlyWorkspaceIds: Array.isArray(doc.readonlyWorkspaceIds) ? doc.readonlyWorkspaceIds.filter((id): id is string => typeof id === 'string') : [], includeLibrary: doc.includeLibrary !== false, openNodeId: ids.has(studio.openNodeId) ? studio.openNodeId : null, selectedNodeId: ids.has(studio.selectedNodeId) ? studio.selectedNodeId : null, selectedNodeIds: studio.selectedNodeIds.filter((id) => ids.has(id)) });
  }, (fn) => useStudioStore.subscribe((next, prev) => { if (next.nodes !== prev.nodes || next.edges !== prev.edges || next.readonlyWorkspaceIds !== prev.readonlyWorkspaceIds || next.includeLibrary !== prev.includeLibrary) fn(); }));
  return () => { for (const dispose of disposers) dispose(); live = false; };
}

export function editorDocumentErrors(workspaceId: string): string[] {
  return Object.entries(useEditorDocumentSync.getState().status).filter(([key, value]) => key.startsWith(`${workspaceId}:`) && value.error).map(([, value]) => value.error!);
}
