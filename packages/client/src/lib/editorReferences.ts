import { create } from 'zustand';
import { EDITOR_REFERENCE_MIME, type EditorAnnotation, type EditorReference } from '@forge/protocol';
import { readActiveWorkspaceId } from './activeWorkspace';
import { useSessionStore } from './sessionStore';
import { useEditorStore, type EntityData } from './editorStore';
import { useWorkbenchStore } from './workbenchStore';
import { useToastStore } from './toastStore';
import { getEditorDocument, resolveEditorReference } from './editorApi';
import { useWorkspaceStore } from './workspaceStore';
import { documentRevision, flushEditorDocuments } from './editorDocumentState';
import { useGraphStore } from './graphStore';
import { useShaderGraphStore } from './shaderGraphStore';

export { EDITOR_REFERENCE_MIME };
export const annotationDraftKey = (sessionId: string | null, workspaceId: string | null) => JSON.stringify([workspaceId, sessionId ?? '@draft']);
export const activeAnnotationDraftKey = () => annotationDraftKey(useSessionStore.getState().activeSessionId, readActiveWorkspaceId());
const storageKey = 'forge:editor-annotation-drafts:v1';
function loadDrafts(): Record<string, EditorAnnotation[]> {
  try { return JSON.parse(localStorage.getItem(storageKey) ?? '{}'); } catch { return {}; }
}
export function annotationKey(annotation: EditorAnnotation): string {
  const r = annotation.reference;
  return JSON.stringify([r.workspaceId, r.kind, r.sceneGuid, r.entityGuid, r.entityId, r.resourceId, r.path, r.selection, r.revision, annotation.observationId]);
}
interface AnnotationState {
  viewportOverlayOpen: boolean;
  drafts: Record<string, EditorAnnotation[]>;
  reveal: { reference: EditorReference; token: number } | null;
  add: (items: EditorAnnotation[], key?: string) => void;
  update: (id: string, note: string, key?: string) => void;
  remove: (id: string, key?: string) => void;
  acknowledge: (items: EditorAnnotation[], key: string) => void;
}
export const useEditorAnnotationStore = create<AnnotationState>((set, get) => {
  const write = (drafts: AnnotationState['drafts']) => {
    set({ drafts });
    try { localStorage.setItem(storageKey, JSON.stringify(drafts)); } catch { /* In-memory draft remains available. */ }
  };
  return {
    drafts: loadDrafts(), reveal: null, viewportOverlayOpen: false,
    add: (items, key = activeAnnotationDraftKey()) => {
      const current = [...(get().drafts[key] ?? [])];
      for (const item of items) if (!current.some((a) => annotationKey(a) === annotationKey(item))) current.push(item);
      if (current.length > 64) { useToastStore.getState().push('warning', '每条消息最多 64 个批注；请先发送当前批注。'); return; }
      write({ ...get().drafts, [key]: current });
      window.dispatchEvent(new Event('forge:focus-composer'));
    },
    update: (id, note, key = activeAnnotationDraftKey()) => write({ ...get().drafts, [key]: (get().drafts[key] ?? []).map((a) => a.id === id ? { ...a, note } : a) }),
    remove: (id, key = activeAnnotationDraftKey()) => write({ ...get().drafts, [key]: (get().drafts[key] ?? []).filter((a) => a.id !== id) }),
    acknowledge: (items, key) => write({ ...get().drafts, [key]: (get().drafts[key] ?? []).filter((a) => !items.some((sent) => sent.id === a.id && JSON.stringify(sent) === JSON.stringify(a))) }),
  };
});
export function editorReference(kind: EditorReference['kind'], extra: Partial<EditorReference> = {}): EditorReference {
  const state = useEditorStore.getState();
  const sceneScoped = ['scene', 'entity', 'component', 'property', 'viewport'].includes(kind);
  const workspaceId = readActiveWorkspaceId() ?? '';
  if (kind === 'blueprint' || kind === 'studio') extra = { revision: documentRevision(workspaceId, kind, extra.resourceId ?? 'main'), ...extra };
  if (kind === 'logicGraph' || kind === 'shaderGraph') {
    const graph = kind === 'logicGraph' ? useGraphStore.getState() : useShaderGraphStore.getState();
    const id = extra.resourceId ?? graph.graph?.id;
    const dirty = graph.dirty || !extra.path;
    extra = { ...extra, resourceId: id, ...(dirty ? { revision: id ? documentRevision(workspaceId, kind, id) : undefined } : {}), selection: { ...extra.selection, dirty } };
  }
  if (extra.path) {
    const path = extra.path.replace(/\\/g, '/').replace(/^\/\/\?\//, '');
    const root = useWorkspaceStore.getState().workspaces.find((w) => w.id === readActiveWorkspaceId())?.root?.replace(/\\/g, '/').replace(/^\/\/\?\//, '').replace(/\/$/, '');
    extra = { ...extra, path: root && path.toLowerCase().startsWith(`${root.toLowerCase()}/`) ? path.slice(root.length + 1) : path };
  }
  return { workspaceId, kind, ...(sceneScoped ? { sceneGuid: state.sceneGuid ?? undefined, hostEpoch: state.hostEpoch ?? undefined,
    identityPersisted: state.identityPersisted ?? undefined, revision: state.contentRevision ?? undefined, targetMode: state.playState === 'edit' ? 'edit' : 'runtime' } : {}), ...extra };
}
export function entityReference(entity: EntityData, extra: Partial<EditorReference> = {}): EditorReference {
  return editorReference('entity', { entityId: entity.id, entityGuid: entity.guid ?? entity.entityGuid, entityIdentityPersisted: entity.identityPersisted, ...extra });
}
export function makeAnnotation(reference: EditorReference, label: string, note?: string): EditorAnnotation {
  return { id: crypto.randomUUID(), reference, label, ...(note ? { note } : {}) };
}
export function decodeAnnotationDrop(data: Pick<DataTransfer, 'getData'>): EditorAnnotation[] {
  try {
    const encoded = data.getData(EDITOR_REFERENCE_MIME);
    if (new TextEncoder().encode(encoded).length > 128 * 1024) return [];
    const raw = JSON.parse(encoded);
    if (!Array.isArray(raw) || raw.length > 64) return [];
    return raw.filter((a) => a && typeof a.id === 'string' && a.reference && typeof a.reference.workspaceId === 'string' && ['scene','entity','component','property','asset','blueprint','studio','logicGraph','shaderGraph','source','viewport'].includes(a.reference.kind));
  } catch { return []; }
}
export async function revealAnnotation(annotation: EditorAnnotation): Promise<void> {
  try {
    const source = annotation.reference;
    if (source.workspaceId !== readActiveWorkspaceId()) throw new Error('请先切换到此批注所属工作区，再定位原对象');
    const result = await resolveEditorReference(source);
    if (source.workspaceId !== readActiveWorkspaceId()) return;
    if (result.status === 'missing') throw new Error(result.message ?? '引用对象已删除');
    const ref = result.reference ?? source;
    const workbench = useWorkbenchStore.getState();
    if (ref.kind === 'source' && ref.path) workbench.openFile(ref.path);
    else {
      workbench.openTab('editor');
      const editor = useEditorStore.getState();
      if (ref.kind === 'blueprint') editor.setCenterTab('design');
      else if (ref.kind === 'studio') editor.setCenterTab('studio');
      else if (ref.kind === 'logicGraph') editor.setCenterTab('nodegraph');
      else if (ref.kind === 'shaderGraph') editor.setCenterTab('shadergraph');
      else if (ref.kind !== 'asset') editor.setCenterTab('viewport');
      if ((ref.kind === 'shaderGraph' || ref.kind === 'logicGraph') && ref.selection?.dirty && ref.resourceId) {
        const current = ref.kind === 'shaderGraph' ? useShaderGraphStore.getState().graph : useGraphStore.getState().graph;
        if (current?.id !== ref.resourceId) {
          await flushEditorDocuments();
          const { document } = await getEditorDocument<import('@forge/protocol').ShaderGraphDoc | import('./graphStore').GraphDoc | null>(ref.workspaceId, ref.kind, ref.resourceId);
          if (source.workspaceId !== readActiveWorkspaceId()) return;
          if (!document) throw new Error('引用的图草稿已不存在');
          if (ref.kind === 'shaderGraph') { useShaderGraphStore.getState().change(() => document as import('@forge/protocol').ShaderGraphDoc); useShaderGraphStore.setState({ path: ref.path ?? '', sourceHash: null }); }
          else useGraphStore.setState({ graph: document as import('./graphStore').GraphDoc, graphPath: ref.path ?? null, dirty: true });
        }
      }
      if (ref.entityId !== undefined) { editor.selectEntity(ref.entityId); await editor.focusSelected(); }
      if (ref.kind === 'asset' && ref.path) (await import('./assetStore')).useAssetStore.getState().focusPath(ref.path);
      if (ref.kind === 'logicGraph' && ref.path && !ref.selection?.dirty) {
        const graph = (await import('./graphStore')).useGraphStore.getState();
        if (graph.graphPath !== ref.path) {
          if (graph.dirty) throw new Error('当前逻辑图有未保存修改，请先保存后再定位其他图');
          await graph.loadByPath(ref.path);
        }
      }
      if (ref.kind === 'blueprint' && ref.selection?.nodeIds?.[0]) (await import('./designBoardStore')).useDesignBoardStore.getState().openNode(ref.selection.nodeIds[0]);
      if (ref.kind === 'studio' && ref.selection?.nodeIds?.[0]) (await import('./studioStore')).useStudioStore.getState().openNode(ref.selection.nodeIds[0]);
    }
    if (source.workspaceId !== readActiveWorkspaceId()) return;
    useEditorAnnotationStore.setState({ reveal: { reference: ref, token: Date.now() } });
    if (result.status === 'stale') useToastStore.getState().push('warning', result.message ?? '对象已更新，已定位到当前版本');
  } catch (error) { useToastStore.getState().push('error', (error as Error).message); }
}
