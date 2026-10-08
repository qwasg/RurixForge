import { describe, it, expect, beforeAll, afterAll, vi } from 'vitest';
import http from 'node:http';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import type { AddressInfo } from 'node:net';
import { buildServer, type ForgeServer } from '../src/server.js';

/**
 * F8 wave.3(G-F8-3 浏览器通路门)host 侧断言:
 * - 静态托管浏览器直开兼容:index.html 200 + no-cache;SPA 回退;vite 指纹资产 immutable;
 *   woff2/js 等 MIME 正确(缺省 application/octet-stream 会被浏览器拒用字体)。
 * - SSE 透传响应头:Content-Type text/event-stream 原样透传 + Cache-Control no-cache +
 *   X-Accel-Buffering: no(禁缓冲);浏览器断连 → 上游请求同步销毁(零孤儿长连接)。
 * host 为 node:http 裸管,无 gzip/压缩中间件,SSE 不被压缩破坏(无压缩可断言面,留痕说明)。
 */

let server: ForgeServer;
let base: string;
let tmpDir: string;
let staticDir: string;
let upstream: http.Server;
let streamClosed: number;

beforeAll(async () => {
  // 静态目录 fixture(config.staticDir 注入 seam)
  tmpDir = fs.mkdtempSync(path.join(os.tmpdir(), 'forge-static-'));
  staticDir = path.join(tmpDir, 'dist');
  fs.mkdirSync(path.join(staticDir, 'assets'), { recursive: true });
  fs.writeFileSync(path.join(staticDir, 'index.html'), '<!doctype html><html><body>f8-shell</body></html>');
  fs.writeFileSync(path.join(staticDir, 'assets', 'app-abc123.js'), 'console.log(1);');
  fs.writeFileSync(path.join(staticDir, 'assets', 'font-deadbeef.woff2'), Buffer.from([0, 1, 2, 3]));

  // stub agentd:SSE 流端点(keep-alive 注释行先行,周期 data 行;记录断连)
  streamClosed = 0;
  upstream = http.createServer((req, res) => {
    const url = new URL(req.url ?? '/', 'http://localhost');
    if (url.pathname.endsWith('/events/stream')) {
      res.writeHead(200, {
        'Content-Type': 'text/event-stream',
        'Cache-Control': 'no-cache',
      });
      res.write(': keep-alive\n\n');
      const timer = setInterval(() => res.write('data: {"seq":1}\n\n'), 100);
      res.on('close', () => {
        streamClosed++;
        clearInterval(timer);
      });
      return;
    }
    res.writeHead(404, { 'Content-Type': 'application/json' });
    res.end(JSON.stringify({ error: { code: 'NOT_FOUND', message: `stub: ${url.pathname}` } }));
  });
  await new Promise<void>((resolve) => upstream.listen(0, '127.0.0.1', resolve));
  process.env.FORGE_AGENTD_ORIGIN = `http://127.0.0.1:${(upstream.address() as AddressInfo).port}`;

  server = buildServer({ dataDir: path.join(tmpDir, 'data'), staticDir });
  const port = await server.listen(0);
  base = `http://127.0.0.1:${port}`;
});

afterAll(async () => {
  await server.close();
  delete process.env.FORGE_AGENTD_ORIGIN;
  await new Promise<void>((resolve) => upstream.close(() => resolve()));
  fs.rmSync(tmpDir, { recursive: true, force: true });
});

describe('host 静态托管浏览器直开兼容(F8 wave.3)', () => {
  it('GET / → 200 text/html + Cache-Control no-cache(防陈旧壳)', async () => {
    const res = await fetch(`${base}/`);
    expect(res.status).toBe(200);
    expect(res.headers.get('content-type')).toBe('text/html; charset=utf-8');
    expect(res.headers.get('cache-control')).toBe('no-cache');
    expect(await res.text()).toContain('f8-shell');
  });

  it('SPA 回退:未知前端路由 → index.html(no-cache)', async () => {
    const res = await fetch(`${base}/some/spa/route`);
    expect(res.status).toBe(200);
    expect(res.headers.get('content-type')).toBe('text/html; charset=utf-8');
    expect(res.headers.get('cache-control')).toBe('no-cache');
    expect(await res.text()).toContain('f8-shell');
  });

  it('vite 指纹资产:js MIME 正确 + immutable 长缓存', async () => {
    const res = await fetch(`${base}/assets/app-abc123.js`);
    expect(res.status).toBe(200);
    expect(res.headers.get('content-type')).toBe('text/javascript; charset=utf-8');
    expect(res.headers.get('cache-control')).toBe('public, max-age=31536000, immutable');
    expect(await res.text()).toBe('console.log(1);');
  });

  it('woff2 字体 MIME = font/woff2(浏览器直开可用)', async () => {
    const res = await fetch(`${base}/assets/font-deadbeef.woff2`);
    expect(res.status).toBe(200);
    expect(res.headers.get('content-type')).toBe('font/woff2');
    const buf = Buffer.from(await res.arrayBuffer());
    expect(buf).toEqual(Buffer.from([0, 1, 2, 3]));
  });

  it("D-044:静态托管响应带 CSP frame-ancestors 'none'(应用壳不可被 Demo iframe 框住),且只此一条指令", async () => {
    for (const p of ['/', '/index.html', '/some/spa/route', '/u/0123abcd/', '/assets/app-abc123.js']) {
      const res = await fetch(`${base}${p}`);
      expect(res.status).toBe(200);
      expect(res.headers.get('content-security-policy')).toBe("frame-ancestors 'none'");
      expect(res.headers.get('x-frame-options')).toBeNull();
      await res.arrayBuffer();
    }
    // 非 GET 落到 serveStatic 的 host 自写 404,同样带头。
    const post = await fetch(`${base}/`, { method: 'POST' });
    expect(post.status).toBe(404);
    expect(post.headers.get('content-security-policy')).toBe("frame-ancestors 'none'");
    await post.arrayBuffer();
  });
});

describe('host SSE 透传浏览器兼容(F8 wave.3)', () => {
  it('events/stream:响应头禁缓冲语义 + 收流 + 断连清理上游(零孤儿)', async () => {
    const controller = new AbortController();
    const res = await fetch(`${base}/api/forge/sessions/sess_1/events/stream`, {
      signal: controller.signal,
    });
    expect(res.status).toBe(200);
    expect(res.headers.get('content-type')).toContain('text/event-stream');
    expect(res.headers.get('cache-control')).toBe('no-cache');
    expect(res.headers.get('x-accel-buffering')).toBe('no');
    expect(res.headers.get('content-security-policy')).toBe("frame-ancestors 'none'");

    // 首块即为上游 keep-alive 注释行(流式,不等整包)
    const reader = res.body!.getReader();
    const first = await reader.read();
    expect(new TextDecoder().decode(first.value)).toContain('keep-alive');
    // 继续收至少一帧 data 行(流未死)
    const second = await reader.read();
    expect(new TextDecoder().decode(second.value)).toContain('data:');

    // 浏览器断开(关闭页面/abort)→ host 销毁上游请求 → agentd 侧连接关闭
    controller.abort();
    await vi.waitFor(() => expect(streamClosed).toBeGreaterThan(0), { timeout: 3000 });
  });
});
