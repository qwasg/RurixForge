import React from 'react';
import { createRoot } from 'react-dom/client';
// 净化波:Inter Variable 包接管西文/数字(中文仍系统栈,见 theme.css --font-sans);
// JetBrains Mono Variable 包导入保留(代码字体)。
// D-046:思源宋体可变字体接管衬线标题(--font-serif),Windows 不再回落宋体;按 unicode-range 分片按需加载。
import '@fontsource-variable/inter';
import '@fontsource-variable/jetbrains-mono';
import '@fontsource-variable/noto-serif-sc';
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
