import { defineConfig } from 'vite';
import react from '@vitejs/plugin-react';
import pkg from './package.json';

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
  // 关于页展示的客户端版本(lib/appVersion.ts 读取)
  define: {
    __APP_VERSION__: JSON.stringify(pkg.version),
  },
  resolve: {
    alias: { '@': srcDir() },
  },
  server: {
    // D-044:开发态应用壳同样禁止被框住(UltraPlan 试玩 Demo iframe 不得加载应用自身);
    // 只加 frame-ancestors,应用至今无 CSP,其余指令不加。
    headers: {
      'Content-Security-Policy': "frame-ancestors 'none'",
    },
    proxy: {
      '/api/forge': {
        target: 'http://127.0.0.1:3080',
        changeOrigin: true,
      },
      // V6 试玩：画面与指令走本机 bridge，去掉跨源 Origin 以免被拒绝。
      '/api/v6': {
        target: 'http://127.0.0.1:3096',
        changeOrigin: true,
        configure: (proxy) => {
          proxy.on('proxyReq', (proxyReq) => {
            proxyReq.removeHeader('origin');
          });
        },
      },
    },
  },
});
