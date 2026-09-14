/**
 * @forge/protocol — 前后端共享 wire 契约。
 * 事件信封语义对标 11_API_CONTRACTS.md §3 与 dsh 的 append-only SessionEvent 日志模型。
 */

/** 会话事件类型(对标 dsh turn/step 生命周期,冻结子集) */
export * from './blender.js';

export const SESSION_EVENT_TYPES = [
  'turn/start',
  'step/start',
  'user/message',
  'assistant/chunk',
  'assistant/message',
  'tool/call',
  'tool/result',
  'step/end',
  'turn/end',
] as const;

export type SessionEventType = (typeof SESSION_EVENT_TYPES)[number];

/** append-only 事件信封:seq 在会话内单调递增,落盘即事实源 */
export interface EventEnvelope<T = unknown> {
  id: string;
  sessionId: string;
  seq: number;
  type: SessionEventType;
  /** ISO8601 UTC */
  ts: string;
  payload: T;
}

export interface SessionSummary {
  id: string;
  title: string;
  createdAt: string;
  updatedAt: string;
  eventCount: number;
}

export interface CreateSessionRequest {
  title?: string;
}

export interface HealthStatus {
  status: 'ok';
  service: 'forge-host';
  version: string;
  port: number;
  uptimeSec: number;
  time: string;
}

/** 结构化错误(I-5 不静默回退) */
export interface ApiError {
  error: {
    code: string;
    message: string;
  };
}

export function isSessionEventType(x: unknown): x is SessionEventType {
  return typeof x === 'string' && (SESSION_EVENT_TYPES as readonly string[]).includes(x);
}

export function isEventEnvelope(x: unknown): x is EventEnvelope {
  if (typeof x !== 'object' || x === null) return false;
  const e = x as Record<string, unknown>;
  return (
    typeof e.id === 'string' &&
    typeof e.sessionId === 'string' &&
    typeof e.seq === 'number' &&
    Number.isInteger(e.seq) &&
    e.seq >= 0 &&
    isSessionEventType(e.type) &&
    typeof e.ts === 'string' &&
    'payload' in e
  );
}
