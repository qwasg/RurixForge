import { create } from 'zustand';
import { apiGet, apiPatch, apiPost, ForgeApiError, getCodexModels } from './forgeApi';
import { notifyAgentToolSettled } from './editorSync';
import { useGoalStore } from './goalStore';
import { usePlanStore } from './planStore';
import { useSessionStore, type AgentEngine, type ForgeSession } from './sessionStore';
import { useToastStore } from './toastStore';
import { subscribeSessionEvents, type SseSubscription } from './sseClient';
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
  id: string;
  role: 'user' | 'assistant';
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
  /**
   * D-038:用户卡的来源。`receipt` = 后台子代理回执送达时系统自动唤醒主 agent 的那一轮——
   * 卡片正文是系统生成的唤醒说明,不是用户说的话,不可编辑重发。缺省(undefined)= 用户发的。
   */
  source?: 'receipt';
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
  /** 来源标记("user" / "plan")。 */
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
  /** 空数组 = 该渠道不收 reasoning_effort(如 deepseek),Effort 行禁用。 */
  effortOptions?: ModelEffortOption[];
  defaultEffort?: string | null;
  contextOptions?: ModelContextOption[];
  defaultContext?: string;
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

export interface PendingPermission {
  id: string;
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
    opts?: { planPath?: string },
  ) => Promise<void>;
  cancelRun: () => Promise<void>;
  /**
   * D-036:中止单个后台子代理(multitask dispatch 派出去的 run)。
   * 与 cancelRun 分开:后台子代理跑的时候父轮早已收束、activeRunId 为空,
   * cancelRun 会直接空转。
   */
  cancelSubagent: (runId: string) => Promise<void>;
  editAndResend: (msgId: string, newText: string) => Promise<void>;
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
  const idx = blocks.findIndex((b) => b.kind === 'subagent' && b.id === id);
  if (idx >= 0) {
    const cur = blocks[idx] as SubagentBlock;
    const next = { ...cur, ...patch, work: patch?.work ?? cur.work };
    blocks[idx] = next;
    return next;
  }
  const created: SubagentBlock = {
    kind: 'subagent',
    id,
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
  ensureSubagent(root, parentId);
  const idx = root.findIndex((b) => b.kind === 'subagent' && b.id === parentId);
  if (idx < 0) return root;
  const cur = root[idx] as SubagentBlock;
  const work = [...cur.work];
  root[idx] = { ...cur, work };
  return work;
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
    if (user.runId) {
      const idx = out.findIndex((m) => m.role === 'user' && m.runId === user.runId);
      if (idx >= 0) {
        out[idx] = user;
        return out;
      }
    }
    // 乐观 local-* 同文替换(参考 rposition:取最后一条)
    for (let i = out.length - 1; i >= 0; i -= 1) {
      const m = out[i];
      if (m.role === 'user' && !m.runId && m.text === user.text) {
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

  /** 参考 normalize_messages:同 runId/同文 local 用户消息去重 + user-before-assistant。 */
  const normalize = (messages: ChatMsg[]): ChatMsg[] => {
    const out = [...messages];
    let idx = 0;
    while (idx < out.length) {
      const m = out[idx];
      if (m.role !== 'user' || !m.runId) {
        idx += 1;
        continue;
      }
      let scan = idx + 1;
      while (scan < out.length) {
        const n = out[scan];
        const dupRunUser = n.role === 'user' && n.runId === m.runId;
        const dupLocalUser = n.role === 'user' && !n.runId && n.text === m.text;
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
      const uidx = out.findIndex((n, j) => j > aidx && n.role === 'user' && n.runId === m.runId);
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
      latestSeq: snap.latestSeq ?? 0,
      models: snap.models?.models ?? [],
      defaultModelId: snap.models?.defaultModelId ?? null,
      selectedModelId: snap.activeSession?.selectedModelId ?? null,
      thinkingEnabled: snap.activeSession?.thinkingEnabled ?? false,
      reasoningEffort: snap.activeSession?.reasoningEffort ?? null,
      contextOptionId: snap.activeSession?.contextOptionId ?? null,
      hydrating: false,
      pendingPermission: null,
      codexAccount: null,
      codexRateLimits: null,
      latestDiff: null,
    });
    useSessionStore.getState().hydrateAgentDefaults(snap.agents?.defaultEngine);
    // D-035:计划指针回填。放在事件回放之前——回放里的 plan.* 会顺带开页签,
    // 这里只负责「有计划但本会话没新事件」时命令面板仍能找到它。
    usePlanStore.getState().setActivePlanPath(snap.activeSession?.activePlanPath ?? null);
    replaying = true;
    try {
      for (const evt of snap.events ?? []) get().applyEvent(evt);
    } finally {
      replaying = false;
    }
    // 快照回放后 run 仍在运行(进程重启前状态) → activeRunId 如实回填;
    // 反之快照 run 为空(服务端 run 注册表才是活运行唯一事实源)→ 强制清空:
    // 事件日志可能存在 run.created 无终止事件的残留(进程被杀),只靠回放会永久卡幽灵运行态(2026-08-29 实测)。
    if (snap.run && snap.run.status === 'running') set({ activeRunId: snap.run.id });
    else set({ activeRunId: null });
  };

  const subscribe = (sessionId: string, fromSeq: number) => {
    closeSse();
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
      onError: () => {
        // 重连由 sseClient 退避承担;离线态由 StatusBar health 轮询如实呈现
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
    codexAccount: null,
    codexRateLimits: null,
    latestDiff: null,
    eventsRing: [],

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
      // 全类型入原始事件环(底部面板 Agent Logs/Output 数据源;在去重后、语义 switch 前)。
      set((st) => ({ eventsRing: ringPush(st.eventsRing, evt) }));
      const isDelta =
        evt.type === 'agent.token.stream.delta' ||
        evt.type === 'agent.reasoning.delta' ||
        evt.type === 'agent.tool.output.delta' ||
        evt.type === 'agent.plan.delta';
      if (!isDelta) flushStreamNow();
      const time = hhmm(evt.ts);
      let messages = get().messages;
      switch (evt.type) {
        case 'composer.user.message': {
          const user: ChatMsg = {
            id: evt.id || `user-${evt.seq}`,
            role: 'user',
            text: payloadStr(evt, 'text') ?? '',
            blocks: [],
            status: 'completed',
            mode: payloadStr(evt, 'composerMode'),
            time,
            ts: evt.ts,
            runId: runIdOf(evt),
          };
          if (payloadStr(evt, 'source') === 'receipt') user.source = 'receipt';
          messages = upsertUser(messages, user);
          break;
        }
        case 'agent.started': {
          const runId = runIdOf(evt);
          const model = payloadStr(evt, 'model');
          const engine = payloadStr(evt, 'engine');
          messages = mutateAssistant(messages, runId, (m) => {
            if (m.time === '') m.time = time;
            if (!m.startedTs) m.startedTs = evt.ts;
            if (model) m.model = model;
            if (engine) m.engine = engine;
            m.status = 'streaming';
          });
          set({ activeRunId: runId });
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
              ensureSubagent(m.blocks, id, {
                status: failed ? 'error' : 'done',
                summary,
              });
            }
          });
          break;
        }
        case 'permission.requested': {
          const id = payloadStr(evt, 'id');
          const tool = payloadStr(evt, 'tool') ?? 'tool';
          const approvalKind = payloadStr(evt, 'kind') ?? 'command';
          const pending: PendingPermission | null = id
            ? {
                id,
                runId: runIdOf(evt),
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
            set({ pendingPermission: pending });
            messages = mutateAssistant(messages, runIdOf(evt), (message) => {
              if (message.blocks.some((block) => block.kind === 'approval' && block.id === pending.id)) return;
              message.blocks.push({
                kind: 'approval',
                id: pending.id,
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
          const pending = get().pendingPermission;
          if (pending && pending.id === id) set({ pendingPermission: null });
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
            settleFinalText(m.blocks, text);
          });
          break;
        }
        case 'agent.completed': {
          const runId = runIdOf(evt);
          const text = payloadStr(evt, 'text') ?? '';
          messages = mutateAssistant(messages, runId, (m) => {
            if (text !== '') settleFinalText(m.blocks, text);
            expireApprovals(m.blocks);
            m.status = 'completed';
            m.finishedTs = evt.ts;
          });
          if (get().pendingPermission?.runId === runId) set({ pendingPermission: null });
          if (get().activeRunId === runId) set({ activeRunId: null });
          usePlanStore.getState().setPlanning(false);
          break;
        }
        case 'agent.failed': {
          const runId = runIdOf(evt);
          const error = payloadStr(evt, 'error');
          messages = mutateAssistant(messages, runId, (m) => {
            expireApprovals(m.blocks);
            m.status = 'failed';
            m.finishedTs = evt.ts;
            if (error) m.error = error;
          });
          if (get().pendingPermission?.runId === runId) set({ pendingPermission: null });
          if (get().activeRunId === runId) set({ activeRunId: null });
          usePlanStore.getState().setPlanning(false);
          break;
        }
        case 'agent.cancelled': {
          const runId = runIdOf(evt);
          messages = mutateAssistant(messages, runId, (m) => {
            expireApprovals(m.blocks);
            m.status = 'cancelled';
            m.finishedTs = evt.ts;
          });
          if (get().pendingPermission?.runId === runId) set({ pendingPermission: null });
          if (get().activeRunId === runId) set({ activeRunId: null });
          usePlanStore.getState().setPlanning(false);
          break;
        }
        case 'agent.usage': {
          const p = evt.payload ?? {};
          const num = (k: string) => (typeof p[k] === 'number' ? (p[k] as number) : 0);
          const t = get().tokens;
          const prompt = num('promptTokens');
          set({
            tokens: {
              prompt: t.prompt + prompt,
              completion: t.completion + num('completionTokens'),
              total: t.total + num('totalTokens'),
            },
            lastPromptTokens: prompt > 0 ? prompt : get().lastPromptTokens,
          });
          return;
        }
        case 'agent.diff.updated': {
          const diff = evt.payload?.diff ?? null;
          set({ latestDiff: diff });
          if (typeof diff === 'string' && diff !== '') {
            messages = mutateAssistant(messages, runIdOf(evt), (message) => {
              mergeDiffIntoLastFileChange(message.blocks, diff);
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
        case 'session.updated': {
          // 转发 sessionStore 刷新(标题/置顶/模型等元信息面)
          void useSessionStore.getState().loadAll();
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
      if (trimmed === '') return;
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
        blocks: [],
        status: 'completed',
        mode,
        time: '',
        runId: null,
      };
      set((st) => ({ messages: [...st.messages, local] }));
      // plan 模式:开跑即进「调研中」态,Plan 页签据此显示进行条(终态由 run 收束事件复位)。
      if (mode === 'plan') usePlanStore.getState().setPlanning(true);
      try {
        await apiPost(`/api/forge/sessions/${encodeURIComponent(sid)}/ask:execute`, {
          userInput: trimmed,
          mode,
          ...(skills !== undefined && skills.length > 0 ? { skills } : {}),
          ...(opts?.planPath !== undefined ? { planPath: opts.planPath } : {}),
        });
        // 响应体不等:UI 由 SSE 事件驱动(参考 send 语义)
      } catch (err) {
        usePlanStore.getState().setPlanning(false);
        // D-038:会话被服务端自起的回执唤醒轮占用(agent.started 尚未到达的那几毫秒里点了发送)。
        // 这条消息后端没收,乐观回显必须撤掉——留着就是一张永远没有回应的幽灵用户卡。
        if (err instanceof ForgeApiError && err.code === 'SESSION_BUSY') {
          set((st) => ({ messages: st.messages.filter((m) => m.id !== local.id) }));
          useToastStore.getState().push('warning', '主 agent 正在处理后台回执,请等它收束后再发');
          return;
        }
        toastError(err, '提交失败');
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

    editAndResend: async (msgId, newText) => {
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
      await get().sendMessage(text, msg.mode ?? 'build');
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
      // D-035:计划指针随会话走(切走即清,applySnapshot 会按新会话回填)。
      usePlanStore.getState().reset();
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
        codexAccount: null,
        codexRateLimits: null,
        latestDiff: null,
      }));
    },
  };
});
