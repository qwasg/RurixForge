import { create } from 'zustand';
import { apiDeleteSkill, apiGet, apiPatch, apiPost, apiPut, ForgeApiError } from './forgeApi';
import { useToastStore } from './toastStore';

/**
 * F11 wave.5 skillStore(照 assetStore 形态:无 middleware、乐观更新 + 回滚 + toast、
 * 写操作后 await load())。接口契约见 11 §2.7,语义见 06 §2。
 *
 * 删除是 destructive:首次 DELETE 必回 409 GOV_PROPOSAL_REQUIRED + proposalId,
 * 存进 pendingDelete.proposalId 后由 UI 走「批准 → 重发 DELETE」两阶段(I-6)。
 * 加载失败一律如实进 error(带 HTTP 状态与错误码),绝不退化成空列表(I-5)。
 */

/** SKILL.md frontmatter(06 §1;存量文档只写 name/description 两键,其余可缺省)。 */
export interface SkillFront {
  name: string;
  description: string;
  version?: string | null;
  license?: string | null;
  tags: string[];
  allowedTools: string[];
}

/** 列表条目(GET /skills/list;dir = 所在目录,builtin=false 即来自 extraDirs)。 */
export interface SkillItem {
  name: string;
  description: string;
  enabled: boolean;
  version?: string | null;
  license?: string | null;
  tags: string[];
  allowedTools: string[];
  builtin: boolean;
  dir: string;
}

/** 详情(GET /skills/{name};content = SKILL.md 全文)。 */
export interface SkillDetail {
  name: string;
  content: string;
  front: SkillFront;
  builtin: boolean;
  path: string;
}

/** 校验结果(POST /skills/{name}:validate;errors 非空 = 拒收)。 */
export interface SkillValidation {
  valid: boolean;
  errors: string[];
  warnings: string[];
}

/** 待删除技能(proposalId 非空 = 后端已建提案,等批准)。 */
export interface PendingSkillDelete {
  name: string;
  proposalId: string | null;
}

interface SkillState {
  items: SkillItem[];
  loading: boolean;
  error: string | null;
  selected: string | null;
  detail: SkillDetail | null;
  detailLoading: boolean;
  draft: string;
  dirty: boolean;
  validation: SkillValidation | null;
  extraDirs: string[];
  pendingDelete: PendingSkillDelete | null;

  load: () => Promise<void>;
  select: (name: string | null) => Promise<void>;
  setDraft: (text: string) => void;
  save: () => Promise<void>;
  validate: () => Promise<void>;
  /** 失败时抛出(UI 据此保留内联输入行并就地提示,不吞错)。 */
  create: (name: string) => Promise<void>;
  toggleEnabled: (name: string, enabled: boolean) => Promise<void>;
  requestDelete: (name: string) => void;
  cancelDelete: () => void;
  confirmDelete: () => Promise<void>;
  approveAndDelete: () => Promise<void>;
  setExtraDirs: (dirs: string[]) => Promise<void>;
}

/** skill 名白名单(06 §1:小写英文 + 数字 + 中划线;同时是路径穿越防线)。 */
export const SKILL_NAME_RE = /^[a-z0-9-]+$/;

/** 错误文案:ForgeApiError 带出 HTTP 状态与错误码,不含糊。 */
export function skillErrorLabel(err: unknown, verb: string): string {
  if (err instanceof ForgeApiError) {
    return `${verb}失败(${err.status} ${err.code}):${err.message}`;
  }
  return `${verb}失败:${err instanceof Error ? err.message : String(err)}`;
}

function toast(kind: 'success' | 'error' | 'warning', msg: string): void {
  useToastStore.getState().push(kind, msg);
}

function strList(v: unknown): string[] {
  return Array.isArray(v) ? v.filter((x): x is string => typeof x === 'string') : [];
}

function normFront(raw: unknown): SkillFront {
  const f = (raw ?? {}) as Partial<SkillFront>;
  return {
    name: typeof f.name === 'string' ? f.name : '',
    description: typeof f.description === 'string' ? f.description : '',
    version: f.version ?? null,
    license: f.license ?? null,
    tags: strList(f.tags),
    allowedTools: strList(f.allowedTools),
  };
}

function normItem(raw: unknown): SkillItem {
  const s = (raw ?? {}) as Partial<SkillItem>;
  return {
    name: typeof s.name === 'string' ? s.name : '',
    description: typeof s.description === 'string' ? s.description : '',
    enabled: s.enabled !== false,
    version: s.version ?? null,
    license: s.license ?? null,
    tags: strList(s.tags),
    allowedTools: strList(s.allowedTools),
    builtin: s.builtin === true,
    dir: typeof s.dir === 'string' ? s.dir : '',
  };
}

function normDetail(raw: unknown): SkillDetail {
  const d = (raw ?? {}) as Partial<SkillDetail>;
  return {
    name: typeof d.name === 'string' ? d.name : '',
    content: typeof d.content === 'string' ? d.content : '',
    front: normFront(d.front),
    builtin: d.builtin === true,
    path: typeof d.path === 'string' ? d.path : '',
  };
}

function normValidation(raw: unknown): SkillValidation {
  const v = (raw ?? {}) as Partial<SkillValidation>;
  const errors = strList(v.errors);
  return { valid: v.valid === true && errors.length === 0, errors, warnings: strList(v.warnings) };
}

const skillPath = (name: string) => `/api/forge/skills/${encodeURIComponent(name)}`;

async function postValidate(name: string, content: string): Promise<SkillValidation> {
  return normValidation(await apiPost(`${skillPath(name)}:validate`, { content }));
}

export const useSkillStore = create<SkillState>((set, get) => ({
  items: [],
  loading: false,
  error: null,
  selected: null,
  detail: null,
  detailLoading: false,
  draft: '',
  dirty: false,
  validation: null,
  extraDirs: [],
  pendingDelete: null,

  load: async () => {
    set({ loading: true, error: null });
    try {
      const r = await apiGet<{ skills?: unknown[] }>('/api/forge/skills/list');
      set({ items: (r.skills ?? []).map(normItem), loading: false, error: null });
    } catch (err) {
      // 不伪造空列表:错误如实呈现,items 保持上一次的真实值。
      set({ loading: false, error: skillErrorLabel(err, '技能清单加载') });
      return;
    }
    // 契约无「读配置」端点:config/write 对缺省字段不改值,空体调用等价只读回显
    // (见 agentd skills.rs skills_config_write)。读不到不阻断列表,extraDirs 保持现值。
    try {
      const cfg = await apiPost<{ extraDirs?: string[] }>('/api/forge/skills/config/write', {});
      if (cfg.extraDirs !== undefined) set({ extraDirs: strList(cfg.extraDirs) });
    } catch {
      /* 目录配置不可用:留白不猜 */
    }
  },

  select: async (name) => {
    if (name === null) {
      set({
        selected: null,
        detail: null,
        detailLoading: false,
        draft: '',
        dirty: false,
        validation: null,
        pendingDelete: null,
      });
      return;
    }
    set({
      selected: name,
      detail: null,
      detailLoading: true,
      draft: '',
      dirty: false,
      validation: null,
      pendingDelete: null,
      error: null,
    });
    try {
      const d = normDetail(await apiGet(skillPath(name)));
      if (get().selected !== name) return; // 已切走,丢弃陈旧响应
      set({ detail: d, draft: d.content, dirty: false, detailLoading: false });
    } catch (err) {
      if (get().selected !== name) return;
      set({ detailLoading: false, error: skillErrorLabel(err, '技能详情加载') });
    }
  },

  setDraft: (text) => set({ draft: text, dirty: text !== (get().detail?.content ?? '') }),

  validate: async () => {
    const { selected, draft } = get();
    if (selected === null) return;
    try {
      set({ validation: await postValidate(selected, draft) });
    } catch (err) {
      set({ validation: null });
      toast('error', skillErrorLabel(err, '校验'));
    }
  },

  save: async () => {
    const { selected, draft } = get();
    if (selected === null) return;
    // 保存前自动校验:errors 非空即拦下,不静默落盘(I-5)。
    let v: SkillValidation;
    try {
      v = await postValidate(selected, draft);
    } catch (err) {
      toast('error', skillErrorLabel(err, '保存前校验'));
      return;
    }
    set({ validation: v });
    if (v.errors.length > 0) {
      toast('error', `校验未通过(${v.errors.length} 项错误),已阻止保存`);
      return;
    }
    try {
      await apiPut(skillPath(selected), { content: draft });
    } catch (err) {
      toast('error', skillErrorLabel(err, '保存'));
      return;
    }
    set((st) => ({ detail: st.detail ? { ...st.detail, content: draft } : st.detail, dirty: false }));
    toast('success', `已保存 ${selected}`);
    await get().load();
    // frontmatter 改动会换掉版本/标签 chips:静默重取详情(编辑器不重挂,光标不丢)。
    try {
      const fresh = normDetail(await apiGet(skillPath(selected)));
      if (get().selected === selected) set({ detail: fresh, dirty: get().draft !== fresh.content });
    } catch {
      /* 重取失败不影响已落盘的保存结果 */
    }
  },

  create: async (name) => {
    await apiPost('/api/forge/skills', { name });
    await get().load();
    await get().select(name);
    toast('success', `已新建技能 ${name}`);
  },

  toggleEnabled: async (name, enabled) => {
    const prev = get().items;
    const next = prev.map((i) => (i.name === name ? { ...i, enabled } : i));
    set({ items: next }); // 乐观
    try {
      await apiPost('/api/forge/skills/config/write', {
        disabled: next.filter((i) => !i.enabled).map((i) => i.name),
      });
    } catch (err) {
      set({ items: prev }); // 回滚
      toast('error', skillErrorLabel(err, '技能启停写回'));
      return;
    }
    await get().load();
  },

  requestDelete: (name) => set({ pendingDelete: { name, proposalId: null } }),
  cancelDelete: () => set({ pendingDelete: null }),

  confirmDelete: async () => {
    const pd = get().pendingDelete;
    if (!pd) return;
    try {
      const r = await apiDeleteSkill(pd.name);
      if ('proposalRequired' in r) {
        set({ pendingDelete: { name: pd.name, proposalId: r.proposalId } });
        toast('warning', `删除需提案确认:${r.proposalId}`);
        return;
      }
      set({ pendingDelete: null });
      toast('success', `已删除技能 ${pd.name}`);
      if (get().selected === pd.name) await get().select(null);
      await get().load();
    } catch (err) {
      // 带 proposalId 的 409 已被 apiDeleteSkill 收进返回值;这里兜「409 却没给提案号」的形态。
      if (err instanceof ForgeApiError && err.code === 'GOV_PROPOSAL_REQUIRED') {
        toast('error', `删除需提案确认,但响应未带 proposalId:${err.message}`);
        return;
      }
      toast('error', skillErrorLabel(err, '删除'));
    }
  },

  approveAndDelete: async () => {
    const pd = get().pendingDelete;
    if (!pd || pd.proposalId === null) return;
    try {
      await apiPatch(`/api/forge/proposals/${encodeURIComponent(pd.proposalId)}`, {
        action: 'approve',
      });
    } catch (err) {
      toast('error', skillErrorLabel(err, '提案批准'));
      return;
    }
    await get().confirmDelete(); // 批准后重发 DELETE
  },

  setExtraDirs: async (dirs) => {
    const prev = get().extraDirs;
    set({ extraDirs: dirs }); // 乐观
    try {
      const r = await apiPost<{ extraDirs?: string[] }>('/api/forge/skills/config/write', {
        extraDirs: dirs,
      });
      if (r.extraDirs !== undefined) set({ extraDirs: strList(r.extraDirs) });
    } catch (err) {
      set({ extraDirs: prev }); // 回滚
      toast('error', skillErrorLabel(err, '技能目录写回'));
      return;
    }
    await get().load();
  },
}));
