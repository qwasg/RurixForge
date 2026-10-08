'use strict';

const { contextBridge, ipcRenderer } = require('electron');

// 与旧 apps/ide preload 的 window.forgeAPI 结构同构(win.* 窗口控制),
// 另加 host 健康查询。client bridge 的 seam 依赖此形状。
contextBridge.exposeInMainWorld('forgeAPI', {
  win: {
    minimize: () => ipcRenderer.send('win:minimize'),
    toggleMaximize: () => ipcRenderer.send('win:toggle-maximize'),
    close: () => ipcRenderer.send('win:close'),
    onMaximizedChanged: (cb) => {
      const listener = (_e, v) => cb(v);
      ipcRenderer.on('win:maximized-changed', listener);
      return () => ipcRenderer.removeListener('win:maximized-changed', listener);
    },
    // 窗口外框形态(main.cjs createWindow 同源):overlay = 系统绘制三钮(Windows/Linux),
    // inset = macOS 红绿灯。renderer 据此决定是否自绘窗口按钮、左右各让出多少宽度。
    chrome: process.platform === 'darwin' ? 'inset' : 'overlay',
    setOverlayTheme: (theme) => ipcRenderer.send('win:set-overlay-theme', theme),
  },
  getHealth: () => ipcRenderer.invoke('forge:health'),
  codex: { openTask: (url) => ipcRenderer.invoke('codex:open-task', url) },
  auth: { openExternal: (channel, url) => ipcRenderer.invoke('auth:open-external', channel, url) },
  // 视口 bounds、实际推流尺寸与 workspaceId 驱动 presenter；共享缓冲 open/close 绑定当前项目。
  viewport: {
    reportBounds: (b) => ipcRenderer.send('viewport:bounds', b),
  },
  // F2 wave.3:Assets 面板桌面能力(导入文件对话框 / 在文件夹中显示)。
  assets: {
    pickImport: () => ipcRenderer.invoke('assets:pick-import'),
    showInFolder: (rel, workspaceRoot) => ipcRenderer.send('assets:show-in-folder', rel, workspaceRoot),
  },
  // 工作区选择器:系统目录对话框选根目录(web 端无此面,入口如实禁用)。
  workspace: {
    pickFolder: () => ipcRenderer.invoke('workspace:pick-folder'),
  },
  platform: process.platform,
  versions: {
    electron: process.versions.electron,
    chrome: process.versions.chrome,
    node: process.versions.node,
  },
});
