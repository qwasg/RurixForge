import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { bindEditorDocuments, flushEditorDocuments, useEditorDocumentSync } from '@/lib/editorDocuments';
import { useDesignBoardStore } from '@/lib/designBoardStore';
import { useStudioStore } from '@/lib/studioStore';

const initialBoard = useDesignBoardStore.getState(), initialStudio = useStudioStore.getState();
let dispose: (() => void) | undefined;
beforeEach(() => { localStorage.clear(); useDesignBoardStore.setState(initialBoard, true); useStudioStore.setState(initialStudio, true); useEditorDocumentSync.setState({ status: {} }); });
afterEach(() => { dispose?.(); dispose = undefined; vi.unstubAllGlobals(); });

describe('canonical editor documents', () => {
  it('finishes a detached workspace draft and claims legacy ownership even when its initial read is still pending', async () => {
    useDesignBoardStore.getState().addNode(initialBoard.kinds[0].id, [0, 0]);
    const releases: Array<() => void> = [];
    const writes: Array<{ workspaceId: string; document: { nodes: unknown[] } }> = [];
    vi.stubGlobal('fetch', vi.fn(async (_url: string, init?: RequestInit) => {
      if (init?.method === 'PUT') { const body = JSON.parse(init.body as string); writes.push(body); return { ok: true, json: async () => ({ document: body.document, revision: 1 }) }; }
      return new Promise((resolve) => releases.push(() => resolve({ ok: true, json: async () => ({ document: null, revision: 0 }) })));
    }));
    localStorage.setItem('forge:activeWorkspace', 'first'); const old = bindEditorDocuments('first'); old();
    localStorage.setItem('forge:activeWorkspace', 'second'); dispose = bindEditorDocuments('second');
    releases.forEach((release) => release()); await flushEditorDocuments();
    await vi.waitFor(() => expect(writes.some((w) => w.workspaceId === 'first' && w.document.nodes.length === 1)).toBe(true));
    expect(writes.filter((w) => w.workspaceId === 'second').every((w) => w.document.nodes.length === 0)).toBe(true);
    expect(useDesignBoardStore.getState().nodes).toHaveLength(0);
  });
  it('assigns a legacy board only to the first workspace and preserves its original JSON backup', async () => {
    useDesignBoardStore.getState().addNode(initialBoard.kinds[0].id, [0, 0]);
    const original = localStorage.getItem('forge:designBoard');
    const writes: Array<{ workspaceId: string; document: { nodes: unknown[]; bindings?: unknown } }> = [];
    vi.stubGlobal('fetch', vi.fn(async (_url: string, init?: RequestInit) => {
      if (init?.method === 'PUT') { const body = JSON.parse(init.body as string); writes.push(body); return { ok: true, json: async () => ({ document: body.document, revision: 1 }) }; }
      return { ok: true, json: async () => ({ document: null, revision: 0 }) };
    }));
    localStorage.setItem('forge:activeWorkspace', 'first'); dispose = bindEditorDocuments('first'); await flushEditorDocuments(); dispose();
    localStorage.setItem('forge:activeWorkspace', 'second'); dispose = bindEditorDocuments('second'); await flushEditorDocuments();
    expect(writes.find((w) => w.workspaceId === 'first')?.document.nodes).toHaveLength(1);
    expect(writes.filter((w) => w.workspaceId === 'second').every((w) => w.document.nodes.length === 0)).toBe(true);
    expect(localStorage.getItem('forge:editor-legacy-blueprint:backup')).toBe(original);
  });
  it('migrates legacy board once, preserves server binding metadata, and writes with revision preconditions', async () => {
    useDesignBoardStore.getState().addNode(initialBoard.kinds[0].id, [0, 0]);
    const node = useDesignBoardStore.getState().nodes[0];
    const writes: Array<{ workspaceId: string; document: Record<string, unknown>; expectedRevision: number }> = [];
    vi.stubGlobal('fetch', vi.fn(async (url: string, init?: RequestInit) => {
      if (init?.method === 'PUT') { const body = JSON.parse(init.body as string); writes.push(body); return { ok: true, json: async () => ({ document: body.document, revision: body.expectedRevision + 1 }) }; }
      return { ok: true, json: async () => ({ document: url.includes('/blueprint/') ? { version: 3, nodes: [node], edges: [], bindings: { [node.id]: { sceneGuid: 'scene', entityId: 7, changeSetId: 'set' } }, futureSchema: { retained: true } } : { nodes: [], edges: [] }, revision: 5 }) };
    }));
    dispose = bindEditorDocuments('ws'); await flushEditorDocuments();
    expect(useDesignBoardStore.getState().bindings[node.id].entityId).toBe(7);
    useDesignBoardStore.getState().renameNode(node.id, 'Updated'); await flushEditorDocuments();
    const saved = writes.find((w) => w.document.futureSchema);
    expect(saved).toMatchObject({ workspaceId: 'ws', expectedRevision: 5, document: { bindings: { [node.id]: { entityId: 7 } }, futureSchema: { retained: true } } });
    expect(localStorage.getItem('forge:editor-migrated:blueprint')).toBe('ws');
  });

  it('keeps a dirty local draft when the remote revision conflicts', async () => {
    useDesignBoardStore.getState().addNode(initialBoard.kinds[0].id, [0, 0]);
    const nodes = useDesignBoardStore.getState().nodes;
    localStorage.setItem('forge:editor-document:ws:blueprint:main', JSON.stringify({ document: { nodes, edges: [] }, dirty: true, baseRevision: 2 }));
    vi.stubGlobal('fetch', vi.fn(async (_url: string, init?: RequestInit) => init?.method === 'PUT'
      ? { ok: false, status: 409, json: async () => ({ error: { message: 'revision conflict' } }) }
      : { ok: true, json: async () => ({ document: { nodes: [], edges: [] }, revision: 9 }) }));
    dispose = bindEditorDocuments('ws'); await expect(flushEditorDocuments()).rejects.toThrow('revision conflict');
    expect(useDesignBoardStore.getState().nodes).toEqual(nodes);
    expect(JSON.parse(localStorage.getItem('forge:editor-document:ws:blueprint:main')!).dirty).toBe(true);
    expect(useEditorDocumentSync.getState().status['ws:blueprint:main'].error).toBe('revision conflict');
  });
});
