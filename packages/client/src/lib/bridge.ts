import { MOCK_FORGE_API } from './mock';

/**
 * 与 Electron preload(apps/ide/src/preload)注入到 window.forgeAPI 的结构同构。
 * 独立 web SPA 下本地定义,切断对 preload 的依赖(去 Electron 耦合)。
 */
export interface ForgeAPI {
  win: {
    minimize: () => void;
    toggleMaximize: () => void;
    close: () => void;
    onMaximizedChanged: (cb: (maximized: boolean) => void) => () => void;
  };
  /** F1 wave.2 G-F1-9:视口 bounds 上报(desktop preload 提供;web/测试环境缺省) */
  viewport?: {
    reportBounds?: (b: {
      x: number;
      y: number;
      w: number;
      h: number;
      dpr: number;
      visible: boolean;
    }) => void;
  };
  /** F2 wave.3:Assets 面板桌面能力(desktop preload 提供;web 端缺省,菜单项如实禁用) */
  assets?: {
    /** 系统文件对话框选源文件(多选),返回绝对路径;取消 → [] */
    pickImport?: () => Promise<string[]>;
    /** 在系统文件管理器中显示(Content 相对路径) */
    showInFolder?: (rel: string) => void;
  };
  platform: string;
}

declare global {
  interface Window {
    forgeAPI?: ForgeAPI;
  }
}

/**
 * Electron 环境使用 preload 注入的 window.forgeAPI;
 * 纯 web 环境(window.forgeAPI 不存在)回退到 lib/mock.ts 的 no-op 实现。
 * seam 保持不变:调用点仍写作 bridge()?.win.xxx()。
 */
export function bridge(): ForgeAPI {
  return window.forgeAPI ?? MOCK_FORGE_API;
}
