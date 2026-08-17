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
