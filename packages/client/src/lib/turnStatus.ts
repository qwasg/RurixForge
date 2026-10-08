import { toolStatus, type ChatBlock } from './timeline';

/**
 * D-047 轮次状态行(Cursor 式「调用等待」显示)的纯状态机。
 *
 * 助手轮在跑但眼前没有任何「正在执行」的行时(工具刚收尾、模型还没吐下一步;或刚发出、
 * 助手卡还没出现),消息末尾挂一行状态,说清楚 agent 现在卡在哪:
 *
 *   连接(最高优先,断了其余状态都不可信)
 *     reconnecting  事件流断开 ≥ RECONNECT_GRACE_MS,正在退避重连   「Reconnecting」
 *     offline       断开 ≥ OFFLINE_AFTER_MS / host 健康探测失败 / 浏览器离线 「Connection lost」
 *   等用户
 *     approval      有未决审批卡                                  「Waiting for approval」
 *     input         未决的是提问 / 表单(userInput / elicitation)    「Waiting for your input」
 *   等模型
 *     planning      空档已过去抖窗口                               「Planning next moves」
 *     slow          空档 ≥ SLOW_AFTER_MS                          「Taking longer than expected · {n}s」
 *
 * 有行在执行(运行中的工具 / 子代理、思考中、计划在写、正文仍在流)时返回 null——那一行自己
 * 扫光,状态行不重复。live = 是否「正在执行」(扫光);等用户与断线是静止态,不扫。
 * 过程链文案统一英文(E-07-008),与 Thinking / Exploring 同列。
 */

export type TurnStatusKind = 'planning' | 'slow' | 'approval' | 'input' | 'reconnecting' | 'offline';

export interface TurnStatus {
  kind: TurnStatusKind;
  label: string;
  /** 浅一档的补充(秒数 / 原因);无则空串。 */
  detail: string;
  /** 正在执行 → 扫光;等用户 / 断线 → 静止。 */
  live: boolean;
}

export interface TurnLink {
  /** 会话事件流(SSE)掉线、正在退避重连。 */
  down: boolean;
  /** 掉线持续时长(ms;未掉线为 0)。 */
  downForMs: number;
  /** host 健康探测失败(systemStore 5s 轮询)。 */
  hostOffline: boolean;
  /** 浏览器报告离线(navigator.onLine === false):模型在远端,本地事件流通也等不来回复。 */
  networkOffline: boolean;
}

export interface TurnStatusInput {
  /** 本轮已有的时间线块(助手卡还没出现时为空数组)。 */
  blocks: ChatBlock[];
  /** 距本轮最后一次可见进展(块变化;助手卡出现前从发出时刻算起)的毫秒数。 */
  idleMs: number;
  link: TurnLink;
}

/** 工具 / 审批收尾后,多久没有下一步才出「Planning next moves」(连续工具调用之间不闪)。 */
export const PLANNING_SETTLE_MS = 400;
/** 正文停流多久才算「在想下一步」(流式偶有半秒级停顿,不能一停就跳状态行)。 */
export const TEXT_SETTLE_MS = 1500;
/** 空档超过即升级为「Taking longer than expected」。 */
export const SLOW_AFTER_MS = 15_000;
/** 事件流断开多久才露出「Reconnecting」(正常重连亚秒级,不闪)。 */
export const RECONNECT_GRACE_MS = 1500;
/** 断开超过即升级为「Connection lost」。 */
export const OFFLINE_AFTER_MS = 10_000;

const NONE = '';

/** 未决审批(含嵌套子代理里的);返回其类别。 */
export function pendingApprovalKind(blocks: ChatBlock[]): 'approval' | 'input' | null {
  for (const block of blocks) {
    if (block.kind === 'approval' && block.decision === undefined) {
      return block.approvalKind === 'userInput' || block.approvalKind === 'elicitation' ? 'input' : 'approval';
    }
    if (block.kind === 'subagent') {
      const nested = pendingApprovalKind(block.work);
      if (nested) return nested;
    }
  }
  return null;
}

/** 有行正在执行(它自己扫光,状态行让位)。 */
function hasLiveRow(blocks: ChatBlock[], idleMs: number): boolean {
  const running = blocks.some(
    (b) => (b.kind === 'tool' && toolStatus(b) === 'running') || (b.kind === 'subagent' && b.status === 'running'),
  );
  if (running) return true;
  const tail = blocks[blocks.length - 1];
  if (!tail) return false;
  if (tail.kind === 'reasoning') return true; // Thinking 行在扫
  if (tail.kind === 'plan' && !tail.final) return true; // Plan · Writing 在扫
  return tail.kind === 'text' && !tail.final && idleMs < TEXT_SETTLE_MS;
}

export function deriveTurnStatus({ blocks, idleMs, link }: TurnStatusInput): TurnStatus | null {
  if (link.networkOffline) {
    return { kind: 'offline', label: 'Connection lost', detail: 'network offline', live: false };
  }
  if (link.hostOffline || (link.down && link.downForMs >= OFFLINE_AFTER_MS)) {
    return { kind: 'offline', label: 'Connection lost', detail: 'retrying', live: false };
  }
  if (link.down && link.downForMs >= RECONNECT_GRACE_MS) {
    return { kind: 'reconnecting', label: 'Reconnecting', detail: NONE, live: true };
  }
  const waiting = pendingApprovalKind(blocks);
  if (waiting === 'input') return { kind: 'input', label: 'Waiting for your input', detail: NONE, live: false };
  if (waiting === 'approval') return { kind: 'approval', label: 'Waiting for approval', detail: NONE, live: false };
  if (hasLiveRow(blocks, idleMs)) return null;
  const tail = blocks[blocks.length - 1];
  // 刚开轮(还没有任何块)立刻出;正文收尾按正文去抖;其余(工具 / 卡片收尾)短去抖。
  const settle = !tail ? 0 : tail.kind === 'text' ? TEXT_SETTLE_MS : PLANNING_SETTLE_MS;
  if (idleMs < settle) return null;
  if (idleMs >= SLOW_AFTER_MS) {
    return { kind: 'slow', label: 'Taking longer than expected', detail: `${Math.floor(idleMs / 1000)}s`, live: true };
  }
  return { kind: 'planning', label: 'Planning next moves', detail: NONE, live: true };
}
