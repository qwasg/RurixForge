import { create } from 'zustand';
import { apiDelete, apiGet, apiPatch, apiPost } from './forgeApi';
import { useToastStore } from './toastStore';
import type { UltraPlanState } from './ultraPlanStore';
import { useWorkspaceStore } from './workspaceStore';

/**
 * F7 wave.3 会话事实源(agentd sessions/chat-folders REST,wave.1/2 产物)。
 * 全走 forgeApi;乐观更新 + 失败回滚 + toast 报错。
 * wire 对齐 agentd sessions.rs(camelCase)。
 */

export type AgentEngine = 'local' | 'codex';

export interface ForgeSession {
  id: string;
  title: string;
  status: string;
  agentKind: string;
  /** 会话使用的执行引擎；fork 会继承该值，但不会继承 codexThreadId。 */
  agentEngine: AgentEngine;
  codexThreadId?: string | null;
  selectedModelId: string | null;
  /** 模型规格三档(agentd modelspec.rs;chatStore 的同名 state 是它的会话内镜像)。 */
  thinkingEnabled: boolean;
  reasoningEffort: string | null;
  contextOptionId: string | null;
  webSearchEnabled: boolean;
  activeRunId: string | null;
  createdAt: string;
  updatedAt: string;
  pinned: boolean;
  titleManuallySet: boolean;
  folderId?: string | null;
  workspaceId?: string | null;
  /** D-035:当前计划文件(工作区相对路径);plan 模式 create_plan 落盘时由后端写入。 */
  activePlanPath?: string | null;
  /** D-044:UltraPlan 流程阶段机(无流程时缺省 / null;fork 不继承)。 */
  ultraplan?: UltraPlanState | null;
}

/** POST /sessions 可选规格(与 chatStore 镜像字段同名;缺省 = 后端模型默认档)。 */
export interface SessionCreateSpec {
  agentEngine?: AgentEngine;
  selectedModelId?: string | null;
  thinkingEnabled?: boolean;
  reasoningEffort?: string | null;
  contextOptionId?: string | null;
}

export interface ChatFolder {
  id: string;
  name: string;
  createdAt: string;
  updatedAt: string;
  workspaceId?: string | null;
}

interface SessionState {
  sessions: ForgeSession[];
  folders: ChatFolder[];
  activeSessionId: string | null;
  loading: boolean;
  /** 后端不可达(首次 loadAll 失败)——侧栏如实显示「后端未连接」 */
  offline: boolean;
  /** 后端 design-snapshot 下发的新会话默认引擎。 */
  defaultAgentEngine: AgentEngine;
  /** 主页尚无会话时 AgentSwitcher 的选择。 */
  draftAgentEngine: AgentEngine;
  draftAgentEngineTouched: boolean;

  loadAll: () => Promise<void>;
  /** spec:全屏主页无会话时 Composer 已勾的模型规格,建会话时一并写入,避免回落到默认档。 */
  create: (title?: string, spec?: SessionCreateSpec) => Promise<ForgeSession | null>;
  select: (id: string) => void;
  rename: (id: string, title: string) => Promise<void>;
  togglePin: (id: string) => Promise<void>;
  remove: (id: string) => Promise<void>;
  moveToFolder: (id: string, folderId: string | null) => Promise<void>;
  setAgentKind: (id: string, agentKind: string) => Promise<void>;
  setAgentEngine: (id: string, agentEngine: AgentEngine) => Promise<void>;
  setDraftAgentEngine: (agentEngine: AgentEngine) => void;
  hydrateAgentDefaults: (agentEngine: unknown) => void;
  fork: (id: string) => Promise<ForgeSession | null>;
  createFolder: (name: string) => Promise<ChatFolder | null>;
  removeFolder: (id: string) => Promise<void>;
  /** 侧栏文件夹组头双击重命名(PATCH /chat-folders/{id};乐观 + 失败回滚)。 */
  renameFolder: (id: string, name: string) => Promise<void>;
}

function toastError(err: unknown, prefix: string): void {
  const msg = err instanceof Error ? err.message : String(err);
  useToastStore.getState().push('error', `${prefix}:${msg}`);
}

export const useSessionStore = create<SessionState>((set, get) => ({
  sessions: [],
  folders: [],
  activeSessionId: null,
  loading: false,
  offline: false,
  defaultAgentEngine: 'local',
  draftAgentEngine: 'local',
  draftAgentEngineTouched: false,

  loadAll: async () => {
    set({ loading: true });
    try {
      const [s, f] = await Promise.all([
        apiGet<{ sessions: ForgeSession[] }>('/api/forge/sessions'),
        apiGet<{ folders: ChatFolder[] }>('/api/forge/chat-folders'),
      ]);
      set({
        sessions: s.sessions.map((session) => ({
          ...session,
          agentEngine: session.agentEngine === 'codex' ? 'codex' : 'local',
        })),
        folders: f.folders,
        loading: false,
        offline: false,
      });
      // agents.defaultEngine 属于 snapshot 面；它不可用不应把已成功加载的会话判成离线。
      try {
        const snap = await apiGet<{ agents?: { defaultEngine?: unknown } }>('/api/forge/design-snapshot');
        get().hydrateAgentDefaults(snap.agents?.defaultEngine);
      } catch {
        // 旧后端没有 agents 字段时继续使用 local。
      }
    } catch (err) {
      set({ loading: false, offline: true });
      toastError(err, '会话加载失败');
    }
  },

  create: async (title, spec) => {
    try {
      const workspaceId = useWorkspaceStore.getState().activeWorkspaceId;
      const agentEngine = spec?.agentEngine ?? get().draftAgentEngine;
      const r = await apiPost<{ session: ForgeSession }>('/api/forge/sessions', {
        title: title ?? '',
        agentEngine,
        ...(workspaceId ? { workspaceId } : {}),
        ...(spec?.selectedModelId ? { selectedModelId: spec.selectedModelId } : {}),
        ...(spec?.thinkingEnabled !== undefined ? { thinkingEnabled: spec.thinkingEnabled } : {}),
        ...(spec?.reasoningEffort ? { reasoningEffort: spec.reasoningEffort } : {}),
        ...(spec?.contextOptionId ? { contextOptionId: spec.contextOptionId } : {}),
      });
      const session = {
        ...r.session,
        agentEngine: r.session.agentEngine === 'codex' ? 'codex' : agentEngine,
      } satisfies ForgeSession;
      set((st) => ({
        sessions: [session, ...st.sessions],
        activeSessionId: session.id,
        offline: false,
      }));
      return session;
    } catch (err) {
      toastError(err, '新建会话失败');
      return null;
    }
  },

  select: (id) => set({ activeSessionId: id }),

  rename: async (id, title) => {
    const prev = get().sessions;
    const trimmed = title.trim();
    if (trimmed === '') return;
    set((st) => ({
      sessions: st.sessions.map((s) => (s.id === id ? { ...s, title: trimmed } : s)),
    }));
    try {
      const r = await apiPatch<{ session: ForgeSession }>(`/api/forge/sessions/${id}`, {
        title: trimmed,
      });
      set((st) => ({
        sessions: st.sessions.map((s) => (s.id === id ? r.session : s)),
      }));
    } catch (err) {
      set({ sessions: prev });
      toastError(err, '重命名失败');
    }
  },

  togglePin: async (id) => {
    const prev = get().sessions;
    const target = prev.find((s) => s.id === id);
    if (!target) return;
    set((st) => ({
      sessions: st.sessions.map((s) => (s.id === id ? { ...s, pinned: !s.pinned } : s)),
    }));
    try {
      const r = await apiPatch<{ session: ForgeSession }>(`/api/forge/sessions/${id}`, {
        pinned: !target.pinned,
      });
      set((st) => ({
        sessions: st.sessions.map((s) => (s.id === id ? r.session : s)),
      }));
    } catch (err) {
      set({ sessions: prev });
      toastError(err, '置顶失败');
    }
  },

  remove: async (id) => {
    const prev = get().sessions;
    const prevActive = get().activeSessionId;
    set((st) => ({
      sessions: st.sessions.filter((s) => s.id !== id),
      activeSessionId: st.activeSessionId === id ? null : st.activeSessionId,
    }));
    try {
      await apiDelete(`/api/forge/sessions/${id}`);
    } catch (err) {
      set({ sessions: prev, activeSessionId: prevActive });
      toastError(err, '删除会话失败');
    }
  },

  setAgentKind: async (id, agentKind) => {
    const prev = get().sessions;
    set((st) => ({
      sessions: st.sessions.map((s) => (s.id === id ? { ...s, agentKind } : s)),
    }));
    try {
      const r = await apiPatch<{ session: ForgeSession }>(`/api/forge/sessions/${id}`, {
        agentKind,
      });
      set((st) => ({
        sessions: st.sessions.map((s) => (s.id === id ? r.session : s)),
      }));
    } catch (err) {
      set({ sessions: prev });
      toastError(err, '切换代理类型失败');
    }
  },

  setAgentEngine: async (id, agentEngine) => {
    const prev = get().sessions;
    const target = prev.find((s) => s.id === id);
    if (!target || target.agentEngine === agentEngine) return;
    set((st) => ({
      sessions: st.sessions.map((s) => (s.id === id ? { ...s, agentEngine } : s)),
    }));
    try {
      const r = await apiPatch<{ session: ForgeSession }>(`/api/forge/sessions/${id}`, {
        agentEngine,
      });
      const session = {
        ...r.session,
        agentEngine:
          r.session.agentEngine === 'codex' || r.session.agentEngine === 'local'
            ? r.session.agentEngine
            : agentEngine,
      } satisfies ForgeSession;
      set((st) => ({
        sessions: st.sessions.map((s) => (s.id === id ? session : s)),
      }));
      // chatStore 静态依赖 sessionStore；这里延迟导入，避免模块初始化循环。
      const { useChatStore } = await import('./chatStore');
      if (get().activeSessionId === id) {
        useChatStore.setState({
          selectedModelId: session.selectedModelId ?? null,
          thinkingEnabled: session.thinkingEnabled,
          reasoningEffort: session.reasoningEffort ?? null,
          contextOptionId: session.contextOptionId ?? null,
        });
      }
    } catch (err) {
      set({ sessions: prev });
      toastError(err, '切换 Agent 引擎失败');
    }
  },

  setDraftAgentEngine: (agentEngine) => {
    set({ draftAgentEngine: agentEngine, draftAgentEngineTouched: true });
  },

  hydrateAgentDefaults: (raw) => {
    if (raw !== 'local' && raw !== 'codex') return;
    const agentEngine: AgentEngine = raw;
    set((st) => ({
      defaultAgentEngine: agentEngine,
      draftAgentEngine: st.draftAgentEngineTouched ? st.draftAgentEngine : agentEngine,
    }));
  },

  moveToFolder: async (id, folderId) => {
    const prev = get().sessions;
    set((st) => ({
      sessions: st.sessions.map((s) => (s.id === id ? { ...s, folderId } : s)),
    }));
    try {
      const r = await apiPatch<{ session: ForgeSession }>(`/api/forge/sessions/${id}`, {
        folderId,
      });
      set((st) => ({
        sessions: st.sessions.map((s) => (s.id === id ? r.session : s)),
      }));
    } catch (err) {
      set({ sessions: prev });
      toastError(err, '移动会话失败');
    }
  },

  fork: async (id) => {
    try {
      const r = await apiPost<{ session: ForgeSession }>(`/api/forge/sessions/${id}/fork`, {});
      set((st) => ({
        sessions: [r.session, ...st.sessions],
        activeSessionId: r.session.id,
      }));
      useToastStore.getState().push('info', '已基于当前会话创建分支');
      return r.session;
    } catch (err) {
      toastError(err, '会话分叉失败');
      return null;
    }
  },

  createFolder: async (name) => {
    const trimmed = name.trim();
    if (trimmed === '') return null;
    try {
      const workspaceId = useWorkspaceStore.getState().activeWorkspaceId;
      const r = await apiPost<{ folder: ChatFolder }>('/api/forge/chat-folders', {
        name: trimmed,
        ...(workspaceId ? { workspaceId } : {}),
      });
      set((st) => ({ folders: [...st.folders, r.folder] }));
      return r.folder;
    } catch (err) {
      toastError(err, '新建文件夹失败');
      return null;
    }
  },

  removeFolder: async (id) => {
    const prevFolders = get().folders;
    const prevSessions = get().sessions;
    // 后端级联清 folderId(sessions.rs clear_folder),前端同步乐观清
    set((st) => ({
      folders: st.folders.filter((f) => f.id !== id),
      sessions: st.sessions.map((s) => (s.folderId === id ? { ...s, folderId: null } : s)),
    }));
    try {
      await apiDelete(`/api/forge/chat-folders/${id}`);
    } catch (err) {
      set({ folders: prevFolders, sessions: prevSessions });
      toastError(err, '删除文件夹失败');
    }
  },

  renameFolder: async (id, name) => {
    const trimmed = name.trim();
    const prev = get().folders;
    if (trimmed === '' || prev.find((f) => f.id === id)?.name === trimmed) return;
    set((st) => ({ folders: st.folders.map((f) => (f.id === id ? { ...f, name: trimmed } : f)) }));
    try {
      const r = await apiPatch<{ folder: ChatFolder }>(`/api/forge/chat-folders/${id}`, { name: trimmed });
      set((st) => ({ folders: st.folders.map((f) => (f.id === id ? r.folder : f)) }));
    } catch (err) {
      set({ folders: prev });
      toastError(err, '重命名文件夹失败');
    }
  },
}));
