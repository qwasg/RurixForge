import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { waitFor } from '@testing-library/react';
import { bindEditorGraphDocuments } from '@/lib/editorGraphDocuments';
import { flushEditorDocuments } from '@/lib/editorDocumentState';
import { newShaderGraph, useShaderGraphStore } from '@/lib/shaderGraphStore';
import { editorReference } from '@/lib/editorReferences';
import { useGraphStore } from '@/lib/graphStore';
import type { ShaderGraphDoc } from '@forge/protocol';

const shaderInitial = useShaderGraphStore.getState(), logicInitial = useGraphStore.getState();
let dispose: (() => void) | undefined;
beforeEach(() => { localStorage.clear(); localStorage.setItem('forge:activeWorkspace', 'ws'); useShaderGraphStore.setState({ ...shaderInitial, workspaceId: 'ws', graph: newShaderGraph(), dirty: true }, true); useGraphStore.setState(logicInitial, true); });
afterEach(() => { dispose?.(); dispose = undefined; vi.unstubAllGlobals(); });

describe('agent-visible graph drafts', () => {
  it('syncs an unsaved graph by graph id and applies a later agent draft without publishing an asset', async () => {
    let document: ShaderGraphDoc | null = null, revision = 0;
    const writes: Array<{ expectedRevision: number; document: ShaderGraphDoc }> = [];
    vi.stubGlobal('fetch', vi.fn(async (_url: string, init?: RequestInit) => {
      if (init?.method === 'PUT') { const body = JSON.parse(init.body as string); writes.push(body); expect(body.expectedRevision).toBe(revision); document = body.document; revision++; }
      return { ok: true, json: async () => ({ document, revision }) };
    }));
    dispose = bindEditorGraphDocuments('ws'); await flushEditorDocuments();
    expect(writes).toHaveLength(1); const id = useShaderGraphStore.getState().graph.id;
    const ref = editorReference('shaderGraph', { resourceId: id });
    expect(ref).toMatchObject({ resourceId: id, revision: 1, selection: { dirty: true } }); expect(ref.sceneGuid).toBeUndefined();
    document = { ...useShaderGraphStore.getState().graph, name: 'Agent adjusted' }; revision++;
    window.dispatchEvent(new Event('forge:editor-document:shaderGraph'));
    await waitFor(() => expect(useShaderGraphStore.getState().graph.name).toBe('Agent adjusted'));
    expect(useShaderGraphStore.getState().path).toBe(''); expect(writes).toHaveLength(1);
    useShaderGraphStore.getState().change((graph) => ({ ...graph, name: 'Human refined' })); await flushEditorDocuments();
    expect(writes[1].expectedRevision).toBe(2); expect(writes[1].document.name).toBe('Human refined');
  });
});
