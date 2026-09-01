import fs from 'node:fs';
import path from 'node:path';
import { randomUUID } from 'node:crypto';
import { isEventEnvelope, type EventEnvelope, type SessionEventType } from '@forge/protocol';
import type { PluginFn } from '../ctx.js';
import type { HostConfig } from './config.js';

/** sessions 目录内单个 jsonl 的元信息 */
export interface EventlogMeta {
  sessionId: string;
  eventCount: number;
  firstTs?: string;
  lastTs?: string;
}

export interface Eventlog {
  append(sessionId: string, type: SessionEventType, payload: unknown): EventEnvelope;
  read(sessionId: string): EventEnvelope[];
  list(): EventlogMeta[];
}

/**
 * append-only 事件日志:每会话一个 jsonl,一行一 JSON。
 * seq 会话内单调递增;只增不改,无修改/删除 API。
 */
export const eventlogPlugin: PluginFn = (ctx) => {
  const config = ctx.get<HostConfig>('config');
  const sessionsDir = path.join(config.dataDir, 'sessions');
  // 会话内下一个 seq 的缓存;未命中时从磁盘行数恢复
  const seqCache = new Map<string, number>();

  function fileOf(sessionId: string): string {
    return path.join(sessionsDir, `${sessionId}.jsonl`);
  }

  function read(sessionId: string): EventEnvelope[] {
    const file = fileOf(sessionId);
    if (!fs.existsSync(file)) return [];
    const lines = fs.readFileSync(file, 'utf8').split('\n').filter((l) => l.trim() !== '');
    return lines.map((l) => JSON.parse(l) as EventEnvelope);
  }

  function nextSeq(sessionId: string): number {
    let n = seqCache.get(sessionId);
    if (n === undefined) n = read(sessionId).length;
    seqCache.set(sessionId, n + 1);
    return n;
  }

  ctx.provide('eventlog', {
    append(sessionId, type, payload) {
      const envelope: EventEnvelope = {
        id: randomUUID(),
        sessionId,
        seq: nextSeq(sessionId),
        type,
        ts: new Date().toISOString(),
        payload,
      };
      fs.mkdirSync(sessionsDir, { recursive: true });
      fs.appendFileSync(fileOf(sessionId), JSON.stringify(envelope) + '\n', 'utf8');
      return envelope;
    },
    read,
    list() {
      if (!fs.existsSync(sessionsDir)) return [];
      return fs
        .readdirSync(sessionsDir)
        .filter((f) => f.endsWith('.jsonl'))
        .map((f) => {
          const sessionId = f.slice(0, -'.jsonl'.length);
          const events = read(sessionId).filter(isEventEnvelope);
          return {
            sessionId,
            eventCount: events.length,
            firstTs: events[0]?.ts,
            lastTs: events[events.length - 1]?.ts,
          } satisfies EventlogMeta;
        });
    },
  } satisfies Eventlog);
};
