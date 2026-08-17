import { create } from 'zustand';
import { callTool } from './forgeApi';

/** 编辑器状态:实体 / 选中 / PIE / 事件流 / 帧统计,action 全部真实打后端。 */

export interface TransformData {
  translation: number[];
  rotation: number[];
  scale: number[];
}

export interface ComponentData {
  type: string;
  enabled: boolean;
  props: Record<string, unknown>;
}

export interface EntityData {
  id: number;
  name: string;
  transform: TransformData;
  components: ComponentData[];
}

export type PlayState = 'edit' | 'play_running' | 'play_paused';

export interface RenderStats {
  frames: number;
  lastTris: number;
  lastNonZeroPixels: number;
}

export interface ComponentTypeInfo {
  name: string;
  fields: Array<{ name: string; type: string }>;
}

/** host_events 返回的 jsonl 行(字段随事件类型变化) */
export type HostEvent = Record<string, unknown>;

export interface ChatMessage {
  role: 'user' | 'assistant' | 'error';
  text: string;
}

export type CenterTab = 'viewport' | 'nodegraph';
export type WorkbenchTab = 'console' | 'problems' | 'output' | 'terminal' | 'logs' | 'metrics';
export type GizmoMode = 'translate' | 'rotate' | 'scale';

/** 编辑器相机(与服务端 EditorCamera 字段一一对应) */
export interface CameraData {
  target: number[];
  yaw: number;
  pitch: number;
  dist: number;
  fovY: number;
}

/** viewport_frame 诊断面 */
export interface ViewportInfo {
  deviceName: string;
  draws: number;
  frames: number;
  nonZeroPixels: number;
  truncated: boolean;
}

export interface PickResult {
  hit: boolean;
  entityId?: number;
  name?: string;
  point?: number[];
}

/** 相机基(与服务端 viewport.rs EditorCamera 同公式:eye = target + dist·[cp·sy, sp, cp·cy]) */
function cameraBasis(c: CameraData): { right: number[]; up: number[] } {
  const yaw = (c.yaw * Math.PI) / 180;
  const pitch = (c.pitch * Math.PI) / 180;
  const [sp, cp] = [Math.sin(pitch), Math.cos(pitch)];
  const [sy, cy] = [Math.sin(yaw), Math.cos(yaw)];
  const eye = [c.target[0] + c.dist * cp * sy, c.target[1] + c.dist * sp, c.target[2] + c.dist * cp * cy];
  const f0 = [c.target[0] - eye[0], c.target[1] - eye[1], c.target[2] - eye[2]];
  const fl = Math.hypot(f0[0], f0[1], f0[2]) || 1;
  const f = [f0[0] / fl, f0[1] / fl, f0[2] / fl];
  // right = norm(cross(f, [0,1,0])) = norm([-f2, 0, f0])(与服务端 v3_cross(f, up) 逐字一致)
  const rl = Math.hypot(f[2], f[0]) || 1;
  const right = [-f[2] / rl, 0, f[0] / rl];
  const up = [
    right[1] * f[2] - right[2] * f[1],
    right[2] * f[0] - right[0] * f[2],
    right[0] * f[1] - right[1] * f[0],
  ];
  return { right, up };
}

/** 四元数乘法 a⊗b([x,y,z,w]) */
function quatMul(a: number[], b: number[]): number[] {
  return [
    a[3] * b[0] + a[0] * b[3] + a[1] * b[2] - a[2] * b[1],
    a[3] * b[1] - a[0] * b[2] + a[1] * b[3] + a[2] * b[0],
    a[3] * b[2] + a[0] * b[1] - a[1] * b[0] + a[2] * b[3],
    a[3] * b[3] - a[0] * b[0] - a[1] * b[1] - a[2] * b[2],
  ];
}

interface SceneSummary {
  name: string;
  entityCount: number;
  playState: PlayState;
  render: RenderStats;
}

interface EditorState {
  entities: EntityData[];
  selectedId: number | null;
  playState: PlayState;
  events: HostEvent[];
  stats: RenderStats | null;
  sceneName: string;
  /** 最近一次 scene_save 落盘路径,供 scene_load 复用 */
  scenePath: string | null;
  componentTypes: ComponentTypeInfo[];
  lastError: string | null;

  gizmo: GizmoMode;
  centerTab: CenterTab;
  workbenchTab: WorkbenchTab;
  chatOpen: boolean;
  chatMessages: ChatMessage[];
  /** F2 wave.3:Assets 右键「生成」预填 seam(F3 gen-image/gen-model 接入前仅预填文案) */
  chatPrefill: string | null;

  /** 编辑器相机(null = 未拉取) */
  camera: CameraData | null;
  /** 视口降级原因(null = 正常;DEV_ENV_DEGRADE 等如实显示,不伪造帧) */
  viewportDegraded: string | null;
  /** 最近一次 viewport_frame 诊断 */
  viewportInfo: ViewportInfo | null;

  loadEntities: () => Promise<void>;
  refreshSummary: () => Promise<void>;
  refreshPlayState: () => Promise<void>;
  loadComponentTypes: () => Promise<void>;
  loadEvents: () => Promise<void>;

  selectEntity: (id: number | null) => void;
  createEntity: (name?: string) => Promise<void>;
  destroyEntity: (id: number) => Promise<void>;
  renameEntity: (id: number, name: string) => Promise<void>;
  setTransform: (id: number, patch: Partial<TransformData>) => Promise<void>;
  addComponent: (id: number, type: string) => Promise<void>;
  removeComponent: (id: number, type: string) => Promise<void>;
  setComponentEnabled: (id: number, type: string, enabled: boolean) => Promise<void>;

  playEnter: () => Promise<void>;
  playPause: () => Promise<void>;
  playResume: () => Promise<void>;
  playStep: () => Promise<void>;
  playExit: () => Promise<void>;

  undo: () => Promise<void>;
  redo: () => Promise<void>;
  saveScene: () => Promise<void>;
  loadScene: () => Promise<void>;

  sendChat: (text: string) => Promise<void>;

  setGizmo: (g: GizmoMode) => void;
  setCenterTab: (t: CenterTab) => void;
  setWorkbenchTab: (t: WorkbenchTab) => void;
  toggleChat: () => void;
  /** 预填 Chat 输入框并确保 Chat 打开(Assets 右键「生成」) */
  prefillChat: (text: string) => void;
  /** ChatDock 消费预填后清除 */
  clearChatPrefill: () => void;

  loadCamera: () => Promise<void>;
  updateCamera: (patch: Partial<CameraData>) => Promise<void>;
  orbitCamera: (dxPx: number, dyPx: number) => Promise<void>;
  zoomCamera: (wheelDeltaY: number) => Promise<void>;
  focusSelected: () => Promise<void>;
  pickAt: (x: number, y: number, w: number, h: number) => Promise<void>;
  /** gizmo 拖拽提交(拖拽结束一次性 transform_set,可 undo) */
  gizmoDragSelected: (dxPx: number, dyPx: number, viewH: number) => Promise<void>;
  setViewportStatus: (degraded: string | null, info: ViewportInfo | null) => void;
}

export const useEditorStore = create<EditorState>((set, get) => {
  /** 统一错误出口:写入 lastError,UI 如实显示 */
  async function run(fn: () => Promise<void>): Promise<void> {
    try {
      await fn();
      set({ lastError: null });
    } catch (err) {
      set({ lastError: (err as Error).message });
    }
  }

  /** 变更成功后局部刷新实体列表 */
  async function reload(): Promise<void> {
    const data = await callTool<{ entities: EntityData[] }>('entity_list');
    set({ entities: data.entities });
  }

  return {
    entities: [],
    selectedId: null,
    playState: 'edit',
    events: [],
    stats: null,
    sceneName: '',
    scenePath: null,
    componentTypes: [],
    lastError: null,

    gizmo: 'translate',
    centerTab: 'viewport',
    workbenchTab: 'console',
    chatOpen: false,
    chatMessages: [],
    chatPrefill: null,

    camera: null,
    viewportDegraded: null,
    viewportInfo: null,

    loadEntities: () => run(reload),

    refreshSummary: () =>
      run(async () => {
        const s = await callTool<SceneSummary>('scene_summary');
        set({ stats: s.render, sceneName: s.name, playState: s.playState });
      }),

    refreshPlayState: () =>
      run(async () => {
        const r = await callTool<{ state: PlayState }>('play_state');
        set({ playState: r.state });
      }),

    loadComponentTypes: () =>
      run(async () => {
        const list = await callTool<ComponentTypeInfo[]>('component_list_types');
        set({ componentTypes: list });
      }),

    loadEvents: () =>
      run(async () => {
        const list = await callTool<HostEvent[]>('host_events');
        set({ events: list });
      }),

    selectEntity: (id) => set({ selectedId: id }),

    createEntity: (name) =>
      run(async () => {
        const finalName = name ?? `Entity ${get().entities.length + 1}`;
        const r = await callTool<{ id: number; entity: EntityData }>('entity_create', {
          name: finalName,
        });
        set((s) => ({ entities: [...s.entities, r.entity], selectedId: r.id }));
      }),

    destroyEntity: (id) =>
      run(async () => {
        await callTool('entity_destroy', { id });
        set((s) => ({
          entities: s.entities.filter((e) => e.id !== id),
          selectedId: s.selectedId === id ? null : s.selectedId,
        }));
      }),

    renameEntity: (id, name) =>
      run(async () => {
        await callTool('entity_rename', { id, name });
        set((s) => ({
          entities: s.entities.map((e) => (e.id === id ? { ...e, name } : e)),
        }));
      }),

    setTransform: (id, patch) =>
      run(async () => {
        const t = await callTool<TransformData>('transform_set', { id, ...patch });
        set((s) => ({
          entities: s.entities.map((e) => (e.id === id ? { ...e, transform: t } : e)),
        }));
      }),

    addComponent: (id, type) =>
      run(async () => {
        await callTool('component_add', { id, type });
        await reload();
      }),

    removeComponent: (id, type) =>
      run(async () => {
        await callTool('component_remove', { id, type });
        set((s) => ({
          entities: s.entities.map((e) =>
            e.id === id
              ? { ...e, components: e.components.filter((c) => c.type !== type) }
              : e,
          ),
        }));
      }),

    setComponentEnabled: (id, type, enabled) =>
      run(async () => {
        await callTool('component_set', { id, type, enabled });
        set((s) => ({
          entities: s.entities.map((e) =>
            e.id === id
              ? {
                  ...e,
                  components: e.components.map((c) => (c.type === type ? { ...c, enabled } : c)),
                }
              : e,
          ),
        }));
      }),

    playEnter: () =>
      run(async () => {
        const r = await callTool<{ state: PlayState }>('play_enter');
        set({ playState: r.state });
        await reload(); // play 态列表为运行态克隆
      }),

    playPause: () =>
      run(async () => {
        const r = await callTool<{ state: PlayState }>('play_pause');
        set({ playState: r.state });
      }),

    playResume: () =>
      run(async () => {
        const r = await callTool<{ state: PlayState }>('play_resume');
        set({ playState: r.state });
      }),

    playStep: () =>
      run(async () => {
        const r = await callTool<{ state: PlayState }>('play_step');
        set({ playState: r.state });
      }),

    playExit: () =>
      run(async () => {
        const r = await callTool<{ state: PlayState }>('play_exit');
        set({ playState: r.state });
        await reload(); // 回到编辑态
      }),

    undo: () =>
      run(async () => {
        await callTool('edit_undo');
        await reload();
      }),

    redo: () =>
      run(async () => {
        await callTool('edit_redo');
        await reload();
      }),

    saveScene: () =>
      run(async () => {
        const r = await callTool<{ path: string; bytes: number }>('scene_save');
        set({ scenePath: r.path });
      }),

    loadScene: () =>
      run(async () => {
        const path = get().scenePath ?? 'data/scene.rxscene';
        await callTool('scene_load', { path });
        await reload();
        const s = await callTool<SceneSummary>('scene_summary');
        set({ stats: s.render, sceneName: s.name, playState: s.playState });
      }),

    sendChat: (text) =>
      run(async () => {
        set((s) => ({ chatMessages: [...s.chatMessages, { role: 'user', text }] }));
        try {
          // F1 seam:无 LLM,以 scene_summary 实测全链路回显
          const r = await callTool<SceneSummary>('scene_summary');
          set((s) => ({
            stats: r.render,
            sceneName: r.name,
            playState: r.playState,
            chatMessages: [
              ...s.chatMessages,
              { role: 'assistant', text: JSON.stringify(r, null, 2) },
            ],
          }));
        } catch (err) {
          set((s) => ({
            chatMessages: [...s.chatMessages, { role: 'error', text: (err as Error).message }],
          }));
        }
      }),

    setGizmo: (g) => set({ gizmo: g }),
    setCenterTab: (t) => set({ centerTab: t }),
    setWorkbenchTab: (t) => set({ workbenchTab: t }),
    toggleChat: () => set((s) => ({ chatOpen: !s.chatOpen })),
    prefillChat: (text) => set({ chatPrefill: text, chatOpen: true }),
    clearChatPrefill: () => set({ chatPrefill: null }),

    loadCamera: () =>
      run(async () => {
        const c = await callTool<CameraData>('viewport_get_camera');
        set({ camera: c });
      }),

    updateCamera: (patch) =>
      run(async () => {
        const c = await callTool<CameraData>('viewport_set_camera', patch as Record<string, unknown>);
        set({ camera: c });
      }),

    orbitCamera: async (dxPx, dyPx) => {
      const c = get().camera;
      if (!c) return;
      // Alt+左键拖拽:右拖 → 方位角减(视线右移),下拖 → 俯仰角增(视线上移)
      await get().updateCamera({ yaw: c.yaw - dxPx * 0.35, pitch: c.pitch + dyPx * 0.35 });
    },

    zoomCamera: async (wheelDeltaY) => {
      const c = get().camera;
      if (!c) return;
      await get().updateCamera({ dist: c.dist * Math.pow(1.0015, wheelDeltaY) });
    },

    focusSelected: async () => {
      const { selectedId, entities } = get();
      const e = entities.find((x) => x.id === selectedId);
      if (!e) return;
      await get().updateCamera({ target: [...e.transform.translation] });
    },

    pickAt: (x, y, w, h) =>
      run(async () => {
        const r = await callTool<PickResult>('viewport_pick', { x, y, width: w, height: h });
        set({ selectedId: r.hit ? (r.entityId ?? null) : null });
      }),

    gizmoDragSelected: (dxPx, dyPx, viewH) =>
      run(async () => {
        const { selectedId, gizmo, camera, entities } = get();
        if (selectedId == null || !camera) return;
        const e = entities.find((x) => x.id === selectedId);
        if (!e) return;
        const t = e.transform;
        if (gizmo === 'translate') {
          // 相机平面拖动:像素→世界 = 2·dist·tan(fovY/2)/视口高
          const wpp = (2 * camera.dist * Math.tan((camera.fovY * Math.PI) / 360)) / Math.max(viewH, 1);
          const { right, up } = cameraBasis(camera);
          const t0 = t.translation;
          await get().setTransform(selectedId, {
            translation: [
              t0[0] + right[0] * dxPx * wpp - up[0] * dyPx * wpp,
              t0[1] + right[1] * dxPx * wpp - up[1] * dyPx * wpp,
              t0[2] + right[2] * dxPx * wpp - up[2] * dyPx * wpp,
            ],
          });
        } else if (gizmo === 'rotate') {
          // 绕世界 Y 轴:dx 像素 → 0.5°/px
          const rad = (dxPx * 0.5 * Math.PI) / 360;
          const qYaw = [0, Math.sin(rad), 0, Math.cos(rad)];
          await get().setTransform(selectedId, { rotation: quatMul(qYaw, t.rotation) });
        } else {
          // 均匀缩放:dx → 1.005^dx,钳 [0.01, 100]
          const f = Math.pow(1.005, dxPx);
          await get().setTransform(selectedId, {
            scale: t.scale.map((v) => Math.min(100, Math.max(0.01, v * f))),
          });
        }
      }),

    setViewportStatus: (degraded, info) =>
      set((s) => ({
        viewportDegraded: degraded,
        viewportInfo: info ?? s.viewportInfo,
      })),
  };
});
