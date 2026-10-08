import { create } from 'zustand';
import { apiGet } from './forgeApi';
import { codexQuotaWindows } from './codexQuota';
import { useChatStore, type SnapshotModel } from './chatStore';
import { createPoller, usePoller } from './poller';
import { useSessionStore, type AgentEngine, type ForgeSession } from './sessionStore';

/**
 * 系统状态单一事实源(D-040):5s 轮询 host 健康接口 + 轻量快照(design-snapshot?events=0),
 * 页面隐藏时暂停。状态栏、首页状态胶囊、账户卡、关于页都读这里,不再各自发请求。
 * 顺带承担原 StatusBar 的两件事:模型目录为空时回填 chatStore、下发新会话默认引擎;
 * 并把快照里各会话的运行态/更新时间回填 sessionStore(侧栏后台会话的运行点与未读点靠它)。
 */

export interface HostHealth {
  status?: string;
  version?: string;
  uptimeSec?: number;
  platform?: string;
  node?: string;
  user?: { name?: string };
  agentd?: { ok: boolean; version?: string; uptimeSec?: number };
}

export interface ProjectFace {
  name: string;
  mode: string;
  root?: string;
}

export interface EngineFace {
  id: string;
  installed?: boolean;
  running?: boolean;
  authMode?: string | null;
  planType?: string | null;
  rateLimits?: unknown;
  version?: string | null;
}

interface LiteSnapshot {
  sessions?: Array<Partial<ForgeSession> & { id: string }>;
  models?: { models?: SnapshotModel[]; defaultModelId?: string };
  todos?: Array<{ status: string }>;
  project?: ProjectFace;
  agents?: { defaultEngine?: string; engines?: EngineFace[] };
}

interface SystemState {
  /** 首轮探测已完成(未完成时各面显示「检测中」,不先亮一个写死的结论)。 */
  checked: boolean;
  /** host 健康接口可达。 */
  online: boolean;
  health: HostHealth | null;
  /** 快照可达(= agentd 经 host 代理可达)。 */
  snapshotOk: boolean;
  project: ProjectFace | null;
  /** 当前会话待办进度(无会话为 null)。 */
  todos: { done: number; total: number } | null;
  engines: EngineFace[];
  /** 最近一轮快照的模型目录(含实时 availability;配好密钥后下一轮即反映)。 */
  catalog: SnapshotModel[];
  defaultModelId: string | null;
  refresh: () => Promise<void>;
}

const POLL_MS = 5000;

function isDone(status: string): boolean {
  return status === 'completed' || status === 'done';
}

/** 快照里的会话运行态回填侧栏:只动 activeRunId / updatedAt,以及仍为空的标题(服务端自动命名)。 */
function mergeSessionRuntime(list: LiteSnapshot['sessions']): void {
  if (!list || list.length === 0) return;
  const byId = new Map(list.map((s) => [s.id, s]));
  const st = useSessionStore.getState();
  let changed = false;
  const next = st.sessions.map((s) => {
    const r = byId.get(s.id);
    if (!r) return s;
    const activeRunId = r.activeRunId === undefined ? s.activeRunId : r.activeRunId;
    const updatedAt = typeof r.updatedAt === 'string' && r.updatedAt !== '' ? r.updatedAt : s.updatedAt;
    const title = s.title === '' && typeof r.title === 'string' && r.title !== '' ? r.title : s.title;
    if (activeRunId === s.activeRunId && updatedAt === s.updatedAt && title === s.title) return s;
    changed = true;
    return { ...s, activeRunId, updatedAt, title };
  });
  if (changed) useSessionStore.setState({ sessions: next });
}

export const useSystemStore = create<SystemState>((set) => ({
  checked: false,
  online: false,
  health: null,
  snapshotOk: false,
  project: null,
  todos: null,
  engines: [],
  catalog: [],
  defaultModelId: null,

  refresh: async () => {
    const sessionId = useSessionStore.getState().activeSessionId;
    const q = sessionId ? `&sessionId=${encodeURIComponent(sessionId)}` : '';
    const [health, snap] = await Promise.allSettled([
      apiGet<HostHealth>('/api/forge/health'),
      apiGet<LiteSnapshot>(`/api/forge/design-snapshot?events=0${q}`),
    ]);
    const patch: Partial<SystemState> = {
      checked: true,
      online: health.status === 'fulfilled',
      health: health.status === 'fulfilled' ? health.value : null,
    };
    if (snap.status === 'fulfilled') {
      const s = snap.value;
      patch.snapshotOk = true;
      patch.project = s.project ?? null;
      patch.engines = s.agents?.engines ?? [];
      // 请求途中切了会话:待办属于旧会话,留给下一轮(切会话会立刻触发一次刷新)
      if (useSessionStore.getState().activeSessionId === sessionId) {
        const list = s.todos ?? [];
        patch.todos = sessionId ? { done: list.filter((t) => isDone(t.status)).length, total: list.length } : null;
      }
      const catalog = s.models?.models ?? [];
      patch.catalog = catalog;
      patch.defaultModelId = s.models?.defaultModelId ?? null;
      const chat = useChatStore.getState();
      if (catalog.length > 0 && chat.models.length === 0) {
        useChatStore.setState({
          models: catalog,
          defaultModelId: s.models?.defaultModelId ?? chat.defaultModelId,
        });
      }
      useSessionStore.getState().hydrateAgentDefaults(s.agents?.defaultEngine);
      mergeSessionRuntime(s.sessions);
    } else {
      patch.snapshotOk = false;
      patch.todos = null;
    }
    set(patch);
  },
}));

const systemPoller = createPoller(() => useSystemStore.getState().refresh(), POLL_MS, {
  // 切会话 / 切引擎时立刻刷新(待办与模型面跟着会话走),不等下一个 5s
  onStart: () =>
    useSessionStore.subscribe((st, prev) => {
      const engineOf = (s: typeof st) =>
        s.sessions.find((x) => x.id === s.activeSessionId)?.agentEngine ?? s.draftAgentEngine;
      if (st.activeSessionId !== prev.activeSessionId || engineOf(st) !== engineOf(prev)) {
        void useSystemStore.getState().refresh();
      }
    }),
});

/** 组件挂载期间保持系统状态轮询(引用计数,多个组件共用一个定时器)。 */
export function useSystemPolling(): void {
  usePoller(systemPoller);
}

// ---------- 纯派生(状态栏 / 首页 / 账户卡共用;导出供单测) ----------

/** 当前会话实际使用的引擎(无会话时取 Composer 草稿选择,与 AgentSwitcher 同源)。 */
export function activeEngineOf(st: ReturnType<typeof useSessionStore.getState>): AgentEngine {
  return st.sessions.find((s) => s.id === st.activeSessionId)?.agentEngine ?? st.draftAgentEngine;
}

export interface CodexQuota {
  /** 主窗口剩余百分比(0–100)。 */
  remaining?: number;
  /** 重置时刻(epoch 秒)。 */
  resetsAt?: number;
  windowMins?: number;
}

/** Codex rateLimits 多种形态(usedPercent / primary / rateLimitsByLimitId)→ 主窗口剩余额度。 */
export function codexQuota(raw: unknown, planType?: string | null): CodexQuota {
  const w = codexQuotaWindows(raw, planType)[0];
  if (!w) return {};
  return {
    remaining: Math.max(0, Math.min(100, Math.round(100 - w.usedPercent))),
    resetsAt: typeof w.resetsAt === 'number' ? w.resetsAt : undefined,
    windowMins: typeof w.windowDurationMins === 'number' ? w.windowDurationMins : undefined,
  };
}

export interface EngineDesc {
  engine: AgentEngine;
  /** 状态栏文案:本地 / Codex · Plus / Codex · 未登录 / Codex · 未安装 */
  label: string;
  ready: boolean;
  plan?: string;
  quota: CodexQuota;
}

export function describeEngine(engine: AgentEngine, engines: EngineFace[]): EngineDesc {
  if (engine === 'local') return { engine, label: '本地', ready: true, quota: {} };
  const face = engines.find((e) => e.id === 'codex');
  const ready = face?.installed !== false && typeof face?.authMode === 'string' && face.authMode !== '';
  const plan = typeof face?.planType === 'string' && face.planType !== '' ? face.planType : undefined;
  const label = ready
    ? `Codex${plan ? ` · ${plan}` : ''}`
    : face?.installed === false
      ? 'Codex · 未安装'
      : 'Codex · 未登录';
  return { engine, label, ready, plan, quota: ready ? codexQuota(face?.rateLimits, plan) : {} };
}

export type ModelState = 'checking' | 'live' | 'mock' | 'auto' | 'unconfigured' | 'login';

export interface ModelDesc {
  state: ModelState;
  label: string;
  /** 悬停说明(为什么是这个状态、点了去哪)。 */
  title: string;
}

/**
 * 状态栏模型段:与 Composer 模型选择器同一口径(会话所选,缺省回落默认模型;
 * Codex 未选即「自动」)。选中的模型缺密钥/未配置时如实显示「模型未配置」。
 */
export function describeModel(args: {
  checked: boolean;
  engine: EngineDesc;
  models: SnapshotModel[];
  wantedId: string | null;
  defaultModelId: string | null;
}): ModelDesc {
  const { checked, engine, models, wantedId, defaultModelId } = args;
  if (!checked && models.length === 0) return { state: 'checking', label: '检测中…', title: '正在读取模型目录' };
  if (engine.engine === 'codex') {
    if (!engine.ready) {
      return { state: 'login', label: engine.label, title: '点击打开设置 · Codex,完成安装或登录' };
    }
    const catalog = models.filter((m) => m.provider === 'codex');
    if (wantedId === null) {
      return { state: 'auto', label: 'Codex 默认模型', title: '未指定模型,由 Codex 配置决定' };
    }
    const m = catalog.find((x) => x.id === wantedId);
    return m
      ? { state: 'live', label: m.label || m.id, title: `Codex 模型 ${m.label || m.id}` }
      : { state: 'unconfigured', label: '模型不可用', title: `Codex 模型目录里没有 ${wantedId}` };
  }
  const catalog = models.filter((m) => m.provider !== 'codex');
  const wanted = wantedId ?? defaultModelId;
  const m =
    catalog.find((x) => x.id === wanted) ??
    (wanted === null
      ? (catalog.find((x) => x.provider !== 'mock' && x.availability === 'available') ??
        catalog.find((x) => x.provider === 'mock'))
      : undefined);
  if (!m) {
    return {
      state: 'unconfigured',
      label: '模型未配置',
      title: wanted ? `模型目录里没有 ${wanted},点击打开设置 · 模型` : '尚未选择模型,点击打开设置 · 模型',
    };
  }
  if (m.provider === 'mock') {
    return { state: 'mock', label: m.label || m.id, title: '模拟模型:不调用真实大模型,回复为固定内容' };
  }
  if (m.availability !== 'available') {
    const why = m.availability === 'needs-key' ? '缺少 API Key' : `状态 ${m.availability ?? '未知'}`;
    return { state: 'unconfigured', label: '模型未配置', title: `${m.label || m.id}:${why},点击打开设置 · 模型` };
  }
  return { state: 'live', label: m.label || m.id, title: `当前模型 ${m.label || m.id}` };
}

/** 秒数 → 「3 分钟 / 2 小时 / 5 天」式时长。 */
export function humanDuration(sec: number | undefined): string {
  if (sec === undefined || !Number.isFinite(sec)) return '';
  const s = Math.max(0, Math.floor(sec));
  if (s < 60) return `${s} 秒`;
  if (s < 3600) return `${Math.floor(s / 60)} 分钟`;
  if (s < 86400) return `${Math.floor(s / 3600)} 小时 ${Math.floor((s % 3600) / 60)} 分钟`;
  return `${Math.floor(s / 86400)} 天`;
}
