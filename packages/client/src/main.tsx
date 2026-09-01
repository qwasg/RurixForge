import React from 'react';
import { createRoot } from 'react-dom/client';
// F7 wave.3:字体栈改系统栈(HarmonyOS Sans SC / Microsoft YaHei UI 等,见 theme.css),
// 不再引 DM Sans webfont;JetBrains Mono Variable 包导入保留(代码字体)。
import '@fontsource-variable/jetbrains-mono';
import './styles/theme.css';
import './styles/index.css';
import { initTheme } from './lib/themeStore';
import App from './App';

// 首帧前注入主题变量(data-theme + CSS 变量;mode=auto 挂系统明暗监听)
initTheme();

const container = document.getElementById('root');
if (!container) throw new Error('missing #root');

createRoot(container).render(
  <React.StrictMode>
    <App />
  </React.StrictMode>,
);
