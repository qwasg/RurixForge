import { create } from 'zustand';
import { apiGet, apiPatch, apiPost } from './forgeApi';
import { useSessionStore } from './sessionStore';
import { useToastStore } from './toastStore';
import { subscribeSessionEvents, type SseSubscription } from './sseClient';
import { bareName, mcpOf, type ChatBlock } from './timeline';

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
 * 差异留痕:①参考还有 token.delta/reasoning.delta/subagent.* 事件,本仓事件面无(组件就绪,
 * 自然不触发,不伪造);②editAndResend 在 revert 后显式 resync(重拉快照+新连接续订)——
 * 服务端 SSE live 去重按「seq>已发上限」,revert 截断致 seq 回退后旧连接会滤掉新事件
 * (参考 revert_to 同样在 revert 后 api.snapshot 重拉);③思考块/子代理 work 为简形 string[]。
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
}

export interface SnapshotModel {
  id: string;
  label: string;
  provider?: string;
  availability?: string;
}

interface DesignSnapshot {
  sessions?: unknown[];
  activeSession?: { id: string; selectedModelId?: string | null } | null;
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
  latestSeq: number;
  models: SnapshotModel[];
  defaultModelId: string | null;
  selectedModelId: string | null;
  /** 快照加载中(切换会话骨架态)。 */
  hydrating: boolean;
  /** SubagentOverlay 目标子代理块 id(参考 subagent_overlay;本仓事件面不触发,组件就绪)。 */
  subagentOverlayId: string | null;
  /**
   * F7 wave.5:原始事件环(底部面板 Agent Logs/Output 数据源;cap 200 FIFO,
   * Agent Logs 取末 120 条,Output 取末 200 条;applyEvent 全类型入环,reset 清空)。
   */
  eventsRing: ForgeEventWire[];

  applyEvent: (evt: ForgeEventWire) => void;
  selectSession: (id: string | null) => Promise<void>;
  resync: () => Promise<void>;
  sendMessage: (text: string, mode: string) => Promise<void>;
  cancelRun: () => Promise<void>;
  editAndResend: (msgId: string, newText: string) => Promise<void>;
  pickModel: (modelId: string) => Promise<void>;
  openSubagent: (id: string | null) => void;
  /** 清空原始事件环(底部面板 trash 钮;不影响消息/待办)。 */
  clearEventsRing: () => void;
  reset: () => void;
}

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

export const useChatStore = create<ChatState>((set, get) => {
  // 非响应式内部态(去重集/SSE 句柄,不进 zustand state 防多余渲染)
  const seenKeys = new Set<string>();
  const seenOrder: string[] = [];
  let sse: SseSubscription | null = null;

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

  const applySnapshot = (snap: DesignSnapshot) => {
    seenKeys.clear();
    seenOrder.length = 0;
    set({
      messages: [],
      activeRunId: null,
      todos: snap.todos ?? [],
      tokens: { prompt: 0, completion: 0, total: 0 },
      latestSeq: snap.latestSeq ?? 0,
      models: snap.models?.models ?? [],
      defaultModelId: snap.models?.defaultModelId ?? null,
      selectedModelId: snap.activeSession?.selectedModelId ?? null,
      hydrating: false,
    });
    for (const evt of snap.events ?? []) get().applyEvent(evt);
    // 快照回放后 run 仍在运行(进程重启前状态) → activeRunId 如实回填
    if (snap.run && snap.run.status === 'running') set({ activeRunId: snap.run.id });
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

  return {
    messages: [],
    activeRunId: null,
    todos: [],
    tokens: { prompt: 0, completion: 0, total: 0 },
    latestSeq: 0,
    models: [],
    defaultModelId: null,
    selectedModelId: null,
    hydrating: false,
    subagentOverlayId: null,
    eventsRing: [],

    openSubagent: (id) => set({ subagentOverlayId: id }),

    clearEventsRing: () => set({ eventsRing: [] }),

    applyEvent: (evt) => {
      if (!markSeen(evt)) return;
      // 全类型入原始事件环(底部面板 Agent Logs/Output 数据源;在去重后、语义 switch 前)。
      set((st) => ({ eventsRing: ringPush(st.eventsRing, evt) }));
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
          const rawArgs = evt.payload?.args;
          const args =
            rawArgs === undefined ? '' : JSON.stringify(rawArgs, null, 2);
          messages = mutateAssistant(messages, runId, (m) => {
            m.status = 'streaming';
            if (bareName(name) === 'task') {
              // name==="task" → subagent 块(本仓事件面不产生,组件就绪)
              const a = (evt.payload?.args ?? {}) as Record<string, unknown>;
              const prompt = typeof a.prompt === 'string' ? a.prompt : '';
              const label =
                (typeof a.description === 'string' && a.description) ||
                (typeof a.title === 'string' && a.title) ||
                prompt.split('\n').find((l) => l.trim() !== '')?.trim() ||
                '子代理任务';
              m.blocks.push({ kind: 'subagent', id: toolCallId, label, status: 'running', work: [] });
            } else {
              m.blocks.push({
                kind: 'tool',
                toolCallId,
                name,
                args,
                mcp: mcpOf(name),
              });
            }
          });
          break;
        }
        case 'agent.tool.completed':
        case 'agent.tool.failed': {
          const failed = evt.type === 'agent.tool.failed';
          const runId = runIdOf(evt);
          const callId = payloadStr(evt, 'toolCallId');
          const durationMs =
            typeof evt.payload?.durationMs === 'number' ? (evt.payload.durationMs as number) : undefined;
          const error = failed
            ? payloadStr(evt, 'error') ?? '工具执行失败'
            : undefined;
          messages = mutateAssistant(messages, runId, (m) => {
            // 倒序找 toolCallId 匹配块;无 id 时退最后一个 running 工具块(参考同口径)
            for (let i = m.blocks.length - 1; i >= 0; i -= 1) {
              const b = m.blocks[i];
              if (b.kind === 'subagent' && (callId ? b.id === callId : b.status === 'running')) {
                m.blocks[i] = { ...b, status: failed ? 'error' : 'done', summary: error };
                return;
              }
              if (
                b.kind === 'tool' &&
                (callId ? b.toolCallId === callId : b.ok === undefined && b.error === undefined)
              ) {
                m.blocks[i] = { ...b, ok: !failed, error, durationMs };
                return;
              }
            }
          });
          break;
        }
        case 'agent.message': {
          const runId = runIdOf(evt);
          const text = payloadStr(evt, 'text') ?? '';
          const provider = payloadStr(evt, 'provider');
          messages = mutateAssistant(messages, runId, (m) => {
            if (provider) m.provider = provider;
            // 权威最终 text 块:替换尾随 text 块,否则追加(参考 agent.completed 语义)
            const last = m.blocks[m.blocks.length - 1];
            if (last?.kind === 'text') {
              m.blocks[m.blocks.length - 1] = { kind: 'text', text, final: true };
            } else {
              m.blocks.push({ kind: 'text', text, final: true });
            }
          });
          break;
        }
        case 'agent.completed': {
          const runId = runIdOf(evt);
          const text = payloadStr(evt, 'text') ?? '';
          messages = mutateAssistant(messages, runId, (m) => {
            if (text !== '') {
              const last = m.blocks[m.blocks.length - 1];
              if (last?.kind === 'text') {
                m.blocks[m.blocks.length - 1] = { kind: 'text', text, final: true };
              } else {
                m.blocks.push({ kind: 'text', text, final: true });
              }
            }
            m.status = 'completed';
            m.finishedTs = evt.ts;
          });
          if (get().activeRunId === runId) set({ activeRunId: null });
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
          break;
        }
        case 'agent.cancelled': {
          const runId = runIdOf(evt);
          messages = mutateAssistant(messages, runId, (m) => {
            m.status = 'cancelled';
            m.finishedTs = evt.ts;
          });
          if (get().activeRunId === runId) set({ activeRunId: null });
          break;
        }
        case 'agent.usage': {
          const p = evt.payload ?? {};
          const num = (k: string) => (typeof p[k] === 'number' ? (p[k] as number) : 0);
          const t = get().tokens;
          set({
            tokens: {
              prompt: t.prompt + num('promptTokens'),
              completion: t.completion + num('completionTokens'),
              total: t.total + num('totalTokens'),
            },
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
          if (idx >= 0) todos[idx] = { ...todos[idx], ...patch };
          else {
            todos.push({
              id,
              title: patch.title ?? id,
              status: patch.status ?? 'queued',
              kind: patch.kind,
              summary: patch.summary,
            });
          }
          set({ todos });
          return;
        }
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
      if (!id) return;
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

    sendMessage: async (text, mode) => {
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
      try {
        await apiPost(`/api/forge/sessions/${encodeURIComponent(sid)}/ask:execute`, {
          userInput: trimmed,
          mode,
        });
        // 响应体不等:UI 由 SSE 事件驱动(参考 send 语义)
      } catch (err) {
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

    pickModel: async (modelId) => {
      const sid = useSessionStore.getState().activeSessionId;
      const prev = get().selectedModelId;
      set({ selectedModelId: modelId });
      if (!sid) return;
      try {
        const r = await apiPatch<{ session: { id: string; selectedModelId?: string | null } }>(
          `/api/forge/sessions/${encodeURIComponent(sid)}`,
          { selectedModelId: modelId },
        );
        // 同步 sessionStore 会话面(selectedModelId 落库)
        useSessionStore.setState((st) => ({
          sessions: st.sessions.map((s) =>
            s.id === sid ? { ...s, selectedModelId: r.session.selectedModelId ?? null } : s,
          ),
        }));
      } catch (err) {
        set({ selectedModelId: prev });
        toastError(err, '切换模型失败');
      }
    },

    reset: () => {
      seenKeys.clear();
      seenOrder.length = 0;
      set({
        messages: [],
        activeRunId: null,
        todos: [],
        tokens: { prompt: 0, completion: 0, total: 0 },
        latestSeq: 0,
        models: [],
        defaultModelId: null,
        selectedModelId: null,
        hydrating: false,
        eventsRing: [],
      });
    },
  };
});
