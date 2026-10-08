import { defineConfig } from 'vite';
import react from '@vitejs/plugin-react';

/**
 * import.meta.url → 本地路径。包不装 @types/node（Docker 构建在 monorepo 之外独立安装），
 * 需处理 Windows 盘符前导斜杠与中文路径的 URL 编码。
 */
function srcDir(): string {
  let p = decodeURIComponent(new URL('./src', import.meta.url).pathname);
  if (/^\/[A-Za-z]:\//.test(p)) p = p.slice(1);
  return p;
}

const CLOUD = 'http://127.0.0.1:8110';

export default defineConfig({
  base: '/admin/',
  plugins: [react()],
  resolve: {
    alias: { '@': srcDir() },
  },
  build: {
    outDir: 'dist',
    emptyOutDir: true,
  },
  server: {
    port: 5188,
    proxy: {
      '/api': { target: CLOUD, changeOrigin: true },
      '/v1': { target: CLOUD, changeOrigin: true },
      '/healthz': { target: CLOUD, changeOrigin: true },
    },
  },
});
