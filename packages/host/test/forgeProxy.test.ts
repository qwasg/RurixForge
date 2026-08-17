import { describe, it, expect, beforeAll, afterAll } from 'vitest';
import http from 'node:http';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import type { AddressInfo } from 'node:net';
import type { ApiError, HealthStatus } from '@forge/protocol';
import { buildServer, type ForgeServer } from '../src/server.js';

/**
 * forgeProxy 测试:假上游记录请求并回放响应。
 * 通过 FORGE_AGENTD_ORIGIN 注入上游地址(buildServer 装载插件时读取)。
 */

interface Recorded {
  method: string;
  url: string;
  body: string;
}

let upstream: http.Server;
let upstreamBase: string;
let recorded: Recorded[];
let server: ForgeServer;
let base: string;
let tmpDir: string;

const UPSTREAM_REPLY = JSON.stringify({ content: [{ type: 'text', text: '{"ok":true}' }] });

beforeAll(async () => {
  recorded = [];
  upstream = http.createServer((req, res) => {
    const chunks: Buffer[] = [];
    req.on('data', (c: Buffer) => chunks.push(c));
    req.on('end', () => {
      recorded.push({
        method: req.method ?? '',
        url: req.url ?? '',
        body: Buffer.concat(chunks).toString('utf8'),
      });
      res.writeHead(200, { 'Content-Type': 'application/json' });
      res.end(UPSTREAM_REPLY);
    });
  });
  await new Promise<void>((resolve) => upstream.listen(0, '127.0.0.1', resolve));
  upstreamBase = `http://127.0.0.1:${(upstream.address() as AddressInfo).port}`;

  process.env.FORGE_AGENTD_ORIGIN = upstreamBase;
  tmpDir = fs.mkdtempSync(path.join(os.tmpdir(), 'forge-proxy-'));
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

describe('forgeProxy', () => {
  it('POST /api/forge/mcp/call 方法与 body 透传,上游响应原样回传', async () => {
    const payload = JSON.stringify({ tool: 'mcp__engine-scene__scene_summary', arguments: {} });
    const res = await fetch(`${base}/api/forge/mcp/call`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: payload,
    });
    expect(res.status).toBe(200);
    expect(await res.text()).toBe(UPSTREAM_REPLY);

    const hit = recorded.find((r) => r.url === '/api/forge/mcp/call');
    expect(hit).toBeDefined();
    expect(hit!.method).toBe('POST');
    expect(hit!.body).toBe(payload);
  });

  it('GET /api/forge/llm/complete 透传(无 body)', async () => {
    const res = await fetch(`${base}/api/forge/llm/complete`);
    expect(res.status).toBe(200);
    expect(await res.text()).toBe(UPSTREAM_REPLY);
    const hit = recorded.find((r) => r.url === '/api/forge/llm/complete');
    expect(hit).toBeDefined();
    expect(hit!.method).toBe('GET');
    expect(hit!.body).toBe('');
  });

  it('自有路由不被代理遮蔽:/api/forge/health 仍由 host 应答', async () => {
    const res = await fetch(`${base}/api/forge/health`);
    expect(res.status).toBe(200);
    const body = (await res.json()) as HealthStatus;
    expect(body.service).toBe('forge-host');
    // 假上游不应收到 health 请求
    expect(recorded.some((r) => r.url === '/api/forge/health')).toBe(false);
  });

  it('自有路由不被代理遮蔽:/api/forge/sessions 仍由 host 应答', async () => {
    const res = await fetch(`${base}/api/forge/sessions`);
    expect(res.status).toBe(200);
    expect(await res.json()).toEqual([]);
    expect(recorded.some((r) => r.url?.startsWith('/api/forge/sessions'))).toBe(false);
  });

  it('上游不可达 → 502 UPSTREAM_UNREACHABLE', async () => {
    // 独立 server 指向一个已关闭的端口
    const dead = http.createServer();
    await new Promise<void>((resolve) => dead.listen(0, '127.0.0.1', resolve));
    const deadPort = (dead.address() as AddressInfo).port;
    await new Promise<void>((resolve) => dead.close(() => resolve()));

    const saved = process.env.FORGE_AGENTD_ORIGIN;
    process.env.FORGE_AGENTD_ORIGIN = `http://127.0.0.1:${deadPort}`;
    const tmp2 = fs.mkdtempSync(path.join(os.tmpdir(), 'forge-proxy-down-'));
    const srv = buildServer({ dataDir: tmp2 });
    const port = await srv.listen(0);
    try {
      const res = await fetch(`http://127.0.0.1:${port}/api/forge/mcp/call`, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ tool: 'mcp__engine-scene__host_ping' }),
      });
      expect(res.status).toBe(502);
      const body = (await res.json()) as ApiError;
      expect(body.error.code).toBe('UPSTREAM_UNREACHABLE');
    } finally {
      await srv.close();
      fs.rmSync(tmp2, { recursive: true, force: true });
      if (saved === undefined) delete process.env.FORGE_AGENTD_ORIGIN;
      else process.env.FORGE_AGENTD_ORIGIN = saved;
    }
  });
});
