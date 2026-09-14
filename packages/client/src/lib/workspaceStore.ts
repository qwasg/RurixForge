import { create } from 'zustand';
import { readActiveWorkspaceId, writeActiveWorkspaceId } from './activeWorkspace';
import { apiDelete, apiGet, apiPatch, apiPost } from './forgeApi';
import { useToastStore } from './toastStore';

/**
 * 工作区事实源(agentd /api/forge/workspaces REST)。
 * 名称 + 磁盘根目录;侧栏顶层分组 + 右栏文件树/原生工具沙箱根。
 */

export interface ForgeWorkspace {
  id: string;
  name: string;
  root: string;
  createdAt: string;
  updatedAt: string;
}

/**
 * 展示用根路径:后端 create/patch 走 canonicalize,Windows 下会带 `\\?\`
 * (UNC 则是 `\\?\UNC\`)扩展长度前缀。存的值照旧(沙箱解析要用),只在 UI 上摘掉前缀。
 */
export function displayRoot(root: string): string {
  if (root.startsWith('\\\\?\\UNC\\')) return `\\\\${root.slice(8)}`;
  if (root.startsWith('\\\\?\\')) return root.slice(4);
  return root;
}

const RECENT_KEY = 'forge:recentWorkspaces';
/** 最近列表上限(参考 Cursor 工作区选择器的 Recents 段落长度)。 */
const RECENT_LIMIT = 8;

// 当前工作区镜像键由 activeWorkspace.ts 统一读写:forgeApi 每次 MCP 调用据此带 workspaceId
// (项目作用域),两侧不互相 import。
const readActiveId = readActiveWorkspaceId;
const writeActiveId = writeActiveWorkspaceId;

function readRecentIds(): string[] {
  try {
    const raw = localStorage.getItem(RECENT_KEY);
    if (raw === null) return [];
    const v: unknown = JSON.parse(raw);
    return Array.isArray(v) ? v.filter((x): x is string => typeof x === 'string') : [];
  } catch {
    return [];
  }
}

function writeRecentIds(ids: string[]): void {
  try {
    localStorage.setItem(RECENT_KEY, JSON.stringify(ids));
  } catch {
    /* ignore */
  }
}

/** id 提到队首并去重截断;返回新序(旧序不变则原样返回)。 */
function touchRecent(ids: readonly string[], id: string): string[] {
  return [id, ...ids.filter((x) => x !== id)].slice(0, RECENT_LIMIT);
}

interface WorkspaceState {
  workspaces: ForgeWorkspace[];
  activeWorkspaceId: string | null;
  /** 最近切换过的工作区 id,新→旧;工作区选择器 Recents 段落的排序依据。 */
  recentIds: string[];
  loading: boolean;
  offline: boolean;

  loadAll: () => Promise<void>;
  create: (name: string, root: string) => Promise<ForgeWorkspace | null>;
  rename: (id: string, name: string) => Promise<void>;
  remove: (id: string) => Promise<void>;
  setActive: (id: string | null) => void;
}

function toastError(err: unknown, prefix: string): void {
  const msg = err instanceof Error ? err.message : String(err);
  useToastStore.getState().push('error', `${prefix}:${msg}`);
}

export const useWorkspaceStore = create<WorkspaceState>((set, get) => ({
  workspaces: [],
  activeWorkspaceId: readActiveId(),
  recentIds: readRecentIds(),
  loading: false,
  offline: false,

  loadAll: async () => {
    set({ loading: true });
    try {
      const r = await apiGet<{ workspaces: ForgeWorkspace[] }>('/api/forge/workspaces');
      const active = get().activeWorkspaceId;
      const validActive =
        active !== null && r.workspaces.some((w) => w.id === active) ? active : null;
      if (validActive !== active) writeActiveId(validActive);
      // 后端已删掉的工作区从最近列表里剔除,免得选择器留死条目
      const live = new Set(r.workspaces.map((w) => w.id));
      const recentIds = get().recentIds.filter((id) => live.has(id));
      if (recentIds.length !== get().recentIds.length) writeRecentIds(recentIds);
      set({
        workspaces: r.workspaces,
        loading: false,
        offline: false,
        activeWorkspaceId: validActive,
        recentIds,
      });
    } catch (err) {
      set({ loading: false, offline: true });
      toastError(err, '工作区加载失败');
    }
  },

  create: async (name, root) => {
    const trimmedName = name.trim();
    const trimmedRoot = root.trim();
    if (trimmedName === '' || trimmedRoot === '') return null;
    try {
      const r = await apiPost<{ workspace: ForgeWorkspace }>('/api/forge/workspaces', {
        name: trimmedName,
        root: trimmedRoot,
      });
      const recentIds = touchRecent(get().recentIds, r.workspace.id);
      set((st) => ({
        workspaces: [...st.workspaces.filter((w) => w.id !== r.workspace.id), r.workspace],
        activeWorkspaceId: r.workspace.id,
        recentIds,
        offline: false,
      }));
      writeActiveId(r.workspace.id);
      writeRecentIds(recentIds);
      return r.workspace;
    } catch (err) {
      toastError(err, '新建工作区失败');
      return null;
    }
  },

  rename: async (id, name) => {
    const trimmed = name.trim();
    if (trimmed === '') return;
    const prev = get().workspaces;
    set((st) => ({
      workspaces: st.workspaces.map((w) => (w.id === id ? { ...w, name: trimmed } : w)),
    }));
    try {
      const r = await apiPatch<{ workspace: ForgeWorkspace }>(`/api/forge/workspaces/${id}`, {
        name: trimmed,
      });
      set((st) => ({
        workspaces: st.workspaces.map((w) => (w.id === id ? r.workspace : w)),
      }));
    } catch (err) {
      set({ workspaces: prev });
      toastError(err, '重命名工作区失败');
    }
  },

  remove: async (id) => {
    const prev = get().workspaces;
    const prevActive = get().activeWorkspaceId;
    const prevRecent = get().recentIds;
    const recentIds = prevRecent.filter((x) => x !== id);
    set((st) => ({
      workspaces: st.workspaces.filter((w) => w.id !== id),
      activeWorkspaceId: st.activeWorkspaceId === id ? null : st.activeWorkspaceId,
      recentIds,
    }));
    if (prevActive === id) writeActiveId(null);
    writeRecentIds(recentIds);
    try {
      await apiDelete(`/api/forge/workspaces/${id}`);
    } catch (err) {
      set({ workspaces: prev, activeWorkspaceId: prevActive, recentIds: prevRecent });
      if (prevActive) writeActiveId(prevActive);
      writeRecentIds(prevRecent);
      toastError(err, '删除工作区失败');
    }
  },

  setActive: (id) => {
    writeActiveId(id);
    if (id === null) {
      set({ activeWorkspaceId: null });
      return;
    }
    const recentIds = touchRecent(get().recentIds, id);
    writeRecentIds(recentIds);
    set({ activeWorkspaceId: id, recentIds });
  },
}));
