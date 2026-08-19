import { describe, it, expect, beforeAll, afterAll } from 'vitest';
import http from 'node:http';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import type { AddressInfo } from 'node:net';
import type { ApiError, HealthStatus } from '@forge/protocol';
import { buildServer, type ForgeServer } from '../src/server.js';
import { isLongLivedPath, proxyMatches, upstreamTimeoutMs } from '../src/plugins/forgeProxy.js';

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

  it('F5 wave.3:/api/forge/gen/* 透传(GET backends + POST configure)', async () => {
    const res = await fetch(`${base}/api/forge/gen/backends`);
    expect(res.status).toBe(200);
    expect(recorded.some((r) => r.url === '/api/forge/gen/backends' && r.method === 'GET')).toBe(
      true,
    );

    const payload = JSON.stringify({ id: 'local-mock', kind: 'local', enabled: true });
    const res2 = await fetch(`${base}/api/forge/gen/backends/configure`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: payload,
    });
    expect(res2.status).toBe(200);
    const hit = recorded.find((r) => r.url === '/api/forge/gen/backends/configure');
    expect(hit).toBeDefined();
    expect(hit!.method).toBe('POST');
    expect(hit!.body).toBe(payload);
  });

  it('自有路由不被代理遮蔽:/api/forge/health 仍由 host 应答', async () => {
    const res = await fetch(`${base}/api/forge/health`);
    expect(res.status).toBe(200);
    const body = (await res.json()) as HealthStatus;
    expect(body.service).toBe('forge-host');
    // 假上游不应收到 health 请求
    expect(recorded.some((r) => r.url === '/api/forge/health')).toBe(false);
  });

  it('F7 wave.1:/api/forge/sessions 改由代理遮蔽透传(agentd 事实源,F0 stub 退役)', async () => {
    // F0 时本断言为「仍由 host 应答」;F7 wave.1 起会话面事实源移到 agentd,
    // 请求须命中上游(遮蔽 host 自有 sessions.ts 插件路由,契约留痕)。
    const res = await fetch(`${base}/api/forge/sessions`);
    expect(res.status).toBe(200);
    expect(await res.text()).toBe(UPSTREAM_REPLY);
    expect(recorded.some((r) => r.url === '/api/forge/sessions' && r.method === 'GET')).toBe(true);
  });

  it('F7 wave.1:/api/forge/chat-folders 与 /api/forge/design-snapshot 透传', async () => {
    const res = await fetch(`${base}/api/forge/chat-folders`);
    expect(res.status).toBe(200);
    expect(recorded.some((r) => r.url === '/api/forge/chat-folders')).toBe(true);
    const res2 = await fetch(`${base}/api/forge/design-snapshot?sessionId=sess_x`);
    expect(res2.status).toBe(200);
    expect(
      recorded.some((r) => r.url === '/api/forge/design-snapshot?sessionId=sess_x'),
    ).toBe(true);
  });

  it('F7 wave.1/2:代理前缀/长生命周期超时豁免纯函数判定', () => {
    // 新前缀命中(含子路径)
    expect(proxyMatches('/api/forge/sessions')).toBe(true);
    expect(proxyMatches('/api/forge/sessions/sess_1')).toBe(true);
    expect(proxyMatches('/api/forge/sessions/sess_1/events/stream')).toBe(true);
    expect(proxyMatches('/api/forge/sessions/sess_1/fork')).toBe(true);
    expect(proxyMatches('/api/forge/chat-folders')).toBe(true);
    expect(proxyMatches('/api/forge/chat-folders/fld_1')).toBe(true);
    expect(proxyMatches('/api/forge/design-snapshot')).toBe(true);
    // F7 wave.2:runs/todos 前缀 + ask:execute(sessions 前缀内)
    expect(proxyMatches('/api/forge/runs')).toBe(true);
    expect(proxyMatches('/api/forge/runs/run_1')).toBe(true);
    expect(proxyMatches('/api/forge/runs/run_1/cancel')).toBe(true);
    expect(proxyMatches('/api/forge/todos')).toBe(true);
    expect(proxyMatches('/api/forge/todos/todo_1')).toBe(true);
    expect(proxyMatches('/api/forge/sessions/sess_1/todos')).toBe(true);
    expect(proxyMatches('/api/forge/sessions/sess_1/ask:execute')).toBe(true);
    // F7 wave.5:workspace 树前缀;llm/key 落在既有 /api/forge/llm 前缀内
    expect(proxyMatches('/api/forge/workspace')).toBe(true);
    expect(proxyMatches('/api/forge/workspace/tree')).toBe(true);
    expect(proxyMatches('/api/forge/llm/key')).toBe(true);
    // 边界:相似串不命中;health 仍 host 自有
    expect(proxyMatches('/api/forge/workspacex')).toBe(false);
    expect(proxyMatches('/api/forge/sessionsx')).toBe(false);
    expect(proxyMatches('/api/forge/runsx')).toBe(false);
    expect(proxyMatches('/api/forge/todosx')).toBe(false);
    expect(proxyMatches('/api/forge/health')).toBe(false);
    expect(proxyMatches('/api/other')).toBe(false);
    // 长生命周期豁免:/events/stream 与 /ask:execute 并列 0(不限时),其余 15s
    expect(isLongLivedPath('/api/forge/sessions/sess_1/events/stream')).toBe(true);
    expect(isLongLivedPath('/api/forge/sessions/sess_1/ask:execute')).toBe(true);
    expect(isLongLivedPath('/api/forge/sessions/sess_1/events')).toBe(false);
    expect(isLongLivedPath('/api/forge/sessions/sess_1/ask')).toBe(false);
    expect(isLongLivedPath('/api/forge/sessions')).toBe(false);
    expect(isLongLivedPath('/api/forge/runs/run_1/cancel')).toBe(false);
    expect(upstreamTimeoutMs('/api/forge/sessions/sess_1/events/stream')).toBe(0);
    expect(upstreamTimeoutMs('/api/forge/sessions/sess_1/ask:execute')).toBe(0);
    expect(upstreamTimeoutMs('/api/forge/sessions')).toBe(15_000);
    expect(upstreamTimeoutMs('/api/forge/design-snapshot')).toBe(15_000);
    expect(upstreamTimeoutMs('/api/forge/runs/run_1')).toBe(15_000);
    expect(upstreamTimeoutMs('/api/forge/todos/todo_1')).toBe(15_000);
  });

  it('F7 wave.2:runs/todos 请求经代理透传到上游', async () => {
    const res = await fetch(`${base}/api/forge/runs/run_x`);
    expect(res.status).toBe(200);
    expect(recorded.some((r) => r.url === '/api/forge/runs/run_x' && r.method === 'GET')).toBe(
      true,
    );
    const res2 = await fetch(`${base}/api/forge/runs/run_x/cancel`, { method: 'POST' });
    expect(res2.status).toBe(200);
    expect(
      recorded.some((r) => r.url === '/api/forge/runs/run_x/cancel' && r.method === 'POST'),
    ).toBe(true);
    const payload = JSON.stringify({ sessionId: 'sess_1', title: 't' });
    const res3 = await fetch(`${base}/api/forge/todos`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: payload,
    });
    expect(res3.status).toBe(200);
    const hit = recorded.find((r) => r.url === '/api/forge/todos');
    expect(hit).toBeDefined();
    expect(hit!.method).toBe('POST');
    expect(hit!.body).toBe(payload);
    const res4 = await fetch(`${base}/api/forge/todos/todo_1`, {
      method: 'PATCH',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ status: 'completed' }),
    });
    expect(res4.status).toBe(200);
    expect(recorded.some((r) => r.url === '/api/forge/todos/todo_1' && r.method === 'PATCH')).toBe(
      true,
    );
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
