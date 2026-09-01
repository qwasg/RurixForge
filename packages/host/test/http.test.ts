import { describe, it, expect, beforeAll, afterAll } from 'vitest';
import http from 'node:http';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import type { AddressInfo } from 'node:net';
import { type ApiError, type HealthStatus } from '@forge/protocol';
import { buildServer, type ForgeServer } from '../src/server.js';

/**
 * F7 wave.1 起 /api/forge/sessions、/api/forge/chat-folders、/api/forge/design-snapshot
 * 由 forgeProxy 遮蔽透传到 agentd(agent 会话事实源;F0 host stub 插件路由退役,
 * sessions.ts/eventlog.ts 插件代码保留不删,运行时被遮蔽)。
 * 本文件用内存 stub 上游模拟 agentd 会话面(agentd wire 形态 {sessions}/{session}),
 * 断言 http 层透传/遮蔽行为;host 自有路由(health / 未知 404 / CORS)不受遮蔽影响。
 * (F0 的 postMessage echo / eventCount / BAD_JSON 断言随 F0 stub 退役移除,F0 已 closed。)
 */

let server: ForgeServer;
let base: string;
let tmpDir: string;
let upstream: http.Server;
let upstreamHits: string[];

beforeAll(async () => {
  // stub agentd:内存会话面。
  upstreamHits = [];
  const store = new Map<string, { id: string; title: string }>();
  upstream = http.createServer((req, res) => {
    const url = new URL(req.url ?? '/', 'http://localhost');
    upstreamHits.push(`${req.method} ${url.pathname}`);
    const send = (status: number, body: unknown) => {
      res.writeHead(status, { 'Content-Type': 'application/json' });
      res.end(JSON.stringify(body));
    };
    if (url.pathname === '/api/forge/sessions' && req.method === 'GET') {
      send(200, { sessions: [...store.values()] });
      return;
    }
    if (url.pathname === '/api/forge/sessions' && req.method === 'POST') {
      const chunks: Buffer[] = [];
      req.on('data', (c: Buffer) => chunks.push(c));
      req.on('end', () => {
        let title = '新会话';
        try {
          const body = JSON.parse(Buffer.concat(chunks).toString('utf8')) as { title?: string };
          if (typeof body.title === 'string' && body.title.trim() !== '') title = body.title;
        } catch {
          /* 坏 JSON 用默认标题(stub 从简) */
        }
        const id = `sess_stub_${store.size + 1}`;
        store.set(id, { id, title });
        send(200, { session: store.get(id) });
      });
      return;
    }
    const m = /^\/api\/forge\/sessions\/([^/]+)$/.exec(url.pathname);
    if (m && req.method === 'GET') {
      const s = store.get(m[1]);
      if (!s) {
        send(404, { error: { code: 'SESSION_NOT_FOUND', message: `session not found: ${m[1]}` } });
        return;
      }
      send(200, { session: s });
      return;
    }
    send(404, { error: { code: 'NOT_FOUND', message: `stub: ${url.pathname}` } });
  });
  await new Promise<void>((resolve) => upstream.listen(0, '127.0.0.1', resolve));
  process.env.FORGE_AGENTD_ORIGIN = `http://127.0.0.1:${(upstream.address() as AddressInfo).port}`;

  tmpDir = fs.mkdtempSync(path.join(os.tmpdir(), 'forge-http-'));
  server = buildServer({ dataDir: tmpDir });
  const port = await server.listen(0);
  base = `http://127.0.0.1:${port}`;
});

afterAll(async () => {
  await server.close();
  delete process.env.FORGE_AGENTD_ORIGIN;
  fs.rmSync(tmpDir, { recursive: true, force: true });
  await new Promise<void>((resolve) => upstream.close(() => resolve()));
});

describe('http api', () => {
  it('GET /api/forge/health 返回合法 HealthStatus(host 自有,不经代理)', async () => {
    const res = await fetch(`${base}/api/forge/health`);
    expect(res.status).toBe(200);
    expect(res.headers.get('access-control-allow-origin')).toBe('http://localhost:5173');
    const body = (await res.json()) as HealthStatus;
    expect(body.status).toBe('ok');
    expect(body.service).toBe('forge-host');
    expect(typeof body.version).toBe('string');
    expect(body.port).toBeGreaterThan(0);
    expect(typeof body.uptimeSec).toBe('number');
    expect(body.uptimeSec).toBeGreaterThanOrEqual(0);
    expect(typeof body.time).toBe('string');
    // 前缀不重叠:health 不命中上游。
    expect(upstreamHits.some((h) => h.endsWith('/api/forge/health'))).toBe(false);
  });

  it('F7:sessions 面经代理透传(agentd wire 形态;host F0 stub 被遮蔽)', async () => {
    const empty = (await (await fetch(`${base}/api/forge/sessions`)).json()) as {
      sessions: unknown[];
    };
    expect(empty.sessions).toEqual([]);

    const createdRes = await fetch(`${base}/api/forge/sessions`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ title: 'demo' }),
    });
    expect(createdRes.status).toBe(200);
    const created = (await createdRes.json()) as { session: { id: string; title: string } };
    expect(created.session.title).toBe('demo');
    expect(created.session.id.startsWith('sess_')).toBe(true);

    const list = (await (await fetch(`${base}/api/forge/sessions`)).json()) as {
      sessions: { id: string }[];
    };
    expect(list.sessions).toHaveLength(1);
    expect(list.sessions[0].id).toBe(created.session.id);

    const getRes = await fetch(`${base}/api/forge/sessions/${created.session.id}`);
    expect(getRes.status).toBe(200);
    expect(((await getRes.json()) as { session: { id: string } }).session.id).toBe(
      created.session.id,
    );

    // 遮蔽证据:以上 4 次请求全部命中 stub 上游(host 自有 sessions 插件路由不应答)。
    const sessionHits = upstreamHits.filter((h) => h.includes('/api/forge/sessions'));
    expect(sessionHits.length).toBeGreaterThanOrEqual(4);
  });

  it('F7:未知会话 404 SESSION_NOT_FOUND 自上游原样透传', async () => {
    const res = await fetch(`${base}/api/forge/sessions/no-such-id`);
    expect(res.status).toBe(404);
    expect(((await res.json()) as ApiError).error.code).toBe('SESSION_NOT_FOUND');
  });

  it('未代理前缀仍走 host 自有路由:/api/forge/unknown 404 NOT_FOUND', async () => {
    const notFound = await fetch(`${base}/api/forge/unknown`);
    expect(notFound.status).toBe(404);
    expect(((await notFound.json()) as ApiError).error.code).toBe('NOT_FOUND');
  });

  it('OPTIONS 预检 204 且带 CORS 头', async () => {
    const res = await fetch(`${base}/api/forge/health`, { method: 'OPTIONS' });
    expect(res.status).toBe(204);
    expect(res.headers.get('access-control-allow-origin')).toBe('http://localhost:5173');
  });
});
