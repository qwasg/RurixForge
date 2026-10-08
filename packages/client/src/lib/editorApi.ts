import type { EditorDocument, EditorDocumentKind, EditorObservation, EditorReference, EditorResolution } from '@forge/protocol';
import { apiGet, apiPost } from './forgeApi';

const base = '/api/forge/editor';
export const editorOverview = (workspaceId: string) => apiGet<Record<string, unknown>>(`${base}/overview?workspaceId=${encodeURIComponent(workspaceId)}`);
export const resolveEditorReference = (reference: EditorReference) => apiPost<EditorResolution>(`${base}/resolve`, { workspaceId: reference.workspaceId, reference });
export const readEditorReference = (reference: EditorReference) => apiPost<Record<string, unknown>>(`${base}/read`, { workspaceId: reference.workspaceId, reference });
export const captureEditorReference = (reference: EditorReference) => apiPost<EditorObservation>(`${base}/capture`, { workspaceId: reference.workspaceId, reference });
export const getEditorDocument = <T>(workspaceId: string, kind: EditorDocumentKind, id = 'main') => apiGet<EditorDocument<T>>(`${base}/documents/${kind}/${encodeURIComponent(id)}?workspaceId=${encodeURIComponent(workspaceId)}`);
export async function putEditorDocument<T>(workspaceId: string, kind: EditorDocumentKind, id: string, document: T, expectedRevision: number): Promise<EditorDocument<T>> {
  const response = await fetch(`${base}/documents/${kind}/${encodeURIComponent(id)}?workspaceId=${encodeURIComponent(workspaceId)}`, {
    method: 'PUT', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify({ workspaceId, document, expectedRevision }),
  });
  const body = await response.json();
  if (!response.ok) throw Object.assign(new Error(body.error?.message ?? `文档保存失败 (${response.status})`), { status: response.status });
  return body;
}
export const shaderAction = <T>(workspaceId: string, action: string, args: Record<string, unknown>) => apiPost<T>(`${base}/shader/${action}`, { workspaceId, arguments: args });
