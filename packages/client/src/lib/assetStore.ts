import { create } from 'zustand';
import { callAssetTool, callTool } from './forgeApi';

/** 资产条目(与 asset_list 返回对齐;F10 语义化:description/tags 来自 .meta semantic 段)。 */
export interface AssetItem {
  path: string;
  guid: string;
  type: string;
  size: number;
  description?: string;
  tags?: string[];
}

/** .meta semantic 段(asset_get_meta 返回;I-7 溯源:source/model/updated_at)。 */
export interface AssetSemantic {
  description?: string;
  tags?: string[];
  source?: string;
  model?: string;
  updated_at?: string;
  content_hash?: string;
}

/** 资产构建状态(asset_build_status 返回)。 */
export interface AssetBuildStatus {
  path: string;
  state: 'current' | 'stale' | 'building' | 'failed';
  hash: string;
}

export type AssetViewMode = 'grid' | 'list';
export type AssetTypeFilter = 'all' | 'mesh' | 'texture' | 'material' | 'prefab' | 'scene' | 'script' | 'audio';

/** 右键菜单动作(六菜单,07 §4;F5 wave.3:gen-chat seam → gen-dialog 真实对话框)。 */
export type AssetMenuAction =
  | 'import-here'
  | 'reimport'
  | 'show-in-folder'
  | 'find-refs'
  | 'delete-proposal'
  | 'gen-dialog';

/** 引用查询结果(GUID 已尽量解析为路径;未知 GUID 原样显示)。 */
export interface RefsResult {
  path: string;
  refs: string[];
  referencedBy: string[];
}

/** 删除提案(非模态;blocked = 引用阻断清单,null = 待确认)。 */
export interface DeleteProposal {
  path: string;
  blocked: string[] | null;
}

interface AssetState {
  items: AssetItem[];
  status: Record<string, AssetBuildStatus['state']>;
  loading: boolean;
  error: string | null;
  viewMode: AssetViewMode;
  typeFilter: AssetTypeFilter;
  search: string;
  currentFolder: string;
  selectedGuid: string | null;
  /** 贴图缩略图 dataUrl 缓存(guid → dataUrl;'none' = 无缩略图/失败,不再重试) */
  thumbs: Record<string, string>;
  /** 引用查询浮层结果(null = 关闭) */
  refsResult: RefsResult | null;
  /** 删除提案条(null = 无进行中提案) */
  pendingDelete: DeleteProposal | null;

  load: () => Promise<void>;
  setViewMode: (m: AssetViewMode) => void;
  setTypeFilter: (t: AssetTypeFilter) => void;
  setSearch: (s: string) => void;
  setCurrentFolder: (f: string) => void;
  setSelectedGuid: (g: string | null) => void;
  /** 在 Assets 底栏定位到某路径(开文件夹 + 搜索 basename) */
  focusPath: (path: string) => void;

  /** 拖拽实例化:mesh/prefab 进 Viewport → entity_create + MeshRenderer.mesh=GUID。 */
  instantiate: (guid: string, translation: [number, number, number]) => Promise<void>;
  /** 直接删除(工具面;UI 走 requestDelete/confirmDelete 提案流) */
  remove: (assetPath: string) => Promise<void>;
  reimport: (assetPath: string) => Promise<void>;
  move: (assetPath: string, destFolder: string) => Promise<void>;

  /** 导入到当前文件夹(sourcePaths = 绝对路径,桌面对话框选出) */
  importToHere: (sourcePaths: string[]) => Promise<void>;
  /** 引用查询(refs + referencedBy 双向,结果进 refsResult 浮层) */
  queryRefs: (assetPath: string) => Promise<void>;
  clearRefs: () => void;
  /** 删除提案:发起/取消/确认(确认走 asset_delete force=false;被引用阻断 → blocked 清单如实显示) */
  requestDelete: (assetPath: string) => void;
  cancelDelete: () => void;
  confirmDelete: () => Promise<void>;
  /** 懒加载贴图缩略图(原图直出;非贴图/失败记 'none' 不重复请求) */
  loadThumb: (item: AssetItem) => Promise<void>;
  /** F10:读 .meta semantic 段(检视器溯源显示;NO_META 等错误返回 null 不抛)。 */
  fetchSemantic: (assetPath: string) => Promise<AssetSemantic | null>;
  /** F10:写文字简介与标签(source=human;写后刷新列表)。 */
  setDescription: (assetPath: string, description: string, tags: string[]) => Promise<void>;
}

export const useAssetStore = create<AssetState>((set, get) => ({
  items: [],
  status: {},
  loading: false,
  error: null,
  viewMode: 'grid',
  typeFilter: 'all',
  search: '',
  currentFolder: '',
  selectedGuid: null,
  thumbs: {},
  refsResult: null,
  pendingDelete: null,

  load: async () => {
    set({ loading: true, error: null });
    // asset_list 为主:成功即显示资产;asset_build_status 失败(如 .rx 无 .meta 的 NO_META)
    // 不阻断列表——状态徽标留空 + 警告如实显示(不伪造全绿)。
    try {
      const list = await callAssetTool<{ assets: AssetItem[] }>('asset_list');
      let statusMap: Record<string, AssetBuildStatus['state']> = {};
      let warning: string | null = null;
      try {
        const statusList = await callAssetTool<{ items: AssetBuildStatus[] }>('asset_build_status', {});
        for (const s of statusList.items) statusMap[s.path] = s.state;
      } catch (err) {
        warning = `构建状态不可用: ${(err as Error).message}`;
      }
      set({ items: list.assets, status: statusMap, error: warning, loading: false });
    } catch (err) {
      set({ error: (err as Error).message, loading: false });
    }
  },

  setViewMode: (m) => set({ viewMode: m }),
  setTypeFilter: (t) => set({ typeFilter: t }),
  setSearch: (s) => set({ search: s }),
  setCurrentFolder: (f) => set({ currentFolder: f }),
  setSelectedGuid: (g) => set({ selectedGuid: g }),
  focusPath: (path) => {
    const norm = path.replace(/\\/g, '/');
    const parts = norm.split('/');
    const folder = parts.length > 1 ? parts[0] : '';
    const base = parts[parts.length - 1] ?? norm;
    set({ currentFolder: folder, search: base, typeFilter: 'all' });
  },

  instantiate: async (guid, translation) => {
    const item = get().items.find((i) => i.guid === guid);
    if (!item) throw new Error(`资产不存在: ${guid}`);
    if (item.type !== 'mesh' && item.type !== 'prefab') {
      throw new Error(`仅 mesh/prefab 可实例化,当前类型: ${item.type}`);
    }
    // 经 engine-scene 创建实体,MeshRenderer.mesh = 资产 GUID。
    await callTool('entity_create', {
      name: item.path.split('/').pop() ?? item.guid,
      translation,
      components: [
        { type: 'MeshRenderer', enabled: true, props: { mesh: guid, material: 'default' } },
      ],
    });
  },

  remove: async (assetPath) => {
    await callAssetTool('asset_delete', { assetPaths: [assetPath], force: false });
    await get().load();
  },

  reimport: async (assetPath) => {
    await callAssetTool('asset_reimport', { assetPaths: [assetPath] });
    await get().load();
  },

  move: async (assetPath, destFolder) => {
    await callAssetTool('asset_move', { assetPath, destFolder });
    await get().load();
  },

  importToHere: async (sourcePaths) => {
    if (sourcePaths.length === 0) return;
    await callAssetTool('asset_import', { sourcePaths, destFolder: get().currentFolder });
    await get().load();
  },

  queryRefs: async (assetPath) => {
    const toPath = (guid: string) =>
      get().items.find((i) => i.guid === guid)?.path ?? guid;
    interface EdgesResp {
      edges?: Array<{ from: string; to: string; type: string }>;
      error?: string;
      message?: string;
    }
    const [out, inbound] = await Promise.all([
      callAssetTool<EdgesResp>('asset_refs', { assetPath, direction: 'refs' }),
      callAssetTool<EdgesResp>('asset_refs', { assetPath, direction: 'referencedBy' }),
    ]);
    if (out.error) throw new Error(out.message ?? out.error);
    if (inbound.error) throw new Error(inbound.message ?? inbound.error);
    set({
      refsResult: {
        path: assetPath,
        refs: (out.edges ?? []).map((e) => `${toPath(e.to)} (${e.type})`),
        referencedBy: (inbound.edges ?? []).map((e) => `${toPath(e.from)} (${e.type})`),
      },
    });
  },

  clearRefs: () => set({ refsResult: null }),

  requestDelete: (assetPath) => set({ pendingDelete: { path: assetPath, blocked: null } }),
  cancelDelete: () => set({ pendingDelete: null }),

  confirmDelete: async () => {
    const pd = get().pendingDelete;
    if (!pd) return;
    interface DeleteResp {
      deleted?: string[];
      blockedByRefs?: Array<{ assetPath: string; referencedBy: string[] }> | string[];
      error?: string;
      message?: string;
    }
    const r = await callAssetTool<DeleteResp>('asset_delete', {
      assetPaths: [pd.path],
      force: false,
    });
    if (r.error) {
      set({ pendingDelete: { path: pd.path, blocked: [r.message ?? r.error] } });
      return;
    }
    const blockedRaw = r.blockedByRefs ?? [];
    if (blockedRaw.length > 0) {
      // 引用阻断:列出引用方(GUID 尽量解析为路径),提案条保持打开如实显示。
      const first = blockedRaw[0];
      const list =
        typeof first === 'string'
          ? (blockedRaw as string[])
          : (blockedRaw as Array<{ referencedBy: string[] }>).flatMap((b) =>
              b.referencedBy.map((g) => get().items.find((i) => i.guid === g)?.path ?? g),
            );
      set({ pendingDelete: { path: pd.path, blocked: list } });
      return;
    }
    set({ pendingDelete: null });
    await get().load();
  },

  loadThumb: async (item) => {
    if (item.type !== 'texture') return;
    if (get().thumbs[item.guid] !== undefined) return;
    try {
      const r = await callAssetTool<{ dataUrl?: string; error?: string }>('asset_thumbnail', {
        assetPath: item.path,
      });
      set((s) => ({ thumbs: { ...s.thumbs, [item.guid]: r.dataUrl ?? 'none' } }));
    } catch {
      set((s) => ({ thumbs: { ...s.thumbs, [item.guid]: 'none' } }));
    }
  },

  fetchSemantic: async (assetPath) => {
    try {
      const r = await callAssetTool<{ meta?: { semantic?: AssetSemantic }; error?: string }>(
        'asset_get_meta',
        { assetPath },
      );
      if (r.error) return null;
      return r.meta?.semantic ?? null;
    } catch {
      return null;
    }
  },

  setDescription: async (assetPath, description, tags) => {
    await callAssetTool('asset_set_description', {
      assetPath,
      description,
      tags,
      source: 'human',
    });
    await get().load();
  },
}));
