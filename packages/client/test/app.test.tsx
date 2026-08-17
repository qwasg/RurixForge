import { cleanup, render, screen } from '@testing-library/react';
import { afterEach, describe, expect, it, vi } from 'vitest';
import App from '@/App';
import { useAppStore } from '@/lib/store';
import { mockForgeBackend } from './forgeMock';

afterEach(() => {
  cleanup();
  vi.unstubAllGlobals();
  useAppStore.setState({ route: 'home', activeAgentId: null });
});

/**
 * 冒烟:window.forgeAPI 保持 undefined,
 * lib/bridge 应回退到 lib/mock 的 no-op 实现,App 可在纯 web 环境完整渲染。
 * 应用内路由由 zustand store 驱动(初始 route='home'),无需 MemoryRouter;
 * TerminalView 为纯静态 DOM(未使用 xterm),无需 mock 任何模块。
 */
describe('<App />', () => {
  it('渲染标题栏与侧边栏骨架(默认 home 视图)', () => {
    render(<App />);
    // 标题栏菜单栏
    expect(screen.getByText('File')).toBeInTheDocument();
    expect(screen.getByText('Edit')).toBeInTheDocument();
    // 侧边栏导航与分组标题
    expect(screen.getByText('New Agent')).toBeInTheDocument();
    expect(screen.getByText('Workspaces')).toBeInTheDocument();
    // home 视图输入区占位文本
    expect(
      screen.getByPlaceholderText('Plan, Build. / for skills, @ for context'),
    ).toBeInTheDocument();
  });

  it('settings 路由:openSettings → 设置页骨架(F3 wave.4)', async () => {
    vi.stubGlobal(
      'fetch',
      mockForgeBackend({}, { '/api/forge/skills/list': { skills: [] } }),
    );
    render(<App />);
    useAppStore.getState().openSettings();
    expect(await screen.findByTestId('settings-view')).toBeInTheDocument();
    // 默认落 skills tab(DEFAULT_TAB):菜单钮 + 内容区标题双处呈现
    expect(screen.getAllByText('Skills').length).toBeGreaterThanOrEqual(2);
  });
});
