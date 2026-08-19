import { create } from 'zustand';
import { apiDelete, apiGet, apiPatch, apiPost } from './forgeApi';
import { useToastStore } from './toastStore';

/**
 * F7 wave.3 会话事实源(agentd sessions/chat-folders REST,wave.1/2 产物)。
 * 全走 forgeApi;乐观更新 + 失败回滚 + toast 报错。
 * wire 对齐 agentd sessions.rs(camelCase)。
 */

export interface ForgeSession {
  id: string;
  title: string;
  status: string;
  agentKind: string;
  selectedModelId: string | null;
  webSearchEnabled: boolean;
  activeRunId: string | null;
  createdAt: string;
  updatedAt: string;
  pinned: boolean;
  titleManuallySet: boolean;
  folderId?: string | null;
}

export interface ChatFolder {
  id: string;
  name: string;
  createdAt: string;
  updatedAt: string;
}

interface SessionState {
  sessions: ForgeSession[];
  folders: ChatFolder[];
  activeSessionId: string | null;
  loading: boolean;
  /** 后端不可达(首次 loadAll 失败)——侧栏如实显示「后端未连接」 */
  offline: boolean;

  loadAll: () => Promise<void>;
  create: (title?: string) => Promise<ForgeSession | null>;
  select: (id: string) => void;
  rename: (id: string, title: string) => Promise<void>;
  togglePin: (id: string) => Promise<void>;
  remove: (id: string) => Promise<void>;
  moveToFolder: (id: string, folderId: string | null) => Promise<void>;
  fork: (id: string) => Promise<ForgeSession | null>;
  createFolder: (name: string) => Promise<ChatFolder | null>;
  removeFolder: (id: string) => Promise<void>;
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

  loadAll: async () => {
    set({ loading: true });
    try {
      const [s, f] = await Promise.all([
        apiGet<{ sessions: ForgeSession[] }>('/api/forge/sessions'),
        apiGet<{ folders: ChatFolder[] }>('/api/forge/chat-folders'),
      ]);
      set({ sessions: s.sessions, folders: f.folders, loading: false, offline: false });
    } catch (err) {
      set({ loading: false, offline: true });
      toastError(err, '会话加载失败');
    }
  },

  create: async (title) => {
    try {
      const r = await apiPost<{ session: ForgeSession }>('/api/forge/sessions', {
        title: title ?? '',
      });
      set((st) => ({
        sessions: [r.session, ...st.sessions],
        activeSessionId: r.session.id,
        offline: false,
      }));
      return r.session;
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
      const r = await apiPost<{ folder: ChatFolder }>('/api/forge/chat-folders', {
        name: trimmed,
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
}));
