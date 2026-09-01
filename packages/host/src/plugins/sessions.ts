import fs from 'node:fs';
import path from 'node:path';
import { randomUUID } from 'node:crypto';
import type { EventEnvelope, SessionSummary } from '@forge/protocol';
import type { PluginFn } from '../ctx.js';
import type { HostConfig } from './config.js';
import type { Eventlog } from './eventlog.js';

/** 落盘的会话元信息(eventCount 由 eventlog 实测,不冗余存储) */
interface SessionMeta {
  id: string;
  title: string;
  createdAt: string;
  updatedAt: string;
}

export interface PostMessageResult {
  userEvent: EventEnvelope;
  assistantEvent: EventEnvelope;
}

export interface Sessions {
  create(title?: string): SessionSummary;
  list(): SessionSummary[];
  get(id: string): SessionSummary | undefined;
  postMessage(id: string, text: string): PostMessageResult | undefined;
}

/** 会话插件:元信息持久化 <dataDir>/sessions/<id>.meta.json,重启可恢复 */
export const sessionsPlugin: PluginFn = (ctx) => {
  const config = ctx.get<HostConfig>('config');
  const eventlog = ctx.get<Eventlog>('eventlog');
  const sessionsDir = path.join(config.dataDir, 'sessions');

  function metaFile(id: string): string {
    return path.join(sessionsDir, `${id}.meta.json`);
  }

  function saveMeta(meta: SessionMeta): void {
    fs.mkdirSync(sessionsDir, { recursive: true });
    fs.writeFileSync(metaFile(meta.id), JSON.stringify(meta, null, 2), 'utf8');
  }

  function loadMeta(id: string): SessionMeta | undefined {
    const file = metaFile(id);
    if (!fs.existsSync(file)) return undefined;
    return JSON.parse(fs.readFileSync(file, 'utf8')) as SessionMeta;
  }

  function toSummary(meta: SessionMeta): SessionSummary {
    return { ...meta, eventCount: eventlog.read(meta.id).length };
  }

  ctx.provide('sessions', {
    create(title) {
      const now = new Date().toISOString();
      const id = randomUUID();
      const meta: SessionMeta = {
        id,
        title: title ?? `session-${id.slice(0, 8)}`,
        createdAt: now,
        updatedAt: now,
      };
      saveMeta(meta);
      return toSummary(meta);
    },
    list() {
      if (!fs.existsSync(sessionsDir)) return [];
      return fs
        .readdirSync(sessionsDir)
        .filter((f) => f.endsWith('.meta.json'))
        .map((f) => loadMeta(f.slice(0, -'.meta.json'.length))!)
        .sort((a, b) => a.createdAt.localeCompare(b.createdAt))
        .map(toSummary);
    },
    get(id) {
      const meta = loadMeta(id);
      return meta ? toSummary(meta) : undefined;
    },
    postMessage(id, text) {
      const meta = loadMeta(id);
      if (!meta) return undefined;
      const userEvent = eventlog.append(id, 'user/message', { text });
      // mock seam:真实 provider 接入点,当前为 echo 占位
      const assistantEvent = eventlog.append(id, 'assistant/message', { text: `echo: ${text}` });
      meta.updatedAt = new Date().toISOString();
      saveMeta(meta);
      return { userEvent, assistantEvent };
    },
  } satisfies Sessions);
};
