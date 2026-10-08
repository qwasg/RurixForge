/**
 * @forge/protocol — 前后端共享 wire 契约。
 * 事件信封语义对标 11_API_CONTRACTS.md §3 与 dsh 的 append-only SessionEvent 日志模型。
 */

/** 会话事件类型(对标 dsh turn/step 生命周期,冻结子集) */
export * from './blender.js';
export * from './collaboration.js';

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

/** D-040:host 健康接口对上游 agentd `/health` 的探测结果(不可达 ok=false,不抛错)。 */
export interface AgentdProbe {
  ok: boolean;
  version?: string;
  uptimeSec?: number;
}

export interface HealthStatus {
  status: 'ok';
  service: 'forge-host';
  version: string;
  port: number;
  uptimeSec: number;
  time: string;
  /** D-040:本机登录用户名(账户卡 / 首页问候);取不到则缺省。 */
  user?: { name: string };
  /** D-040:运行平台(process.platform)。 */
  platform?: string;
  /** D-040:host 所用 Node 版本。 */
  node?: string;
  /** D-040:上游 agentd 探测;无代理插件(base profile)时缺省。 */
  agentd?: AgentdProbe;
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
export * from './editor.js';
export * from './shaderGraph.js';
