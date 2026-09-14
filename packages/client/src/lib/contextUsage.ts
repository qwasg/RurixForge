import { useMemo } from 'react';
import { useChatStore, type ChatMsg } from './chatStore';
import { useEffectiveSpec } from './modelSpec';
import { editTargetFiles, toolVisual, type ChatBlock } from './timeline';

/**
 * Composer 上下文窗口计量(灰环 + 百分比 + 胶囊上方明细表)的数据面。
 *
 * - 窗口大小:由调用方给 contextWindow(Composer 传当前 Context 档的 tokens,见 modelSpec.ts)。
 *   模型规格波之前这里是写死的 64K 静态表 + 后端不回窗口的数据缺口留痕,现在窗口档由
 *   agentd modelspec.rs 随 models[] 下发、由人在模型菜单里选,缺口已补;
 * - 明细行:文件(工具 args/结果按目标路径归并)、工具结果、对话消息、思考过程、
 *   已选技能、当前草稿——全部由 chatStore 消息树与 Composer 本地态派生;
 * - 「系统提示与工具定义」行:会话收到过 agent.usage 时 = 实测 promptTokens − 可归因估算
 *   (倒算,把工具 schema 与协议开销如实归到这一行),否则 = BASE_PROMPT_TOKENS 静态基线;
 *   mock provider 不发 usage(llm.rs Usage 注),该腿恒走基线;
 * - token 估算走 DeepSeek 官方口径(中文字符 ≈ 0.6 token,其余字符 ≈ 0.3 token),
 *   是估算不是分词器精确值,面板底注如实标注。
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

/** 1234 → 1.2k;65536 → 66k(表格与胶囊下方钮共用)。 */
export function formatTokens(n: number): string {
  if (n < 1000) return String(n);
  if (n < 10000) return `${(n / 1000).toFixed(1)}k`;
  return `${Math.round(n / 1000)}k`;
}

export type ContextRowKind =
  | 'base'
  | 'file'
  | 'tool'
  | 'message'
  | 'reasoning'
  | 'skill'
  | 'draft';

export interface ContextRow {
  key: string;
  kind: ContextRowKind;
  /** 表格主列(文件取 basename,超长截断由渲染层负责)。 */
  label: string;
  /** 副行/悬停全文(文件全路径、工具调用次数等)。 */
  detail: string;
  tokens: number;
}

export interface ContextUsage {
  /** 按 tokens 降序;零占用行已剔除。 */
  rows: ContextRow[];
  used: number;
  window: number;
  /** used/window,clamp 0–1(圆环填充用;超窗封顶)。 */
  ratio: number;
  /** 百分比整数,不 clamp(> 100 即已超窗)。 */
  percent: number;
  /** 最近一次 agent.usage 的 promptTokens(0 = 本会话无实测)。 */
  measured: number;
  /** 基线行是否由实测倒算。 */
  calibrated: boolean;
}

interface Bucket {
  tokens: number;
  calls: number;
}

export interface HistoryScan {
  files: Array<{ path: string } & Bucket>;
  tools: Array<{ name: string } & Bucket>;
  messageTokens: number;
  messageCount: number;
  reasoningTokens: number;
  /** files + tools + messages + reasoning(倒算基线时的可归因部分)。 */
  total: number;
}

function baseName(p: string): string {
  const i = Math.max(p.lastIndexOf('/'), p.lastIndexOf('\\'));
  return i >= 0 ? p.slice(i + 1) : p;
}

function bump(map: Map<string, Bucket>, key: string, tokens: number): void {
  const cur = map.get(key);
  if (cur) {
    cur.tokens += tokens;
    cur.calls += 1;
  } else {
    map.set(key, { tokens, calls: 1 });
  }
}

/** 消息树全量扫描(子代理 work 递归计入,与真实随轮次回传的历史同口径)。 */
export function scanHistory(messages: ChatMsg[]): HistoryScan {
  const files = new Map<string, Bucket>();
  const tools = new Map<string, Bucket>();
  let messageTokens = 0;
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
        case 'tool': {
          const t =
            estimateTokens(b.args) + estimateTokens(b.result ?? '') + estimateTokens(b.error ?? '');
          // editTargetFiles = TARGET_KEYS(path/file/filePath/scenePath/assetPath/destFolder)
          // 键序取一,对读写类工具同样成立;取不到路径的归工具结果桶。
          const [path] = editTargetFiles(b.args);
          if (path) bump(files, path, t);
          else bump(tools, toolVisual(b.name), t);
          break;
        }
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

  const fileRows = [...files.entries()].map(([path, b]) => ({ path, ...b }));
  const toolRows = [...tools.entries()].map(([name, b]) => ({ name, ...b }));
  const total =
    fileRows.reduce((s, r) => s + r.tokens, 0) +
    toolRows.reduce((s, r) => s + r.tokens, 0) +
    messageTokens +
    reasoningTokens;

  return {
    files: fileRows,
    tools: toolRows,
    messageTokens,
    messageCount: messages.length,
    reasoningTokens,
    total,
  };
}

export interface PendingContext {
  /** 已选技能名(发送时经 ask:execute 结构化 skills[] 下发,不再拼文本前缀)。 */
  skills: string[];
  /** Composer 草稿正文。 */
  draft: string;
  /** 计量环分母 = 当前 Context 档的 tokens(modelSpec.resolveModelSpec 归一后)。 */
  contextWindow: number;
  /** chatStore.lastPromptTokens。 */
  measured: number;
}

export function assembleUsage(scan: HistoryScan, pending: PendingContext): ContextUsage {
  const calibrated = pending.measured > 0;
  const base = calibrated ? Math.max(0, pending.measured - scan.total) : BASE_PROMPT_TOKENS;

  const rows: ContextRow[] = [
    {
      key: 'base',
      kind: 'base',
      label: '系统提示与工具定义',
      detail: calibrated ? `由实测 prompt ${pending.measured} 倒算` : '静态基线（本会话尚无实测）',
      tokens: base,
    },
  ];

  for (const f of scan.files) {
    rows.push({
      key: `file:${f.path}`,
      kind: 'file',
      label: baseName(f.path),
      detail: `${f.path} · ${f.calls} 次读写`,
      tokens: f.tokens,
    });
  }
  for (const t of scan.tools) {
    rows.push({
      key: `tool:${t.name}`,
      kind: 'tool',
      label: t.name,
      detail: `${t.calls} 次调用的参数与结果`,
      tokens: t.tokens,
    });
  }
  rows.push({
    key: 'messages',
    kind: 'message',
    label: '对话消息',
    detail: `${scan.messageCount} 条消息正文`,
    tokens: scan.messageTokens,
  });
  rows.push({
    key: 'reasoning',
    kind: 'reasoning',
    label: '思考过程',
    detail: '助手 reasoning 块',
    tokens: scan.reasoningTokens,
  });
  for (const name of [...pending.skills].sort()) {
    rows.push({
      key: `skill:${name}`,
      kind: 'skill',
      label: name,
      detail: '发送时注入的技能前缀',
      tokens: estimateTokens(name) + 2,
    });
  }
  rows.push({
    key: 'draft',
    kind: 'draft',
    label: '当前草稿',
    detail: `${[...pending.draft].length} 字`,
    tokens: estimateTokens(pending.draft),
  });

  const kept = rows.filter((r) => r.tokens > 0).sort((a, b) => b.tokens - a.tokens);
  const used = kept.reduce((s, r) => s + r.tokens, 0);
  const limit = pending.contextWindow;

  return {
    rows: kept,
    used,
    window: limit,
    ratio: Math.min(1, used / limit),
    percent: Math.round((used / limit) * 100),
    measured: pending.measured,
    calibrated,
  };
}

export function computeContextUsage(
  messages: ChatMsg[],
  pending: PendingContext,
): ContextUsage {
  return assembleUsage(scanHistory(messages), pending);
}

/**
 * 订阅式装配。历史扫描与待发段分两段 memo:
 * 逐键入草稿只重算装配段,长会话消息树不随每次按键重扫。
 */
export function useContextUsage(skills: string[], draft: string): ContextUsage {
  const messages = useChatStore((st) => st.messages);
  const measured = useChatStore((st) => st.lastPromptTokens);
  const contextWindow = useEffectiveSpec().contextTokens;

  const scan = useMemo(() => scanHistory(messages), [messages]);
  return useMemo(
    () => assembleUsage(scan, { skills, draft, contextWindow, measured }),
    [scan, skills, draft, contextWindow, measured],
  );
}
