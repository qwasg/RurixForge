import { useMemo } from 'react';
import { useChatStore, type ChatMsg } from './chatStore';
import { useEffectiveSpec } from './modelSpec';
import type { ChatBlock } from './timeline';

/**
 * Composer 上下文窗口计量(灰环 + 百分比 + 胶囊上方面板)的数据面。
 *
 * - 窗口大小:Codex 的 agent.usage 带 modelContextWindow 时用它;否则由调用方给当前 Context 档
 *   (modelSpec.ts,agentd modelspec.rs 随 models[] 下发、人在模型菜单里选);
 * - 面板只分三行(2026-10-07 用户:明细太碎、字看不清):系统提示与工具 / 对话历史 / 待发送。
 *   文件、工具结果、思考都并进「对话历史」——它们是同一份历史,也是「压缩上下文」能腾出的部分;
 * - 会话收到过 agent.usage 时以实测为准:最近一次请求送进模型的 prompt(Codex 取 last,不取线程累计),
 *   其中按静态基线切出系统提示与工具,其余归对话历史;mock provider 不发 usage,恒走估算;
 * - 估算:最近一次压缩(context.compacted)之后的消息 + 摘要体量;token 走 DeepSeek 官方口径
 *   (中文字符 ≈ 0.6 token,其余 ≈ 0.3 token),不是分词器精确值,面板以「约」标注。
 */

/** 中日韩表意字与全角标点(逐字 0.6 token 档)。 */
const CJK_RE =
  /[\u3000-\u303f\u3040-\u30ff\u3400-\u4dbf\u4e00-\u9fff\uf900-\ufaff\uff00-\uffef]/;

/**
 * 系统提示 + 工具定义静态基线(tokens)。
 * 依据:llm.rs SYSTEM_PROMPT ≈ 80 token;工具定义为 MCP(engine-scene 43 / asset-pipeline 15 /
 * code-forge 11 / gen-image 6 / gen-model 6)加运行时原生工具约 90 条 inputSchema,
 * 单条 JSON schema ≈ 120 token → ≈ 10.8k;按 agentKind/mode 裁剪后取 8000 作保守基线。
 * 仅在会话尚无 agent.usage 实测时生效。
 */
export const BASE_PROMPT_TOKENS = 8000;

/** 模型未下发窗口档时的回落(定义在 modelSpec.ts,此处转出供计量面与其测试直接引用)。 */
export { DEFAULT_CONTEXT_WINDOW } from './modelSpec';

/** DeepSeek 官方估算口径:中文 0.6 / 其余 0.3,向上取整。 */
export function estimateTokens(text: string): number {
  if (text === '') return 0;
  let cjk = 0;
  let other = 0;
  for (const ch of text) {
    if (CJK_RE.test(ch)) cjk += 1;
    else other += 1;
  }
  return Math.ceil(cjk * 0.6 + other * 0.3);
}

/** 1234 → 1.2k;65536 → 66k;1048576 → 1M(与模型菜单的窗口档同写法;面板与环共用)。 */
export function formatTokens(n: number): string {
  if (n < 1000) return String(n);
  if (n < 10000) return `${(n / 1000).toFixed(1)}k`;
  if (n < 1_000_000) return `${Math.round(n / 1000)}k`;
  return `${Number((n / 1_000_000).toFixed(n < 10_000_000 ? 1 : 0))}M`;
}

/** 面板三行:系统提示与工具 / 对话历史 / 待发送(草稿 + 已选技能)。 */
export type ContextRowKind = 'base' | 'history' | 'input';

export interface ContextRow {
  kind: ContextRowKind;
  label: string;
  tokens: number;
}

export interface ContextUsage {
  /** 固定顺序 base → history → input;零占用行已剔除。 */
  rows: ContextRow[];
  used: number;
  window: number;
  /** used/window,clamp 0–1(圆环填充用;超窗封顶)。 */
  ratio: number;
  /** 百分比整数,不 clamp(> 100 即已超窗)。 */
  percent: number;
  /** 最近一次请求实测的 prompt tokens(0 = 本会话无实测或刚压缩过)。 */
  measured: number;
  /** 是否按实测校准。 */
  calibrated: boolean;
  /** 最近一次压缩之后的消息数(0 = 没有可压缩的新对话)。 */
  messageCount: number;
  /** 本会话是否压缩过。 */
  compacted: boolean;
}

export interface HistoryScan {
  /** 用户消息、助手正文与子代理派单/回执。 */
  messageTokens: number;
  /** 工具调用的参数与结果(含读写文件)。 */
  toolTokens: number;
  reasoningTokens: number;
  messageCount: number;
  /** 三类之和(未校准时即对话历史的估算)。 */
  total: number;
}

/** 消息树全量扫描(子代理 work 递归计入,与真实随轮次回传的历史同口径)。 */
export function scanHistory(messages: ChatMsg[]): HistoryScan {
  let messageTokens = 0;
  let toolTokens = 0;
  let reasoningTokens = 0;

  const walk = (blocks: ChatBlock[]): void => {
    for (const b of blocks) {
      switch (b.kind) {
        case 'text':
          messageTokens += estimateTokens(b.text);
          break;
        case 'reasoning':
          reasoningTokens += estimateTokens(b.text);
          break;
        case 'tool':
          toolTokens +=
            estimateTokens(b.args) + estimateTokens(b.result ?? '') + estimateTokens(b.error ?? '');
          break;
        case 'subagent':
          messageTokens += estimateTokens(b.prompt ?? '') + estimateTokens(b.summary ?? '');
          walk(b.work);
          break;
      }
    }
  };

  for (const m of messages) {
    if (m.role === 'user') messageTokens += estimateTokens(m.text);
    walk(m.blocks);
  }

  return {
    messageTokens,
    toolTokens,
    reasoningTokens,
    messageCount: messages.length,
    total: messageTokens + toolTokens + reasoningTokens,
  };
}

export interface PendingContext {
  /** 已选技能名(发送时经 ask:execute 结构化 skills[] 下发,不再拼文本前缀)。 */
  skills: string[];
  /** Composer 草稿正文。 */
  draft: string;
  /** 计量环分母(Codex 实测窗口,或当前 Context 档的 tokens)。 */
  contextWindow: number;
  /** chatStore.lastPromptTokens。 */
  measured: number;
  /** 最近一次压缩后的摘要体量(本地引擎回报;无摘要或未知 = 0)。 */
  summaryTokens?: number;
  /** 本会话是否压缩过。 */
  compacted?: boolean;
}

export function assembleUsage(scan: HistoryScan, pending: PendingContext): ContextUsage {
  const calibrated = pending.measured > 0;
  // 实测 = 最近一次请求送进模型的全部内容:按静态基线切出系统提示与工具,其余都是对话历史。
  const base = calibrated ? Math.min(BASE_PROMPT_TOKENS, pending.measured) : BASE_PROMPT_TOKENS;
  const history = calibrated
    ? pending.measured - base
    : scan.total + (pending.summaryTokens ?? 0);
  const input =
    estimateTokens(pending.draft) +
    pending.skills.reduce((sum, name) => sum + estimateTokens(name) + 2, 0);

  const rows: ContextRow[] = [
    { kind: 'base' as const, label: '系统提示与工具', tokens: base },
    { kind: 'history' as const, label: '对话历史', tokens: history },
    { kind: 'input' as const, label: '待发送', tokens: input },
  ].filter((r) => r.tokens > 0);
  const used = base + history + input;
  const limit = pending.contextWindow;

  return {
    rows,
    used,
    window: limit,
    ratio: Math.min(1, used / limit),
    percent: Math.round((used / limit) * 100),
    measured: pending.measured,
    calibrated,
    messageCount: scan.messageCount,
    compacted: pending.compacted ?? false,
  };
}

/** 最近一个压缩分隔点之后的消息(无分隔点,或分隔点已不在列表里 → 全部)。 */
export function messagesAfter(messages: ChatMsg[], afterId: string | null | undefined): ChatMsg[] {
  if (!afterId) return messages;
  const i = messages.findIndex((m) => m.id === afterId);
  return i < 0 ? messages : messages.slice(i + 1);
}

export function computeContextUsage(
  messages: ChatMsg[],
  pending: PendingContext,
): ContextUsage {
  return assembleUsage(scanHistory(messages), pending);
}

/** 面板与环上的百分比文案:有占用但不足 1% 时写「<1%」,不显示成 0%。 */
export function percentLabel(usage: ContextUsage): string {
  return usage.used > 0 && usage.percent < 1 ? '<1%' : `${usage.percent}%`;
}

/**
 * 订阅式装配。历史扫描与待发段分两段 memo:
 * 逐键入草稿只重算装配段,长会话消息树不随每次按键重扫。
 */
export function useContextUsage(skills: string[], draft: string): ContextUsage {
  const messages = useChatStore((st) => st.messages);
  const measured = useChatStore((st) => st.lastPromptTokens);
  const usageWindow = useChatStore((st) => st.usageContextWindow);
  const mark = useChatStore((st) => st.compactions[st.compactions.length - 1]);
  const specWindow = useEffectiveSpec().contextTokens;
  const contextWindow = usageWindow ?? specWindow;

  const scan = useMemo(
    () => scanHistory(messagesAfter(messages, mark?.afterMessageId)),
    [messages, mark],
  );
  return useMemo(
    () =>
      assembleUsage(scan, {
        skills,
        draft,
        contextWindow,
        measured,
        summaryTokens: mark?.summaryTokens ?? 0,
        compacted: mark !== undefined,
      }),
    [scan, skills, draft, contextWindow, measured, mark],
  );
}
