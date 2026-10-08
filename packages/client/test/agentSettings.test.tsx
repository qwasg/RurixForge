import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import AgentPage from '@/components/settings/AgentPage';
import { useSessionStore } from '@/lib/sessionStore';
import { useToastStore } from '@/lib/toastStore';
import { useChatStore } from '@/lib/chatStore';
import type { AgentConfigFace, AgentPermissionFace } from '@/lib/forgeApi';

const sessions = useSessionStore.getState();
const chat = useChatStore.getState();
const toasts = useToastStore.getState();
const permissionOptions: AgentPermissionFace['options'] = [
  { id: 'plan', label: '只读', description: '后台只读规则' },
  { id: 'auto', label: '写入需批准', description: '后台写入审批规则' },
  { id: 'bypass', label: '自动执行', description: '后台自动执行规则' },
];

function response(data: unknown, status = 200): Response {
  return { ok: status < 400, status, json: async () => data } as Response;
}

function backend() {
  const face: AgentConfigFace = {
    config: { exploreModel: '', defaultPermissionMode: 'bypass' },
    options: {
      exploreModels: [
        { id: 'cloud:research', label: 'Research', group: '云端', availability: 'available' },
        { id: 'deepseek-chat', label: 'DeepSeek', group: 'DeepSeek', availability: 'needs-key' },
      ],
      permissionModes: permissionOptions,
    },
  };
  const permission: AgentPermissionFace = { mode: 'plan', options: permissionOptions };
  const writes: Array<{ path: string; body: unknown }> = [];
  let rejectWrites = false;
  const fetch = vi.fn(async (url: unknown, init?: RequestInit) => {
    const path = String(url);
    if (init?.method === 'PATCH') {
      const body = JSON.parse(String(init.body)) as Record<string, unknown>;
      writes.push({ path, body });
      if (rejectWrites) return response({ error: { code: 'SAVE_FAILED', message: '磁盘写入失败' } }, 500);
      if (path === '/api/forge/agent/config') Object.assign(face.config, body);
      else Object.assign(permission, body);
    }
    if (path === '/api/forge/agent/config') return response(structuredClone(face));
    if (path.endsWith('/permission')) return response(structuredClone(permission));
    throw new Error(`Unexpected request: ${path}`);
  });
  vi.stubGlobal('fetch', fetch);
  return { face, permission, writes, fetch, failWrites: () => { rejectWrites = true; } };
}

beforeEach(() => {
  useSessionStore.setState({ ...sessions, activeSessionId: null }, true);
  useChatStore.setState({ ...chat, pendingPermission: null }, true);
  useToastStore.setState(toasts, true);
});
afterEach(() => {
  cleanup();
  vi.unstubAllGlobals();
});

describe('Agent defaults and execution permissions', () => {
  it('uses backend model choices, saves Explore selection, and restores it on reopening', async () => {
    const api = backend();
    const page = render(<AgentPage />);
    await waitFor(() => expect(screen.getByTestId('agent-explore-model-trigger')).toBeEnabled());
    fireEvent.click(screen.getByTestId('agent-explore-model-trigger'));
    expect(screen.getByTestId('agent-explore-model-item-deepseek-chat')).toBeDisabled();
    fireEvent.click(screen.getByTestId('agent-explore-model-item-cloud:research'));
    await waitFor(() => expect(screen.getByTestId('agent-explore-model-trigger')).toHaveTextContent('Research'));
    expect(api.writes).toEqual([{ path: '/api/forge/agent/config', body: { exploreModel: 'cloud:research' } }]);
    page.unmount();
    render(<AgentPage />);
    await waitFor(() => expect(screen.getByTestId('agent-explore-model-trigger')).toHaveTextContent('Research'));
  });

  it('can set default permissions before creating a session without changing session permissions', async () => {
    const api = backend();
    render(<AgentPage />);
    await waitFor(() => expect(screen.getByTestId('agent-default-permission-trigger')).toBeEnabled());
    fireEvent.click(screen.getByTestId('agent-default-permission-trigger'));
    fireEvent.click(screen.getByTestId('agent-default-permission-item-auto'));
    await waitFor(() => expect(screen.getByTestId('agent-default-permission-trigger')).toHaveTextContent('写入需批准'));
    expect(api.writes).toEqual([{ path: '/api/forge/agent/config', body: { defaultPermissionMode: 'auto' } }]);
    expect(api.fetch.mock.calls.every(([url]) => !String(url).includes('/sessions/'))).toBe(true);
  });


  it('keeps the previous defaults when the backend cannot save, even with an active session', async () => {
    const api = backend();
    api.failWrites();
    useSessionStore.setState({ activeSessionId: 'a' });
    render(<AgentPage />);
    await waitFor(() => expect(screen.getByTestId('agent-explore-model-trigger')).toBeEnabled());
    fireEvent.click(screen.getByTestId('agent-explore-model-trigger'));
    fireEvent.click(screen.getByTestId('agent-explore-model-item-cloud:research'));
    await waitFor(() => expect(api.writes).toHaveLength(1));
    await waitFor(() => expect(screen.getByTestId('agent-explore-model-trigger')).toBeEnabled());
    expect(screen.getByTestId('agent-explore-model-trigger')).toHaveTextContent('自动');
    expect(useToastStore.getState().items.length).toBeGreaterThan(0);
    expect(api.fetch.mock.calls.every(([url]) => !String(url).includes('/sessions/'))).toBe(true);
  });


  it('shows an unavailable saved model and lets the user reset it to automatic', async () => {
    const api = backend();
    api.face.config.exploreModel = 'cloud:removed';
    render(<AgentPage />);
    await waitFor(() => expect(screen.getByTestId('agent-explore-model-trigger')).toHaveTextContent('cloud:removed（当前不可用）'));
    fireEvent.click(screen.getByTestId('agent-explore-model-trigger'));
    fireEvent.click(screen.getByTestId('agent-explore-model-item-'));
    await waitFor(() => expect(screen.getByTestId('agent-explore-model-trigger')).toHaveTextContent('自动'));
    expect(api.writes[0]?.body).toEqual({ exploreModel: '' });
  });
});
