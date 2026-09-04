import { useEffect } from 'react';
import Shell from './components/shell/Shell';
import { useChatStore } from './lib/chatStore';
import { useOverlayStore } from './lib/overlayStore';
import { useSessionStore } from './lib/sessionStore';
import { useSettingsStore, type SettingsPage } from './lib/settingsStore';
import { useThemeStore, type ThemeMode } from './lib/themeStore';
import { useWorkbenchStore, type BuiltinTabKind } from './lib/workbenchStore';

/**
 * F7 wave.3:App = 新壳(titlebar/三栏/statusbar + 浮层)。
 * 快捷键(Ctrl+K / Ctrl+Shift+N / Ctrl+J / Esc)与首次 loadAll 在 Shell 内挂载;
 * 主题初始化在 main.tsx(initTheme,首帧前注入 CSS 变量)。
 */
export default function App() {
  // desktop 冒烟 seam(F7 wave.3):window.__forgeShell 暴露最小 action 面,
  // 供 apps/desktop main.cjs 冒烟场景 executeJavaScript 驱动(开编辑器 tab /
  // 主题切换 / 建会话)。与 F5 gen 冒烟的 data-* 选择器同级,属调试 seam。
  // wave.5 扩:设置开/翻页、内建 tab、底部面板。
  useEffect(() => {
    const w = window as unknown as { __forgeShell?: Record<string, unknown> };
    w.__forgeShell = {
      openEditor: () => useWorkbenchStore.getState().openEditor(),
      openTab: (k: BuiltinTabKind) => useWorkbenchStore.getState().openTab(k),
      openFile: (path: string) => useWorkbenchStore.getState().openFile(path),
      // D-035:计划页按路径开(plan 已不是单例内建 tab)。
      openPlan: (path: string) => useWorkbenchStore.getState().openPlan(path),
      toggleBottom: () => useWorkbenchStore.getState().toggleBottom(),
      openSettings: (page?: SettingsPage) => {
        if (page) useSettingsStore.getState().setPage(page);
        useOverlayStore.getState().open('settings');
      },
      setThemeMode: (m: ThemeMode) => useThemeStore.getState().setMode(m),
      createSession: (title?: string) => useSessionStore.getState().create(title),
      stores: {
        theme: useThemeStore,
        workbench: useWorkbenchStore,
        sessions: useSessionStore,
        chat: useChatStore,
        settings: useSettingsStore,
      },
    };
    return () => {
      delete w.__forgeShell;
    };
  }, []);

  return <Shell />;
}
