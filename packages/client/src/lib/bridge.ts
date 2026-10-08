/**
 * 与 Electron preload(apps/desktop/src/preload.cjs)注入到 window.forgeAPI 的结构同构。
 * 独立 web SPA 下本地定义,切断对 preload 的依赖(去 Electron 耦合)。
 */
export interface ForgeAPI {
  auth?: { openExternal: (channel: 'codex' | 'antigravity' | 'kimi' | 'glm', url: string) => Promise<void> };
  /** Opens only the documented Codex new-task deep link in the native app. */
  codex?: { openTask: (url: string) => Promise<void> };
  win: {
    minimize: () => void;
    toggleMaximize: () => void;
    close: () => void;
    onMaximizedChanged: (cb: (maximized: boolean) => void) => () => void;
    /**
     * 窗口外框形态:overlay = 系统绘制三钮(Windows/Linux titleBarOverlay);
     * inset = macOS 红绿灯;缺省 = 旧桌面壳,由 TitleBar 自绘三钮。
     */
    chrome?: 'overlay' | 'inset';
    /** overlay 形态下把标题栏底色/符号色同步给系统三钮(#RRGGBB)。 */
    setOverlayTheme?: (theme: { color: string; symbolColor: string }) => void;
  };
  /** F1 wave.2 G-F1-9:视口 bounds 上报(desktop preload 提供;web/测试环境缺省) */
  viewport?: {
    reportBounds?: (b: {
      x: number;
      y: number;
      w: number;
      h: number;
      dpr: number;
      streamW?: number;
      streamH?: number;
      workspaceId?: string | null;
      visible: boolean;
    }) => void;
  };
  /** F2 wave.3:Assets 面板桌面能力(desktop preload 提供;web 端缺省,菜单项如实禁用) */
  assets?: {
    /** 系统文件对话框选源文件(多选),返回绝对路径;取消 → [] */
    pickImport?: () => Promise<string[]>;
    /** 在系统文件管理器中显示(Content 相对路径;workspaceRoot = 当前工作区根,缺省按默认项目) */
    showInFolder?: (rel: string, workspaceRoot?: string) => void;
  };
  /** 工作区选择器桌面能力(desktop preload 提供;web 端缺省,入口如实禁用) */
  workspace?: {
    /** 系统目录对话框选工作区根(可在框内新建文件夹),返回绝对路径;取消 → null */
    pickFolder?: () => Promise<string | null>;
  };
  platform: string;
  /** 桌面端运行时版本(关于页展示;web 端缺省)。 */
  versions?: { electron?: string; chrome?: string; node?: string };
}

declare global {
  interface Window {
    forgeAPI?: ForgeAPI;
  }
}

/**
 * 纯 web 环境下的 window.forgeAPI 桩:各面诚实禁用态。
 * - win.*: no-op + console.info(不抛异常)
 * - viewport.reportBounds: no-op(让 ViewportCanvas 走回退腿)
 * - assets.pickImport: rejected promise + console.info
 * - assets.showInFolder: no-op + console.info
 * - workspace.pickFolder: resolve(null) + console.info(等价「用户取消」)
 * (F7 wave.3:自 lib/mock.ts 迁入本文件——mock.ts 假数据体系随旧面下线。)
 */
export const MOCK_FORGE_API: ForgeAPI = {
  win: {
    minimize: () => { console.info('[bridge] minimize 不可用(仅桌面端)'); },
    toggleMaximize: () => { console.info('[bridge] toggleMaximize 不可用(仅桌面端)'); },
    close: () => { console.info('[bridge] close 不可用(仅桌面端)'); },
    onMaximizedChanged: () => () => {},
  },
  viewport: {
    reportBounds: () => {},
  },
  assets: {
    pickImport: () => {
      console.info('[bridge] pickImport 不可用(仅桌面端)');
      return Promise.reject(new Error('仅桌面端可用'));
    },
    showInFolder: () => {
      console.info('[bridge] showInFolder 不可用(仅桌面端)');
    },
  },
  workspace: {
    pickFolder: () => {
      console.info('[bridge] pickFolder 不可用(仅桌面端)');
      return Promise.resolve(null);
    },
  },
  platform: 'web',
};

/** 判断当前是否在桌面 Electron 环境(有 preload 注入的 forgeAPI)。 */
export function isDesktopBridge(): boolean {
  return typeof window !== 'undefined' && !!window.forgeAPI;
}

/**
 * Electron 环境使用 preload 注入的 window.forgeAPI;
 * 纯 web 环境(window.forgeAPI 不存在)回退到上面的 no-op 实现。
 * seam 保持不变:调用点仍写作 bridge().win.xxx()。
 */
export function bridge(): ForgeAPI {
  return window.forgeAPI ?? MOCK_FORGE_API;
}
