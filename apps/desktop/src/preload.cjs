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
  platform: process.platform,
  versions: {
    electron: process.versions.electron,
    node: process.versions.node,
  },
});
