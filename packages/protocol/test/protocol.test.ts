import { describe, expect, it } from 'vitest';
import {
  SESSION_EVENT_TYPES,
  isEventEnvelope,
  isSessionEventType,
  type EventEnvelope,
} from '../src/index.js';

describe('SESSION_EVENT_TYPES', () => {
  it('非空且无重复', () => {
    expect(SESSION_EVENT_TYPES.length).toBeGreaterThan(0);
    expect(new Set(SESSION_EVENT_TYPES).size).toBe(SESSION_EVENT_TYPES.length);
  });

  it('覆盖 turn/step 生命周期锚点', () => {
    for (const t of ['turn/start', 'turn/end', 'step/start', 'step/end', 'user/message', 'assistant/message']) {
      expect(SESSION_EVENT_TYPES).toContain(t);
    }
  });
});

describe('isSessionEventType', () => {
  it('接受合法类型,拒绝非法类型', () => {
    expect(isSessionEventType('tool/call')).toBe(true);
    expect(isSessionEventType('tool/execute')).toBe(false);
    expect(isSessionEventType(42)).toBe(false);
    expect(isSessionEventType(undefined)).toBe(false);
  });
});

describe('isEventEnvelope', () => {
  const good: EventEnvelope = {
    id: 'evt_1',
    sessionId: 'ses_1',
    seq: 0,
    type: 'user/message',
    ts: new Date().toISOString(),
    payload: { text: 'hi' },
  };

  it('接受合法信封', () => {
    expect(isEventEnvelope(good)).toBe(true);
  });

  it('拒绝缺字段/坏类型/负 seq', () => {
    expect(isEventEnvelope(null)).toBe(false);
    expect(isEventEnvelope({})).toBe(false);
    expect(isEventEnvelope({ ...good, type: 'bogus' })).toBe(false);
    expect(isEventEnvelope({ ...good, seq: -1 })).toBe(false);
    expect(isEventEnvelope({ ...good, seq: 1.5 })).toBe(false);
    const { payload: _drop, ...noPayload } = good;
    expect(isEventEnvelope(noPayload)).toBe(false);
  });
});
