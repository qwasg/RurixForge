import { create } from 'zustand';
import type { EditorAnnotation, EditorDocumentKind } from '@forge/protocol';
import { readActiveWorkspaceId } from './activeWorkspace';
interface DocumentSyncState { status: Record<string, { revision: number; pending: boolean; error: string | null }> }
export const useEditorDocumentSync = create<DocumentSyncState>(() => ({ status: {} }));
export const documentSyncTasks = new Map<string, () => Promise<void>>();
export const documentSyncKey = (workspaceId: string, kind: EditorDocumentKind, id = 'main') => `${workspaceId}:${kind}:${id}`;
export const documentRevision = (workspaceId: string, kind: EditorDocumentKind, id = 'main') => useEditorDocumentSync.getState().status[documentSyncKey(workspaceId, kind, id)]?.revision;
export const flushEditorDocuments = async (annotations?: EditorAnnotation[]) => {
  const workspaceId = readActiveWorkspaceId();
  const referenced = annotations ? new Set(annotations.map(({ reference: r }) => documentSyncKey(r.workspaceId, r.kind as EditorDocumentKind, r.resourceId ?? 'main'))) : null;
  await Promise.all([...documentSyncTasks].filter(([key]) => (!workspaceId || key.startsWith(`${workspaceId}:`)) && (!referenced || referenced.has(key))).map(([, flush]) => flush()));
};
