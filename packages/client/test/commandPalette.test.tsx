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
