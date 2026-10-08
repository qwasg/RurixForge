import { create } from 'zustand';
import type { EditorAnnotation } from '@forge/protocol';
import { apiGet, apiPatch, apiPost, compactSession, ForgeApiError, getCodexModels } from './forgeApi';
import { notifyAgentToolSettled } from './editorSync';
import { useGoalStore } from './goalStore';
import { useDesignFlowStore } from './designFlowStore';
import { usePlanStore } from './planStore';
import { useSessionStore, type AgentEngine, type ForgeSession } from './sessionStore';
import { useToastStore } from './toastStore';
import { subscribeSessionEvents, type SseSubscription } from './sseClient';
import {
  isPlanningTurn,
  useUltraPlanStore,
  type UltraPlanActionRef,
  type UltraPlanState,
} from './ultraPlanStore';
import { useWorkbenchStore } from './workbenchStore';
import { useCollaborationStore } from './collaborationStore';
import {
  bareName,
  mcpOf,
  toolStatus,
  type ApprovalQuestion,
  type BlockStatus,
  type ChatBlock,
  type PermissionDecision,
  type ToolChange,
  type ToolKind,
} from './timeline';

/**
 * F7 wave.4 chatStore(参考 lib.rs ChatStore::apply_event 语义移植;G-F7-4)。
 *
 * - applyEvent 全覆盖本仓事件面(wave.2 实测):composer.user.message / agent.started /
 *   agent.tool.invoked|completed|failed / agent.usage / agent.message /
 *   agent.completed|failed|cancelled / todo.created|updated / session.updated。
 * - 去重:id → `id:{id}`;否则 `fallback:{seq}:{type}:{runId}:{payload}`(cap 4096, FIFO)。
 * - normalize:同 runId 用户消息去重 + local-* 同文去重 + user-before-assistant 校正。
 * - 乐观回显:sendMessage/editAndResend 先 push local-* 用户卡,composer.user.message
 *   事件到达时 upsert 替换(同文 runId=null 匹配)。
 * - SSE:selectSession 快照回放 → latestSeq 起订;stream.gap → 重拉快照全量重放再续订。
 *
 * 对齐参考仓 apply_event,并修其裂缝:继续收 agent.message 终稿;收 args.delta /
 * stream.reset;todo_write 与 write_todos 都当里程碑;token/reasoning 合帧(测试同步刷)。
 * 瞬时事件不落盘。permission.requested 同时保留 toast 与助手卡片内 approval 块。
 *
 * D-044 UltraPlan:ultraplan.* 里的关口事件建 `ultraplan` 卡片块(回放与实时同路径);
 * 流程阶段本身不由事件推导——回放只建卡,实时事件只通知 ultraPlanStore 去重拉。
 */

export interface ForgeEventWire {
  id: string;
  sessionId: string;
  seq: number;
  type: string;
  ts: string;
  source?: Record<string, string>;
  correlationId?: string | null;
  channel?: string;
  payload?: Record<string, unknown>;
}

export interface ChatMsg {
  annotations?: EditorAnnotation[];
  id: string;
  role: 'user' | 'assistant';
  agentId?: string;
  agentName?: string;
  /** Durable message identity for additional input during an existing run. */
  messageId?: string;
  clientMessageId?: string;
  messageStatus?: string;
  /** 用户消息正文(assistant 正文在 blocks 的 text 块)。 */
  text: string;
  blocks: ChatBlock[];
  status?: 'streaming' | 'completed' | 'failed' | 'cancelled';
  model?: string;
  provider?: string;
  engine?: AgentEngine | string;
  mode?: string;
  /** HH:MM(事件 ts[11..16],参考 hhmm_from_ts)。 */
  time: string;
  ts?: string;
  runId?: string | null;
  startedTs?: string | null;
  finishedTs?: string | null;
  error?: string;
  /** agent.failed 的结构化失败码(D-041 云模式:CLOUD_LOGIN_REQUIRED / INSUFFICIENT_BALANCE 等)。 */
  errorCode?: string;
  /**
   * D-038:用户卡的来源。`receipt` = 后台子代理回执送达时系统自动唤醒主 agent 的那一轮——
   * 卡片正文是系统生成的唤醒说明,不是用户说的话,不可编辑重发。缺省(undefined)= 用户发的。
   */
  source?: 'receipt';
  /**
   * D-044:这条用户消息是 UltraPlan 卡片动作(提交问卷 / 通过 Demo / 开始制作…)的回显,
   * 正文是后端写的展示文案。带此标记的卡不可编辑重发——回退会让事件日志落后于阶段机。
   */
  ultraAction?: UltraPlanActionRef;
}

/** 一次上下文压缩(context.compacted)在时间线上的位置。 */
export interface CompactionMark {
  /** 压缩时最后一条消息的 id(分隔线画在它之后)。 */
  afterMessageId: string | null;
  ts: string;
  engine: string;
  /** 压缩后摘要的估算 token(本地引擎回报;Codex 自己总结、不回报 → null)。 */
  summaryTokens: number | null;
}

export interface TodoItem {
  id: string;
  sessionId?: string;
  title: string;
  status: string;
  kind?: string;
  summary?: string | null;
  /** F7 wave.5:看板卡两行截断描述(快照面有;todo.* 事件载荷无 description,如实缺省)。 */
  description?: string | null;
  /** D-035:来源计划文件里的待办 id(Plan 页签按它映射实时状态);非计划来源缺省。 */
  planTodoId?: string | null;
  /** 来源标记("user" / "plan" / "ultraplan")。 */
  source?: string;
}

/** reasoning_effort 档(id 即后端实发的 API 值)。 */
export interface ModelEffortOption {
  id: string;
  label: string;
}

/** 上下文窗口档(tokens 即计量环分母)。 */
export interface ModelContextOption {
  id: string;
  label: string;
  tokens: number;
}

/**
 * 模型条目 + 规格能力面(agentd modelspec.rs CATALOG 下发)。
 * 能力字段全可选:后端若未升级(旧快照)则退化为「只有模型选择」的形态,菜单三档整体禁用。
 */
export interface SnapshotModel {
  id: string;
  label: string;
  provider?: string;
  availability?: string;
  /** 菜单分组标题(DeepSeek / 本地 / 自定义渠道)。 */
  group?: string;
  supportsThinking?: boolean;
  thinkingMode?: string;
  thinkingAlwaysOn?: boolean;
  /** 空数组 = 该渠道不收 reasoning_effort(如 deepseek),Effort 行禁用。 */
  effortOptions?: ModelEffortOption[];
  defaultEffort?: string | null;
  contextOptions?: ModelContextOption[];
  defaultContext?: string;
  /** 云端条目(provider=cloud)附带:是否收图、单价(micros / 1M tokens)与币种。 */
  vision?: boolean;
  pricing?: {
    inputPer1M?: number;
    outputPer1M?: number;
    cacheReadPer1M?: number;
    cacheWritePer1M?: number;
  };
  currency?: string;
}

interface DesignSnapshot {
  sessions?: unknown[];
  activeSession?: {
    id: string;
    selectedModelId?: string | null;
    thinkingEnabled?: boolean;
    reasoningEffort?: string | null;
    contextOptionId?: string | null;
    agentEngine?: AgentEngine;
    /** D-035:当前计划文件(刷新/换会话后 Plan 页签据此定位)。 */
    activePlanPath?: string | null;
    /** D-044:UltraPlan 阶段机(无流程缺省 / null);ultraPlanStore 的首要事实源。 */
    ultraplan?: UltraPlanState | null;
    /** D-045:Design 阶段机(无流程缺省 / null)。 */
    design?: unknown;
  } | null;
  events?: ForgeEventWire[];
  todos?: TodoItem[];
  run?: { id: string; status: string } | null;
  models?: { models?: SnapshotModel[]; defaultModelId?: string };
  agents?: {
    defaultEngine?: AgentEngine;
    engines?: Array<Record<string, unknown> & { id?: string }>;
  };
  latestSeq?: number;
  chatFolders?: unknown[];
}

interface TokenTotals {
  prompt: number;
  completion: number;
  total: number;
}

export type StreamLink = 'idle' | 'connecting' | 'live' | 'down';

export interface PendingPermission {
  id: string;
  agentId?: string;
  agentName?: string;
  runId?: string | null;
  tool: string;
  approvalKind: string;
  command?: string;
  cwd?: string;
  changes?: ToolChange[];
  reason?: string;
  message?: string;
  questions?: ApprovalQuestion[];
  permissions?: Record<string, unknown>;
  schema?: Record<string, unknown>;
  networkApprovalContext?: Record<string, unknown>;
  proposedExecpolicyAmendment?: string[];
  proposedNetworkPolicyAmendments?: Array<Record<string, unknown>>;
  grantRoot?: string;
  mode?: string;
  serverName?: string;
  url?: string;
  elicitationId?: string;
  availableDecisions?: PermissionDecision[];
}

export interface ResolvePermissionOptions {
  decision?: PermissionDecision;
  answers?: Record<string, unknown>;
}

export interface ResolvePermission {
  (allow: boolean, options?: ResolvePermissionOptions): Promise<void>;
  (id: string, allow: boolean, options?: ResolvePermissionOptions): Promise<void>;
}

interface ChatState {
  messages: ChatMsg[];
  activeRunId: string | null;
  todos: TodoItem[];
  tokens: TokenTotals;
  /**
   * 最近一次 agent.usage 的 promptTokens(0 = 本会话无实测)。
   * tokens.prompt 是跨轮累计,衡量不了「当前上下文有多满」——最后一次 usage 的 prompt
   * 才是最近一轮真实送进模型的上下文体量,Composer 上下文环用它校准基线。
   */
  lastPromptTokens: number;
  /**
   * 最近一次 agent.usage 带的模型窗口(Codex 的 modelContextWindow;本地引擎不带 → null)。
   * 有值时上下文环以它为分母,比模型目录的 Context 档准。
   */
  usageContextWindow: number | null;
  /** 上下文压缩分隔点(context.compacted 事件,按先后);计量面只估算最近一个之后的消息。 */
  compactions: CompactionMark[];
  /** 正在手动压缩上下文的会话 id(请求在途;Composer 据此暂停发送)。 */
  compactingSessionId: string | null;
  latestSeq: number;
  models: SnapshotModel[];
  defaultModelId: string | null;
  selectedModelId: string | null;
  /**
   * 模型规格三档的会话级选择(落库同源 agentd DebugSession)。
   * 只存「选择」,不存归一结果:换模型后旧档位是否仍适用由 resolveModelSpec 现算
   * (与后端 modelspec::resolve 同规则)。
   */
  thinkingEnabled: boolean;
  reasoningEffort: string | null;
  contextOptionId: string | null;
  /** 快照加载中(切换会话骨架态)。 */
  hydrating: boolean;
  /**
   * 当前已回放/订阅的会话 id。selectSession 对同目标的在途调用复用 Promise、
   * 对已就绪目标幂等，避免 reset() 抹掉全屏主页「无会话直接发」的乐观回显。
   */
  currentSessionId: string | null;
  /** SubagentOverlay 目标子代理块 id。 */
  subagentOverlayId: string | null;
  /** auto 权限挂起的审批(toast + 设置页)。 */
  pendingPermission: PendingPermission | null;
  pendingPermissions: PendingPermission[];
  /** 最近一次 Codex 账户通知；设置页可立即反映登录态变化。 */
  codexAccount: Record<string, unknown> | null;
  codexRateLimits: Record<string, unknown> | null;
  /** turn/diff/updated 的最近值；文件变更块也会尽量同步合并该 diff。 */
  latestDiff: unknown | null;
  /**
   * F7 wave.5:原始事件环(底部面板 Agent Logs/Output 数据源;cap 200 FIFO,
   * Agent Logs 取末 120 条,Output 取末 200 条;applyEvent 全类型入环,reset 清空)。
   */
  eventsRing: ForgeEventWire[];
  /**
   * D-047:会话事件流(SSE)连接态,轮次状态行据此显示 Reconnecting / Connection lost。
   * idle = 未订阅;connecting = 订阅后首连未开;live = 已开;down = 出错、sseClient 正在退避重连。
   */
  streamLink: StreamLink;
  /** 本次掉线的起点(ms epoch);连上即清空。 */
  streamDownSince: number | null;
  /**
   * D-047:已发出、还没等到本轮 agent.started 的起点(ms epoch)。助手卡出现之前,
   * MessageList 靠它在用户卡下面先挂「Planning next moves」;开轮 / 收束 / 发送失败即清空。
   */
  pendingTurnSince: number | null;

  applyEvent: (evt: ForgeEventWire) => void;
  selectSession: (id: string | null) => Promise<void>;
  resync: () => Promise<void>;
  /**
   * skills:选中技能名(F11:ask:execute 结构化字段,后端按名注入 SKILL.md 全文;空/缺省 = 不发该字段)。
   * opts.planPath:D-035 Plan 页签 Build——后端据此读计划文件、物化待办并把全文注入本轮。
   */
  sendMessage: (
    text: string,
    mode: string,
    skills?: string[],
    opts?: { planPath?: string; annotations?: EditorAnnotation[] },
  ) => Promise<boolean | void>;
  steerAgent: (text: string, agentId?: string, clientMessageId?: string, annotations?: EditorAnnotation[]) => Promise<boolean>;
  cancelRun: () => Promise<void>;
  /** 手动压缩当前会话的上下文(结果与失败以 toast 告知;分隔线随 context.compacted 事件出现)。 */
  compactContext: () => Promise<void>;
  /**
   * D-036:中止单个后台子代理(multitask dispatch 派出去的 run)。
   * 与 cancelRun 分开:后台子代理跑的时候父轮早已收束、activeRunId 为空,
   * cancelRun 会直接空转。
   */
  cancelSubagent: (runId: string) => Promise<void>;
  /** mode:回退重发时的模式覆盖;缺省沿用原消息 mode。 */
  editAndResend: (msgId: string, newText: string, mode?: string) => Promise<void>;
  pickModel: (modelId: string | null) => Promise<void>;
  setThinking: (on: boolean) => Promise<void>;
  pickEffort: (effortId: string) => Promise<void>;
  pickContext: (contextId: string) => Promise<void>;
  openSubagent: (id: string | null) => void;
  resolvePermission: ResolvePermission;
  /** 清空原始事件环(底部面板 trash 钮;不影响消息/待办)。 */
  clearEventsRing: () => void;
  ensureModels: (force?: boolean, requiredProvider?: string) => Promise<void>;
  reset: () => void;
}

/**
 * 模型规格四项(模型 / Thinking / Effort / Context)。这四个 state 键与会话 PATCH 的 wire 键
 * 逐字同名；仅 selectedModelId=null 在 HTTP wire 上编码成空串，因为 serde
 * Option<Option<T>> 会把 JSON null 与字段缺省折叠，而 agentd 以空串表示清除。
 */
type SpecPatch = Partial<
  Pick<ChatState, 'selectedModelId' | 'thinkingEnabled' | 'reasoningEffort' | 'contextOptionId'>
>;

const MAX_SEEN = 4096;
/** eventsRing 上限(Agent Logs 取末 120,Output 取末 200 → 环容 200)。 */
export const EVENTS_RING_CAP = 200;
/** 会话快照首次不可用时的固定短间隔重试；SSE 建立后由 sseClient 自己退避。 */
export const SESSION_SNAPSHOT_RETRY_MS = 500;
let localCounter = 0;
let selectToken = 0;

/** 事件入环(FIFO,cap EVENTS_RING_CAP)。 */
function ringPush(ring: ForgeEventWire[], evt: ForgeEventWire): ForgeEventWire[] {
  const next = ring.length >= EVENTS_RING_CAP ? ring.slice(ring.length - EVENTS_RING_CAP + 1) : ring.slice();
  next.push(evt);
  return next;
}

function toastError(err: unknown, prefix: string): void {
  const msg = err instanceof Error ? err.message : String(err);
  useToastStore.getState().push('error', `${prefix}:${msg}`);
}

function hhmm(ts: string | undefined): string {
  return ts && ts.length >= 16 ? ts.slice(11, 16) : '';
}

function payloadStr(evt: ForgeEventWire, key: string): string | undefined {
  const v = evt.payload?.[key];
  return typeof v === 'string' ? v : undefined;
}

function runIdOf(evt: ForgeEventWire): string | null {
  return payloadStr(evt, 'runId') ?? (evt.correlationId || null);
}

function parentOf(evt: ForgeEventWire): string | undefined {
  return payloadStr(evt, 'parentToolCallId');
}

const COALESCE_STREAM =
  typeof requestAnimationFrame === 'function' &&
  !(typeof process !== 'undefined' && process.env?.VITEST);

type SubagentBlock = Extract<ChatBlock, { kind: 'subagent' }>;
type ToolBlock = Extract<ChatBlock, { kind: 'tool' }>;
type UltraBlock = Extract<ChatBlock, { kind: 'ultraplan' }>;
type DesignBlock = Extract<ChatBlock, { kind: 'design' }>;

/** D-045:design.* 建卡事件 → 卡片 step + 版本号键。 */
const DESIGN_CARD_EVENTS: Record<string, { step: DesignBlock['step']; revKey: string }> = {
  'design.review.ready': { step: 'review', revKey: 'rev' },
  'design.layout.ready': { step: 'layout', revKey: 'round' },
  'design.verify.result': { step: 'verify', revKey: 'n' },
  'design.done': { step: 'done', revKey: 'round' },
};

/** 同一 (flowId, step, rev) 只留一张卡(重交元素清单原地替换)。 */
function upsertDesignBlock(blocks: ChatBlock[], next: DesignBlock): void {
  const idx = blocks.findIndex(
    (b) => b.kind === 'design' && b.flowId === next.flowId && b.step === next.step && b.rev === next.rev,
  );
  if (idx < 0) {
    blocks.push(next);
    return;
  }
  const prev = blocks[idx] as DesignBlock;
  blocks[idx] = { ...next, ...(prev.submitted ? { submitted: prev.submitted } : {}) };
}

/** 审阅卡回填用户决定(跨消息从新往旧找第一张)。 */
function markDesignDecided(messages: ChatMsg[], flowId: string, rev: number, submitted: Record<string, unknown>): ChatMsg[] {
  for (let i = messages.length - 1; i >= 0; i -= 1) {
    const message = messages[i];
    if (message.role !== 'assistant') continue;
    const j = message.blocks.findIndex(
      (b) => b.kind === 'design' && b.step === 'review' && b.flowId === flowId && b.rev === rev,
    );
    if (j < 0) continue;
    const blocks = [...message.blocks];
    blocks[j] = { ...(blocks[j] as DesignBlock), submitted };
    const out = [...messages];
    out[i] = { ...message, blocks };
    return out;
  }
  return messages;
}

/** D-044:建卡事件 → 卡片 step + 载荷里的版本号键(done 无版本,rev 恒 0)。 */
const ULTRA_CARD_EVENTS: Record<string, { step: UltraBlock['step']; revKey: string | null }> = {
  'ultraplan.questionnaire': { step: 'questionnaire', revKey: 'rev' },
  'ultraplan.demo.ready': { step: 'demo', revKey: 'iteration' },
  'ultraplan.plan.ready': { step: 'plan', revKey: 'rev' },
  'ultraplan.acceptance.ready': { step: 'acceptance', revKey: 'round' },
  'ultraplan.done': { step: 'done', revKey: null },
};

/** D-044:「已提交」事件 → 要回填 submitted 的卡(同一 upId + step + rev)。 */
const ULTRA_SUBMIT_EVENTS: Record<string, { step: UltraBlock['step']; revKey: string }> = {
  'ultraplan.answers.submitted': { step: 'questionnaire', revKey: 'rev' },
  'ultraplan.demo.decision': { step: 'demo', revKey: 'iteration' },
  'ultraplan.acceptance.recorded': { step: 'acceptance', revKey: 'round' },
};

function payloadRev(payload: Record<string, unknown>, key: string | null): number {
  const value = key === null ? undefined : payload[key];
  return typeof value === 'number' && Number.isFinite(value) ? value : 0;
}

/** composer.user.message.payload.ultraplan → 动作回显(resume_production 无 rev)。 */
function ultraActionOf(raw: unknown): UltraPlanActionRef | undefined {
  if (!raw || typeof raw !== 'object') return undefined;
  const rec = raw as Record<string, unknown>;
  if (typeof rec.id !== 'string' || typeof rec.action !== 'string') return undefined;
  return { id: rec.id, action: rec.action, rev: typeof rec.rev === 'number' ? rec.rev : null };
}

/** 同一 (upId, step, rev) 只留一张卡:重复到达时换载荷、保留已回填的 submitted。 */
function upsertUltraBlock(blocks: ChatBlock[], next: UltraBlock): void {
  const idx = blocks.findIndex(
    (b) => b.kind === 'ultraplan' && b.upId === next.upId && b.step === next.step && b.rev === next.rev,
  );
  if (idx < 0) {
    blocks.push(next);
    return;
  }
  const prev = blocks[idx] as UltraBlock;
  blocks[idx] = { ...next, ...(prev.submitted ? { submitted: prev.submitted } : {}) };
}

/**
 * 给匹配的卡回填 submitted。提交与建卡不在同一轮(建卡在上一轮的助手消息里),
 * 所以要跨消息找,从新往旧取第一张;找不到(历史被截断)就不动。
 */
function markUltraSubmitted(
  messages: ChatMsg[],
  upId: string,
  step: UltraBlock['step'],
  rev: number,
  submitted: Record<string, unknown>,
): ChatMsg[] {
  for (let i = messages.length - 1; i >= 0; i -= 1) {
    const message = messages[i];
    if (message.role !== 'assistant') continue;
    for (let j = message.blocks.length - 1; j >= 0; j -= 1) {
      const block = message.blocks[j];
      if (block.kind !== 'ultraplan' || block.upId !== upId || block.step !== step || block.rev !== rev) {
        continue;
      }
      const blocks = [...message.blocks];
      blocks[j] = { ...block, submitted };
      const out = [...messages];
      out[i] = { ...message, blocks };
      return out;
    }
  }
  return messages;
}

function prettyArgs(raw: unknown): string {
  if (raw === undefined) return '';
  if (typeof raw === 'string') return raw;
  try {
    return JSON.stringify(raw, null, 2);
  } catch {
    return String(raw);
  }
}

function toolKindOf(name: string, raw: unknown): ToolKind {
  if (
    raw === 'command' ||
    raw === 'fileChange' ||
    raw === 'webSearch' ||
    raw === 'mcp' ||
    raw === 'native'
  ) {
    return raw;
  }
  if (name === 'shell') return 'command';
  if (name === 'apply_patch') return 'fileChange';
  if (name === 'web_search') return 'webSearch';
  return mcpOf(name) ? 'mcp' : 'native';
}

function normalizeChanges(raw: unknown): ToolChange[] | undefined {
  if (!Array.isArray(raw)) return undefined;
  const changes: ToolChange[] = [];
  for (const value of raw) {
    if (!value || typeof value !== 'object') continue;
    const rec = value as Record<string, unknown>;
    const path =
      typeof rec.path === 'string'
        ? rec.path
        : typeof rec.file === 'string'
          ? rec.file
          : typeof rec.filePath === 'string'
            ? rec.filePath
            : '';
    if (path === '') continue;
    changes.push({
      path,
      ...(typeof rec.kind === 'string' ? { kind: rec.kind } : {}),
      ...(typeof rec.diff === 'string' ? { diff: rec.diff } : {}),
    });
  }
  return changes.length > 0 ? changes : undefined;
}

function changesFromPayload(payload: Record<string, unknown> | undefined): ToolChange[] | undefined {
  const direct = normalizeChanges(payload?.changes);
  if (direct) return direct;
  const args = payload?.args;
  if (args && typeof args === 'object') return normalizeChanges((args as Record<string, unknown>).changes);
  if (typeof args === 'string') {
    try {
      const parsed = JSON.parse(args) as { changes?: unknown };
      return normalizeChanges(parsed.changes);
    } catch {
      return undefined;
    }
  }
  return undefined;
}

function normalizeQuestions(raw: unknown): ApprovalQuestion[] | undefined {
  if (!Array.isArray(raw)) return undefined;
  const questions: ApprovalQuestion[] = [];
  raw.forEach((value, index) => {
    if (!value || typeof value !== 'object') return;
    const rec = value as Record<string, unknown>;
    const id =
      typeof rec.id === 'string'
        ? rec.id
        : typeof rec.name === 'string'
          ? rec.name
          : `question-${index + 1}`;
    const options = Array.isArray(rec.options)
      ? rec.options.flatMap((option): Array<NonNullable<ApprovalQuestion['options']>[number]> => {
          if (typeof option === 'string') return [{ label: option }];
          if (!option || typeof option !== 'object') return [];
          const item = option as Record<string, unknown>;
          if (typeof item.label !== 'string') return [];
          return [{
            label: item.label,
            ...(typeof item.description === 'string' ? { description: item.description } : {}),
            ...(typeof item.isOther === 'boolean' ? { isOther: item.isOther } : {}),
          }];
        })
      : undefined;
    questions.push({
      id,
      ...(typeof rec.header === 'string' ? { header: rec.header } : {}),
      ...(typeof rec.question === 'string'
        ? { question: rec.question }
        : typeof rec.prompt === 'string'
          ? { question: rec.prompt }
          : {}),
      ...(options && options.length > 0 ? { options } : {}),
      ...(typeof rec.required === 'boolean' ? { required: rec.required } : {}),
      ...(typeof rec.secret === 'boolean' ? { secret: rec.secret } : {}),
      ...(typeof rec.isSecret === 'boolean' ? { isSecret: rec.isSecret } : {}),
      ...(typeof rec.isOther === 'boolean' ? { isOther: rec.isOther } : {}),
    });
  });
  return questions.length > 0 ? questions : undefined;
}

function normalizeRecord(raw: unknown): Record<string, unknown> | undefined {
  return raw && typeof raw === 'object' && !Array.isArray(raw)
    ? raw as Record<string, unknown>
    : undefined;
}

function normalizeStrings(raw: unknown): string[] | undefined {
  if (!Array.isArray(raw)) return undefined;
  const values = raw.filter((value): value is string => typeof value === 'string');
  return values.length > 0 ? values : undefined;
}

function normalizeRecords(raw: unknown): Array<Record<string, unknown>> | undefined {
  if (!Array.isArray(raw)) return undefined;
  const values = raw.filter(
    (value): value is Record<string, unknown> =>
      Boolean(value) && typeof value === 'object' && !Array.isArray(value),
  );
  return values.length > 0 ? values : undefined;
}

function normalizeDecisions(raw: unknown): PermissionDecision[] | undefined {
  if (!Array.isArray(raw)) return undefined;
  const values = raw.filter(
    (value): value is PermissionDecision =>
      value === 'accept' || value === 'acceptForSession' || value === 'decline',
  );
  return values.length > 0 ? values : undefined;
}

function appendTextDelta(blocks: ChatBlock[], delta: string): void {
  const last = blocks[blocks.length - 1];
  if (last?.kind === 'text' && !last.final) {
    blocks[blocks.length - 1] = { ...last, text: last.text + delta };
  } else {
    blocks.push({ kind: 'text', text: delta, final: false });
  }
}

function appendPlanDelta(blocks: ChatBlock[], delta: string): void {
  for (let i = blocks.length - 1; i >= 0; i -= 1) {
    const block = blocks[i];
    if (block.kind === 'plan' && !block.final) {
      blocks[i] = { ...block, text: block.text + delta };
      return;
    }
  }
  blocks.push({ kind: 'plan', text: delta, final: false });
}

function appendToolOutput(blocks: ChatBlock[], callId: string | undefined, delta: string): boolean {
  for (let i = blocks.length - 1; i >= 0; i -= 1) {
    const block = blocks[i];
    if (block.kind === 'tool' && (callId ? block.toolCallId === callId : toolStatus(block) === 'running')) {
      const output = (block.output ?? block.result ?? '') + delta;
      blocks[i] = { ...block, output, result: output };
      return true;
    }
    if (block.kind === 'subagent') {
      const work = [...block.work];
      if (appendToolOutput(work, callId, delta)) {
        blocks[i] = { ...block, work };
        return true;
      }
    }
  }
  return false;
}

function settleApproval(
  blocks: ChatBlock[],
  id: string | undefined,
  decision: string | undefined,
  answers?: Record<string, unknown>,
): boolean {
  for (let i = blocks.length - 1; i >= 0; i -= 1) {
    const block = blocks[i];
    if (block.kind === 'approval' && (!id || block.id === id)) {
      blocks[i] = { ...block, ...(decision ? { decision } : {}), ...(answers ? { answers } : {}) };
      return true;
    }
    if (block.kind === 'subagent') {
      const work = [...block.work];
      if (settleApproval(work, id, decision, answers)) {
        blocks[i] = { ...block, work };
        return true;
      }
    }
  }
  return false;
}

/** 轮已终止但没有 permission.resolved 的历史卡片不能继续可点。 */
function expireApprovals(blocks: ChatBlock[]): boolean {
  let changed = false;
  for (let i = 0; i < blocks.length; i += 1) {
    const block = blocks[i];
    if (block.kind === 'approval' && block.decision === undefined) {
      blocks[i] = { ...block, decision: 'expired' };
      changed = true;
    } else if (block.kind === 'subagent') {
      const work = [...block.work];
      if (expireApprovals(work)) {
        blocks[i] = { ...block, work };
        changed = true;
      }
    }
  }
  return changed;
}

function finalizePlan(blocks: ChatBlock[], path: string): boolean {
  for (let i = blocks.length - 1; i >= 0; i -= 1) {
    const block = blocks[i];
    if (block.kind === 'plan' && !block.final) {
      blocks[i] = { ...block, final: true, planPath: path };
      return true;
    }
  }
  return false;
}

function mergeDiffIntoLastFileChange(blocks: ChatBlock[], diff: string): boolean {
  for (let i = blocks.length - 1; i >= 0; i -= 1) {
    const block = blocks[i];
    if (
      block.kind === 'tool' &&
      (block.toolKind === 'fileChange' || block.name === 'apply_patch')
    ) {
      if (!block.changes?.length) return false;
      const changes = block.changes.map((change, index) =>
        index === 0 && !change.diff ? { ...change, diff } : change,
      );
      blocks[i] = { ...block, changes };
      return true;
    }
    if (block.kind === 'subagent') {
      const work = [...block.work];
      if (mergeDiffIntoLastFileChange(work, diff)) {
        blocks[i] = { ...block, work };
        return true;
      }
    }
  }
  return false;
}

/** 终稿 reasoning:覆盖本轮最后一块思考(流式后正文已接在后面时,不能只认尾块)。 */
function settleReasoning(blocks: ChatBlock[], text: string, ts: string): void {
  for (let i = blocks.length - 1; i >= 0; i -= 1) {
    const last = blocks[i];
    if (last.kind === 'reasoning') {
      blocks[i] = { ...last, text, startedTs: last.startedTs ?? ts, endedTs: ts };
      return;
    }
  }
  blocks.push({ kind: 'reasoning', text, startedTs: ts, endedTs: ts });
}

/** 终稿正文:覆盖本轮最后一块 text(中间若又插了思考块,不能只认尾块)。 */
function settleFinalText(blocks: ChatBlock[], text: string): void {
  for (let i = blocks.length - 1; i >= 0; i -= 1) {
    if (blocks[i].kind === 'text') {
      blocks[i] = { kind: 'text', text, final: true };
      return;
    }
  }
  blocks.push({ kind: 'text', text, final: true });
}

/** ts = 本帧末一条 reasoning 事件的 ts;首末之差即思考行显示的时长(合帧误差 ≤ 1 帧)。 */
function appendReasoningDelta(blocks: ChatBlock[], delta: string, ts: string): void {
  const last = blocks[blocks.length - 1];
  if (last?.kind === 'reasoning') {
    blocks[blocks.length - 1] = {
      ...last,
      text: last.text + delta,
      startedTs: last.startedTs ?? ts,
      endedTs: ts,
    };
  } else {
    blocks.push({ kind: 'reasoning', text: delta, startedTs: ts, endedTs: ts });
  }
}

function resetStreamingBlocks(blocks: ChatBlock[]): void {
  for (let i = blocks.length - 1; i >= 0; i -= 1) {
    const b = blocks[i];
    if (b.kind === 'text' && !b.final) blocks.splice(i, 1);
    else if (b.kind === 'subagent') {
      const work = [...b.work];
      resetStreamingBlocks(work);
      blocks[i] = { ...b, work };
    }
  }
}

function ensureSubagent(blocks: ChatBlock[], id: string, patch?: Partial<SubagentBlock>): SubagentBlock {
  const updateExisting = (items: ChatBlock[]): SubagentBlock | undefined => {
    for (let index = 0; index < items.length; index += 1) {
      const item = items[index];
      if (item.kind !== 'subagent') continue;
      const adoptPlaceholder = item.id.startsWith('agent-turn-') && patch?.agentId && patch.agentRunId && item.agentId === patch.agentId && item.agentRunId === patch.agentRunId;
      if (item.id === id || adoptPlaceholder) {
        const next = { ...item, ...patch, id, work: patch?.work ?? item.work };
        if (adoptPlaceholder && patch?.status === 'running' && item.status !== 'running') next.status = item.status;
        items[index] = next;
        return next;
      }
      const work = [...item.work];
      const nested = updateExisting(work);
      if (nested) {
        items[index] = { ...item, work };
        return nested;
      }
    }
    return undefined;
  };
  const existing = updateExisting(blocks);
  if (existing) return existing;
  const created: SubagentBlock = {
    kind: 'subagent',
    id,
    agentId: patch?.agentId,
    agentRunId: patch?.agentRunId,
    label: patch?.label ?? '子代理任务',
    status: patch?.status ?? 'running',
    summary: patch?.summary,
    prompt: patch?.prompt,
    parentToolCallId: patch?.parentToolCallId,
    detachedRunId: patch?.detachedRunId,
    work: patch?.work ?? [],
  };
  blocks.push(created);
  return created;
}

function targetBlocks(root: ChatBlock[], parentId: string | undefined): ChatBlock[] {
  if (!parentId) return root;
  const current = ensureSubagent(root, parentId);
  const work = [...current.work];
  ensureSubagent(root, parentId, { work });
  return work;
}

function targetForAgent(blocks: ChatBlock[], agentId: string, agentRunId?: string): string | undefined {
  for (let index = blocks.length - 1; index >= 0; index -= 1) {
    const block = blocks[index];
    if (block.kind !== 'subagent') continue;
    if (block.agentId === agentId && (!agentRunId || !block.agentRunId || block.agentRunId === agentRunId)) return block.id;
    const nested = targetForAgent(block.work, agentId, agentRunId);
    if (nested) return nested;
  }
  return undefined;
}

function settleToolOrSub(
  blocks: ChatBlock[],
  callId: string | undefined,
  patch: Partial<ToolBlock> & { subStatus?: BlockStatus; subSummary?: string },
): boolean {
  for (let i = blocks.length - 1; i >= 0; i -= 1) {
    const b = blocks[i];
    if (b.kind === 'subagent' && (callId ? b.id === callId : b.status === 'running')) {
      blocks[i] = {
        ...b,
        status: patch.subStatus ?? (patch.ok === false ? 'error' : 'done'),
        summary: patch.subSummary ?? patch.error ?? b.summary,
      };
      return true;
    }
    if (
      b.kind === 'tool' &&
      (callId ? b.toolCallId === callId : b.ok === undefined && b.error === undefined)
    ) {
      blocks[i] = { ...b, ...patch };
      return true;
    }
    if (b.kind === 'subagent') {
      const work = [...b.work];
      if (settleToolOrSub(work, callId, patch)) {
        blocks[i] = { ...b, work };
        return true;
      }
    }
  }
  return false;
}

export const useChatStore = create<ChatState>((set, get) => {
  // 非响应式内部态(去重集/SSE 句柄,不进 zustand state 防多余渲染)
  const seenKeys = new Set<string>();
  const seenOrder: string[] = [];
  /** UI 融合波 C3:toolCallId → 工具名(invoked 记录,completed/failed 载荷无 name,查后删)。 */
  const toolNames = new Map<string, string>();
  const streamBuf = new Map<string, { token: string; reasoning: string; plan: string; ts: string }>();
  const toolOutputBuf = new Map<
    string,
    { runId: string | null; parent?: string; callId?: string; delta: string }
  >();
  let streamRaf: number | null = null;
  let sse: SseSubscription | null = null;
  let pendingSelection: { id: string | null; promise: Promise<void> } | null = null;

  const streamKey = (runId: string | null, parent?: string) => `${runId ?? ''}\0${parent ?? ''}`;

  const applyStreamChunk = (
    runId: string | null,
    parent: string | undefined,
    kind: 'token' | 'reasoning' | 'plan',
    text: string,
    ts: string,
  ) => {
    let messages = get().messages;
    messages = mutateAssistant(messages, runId, (m) => {
      const dest = targetBlocks(m.blocks, parent);
      if (kind === 'token') appendTextDelta(dest, text);
      else if (kind === 'reasoning') appendReasoningDelta(dest, text, ts);
      else appendPlanDelta(dest, text);
    });
    set({ messages: normalize(messages) });
  };

  const flushStreamNow = () => {
    if (streamRaf != null && typeof cancelAnimationFrame === 'function') {
      cancelAnimationFrame(streamRaf);
      streamRaf = null;
    }
    if (streamBuf.size === 0 && toolOutputBuf.size === 0) return;
    const pending = [...streamBuf.entries()];
    streamBuf.clear();
    for (const [key, buf] of pending) {
      const [runId, parent] = key.split('\0');
      const rid = runId === '' ? null : runId;
      const pid = parent === '' ? undefined : parent;
      if (buf.reasoning) applyStreamChunk(rid, pid, 'reasoning', buf.reasoning, buf.ts);
      if (buf.plan) applyStreamChunk(rid, pid, 'plan', buf.plan, buf.ts);
      if (buf.token) applyStreamChunk(rid, pid, 'token', buf.token, buf.ts);
    }
    const outputs = [...toolOutputBuf.values()];
    toolOutputBuf.clear();
    for (const output of outputs) {
      let messages = get().messages;
      messages = mutateAssistant(messages, output.runId, (message) => {
        appendToolOutput(targetBlocks(message.blocks, output.parent), output.callId, output.delta);
      });
      set({ messages: normalize(messages) });
    }
  };

  const enqueueStream = (
    runId: string | null,
    parent: string | undefined,
    kind: 'token' | 'reasoning' | 'plan',
    text: string,
    ts: string,
  ) => {
    const key = streamKey(runId, parent);
    const cur = streamBuf.get(key) ?? { token: '', reasoning: '', plan: '', ts };
    if (kind === 'token') cur.token += text;
    else if (kind === 'reasoning') cur.reasoning += text;
    else cur.plan += text;
    cur.ts = ts;
    streamBuf.set(key, cur);
    if (!COALESCE_STREAM) {
      flushStreamNow();
      return;
    }
    if (streamRaf == null) {
      streamRaf = requestAnimationFrame(() => {
        streamRaf = null;
        flushStreamNow();
      });
    }
  };

  const enqueueToolOutput = (
    runId: string | null,
    parent: string | undefined,
    callId: string | undefined,
    delta: string,
  ) => {
    const key = `${streamKey(runId, parent)}\0${callId ?? ''}`;
    const current = toolOutputBuf.get(key);
    if (current) current.delta += delta;
    else toolOutputBuf.set(key, { runId, parent, callId, delta });
    if (!COALESCE_STREAM) {
      flushStreamNow();
      return;
    }
    if (streamRaf == null) {
      streamRaf = requestAnimationFrame(() => {
        streamRaf = null;
        flushStreamNow();
      });
    }
  };

  const markSeen = (evt: ForgeEventWire): boolean => {
    const key = evt.id
      ? `id:${evt.id}`
      : `fallback:${evt.seq}:${evt.type}:${runIdOf(evt) ?? ''}:${JSON.stringify(evt.payload ?? null)}`;
    if (seenKeys.has(key)) return false;
    seenKeys.add(key);
    seenOrder.push(key);
    while (seenOrder.length > MAX_SEEN) {
      const stale = seenOrder.shift();
      if (stale) seenKeys.delete(stale);
    }
    return true;
  };

  const upsertUser = (messages: ChatMsg[], user: ChatMsg): ChatMsg[] => {
    const out = [...messages];
    if (user.messageId || user.clientMessageId) {
      const index = out.findIndex((m) => m.role === 'user' && (
        (user.messageId && m.messageId === user.messageId) ||
        (user.clientMessageId && m.clientMessageId === user.clientMessageId)
      ));
      if (index >= 0) out[index] = user;
      else out.push(user);
      return out;
    }
    if (user.runId) {
      const idx = out.findIndex((m) => m.role === 'user' && !m.messageId && !m.clientMessageId && m.runId === user.runId);
      if (idx >= 0) {
        out[idx] = user;
        return out;
      }
    }
    // 乐观 local-* 同文替换(参考 rposition:取最后一条)
    for (let i = out.length - 1; i >= 0; i -= 1) {
      const m = out[i];
      if (m.role === 'user' && !m.messageId && !m.clientMessageId && !m.runId && m.text === user.text && JSON.stringify(m.annotations ?? []) === JSON.stringify(user.annotations ?? [])) {
        out[i] = user;
        return out;
      }
    }
    if (user.runId) {
      const aidx = out.findIndex((m) => m.role === 'assistant' && m.runId === user.runId);
      if (aidx >= 0) {
        out.splice(aidx, 0, user);
        return out;
      }
    }
    out.push(user);
    return out;
  };

  const ensureAssistant = (messages: ChatMsg[], runId: string | null): ChatMsg[] => {
    if (runId) {
      const idx = messages.findIndex((m) => m.role === 'assistant' && m.runId === runId);
      if (idx >= 0) return messages;
    }
    const assistant: ChatMsg = {
      id: `assistant-${messages.length + 1}-${localCounter += 1}`,
      role: 'assistant',
      text: '',
      blocks: [],
      status: 'streaming',
      time: '',
      runId,
    };
    const out = [...messages];
    if (runId) {
      for (let i = out.length - 1; i >= 0; i -= 1) {
        if (out[i].role === 'user' && out[i].runId === runId) {
          out.splice(i + 1, 0, assistant);
          return out;
        }
      }
    }
    out.push(assistant);
    return out;
  };

  const mutateAssistant = (messages: ChatMsg[], runId: string | null, fn: (m: ChatMsg) => void): ChatMsg[] => {
    const next = ensureAssistant(messages, runId);
    return next.map((m) => {
      if (m.role === 'assistant' && m.runId === runId) {
        const clone: ChatMsg = { ...m, blocks: [...m.blocks] };
        fn(clone);
        return clone;
      }
      return m;
    });
  };

  /**
   * D-044:UltraPlan 卡片落在哪条助手消息上。
   * - 带 runId(turn 内产生):该 run 的助手消息,与其余 run 内事件同口径。
   * - 不带 runId(REST 产生:回退 Demo 的 demo.ready、验收全过后的 done):挂到**最后一条
   *   助手消息**——那正是流程上一轮的卡片所在,新卡接在它后面读起来连贯。
   *   不走 mutateAssistant(null):那条路每次都新建一条 runId=null、status=streaming 的消息,
   *   而 REST 事件没有后续的收束事件,会留下一张永远在转的空卡。
   *   会话里还没有助手消息时才单开一条已完成的独立消息。
   */
  const mutateUltraTarget = (
    messages: ChatMsg[],
    runId: string | null,
    evt: ForgeEventWire,
    fn: (m: ChatMsg) => void,
  ): ChatMsg[] => {
    if (runId) return mutateAssistant(messages, runId, fn);
    let idx = -1;
    for (let i = messages.length - 1; i >= 0; i -= 1) {
      if (messages[i].role === 'assistant') {
        idx = i;
        break;
      }
    }
    if (idx < 0) {
      const standalone: ChatMsg = {
        id: `ultraplan-${evt.id || evt.seq}`,
        role: 'assistant',
        text: '',
        blocks: [],
        status: 'completed',
        time: hhmm(evt.ts),
        ts: evt.ts,
        runId: null,
      };
      fn(standalone);
      return [...messages, standalone];
    }
    return messages.map((m, i) => {
      if (i !== idx) return m;
      const clone: ChatMsg = { ...m, blocks: [...m.blocks] };
      fn(clone);
      return clone;
    });
  };

  /** 参考 normalize_messages:同 runId/同文 local 用户消息去重 + user-before-assistant。 */
  const normalize = (messages: ChatMsg[]): ChatMsg[] => {
    const out = [...messages];
    let idx = 0;
    while (idx < out.length) {
      const m = out[idx];
      if (m.role !== 'user' || !m.runId || m.messageId || m.clientMessageId) {
        idx += 1;
        continue;
      }
      let scan = idx + 1;
      while (scan < out.length) {
        const n = out[scan];
        const ordinary = !n.messageId && !n.clientMessageId;
        const dupRunUser = ordinary && n.role === 'user' && n.runId === m.runId;
        const dupLocalUser = ordinary && n.role === 'user' && !n.runId && n.text === m.text;
        if (dupRunUser || dupLocalUser) out.splice(scan, 1);
        else scan += 1;
      }
      idx += 1;
    }
    let aidx = 0;
    while (aidx < out.length) {
      const m = out[aidx];
      if (m.role !== 'assistant' || !m.runId) {
        aidx += 1;
        continue;
      }
      const uidx = out.findIndex((n, j) => j > aidx && n.role === 'user' && !n.messageId && !n.clientMessageId && n.runId === m.runId);
      if (uidx > aidx) {
        const [user] = out.splice(uidx, 1);
        out.splice(aidx, 0, user);
        aidx += 2;
      } else {
        aidx += 1;
      }
    }
    return out;
  };

  const closeSse = () => {
    sse?.close();
    sse = null;
  };

  /**
   * 快照回放中(applySnapshot 的事件重放)。
   * D-035:回放里的历史 plan.* 只回填指针,不弹页签——否则每次切会话都会把旧计划
   * 顶到工作台前面。只有实时到达的 plan.* 才自动开页签。
   */
  let replaying = false;

  const applySnapshot = (snap: DesignSnapshot) => {
    seenKeys.clear();
    seenOrder.length = 0;
    set({
      messages: [],
      activeRunId: null,
      todos: snap.todos ?? [],
      tokens: { prompt: 0, completion: 0, total: 0 },
      lastPromptTokens: 0,
      usageContextWindow: null,
      compactions: [],
      latestSeq: snap.latestSeq ?? 0,
      models: snap.models?.models ?? [],
      defaultModelId: snap.models?.defaultModelId ?? null,
      selectedModelId: snap.activeSession?.selectedModelId ?? null,
      thinkingEnabled: snap.activeSession?.thinkingEnabled ?? false,
      reasoningEffort: snap.activeSession?.reasoningEffort ?? null,
      contextOptionId: snap.activeSession?.contextOptionId ?? null,
      hydrating: false,
      pendingPermission: null,
      pendingPermissions: [],
      codexAccount: null,
      codexRateLimits: null,
      latestDiff: null,
    });
    useSessionStore.getState().hydrateAgentDefaults(snap.agents?.defaultEngine);
    // D-035:计划指针回填。放在事件回放之前——回放里的 plan.* 会顺带开页签,
    // 这里只负责「有计划但本会话没新事件」时命令面板仍能找到它。
    usePlanStore.getState().setActivePlanPath(snap.activeSession?.activePlanPath ?? null);
    // D-044:UltraPlan 阶段以快照里的会话字段为准;下面回放的历史 ultraplan.* 只建卡片,
    // 不改阶段(否则会把旧阶段从头走一遍)。有流程时再补拉一次 GET,取问卷 / Demo 坐标等
    // 快照不带的附属数据(回放循环之外发起,故不算回放的副作用)。
    const ultraSessionId = snap.activeSession?.id ?? null;
    useCollaborationStore.getState().reset(ultraSessionId);
    useUltraPlanStore.getState().hydrate(snap.activeSession?.ultraplan ?? null, ultraSessionId);
    if (snap.activeSession?.ultraplan && ultraSessionId) {
      void useUltraPlanStore.getState().refresh(ultraSessionId);
    }
    // D-045:Design 流程同口径(快照为阶段事实源,有流程再补拉附属数据)。
    useDesignFlowStore.getState().hydrate(snap.activeSession?.design ?? null, ultraSessionId);
    if (snap.activeSession?.design && ultraSessionId) {
      void useDesignFlowStore.getState().refresh(ultraSessionId);
    }
    replaying = true;
    try {
      for (const evt of snap.events ?? []) get().applyEvent(evt);
    } finally {
      replaying = false;
    }
    // D-044:回放里历史 run 的收束事件会把「正在调研」复位;阶段机说 Planning turn 仍在跑
    // (刷新 / 重连时赶上写计划那一轮)→ 重新点亮,Plan 页签照常显示进行条。
    if (isPlanningTurn(useUltraPlanStore.getState().state)) usePlanStore.getState().setPlanning(true);
    // 快照回放后 run 仍在运行(进程重启前状态) → activeRunId 如实回填;
    // 反之快照 run 为空(服务端 run 注册表才是活运行唯一事实源)→ 强制清空:
    // 事件日志可能存在 run.created 无终止事件的残留(进程被杀),只靠回放会永久卡幽灵运行态(2026-08-29 实测)。
    if (snap.run && snap.run.status === 'running') set({ activeRunId: snap.run.id });
    else set({ activeRunId: null });
    if (ultraSessionId) void useCollaborationStore.getState().refresh(ultraSessionId);
  };

  const subscribe = (sessionId: string, fromSeq: number) => {
    closeSse();
    set({ streamLink: 'connecting', streamDownSince: null });
    sse = subscribeSessionEvents(sessionId, fromSeq, {
      onEvent: (frame) => {
        if (frame.data && typeof frame.data === 'object') {
          get().applyEvent(frame.data as ForgeEventWire);
        }
      },
      onGap: () => {
        // 超窗/滞后 → 全量重拉快照再以最新 latestSeq 重订(参考 gap 恢复语义)
        void get().resync();
      },
      onOpen: () => set({ streamLink: 'live', streamDownSince: null }),
      onError: () => {
        // 重连由 sseClient 退避承担;这里只记掉线起点(D-047 轮次状态行据此报 Reconnecting /
        // Connection lost),连续失败不刷新起点。离线态另由 StatusBar health 轮询如实呈现。
        set((st) => ({ streamLink: 'down', streamDownSince: st.streamDownSince ?? Date.now() }));
      },
    });
  };

  /**
   * 模型规格落库:先乐观改本地态 → PATCH 会话 → 用响应回写 sessionStore 会话面;
   * 失败只回滚本次改动的项 + toast。无会话时只改本地态——全屏主页首屏尚无会话,
   * 发送建会话时把当前规格带进 POST /sessions,selectSession 回放即继承。
   */
  const patchSpec = async (patch: SpecPatch, failLabel: string): Promise<void> => {
    const sid = useSessionStore.getState().activeSessionId;
    const st = get();
    const prev = Object.fromEntries(
      Object.keys(patch).map((k) => [k, st[k as keyof SpecPatch]]),
    ) as SpecPatch;
    set(patch);
    if (!sid) return;
    try {
      const wirePatch =
        Object.prototype.hasOwnProperty.call(patch, 'selectedModelId') &&
        patch.selectedModelId === null
          ? { ...patch, selectedModelId: '' }
          : patch;
      const r = await apiPatch<{ session: ForgeSession }>(
        `/api/forge/sessions/${encodeURIComponent(sid)}`,
        wirePatch,
      );
      useSessionStore.setState((s) => ({
        sessions: s.sessions.map((x) => (x.id === sid ? { ...x, ...r.session } : x)),
      }));
    } catch (err) {
      set(prev);
      toastError(err, failLabel);
    }
  };

  return {
    messages: [],
    activeRunId: null,
    todos: [],
    tokens: { prompt: 0, completion: 0, total: 0 },
    lastPromptTokens: 0,
    usageContextWindow: null,
    compactions: [],
    compactingSessionId: null,
    latestSeq: 0,
    models: [],
    defaultModelId: null,
    selectedModelId: null,
    thinkingEnabled: false,
    reasoningEffort: null,
    contextOptionId: null,
    hydrating: false,
    currentSessionId: null,
    subagentOverlayId: null,
    pendingPermission: null,
    pendingPermissions: [],
    codexAccount: null,
    codexRateLimits: null,
    latestDiff: null,
    eventsRing: [],
    streamLink: 'idle',
    streamDownSince: null,
    pendingTurnSince: null,

    openSubagent: (id) => set({ subagentOverlayId: id }),

    resolvePermission: (async (
      idOrAllow: string | boolean,
      allowOrOptions?: boolean | ResolvePermissionOptions,
      maybeOptions?: ResolvePermissionOptions,
    ) => {
      const pending = get().pendingPermission;
      const explicitId = typeof idOrAllow === 'string' ? idOrAllow : undefined;
      const id = explicitId ?? pending?.id;
      const allow = typeof idOrAllow === 'boolean' ? idOrAllow : allowOrOptions === true;
      const options =
        typeof idOrAllow === 'boolean'
          ? (allowOrOptions as ResolvePermissionOptions | undefined)
          : maybeOptions;
      if (!id) return;
      const decision = options?.decision ?? (allow ? 'accept' : 'decline');
      try {
        const result = await apiPost<{ ok?: boolean }>(
          `/api/forge/permissions/${encodeURIComponent(id)}/${allow ? 'approve' : 'deny'}`,
          {
            decision,
            ...(options?.answers ? { answers: options.answers } : {}),
          },
        );
        if (result.ok === false) {
          throw new Error('该审批已失效或已被处理');
        }
      } catch (err) {
        toastError(err, allow ? '批准失败' : '拒绝失败');
      }
    }) as ResolvePermission,

    clearEventsRing: () => set({ eventsRing: [] }),

    applyEvent: (evt) => {
      const selectedSessionId = useSessionStore.getState().activeSessionId;
      const boundSessionId = get().currentSessionId;
      // EventSource.close() 不能撤回已排进任务队列的最后一帧。切会话后若不先验
      // sessionId，旧 Codex 的审批/goal/stream 可能污染新会话。
      if (
        (selectedSessionId !== null && evt.sessionId !== selectedSessionId) ||
        (boundSessionId !== null && evt.sessionId !== boundSessionId)
      ) return;
      if (!markSeen(evt)) return;
      const collaboration = useCollaborationStore.getState();
      if (collaboration.sessionId === null) collaboration.reset(evt.sessionId);
      useCollaborationStore.getState().applyEvent(evt);
      // 全类型入原始事件环(底部面板 Agent Logs/Output 数据源;在去重后、语义 switch 前)。
      set((st) => ({ eventsRing: ringPush(st.eventsRing, evt) }));
      const isDelta =
        evt.type === 'agent.token.stream.delta' ||
        evt.type === 'agent.reasoning.delta' ||
        evt.type === 'agent.tool.output.delta' ||
        evt.type === 'agent.plan.delta';
      if (!isDelta) flushStreamNow();
      const time = hhmm(evt.ts);
      const actorId = payloadStr(evt, 'agentId');
      const actor = useCollaborationStore.getState().agents.find((agent) => agent.id === actorId);
      const childEvent = !!parentOf(evt) || !!payloadStr(evt, 'parentAgentId') ||
        (payloadStr(evt, 'agentRole') !== undefined && payloadStr(evt, 'agentRole') !== 'root') ||
        (actor !== undefined && actor.role !== 'root');
      let messages = get().messages;
      // Some durable child reasoning/approval events carry the stable actor but no
      // legacy parentToolCallId. Route them to that actor's card in the parent run.
      if (childEvent && actorId && !parentOf(evt) && !evt.type.startsWith('subagent.')) {
        const displayRun = runIdOf(evt);
        const actualRun = payloadStr(evt, 'agentRunId');
        const parentMessage = messages.find((message) => message.role === 'assistant' && message.runId === displayRun);
        let target = parentMessage && targetForAgent(parentMessage.blocks, actorId, actualRun);
        // Snapshot/reconnect can expose the actor before its legacy started card.
        // A child event displayed in the parent run must still stay in a member card.
        if (!target && ((actualRun && actualRun !== displayRun) || (displayRun && displayRun === get().activeRunId))) {
          target = `agent-turn-${actualRun ?? actorId}`;
          messages = mutateAssistant(messages, displayRun, (message) => {
            ensureSubagent(message.blocks, target!, { agentId: actorId, agentRunId: actualRun, label: actor?.name ?? '子代理任务', status: 'running' });
          });
        }
        if (target) evt = { ...evt, payload: { ...evt.payload, parentToolCallId: target } };
      }
      switch (evt.type) {
        case 'agent.message.queued':
        case 'agent.message.injected':
        case 'agent.message.failed':
        case 'agent.message.recoveryRequired': {
          const payload = evt.payload;
          const target = useCollaborationStore.getState().agents.find((agent) => agent.id === payload?.toAgentId);
          if (!payload || payload.sessionId !== evt.sessionId || payload.source !== 'user' || target?.role !== 'root' || typeof payload.id !== 'string') return;
          messages = upsertUser(messages, {
            id: `agent-message-${payload.id}`,
            messageId: payload.id,
            clientMessageId: typeof payload.clientMessageId === 'string' ? payload.clientMessageId : undefined,
            messageStatus: typeof payload.status === 'string' ? payload.status : undefined,
            annotations: Array.isArray(payload.annotations) ? payload.annotations as EditorAnnotation[] : undefined,
            agentId: target.id,
            agentName: target.name,
            role: 'user', text: String(payload.text ?? ''), blocks: [], status: 'completed',
            time, ts: evt.ts, runId: typeof payload.runId === 'string' ? payload.runId : null,
          });
          break;
        }
        case 'composer.user.message': {
          const user: ChatMsg = {
            id: evt.id || `user-${evt.seq}`,
            role: 'user',
            text: payloadStr(evt, 'text') ?? '',
            annotations: Array.isArray(evt.payload?.annotations) ? evt.payload.annotations as EditorAnnotation[] : undefined,
            blocks: [],
            status: 'completed',
            mode: payloadStr(evt, 'composerMode'),
            time,
            ts: evt.ts,
            runId: runIdOf(evt),
            ...(payloadStr(evt, 'messageId') ? { messageId: payloadStr(evt, 'messageId') } : {}),
            ...(payloadStr(evt, 'clientMessageId') ? { clientMessageId: payloadStr(evt, 'clientMessageId') } : {}),
            ...(payloadStr(evt, 'agentId') ? { agentId: payloadStr(evt, 'agentId') } : {}),
          };
          if (payloadStr(evt, 'source') === 'receipt') user.source = 'receipt';
          // D-044:卡片动作的回显。实时到达 = 后端已受理该动作(act() 的在途态据此收束);
          // 回放的历史回显只打标记,不碰 ultraPlanStore。
          const ultraAction = ultraActionOf(evt.payload?.ultraplan);
          if (ultraAction) {
            user.ultraAction = ultraAction;
            if (!replaying) useUltraPlanStore.getState().noteUserMessage(ultraAction);
          }
          messages = upsertUser(messages, user);
          break;
        }
        case 'agent.started': {
          const runId = runIdOf(evt);
          const parent = parentOf(evt);
          if (parent) {
            messages = mutateAssistant(messages, runId, (message) => {
              ensureSubagent(message.blocks, parent, { status: 'running', agentId: actorId });
            });
            break;
          }
          const model = payloadStr(evt, 'model');
          const engine = payloadStr(evt, 'engine');
          messages = mutateAssistant(messages, runId, (m) => {
            if (m.time === '') m.time = time;
            if (!m.startedTs) m.startedTs = evt.ts;
            if (model) m.model = model;
            if (engine) m.engine = engine;
            if (actorId) m.agentId = actorId;
            if (actor?.name) m.agentName = actor.name;
            m.status = 'streaming';
          });
          if (!childEvent) set({ activeRunId: runId, pendingTurnSince: null });
          break;
        }
        case 'agent.tool.invoked': {
          const runId = runIdOf(evt);
          const name = payloadStr(evt, 'name') ?? 'tool';
          const toolCallId = payloadStr(evt, 'toolCallId') ?? `tool-${evt.seq}`;
          const parent = parentOf(evt);
          toolNames.set(toolCallId, name);
          const args = prettyArgs(evt.payload?.args);
          const toolKind = toolKindOf(name, evt.payload?.toolKind);
          const changes = changesFromPayload(evt.payload);
          messages = mutateAssistant(messages, runId, (m) => {
            m.status = 'streaming';
            const dest = targetBlocks(m.blocks, parent);
            if (bareName(name) === 'task') {
              const a = (evt.payload?.args ?? {}) as Record<string, unknown>;
              const prompt = typeof a.prompt === 'string' ? a.prompt : '';
              const label =
                (typeof a.description === 'string' && a.description) ||
                (typeof a.title === 'string' && a.title) ||
                prompt.split('\n').find((l) => l.trim() !== '')?.trim() ||
                '子代理任务';
              ensureSubagent(parent ? dest : m.blocks, toolCallId, {
                label,
                prompt,
                status: 'running',
                parentToolCallId: parent,
                work: [],
              });
            } else {
              dest.push({
                kind: 'tool',
                toolCallId,
                name,
                args,
                toolKind,
                changes,
                status: 'running',
                mcp: mcpOf(name),
              });
            }
          });
          break;
        }
        case 'agent.tool.completed':
        case 'agent.tool.failed':
        case 'agent.tool.denied': {
          const failed = evt.type !== 'agent.tool.completed';
          const runId = runIdOf(evt);
          const callId = payloadStr(evt, 'toolCallId');
          const durationMs =
            typeof evt.payload?.durationMs === 'number' ? (evt.payload.durationMs as number) : undefined;
          const error = failed
            ? payloadStr(evt, 'error') ?? (evt.type === 'agent.tool.denied' ? '工具被拒绝' : '工具执行失败')
            : undefined;
          const result = failed
            ? payloadStr(evt, 'output')
            : payloadStr(evt, 'output') ?? payloadStr(evt, 'outputPreview');
          const exitCode =
            typeof evt.payload?.exitCode === 'number' ? (evt.payload.exitCode as number) : undefined;
          const changes = changesFromPayload(evt.payload);
          const eventName = payloadStr(evt, 'name');
          const eventToolKind = eventName
            ? toolKindOf(eventName, evt.payload?.toolKind)
            : undefined;
          messages = mutateAssistant(messages, runId, (m) => {
            settleToolOrSub(m.blocks, callId, {
              ok: !failed,
              error,
              durationMs,
              ...(result !== undefined ? { result, output: result } : {}),
              ...(exitCode !== undefined ? { exitCode } : {}),
              ...(changes ? { changes } : {}),
              ...(eventToolKind ? { toolKind: eventToolKind } : {}),
              status: failed ? 'error' : 'done',
              subStatus: failed ? 'error' : 'done',
              subSummary: error,
            });
          });
          const settledName = callId ? toolNames.get(callId) : undefined;
          if (callId) toolNames.delete(callId);
          if (settledName !== undefined) notifyAgentToolSettled(settledName);
          break;
        }
        case 'agent.image.generated': {
          const url = payloadStr(evt, 'url');
          const toolCallId = payloadStr(evt, 'toolCallId');
          const imageFileRef = payloadStr(evt, 'imageFileRef');
          if (!url?.startsWith('/api/forge/gen/image/file?') || !toolCallId || !imageFileRef) break;
          messages = mutateAssistant(messages, runIdOf(evt), (m) => {
            const dest = targetBlocks(m.blocks, parentOf(evt));
            if (dest.some((b) => b.kind === 'image' && b.toolCallId === toolCallId)) return;
            dest.push({ kind: 'image', toolCallId, url, imageFileRef, revisedPrompt: payloadStr(evt, 'revisedPrompt') });
          });
          break;
        }
        case 'agent.token.stream.delta': {
          const delta = payloadStr(evt, 'delta') ?? payloadStr(evt, 'text') ?? '';
          if (delta !== '') enqueueStream(runIdOf(evt), parentOf(evt), 'token', delta, evt.ts);
          return;
        }
        case 'agent.reasoning.delta': {
          const delta = payloadStr(evt, 'delta') ?? payloadStr(evt, 'text') ?? '';
          if (delta !== '') enqueueStream(runIdOf(evt), parentOf(evt), 'reasoning', delta, evt.ts);
          return;
        }
        case 'agent.tool.output.delta': {
          const delta = payloadStr(evt, 'delta') ?? payloadStr(evt, 'output') ?? '';
          if (delta !== '') {
            enqueueToolOutput(
              runIdOf(evt),
              parentOf(evt),
              payloadStr(evt, 'toolCallId'),
              delta,
            );
          }
          return;
        }
        case 'agent.plan.delta': {
          const delta = payloadStr(evt, 'delta') ?? payloadStr(evt, 'text') ?? '';
          if (delta !== '') enqueueStream(runIdOf(evt), parentOf(evt), 'plan', delta, evt.ts);
          return;
        }
        case 'agent.reasoning': {
          const text = payloadStr(evt, 'text') ?? '';
          const runId = runIdOf(evt);
          messages = mutateAssistant(messages, runId, (m) => {
            const dest = targetBlocks(m.blocks, parentOf(evt));
            // 终稿覆盖本轮思考块:流式时思考在前、正文在后,尾块已是 text,
            // 只认尾块会再插一条 Thought,一句话看起来重复两遍。
            settleReasoning(dest, text, evt.ts);
          });
          break;
        }
        case 'agent.tool.args.delta': {
          const runId = runIdOf(evt);
          const callId = payloadStr(evt, 'toolCallId');
          const name = payloadStr(evt, 'name') ?? 'tool';
          const delta = payloadStr(evt, 'delta') ?? '';
          const parent = parentOf(evt);
          messages = mutateAssistant(messages, runId, (m) => {
            const dest = targetBlocks(m.blocks, parent);
            let found = false;
            for (let i = dest.length - 1; i >= 0; i -= 1) {
              const b = dest[i];
              if (b.kind === 'tool' && (callId ? b.toolCallId === callId : toolStatus(b) === 'running')) {
                dest[i] = { ...b, args: (b.args || '') + delta };
                found = true;
                break;
              }
            }
            if (!found) {
              dest.push({
                kind: 'tool',
                toolCallId: callId ?? `tool-${evt.seq}`,
                name,
                args: delta,
                toolKind: toolKindOf(name, evt.payload?.toolKind),
                status: 'running',
                mcp: mcpOf(name),
              });
            }
          });
          break;
        }
        case 'agent.stream.reset': {
          const runId = runIdOf(evt);
          streamBuf.delete(streamKey(runId, parentOf(evt)));
          messages = mutateAssistant(messages, runId, (m) => {
            resetStreamingBlocks(targetBlocks(m.blocks, parentOf(evt)));
          });
          break;
        }
        case 'subagent.started': {
          const runId = runIdOf(evt) ?? payloadStr(evt, 'parentRunId') ?? null;
          const id =
            payloadStr(evt, 'subRunId') ??
            payloadStr(evt, 'subagentId') ??
            payloadStr(evt, 'parentToolCallId') ??
            `sub-${evt.seq}`;
          const prompt = payloadStr(evt, 'prompt') ?? '';
          const label = payloadStr(evt, 'description') ?? payloadStr(evt, 'label') ?? '子代理任务';
          // D-036:后台子代理(detached)自己占一张助手卡 —— 它的 parentRunId 就是自己的
          // 后台 runId,派它的那一轮早已收束。卡片元数据(时间/模型/流式态)此前只由
          // agent.started 填,而后台腿刻意不发 agent.started(发了会顶掉前端 activeRunId、
          // 锁住输入框),故在这里补齐,否则卡片没有时间戳、状态点永远停在初始态。
          const detached = evt.payload?.detached === true;
          const model = payloadStr(evt, 'model');
          messages = mutateAssistant(messages, runId, (m) => {
            if (detached) {
              if (m.time === '') m.time = time;
              if (!m.startedTs) m.startedTs = evt.ts;
              if (model) m.model = model;
              m.status = 'streaming';
            }
            ensureSubagent(m.blocks, id, {
              label,
              prompt,
              status: 'running',
              parentToolCallId: payloadStr(evt, 'parentToolCallId') ?? id,
              agentId: payloadStr(evt, 'agentId'),
              agentRunId: payloadStr(evt, 'agentRunId') ?? payloadStr(evt, 'subRunId'),
              // Stop 打后台 run 本身(父轮已结束,全局 activeRunId 为空)。
              detachedRunId: detached ? runId ?? undefined : undefined,
            });
          });
          break;
        }
        case 'subagent.completed':
        case 'subagent.failed': {
          const runId = runIdOf(evt) ?? payloadStr(evt, 'parentRunId') ?? null;
          const id =
            payloadStr(evt, 'subRunId') ??
            payloadStr(evt, 'subagentId') ??
            payloadStr(evt, 'parentToolCallId');
          const failed = evt.type === 'subagent.failed';
          const summary =
            payloadStr(evt, 'summary') ?? payloadStr(evt, 'result') ?? payloadStr(evt, 'error');
          messages = mutateAssistant(messages, runId, (m) => {
            if (id) {
              const sub = ensureSubagent(m.blocks, id, {
                status: failed ? 'error' : 'done',
                summary,
                agentId: actorId,
                agentRunId: payloadStr(evt, 'agentRunId'),
              });
              expireApprovals(sub.work);
            }
          });
          if (actorId) {
            const queue = get().pendingPermissions.filter((permission) => permission.agentId !== actorId);
            set({ pendingPermissions: queue, pendingPermission: queue[0] ?? null });
          }
          break;
        }
        case 'permission.requested': {
          const id = payloadStr(evt, 'id');
          const tool = payloadStr(evt, 'tool') ?? 'tool';
          const approvalKind = payloadStr(evt, 'kind') ?? 'command';
          const pending: PendingPermission | null = id
            ? {
                id,
                runId: payloadStr(evt, 'agentRunId') ?? runIdOf(evt),
                agentId: payloadStr(evt, 'agentId'),
                agentName: payloadStr(evt, 'agentName'),
                tool,
                approvalKind,
                ...(payloadStr(evt, 'command') ? { command: payloadStr(evt, 'command') } : {}),
                ...(payloadStr(evt, 'cwd') ? { cwd: payloadStr(evt, 'cwd') } : {}),
                ...(changesFromPayload(evt.payload) ? { changes: changesFromPayload(evt.payload) } : {}),
                ...(payloadStr(evt, 'reason') ? { reason: payloadStr(evt, 'reason') } : {}),
                ...(payloadStr(evt, 'message') ? { message: payloadStr(evt, 'message') } : {}),
                ...(normalizeQuestions(evt.payload?.questions)
                  ? { questions: normalizeQuestions(evt.payload?.questions) }
                  : {}),
                ...(normalizeRecord(evt.payload?.permissions)
                  ? { permissions: normalizeRecord(evt.payload?.permissions) }
                  : {}),
                ...(normalizeRecord(evt.payload?.schema)
                  ? { schema: normalizeRecord(evt.payload?.schema) }
                  : {}),
                ...(normalizeRecord(evt.payload?.networkApprovalContext)
                  ? { networkApprovalContext: normalizeRecord(evt.payload?.networkApprovalContext) }
                  : {}),
                ...(normalizeStrings(evt.payload?.proposedExecpolicyAmendment)
                  ? { proposedExecpolicyAmendment: normalizeStrings(evt.payload?.proposedExecpolicyAmendment) }
                  : {}),
                ...(normalizeRecords(evt.payload?.proposedNetworkPolicyAmendments)
                  ? { proposedNetworkPolicyAmendments: normalizeRecords(evt.payload?.proposedNetworkPolicyAmendments) }
                  : {}),
                ...(payloadStr(evt, 'grantRoot') ? { grantRoot: payloadStr(evt, 'grantRoot') } : {}),
                ...(payloadStr(evt, 'mode') ? { mode: payloadStr(evt, 'mode') } : {}),
                ...(payloadStr(evt, 'serverName') ? { serverName: payloadStr(evt, 'serverName') } : {}),
                ...(payloadStr(evt, 'url') ? { url: payloadStr(evt, 'url') } : {}),
                ...(payloadStr(evt, 'elicitationId') ? { elicitationId: payloadStr(evt, 'elicitationId') } : {}),
                ...(normalizeDecisions(evt.payload?.availableDecisions)
                  ? { availableDecisions: normalizeDecisions(evt.payload?.availableDecisions) }
                  : {}),
              }
            : null;
          if (pending) {
            const queue = [...get().pendingPermissions.filter((item) => item.id !== pending.id), pending];
            set({ pendingPermissions: queue, pendingPermission: queue[0] ?? null });
            messages = mutateAssistant(messages, runIdOf(evt), (message) => {
              const destination = targetBlocks(message.blocks, parentOf(evt));
              if (destination.some((block) => block.kind === 'approval' && block.id === pending.id)) return;
              destination.push({
                kind: 'approval',
                id: pending.id,
                agentId: pending.agentId,
                agentName: pending.agentName,
                approvalKind: pending.approvalKind,
                tool: pending.tool,
                command: pending.command,
                cwd: pending.cwd,
                changes: pending.changes,
                reason: pending.reason,
                message: pending.message,
                questions: pending.questions,
                permissions: pending.permissions,
                schema: pending.schema,
                networkApprovalContext: pending.networkApprovalContext,
                proposedExecpolicyAmendment: pending.proposedExecpolicyAmendment,
                proposedNetworkPolicyAmendments: pending.proposedNetworkPolicyAmendments,
                grantRoot: pending.grantRoot,
                mode: pending.mode,
                serverName: pending.serverName,
                url: pending.url,
                elicitationId: pending.elicitationId,
                availableDecisions: pending.availableDecisions,
              });
            });
          }
          useToastStore.getState().push('warning', `工具 ${tool} 需要批准`);
          break;
        }
        case 'permission.resolved': {
          const id = payloadStr(evt, 'id');
          const allowed = evt.payload?.allowed === true;
          const decision =
            payloadStr(evt, 'decision') ?? (allowed ? 'accept' : 'decline');
          const rawAnswers = evt.payload?.answers;
          const answers = normalizeRecord(rawAnswers);
          const queue = get().pendingPermissions.filter((item) => item.id !== id);
          set({ pendingPermissions: queue, pendingPermission: queue[0] ?? null });
          messages = mutateAssistant(messages, runIdOf(evt), (message) => {
            settleApproval(message.blocks, id, decision, answers);
          });
          useToastStore.getState().push(allowed ? 'success' : 'info', allowed ? '已批准工具执行' : '已拒绝工具执行');
          break;
        }
        case 'agent.message': {
          const runId = runIdOf(evt);
          const text = payloadStr(evt, 'text') ?? '';
          const provider = payloadStr(evt, 'provider');
          messages = mutateAssistant(messages, runId, (m) => {
            if (provider) m.provider = provider;
            settleFinalText(targetBlocks(m.blocks, parentOf(evt)), text);
          });
          break;
        }
        case 'agent.completed': {
          const runId = runIdOf(evt);
          const text = payloadStr(evt, 'text') ?? '';
          const parent = parentOf(evt);
          if (parent) {
            messages = mutateAssistant(messages, runId, (message) => {
              const work = targetBlocks(message.blocks, parent);
              if (text) settleFinalText(work, text);
              expireApprovals(work);
              ensureSubagent(message.blocks, parent, { status: 'done' });
            });
            const queue = get().pendingPermissions.filter((permission) => actorId ? permission.agentId !== actorId : permission.runId !== payloadStr(evt, 'agentRunId'));
            set({ pendingPermissions: queue, pendingPermission: queue[0] ?? null });
            break;
          }
          messages = mutateAssistant(messages, runId, (m) => {
            if (text !== '') settleFinalText(m.blocks, text);
            expireApprovals(m.blocks);
            m.status = 'completed';
            m.finishedTs = evt.ts;
          });
          const queue = get().pendingPermissions.filter((item) => item.runId !== runId);
          set({ pendingPermissions: queue, pendingPermission: queue[0] ?? null });
          if (!childEvent && get().activeRunId === runId) set({ activeRunId: null });
          if (!childEvent) set({ pendingTurnSince: null });
          if (!childEvent) usePlanStore.getState().setPlanning(false);
          break;
        }
        case 'agent.failed': {
          const runId = runIdOf(evt);
          const error = payloadStr(evt, 'error');
          const code = payloadStr(evt, 'code');
          const parent = parentOf(evt);
          if (parent) {
            messages = mutateAssistant(messages, runId, (message) => {
              expireApprovals(targetBlocks(message.blocks, parent));
              ensureSubagent(message.blocks, parent, { status: 'error', summary: error });
            });
            const queue = get().pendingPermissions.filter((permission) => actorId ? permission.agentId !== actorId : permission.runId !== payloadStr(evt, 'agentRunId'));
            set({ pendingPermissions: queue, pendingPermission: queue[0] ?? null });
            break;
          }
          messages = mutateAssistant(messages, runId, (m) => {
            expireApprovals(m.blocks);
            m.status = 'failed';
            m.finishedTs = evt.ts;
            if (error) m.error = error;
            if (code) m.errorCode = code;
          });
          const queue = get().pendingPermissions.filter((item) => item.runId !== runId);
          set({ pendingPermissions: queue, pendingPermission: queue[0] ?? null });
          if (!childEvent && get().activeRunId === runId) set({ activeRunId: null });
          // 开轮前就失败(没有 agent.started)也要撤掉「等待开轮」
          if (!childEvent) set({ pendingTurnSince: null });
          if (!childEvent) usePlanStore.getState().setPlanning(false);
          break;
        }
        case 'agent.cancelled': {
          const runId = runIdOf(evt);
          const parent = parentOf(evt);
          if (parent) {
            messages = mutateAssistant(messages, runId, (message) => {
              expireApprovals(targetBlocks(message.blocks, parent));
              ensureSubagent(message.blocks, parent, { status: 'error', summary: '已中止' });
            });
            const queue = get().pendingPermissions.filter((permission) => actorId ? permission.agentId !== actorId : permission.runId !== payloadStr(evt, 'agentRunId'));
            set({ pendingPermissions: queue, pendingPermission: queue[0] ?? null });
            break;
          }
          messages = mutateAssistant(messages, runId, (m) => {
            expireApprovals(m.blocks);
            m.status = 'cancelled';
            m.finishedTs = evt.ts;
          });
          const queue = get().pendingPermissions.filter((item) => item.runId !== runId);
          set({ pendingPermissions: queue, pendingPermission: queue[0] ?? null });
          if (!childEvent && get().activeRunId === runId) set({ activeRunId: null });
          if (!childEvent) set({ pendingTurnSince: null });
          if (!childEvent) usePlanStore.getState().setPlanning(false);
          break;
        }
        case 'agent.usage': {
          const p = evt.payload ?? {};
          const num = (k: string) => (typeof p[k] === 'number' ? (p[k] as number) : 0);
          const t = get().tokens;
          const prompt = num('promptTokens');
          // Codex 的 promptTokens 是线程累计;最近一次请求真正送进模型的体量在 last 里(本地引擎无 last)。
          const last = p.last && typeof p.last === 'object' ? (p.last as Record<string, unknown>) : null;
          const current = typeof last?.promptTokens === 'number' ? last.promptTokens : prompt;
          const window = num('modelContextWindow');
          set({
            tokens: {
              prompt: t.prompt + prompt,
              completion: t.completion + num('completionTokens'),
              total: t.total + num('totalTokens'),
            },
            lastPromptTokens: !childEvent && current > 0 ? current : get().lastPromptTokens,
            usageContextWindow: !childEvent && window > 0 ? window : get().usageContextWindow,
          });
          return;
        }
        case 'context.compacted': {
          const p = evt.payload ?? {};
          set({
            compactions: [
              ...get().compactions,
              {
                afterMessageId: messages[messages.length - 1]?.id ?? null,
                ts: evt.ts,
                engine: typeof p.engine === 'string' ? p.engine : '',
                summaryTokens: typeof p.tokensAfter === 'number' ? p.tokensAfter : null,
              },
            ],
            // 实测值量的是压缩前的请求,已经失真;下一轮 usage 到来之前改走估算。
            lastPromptTokens: 0,
          });
          return;
        }
        case 'agent.diff.updated': {
          const diff = evt.payload?.diff ?? null;
          if (!childEvent) set({ latestDiff: diff });
          if (typeof diff === 'string' && diff !== '') {
            messages = mutateAssistant(messages, runIdOf(evt), (message) => {
              mergeDiffIntoLastFileChange(targetBlocks(message.blocks, parentOf(evt)), diff);
            });
          } else {
            return;
          }
          break;
        }
        case 'goal.updated':
        case 'goal.cleared': {
          useGoalStore.getState().applyGoalEvent(evt, replaying);
          return;
        }
        case 'codex.rateLimits.updated': {
          const value = evt.payload?.rateLimits;
          set({
            codexRateLimits:
              value && typeof value === 'object' && !Array.isArray(value)
                ? (value as Record<string, unknown>)
                : null,
          });
          return;
        }
        case 'codex.account.updated': {
          const value = evt.payload?.account;
          set({
            codexAccount:
              value && typeof value === 'object' && !Array.isArray(value)
                ? (value as Record<string, unknown>)
                : null,
          });
          return;
        }
        case 'todo.created':
        case 'todo.updated': {
          const p = evt.payload ?? {};
          const id = typeof p.id === 'string' ? p.id : null;
          if (!id) return;
          const todos = [...get().todos];
          const idx = todos.findIndex((t) => t.id === id);
          const patch: Partial<TodoItem> = {};
          if (typeof p.title === 'string') patch.title = p.title;
          if (typeof p.status === 'string') patch.status = p.status;
          if (typeof p.kind === 'string') patch.kind = p.kind;
          if ('summary' in p) patch.summary = (p.summary as string | null) ?? null;
          if ('description' in p) patch.description = (p.description as string | null) ?? null;
          // D-035:计划来源标记——Plan 页签靠 planTodoId 把清单与实时状态对上。
          if (typeof p.planTodoId === 'string') patch.planTodoId = p.planTodoId;
          if (typeof p.source === 'string') patch.source = p.source;
          if (idx >= 0) todos[idx] = { ...todos[idx], ...patch };
          else {
            todos.push({
              id,
              title: patch.title ?? id,
              status: patch.status ?? 'queued',
              kind: patch.kind,
              summary: patch.summary,
              description: patch.description,
              planTodoId: patch.planTodoId,
              source: patch.source,
            });
          }
          set({ todos });
          return;
        }
        // D-035:计划落盘 / 覆盖 → 回填路径、开(或刷新)Plan 页签。
        case 'plan.created':
        case 'plan.updated': {
          const path = payloadStr(evt, 'path');
          if (!path) return;
          messages = mutateAssistant(messages, runIdOf(evt), (message) => {
            finalizePlan(message.blocks, path);
          });
          set({ messages: normalize(messages) });
          if (replaying) usePlanStore.getState().setActivePlanPath(path);
          else usePlanStore.getState().onPlanEvent(path, evt.type === 'plan.created');
          return;
        }
        // Build 起步:此刻计划待办已物化,后续 todo.* 会带 planTodoId 回来。
        case 'plan.build.started':
          return;
        // D-044:关口卡片(问卷 / Demo / 计划 / 验收清单 / 完成)。事件持久化且带全量载荷,
        // 回放与实时走同一条建卡路径;差别只在末尾——实时才通知 ultraPlanStore 重拉阶段。
        case 'ultraplan.questionnaire':
        case 'ultraplan.demo.ready':
        case 'ultraplan.plan.ready':
        case 'ultraplan.acceptance.ready':
        case 'ultraplan.done': {
          const meta = ULTRA_CARD_EVENTS[evt.type];
          const payload = evt.payload ?? {};
          const upId = payloadStr(evt, 'id');
          if (!upId) return;
          const block: UltraBlock = {
            kind: 'ultraplan',
            step: meta.step,
            upId,
            rev: payloadRev(payload, meta.revKey),
            payload,
          };
          // 只认载荷里的 runId(契约:turn 内事件必带,REST 事件不带),不回落 correlationId——
          // REST 事件的 correlationId 不是 run,回落会凭空造出一条 streaming 空消息。
          messages = mutateUltraTarget(messages, payloadStr(evt, 'runId') ?? null, evt, (m) => {
            upsertUltraBlock(m.blocks, block);
          });
          if (!replaying) {
            useUltraPlanStore.getState().applyLiveEvent(evt);
            // 新一版 Demo 就绪 → 开(或激活)Demo 页签,同 plan.created 开 Plan 页签;回放不开,
            // 否则每次切会话都会把旧 Demo 顶到工作台前面。
            const sessionId = evt.sessionId || useSessionStore.getState().activeSessionId;
            if (evt.type === 'ultraplan.demo.ready' && sessionId) {
              const flow = useUltraPlanStore.getState().state;
              useWorkbenchStore.getState().openDemo(upId, sessionId, flow?.id === upId ? flow.title : '');
            }
            if (evt.type === 'ultraplan.plan.ready' && sessionId && typeof payload.planPath === 'string') {
              useWorkbenchStore.getState().openPlan(payload.planPath, { upId, sessionId });
            }
          }
          break;
        }
        // 用户在某张卡上提交过什么(答案 / 通过或要求修改 / 验收结果):回填到那张卡,
        // 卡片随即只读,回放后也能看到当时的提交内容。
        case 'ultraplan.answers.submitted':
        case 'ultraplan.demo.decision':
        case 'ultraplan.acceptance.recorded': {
          const meta = ULTRA_SUBMIT_EVENTS[evt.type];
          const payload = evt.payload ?? {};
          const upId = payloadStr(evt, 'id');
          if (upId) {
            messages = markUltraSubmitted(messages, upId, meta.step, payloadRev(payload, meta.revKey), payload);
          }
          if (!replaying) useUltraPlanStore.getState().applyLiveEvent(evt);
          if (!upId) return;
          break;
        }
        // 其余 ultraplan.* 不进时间线(阶段 / 提示 / 注入统计 / 制作起步由状态条与既有
        // subagent.*、todo.* 呈现);实时到达时只触发阶段重拉。
        case 'ultraplan.started':
        case 'ultraplan.stage':
        case 'ultraplan.notice':
        case 'ultraplan.cleared':
        case 'ultraplan.context.injected':
        case 'ultraplan.spec.written':
        case 'ultraplan.production.started': {
          if (!replaying) useUltraPlanStore.getState().applyLiveEvent(evt);
          return;
        }
        // D-045:Design 流程卡(审阅 / 元素清单 / 验收 / 完成),回放与实时同一条建卡路径。
        case 'design.review.ready':
        case 'design.layout.ready':
        case 'design.verify.result':
        case 'design.done': {
          const meta = DESIGN_CARD_EVENTS[evt.type];
          const payload = evt.payload ?? {};
          const flowId = payloadStr(evt, 'id');
          if (!flowId) return;
          const block: DesignBlock = {
            kind: 'design',
            step: meta.step,
            flowId,
            rev: payloadRev(payload, meta.revKey),
            payload,
          };
          messages = mutateUltraTarget(messages, payloadStr(evt, 'runId') ?? null, evt, (m) => {
            upsertDesignBlock(m.blocks, block);
          });
          if (!replaying) useDesignFlowStore.getState().applyLiveEvent(evt);
          break;
        }
        case 'design.decision': {
          const payload = evt.payload ?? {};
          const flowId = payloadStr(evt, 'id');
          if (flowId && typeof payload.rev === 'number') {
            messages = markDesignDecided(messages, flowId, payload.rev, payload);
          }
          if (!replaying) useDesignFlowStore.getState().applyLiveEvent(evt);
          if (!flowId) return;
          break;
        }
        case 'design.started':
        case 'design.stage':
        case 'design.notice':
        case 'design.candidates.generated':
        case 'design.assets.ready':
        case 'design.scene.built': {
          if (!replaying) useDesignFlowStore.getState().applyLiveEvent(evt);
          return;
        }
        case 'session.updated': {
          // 转发 sessionStore 刷新(标题/置顶/模型等元信息面)
          void useSessionStore.getState().loadAll();
          if (!replaying && useDesignFlowStore.getState().state !== null) {
            void useDesignFlowStore.getState().refresh(evt.sessionId);
          }
          // D-044:会话字段变了且本会话有流程(如「重新开始」)→ 阶段一并重拉。
          if (!replaying && useUltraPlanStore.getState().state !== null) {
            void useUltraPlanStore.getState().refresh(evt.sessionId);
          }
          return;
        }
        default:
          return;
      }
      set({ messages: normalize(messages) });
    },

    selectSession: (id) => {
      // Composer 无会话直发与 ChatColumn 的 activeSessionId 副作用可能在同一拍撞上。
      // 同目标共享一条选择 Promise，既避免重复 GET/reset，也保证 Composer 等到 SSE
      // 已订阅后才发送；已经就绪的同会话则直接幂等返回。
      if (pendingSelection?.id === id) return pendingSelection.promise;
      if (id !== null && get().currentSessionId === id && !get().hydrating) {
        return Promise.resolve();
      }

      const token = (selectToken += 1);
      const run = async () => {
        if (token !== selectToken) return;
        closeSse();
        get().reset();
        if (!id) {
          useGoalStore.getState().reset();
          try {
            const snap = await apiGet<DesignSnapshot>('/api/forge/design-snapshot');
            if (token !== selectToken) return;
            applySnapshot(snap);
          } catch {
            // 后端离线等错误交由全局 offline 呈现
          }
          return;
        }

        set({ hydrating: true });
        let reportedFailure = false;
        for (;;) {
          let snap: DesignSnapshot;
          try {
            snap = await apiGet<DesignSnapshot>(
              `/api/forge/design-snapshot?sessionId=${encodeURIComponent(id)}`,
            );
          } catch (err) {
            if (token !== selectToken) return;
            // currentSessionId 只代表“快照已回放且 SSE 已订阅”，失败期间保持 null，
            // 不再留下会让 ChatColumn 永久误判为已就绪的假绑定。
            if (!reportedFailure) {
              reportedFailure = true;
              toastError(err, '会话快照加载失败');
            }
            await new Promise<void>((resolve) => {
              setTimeout(resolve, SESSION_SNAPSHOT_RETRY_MS);
            });
            if (token !== selectToken) return;
            continue;
          }
          if (token !== selectToken) return; // 已切走,丢弃陈旧快照
          applySnapshot(snap);
          void useGoalStore.getState().refreshGoal(id);
          subscribe(id, snap.latestSeq ?? 0);
          set({ currentSessionId: id });
          return;
        }
      };

      // 先登记再在微任务中启动，确保同步触发的第二个同目标调用也能复用 Promise。
      const promise = Promise.resolve().then(run);
      const pending = { id, promise };
      pendingSelection = pending;
      void promise.then(
        () => {
          if (pendingSelection === pending) pendingSelection = null;
        },
        () => {
          if (pendingSelection === pending) pendingSelection = null;
        },
      );
      return promise;
    },

    resync: async () => {
      const id = useSessionStore.getState().activeSessionId;
      if (!id) return;
      // resync 可能由旧 SSE 的 gap 回调触发，并与用户切换会话并发。复用
      // selectSession 的代次令牌：只要期间发生过新的选择，本次旧快照就不得回盖状态，
      // 更不能 subscribe(old) 把新会话刚建好的 SSE 关掉。
      const token = selectToken;
      try {
        const snap = await apiGet<DesignSnapshot>(
          `/api/forge/design-snapshot?sessionId=${encodeURIComponent(id)}`,
        );
        if (
          token !== selectToken ||
          useSessionStore.getState().activeSessionId !== id
        ) return;
        applySnapshot(snap);
        void useGoalStore.getState().refreshGoal(id);
        subscribe(id, snap.latestSeq ?? 0);
      } catch (err) {
        // 已切走的旧请求失败也不应在新会话里弹错误。
        if (
          token !== selectToken ||
          useSessionStore.getState().activeSessionId !== id
        ) return;
        toastError(err, '会话快照重拉失败');
      }
    },

    sendMessage: async (text, mode, skills, opts) => {
      const sid = useSessionStore.getState().activeSessionId;
      const trimmed = text.trim();
      if (trimmed === '' && !opts?.annotations?.length) return false;
      if (!sid) {
        useToastStore.getState().push('error', '请先选择会话');
        return;
      }
      // 乐观回显:local-* 用户卡(composer.user.message 事件到达后 upsert 替换)
      localCounter += 1;
      const local: ChatMsg = {
        id: `local-${localCounter}`,
        role: 'user',
        text: trimmed,
        annotations: opts?.annotations,
        blocks: [],
        status: 'completed',
        mode,
        time: '',
        runId: null,
      };
      // D-047:助手卡要等 agent.started 才建(Codex 冷启动可达数秒),先记下发出时刻,
      // 用户卡下面立刻就有「Planning next moves」,而不是一片空白。
      set((st) => ({ messages: [...st.messages, local], pendingTurnSince: Date.now() }));
      // plan 模式:开跑即进「调研中」态,Plan 页签据此显示进行条(终态由 run 收束事件复位)。
      // D-044:ultraplan 模式的自由文本只有在计划关口才是「改计划」(Planning turn),此时同样亮;
      // 其余关口(新流程 / 补充需求 / 重出问卷 / 改 Demo)不产出计划,不亮。
      const planningTurn =
        mode === 'plan' ||
        (mode === 'ultraplan' && useUltraPlanStore.getState().state?.stage === 'plan_review');
      if (planningTurn) usePlanStore.getState().setPlanning(true);
      try {
        await apiPost(`/api/forge/sessions/${encodeURIComponent(sid)}/ask:execute`, {
          userInput: trimmed,
          mode,
          ...(skills !== undefined && skills.length > 0 ? { skills } : {}),
          ...(opts?.planPath !== undefined ? { planPath: opts.planPath } : {}),
          ...(opts?.annotations?.length ? { annotations: opts.annotations } : {}),
        });
        return true;
      } catch (err) {
        usePlanStore.getState().setPlanning(false);
        // 后端没收下这一轮:「等待开轮」随之撤掉(三种失败分支同口径)
        set({ pendingTurnSince: null });
        // D-038:会话被服务端自起的回执唤醒轮占用(agent.started 尚未到达的那几毫秒里点了发送)。
        // 这条消息后端没收,乐观回显必须撤掉——留着就是一张永远没有回应的幽灵用户卡。
        if (err instanceof ForgeApiError && err.code === 'SESSION_BUSY') {
          set((st) => ({ messages: st.messages.filter((m) => m.id !== local.id) }));
          useToastStore.getState().push('warning', '主 agent 正在处理后台回执,请等它收束后再发');
          return false;
        }
        // D-044:UltraPlan 的阶段 / 权限校验在建 run 之前就 4xx(如流程正在制作时又发了自由文本)。
        // 消息同样没被收下:撤掉乐观卡,原样提示后端给的原因,并重拉阶段(本地多半已过期)。
        if (
          err instanceof ForgeApiError &&
          err.status >= 400 &&
          err.status < 500 &&
          err.code.startsWith('ULTRAPLAN_')
        ) {
          set((st) => ({ messages: st.messages.filter((m) => m.id !== local.id) }));
          useToastStore.getState().push('warning', err.message === '' ? err.code : err.message);
          void useUltraPlanStore.getState().refresh(sid);
          return false;
        }
        toastError(err, '提交失败');
        return false;
      }
    },

    steerAgent: async (text, agentId, clientMessageId = crypto.randomUUID(), annotations) => {
      const body = text.trim();
      const sid = useSessionStore.getState().activeSessionId;
      if ((!body && !annotations?.length) || !sid) return false;
      const store = useCollaborationStore.getState();
      if (store.sessionId !== sid || !store.agents.length) await store.refresh(sid);
      if (useSessionStore.getState().activeSessionId !== sid) return false;
      const participant = useCollaborationStore.getState().agents.find((agent) => agentId ? agent.id === agentId : agent.role === 'root');
      if (!participant) {
        useToastStore.getState().push('warning', '暂未取得 agent 身份，请稍后重试');
        return false;
      }
      try {
        const message = await useCollaborationStore.getState().send(participant.id, {
          text: body, clientMessageId,
          ...(annotations?.length ? { annotations } : {}),
          ...(participant.activeRunId ? { expectedRunId: participant.activeRunId } : {}),
        });
        if (useSessionStore.getState().activeSessionId === sid) get().applyEvent({
          id: `mailbox-ack-${message.id}-${message.status}`,
          sessionId: sid, seq: get().latestSeq, type: `agent.message.${message.status}`,
          ts: message.createdAt, payload: { ...message },
        });
        return true;
      } catch (err) {
        if (useSessionStore.getState().activeSessionId === sid) toastError(err, '消息未确认送达，草稿已保留');
        return false;
      }
    },

    cancelRun: async () => {
      const runId = get().activeRunId;
      if (!runId) return;
      try {
        const result = await apiPost<{ ok?: boolean }>(
          `/api/forge/runs/${encodeURIComponent(runId)}/cancel`,
          {},
        );
        if (result.ok === false) {
          await get().resync();
          useToastStore.getState().push('info', '运行已经结束，状态已同步');
        }
      } catch (err) {
        toastError(err, '中止失败');
      }
    },

    compactContext: async () => {
      const sessionId = get().currentSessionId;
      if (!sessionId || get().compactingSessionId !== null) return;
      set({ compactingSessionId: sessionId });
      try {
        const r = await compactSession(sessionId);
        useToastStore
          .getState()
          .push('success', r.turns ? `上下文已压缩:${r.turns} 轮对话已总结为摘要` : '上下文已压缩');
      } catch (err) {
        if (err instanceof ForgeApiError && err.code === 'AGENT_COMPACT_NOTHING') {
          useToastStore.getState().push('info', '没有需要压缩的新对话');
        } else {
          toastError(err, '压缩上下文失败');
        }
      } finally {
        if (get().compactingSessionId === sessionId) set({ compactingSessionId: null });
      }
    },

    cancelSubagent: async (runId) => {
      if (!runId) return;
      try {
        const result = await apiPost<{ ok?: boolean }>(
          `/api/forge/runs/${encodeURIComponent(runId)}/cancel`,
          {},
        );
        if (result.ok === false) {
          useToastStore.getState().push('info', '子代理已经结束');
        }
      } catch (err) {
        toastError(err, '中止子代理失败');
      }
    },

    editAndResend: async (msgId, newText, mode) => {
      const st = get();
      const sid = useSessionStore.getState().activeSessionId;
      const msg = st.messages.find((m) => m.id === msgId);
      if (!msg || msg.role !== 'user') return;
      if (!sid) {
        useToastStore.getState().push('error', '请先选择会话');
        return;
      }
      if (st.activeRunId) {
        useToastStore.getState().push('error', '当前有任务运行中，请先等待完成或中止');
        return;
      }
      const text = newText.trim();
      if (text === '') {
        useToastStore.getState().push('error', '请输入新的消息内容');
        return;
      }
      // 本地乐观截断(参考 truncate_at:该消息及其后全部移除)
      const idx = get().messages.findIndex((m) => m.id === msgId);
      if (idx >= 0) set({ messages: get().messages.slice(0, idx), activeRunId: null });
      // local-* 是未达后端的乐观回显,无需 revert(参考同口径)
      if (!msgId.startsWith('local-')) {
        try {
          await apiPost(`/api/forge/sessions/${encodeURIComponent(sid)}/revert`, {
            messageId: msgId,
            mode: 'before',
          });
        } catch (err) {
          toastError(err, '回退失败');
          await get().resync();
          return;
        }
        // revert 截断使服务端 seq 回退,既有 SSE 长连的 live 去重(seq>已发上限)会把
        // 后续新事件(小号 seq)全部滤掉——参考 revert_to 同样在 revert 后重拉快照;
        // 必须先 resync(新连接 fromSeq=新 latestSeq)再重发,否则新 turn 事件永远到不了。
        await get().resync();
      }
      await get().sendMessage(text, mode ?? msg.mode ?? 'build', undefined, { annotations: msg.annotations });
    },

    pickModel: (modelId) => patchSpec({ selectedModelId: modelId }, '切换模型失败'),

    setThinking: (on) => patchSpec({ thinkingEnabled: on }, '切换思考模式失败'),

    pickEffort: (effortId) => patchSpec({ reasoningEffort: effortId }, '切换推理强度失败'),

    pickContext: (contextId) => patchSpec({ contextOptionId: contextId }, '切换上下文规格失败'),

    ensureModels: async (force = false, requiredProvider) => {
      const provider = requiredProvider?.trim().toLowerCase();
      const hasRequiredModels = provider
        ? get().models.some((model) => (model.provider ?? '').toLowerCase() === provider)
        : get().models.length > 0;
      if (!force && hasRequiredModels) return;
      try {
        // Codex 模型由 app-server 动态枚举；普通 design-snapshot 只带当前缓存。
        // 冷启动时本地模型已存在，不能据此误判「模型目录已加载」。先预热
        // Codex 缓存，再拉统一快照，保持模型能力面仍以 agentd 为单一事实源。
        if (provider === 'codex') {
          await getCodexModels(force).catch(() => null);
        }
        const snap = await apiGet<DesignSnapshot>('/api/forge/design-snapshot');
        if (snap.models?.models && snap.models.models.length > 0) {
          set({
            models: snap.models.models,
            defaultModelId: snap.models.defaultModelId ?? get().defaultModelId,
          });
        }
        useSessionStore.getState().hydrateAgentDefaults(snap.agents?.defaultEngine);
      } catch {
        // 后端离线等错误暂不打断
      }
    },

    reset: () => {
      seenKeys.clear();
      seenOrder.length = 0;
      toolNames.clear();
      streamBuf.clear();
      toolOutputBuf.clear();
      useGoalStore.getState().reset();
      useCollaborationStore.getState().reset();
      // D-035:计划指针随会话走(切走即清,applySnapshot 会按新会话回填)。
      usePlanStore.getState().reset();
      // D-044:UltraPlan 阶段同样随会话走(含在途动作与草稿之外的一切本地态)。
      useUltraPlanStore.getState().reset();
      if (streamRaf != null && typeof cancelAnimationFrame === 'function') {
        cancelAnimationFrame(streamRaf);
        streamRaf = null;
      }
      set((st) => ({
        messages: [],
        activeRunId: null,
        todos: [],
        tokens: { prompt: 0, completion: 0, total: 0 },
        lastPromptTokens: 0,
        usageContextWindow: null,
        compactions: [],
        latestSeq: 0,
        models: st.models,
        defaultModelId: st.defaultModelId,
        selectedModelId: null,
        thinkingEnabled: false,
        reasoningEffort: null,
        contextOptionId: null,
        hydrating: false,
        currentSessionId: null,
        eventsRing: [],
        pendingPermission: null,
        pendingPermissions: [],
        codexAccount: null,
        codexRateLimits: null,
        latestDiff: null,
        streamLink: 'idle',
        streamDownSince: null,
        pendingTurnSince: null,
      }));
    },
  };
});
