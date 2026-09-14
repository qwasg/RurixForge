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
  },
  getHealth: () => ipcRenderer.invoke('forge:health'),
  codex: { openTask: (url) => ipcRenderer.invoke('codex:open-task', url) },
  // F1 wave.2 G-F1-9:视口容器 bounds 上报(CSS px + dpr),驱动 viewport-presenter
  // 子窗口嵌入;visible=false 表示视口卸载(关闭原生呈现层)。
  viewport: {
    reportBounds: (b) => ipcRenderer.send('viewport:bounds', b),
  },
  // F2 wave.3:Assets 面板桌面能力(导入文件对话框 / 在文件夹中显示)。
  assets: {
    pickImport: () => ipcRenderer.invoke('assets:pick-import'),
    showInFolder: (rel) => ipcRenderer.send('assets:show-in-folder', rel),
  },
  // 工作区选择器:系统目录对话框选根目录(web 端无此面,入口如实禁用)。
  workspace: {
    pickFolder: () => ipcRenderer.invoke('workspace:pick-folder'),
  },
  platform: process.platform,
  versions: {
    electron: process.versions.electron,
    node: process.versions.node,
  },
});
