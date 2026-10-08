import { act, cleanup, fireEvent, render, screen } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import TitleBar from '@/components/shell/TitleBar';
import type { ForgeAPI } from '@/lib/bridge';
import { applyPalette, useThemeStore } from '@/lib/themeStore';
import { useWorkbenchStore } from '@/lib/workbenchStore';

/**
 * 桌面外壳单层标题栏:按 bridge().win.chrome 区分系统绘制三钮(overlay)、
 * macOS 红绿灯(inset)与旧壳自绘三钮;overlay 下主题色同步给系统三钮。
 */

function stubDesktop(chrome: ForgeAPI['win']['chrome'], setOverlayTheme = vi.fn()) {
  window.forgeAPI = {
    win: {
      minimize: vi.fn(),
      toggleMaximize: vi.fn(),
      close: vi.fn(),
      onMaximizedChanged: vi.fn(() => () => {}),
      chrome,
      setOverlayTheme,
    },
    platform: chrome === 'inset' ? 'darwin' : 'win32',
  };
  return setOverlayTheme;
}

beforeEach(() => {
  delete window.forgeAPI;
});

afterEach(() => {
  cleanup();
  delete window.forgeAPI;
  vi.restoreAllMocks();
});

describe('<TitleBar /> 窗口外框形态', () => {
  it('overlay:不自绘三钮,右侧给系统三钮让位,并下发标题栏底色/符号色', () => {
    const setOverlayTheme = stubDesktop('overlay');
    render(<TitleBar />);
    expect(screen.getByTestId('shell-titlebar')).toHaveAttribute('data-chrome', 'overlay');
    expect(screen.queryByLabelText('Minimize')).toBeNull();
    expect(screen.queryByLabelText('Close')).toBeNull();
    expect(screen.getByTestId('titlebar-overlay-inset')).toBeInTheDocument();
    const { isDark, light, dark } = useThemeStore.getState();
    const tokens = applyPalette(isDark ? dark : light, isDark);
    expect(setOverlayTheme).toHaveBeenLastCalledWith({
      color: tokens['bg-sunk'],
      symbolColor: tokens['text-2'],
    });
  });

  it('overlay:切主题后重新下发配色', () => {
    const setOverlayTheme = stubDesktop('overlay');
    render(<TitleBar />);
    const before = setOverlayTheme.mock.calls.length;
    const wasDark = useThemeStore.getState().isDark;
    act(() => useThemeStore.getState().setMode(wasDark ? 'light' : 'dark'));
    expect(setOverlayTheme.mock.calls.length).toBeGreaterThan(before);
    act(() => useThemeStore.getState().setMode('auto'));
  });

  it('inset(macOS):不自绘三钮也不留右侧让位', () => {
    stubDesktop('inset');
    render(<TitleBar />);
    expect(screen.getByTestId('shell-titlebar')).toHaveAttribute('data-chrome', 'inset');
    expect(screen.queryByLabelText('Minimize')).toBeNull();
    expect(screen.queryByTestId('titlebar-overlay-inset')).toBeNull();
  });

  it('面板开关在右侧组,提示带快捷键,按下态随折叠状态', () => {
    render(<TitleBar />);
    const sessions = screen.getByLabelText('切换会话栏');
    expect(sessions).toHaveAttribute('title', '切换会话栏(Ctrl+B)');
    const was = useWorkbenchStore.getState().collapsed.sessions;
    expect(sessions).toHaveAttribute('aria-pressed', String(!was));
    fireEvent.click(sessions);
    expect(useWorkbenchStore.getState().collapsed.sessions).toBe(!was);
    fireEvent.click(sessions);
    expect(screen.getByLabelText('切换 Inspector')).toHaveAttribute('title', '切换右栏(Ctrl+Alt+B)');
  });

  it('View 菜单行显示命令注册表里的快捷键', () => {
    render(<TitleBar />);
    fireEvent.click(screen.getByTestId('menu-view'));
    const row = screen.getByText('切换会话栏').closest('button');
    expect(row).toHaveTextContent('Ctrl+B');
  });
});
