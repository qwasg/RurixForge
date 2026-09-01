import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import Sidebar from '@/components/shell/Sidebar';
import { useSessionStore } from '@/lib/sessionStore';
import { useWorkspaceStore } from '@/lib/workspaceStore';

const initialSession = useSessionStore.getState();
const initialWorkspace = useWorkspaceStore.getState();

describe('Sidebar workspaces', () => {
  beforeEach(() => {
    useSessionStore.setState(initialSession, true);
    useWorkspaceStore.setState(
      { ...initialWorkspace, workspaces: [], activeWorkspaceId: null, recentIds: [] },
      true,
    );
    localStorage.removeItem('forge:activeWorkspace');
    localStorage.removeItem('forge:recentWorkspaces');
  });

  afterEach(() => {
    cleanup();
    vi.unstubAllGlobals();
  });

  it('creates workspace and filters sessions by active workspace', async () => {
    const workspaces: Array<{ id: string; name: string; root: string; createdAt: string; updatedAt: string }> =
      [];
    vi.stubGlobal(
      'fetch',
      vi.fn(async (url: unknown, init?: { method?: string; body?: string }) => {
        const u = String(url);
        const method = init?.method ?? 'GET';
        if (u === '/api/forge/workspaces' && method === 'GET') {
          return { ok: true, status: 200, json: async () => ({ workspaces }) } as Response;
        }
        if (u === '/api/forge/workspaces' && method === 'POST') {
          const body = JSON.parse(init?.body ?? '{}') as { name: string; root: string };
          const ws = {
            id: 'ws_test1',
            name: body.name,
            root: body.root,
            createdAt: '2026-08-24T00:00:00Z',
            updatedAt: '2026-08-24T00:00:00Z',
          };
          workspaces.push(ws);
          return { ok: true, status: 200, json: async () => ({ workspace: ws }) } as Response;
        }
        if (u === '/api/forge/sessions') {
          return {
            ok: true,
            status: 200,
            json: async () => ({
              sessions: [
                {
                  id: 'sess_a',
                  title: 'A',
                  status: 'idle',
                  agentKind: 'coding',
                  selectedModelId: null,
                  webSearchEnabled: true,
                  activeRunId: null,
                  createdAt: '2026-08-24T00:00:00Z',
                  updatedAt: '2026-08-24T00:00:00Z',
                  pinned: false,
                  titleManuallySet: false,
                  workspaceId: 'ws_test1',
                },
                {
                  id: 'sess_b',
                  title: 'B',
                  status: 'idle',
                  agentKind: 'coding',
                  selectedModelId: null,
                  webSearchEnabled: true,
                  activeRunId: null,
                  createdAt: '2026-08-24T00:00:00Z',
                  updatedAt: '2026-08-24T00:00:00Z',
                  pinned: false,
                  titleManuallySet: false,
                  workspaceId: null,
                },
              ],
            }),
          } as Response;
        }
        if (u === '/api/forge/chat-folders') {
          return { ok: true, status: 200, json: async () => ({ folders: [] }) } as Response;
        }
        throw new Error(`未 mock: ${method} ${u}`);
      }),
    );

    render(<Sidebar />);
    await useSessionStore.getState().loadAll();
    await useWorkspaceStore.getState().loadAll();

    expect(screen.getByTestId('session-row-sess_a')).toBeInTheDocument();
    expect(screen.getByTestId('session-row-sess_b')).toBeInTheDocument();

    // 「+」= 展开选择器并直接落到新建表单(收起态下的快捷入口)
    fireEvent.click(screen.getByTestId('sidebar-new-workspace'));
    fireEvent.change(screen.getByTestId('sidebar-workspace-name'), { target: { value: '项目A' } });
    fireEvent.change(screen.getByTestId('sidebar-workspace-root'), {
      target: { value: 'D:/proj-a' },
    });
    fireEvent.click(screen.getByRole('button', { name: '创建' }));

    await waitFor(() => {
      expect(screen.getAllByTestId('workspace-row-ws_test1')).toHaveLength(1);
    });
    expect(useWorkspaceStore.getState().activeWorkspaceId).toBe('ws_test1');

    // 选中即收起面板,会话列按 workspaceId 过滤
    fireEvent.click(screen.getByTestId('workspace-row-ws_test1'));
    await waitFor(() => {
      expect(screen.queryByTestId('workspace-picker-panel')).not.toBeInTheDocument();
      expect(screen.getByTestId('session-row-sess_a')).toBeInTheDocument();
      expect(screen.queryByTestId('session-row-sess_b')).not.toBeInTheDocument();
    });

    fireEvent.click(screen.getByTestId('workspace-picker-toggle'));
    fireEvent.click(screen.getByTestId('workspace-row-all'));
    await waitFor(() => {
      expect(screen.getByTestId('session-row-sess_b')).toBeInTheDocument();
    });
  });

  it('picker 收起态只留触发条,展开后出搜索/最近/打开三段', async () => {
    useWorkspaceStore.setState({
      workspaces: [
        {
          id: 'ws_a',
          name: '游戏引擎',
          root: 'D:/游戏引擎',
          createdAt: '2026-08-24T00:00:00Z',
          updatedAt: '2026-08-24T00:00:00Z',
        },
        {
          id: 'ws_b',
          name: '二次元直播',
          root: 'D:/二次元直播',
          createdAt: '2026-08-24T00:00:00Z',
          updatedAt: '2026-08-25T00:00:00Z',
        },
      ],
      recentIds: ['ws_a'],
    });

    render(<Sidebar />);

    // 收起态:工作区行不在 DOM 里,侧栏只剩一条 WORKSPACES 触发条
    expect(screen.queryByTestId('workspace-picker-panel')).not.toBeInTheDocument();
    expect(screen.queryByTestId('workspace-row-ws_a')).not.toBeInTheDocument();
    expect(screen.queryByTestId('workspace-row-all')).not.toBeInTheDocument();

    fireEvent.click(screen.getByTestId('workspace-picker-toggle'));
    expect(screen.getByTestId('workspace-picker-panel')).toBeInTheDocument();
    // 最近序:recentIds 在前,其余按 updatedAt 兜底
    const rows = screen
      .getAllByTestId(/^workspace-row-ws_/)
      .map((el) => el.getAttribute('data-testid'));
    expect(rows).toEqual(['workspace-row-ws_a', 'workspace-row-ws_b']);
    expect(screen.getByTestId('workspace-row-all')).toBeInTheDocument();
    // 云端无后端语义 → 恒禁用;浏览器环境无 preload → 本机目录同样禁用
    expect(screen.getByTestId('workspace-cloud')).toBeDisabled();
    expect(screen.getByTestId('workspace-pick-folder')).toBeDisabled();

    fireEvent.change(screen.getByTestId('workspace-picker-search'), { target: { value: '二次元' } });
    expect(screen.queryByTestId('workspace-row-ws_a')).not.toBeInTheDocument();
    expect(screen.getByTestId('workspace-row-ws_b')).toBeInTheDocument();

    fireEvent.click(screen.getByTestId('workspace-picker-toggle'));
    expect(screen.queryByTestId('workspace-picker-panel')).not.toBeInTheDocument();
  });
});
