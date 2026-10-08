import type { EditorReference } from '@forge/protocol';
import { readActiveWorkspaceId } from './activeWorkspace';
import { apiPost } from './forgeApi';
import { useEditorStore } from './editorStore';
import { editorReference, entityReference } from './editorReferences';
import { useDesignBoardStore } from './designBoardStore';
import { useStudioStore } from './studioStore';
import { useGraphStore } from './graphStore';
import { useShaderGraphStore } from './shaderGraphStore';

let timer: ReturnType<typeof setTimeout> | undefined;
/** Selection is a small discoverable index; source contents remain behind editor.read. */
export function publishEditorSelection(references: EditorReference[]): void {
  const workspaceId = readActiveWorkspaceId();
  clearTimeout(timer);
  if (!workspaceId) return;
  timer = setTimeout(() => {
    if (readActiveWorkspaceId() !== workspaceId) return;
    void apiPost('/api/forge/editor/selection', { workspaceId, references: references.filter((r) => r.workspaceId === workspaceId).slice(0, 64) }).catch(() => undefined);
  }, 200);
}

export function bindEditorSelection(workspaceId: string): () => void {
  const publish = () => {
    if (readActiveWorkspaceId() !== workspaceId) return;
    const editor = useEditorStore.getState();
    if (editor.centerTab === 'design') {
      const board = useDesignBoardStore.getState();
      publishEditorSelection([editorReference('blueprint', { resourceId: 'main', selection: board.selectedNodeIds.length ? { nodeIds: board.selectedNodeIds } : board.selectedNodeId ? { nodeIds: [board.selectedNodeId] } : undefined })]);
    } else if (editor.centerTab === 'studio') {
      const studio = useStudioStore.getState();
      publishEditorSelection([editorReference('studio', { resourceId: 'main', selection: studio.selectedNodeIds.length ? { nodeIds: studio.selectedNodeIds } : studio.selectedNodeId ? { nodeIds: [studio.selectedNodeId] } : undefined })]);
    } else if (editor.centerTab === 'shadergraph') {
      const shader = useShaderGraphStore.getState();
      publishEditorSelection([editorReference('shaderGraph', { resourceId: shader.graph.id, path: shader.path ?? undefined, revision: shader.sourceHash ?? undefined, selection: { nodeIds: shader.selected } })]);
    } else if (editor.centerTab === 'nodegraph') {
      const logic = useGraphStore.getState();
      publishEditorSelection(logic.graph ? [editorReference('logicGraph', { resourceId: logic.graph.id, path: logic.graphPath ?? undefined, selection: { nodeIds: logic.selectedNodeIds } })] : []);
    } else {
      const ids = new Set(editor.selectedIds.length ? editor.selectedIds : editor.selectedId === null ? [] : [editor.selectedId]);
      publishEditorSelection(editor.entities.filter((e) => ids.has(e.id)).map((e) => entityReference(e)));
    }
  };
  const disposers = [
    useEditorStore.subscribe((s, prev) => { if (s.selectedId !== prev.selectedId || s.selectedIds !== prev.selectedIds || s.centerTab !== prev.centerTab || s.contentRevision !== prev.contentRevision) publish(); }),
    useDesignBoardStore.subscribe((s, prev) => { if (s.selectedNodeId !== prev.selectedNodeId || s.selectedNodeIds !== prev.selectedNodeIds) publish(); }),
    useStudioStore.subscribe((s, prev) => { if (s.selectedNodeId !== prev.selectedNodeId || s.selectedNodeIds !== prev.selectedNodeIds) publish(); }),
    useGraphStore.subscribe((s, prev) => { if (s.selectedNodeIds !== prev.selectedNodeIds || s.graphPath !== prev.graphPath) publish(); }),
    useShaderGraphStore.subscribe((s, prev) => { if (s.selected !== prev.selected || s.sourceHash !== prev.sourceHash) publish(); }),
  ];
  publish();
  return () => { disposers.forEach((dispose) => dispose()); clearTimeout(timer); };
}
