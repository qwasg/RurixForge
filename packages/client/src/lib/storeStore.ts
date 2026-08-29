import { create } from 'zustand';
import { ForgeApiError } from './forgeApi';
import {
  StoreProposalRequiredError,
  storeAddLibrary,
  storeAddSource,
  storeApproveProposal,
  storeCheckUpdates,
  storeDeleteSource,
  storeGetPackage,
  storeGetTask,
  storeGetVersion,
  storeInstall,
  storeInstallLibraryItem,
  storeListInstalled,
  storeListLibrary,
  storeListSources,
  storePatchSource,
  storeRemoveLibrary,
  storeSearch,
  storeUninstall,
  type InstallRecord,
  type LibraryItem,
  type PackageDetail,
  type PackageKind,
  type PackageManifest,
  type SearchResponse,
  type StoreSource,
  type StoreTask,
  type UpdateInfo,
} from './storeApi';
import { useToastStore } from './toastStore';

/**
 * F11 wave.4 资产商店状态(照 assetStore 形态:裸 create,无 middleware)。
 * 三块业务态(discover / installed / library)+ 源清单 + 当前详情 + 安装长任务。
 *
 * 长任务轮询整体收在 store 里(startPolling/stopPolling),组件只订阅 task 字段;
 * 定时器句柄放模块级而非 state——它不参与渲染,进 state 只会白白触发订阅者刷新。
 */

export type StoreSubTab = 'discover' | 'installed' | 'library';
export type KindFilter = 'all' | PackageKind;
/** 当前长任务在干什么(安装与卸载共用一套 taskId 轮询,文案要分得清)。 */
export type TaskKind = 'install' | 'uninstall';

export const STORE_PAGE_SIZE = 12;
export const POLL_INTERVAL_MS = 1000;

/** 安装阶段中文文案(download 带 x/y 计数;未知 phase 原样透出,不假装认识)。 */
export function phaseLabel(task: StoreTask): string {
  switch (task.phase) {
    case 'resolve':
      return '解析清单';
    case 'download':
      return `下载中 ${task.done}/${task.total}`;
    case 'verify':
      return '校验完整性';
    case 'import':
      return '导入构建';
    case 'record':
      return '登记';
    default:
      return task.phase === '' ? '处理中' : task.phase;
  }
}

/** 错误码 → 可读文案(缺省用后端 message;错误码本身始终由 UI 一并展示)。 */
export const STORE_ERROR_TEXT: Record<string, string> = {
  STORE_SOURCE_UNREACHABLE: '源不可达，无法取得包清单',
  STORE_SOURCE_NOT_FOUND: '源不存在',
  STORE_PACKAGE_NOT_FOUND: '包不存在',
  STORE_VERSION_NOT_FOUND: '该版本不存在',
  STORE_MANIFEST_INVALID: '包清单格式非法',
  STORE_CHECKSUM_MISMATCH: '文件校验失败，已中止安装',
  STORE_ALREADY_INSTALLED: '该包已安装',
  STORE_NOT_INSTALLED: '该包未安装',
  STORE_DEPENDENCY_UNRESOLVED: '依赖无法解析',
  STORE_PAYMENT_REQUIRED: '该包需付费获取，当前没有支付通道',
  STORE_TASK_NOT_FOUND: '任务不存在或已过期',
  GOV_PROPOSAL_REQUIRED: '需治理提案批准后才能执行',
};

export function storeErrorText(code: string, message: string): string {
  return STORE_ERROR_TEXT[code] ?? message;
}

/** 任意异常 → {code,message}(非 ForgeApiError 退 UNKNOWN,消息如实保留)。 */
export function toErr(err: unknown): { code: string; message: string } {
  if (err instanceof ForgeApiError) return { code: err.code, message: err.message };
  return { code: 'UNKNOWN', message: err instanceof Error ? err.message : String(err) };
}

/** 卸载两阶段确认条(非模态,行内展开;模态 dialog 会永久阻塞无人值守冒烟,见 F1 坑)。 */
export interface PendingUninstall {
  sourceId: string;
  packageId: string;
  /** confirm = 待确认;proposal = 已收到 409 待批准;working = 已发出、等任务 */
  stage: 'confirm' | 'proposal' | 'working';
  proposalId: string | null;
  error: string | null;
}

export function pkgKey(sourceId: string, packageId: string): string {
  return `${sourceId}/${packageId}`;
}

interface StoreState {
  subTab: StoreSubTab;

  // ---- 源 ----
  sources: StoreSource[];
  sourcesLoading: boolean;
  sourcesError: string | null;
  sourcesPanelOpen: boolean;

  // ---- 发现 ----
  query: string;
  submittedQuery: string;
  kindFilter: KindFilter;
  sourceFilter: string;
  page: number;
  search: SearchResponse | null;
  searchLoading: boolean;
  searchError: string | null;

  // ---- 详情(同 tab 右侧分栏,非弹窗) ----
  detail: PackageDetail | null;
  manifest: PackageManifest | null;
  detailVersion: string | null;
  detailLoading: boolean;
  detailError: string | null;
  destFolder: string;

  // ---- 安装/卸载长任务 ----
  task: StoreTask | null;
  taskKind: TaskKind;
  taskError: { code: string; message: string } | null;
  installing: boolean;
  pollingTaskId: string | null;

  // ---- 已安装 ----
  installed: InstallRecord[];
  installedLoading: boolean;
  installedError: string | null;
  updates: UpdateInfo[];
  updatesLoading: boolean;
  updatesError: string | null;
  pendingUninstall: PendingUninstall | null;

  // ---- 个人资产库 ----
  library: LibraryItem[];
  libraryLoading: boolean;
  libraryError: string | null;

  setSubTab: (t: StoreSubTab) => void;
  toggleSourcesPanel: () => void;

  loadSources: () => Promise<void>;
  addSource: (payload: { id: string; name: string; baseUrl: string; token?: string }) => Promise<void>;
  setSourceEnabled: (id: string, enabled: boolean) => Promise<void>;
  removeSource: (id: string) => Promise<void>;

  setQuery: (q: string) => void;
  setKindFilter: (k: KindFilter) => void;
  setSourceFilter: (id: string) => void;
  setPage: (p: number) => void;
  runSearch: () => Promise<void>;

  openDetail: (sourceId: string, pkgId: string) => Promise<void>;
  closeDetail: () => void;
  selectVersion: (version: string) => Promise<void>;
  setDestFolder: (f: string) => void;

  install: (sourceId: string, pkgId: string, version: string, destFolder?: string) => Promise<void>;
  startPolling: (taskId: string, kind?: TaskKind) => void;
  stopPolling: () => void;

  loadInstalled: () => Promise<void>;
  checkUpdates: () => Promise<void>;
  requestUninstall: (sourceId: string, packageId: string) => void;
  cancelUninstall: () => void;
  confirmUninstall: () => Promise<void>;
  approveUninstall: () => Promise<void>;

  loadLibrary: () => Promise<void>;
  addToLibrary: (assetPath: string) => Promise<void>;
  removeFromLibrary: (id: string) => Promise<void>;
  installLibraryItem: (id: string, destFolder?: string) => Promise<void>;
}

let pollTimer: ReturnType<typeof setInterval> | null = null;
/** 单次轮询在飞:防止慢响应下定时器叠请求。 */
let pollInFlight = false;

const toast = (kind: 'success' | 'error' | 'warning' | 'info', msg: string) =>
  useToastStore.getState().push(kind, msg);

export const useStoreStore = create<StoreState>((set, get) => ({
  subTab: 'discover',

  sources: [],
  sourcesLoading: false,
  sourcesError: null,
  sourcesPanelOpen: false,

  query: '',
  submittedQuery: '',
  kindFilter: 'all',
  sourceFilter: '',
  page: 1,
  search: null,
  searchLoading: false,
  searchError: null,

  detail: null,
  manifest: null,
  detailVersion: null,
  detailLoading: false,
  detailError: null,
  destFolder: '',

  task: null,
  taskKind: 'install',
  taskError: null,
  installing: false,
  pollingTaskId: null,

  installed: [],
  installedLoading: false,
  installedError: null,
  updates: [],
  updatesLoading: false,
  updatesError: null,
  pendingUninstall: null,

  library: [],
  libraryLoading: false,
  libraryError: null,

  setSubTab: (t) => set({ subTab: t }),
  toggleSourcesPanel: () => set((s) => ({ sourcesPanelOpen: !s.sourcesPanelOpen })),

  // ---------- 源 ----------

  loadSources: async () => {
    set({ sourcesLoading: true, sourcesError: null });
    try {
      const r = await storeListSources();
      set({ sources: r.sources ?? [], sourcesLoading: false });
    } catch (err) {
      const e = toErr(err);
      set({ sourcesError: `${storeErrorText(e.code, e.message)}(${e.code})`, sourcesLoading: false });
    }
  },

  addSource: async (payload) => {
    try {
      await storeAddSource(payload);
      toast('success', `已添加源 ${payload.id}`);
      await get().loadSources();
    } catch (err) {
      const e = toErr(err);
      toast('error', `添加源失败：${storeErrorText(e.code, e.message)}(${e.code})`);
      throw err;
    }
  },

  setSourceEnabled: async (id, enabled) => {
    // 乐观翻转 + 失败回滚(照 assetStore 写法)。
    const before = get().sources;
    set({ sources: before.map((s) => (s.id === id ? { ...s, enabled } : s)) });
    try {
      await storePatchSource(id, { enabled });
      await get().loadSources();
    } catch (err) {
      const e = toErr(err);
      set({ sources: before });
      toast('error', `切换源状态失败：${storeErrorText(e.code, e.message)}(${e.code})`);
    }
  },

  removeSource: async (id) => {
    const before = get().sources;
    set({ sources: before.filter((s) => s.id !== id) });
    try {
      await storeDeleteSource(id);
      toast('success', `已删除源 ${id}`);
      await get().loadSources();
    } catch (err) {
      const e = toErr(err);
      set({ sources: before });
      toast('error', `删除源失败：${storeErrorText(e.code, e.message)}(${e.code})`);
    }
  },

  // ---------- 发现 ----------

  setQuery: (q) => set({ query: q }),
  setKindFilter: (k) => {
    set({ kindFilter: k, page: 1 });
    void get().runSearch();
  },
  setSourceFilter: (id) => {
    set({ sourceFilter: id, page: 1 });
    void get().runSearch();
  },
  setPage: (p) => {
    set({ page: Math.max(1, p) });
    void get().runSearch();
  },

  runSearch: async () => {
    const { query, kindFilter, sourceFilter, page } = get();
    set({ searchLoading: true, searchError: null, submittedQuery: query });
    try {
      const r = await storeSearch({
        q: query.trim() === '' ? undefined : query.trim(),
        kind: kindFilter === 'all' ? undefined : kindFilter,
        sourceId: sourceFilter === '' ? undefined : sourceFilter,
        page,
        pageSize: STORE_PAGE_SIZE,
      });
      set({ search: r, searchLoading: false });
    } catch (err) {
      const e = toErr(err);
      // 搜索整体失败不伪装成空结果:清掉旧命中并如实上报。
      set({
        search: null,
        searchError: `${storeErrorText(e.code, e.message)}(${e.code})`,
        searchLoading: false,
      });
    }
  },

  // ---------- 详情 ----------

  openDetail: async (sourceId, pkgId) => {
    set({
      detailLoading: true,
      detailError: null,
      detail: null,
      manifest: null,
      detailVersion: null,
      task: null,
      taskError: null,
    });
    try {
      const d = await storeGetPackage(sourceId, pkgId);
      const version = d.summary?.latestVersion ?? d.versions?.[0] ?? '';
      set({ detail: d, detailVersion: version, detailLoading: false });
      if (version !== '') {
        const v = await storeGetVersion(sourceId, pkgId, version);
        set({ manifest: v.manifest });
      }
    } catch (err) {
      const e = toErr(err);
      set({ detailError: `${storeErrorText(e.code, e.message)}(${e.code})`, detailLoading: false });
    }
  },

  closeDetail: () => {
    get().stopPolling();
    set({
      detail: null,
      manifest: null,
      detailVersion: null,
      detailError: null,
      task: null,
      taskError: null,
    });
  },

  selectVersion: async (version) => {
    const d = get().detail;
    if (!d) return;
    set({ detailVersion: version, manifest: null, detailError: null });
    try {
      const v = await storeGetVersion(d.sourceId, d.summary.id, version);
      set({ manifest: v.manifest });
    } catch (err) {
      const e = toErr(err);
      set({ detailError: `${storeErrorText(e.code, e.message)}(${e.code})` });
    }
  },

  setDestFolder: (f) => set({ destFolder: f }),

  // ---------- 安装 / 轮询 ----------

  install: async (sourceId, pkgId, version, destFolder) => {
    if (get().installing) return;
    set({ installing: true, task: null, taskError: null, taskKind: 'install' });
    try {
      const r = await storeInstall({
        sourceId,
        pkgId,
        version,
        ...(destFolder && destFolder.trim() !== '' ? { destFolder: destFolder.trim() } : {}),
      });
      set({ installing: false });
      get().startPolling(r.taskId, 'install');
    } catch (err) {
      const e = toErr(err);
      set({ installing: false, taskError: e });
      toast('error', `安装失败：${storeErrorText(e.code, e.message)}(${e.code})`);
    }
  },

  startPolling: (taskId, kind = 'install') => {
    get().stopPolling();
    set({ pollingTaskId: taskId, taskKind: kind });

    const tick = async () => {
      if (pollInFlight) return;
      if (get().pollingTaskId !== taskId) return;
      pollInFlight = true;
      try {
        const t = await storeGetTask(taskId);
        if (get().pollingTaskId !== taskId) return;
        set({ task: t });
        const verb = kind === 'uninstall' ? '卸载' : '安装';
        if (t.status === 'completed') {
          get().stopPolling();
          toast('success', `${verb}完成`);
          await get().loadInstalled();
        } else if (t.status === 'failed') {
          get().stopPolling();
          const e = t.error ?? { code: 'UNKNOWN', message: '任务失败但未给出错误信息' };
          set({ taskError: e });
          toast('error', `${verb}失败：${storeErrorText(e.code, e.message)}(${e.code})`);
        }
      } catch (err) {
        if (get().pollingTaskId !== taskId) return;
        const e = toErr(err);
        get().stopPolling();
        set({ taskError: e });
        toast('error', `进度查询失败：${storeErrorText(e.code, e.message)}(${e.code})`);
      } finally {
        pollInFlight = false;
      }
    };

    pollTimer = setInterval(() => void tick(), POLL_INTERVAL_MS);
    void tick();
  },

  stopPolling: () => {
    if (pollTimer !== null) {
      clearInterval(pollTimer);
      pollTimer = null;
    }
    pollInFlight = false;
    if (get().pollingTaskId !== null) set({ pollingTaskId: null });
  },

  // ---------- 已安装 ----------

  loadInstalled: async () => {
    set({ installedLoading: true, installedError: null });
    try {
      const r = await storeListInstalled();
      set({ installed: r.installed ?? [], installedLoading: false });
    } catch (err) {
      const e = toErr(err);
      set({
        installedError: `${storeErrorText(e.code, e.message)}(${e.code})`,
        installedLoading: false,
      });
    }
  },

  checkUpdates: async () => {
    set({ updatesLoading: true, updatesError: null });
    try {
      const r = await storeCheckUpdates();
      const updates = r.updates ?? [];
      set({ updates, updatesLoading: false });
      const n = updates.filter((u) => u.hasUpdate).length;
      toast('info', n === 0 ? '全部为最新版本' : `${n} 个包有更新`);
    } catch (err) {
      const e = toErr(err);
      set({
        updatesError: `${storeErrorText(e.code, e.message)}(${e.code})`,
        updatesLoading: false,
      });
    }
  },

  requestUninstall: (sourceId, packageId) =>
    set({ pendingUninstall: { sourceId, packageId, stage: 'confirm', proposalId: null, error: null } }),

  cancelUninstall: () => set({ pendingUninstall: null }),

  confirmUninstall: async () => {
    const pu = get().pendingUninstall;
    if (!pu) return;
    try {
      const r = await storeUninstall({ sourceId: pu.sourceId, pkgId: pu.packageId });
      set({ pendingUninstall: null });
      get().startPolling(r.taskId, 'uninstall');
    } catch (err) {
      if (err instanceof StoreProposalRequiredError) {
        // 治理两阶段:409 带回 proposalId,确认条就地变「已创建提案 + 在此批准」。
        set({ pendingUninstall: { ...pu, stage: 'proposal', proposalId: err.proposalId, error: null } });
        return;
      }
      const e = toErr(err);
      set({
        pendingUninstall: { ...pu, stage: 'confirm', error: `${storeErrorText(e.code, e.message)}(${e.code})` },
      });
    }
  },

  approveUninstall: async () => {
    const pu = get().pendingUninstall;
    if (!pu || pu.proposalId === null) return;
    try {
      await storeApproveProposal(pu.proposalId);
    } catch (err) {
      const e = toErr(err);
      set({ pendingUninstall: { ...pu, error: `批准失败：${storeErrorText(e.code, e.message)}(${e.code})` } });
      return;
    }
    set({ pendingUninstall: { ...pu, stage: 'working', error: null } });
    // 批准后自动重发卸载。
    try {
      const r = await storeUninstall({ sourceId: pu.sourceId, pkgId: pu.packageId });
      set({ pendingUninstall: null });
      get().startPolling(r.taskId, 'uninstall');
    } catch (err) {
      const e = toErr(err);
      set({
        pendingUninstall: { ...pu, stage: 'proposal', error: `${storeErrorText(e.code, e.message)}(${e.code})` },
      });
    }
  },

  // ---------- 个人资产库 ----------

  loadLibrary: async () => {
    set({ libraryLoading: true, libraryError: null });
    try {
      const r = await storeListLibrary();
      set({ library: r.items ?? [], libraryLoading: false });
    } catch (err) {
      const e = toErr(err);
      set({ libraryError: `${storeErrorText(e.code, e.message)}(${e.code})`, libraryLoading: false });
    }
  },

  addToLibrary: async (assetPath) => {
    try {
      await storeAddLibrary(assetPath);
      toast('success', `已收藏 ${assetPath}`);
      await get().loadLibrary();
    } catch (err) {
      const e = toErr(err);
      toast('error', `收藏失败：${storeErrorText(e.code, e.message)}(${e.code})`);
    }
  },

  removeFromLibrary: async (id) => {
    const before = get().library;
    set({ library: before.filter((i) => i.id !== id) });
    try {
      await storeRemoveLibrary(id);
      toast('success', '已移出资产库');
      await get().loadLibrary();
    } catch (err) {
      const e = toErr(err);
      set({ library: before });
      toast('error', `移出失败：${storeErrorText(e.code, e.message)}(${e.code})`);
    }
  },

  installLibraryItem: async (id, destFolder) => {
    try {
      const r = await storeInstallLibraryItem(id, destFolder);
      toast('success', `已装进项目：${r.assetPath}`);
    } catch (err) {
      const e = toErr(err);
      toast('error', `装进项目失败：${storeErrorText(e.code, e.message)}(${e.code})`);
    }
  },
}));
