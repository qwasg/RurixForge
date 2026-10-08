import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import Sidebar from '@/components/shell/Sidebar';
import WorkspacePicker from '@/components/shell/WorkspacePicker';
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

  it('keeps the 2D form and draft after initialization fails, then allows retry', async () => {
    let attempts = 0;
    const workspaceRequests: unknown[] = [];
    const draft = { name: '重试项目', root: 'D:/games/retry', mode: '2d', backend: 'godot' };
    vi.stubGlobal('fetch', vi.fn(async (url: unknown, init?: { body?: string }) => {
      if (String(url) === '/api/forge/project/init') {
        expect(JSON.parse(init?.body ?? '{}')).toEqual(draft);
        attempts += 1;
        return attempts === 1
          ? { ok: false, status: 500, json: async () => ({ error: { code: 'IO_ERR', message: '创建项目根失败:拒绝访问' } }) } as Response
          : { ok: true, status: 200, json: async () => ({ project: {} }) } as Response;
      }
      workspaceRequests.push(JSON.parse(init?.body ?? '{}'));
      return { ok: true, status: 200, json: async () => ({ workspace: {
        id: 'ws_retry', name: draft.name, root: draft.root, createdAt: '', updatedAt: '',
      } }) } as Response;
    }));

    render(<WorkspacePicker />);
    fireEvent.click(screen.getByTestId('sidebar-new-workspace'));
    expect(screen.getByTestId('sidebar-workspace-create')).toBeDisabled();
    fireEvent.change(screen.getByTestId('sidebar-workspace-name'), { target: { value: draft.name } });
    expect(screen.getByTestId('sidebar-workspace-create')).toBeEnabled();
    fireEvent.change(screen.getByTestId('sidebar-workspace-root'), { target: { value: draft.root } });
    fireEvent.click(screen.getByTestId('sidebar-workspace-create'));

    await waitFor(() => expect(screen.getByRole('alert')).toHaveTextContent('拒绝访问'));
    expect(screen.getByTestId('sidebar-workspace-name')).toHaveValue(draft.name);
    expect(screen.getByTestId('sidebar-workspace-root')).toHaveValue(draft.root);
    expect(screen.getByTestId('workspace-gametype-2d')).toHaveAttribute('aria-checked', 'true');
    expect(workspaceRequests).toEqual([]);
    expect(useWorkspaceStore.getState().activeWorkspaceId).toBeNull();
    fireEvent.click(screen.getByTestId('sidebar-workspace-create'));
    await waitFor(() => expect(useWorkspaceStore.getState().activeWorkspaceId).toBe('ws_retry'));
    expect(workspaceRequests).toEqual([{ name: draft.name, root: draft.root, createRoot: true }]);
    expect(screen.queryByTestId('sidebar-workspace-name')).not.toBeInTheDocument();
  });

  it('creates a 2D workspace without a path and registers the directory returned by initialization', async () => {
    const requests: Array<{ url: string; body: unknown }> = [];
    const generatedRoot = 'D:/RurixForge/data/workspaces/project_generated';
    vi.stubGlobal('fetch', vi.fn(async (url: unknown, init?: { body?: string }) => {
      requests.push({ url: String(url), body: JSON.parse(init?.body ?? '{}') });
      if (String(url) === '/api/forge/project/init') {
        return { ok: true, status: 200, json: async () => ({ project: { root: generatedRoot } }) } as Response;
      }
      return { ok: true, status: 200, json: async () => ({ workspace: {
        id: 'ws_default', name: '新游戏', root: generatedRoot, createdAt: '', updatedAt: '',
      } }) } as Response;
    }));
    render(<WorkspacePicker />);
    fireEvent.click(screen.getByTestId('sidebar-new-workspace'));
    fireEvent.change(screen.getByTestId('sidebar-workspace-name'), { target: { value: '新游戏' } });
    fireEvent.click(screen.getByTestId('sidebar-workspace-create'));
    await waitFor(() => expect(useWorkspaceStore.getState().activeWorkspaceId).toBe('ws_default'));
    expect(requests).toEqual([
      { url: '/api/forge/project/init', body: { root: '', name: '新游戏', mode: '2d', backend: 'godot' } },
      { url: '/api/forge/workspaces', body: { root: generatedRoot, name: '新游戏', createRoot: true } },
    ]);
  });

  it('creates a directory-only workspace without a path', async () => {
    const fetchMock = vi.fn(async (_url: unknown, _init?: { body?: string }) => ({
      ok: true, status: 200, json: async () => ({ workspace: {
        id: 'ws_plain_default', name: '目录', root: 'D:/data/workspaces/project_plain', createdAt: '', updatedAt: '',
      } }),
    } as Response));
    vi.stubGlobal('fetch', fetchMock);
    render(<WorkspacePicker />);
    fireEvent.click(screen.getByTestId('sidebar-new-workspace'));
    fireEvent.change(screen.getByTestId('sidebar-workspace-name'), { target: { value: '目录' } });
    fireEvent.click(screen.getByTestId('workspace-gametype-none'));
    fireEvent.click(screen.getByTestId('sidebar-workspace-create'));
    await waitFor(() => expect(useWorkspaceStore.getState().activeWorkspaceId).toBe('ws_plain_default'));
    expect(fetchMock).toHaveBeenCalledOnce();
    expect(fetchMock.mock.calls[0]?.[0]).toBe('/api/forge/workspaces');
    expect(JSON.parse(fetchMock.mock.calls[0]?.[1]?.body ?? '{}')).toEqual({
      name: '目录', root: '', createRoot: true,
    });
  });

  it('keeps the form visible and blocks duplicate submissions while initialization is pending', async () => {
    let resolveInit!: (response: Response) => void;
    const pendingInit = new Promise<Response>((resolve) => { resolveInit = resolve; });
    const requests: string[] = [];
    vi.stubGlobal('fetch', vi.fn(async (url: unknown) => {
      requests.push(String(url));
      if (String(url) === '/api/forge/project/init') return pendingInit;
      return { ok: true, status: 200, json: async () => ({ workspace: {
        id: 'ws_pending', name: 'Pending', root: 'D:/games/pending', createdAt: '', updatedAt: '',
      } }) } as Response;
    }));

    render(<WorkspacePicker />);
    fireEvent.click(screen.getByTestId('sidebar-new-workspace'));
    fireEvent.change(screen.getByTestId('sidebar-workspace-name'), { target: { value: 'Pending' } });
    fireEvent.change(screen.getByTestId('sidebar-workspace-root'), { target: { value: 'D:/games/pending' } });
    fireEvent.click(screen.getByTestId('sidebar-workspace-create'));
    expect(screen.getByTestId('sidebar-workspace-create')).toBeDisabled();
    expect(screen.getByTestId('sidebar-workspace-create')).toHaveTextContent('创建中');
    expect(screen.getByTestId('sidebar-workspace-name')).toHaveValue('Pending');
    expect(screen.getByTestId('sidebar-workspace-root')).toBeDisabled();
    fireEvent.click(screen.getByTestId('sidebar-workspace-create'));
    fireEvent.keyDown(screen.getByTestId('sidebar-workspace-root'), { key: 'Enter' });
    expect(requests).toEqual(['/api/forge/project/init']);
    resolveInit({ ok: true, status: 200, json: async () => ({ project: {} }) } as Response);
    await waitFor(() => expect(useWorkspaceStore.getState().activeWorkspaceId).toBe('ws_pending'));
    expect(requests).toEqual(['/api/forge/project/init', '/api/forge/workspaces']);
  });

  it('can retry registration of an initialized project without losing its draft', async () => {
    let attempts = 0;
    const initRoots: string[] = [];
    vi.stubGlobal('fetch', vi.fn(async (url: unknown, init?: { body?: string }) => {
      if (String(url) === '/api/forge/project/init') {
        initRoots.push((JSON.parse(init?.body ?? '{}') as { root: string }).root);
        if (initRoots.length === 1) {
          return { ok: true, status: 200, json: async () => ({ project: { root: 'D:/games/existing' } }) } as Response;
        }
        return { ok: false, status: 409, json: async () => ({ error: {
          code: 'PROJECT_ALREADY_INITIALIZED', message: '项目已初始化',
        } }) } as Response;
      }
      attempts += 1;
      return attempts === 1
        ? { ok: false, status: 400, json: async () => ({ error: { code: 'INVALID_ROOT', message: '目录暂不可用' } }) } as Response
        : { ok: true, status: 200, json: async () => ({ workspace: {
          id: 'ws_registered', name: 'Existing', root: 'D:/games/existing', createdAt: '', updatedAt: '',
        } }) } as Response;
    }));
    render(<WorkspacePicker />);
    fireEvent.click(screen.getByTestId('sidebar-new-workspace'));
    fireEvent.change(screen.getByTestId('sidebar-workspace-name'), { target: { value: 'Existing' } });
    fireEvent.click(screen.getByTestId('sidebar-workspace-create'));
    await waitFor(() => expect(screen.getByRole('alert')).toHaveTextContent('工作区创建失败'));
    expect(screen.getByTestId('sidebar-workspace-root')).toHaveValue('D:/games/existing');
    fireEvent.click(screen.getByTestId('sidebar-workspace-create'));
    await waitFor(() => expect(useWorkspaceStore.getState().activeWorkspaceId).toBe('ws_registered'));
    expect(attempts).toBe(2);
    expect(initRoots).toEqual(['', 'D:/games/existing']);
  });

  it('creates workspace and filters sessions by active workspace', async () => {
    const workspaces: Array<{ id: string; name: string; root: string; createdAt: string; updatedAt: string }> =
      [];
    const initCalls: Array<{ root: string; name: string; mode: string; backend: string }> = [];
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
                  agentEngine: 'local',
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
                  agentEngine: 'local',
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
        // F-GAME-3:项目选型脚手架(默认 2D;记录调用供断言)
        if (u === '/api/forge/project/init' && method === 'POST') {
          const body = JSON.parse(init?.body ?? '{}') as { root: string; name: string; mode: string; backend: string };
          initCalls.push(body);
          return {
            ok: true,
            status: 200,
            json: async () => ({
              project: { root: body.root, name: body.name, mode: body.mode, entryScene: 'Content/Scenes/Main.rxscene' },
            }),
          } as Response;
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
    expect(screen.getByTestId('workspace-backend-2d')).toHaveTextContent('Godot（默认）');
    fireEvent.change(screen.getByTestId('sidebar-workspace-name'), { target: { value: '项目A' } });
    fireEvent.change(screen.getByTestId('sidebar-workspace-root'), {
      target: { value: 'D:/proj-a' },
    });
    fireEvent.click(screen.getByRole('button', { name: '创建' }));

    await waitFor(() => {
      expect(screen.getAllByTestId('workspace-row-ws_test1')).toHaveLength(1);
    });
    expect(useWorkspaceStore.getState().activeWorkspaceId).toBe('ws_test1');
    // F-GAME-3:默认 2D 选型 → 先 project/init 落定模式,再登记工作区
    expect(initCalls).toEqual([{ root: 'D:/proj-a', name: '项目A', mode: '2d', backend: 'godot' }]);

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

  // F-GAME-3:游戏选型控件——3D 走 project/init mode=3d;仅目录不初始化项目
  it('game type selection: 3d supports rurix and Godot, none skips init', async () => {
    const initCalls: Array<{ root: string; name: string; mode: string; backend: string }> = [];
    let wsCount = 0;
    vi.stubGlobal(
      'fetch',
      vi.fn(async (url: unknown, init?: { method?: string; body?: string }) => {
        const u = String(url);
        const method = init?.method ?? 'GET';
        if (u === '/api/forge/workspaces' && method === 'GET') {
          return { ok: true, status: 200, json: async () => ({ workspaces: [] }) } as Response;
        }
        if (u === '/api/forge/workspaces' && method === 'POST') {
          const body = JSON.parse(init?.body ?? '{}') as { name: string; root: string };
          wsCount += 1;
          const ws = {
            id: `ws_${wsCount}`,
            name: body.name,
            root: body.root,
            createdAt: '2026-08-31T00:00:00Z',
            updatedAt: '2026-08-31T00:00:00Z',
          };
          return { ok: true, status: 200, json: async () => ({ workspace: ws }) } as Response;
        }
        if (u === '/api/forge/project/init' && method === 'POST') {
          initCalls.push(JSON.parse(init?.body ?? '{}') as { root: string; name: string; mode: string; backend: string });
          return { ok: true, status: 200, json: async () => ({ project: {} }) } as Response;
        }
        if (u === '/api/forge/sessions') {
          return { ok: true, status: 200, json: async () => ({ sessions: [] }) } as Response;
        }
        if (u === '/api/forge/chat-folders') {
          return { ok: true, status: 200, json: async () => ({ folders: [] }) } as Response;
        }
        throw new Error(`未 mock: ${method} ${u}`);
      }),
    );

    render(<Sidebar />);
    // 选 3D:init 收 mode=3d
    fireEvent.click(screen.getByTestId('sidebar-new-workspace'));
    fireEvent.change(screen.getByTestId('sidebar-workspace-name'), { target: { value: 'P3' } });
    fireEvent.change(screen.getByTestId('sidebar-workspace-root'), { target: { value: 'D:/p3' } });
    fireEvent.click(screen.getByTestId('workspace-gametype-3d'));
    expect(screen.getByTestId('workspace-backend-rurix')).toHaveAttribute('aria-checked', 'true');
    fireEvent.click(screen.getByTestId('sidebar-workspace-create'));
    await waitFor(() => expect(wsCount).toBe(1));
    expect(initCalls).toEqual([{ root: 'D:/p3', name: 'P3', mode: '3d', backend: 'rurix' }]);

    // 3D 也可显式选择 Godot，选择随请求落盘。
    fireEvent.click(screen.getByTestId('sidebar-new-workspace'));
    fireEvent.change(screen.getByTestId('sidebar-workspace-name'), { target: { value: 'P3 Godot' } });
    fireEvent.change(screen.getByTestId('sidebar-workspace-root'), { target: { value: 'D:/p3-godot' } });
    fireEvent.click(screen.getByTestId('workspace-gametype-3d'));
    fireEvent.click(screen.getByTestId('workspace-backend-godot'));
    expect(screen.getByTestId('workspace-backend-godot')).toHaveAttribute('aria-checked', 'true');
    fireEvent.click(screen.getByTestId('sidebar-workspace-create'));
    await waitFor(() => expect(wsCount).toBe(2));
    expect(initCalls[1]).toEqual({ root: 'D:/p3-godot', name: 'P3 Godot', mode: '3d', backend: 'godot' });

    // 选「仅目录」:不调 init,直接登记
    fireEvent.click(screen.getByTestId('sidebar-new-workspace'));
    fireEvent.change(screen.getByTestId('sidebar-workspace-name'), { target: { value: 'Plain' } });
    fireEvent.change(screen.getByTestId('sidebar-workspace-root'), { target: { value: 'D:/plain' } });
    fireEvent.click(screen.getByTestId('workspace-gametype-none'));
    fireEvent.click(screen.getByTestId('sidebar-workspace-create'));
    await waitFor(() => expect(wsCount).toBe(3));
    expect(initCalls).toHaveLength(2);
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
    // D-040:无后端语义的「云端」行下线;浏览器环境无 preload → 本机目录如实禁用
    expect(screen.queryByTestId('workspace-cloud')).toBeNull();
    expect(screen.getByTestId('workspace-pick-folder')).toBeDisabled();

    fireEvent.change(screen.getByTestId('workspace-picker-search'), { target: { value: '二次元' } });
    expect(screen.queryByTestId('workspace-row-ws_a')).not.toBeInTheDocument();
    expect(screen.getByTestId('workspace-row-ws_b')).toBeInTheDocument();

    fireEvent.click(screen.getByTestId('workspace-picker-toggle'));
    expect(screen.queryByTestId('workspace-picker-panel')).not.toBeInTheDocument();
  });
});
