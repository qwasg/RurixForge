import { create } from 'zustand';
import { useSessionStore } from './sessionStore';
import { useToastStore } from './toastStore';

/**
 * M3 Feature 6: WorldBuild (Build Composer Mode) 前端状态面。
 * 对应后端 DebugSession.world_build 状态机与 WebUI 3D Preview 交互。
 */

export interface WorldBuildBuilding {
  id: string;
  name: string;
  type: string;
  transform?: {
    position: [number, number, number];
    rotation: [number, number, number];
    scale: [number, number, number];
  };
  bounds?: {
    min: [number, number, number];
    max: [number, number, number];
    dimensions?: [number, number, number];
  };
  materials?: string[];
  interiorPropsCount?: number;
  prompt?: string;
  parameters?: Record<string, unknown>;
  status?: string;
  [key: string]: unknown;
}

export interface BuildPreviewSceneState {
  slug: string;
  rev: number;
  buildings: WorldBuildBuilding[];
  selectedBuildingId?: string | null;
  stylePreset?: string;
  cameraViews?: string[];
  environment?: {
    skybox?: string;
    fog?: boolean;
    ambientIntensity?: number;
  };
  [key: string]: unknown;
}

export type WorldBuildStage =
  | 'survey'
  | 'preview_review'
  | 'construction'
  | 'coherence_verify'
  | 'done';

export type WorldBuildPhase = 'waiting' | 'running' | 'failed';

export interface WorldBuildFlowError {
  code: string;
  message: string;
}

export interface WorldBuildState {
  id: string;
  slug: string;
  dir: string;
  title: string;
  token?: string;
  stage: WorldBuildStage;
  phase: WorldBuildPhase;
  running?: string | null;
  lastError?: WorldBuildFlowError | null;
  previewRev: number;
  selectedBuilding?: string | null;
  stylePreset?: string | null;
  approvedPreview?: {
    rev: number;
    buildingCount: number;
    style: string;
    approvedAt: string;
  } | null;
  buildings?: WorldBuildBuilding[];
  [key: string]: unknown;
}

export interface ActResult {
  ok: boolean;
  code?: string;
  message?: string;
  details?: Record<string, unknown> | null;
}

export interface ActOptions {
  id?: string;
  rev?: number;
  buildingId?: string;
  instruction?: string;
  parameters?: Record<string, unknown>;
  stylePreset?: string;
}

function asRecord(raw: unknown): Record<string, unknown> | null {
  return raw !== null && typeof raw === 'object' && !Array.isArray(raw)
    ? (raw as Record<string, unknown>)
    : null;
}

function activeSessionId(): string | null {
  return useSessionStore.getState().activeSessionId;
}

function sessionPath(sessionId: string, tail: string): string {
  return `/api/forge/sessions/${encodeURIComponent(sessionId)}/${tail}`;
}

async function postJson<T>(
  path: string,
  body: unknown,
): Promise<
  | { ok: true; body: T }
  | { ok: false; status: number; code: string; message: string; details: Record<string, unknown> | null }
> {
  let res: Response;
  try {
    res = await fetch(path, {
      method: 'POST',
      headers: { credentials: 'omit', 'Content-Type': 'application/json' },
      body: JSON.stringify(body),
    });
  } catch (err) {
    return {
      ok: false,
      status: 0,
      code: 'NETWORK',
      message: `网络请求失败: ${err instanceof Error ? err.message : String(err)}`,
      details: null,
    };
  }
  let parsed: unknown = null;
  try {
    parsed = await res.json();
  } catch {
    parsed = null;
  }
  if (!res.ok) {
    const err = asRecord(asRecord(parsed)?.error);
    return {
      ok: false,
      status: res.status,
      code: typeof err?.code === 'string' ? err.code : `HTTP_${res.status}`,
      message: typeof err?.message === 'string' ? err.message : `HTTP ${res.status}`,
      details: asRecord(err?.details),
    };
  }
  return { ok: true, body: parsed as T };
}

export function previewUrl(
  slug: string,
  token?: string,
  port?: number,
  rev?: number,
  locationHostname?: string,
): string {
  const t = token && token.trim() !== '' ? token.trim() : slug;
  const versionParam = typeof rev === 'number' ? `?v=${rev}` : '';
  if (port && port > 0) {
    const host = locationHostname === '127.0.0.1' ? 'localhost' : '127.0.0.1';
    return `http://${host}:${port}/u/${t}/${versionParam}`;
  }
  return `/u/${t}/${versionParam}`;
}

export interface WorldBuildStoreState {
  sessionId: string | null;
  state: WorldBuildState | null;
  sceneState: BuildPreviewSceneState | null;
  selectedBuildingId: string | null;
  pending: { action: string; rev: number | null } | null;
  lastActionError: { code: string; message: string; details?: Record<string, unknown> | null } | null;

  hydrate: (raw: unknown, sessionId?: string | null) => void;
  refresh: (sessionId?: string | null) => Promise<void>;
  selectBuilding: (id: string | null) => void;
  setSceneState: (sceneState: BuildPreviewSceneState | null) => void;
  setStylePreset: (preset: string) => void;

  act: (action: string, opts?: ActOptions) => Promise<ActResult>;
  rebuildBuilding: (buildingId: string, instruction: string, opts?: ActOptions) => Promise<ActResult>;
  tuneBuilding: (buildingId: string, instruction?: string, parameters?: Record<string, unknown>, opts?: ActOptions) => Promise<ActResult>;
  adjustStyle: (stylePreset: string, opts?: ActOptions) => Promise<ActResult>;
  approvePreview: (opts?: ActOptions) => Promise<ActResult>;
  reset: () => void;
}

export const useWorldBuildStore = create<WorldBuildStoreState>((set, get) => ({
  sessionId: null,
  state: null,
  sceneState: null,
  selectedBuildingId: null,
  pending: null,
  lastActionError: null,

  hydrate: (raw: unknown, sessionId?: string | null) => {
    const rec = asRecord(raw);
    if (!rec || typeof rec.id !== 'string') {
      set({ state: null });
      return;
    }
    const state: WorldBuildState = {
      id: rec.id as string,
      slug: (rec.slug as string) || (rec.id as string),
      dir: (rec.dir as string) || '',
      title: (rec.title as string) || '',
      token: (rec.token as string) || undefined,
      stage: (rec.stage as WorldBuildStage) || 'survey',
      phase: (rec.phase as WorldBuildPhase) || 'waiting',
      running: (rec.running as string) || null,
      lastError: asRecord(rec.lastError) as WorldBuildFlowError | null,
      previewRev: typeof rec.previewRev === 'number' ? rec.previewRev : 0,
      selectedBuilding: (rec.selectedBuilding as string) || null,
      stylePreset: (rec.stylePreset as string) || null,
      approvedPreview: asRecord(rec.approvedPreview) as WorldBuildState['approvedPreview'],
      buildings: Array.isArray(rec.buildings) ? (rec.buildings as WorldBuildBuilding[]) : undefined,
    };
    set({
      sessionId: sessionId ?? activeSessionId(),
      state,
      selectedBuildingId: state.selectedBuilding ?? get().selectedBuildingId,
    });
  },

  refresh: async (sessionId?: string | null) => {
    const sid = sessionId ?? activeSessionId();
    if (!sid) return;
    try {
      const res = await fetch(sessionPath(sid, 'world_build'), { credentials: 'omit' });
      if (!res.ok) return;
      const json = await res.json();
      get().hydrate(json?.worldBuild ?? json, sid);
    } catch {
      // 静默处理重拉错误
    }
  },

  selectBuilding: (id: string | null) => {
    set({ selectedBuildingId: id });
    const { sceneState } = get();
    if (sceneState) {
      set({ sceneState: { ...sceneState, selectedBuildingId: id } });
    }
  },

  setSceneState: (sceneState: BuildPreviewSceneState | null) => {
    set({
      sceneState,
      selectedBuildingId: sceneState?.selectedBuildingId ?? get().selectedBuildingId,
    });
  },

  setStylePreset: (preset: string) => {
    const { sceneState, state } = get();
    if (sceneState) {
      set({ sceneState: { ...sceneState, stylePreset: preset } });
    }
    if (state) {
      set({ state: { ...state, stylePreset: preset } });
    }
  },

  act: async (action: string, opts: ActOptions = {}) => {
    const sessionId = activeSessionId();
    const state = get().state;
    if (!sessionId) {
      return { ok: false, code: 'NO_SESSION', message: '请先选择会话' };
    }
    if (!state) {
      return { ok: false, code: 'NO_FLOW', message: '当前会话没有进行中的场景搭建流程' };
    }
    if (get().pending) {
      return { ok: false, code: 'PENDING', message: '上一个操作仍在提交中，请稍候' };
    }

    const id = opts.id ?? state.id;
    const rev = opts.rev ?? state.previewRev;
    const instruction = opts.instruction ?? '';

    set({ pending: { action, rev }, lastActionError: null });

    const payload = {
      userInput: instruction,
      mode: 'world_build',
      worldBuild: {
        id,
        rev,
        action,
        ...(opts.buildingId ? { buildingId: opts.buildingId } : {}),
        ...(opts.instruction ? { instruction: opts.instruction } : {}),
        ...(opts.parameters ? { parameters: opts.parameters } : {}),
        ...(opts.stylePreset ? { stylePreset: opts.stylePreset } : {}),
      },
    };

    const result = await postJson<unknown>(sessionPath(sessionId, 'ask:execute'), payload);
    set({ pending: null });

    if (result.ok) {
      return { ok: true };
    }

    const fail: ActResult = {
      ok: false,
      code: result.code,
      message: result.message,
      details: result.details,
    };
    set({ lastActionError: { code: result.code, message: result.message, details: result.details } });
    useToastStore.getState().push('warning', result.message || '操作未执行成功');
    return fail;
  },

  rebuildBuilding: (buildingId: string, instruction: string, opts: ActOptions = {}) => {
    return get().act('rebuild_building', { ...opts, buildingId, instruction });
  },

  tuneBuilding: (buildingId: string, instruction?: string, parameters?: Record<string, unknown>, opts: ActOptions = {}) => {
    return get().act('tune_building', { ...opts, buildingId, instruction, parameters });
  },

  adjustStyle: (stylePreset: string, opts: ActOptions = {}) => {
    return get().act('adjust_style', { ...opts, stylePreset });
  },

  approvePreview: (opts: ActOptions = {}) => {
    return get().act('approve_preview', opts);
  },

  reset: () => {
    set({
      sessionId: null,
      state: null,
      sceneState: null,
      selectedBuildingId: null,
      pending: null,
      lastActionError: null,
    });
  },
}));
