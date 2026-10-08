import type { ShaderGraphDoc } from '@forge/protocol';
import { getEditorDocument, putEditorDocument } from './editorApi';
import { documentSyncKey, documentSyncTasks, useEditorDocumentSync } from './editorDocumentState';
import { useGraphStore, type GraphDoc } from './graphStore';
import { useShaderGraphStore } from './shaderGraphStore';
import { readActiveWorkspaceId } from './activeWorkspace';

type Graph = ShaderGraphDoc | GraphDoc;
/** Draft graph documents are agent-visible without publishing an asset. Each id has its own CAS revision. */
export function bindEditorGraphDocuments(workspaceId: string): () => void {
  let live = true;
  const disposers: Array<() => void> = [];
  const attach = (kind: 'shaderGraph' | 'logicGraph', read: () => { graph: Graph | null; dirty: boolean }, apply: (graph: Graph) => void, subscribe: (fn: () => void) => () => void) => {
    const entries = new Map<string, { update: (graph: Graph) => void; reload: () => void; flush: () => Promise<void>; dispose: () => void }>();
    let applying = false;
    const changed = () => {
      if (!live || applying || readActiveWorkspaceId() !== workspaceId) return;
      const initial = read();
      if (!initial.graph) return;
      const id = initial.graph.id;
      const existing = entries.get(id);
      if (existing) { existing.update(initial.graph); return; }
      const key = documentSyncKey(workspaceId, kind, id);
      const cacheKey = `forge:graph-document:${key}`;
      let latest = initial.graph, revision = 0, baseline = '', loaded = false;
      let cache: { revision: number; document: Graph } | null = null;
      try { cache = JSON.parse(localStorage.getItem(cacheKey) ?? 'null'); } catch { /* recover in memory */ }
      if (cache) { revision = cache.revision; baseline = JSON.stringify(cache.document); }
      let pending = cache ? JSON.stringify(latest) !== baseline : initial.dirty;
      let saving: Promise<void> | null = null;
      let timer: ReturnType<typeof setTimeout> | undefined;
      let error: string | null = null;
      const status = () => useEditorDocumentSync.setState((s) => ({ status: { ...s.status, [key]: { revision, pending, error } } }));
      const save = async (): Promise<void> => {
        if (saving) { await saving; if (pending) return save(); return; }
        if (!loaded || !pending) return;
        const document = latest, encoded = JSON.stringify(document);
        saving = (async () => {
          try {
            const result = await putEditorDocument(workspaceId, kind, id, document, revision);
            revision = result.revision; baseline = encoded; pending = JSON.stringify(latest) !== baseline; error = null;
            localStorage.setItem(cacheKey, JSON.stringify({ revision, document })); status();
          } catch (e) { error = (e as Error).message; status(); throw e; }
          finally { saving = null; }
        })();
        await saving;
        if (pending) await save();
      };
      const load = async () => {
        try {
          const result = await getEditorDocument<Graph | null>(workspaceId, kind, id);
          if (result.document && !pending) {
            revision = result.revision; latest = result.document; baseline = JSON.stringify(latest); error = null;
            if (live && readActiveWorkspaceId() === workspaceId && read().graph?.id === id && JSON.stringify(read().graph) !== baseline) { applying = true; apply(latest); applying = false; }
            localStorage.setItem(cacheKey, JSON.stringify({ revision, document: latest }));
          } else if (!result.document) { revision = 0; pending = true; }
          loaded = true; status();
          if (pending) await save();
        } catch (e) { error = (e as Error).message; status(); }
      };
      const ready = load();
      const flush = async () => { await ready; if (!loaded) throw new Error(`${kind} 草稿尚未读取`); await save(); };
      const entry = {
        update: (graph: Graph) => { latest = graph; pending = JSON.stringify(graph) !== baseline; status(); clearTimeout(timer); timer = setTimeout(() => { void flush().catch(() => {}); }, 450); },
        reload: () => { if (pending || saving) return; void load(); },
        flush,
        dispose: () => { clearTimeout(timer); void flush().catch(() => {}).finally(() => { if (documentSyncTasks.get(key) === flush) documentSyncTasks.delete(key); }); },
      };
      entries.set(id, entry); documentSyncTasks.set(key, flush);
    };
    const off = subscribe(changed);
    const reload = () => { for (const entry of entries.values()) entry.reload(); };
    window.addEventListener(`forge:editor-document:${kind}`, reload);
    changed();
    disposers.push(() => { off(); window.removeEventListener(`forge:editor-document:${kind}`, reload); for (const entry of entries.values()) entry.dispose(); });
  };
  attach('shaderGraph', () => { const state = useShaderGraphStore.getState(); return { graph: state.workspaceId === workspaceId ? state.graph : null, dirty: state.dirty }; }, (graph) => { useShaderGraphStore.getState().change(() => graph as ShaderGraphDoc); useShaderGraphStore.setState({ compilation: null }); }, (fn) => useShaderGraphStore.subscribe((s, p) => { if (s.graph !== p.graph) fn(); }));
  attach('logicGraph', () => useGraphStore.getState(), (graph) => useGraphStore.setState({ graph: graph as GraphDoc, dirty: true }), (fn) => useGraphStore.subscribe((s, p) => { if (s.graph !== p.graph) fn(); }));
  return () => { for (const dispose of disposers) dispose(); live = false; };
}
