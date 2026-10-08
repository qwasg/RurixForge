import { describe, expect, it } from 'vitest';
import type { ChatBlock } from '@/lib/timeline';
import {
  deriveTurnStatus,
  OFFLINE_AFTER_MS,
  PLANNING_SETTLE_MS,
  RECONNECT_GRACE_MS,
  SLOW_AFTER_MS,
  TEXT_SETTLE_MS,
  type TurnLink,
} from '@/lib/turnStatus';

/** D-047 轮次状态行纯状态机:连接 > 等用户 > 有行在执行(让位)> 去抖 > Planning / Taking longer。 */

const LIVE_LINK: TurnLink = { down: false, downForMs: 0, hostOffline: false, networkOffline: false };

function tool(over: Partial<Extract<ChatBlock, { kind: 'tool' }>> = {}): ChatBlock {
  return { kind: 'tool', toolCallId: 'c1', name: 'read_file', args: '{}', mcp: null, ok: true, ...over };
}

const derive = (blocks: ChatBlock[], idleMs: number, link: Partial<TurnLink> = {}) =>
  deriveTurnStatus({ blocks, idleMs, link: { ...LIVE_LINK, ...link } });

describe('deriveTurnStatus', () => {
  it('刚开轮(无块)立即 Planning next moves,扫光', () => {
    expect(derive([], 0)).toEqual({ kind: 'planning', label: 'Planning next moves', detail: '', live: true });
  });

  it('有行在执行时让位:运行中的工具 / 子代理、思考中、计划在写、正文仍在流', () => {
    expect(derive([tool({ ok: undefined })], 60_000)).toBeNull();
    expect(derive([{ kind: 'subagent', id: 's', label: 'x', status: 'running', work: [] }], 60_000)).toBeNull();
    expect(derive([{ kind: 'reasoning', text: '…' }], 60_000)).toBeNull();
    expect(derive([{ kind: 'plan', text: '# p', final: false }], 60_000)).toBeNull();
    expect(derive([{ kind: 'text', text: '正在说', final: false }], TEXT_SETTLE_MS - 1)).toBeNull();
    // 并行工具:末块已完成、更早的还在跑 → 仍让位
    expect(derive([tool({ toolCallId: 'a', ok: undefined }), tool({ toolCallId: 'b' })], 60_000)).toBeNull();
  });

  it('工具收尾后短去抖再出 Planning(连续调用之间不闪);正文停流按更长的正文去抖', () => {
    expect(derive([tool()], PLANNING_SETTLE_MS - 1)).toBeNull();
    expect(derive([tool()], PLANNING_SETTLE_MS)?.kind).toBe('planning');
    const text: ChatBlock = { kind: 'text', text: '先看一下', final: false };
    expect(derive([text], PLANNING_SETTLE_MS)).toBeNull();
    expect(derive([text], TEXT_SETTLE_MS)?.kind).toBe('planning');
  });

  it('空档过久升级为 Taking longer than expected,补充为空档秒数', () => {
    expect(derive([tool()], SLOW_AFTER_MS - 1)?.kind).toBe('planning');
    expect(derive([tool()], SLOW_AFTER_MS + 2_400)).toEqual({
      kind: 'slow',
      label: 'Taking longer than expected',
      detail: '17s',
      live: true,
    });
    expect(derive([], SLOW_AFTER_MS)?.kind).toBe('slow');
  });

  it('未决审批(含嵌套子代理里的)→ 等用户,静止不扫;提问 / 表单单列文案;已决不算', () => {
    const approval = (approvalKind: string, decision?: string): ChatBlock => ({
      kind: 'approval',
      id: 'p1',
      approvalKind,
      ...(decision ? { decision } : {}),
    });
    expect(derive([approval('command')], 0)).toEqual({
      kind: 'approval',
      label: 'Waiting for approval',
      detail: '',
      live: false,
    });
    expect(derive([approval('userInput')], 0)?.label).toBe('Waiting for your input');
    expect(derive([approval('elicitation')], 0)?.kind).toBe('input');
    const nested: ChatBlock = { kind: 'subagent', id: 's', label: 'x', status: 'running', work: [approval('fileChange')] };
    expect(derive([nested], 0)?.kind).toBe('approval');
    expect(derive([approval('command', 'accept')], PLANNING_SETTLE_MS)?.kind).toBe('planning');
  });

  it('事件流掉线:宽限内不露,超宽限 Reconnecting(扫光),再久升级 Connection lost', () => {
    expect(derive([tool({ ok: undefined })], 0, { down: true, downForMs: RECONNECT_GRACE_MS - 1 })).toBeNull();
    expect(derive([tool({ ok: undefined })], 0, { down: true, downForMs: RECONNECT_GRACE_MS })).toEqual({
      kind: 'reconnecting',
      label: 'Reconnecting',
      detail: '',
      live: true,
    });
    expect(derive([], 0, { down: true, downForMs: OFFLINE_AFTER_MS })).toEqual({
      kind: 'offline',
      label: 'Connection lost',
      detail: 'retrying',
      live: false,
    });
  });

  it('连接优先于等用户;host 探测失败 / 浏览器离线直接 Connection lost', () => {
    const pending: ChatBlock = { kind: 'approval', id: 'p1', approvalKind: 'command' };
    expect(derive([pending], 0, { hostOffline: true })?.kind).toBe('offline');
    expect(derive([pending], 0, { networkOffline: true })).toEqual({
      kind: 'offline',
      label: 'Connection lost',
      detail: 'network offline',
      live: false,
    });
  });
});
