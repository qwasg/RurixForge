/**
 * genStore(F5 wave.3):生成后端配置状态 + Assets 生成链(GenerateDialog/CandidatesModal)。
 * 数据源:GET /api/forge/gen/backends(agentd REST,经 host 代理);
 * 生成/入库:mcp__gen-image__gen_image / gen_accept(候选 dataUrl 直接来自工具响应,R-5 不涉密钥)。
 */
import { create } from 'zustand';
import { apiGet, callGenTool, ForgeApiError } from './forgeApi';
import { useAssetStore } from './assetStore';

/** 后端清单条目(与 agentd GET /api/forge/gen/backends 对齐;密钥值永不在此面)。 */
export interface GenBackendInfo {
  id: string;
  kind: string;
  configured: boolean;
  endpointSet: boolean;
  capabilities?: Record<string, unknown>;
}

/** 生成候选(gen_image 响应超集:dataUrl = base64 PNG 缩略图)。 */
export interface GenCandidate {
  imageFileRef: string;
  seed: number;
  backendId: string;
  dataUrl?: string;
}

export interface GenImageParams {
  prompt: string;
  negativePrompt?: string;
  size: 256 | 512 | 1024;
  n: number;
  backend?: string;
}

/** prompt + seed → 入库文件名(ascii slug;与 gen-image-mcp slugify 同规则)。 */
export function slugifyName(prompt: string, seed: number): string {
  const slug =
    prompt
      .toLowerCase()
      .replace(/[^a-z0-9]+/g, '-')
      .replace(/^-+/, '')
      .replace(/-+$/, '')
      .slice(0, 24)
      .replace(/-+$/, '') || 'gen';
  return `${slug}-${seed}`;
}

interface GenState {
  backends: GenBackendInfo[];
  backendsLoaded: boolean;
  backendsError: string | null;
  /** GenerateDialog 开关 + 目标文件夹(gen_accept destFolder;'' 已归一为 Textures)。 */
  dialogOpen: boolean;
  destFolder: string;
  /** CandidatesModal 候选(null = 关闭);全部候选可逐个 accept(不互斥)。 */
  candidates: GenCandidate[] | null;
  lastPrompt: string;
  /** 已入库候选 imageFileRef(卡片标「已入库」,不阻止重复点)。 */
  acceptedRefs: string[];
  busy: boolean;
  acceptBusy: string | null;
  lastError: string | null;
  lastErrorCode: string | null;

  loadBackends: () => Promise<void>;
  openDialog: (destFolder: string) => void;
  closeDialog: () => void;
  closeCandidates: () => void;
  /** 提交 gen_image;成功 → 关对话框开候选 modal;失败 → lastError 如实条(码保留)。 */
  generate: (params: GenImageParams) => Promise<void>;
  /** gen_accept 单候选入管线 → assetStore 刷新 + 选中新资产。 */
  accept: (c: GenCandidate) => Promise<void>;
}

export const useGenStore = create<GenState>((set, get) => ({
  backends: [],
  backendsLoaded: false,
  backendsError: null,
  dialogOpen: false,
  destFolder: 'Textures',
  candidates: null,
  lastPrompt: '',
  acceptedRefs: [],
  busy: false,
  acceptBusy: null,
  lastError: null,
  lastErrorCode: null,

  loadBackends: async () => {
    try {
      const r = await apiGet<{ backends: GenBackendInfo[] }>('/api/forge/gen/backends');
      set({ backends: r.backends, backendsLoaded: true, backendsError: null });
    } catch (err) {
      set({ backendsError: (err as Error).message, backendsLoaded: true });
    }
  },

  openDialog: (destFolder) => {
    set({ dialogOpen: true, destFolder: destFolder || 'Textures', lastError: null, lastErrorCode: null });
    if (!get().backendsLoaded) void get().loadBackends();
  },
  closeDialog: () => set({ dialogOpen: false }),
  closeCandidates: () => set({ candidates: null, acceptedRefs: [] }),

  generate: async (params) => {
    set({ busy: true, lastError: null, lastErrorCode: null, lastPrompt: params.prompt });
    try {
      const args: Record<string, unknown> = {
        prompt: params.prompt,
        size: params.size,
        n: params.n,
      };
      if (params.negativePrompt?.trim()) args.negativePrompt = params.negativePrompt.trim();
      if (params.backend) args.backend = params.backend;
      const r = await callGenTool<{ candidates: GenCandidate[] }>('gen_image', args);
      set({ busy: false, dialogOpen: false, candidates: r.candidates, acceptedRefs: [] });
    } catch (err) {
      const code = err instanceof ForgeApiError ? err.code : null;
      set({ busy: false, lastError: (err as Error).message, lastErrorCode: code });
    }
  },

  accept: async (c) => {
    set({ acceptBusy: c.imageFileRef, lastError: null, lastErrorCode: null });
    try {
      const r = await callGenTool<{ assetPath: string; guid: string }>('gen_accept', {
        imageFileRef: c.imageFileRef,
        destFolder: get().destFolder,
        name: slugifyName(get().lastPrompt, c.seed),
      });
      set((s) => ({ acceptBusy: null, acceptedRefs: [...s.acceptedRefs, c.imageFileRef] }));
      const assets = useAssetStore.getState();
      await assets.load();
      useAssetStore.getState().setSelectedGuid(r.guid);
    } catch (err) {
      const code = err instanceof ForgeApiError ? err.code : null;
      set({ acceptBusy: null, lastError: (err as Error).message, lastErrorCode: code });
    }
  },
}));

/** configured=true 的后端(GenerateDialog 下拉数据源)。 */
export function configuredBackends(backends: GenBackendInfo[]): GenBackendInfo[] {
  return backends.filter((b) => b.configured);
}
