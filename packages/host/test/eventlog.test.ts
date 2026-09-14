import { describe, it, expect, beforeAll, afterAll } from 'vitest';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { isEventEnvelope } from '@forge/protocol';
import { createContext } from '../src/ctx.js';
import { configPlugin } from '../src/plugins/config.js';
import { eventlogPlugin, type Eventlog } from '../src/plugins/eventlog.js';

let tmpDir: string;
let eventlog: Eventlog;

beforeAll(() => {
  tmpDir = fs.mkdtempSync(path.join(os.tmpdir(), 'forge-eventlog-'));
  const ctx = createContext();
  ctx.plugin(configPlugin({ dataDir: tmpDir }));
  ctx.plugin(eventlogPlugin);
  eventlog = ctx.get<Eventlog>('eventlog');
});

afterAll(() => {
  fs.rmSync(tmpDir, { recursive: true, force: true });
});

describe('eventlog append-only', () => {
  it('seq 会话内单调递增,信封逐条通过 isEventEnvelope', () => {
    const sid = 's-1';
    const e0 = eventlog.append(sid, 'user/message', { text: 'a' });
    const e1 = eventlog.append(sid, 'assistant/message', { text: 'b' });
    const e2 = eventlog.append(sid, 'turn/end', {});
    expect([e0.seq, e1.seq, e2.seq]).toEqual([0, 1, 2]);
    for (const e of [e0, e1, e2]) expect(isEventEnvelope(e)).toBe(true);
    // ts 为 ISO8601 UTC
    expect(() => new Date(e0.ts)).not.toThrow();
    expect(e0.ts.endsWith('Z')).toBe(true);
  });

  it('JSONL 落盘行数正确,read 可重建', () => {
    const sid = 's-1';
    const file = path.join(tmpDir, 'sessions', `${sid}.jsonl`);
    expect(fs.existsSync(file)).toBe(true);
    const lines = fs.readFileSync(file, 'utf8').split('\n').filter((l) => l.trim() !== '');
    expect(lines).toHaveLength(3);

    const restored = eventlog.read(sid);
    expect(restored).toHaveLength(3);
    restored.forEach((e, i) => {
      expect(isEventEnvelope(e)).toBe(true);
      expect(e.seq).toBe(i);
    });
  });

  it('read 不存在会话返回 [];不同会话 seq 各自独立;list 元信息正确', () => {
    expect(eventlog.read('nope')).toEqual([]);
    const other = eventlog.append('s-2', 'user/message', { text: 'x' });
    expect(other.seq).toBe(0);

    const metas = eventlog.list();
    const m1 = metas.find((m) => m.sessionId === 's-1');
    const m2 = metas.find((m) => m.sessionId === 's-2');
    expect(m1?.eventCount).toBe(3);
    expect(m2?.eventCount).toBe(1);
    expect(m1?.firstTs).toBeDefined();
    expect(m1?.lastTs).toBeDefined();
  });
});
