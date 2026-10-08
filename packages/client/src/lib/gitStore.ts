import { create } from 'zustand';
import { apiWorkspaceGit, type GitFileEntry, type GitFileStatus, type WorkspaceGitResp } from './forgeApi';
import { useChatStore } from './chatStore';
import { createPoller, usePoller } from './poller';
import { useWorkspaceStore } from './workspaceStore';

/**
 * 工作区 git 状态(D-040;agentd GET /api/forge/workspace/git):状态栏分支段 + 文件树改动标记。
 * 刷新时机:切工作区、agent 运行结束、文件保存成功各一次;平时 15s 轮询,页面隐藏时暂停。
 */

interface GitState {
  status: WorkspaceGitResp | null;
  /** status 所属工作区(null = 默认根)。 */
  workspaceId: string | null;
  error: string | null;
  loading: boolean;
  /** 文件树「仅看改动」(状态栏分支段点进来时打开)。 */
  changesOnly: boolean;
  refresh: () => Promise<void>;
  setChangesOnly: (v: boolean) => void;
}

const POLL_MS = 15_000;

export const useGitStore = create<GitState>((set, get) => ({
  status: null,
  workspaceId: null,
  error: null,
  loading: false,
  changesOnly: false,

  refresh: async () => {
    const workspaceId = useWorkspaceStore.getState().activeWorkspaceId;
    set({ loading: true });
    try {
      const status = await apiWorkspaceGit(workspaceId);
      // 请求途中切了工作区:丢弃旧结果(切换本身会再触发一次)
      if (useWorkspaceStore.getState().activeWorkspaceId !== workspaceId) return;
      set({ status, workspaceId, error: null, loading: false });
    } catch (err) {
      if (useWorkspaceStore.getState().activeWorkspaceId !== workspaceId) return;
      set({ status: null, workspaceId, error: err instanceof Error ? err.message : String(err), loading: false });
    } finally {
      if (get().loading) set({ loading: false });
    }
  },

  setChangesOnly: (v) => set({ changesOnly: v }),
}));

const gitPoller = createPoller(() => useGitStore.getState().refresh(), POLL_MS, {
  onStart: () => {
    const offWs = useWorkspaceStore.subscribe((st, prev) => {
      if (st.activeWorkspaceId !== prev.activeWorkspaceId) {
        useGitStore.setState({ status: null, changesOnly: false });
        void useGitStore.getState().refresh();
      }
    });
    // agent 一轮跑完(可能改了文件)立即刷新,不等下一个 15s
    const offRun = useChatStore.subscribe((st, prev) => {
      if (prev.activeRunId !== null && st.activeRunId === null) void useGitStore.getState().refresh();
    });
    return () => {
      offWs();
      offRun();
    };
  },
});

/** 组件挂载期间保持 git 状态轮询(引用计数)。 */
export function useGitPolling(): void {
  usePoller(gitPoller);
}

// ---------- 文件树索引(纯函数,导出供单测) ----------

export interface GitIndex {
  /** 工作区相对路径 → 条目(文件与未跟踪目录)。 */
  entries: Map<string, GitFileEntry>;
  /** 含改动的祖先目录(文件树目录行画点)。 */
  dirty: Set<string>;
  /** 未跟踪目录(其下所有文件都算未跟踪)。 */
  untrackedDirs: string[];
}

export function buildGitIndex(status: WorkspaceGitResp | null): GitIndex {
  const entries = new Map<string, GitFileEntry>();
  const dirty = new Set<string>();
  const untrackedDirs: string[] = [];
  for (const f of status?.files ?? []) {
    entries.set(f.path, f);
    if (f.dir) untrackedDirs.push(f.path);
    const parts = f.path.split('/');
    for (let i = 1; i < parts.length; i++) dirty.add(parts.slice(0, i).join('/'));
  }
  return { entries, dirty, untrackedDirs };
}

/** 路径的 git 状态:直接命中,或落在未跟踪目录之下(继承 U)。 */
export function gitStatusOf(index: GitIndex, path: string): GitFileStatus | null {
  const hit = index.entries.get(path);
  if (hit) return hit.status;
  return index.untrackedDirs.some((d) => path.startsWith(`${d}/`)) ? 'U' : null;
}

/** 状态字母 → 语义色(与文件树/状态栏共用;删除用 danger 属于「内容状态」而非报错提示)。 */
export const GIT_STATUS_TONE: Record<GitFileStatus, string> = {
  M: 'text-warn',
  A: 'text-sage',
  U: 'text-sage',
  D: 'text-danger',
  R: 'text-info',
  C: 'text-danger',
};

export const GIT_STATUS_LABEL: Record<GitFileStatus, string> = {
  M: '已修改',
  A: '已新增',
  U: '未跟踪',
  D: '已删除',
  R: '已重命名',
  C: '有冲突',
};
