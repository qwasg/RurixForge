import { cleanup, fireEvent, render, screen, within } from '@testing-library/react';
import { afterEach, expect, it, vi } from 'vitest';
import EditorChanges from '@/components/editor/EditorChanges';
import { useWorkspaceStore } from '@/lib/workspaceStore';
import { useEditorStore } from '@/lib/editorStore';

afterEach(() => { cleanup(); vi.restoreAllMocks(); vi.unstubAllGlobals(); });

it('renders the actual items response and coordinates undo and redo by change-set ID', async () => {
  useWorkspaceStore.setState({ activeWorkspaceId: 'history-workspace' });
  vi.spyOn(useEditorStore.getState(), 'refreshSummary').mockResolvedValue();
  let status = 'applied';
  const requests: Array<{ url: string; body: unknown }> = [];
  vi.stubGlobal('fetch', vi.fn(async (url, init) => {
    if (init?.method === 'POST') {
      requests.push({ url: String(url), body: JSON.parse(init.body) }); status = String(url).endsWith('/undo') ? 'undone' : 'applied';
      return { ok: true, json: async () => ({ status }) };
    }
    return { ok: true, json: async () => ({ items: [{ id: 'change-123', status, result: { sceneGuid: 'scene' } }] }) };
  }));
  render(<EditorChanges />); fireEvent.click(screen.getByRole('button', { name: '协作变更记录' }));
  const dialog = within(screen.getByRole('dialog'));
  fireEvent.click(await dialog.findByRole('button', { name: '撤销' }));
  fireEvent.click(await dialog.findByRole('button', { name: '重做' }));
  await dialog.findByText('已应用');
  expect(requests).toEqual(['undo', 'redo'].map((action) => ({ url: `/api/forge/editor/${action}`, body: { workspaceId: 'history-workspace', changeSetId: 'change-123' } })));
});
