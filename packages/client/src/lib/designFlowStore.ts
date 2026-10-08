import { create } from 'zustand';
import { ForgeApiError, apiGet } from './forgeApi';
import { useSessionStore } from './sessionStore';
import { useToastStore } from './toastStore';

/**
 * D-045:Design 流程(设计稿 → 审阅 → 原子级复刻)的前端镜像(wire 契约见 11 E-11-008)。
 *
 * 命名避开既有的 designBoardStore(画板设计,与本流程无关)。
 * 流程本体是后端按会话持久化的阶段机(DebugSession.design);state 只来自三处:
 * 快照 activeSession.design(hydrate)、GET …/design 重拉(refresh,由实时 design.* /
 * session.updated 触发)、本 store 自己的 REST 动作。回放的历史事件不改这里(只建卡片)。
 * 卡片动作(采用 / 修改 / 重出 / 继续 / 修复)经 act() 直接 POST ask:execute,带 {id, rev, action}。
 */

export const DESIGN_STAGES = ['concept', 'design_review', 'replication', 'done'] as const;
export type DesignStage = (typeof DESIGN_STAGES)[number];
export type DesignPhase = 'waiting' | 'running' | 'failed';
export type DesignAction =
  | 'approve_design'
  | 'revise_design'
  | 'regenerate_design'
  | 'resume_replication'
  | 'fix_replication';

export interface DesignError {
  code: string;
  message: string;
}

export interface DesignApproved {
  rev: number;
  candidate: number;
  sha256: string;
  width: number;
  height: number;
  assetPath: string | null;
  guid: string | null;
}

export interface DesignVerifySummary {
  n: number;
  passed: boolean;
  globalSsim: number;
  failedElements: number;
}

export interface DesignState {
  id: string;
  slug: string;
  dir: string;
  title: string;
  workspaceId: string | null;
  stage: DesignStage;
  phase: DesignPhase;
  running: string | null;
  lastError: DesignError | null;
  designRev: number;
  candidates: number[];
  selected: number | null;
  designType: string | null;
  aspect: string | null;
  approved: DesignApproved | null;
  replicationRound: number;
  layoutReady: boolean;
  assetsReady: boolean;
  scenePath: string | null;
  verifyCount: number;
  lastVerify: DesignVerifySummary | null;
  passed: boolean | null;
}

export interface DesignFace {
  review: Record<string, unknown> | null;
  layout: Record<string, unknown> | null;
  assets: Record<string, unknown> | null;
  verify: Record<string, unknown> | null;
  result: Record<string, unknown> | null;
}

export type DesignActResult = { ok: true } | { ok: false; code: string; message: string };

// ---------- 归一 ----------

function asRecord(raw: unknown): Record<string, unknown> | null {
  return raw !== null && typeof raw === 'object' && !Array.isArray(raw) ? (raw as Record<string, unknown>) : null;
}

const num = (v: unknown): number => (typeof v === 'number' && Number.isFinite(v) ? v : 0);
const strOrNull = (v: unknown): string | null => (typeof v === 'string' ? v : null);

export function normalizeDesignState(raw: unknown): DesignState | null {
  const rec = asRecord(raw);
  if (!rec || typeof rec.id !== 'string' || rec.id === '') return null;
  const stage = DESIGN_STAGES.includes(rec.stage as DesignStage) ? (rec.stage as DesignStage) : 'concept';
  const phase: DesignPhase = rec.phase === 'running' || rec.phase === 'failed' ? rec.phase : 'waiting';
  const err = asRecord(rec.lastError);
  const ap = asRecord(rec.approved);
  const lv = asRecord(rec.lastVerify);
  return {
    id: rec.id,
    slug: typeof rec.slug === 'string' ? rec.slug : '',
    dir: typeof rec.dir === 'string' ? rec.dir : '',
    title: typeof rec.title === 'string' ? rec.title : '',
    workspaceId: strOrNull(rec.workspaceId),
    stage,
    phase,
    running: strOrNull(rec.running),
    lastError: err && typeof err.code === 'string' ? { code: err.code, message: String(err.message ?? '') } : null,
    designRev: num(rec.designRev),
    candidates: Array.isArray(rec.candidates) ? rec.candidates.filter((c): c is number => typeof c === 'number') : [],
    selected: typeof rec.selected === 'number' ? rec.selected : null,
    designType: strOrNull(rec.designType),
    aspect: strOrNull(rec.aspect),
    approved: ap
      ? {
          rev: num(ap.rev),
          candidate: num(ap.candidate),
          sha256: String(ap.sha256 ?? ''),
          width: num(ap.width),
          height: num(ap.height),
          assetPath: strOrNull(ap.assetPath),
          guid: strOrNull(ap.guid),
        }
      : null,
    replicationRound: num(rec.replicationRound),
    layoutReady: rec.layoutReady === true,
    assetsReady: rec.assetsReady === true,
    scenePath: strOrNull(rec.scenePath),
    verifyCount: num(rec.verifyCount),
    lastVerify: lv
      ? { n: num(lv.n), passed: lv.passed === true, globalSsim: num(lv.globalSsim), failedElements: num(lv.failedElements) }
      : null,
    passed: typeof rec.passed === 'boolean' ? rec.passed : null,
  };
}

// ---------- 展示文案 ----------

const STAGE_LABELS: Record<DesignStage, string> = {
  concept: '构思出图',
  design_review: '审阅设计稿',
  replication: '原子级复刻',
  done: '已完成',
};

export function designStageLabel(stage: string | null | undefined): string {
  return STAGE_LABELS[stage as DesignStage] ?? '未知阶段';
}

export function designStageIndex(stage: string | null | undefined): number {
  return DESIGN_STAGES.indexOf(stage as DesignStage);
}

const RUNNING_TEXT: Record<string, string> = {
  concept: '正在构思并生成设计稿…',
  revise: '正在按意见修改设计稿…',
  regenerate: '正在重新生成设计稿…',
  replication: '正在引擎里复刻设计稿…',
};

export function designPhaseText(d: DesignState | null): string {
  if (!d) return '';
  if (d.phase === 'running') return RUNNING_TEXT[d.running ?? ''] ?? '正在处理…';
  if (d.phase === 'failed') return `上一轮失败${d.lastError ? `:${d.lastError.message}` : ''}`;
  switch (d.stage) {
    case 'concept':
      return '等待补充说明后重试';
    case 'design_review':
      return '等你挑选设计稿:采用、修改或重新生成';
    case 'replication':
      return '复刻未收尾,可继续';
    case 'done':
      return d.passed === false ? '已收尾(验收有未通过项)' : '复刻完成';
  }
}

/** 流程目录内文件 → 可直接给 <img src> 的 URL(后端只出流程目录内 png/json)。 */
export function designFileUrl(sessionId: string, path: string): string {
  return `/api/forge/sessions/${encodeURIComponent(sessionId)}/design/file?path=${encodeURIComponent(path)}`;
}

// ---------- store ----------

interface DesignFlowStore {
  sessionId: string | null;
  state: DesignState | null;
  face: DesignFace;
  pending: { action: DesignAction | 'select' | 'restart' } | null;
  lastActionError: DesignError | null;
  hydrate: (raw: unknown, sessionId: string | null) => void;
  refresh: (sessionId?: string | null) => Promise<void>;
  applyLiveEvent: (evt: { type: string; sessionId?: string; payload?: Record<string, unknown> }) => void;
  act: (
    action: DesignAction,
    opts?: { id?: string; rev?: number; candidate?: number; userInput?: string },
  ) => Promise<DesignActResult>;
  select: (candidate: number, target?: { id: string; rev: number }) => Promise<DesignActResult>;
  restart: () => Promise<DesignActResult>;
  reset: () => void;
}

const EMPTY_FACE: DesignFace = { review: null, layout: null, assets: null, verify: null, result: null };

function activeSessionId(): string | null {
  return useSessionStore.getState().activeSessionId;
}

function sessionPath(sessionId: string, tail: string): string {
  return `/api/forge/sessions/${encodeURIComponent(sessionId)}/${tail}`;
}

async function postJson(path: string, payload: unknown): Promise<DesignActResult & { body?: unknown }> {
  try {
    const res = await fetch(path, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify(payload ?? {}),
    });
    let body: unknown = null;
    try {
      body = await res.json();
    } catch {
      body = null;
    }
    if (!res.ok) {
      const err = asRecord(asRecord(body)?.error);
      return {
        ok: false,
        code: typeof err?.code === 'string' ? err.code : `HTTP_${res.status}`,
        message: typeof err?.message === 'string' ? err.message : `HTTP ${res.status}`,
      };
    }
    return { ok: true, body };
  } catch (err) {
    return { ok: false, code: 'NETWORK', message: `请求失败: ${err instanceof Error ? err.message : String(err)}` };
  }
}

/** 动作被拒的人话(409 = 状态已变:提示刷新)。 */
export function designActionErrorText(code: string, message: string): string {
  if (code === 'DESIGN_STAGE_MISMATCH') return `流程状态已变化,已刷新:${message}`;
  if (code === 'SESSION_BUSY') return '有任务正在运行,请等它结束';
  if (code === 'DESIGN_VISION_REQUIRED' || code === 'DESIGN_NEEDS_WRITE') return message;
  return message || code;
}

let refreshEpoch = 0;

export const useDesignFlowStore = create<DesignFlowStore>((set, get) => ({
  sessionId: null,
  state: null,
  face: EMPTY_FACE,
  pending: null,
  lastActionError: null,

  hydrate: (raw, sessionId) => {
    set({ state: normalizeDesignState(raw), sessionId, face: EMPTY_FACE, pending: null, lastActionError: null });
  },

  refresh: async (sid) => {
    const sessionId = sid ?? get().sessionId ?? activeSessionId();
    if (!sessionId) return;
    const epoch = ++refreshEpoch;
    try {
      const body = await apiGet<Record<string, unknown>>(sessionPath(sessionId, 'design'));
      if (epoch !== refreshEpoch) return;
      set({
        sessionId,
        state: normalizeDesignState(body.design),
        face: {
          review: asRecord(body.review),
          layout: asRecord(body.layout),
          assets: asRecord(body.assets),
          verify: asRecord(body.verify),
          result: asRecord(body.result),
        },
      });
    } catch (err) {
      // 旧后端没有该路由 / 会话已删:保持现状,不把「取不到」当成「没有流程」。
      if (!(err instanceof ForgeApiError)) return;
    }
  },

  applyLiveEvent: (evt) => {
    const sid = evt.sessionId || activeSessionId();
    if (evt.type === 'design.stage' && evt.payload?.phase === 'running') set({ pending: null });
    void get().refresh(sid);
  },

  act: async (action, opts = {}) => {
    const sessionId = activeSessionId();
    const state = get().state;
    if (!sessionId) return { ok: false, code: 'NO_SESSION', message: '请先选择会话' };
    if (!state) return { ok: false, code: 'NO_FLOW', message: '当前会话没有 Design 流程' };
    if (get().pending) return { ok: false, code: 'PENDING', message: '上一个操作还在提交中' };
    const userInput = (opts.userInput ?? '').trim();
    if ((action === 'revise_design' || action === 'fix_replication') && userInput === '') {
      return { ok: false, code: 'INVALID_INPUT', message: '请先填写意见' };
    }
    const rev =
      opts.rev ??
      (action === 'resume_replication' || action === 'fix_replication' ? state.replicationRound : state.designRev);
    set({ pending: { action }, lastActionError: null });
    const res = await postJson(sessionPath(sessionId, 'ask:execute'), {
      userInput,
      mode: 'design',
      design: {
        id: opts.id ?? state.id,
        rev,
        action,
        ...(opts.candidate !== undefined ? { candidate: opts.candidate } : {}),
      },
    });
    set({ pending: null });
    if (!res.ok) {
      set({ lastActionError: { code: res.code, message: res.message } });
      useToastStore.getState().push('warning', designActionErrorText(res.code, res.message));
      void get().refresh(sessionId);
      return { ok: false, code: res.code, message: res.message };
    }
    void get().refresh(sessionId);
    return { ok: true };
  },

  select: async (candidate, target) => {
    const sessionId = activeSessionId();
    const state = get().state;
    if (!sessionId || !state) return { ok: false, code: 'NO_FLOW', message: '当前会话没有 Design 流程' };
    // 乐观更新:选中环立刻跟手;被拒再按后端状态回滚。
    set({ state: { ...state, selected: candidate } });
    const res = await postJson(sessionPath(sessionId, 'design/select'), {
      id: target?.id ?? state.id,
      rev: target?.rev ?? state.designRev,
      candidate,
    });
    if (!res.ok) {
      void get().refresh(sessionId);
      return { ok: false, code: res.code, message: res.message };
    }
    return { ok: true };
  },

  restart: async () => {
    const sessionId = activeSessionId();
    if (!sessionId) return { ok: false, code: 'NO_SESSION', message: '请先选择会话' };
    set({ pending: { action: 'restart' } });
    const res = await postJson(sessionPath(sessionId, 'design/restart'), {});
    set({ pending: null });
    if (!res.ok) {
      useToastStore.getState().push('warning', designActionErrorText(res.code, res.message));
      return { ok: false, code: res.code, message: res.message };
    }
    set({ state: null, face: EMPTY_FACE });
    return { ok: true };
  },

  reset: () => set({ sessionId: null, state: null, face: EMPTY_FACE, pending: null, lastActionError: null }),
}));
