import { describe, it, expect, beforeAll, afterAll } from 'vitest';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import {
  isEventEnvelope,
  type ApiError,
  type EventEnvelope,
  type HealthStatus,
  type SessionSummary,
} from '@forge/protocol';
import { buildServer, type ForgeServer } from '../src/server.js';

let server: ForgeServer;
let base: string;
let tmpDir: string;

beforeAll(async () => {
  tmpDir = fs.mkdtempSync(path.join(os.tmpdir(), 'forge-http-'));
  server = buildServer({ dataDir: tmpDir });
  const port = await server.listen(0);
  base = `http://127.0.0.1:${port}`;
});

afterAll(async () => {
  await server.close();
  fs.rmSync(tmpDir, { recursive: true, force: true });
});

describe('http api', () => {
  it('GET /api/forge/health 返回合法 HealthStatus', async () => {
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
  });

  it('sessions 生命周期:空表 → 创建 → 列表 1 条 → get 200', async () => {
    const empty = (await (await fetch(`${base}/api/forge/sessions`)).json()) as SessionSummary[];
    expect(empty).toEqual([]);

    const createdRes = await fetch(`${base}/api/forge/sessions`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ title: 'demo' }),
    });
    expect(createdRes.status).toBe(201);
    const created = (await createdRes.json()) as SessionSummary;
    expect(created.title).toBe('demo');
    expect(created.eventCount).toBe(0);

    const list = (await (await fetch(`${base}/api/forge/sessions`)).json()) as SessionSummary[];
    expect(list).toHaveLength(1);
    expect(list[0].id).toBe(created.id);

    const getRes = await fetch(`${base}/api/forge/sessions/${created.id}`);
    expect(getRes.status).toBe(200);
    expect(((await getRes.json()) as SessionSummary).id).toBe(created.id);
  });

  it('postMessage 返回双事件,events 端点可见 2 条', async () => {
    const created = (await (
      await fetch(`${base}/api/forge/sessions`, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({}),
      })
    ).json()) as SessionSummary;

    const msgRes = await fetch(`${base}/api/forge/sessions/${created.id}/messages`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ text: 'hello' }),
    });
    expect(msgRes.status).toBe(201);
    const { userEvent, assistantEvent } = (await msgRes.json()) as {
      userEvent: EventEnvelope;
      assistantEvent: EventEnvelope;
    };
    expect(userEvent.type).toBe('user/message');
    expect(assistantEvent.type).toBe('assistant/message');
    expect(isEventEnvelope(userEvent)).toBe(true);
    expect(isEventEnvelope(assistantEvent)).toBe(true);
    expect(assistantEvent.seq).toBe(userEvent.seq + 1);

    const events = (await (
      await fetch(`${base}/api/forge/sessions/${created.id}/events`)
    ).json()) as EventEnvelope[];
    expect(events).toHaveLength(2);
    expect(events.every(isEventEnvelope)).toBe(true);

    // 列表里的 eventCount 同步为 2
    const list = (await (await fetch(`${base}/api/forge/sessions`)).json()) as SessionSummary[];
    expect(list.find((s) => s.id === created.id)?.eventCount).toBe(2);
  });

  it('未知会话 404 SESSION_NOT_FOUND', async () => {
    const res = await fetch(`${base}/api/forge/sessions/no-such-id`);
    expect(res.status).toBe(404);
    expect(((await res.json()) as ApiError).error.code).toBe('SESSION_NOT_FOUND');

    const msgRes = await fetch(`${base}/api/forge/sessions/no-such-id/messages`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ text: 'x' }),
    });
    expect(msgRes.status).toBe(404);
    expect(((await msgRes.json()) as ApiError).error.code).toBe('SESSION_NOT_FOUND');
  });

  it('坏 JSON 400 BAD_JSON;text 非字符串 400 BAD_REQUEST;未知路由 404 NOT_FOUND', async () => {
    const created = (await (
      await fetch(`${base}/api/forge/sessions`, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({}),
      })
    ).json()) as SessionSummary;

    const badJson = await fetch(`${base}/api/forge/sessions/${created.id}/messages`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: '{oops',
    });
    expect(badJson.status).toBe(400);
    expect(((await badJson.json()) as ApiError).error.code).toBe('BAD_JSON');

    const badText = await fetch(`${base}/api/forge/sessions/${created.id}/messages`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ text: 123 }),
    });
    expect(badText.status).toBe(400);
    expect(((await badText.json()) as ApiError).error.code).toBe('BAD_REQUEST');

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
