import { defineConfig } from 'vite';
import react from '@vitejs/plugin-react';

/**
 * import.meta.url → 本地路径(自研小工具)。
 * 包未安装 @types/node,不能用 node:path/__dirname;
 * 需处理 Windows 盘符前导斜杠与中文路径的 URL 编码。
 */
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
  server: {
    proxy: {
      '/api/forge': {
        target: 'http://127.0.0.1:3080',
        changeOrigin: true,
      },
    },
  },
});
