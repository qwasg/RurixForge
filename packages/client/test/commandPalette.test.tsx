import { cleanup, fireEvent, render, screen } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import App from '@/App';
import { COMMANDS, filterCommands } from '@/lib/commands';
import { useOverlayStore } from '@/lib/overlayStore';
import { useSessionStore } from '@/lib/sessionStore';
import { useThemeStore } from '@/lib/themeStore';
import { useWorkbenchStore } from '@/lib/workbenchStore';
import { mockForgeBackend } from './forgeMock';

/** F7 wave.3 命令面板:过滤 / ↑↓ 循环 / Enter 执行 / Esc 关;分组小节。 */

const initialSessions = useSessionStore.getState();
const initialWorkbench = useWorkbenchStore.getState();
const initialTheme = useThemeStore.getState();
const initialOverlay = useOverlayStore.getState();

beforeEach(() => {
  useSessionStore.setState(initialSessions, true);
  useWorkbenchStore.setState(initialWorkbench, true);
  useThemeStore.setState(initialTheme, true);
  useOverlayStore.setState(initialOverlay, true);
  globalThis.localStorage?.clear();
  vi.stubGlobal(
    'fetch',
    mockForgeBackend(
      {},
      {
        '/api/forge/sessions': { sessions: [] },
        '/api/forge/chat-folders': { folders: [] },
        '/api/forge/health': { status: 'ok' },
        '/api/forge/design-snapshot': { models: { models: [] }, todos: [] },
      },
    ),
  );
});

afterEach(() => {
  cleanup();
  vi.unstubAllGlobals();
});

describe('命令注册表', () => {
  it('filterCommands:label/id 子串过滤', () => {
    expect(filterCommands('').length).toBe(COMMANDS.length);
    const hit = filterCommands('主题');
    expect(hit.map((c) => c.id)).toEqual(['theme.toggle']);
    expect(filterCommands('session.new').map((c) => c.id)).toEqual(['session.new']);
    expect(filterCommands('不存在的命令xyz')).toHaveLength(0);
  });
});

describe('<CommandPalette />', () => {
  it('Ctrl+K 开 → 分组小节 + 全量命令;Esc 关', async () => {
    render(<App />);
    fireEvent.keyDown(window, { key: 'k', ctrlKey: true });
    expect(await screen.findByTestId('command-palette-input')).toBeInTheDocument();
    // 分组小节(Agent / 导航 / 视图)
    expect(screen.getByText('Agent')).toBeInTheDocument();
    expect(screen.getByText('导航')).toBeInTheDocument();
    expect(screen.getByText('视图')).toBeInTheDocument();
    expect(screen.getByTestId('command-row-session.new')).toBeInTheDocument();
    fireEvent.keyDown(screen.getByTestId('command-palette-input'), { key: 'Escape' });
    expect(screen.queryByTestId('command-palette-input')).not.toBeInTheDocument();
  });

  it('过滤 + ↑↓ 循环 + Enter 执行(tab.editor → 编辑器 tab)', async () => {
    render(<App />);
    useOverlayStore.getState().open('palette');
    const input = await screen.findByTestId('command-palette-input');
    fireEvent.change(input, { target: { value: '编辑器' } });
    const rows = screen.getAllByTestId(/^command-row-/);
    expect(rows).toHaveLength(1);
    expect(rows[0]).toHaveAttribute('data-testid', 'command-row-tab.editor');
    // ↓ 越顶循环回 0;↑ 回绕到底
    fireEvent.keyDown(input, { key: 'ArrowDown' });
    fireEvent.keyDown(input, { key: 'ArrowUp' });
    fireEvent.keyDown(input, { key: 'Enter' });
    // 执行后关面板 + 编辑器 tab 打开
    expect(screen.queryByTestId('command-palette-input')).not.toBeInTheDocument();
    expect(await screen.findByTestId('workbench-tab-editor')).toBeInTheDocument();
  });

  it('Enter 执行 theme.toggle → data-theme 翻转', async () => {
    render(<App />);
    useThemeStore.getState().setMode('light');
    useOverlayStore.getState().open('palette');
    const input = await screen.findByTestId('command-palette-input');
    fireEvent.change(input, { target: { value: '主题' } });
    fireEvent.keyDown(input, { key: 'Enter' });
    expect(useThemeStore.getState().isDark).toBe(true);
    expect(document.documentElement.dataset.theme).toBe('dark');
  });

  it('搜索胶囊点击开面板(titlebar)', async () => {
    render(<App />);
    fireEvent.click(screen.getByTestId('titlebar-search'));
    expect(await screen.findByTestId('command-palette-input')).toBeInTheDocument();
  });
});

describe('<CommandPalette /> D-040 会话与文件', () => {
  const session = (id: string, title: string, updatedAt: string) => ({
    id,
    title,
    status: 'idle',
    agentKind: 'coding',
    agentEngine: 'local' as const,
    selectedModelId: null,
    thinkingEnabled: false,
    reasoningEffort: null,
    contextOptionId: null,
    webSearchEnabled: false,
    activeRunId: null,
    createdAt: updatedAt,
    updatedAt,
    pinned: false,
    titleManuallySet: true,
  });

  function stubSearch(searchCalls: string[], sessions: unknown[] = []) {
    vi.stubGlobal(
      'fetch',
      mockForgeBackend(
        {},
        {
          '/api/forge/sessions': { sessions },
          '/api/forge/chat-folders': { folders: [] },
          '/api/forge/health': { status: 'ok' },
          '/api/forge/design-snapshot': { models: { models: [] }, todos: [] },
          '/api/forge/workspace/search': () => {
            const url = String((globalThis.fetch as unknown as { mock: { calls: unknown[][] } }).mock.calls.at(-1)?.[0]);
            const q = new URL(url, 'http://x').searchParams.get('q') ?? '';
            searchCalls.push(q);
            return q === ''
              ? { query: '', results: [], total: 0, truncated: false, source: 'git', scanned: 3, scanTruncated: false }
              : {
                  query: q,
                  results: [{ path: 'src/shell/Sidebar.tsx', name: 'Sidebar.tsx', dir: 'src/shell' }],
                  total: 1,
                  truncated: false,
                  source: 'git',
                  scanned: 3,
                  scanTruncated: false,
                };
          },
        },
      ),
    );
  }

  it('空查询列最近会话;按标题过滤后 Enter 切到该会话', async () => {
    stubSearch(
      [],
      [session('s_old', '旧的会话', '2026-09-01T00:00:00Z'), session('s_new', '搭灰盒关卡', '2026-09-20T00:00:00Z')],
    );
    render(<App />);
    await vi.waitFor(() => expect(useSessionStore.getState().sessions).toHaveLength(2));
    useOverlayStore.getState().open('palette');
    const input = await screen.findByTestId('command-palette-input');
    expect(screen.getByText('最近会话')).toBeInTheDocument();
    const rows = screen.getAllByTestId(/^palette-session-/).map((el) => el.getAttribute('data-testid'));
    expect(rows).toEqual(['palette-session-s_new', 'palette-session-s_old']);
    fireEvent.change(input, { target: { value: '灰盒' } });
    expect(screen.getAllByTestId(/^palette-session-/)).toHaveLength(1);
    fireEvent.keyDown(input, { key: 'Enter' });
    expect(useSessionStore.getState().activeSessionId).toBe('s_new');
    expect(useOverlayStore.getState().palette).toBe(false);
  });

  it('打开即预热;输入后防抖搜文件,Enter 在工作台打开文件', async () => {
    const calls: string[] = [];
    stubSearch(calls);
    render(<App />);
    useOverlayStore.getState().open('palette');
    const input = await screen.findByTestId('command-palette-input');
    await vi.waitFor(() => expect(calls).toContain(''));
    fireEvent.change(input, { target: { value: 'sidebar' } });
    const row = await screen.findByTestId('palette-file-src/shell/Sidebar.tsx');
    expect(row).toHaveTextContent('Sidebar.tsx');
    expect(calls).toContain('sidebar');
    fireEvent.click(row);
    expect(useWorkbenchStore.getState().activeTabId).toBe('file:src/shell/Sidebar.tsx');
  });

  it('文件搜索失败如实提示,命令照常可执行', async () => {
    render(<App />);
    useOverlayStore.getState().open('palette');
    const input = await screen.findByTestId('command-palette-input');
    fireEvent.change(input, { target: { value: '主题' } });
    expect(await screen.findByTestId('palette-files-empty')).toHaveTextContent('文件搜索不可用');
    expect(screen.getByTestId('command-row-theme.toggle')).toBeInTheDocument();
  });
});
