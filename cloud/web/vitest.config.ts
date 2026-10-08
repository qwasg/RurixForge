import { defineConfig } from 'vitest/config';
import react from '@vitejs/plugin-react';

function srcDir(): string {
  let p = decodeURIComponent(new URL('./src', import.meta.url).pathname);
  if (/^\/[A-Za-z]:\//.test(p)) p = p.slice(1);
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
    css: false,
  },
});
