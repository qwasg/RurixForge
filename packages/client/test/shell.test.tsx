import { cleanup, fireEvent, render, screen } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import App from '@/App';
import { useOverlayStore } from '@/lib/overlayStore';
import { useSessionStore } from '@/lib/sessionStore';
import { useThemeStore } from '@/lib/themeStore';
import { useWorkbenchStore } from '@/lib/workbenchStore';
import { mockForgeBackend } from './forgeMock';

/**
 * F7 wave.3 壳组件测试:三栏渲染 / 折叠 / clamp / 分隔条拖拽模拟 / 编辑器 tab 嵌入。
 * fetch mock:sessions/folders 空 + health + design-snapshot + 编辑器 MCP 面
 * (EditorView 挂载走真实 action,错误经 run() 收进 lastError,不崩)。
 */

const initialSessions = useSessionStore.getState();
const initialWorkbench = useWorkbenchStore.getState();
const initialTheme = useThemeStore.getState();
const initialOverlay = useOverlayStore.getState();

function stubShellFetch() {
  // 有状态迷你会话后端(wire 对齐 agentd:POST {session} / PATCH {session} / DELETE 空体)
  let seq = 0;
  const sessions: Array<Record<string, unknown>> = [];
  const sessionsHandler = (init?: { body?: string; method?: string }) => {
    const method = init?.method ?? 'GET';
    if (method === 'POST') {
      const body = JSON.parse(init?.body ?? '{}') as { title?: string };
      const ts = new Date().toISOString();
      const s = {
        id: `sess_t${++seq}`,
        title: body.title ?? '',
        status: 'idle',
        agentKind: 'coding',
        selectedModelId: null,
        webSearchEnabled: true,
        activeRunId: null,
        createdAt: ts,
        updatedAt: ts,
        pinned: false,
        titleManuallySet: false,
        folderId: null,
      };
      sessions.unshift(s);
      return { session: s };
    }
    if (method === 'PATCH') {
      const body = JSON.parse(init?.body ?? '{}') as Record<string, unknown>;
      const s = sessions[0];
      if (body.title !== undefined) {
        s.title = body.title;
        s.titleManuallySet = true;
      }
      if (body.pinned !== undefined) s.pinned = body.pinned;
      if ('folderId' in body) s.folderId = body.folderId;
      return { session: s };
    }
    if (method === 'DELETE') {
      sessions.splice(0, sessions.length);
      return null;
    }
    return { sessions };
  };
  vi.stubGlobal(
    'fetch',
    mockForgeBackend(
      {
        entity_list: { entities: [] },
        scene_summary: {
          name: 'Demo',
          entityCount: 1,
          playState: 'edit',
          render: { frames: 1, lastTris: 0, lastNonZeroPixels: 0 },
        },
        play_state: { state: 'edit' },
        host_events: [],
      },
      {
        '/api/forge/sessions': sessionsHandler,
        '/api/forge/chat-folders': { folders: [] },
        '/api/forge/health': { status: 'ok' },
        '/api/forge/design-snapshot': {
          sessions: [],
          activeSession: null,
          events: [],
          todos: [],
          run: null,
          models: {
            models: [
              { id: 'deepseek-chat', label: 'deepseek-chat', provider: 'deepseek', availability: 'unavailable' },
              { id: 'mock', label: 'Mock provider', provider: 'mock', availability: 'available' },
            ],
          },
          latestSeq: 0,
          chatFolders: [],
        },
      },
    ),
  );
}

beforeEach(() => {
  useSessionStore.setState(initialSessions, true);
  useWorkbenchStore.setState(initialWorkbench, true);
  useThemeStore.setState(initialTheme, true);
  useOverlayStore.setState(initialOverlay, true);
  globalThis.localStorage?.clear();
  stubShellFetch();
});

afterEach(() => {
  cleanup();
  vi.unstubAllGlobals();
});

describe('<App /> 新壳', () => {
  it('三栏 + titlebar + statusbar 渲染(默认 moonlit 空态)', async () => {
    render(<App />);
    expect(screen.getByTestId('shell-titlebar')).toBeInTheDocument();
    expect(screen.getByTestId('pane-sessions')).toBeInTheDocument();
    expect(screen.getByTestId('pane-chat')).toBeInTheDocument();
    expect(screen.getByTestId('pane-main')).toBeInTheDocument();
    expect(screen.getByTestId('pane-inspector')).toBeInTheDocument();
    expect(screen.getByTestId('shell-statusbar')).toBeInTheDocument();
    // 三条 9px 分隔条
    expect(screen.getByTestId('pane-divider-sessions')).toBeInTheDocument();
    expect(screen.getByTestId('pane-divider-chat')).toBeInTheDocument();
    expect(screen.getByTestId('pane-divider-inspector')).toBeInTheDocument();
    // 侧栏 New Agent + 空态文案;对话列未选会话空态;主区空态卡
    expect(screen.getByTestId('sidebar-new-agent')).toBeInTheDocument();
    expect(await screen.findByText('暂无会话，可点击「New Agent」创建。')).toBeInTheDocument();
    expect(screen.getByText('选择左侧会话或点击 New Agent')).toBeInTheDocument();
    expect(screen.getByText('个人工作区')).toBeInTheDocument();
    // StatusBar provider 段(design-snapshot:deepseek unavailable → Mock provider)
    expect(await screen.findByText('Mock provider')).toBeInTheDocument();
  });

  it('折叠胶囊:点击折叠/展开 + 持久化 forge:paneSizes', () => {
    render(<App />);
    fireEvent.click(screen.getByTestId('pane-toggle-sessions'));
    expect(useWorkbenchStore.getState().collapsed.sessions).toBe(true);
    expect(screen.queryByTestId('pane-sessions')).not.toBeInTheDocument();
    const raw = globalThis.localStorage?.getItem('forge:paneSizes');
    expect(raw).toBeTruthy();
    expect(JSON.parse(raw as string).collapsed.sessions).toBe(true);
    fireEvent.click(screen.getByTestId('pane-toggle-sessions'));
    expect(useWorkbenchStore.getState().collapsed.sessions).toBe(false);
    expect(screen.getByTestId('pane-sessions')).toBeInTheDocument();
  });

  it('栏宽 clamp(200–360 / 300–560 / 240–420)', () => {
    const st = useWorkbenchStore.getState();
    st.setPaneW('sessions', 9999);
    expect(useWorkbenchStore.getState().paneW.sessions).toBe(360);
    st.setPaneW('sessions', 1);
    expect(useWorkbenchStore.getState().paneW.sessions).toBe(200);
    st.setPaneW('chat', 9999);
    expect(useWorkbenchStore.getState().paneW.chat).toBe(560);
    st.setPaneW('inspector', 1);
    expect(useWorkbenchStore.getState().paneW.inspector).toBe(240);
  });

  it('分隔条拖拽模拟:mousemove 实时改宽(clamp 内)', () => {
    render(<App />);
    const divider = screen.getByTestId('pane-divider-sessions');
    const startW = useWorkbenchStore.getState().paneW.sessions; // 默认 256
    fireEvent.mouseDown(divider, { clientX: 100 });
    fireEvent.mouseMove(window, { clientX: 150 });
    expect(useWorkbenchStore.getState().paneW.sessions).toBe(startW + 50);
    fireEvent.mouseMove(window, { clientX: -500 }); // 超 clamp → 200
    expect(useWorkbenchStore.getState().paneW.sessions).toBe(200);
    fireEvent.mouseUp(window);
    // inspector 反向:向右拖变窄
    const d2 = screen.getByTestId('pane-divider-inspector');
    const w2 = useWorkbenchStore.getState().paneW.inspector;
    fireEvent.mouseDown(d2, { clientX: 800 });
    fireEvent.mouseMove(window, { clientX: 830 });
    expect(useWorkbenchStore.getState().paneW.inspector).toBe(w2 - 30);
    // 大幅右拖 → 越 min 240 → clamp
    fireEvent.mouseMove(window, { clientX: 2000 });
    expect(useWorkbenchStore.getState().paneW.inspector).toBe(240);
    fireEvent.mouseUp(window);
  });

  it('开编辑器 tab:tabbar 出现「编辑器」+ EditorView 骨架挂载;关闭回空态', async () => {
    render(<App />);
    useWorkbenchStore.getState().openEditor();
    expect(await screen.findByTestId('workbench-tab-editor')).toBeInTheDocument();
    // EditorView(游戏原生)骨架:Hierarchy / Viewport 页签
    expect(await screen.findByText('Hierarchy')).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Viewport' })).toBeInTheDocument();
    fireEvent.click(screen.getByLabelText('关闭 编辑器'));
    expect(screen.queryByTestId('workbench-tab-editor')).not.toBeInTheDocument();
    expect(await screen.findByText('个人工作区')).toBeInTheDocument();
  });

  it('主题切换:store action → data-theme + CSS 变量实测变化', () => {
    render(<App />);
    useThemeStore.getState().setMode('light');
    expect(document.documentElement.dataset.theme).toBe('light');
    expect(document.documentElement.style.getPropertyValue('--accent')).toBe('#C96442');
    useThemeStore.getState().toggleMode();
    expect(document.documentElement.dataset.theme).toBe('dark');
    expect(document.documentElement.style.getPropertyValue('--accent')).toBe('#E2886A');
    expect(document.documentElement.style.getPropertyValue('--bg')).toBe('#1C1B18');
  });

  it('会话流:New Agent 建行 → 选中 → 对话列头标题;重命名/置顶/删除', async () => {
    render(<App />);
    // 直接经 store(等价 POST /sessions;stub 见上)
    const created = await useSessionStore.getState().create('冒烟会话');
    expect(created).not.toBeNull();
    // 侧栏行 + 对话列头双处呈现
    expect((await screen.findAllByText('冒烟会话')).length).toBeGreaterThanOrEqual(1);
    expect(useSessionStore.getState().activeSessionId).toBe(created!.id);
    // 对话列头标题
    expect(screen.getByTestId('chat-head-title')).toHaveTextContent('冒烟会话');
    // 状态点:无 activeRunId → idle(灰)
    // 重命名(双击行内)
    const row = screen.getByTestId(`session-row-${created!.id}`);
    fireEvent.doubleClick(row);
    const input = await screen.findByTestId(`session-rename-${created!.id}`);
    fireEvent.change(input, { target: { value: '改名后' } });
    fireEvent.keyDown(input, { key: 'Enter' });
    expect((await screen.findAllByText('改名后')).length).toBeGreaterThanOrEqual(1);
    // 置顶 → PINNED 区
    fireEvent.click(screen.getByLabelText('置顶'));
    expect(await screen.findByText('PINNED')).toBeInTheDocument();
    // 删除 → 回空态
    fireEvent.click(screen.getByLabelText('删除会话'));
    expect(await screen.findByText('暂无会话，可点击「New Agent」创建。')).toBeInTheDocument();
  });
});
