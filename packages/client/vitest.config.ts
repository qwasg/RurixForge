import { defineConfig } from 'vitest/config';
import react from '@vitejs/plugin-react';

/** 与 vite.config.ts 相同的 import.meta.url → 本地路径小工具(无 @types/node 可用)。 */
function srcDir(): string {
  let p = decodeURIComponent(new URL('./src', import.meta.url).pathname);
  if (/^\/[A-Za-z]:\//.test(p)) p = p.slice(1); // /D:/x → D:/x
  return p;
}

export default defineConfig({
  plugins: [react()],
  resolve: {
    alias: { '@': srcDir() },
  },
  test: {
    environment: 'jsdom',
    setupFiles: './test/setup.ts',
    css: false, // jsdom 下不加载 tailwind CSS,import 一律按空模块处理
  },
});
