import { create } from 'zustand';
import { apiGet, apiPatch, apiPost, ForgeApiError } from './forgeApi';
import { notifyAgentToolSettled } from './editorSync';
import { usePlanStore } from './planStore';
import { useSessionStore, type ForgeSession } from './sessionStore';
import { useToastStore } from './toastStore';
import { subscribeSessionEvents, type SseSubscription } from './sseClient';
import { bareName, mcpOf, toolStatus, type BlockStatus, type ChatBlock } from './timeline';

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
 * 瞬时事件不落盘。permission.requested 走 toast,不进气泡。
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
    /** D-035:当前计划文件(刷新/换会话后 Plan 页签据此定位)。 */
    activePlanPath?: string | null;
  } | null;
  events?: ForgeEventWire[];
  todos?: TodoItem[];
  run?: { id: string; status: string } | null;
  models?: { models?: SnapshotModel[]; defaultModelId?: string };
  latestSeq?: number;
  chatFolders?: unknown[];
}

interface TokenTotals {
  prompt: number;
  completion: number;
  total: number;
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
   * 当前已回放/订阅的会话 id。调用方据此跳过重复 selectSession——
   * 重复调用会 reset() 抹掉刚 push 的乐观回显(全屏主页「无会话直接发」会撞上)。
   */
  currentSessionId: string | null;
  /** SubagentOverlay 目标子代理块 id。 */
  subagentOverlayId: string | null;
  /** auto 权限挂起的审批(toast + 设置页)。 */
  pendingPermission: { id: string; tool: string } | null;
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
  pickModel: (modelId: string) => Promise<void>;
  setThinking: (on: boolean) => Promise<void>;
  pickEffort: (effortId: string) => Promise<void>;
  pickContext: (contextId: string) => Promise<void>;
  openSubagent: (id: string | null) => void;
  resolvePermission: (allow: boolean) => Promise<void>;
  /** 清空原始事件环(底部面板 trash 钮;不影响消息/待办)。 */
  clearEventsRing: () => void;
  ensureModels: () => Promise<void>;
  reset: () => void;
}

/**
 * 模型规格四项(模型 / Thinking / Effort / Context)。这四个 state 键与会话 PATCH 的 wire 键
 * 逐字同名,故 patchSpec 可以拿同一个对象既做乐观更新又做请求体。
 */
type SpecPatch = Partial<
  Pick<ChatState, 'selectedModelId' | 'thinkingEnabled' | 'reasoningEffort' | 'contextOptionId'>
>;

const MAX_SEEN = 4096;
/** eventsRing 上限(Agent Logs 取末 120,Output 取末 200 → 环容 200)。 */
export const EVENTS_RING_CAP = 200;
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

function appendTextDelta(blocks: ChatBlock[], delta: string): void {
  const last = blocks[blocks.length - 1];
  if (last?.kind === 'text' && !last.final) {
    blocks[blocks.length - 1] = { ...last, text: last.text + delta };
  } else {
    blocks.push({ kind: 'text', text: delta, final: false });
  }
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
  const streamBuf = new Map<string, { token: string; reasoning: string; ts: string }>();
  let streamRaf: number | null = null;
  let sse: SseSubscription | null = null;

  const streamKey = (runId: string | null, parent?: string) => `${runId ?? ''}\0${parent ?? ''}`;

  const applyStreamChunk = (
    runId: string | null,
    parent: string | undefined,
    kind: 'token' | 'reasoning',
    text: string,
    ts: string,
  ) => {
    let messages = get().messages;
    messages = mutateAssistant(messages, runId, (m) => {
      const dest = targetBlocks(m.blocks, parent);
      if (kind === 'token') appendTextDelta(dest, text);
      else appendReasoningDelta(dest, text, ts);
    });
    set({ messages: normalize(messages) });
  };

  const flushStreamNow = () => {
    if (streamRaf != null && typeof cancelAnimationFrame === 'function') {
      cancelAnimationFrame(streamRaf);
      streamRaf = null;
    }
    if (streamBuf.size === 0) return;
    const pending = [...streamBuf.entries()];
    streamBuf.clear();
    for (const [key, buf] of pending) {
      const [runId, parent] = key.split('\0');
      const rid = runId === '' ? null : runId;
      const pid = parent === '' ? undefined : parent;
      if (buf.reasoning) applyStreamChunk(rid, pid, 'reasoning', buf.reasoning, buf.ts);
      if (buf.token) applyStreamChunk(rid, pid, 'token', buf.token, buf.ts);
    }
  };

  const enqueueStream = (
    runId: string | null,
    parent: string | undefined,
    kind: 'token' | 'reasoning',
    text: string,
    ts: string,
  ) => {
    const key = streamKey(runId, parent);
    const cur = streamBuf.get(key) ?? { token: '', reasoning: '', ts };
    if (kind === 'token') cur.token += text;
    else cur.reasoning += text;
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
    });
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
      const r = await apiPatch<{ session: ForgeSession }>(
        `/api/forge/sessions/${encodeURIComponent(sid)}`,
        patch,
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
    eventsRing: [],

    openSubagent: (id) => set({ subagentOverlayId: id }),

    resolvePermission: async (allow) => {
      const pending = get().pendingPermission;
      if (!pending) return;
      try {
        await apiPost(
          `/api/forge/permissions/${encodeURIComponent(pending.id)}/${allow ? 'approve' : 'deny'}`,
          {},
        );
      } catch (err) {
        toastError(err, allow ? '批准失败' : '拒绝失败');
      }
    },

    clearEventsRing: () => set({ eventsRing: [] }),

    applyEvent: (evt) => {
      if (!markSeen(evt)) return;
      // 全类型入原始事件环(底部面板 Agent Logs/Output 数据源;在去重后、语义 switch 前)。
      set((st) => ({ eventsRing: ringPush(st.eventsRing, evt) }));
      const isDelta =
        evt.type === 'agent.token.stream.delta' || evt.type === 'agent.reasoning.delta';
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
          messages = mutateAssistant(messages, runId, (m) => {
            if (m.time === '') m.time = time;
            if (!m.startedTs) m.startedTs = evt.ts;
            if (model) m.model = model;
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
            ? undefined
            : payloadStr(evt, 'output') ?? payloadStr(evt, 'outputPreview');
          messages = mutateAssistant(messages, runId, (m) => {
            settleToolOrSub(m.blocks, callId, {
              ok: !failed,
              error,
              durationMs,
              result,
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
          const id = payloadStr(evt, 'subRunId') ?? payloadStr(evt, 'parentToolCallId') ?? `sub-${evt.seq}`;
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
          const id = payloadStr(evt, 'subRunId') ?? payloadStr(evt, 'parentToolCallId');
          const failed = evt.type === 'subagent.failed';
          const summary = payloadStr(evt, 'summary') ?? payloadStr(evt, 'error');
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
          if (id) set({ pendingPermission: { id, tool } });
          useToastStore.getState().push('warning', `工具 ${tool} 需要批准`);
          return;
        }
        case 'permission.resolved': {
          const id = payloadStr(evt, 'id');
          const allowed = evt.payload?.allowed === true;
          const pending = get().pendingPermission;
          if (pending && pending.id === id) set({ pendingPermission: null });
          useToastStore.getState().push(allowed ? 'success' : 'info', allowed ? '已批准工具执行' : '已拒绝工具执行');
          return;
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
            m.status = 'completed';
            m.finishedTs = evt.ts;
          });
          if (get().activeRunId === runId) set({ activeRunId: null });
          usePlanStore.getState().setPlanning(false);
          break;
        }
        case 'agent.failed': {
          const runId = runIdOf(evt);
          const error = payloadStr(evt, 'error');
          messages = mutateAssistant(messages, runId, (m) => {
            m.status = 'failed';
            m.finishedTs = evt.ts;
            if (error) m.error = error;
          });
          if (get().activeRunId === runId) set({ activeRunId: null });
          usePlanStore.getState().setPlanning(false);
          break;
        }
        case 'agent.cancelled': {
          const runId = runIdOf(evt);
          messages = mutateAssistant(messages, runId, (m) => {
            m.status = 'cancelled';
            m.finishedTs = evt.ts;
          });
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

    selectSession: async (id) => {
      const token = (selectToken += 1);
      closeSse();
      get().reset();
      set({ currentSessionId: id });
      if (!id) {
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
      let snap: DesignSnapshot;
      try {
        snap = await apiGet<DesignSnapshot>(
          `/api/forge/design-snapshot?sessionId=${encodeURIComponent(id)}`,
        );
      } catch (err) {
        if (token !== selectToken) return;
        set({ hydrating: false });
        toastError(err, '会话快照加载失败');
        return;
      }
      if (token !== selectToken) return; // 已切走,丢弃陈旧快照
      applySnapshot(snap);
      subscribe(id, snap.latestSeq ?? 0);
    },

    resync: async () => {
      const id = useSessionStore.getState().activeSessionId;
      if (!id) return;
      try {
        const snap = await apiGet<DesignSnapshot>(
          `/api/forge/design-snapshot?sessionId=${encodeURIComponent(id)}`,
        );
        applySnapshot(snap);
        subscribe(id, snap.latestSeq ?? 0);
      } catch (err) {
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
        await apiPost(`/api/forge/runs/${encodeURIComponent(runId)}/cancel`, {});
      } catch (err) {
        toastError(err, '中止失败');
      }
    },

    cancelSubagent: async (runId) => {
      if (!runId) return;
      try {
        await apiPost(`/api/forge/runs/${encodeURIComponent(runId)}/cancel`, {});
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

    ensureModels: async () => {
      if (get().models.length > 0) return;
      try {
        const snap = await apiGet<DesignSnapshot>('/api/forge/design-snapshot');
        if (snap.models?.models && snap.models.models.length > 0) {
          set({
            models: snap.models.models,
            defaultModelId: snap.models.defaultModelId ?? get().defaultModelId,
          });
        }
      } catch {
        // 后端离线等错误暂不打断
      }
    },

    reset: () => {
      seenKeys.clear();
      seenOrder.length = 0;
      toolNames.clear();
      streamBuf.clear();
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
      }));
    },
  };
});
