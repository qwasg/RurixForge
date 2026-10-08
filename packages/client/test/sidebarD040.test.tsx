import { act, cleanup, fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import Sidebar from '@/components/shell/Sidebar';
import { useOverlayStore } from '@/lib/overlayStore';
import { useSessionSeen } from '@/lib/sessionSeen';
import { useSessionStore, type ForgeSession } from '@/lib/sessionStore';
import { useSettingsStore } from '@/lib/settingsStore';
import { useSystemStore } from '@/lib/systemStore';
import { useWorkspaceStore } from '@/lib/workspaceStore';
import { mockForgeBackend } from './forgeMock';

/** D-040 侧栏:按工作区分组 / 未读点与运行点 / 文件夹双击重命名 / 真实账户卡。 */

const initial = {
  sessions: useSessionStore.getState(),
  workspace: useWorkspaceStore.getState(),
  system: useSystemStore.getState(),
  overlay: useOverlayStore.getState(),
};

function sess(id: string, patch: Partial<ForgeSession> = {}): ForgeSession {
  return {
    id,
    title: id.toUpperCase(),
    status: 'idle',
    agentKind: 'coding',
    agentEngine: 'local',
    selectedModelId: null,
    thinkingEnabled: false,
    reasoningEffort: null,
    contextOptionId: null,
    webSearchEnabled: false,
    activeRunId: null,
    createdAt: '2026-09-20T00:00:00Z',
    updatedAt: '2026-09-20T00:00:00Z',
    pinned: false,
    titleManuallySet: false,
    ...patch,
  };
}

const WORKSPACES = [
  { id: 'ws_game', name: 'Code Sentinels', root: '\\\\?\\D:\\proj\\cs', createdAt: '', updatedAt: '' },
  { id: 'ws_eng', name: '引擎', root: 'D:/eng', createdAt: '', updatedAt: '' },
];

beforeEach(() => {
  useSessionStore.setState(initial.sessions, true);
  useWorkspaceStore.setState({ ...initial.workspace, workspaces: WORKSPACES, activeWorkspaceId: null }, true);
  useSystemStore.setState(initial.system, true);
  useOverlayStore.setState(initial.overlay, true);
  localStorage.removeItem('forge:sessionSeen');
  useSessionSeen.setState({ seen: {} });
  vi.stubGlobal(
    'fetch',
    mockForgeBackend({}, {
      '/api/forge/health': { status: 'ok', version: '0.1.0', user: { name: 'wcj20' } },
      '/api/forge/design-snapshot': {
        sessions: [],
        agents: { engines: [{ id: 'codex', authMode: 'chatgpt', planType: 'Plus' }] },
      },
      '/api/forge/chat-folders/': (init?: { body?: string }) => ({
        folder: { id: 'f1', name: JSON.parse(init?.body ?? '{}').name, createdAt: '', updatedAt: '' },
      }),
    }),
  );
});

afterEach(() => {
  cleanup();
  vi.unstubAllGlobals();
});

describe('<Sidebar /> D-040', () => {
  it('全部会话:未归档会话按工作区分组,未绑定的归「默认工作区」;选定工作区后只列「最近」', () => {
    useSessionStore.setState({
      sessions: [
        sess('a', { workspaceId: 'ws_game', updatedAt: '2026-09-25T00:00:00Z' }),
        sess('b', { workspaceId: 'ws_eng' }),
        sess('c', { workspaceId: null }),
      ],
    });
    render(<Sidebar />);
    const game = screen.getByTestId('session-group-ws:ws_game');
    expect(game).toHaveTextContent('Code Sentinels');
    expect(game).toHaveAttribute('title', 'D:\\proj\\cs');
    expect(screen.getByTestId('session-group-ws:ws_eng')).toHaveTextContent('引擎');
    expect(screen.getByTestId('session-group-ws:none')).toHaveTextContent('默认工作区');
    // 组按最近活动排序:Code Sentinels(9-25)在最前
    const order = screen.getAllByTestId(/^session-group-ws:/).map((el) => el.getAttribute('data-testid'));
    expect(order[0]).toBe('session-group-ws:ws_game');
    act(() => useWorkspaceStore.getState().setActive('ws_game'));
    expect(screen.getByTestId('session-group-plain')).toHaveTextContent('最近');
    expect(screen.getByTestId('session-row-a')).toBeInTheDocument();
    expect(screen.queryByTestId('session-row-b')).toBeNull();
  });

  it('运行中 = 动态点;后台会话 updatedAt 前进 = 未读点,点开即已读', () => {
    useSessionStore.setState({
      sessions: [sess('run', { activeRunId: 'run_1' }), sess('idle'), sess('bg')],
      activeSessionId: 'idle',
    });
    render(<Sidebar />);
    expect(within(screen.getByTestId('session-row-run')).getByTestId('session-running')).toBeInTheDocument();
    expect(within(screen.getByTestId('session-row-bg')).queryByTestId('session-unread')).toBeNull();
    // bg 在后台出了新结果(快照回填 updatedAt)
    act(() =>
      useSessionStore.setState((st) => ({
        sessions: st.sessions.map((s) => (s.id === 'bg' ? { ...s, updatedAt: '2026-09-26T08:00:00Z' } : s)),
      })),
    );
    const bg = screen.getByTestId('session-row-bg');
    expect(bg).toHaveAttribute('data-unread', '1');
    expect(within(bg).getByTestId('session-unread')).toBeInTheDocument();
    fireEvent.click(bg);
    expect(screen.getByTestId('session-row-bg')).not.toHaveAttribute('data-unread');
  });

  it('文件夹组头双击重命名(PATCH chat-folders)', async () => {
    useSessionStore.setState({
      folders: [{ id: 'f1', name: '旧名', createdAt: '', updatedAt: '' }],
      sessions: [sess('x', { folderId: 'f1' })],
    });
    render(<Sidebar />);
    fireEvent.doubleClick(screen.getByTestId('session-group-folder:f1'));
    const input = screen.getByTestId('folder-rename-f1');
    fireEvent.change(input, { target: { value: '新名' } });
    fireEvent.keyDown(input, { key: 'Enter' });
    await waitFor(() => expect(useSessionStore.getState().folders[0].name).toBe('新名'));
    expect(screen.getByTestId('session-group-folder:f1')).toHaveTextContent('新名');
  });

  it('账户卡:真实用户名首字母 + Codex 套餐;菜单直达 Codex 设置', async () => {
    render(<Sidebar />);
    expect(await screen.findByText('wcj20')).toBeInTheDocument();
    expect(screen.getByTestId('account-avatar')).toHaveTextContent('W');
    await waitFor(() => expect(screen.getByTestId('account-subtitle')).toHaveTextContent('Codex · Plus'));
    fireEvent.click(screen.getByTestId('account-card'));
    fireEvent.click(within(screen.getByTestId('account-menu')).getByText('Codex 账户'));
    expect(useOverlayStore.getState().settings).toBe(true);
    expect(useSettingsStore.getState().page).toBe('codex');
  });

  it('账户菜单「账户」直达设置·账户页并收起菜单(D-046)', async () => {
    render(<Sidebar />);
    expect(await screen.findByText('wcj20')).toBeInTheDocument();
    fireEvent.click(screen.getByTestId('account-card'));
    expect(screen.getByTestId('account-card')).toHaveAttribute('aria-expanded', 'true');
    fireEvent.click(within(screen.getByTestId('account-menu')).getByText('账户'));
    expect(useOverlayStore.getState().settings).toBe(true);
    expect(useSettingsStore.getState().page).toBe('account');
    expect(screen.queryByTestId('account-menu')).not.toBeInTheDocument();
  });

  it('搜索无命中给出明确空态', () => {
    useSessionStore.setState({ sessions: [sess('a')] });
    render(<Sidebar />);
    fireEvent.change(screen.getByTestId('sidebar-search'), { target: { value: 'zzz' } });
    expect(screen.getByText('没有匹配的会话。')).toBeInTheDocument();
  });
});
