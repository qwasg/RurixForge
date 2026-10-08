import { StrictMode } from 'react';
import { createRoot } from 'react-dom/client';
import { App } from './App';
import { applyThemeMode, readThemeMode } from './lib/theme';
import './index.css';

// 首帧前套用主题，避免闪烁。
applyThemeMode(readThemeMode());

const root = document.getElementById('root');
if (!root) throw new Error('缺少 #root 挂载点');

createRoot(root).render(
  <StrictMode>
    <App />
  </StrictMode>,
);
