import { create } from 'zustand';
import { apiPost, callTool, getForgeRenderConfig, renderConfigMismatch, type RenderBackendInfo, type RenderCapabilities } from './forgeApi';
import { readActiveWorkspaceId } from './activeWorkspace';
import { useToastStore } from './toastStore';
import { useWorkbenchStore } from './workbenchStore';
import type { EntityCategory } from './entityCategory';

/**
 * 编辑器状态:实体 / 选中 / PIE / 帧统计,action 全部真实打后端。
 * F7 wave.3(D-F7-B):chat 子集(chatOpen/chatMessages/sendChat/toggleChat/
 * COMPOSER_MODES/ComposerMode/SwarmReport/LlmChatResponse/executeMultitask)退役——
 * agent 对话统一由壳内对话列承接(wave.4 chatStore);chatPrefill 三件套保留
 * (AssetsPanel「生成」预填 seam,wave.4 composer 消费)。
 * 底栏波(2026-08-24 用户拍板):Workbench 面板整块退役,随之退役 host 事件流
 * (events/loadEvents/HostEvent)、Metrics 采样环(metricsHistory)与 workbenchTab;
 * playtest 报告改走 toast 汇总(唯一展示位)。
 */

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
  identityPersisted?: boolean;
  guid?: string;
  entityGuid?: string;
  id: number;
  name: string;
  transform: TransformData;
  components: ComponentData[];
  /** 计算字段:role/map/interaction(由 entity_list 附加) */
  category?: EntityCategory;
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

/** playtest 矩阵报告(/api/forge/playtest/run 响应面;toast 汇总用) */
export interface PlaytestReport {
  scene: string;
  ok: boolean;
  passed: number;
  failed: number;
  durationMs: number;
  cases: Array<{ name: string; kind: string; pass: boolean; actual: unknown; expected: unknown; detail: string }>;
}

/** 中央区同位页签(画板波 2026-08-24:+design 画板设计;素材创作波:+studio 素材创作) */
export type CenterTab = 'viewport' | 'nodegraph' | 'design' | 'studio' | 'shadergraph';
export type GizmoMode = 'translate' | 'rotate' | 'scale';

/**
 * 编辑器面板显隐(2026-08-24 底栏波):
 * Inspector / Hierarchy 已迁壳右栏;Assets 迁底栏后横向铺满、不再与视口争宽度,
 * 故响应式波的宽度分档收放退役,只留工具条手动开合 + 持久化。
 */
export interface EditorPanes {
  assets: boolean;
}

const EDITOR_PANES_KEY = 'forge:editorPanes';

function loadPanePref(): EditorPanes {
  try {
    const raw = globalThis.localStorage?.getItem(EDITOR_PANES_KEY);
    if (!raw) return { assets: true };
    const p = JSON.parse(raw) as Partial<EditorPanes>;
    return { assets: p.assets !== false };
  } catch {
    return { assets: true };
  }
}

function persistPanePref(p: EditorPanes): void {
  try {
    globalThis.localStorage?.setItem(EDITOR_PANES_KEY, JSON.stringify(p));
  } catch {
    // 写不进静默
  }
}

/** 编辑器相机(与服务端 EditorCamera 字段一一对应;F-GAME-3:+ortho/orthoSize 正交) */
export interface CameraData {
  target: number[];
  yaw: number;
  pitch: number;
  dist: number;
  fovY: number;
  /** true = 正交(2D 视口);false = 透视 */
  ortho: boolean;
  /** 正交半高(世界单位;2D 缩放即调它) */
  orthoSize: number;
}

/** 视口帧诊断面(轮询腿来自 viewport_frame 响应;直连流腿来自 1Hz status 消息) */
export interface ViewportInfo {
  deviceName: string;
  draws: number;
  truncated: boolean;
  /** 累计帧数(轮询腿) */
  frames?: number;
  /** 非背景像素数(轮询腿诊断;流腿跳过全帧扫描,无此值) */
  nonZeroPixels?: number;
  /** 服务端实测推流 fps(流腿) */
  fps?: number;
  /** 当前帧通道(直连流 / 轮询回退) */
  channel?: 'stream' | 'poll';
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

/** 像素→世界换算(F-GAME-3:正交 = 2·orthoSize/视口高;透视 = 2·dist·tan(fovY/2)/视口高) */
function worldPerPixel(c: CameraData, viewH: number): number {
  const h = Math.max(viewH, 1);
  return c.ortho
    ? (2 * c.orthoSize) / h
    : (2 * c.dist * Math.tan((c.fovY * Math.PI) / 360)) / h;
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
  identityPersisted?: boolean;
  sceneGuid?: string;
  hostEpoch?: string;
  contentRevision?: number;
  name: string;
  entityCount: number;
  playState: PlayState;
  render: RenderStats;
  /** F-GAME-3:场景模式("2d"|"3d";旧服务端无此字段 → undefined 按 3d) */
  mode?: string;
}

interface EditorState {
  identityPersisted: boolean | null;
  sceneGuid: string | null;
  hostEpoch: string | null;
  contentRevision: number | null;
  entities: EntityData[];
  selectedId: number | null;
  selectedIds: number[];
  playState: PlayState;
  stats: RenderStats | null;
  sceneName: string;
  /** 最近一次 scene_save 落盘路径,供 scene_load 复用 */
  scenePath: string | null;
  componentTypes: ComponentTypeInfo[];
  lastError: string | null;

  gizmo: GizmoMode;
  centerTab: CenterTab;
  /** F-GAME-3:当前场景模式(2d/3d;驱动视口手势/网格/徽标) */
  sceneMode: '2d' | '3d';
  /** F2 wave.3:Assets 右键「生成」预填 seam(F7 wave.3 保留;wave.4 composer 消费) */
  chatPrefill: string | null;

  /** 编辑器面板显隐(持久化的手动开合结果) */
  editorPanes: EditorPanes;

  /** 编辑器相机(null = 未拉取) */
  camera: CameraData | null;
  /** 视口降级原因(null = 正常;DEV_ENV_DEGRADE 等如实显示,不伪造帧) */
  viewportDegraded: string | null;
  /** 最近一次 viewport_frame 诊断 */
  viewportInfo: ViewportInfo | null;
  /** 当前工作区 engine-host 返回的真实渲染后端 / 能力面。 */
  renderBackendInfo: RenderBackendInfo | null;
  renderCapabilities: RenderCapabilities | null;
  renderConfigMismatch: string | null;
  renderStatusError: string | null;
  renderStatusWorkspaceId: string | null;
  renderStatusLoaded: boolean;

  loadEntities: () => Promise<void>;
  refreshSummary: () => Promise<void>;
  refreshPlayState: () => Promise<void>;
  loadComponentTypes: () => Promise<void>;
  /** 跑 playtest 矩阵,汇总行 + 失败用例走 toast(红绿如实) */
  runPlaytest: (matrixRef: string) => Promise<void>;

  selectEntity: (id: number | null, additive?: boolean) => void;
  createEntity: (name?: string) => Promise<void>;
  destroyEntity: (id: number) => Promise<void>;
  renameEntity: (id: number, name: string) => Promise<void>;
  setTransform: (id: number, patch: Partial<TransformData>) => Promise<void>;
  addComponent: (id: number, type: string) => Promise<void>;
  removeComponent: (id: number, type: string) => Promise<void>;
  setComponentEnabled: (id: number, type: string, enabled: boolean) => Promise<void>;
  /** D-045:整份替换组件 props(component_set 语义;调用方负责合并)。 */
  setComponentProps: (id: number, type: string, props: Record<string, unknown>) => Promise<void>;
  /** 设置实体分类(写 Category 组件;可 undo) */
  setCategory: (id: number, category: EntityCategory) => Promise<void>;

  playEnter: () => Promise<void>;
  playPause: () => Promise<void>;
  playResume: () => Promise<void>;
  playStep: () => Promise<void>;
  playExit: () => Promise<void>;

  undo: () => Promise<void>;
  redo: () => Promise<void>;
  saveScene: () => Promise<void>;
  loadScene: () => Promise<void>;
  /** 打开指定 .rxscene(项目相对路径;Assets 面板双击 Scene 资产的入口;play 态先退出)。 */
  openScenePath: (path: string) => Promise<void>;
  /** 空场景时加载默认场景(demo 迷宫;entityCount>0 不动)——打开 IDE 即见真实场景而非空工程。 */
  ensureDefaultScene: () => Promise<void>;

  setGizmo: (g: GizmoMode) => void;
  setCenterTab: (t: CenterTab) => void;
  /** 手动显隐某面板(持久化) */
  toggleEditorPane: (which: keyof EditorPanes) => void;
  /** 预填 chat 输入框(F7 wave.3:仅写 chatPrefill;wave.4 composer 消费) */
  prefillChat: (text: string) => void;
  /** 消费预填后清除 */
  clearChatPrefill: () => void;

  loadCamera: () => Promise<void>;
  updateCamera: (patch: Partial<CameraData>) => Promise<void>;
  /** 直连流模式本地先行:只改本地相机态(钳制与服务端一致);引擎侧由 WS camera 消息喂回 */
  setCameraLocal: (patch: Partial<CameraData>) => void;
  orbitCamera: (dxPx: number, dyPx: number) => Promise<void>;
  /** F-GAME-3:2D 平移(像素 → 世界,按投影模式换算);3D 下等效于移动 target 的平移手势 */
  panCamera: (dxPx: number, dyPx: number, viewH: number) => Promise<void>;
  /** 直连流本地先行平移(与 panCamera 同公式,只改本地相机态;引擎侧由 WS camera 消息喂回) */
  panCameraLocal: (dxPx: number, dyPx: number, viewH: number) => void;
  zoomCamera: (wheelDeltaY: number) => Promise<void>;
  focusSelected: () => Promise<void>;
  pickAt: (x: number, y: number, w: number, h: number) => Promise<void>;
  /** gizmo 拖拽提交(拖拽结束一次性 transform_set,可 undo);F-GAME-3:snap=false 临时禁用 2D 网格吸附(Ctrl) */
  gizmoDragSelected: (dxPx: number, dyPx: number, viewH: number, snap?: boolean) => Promise<void>;
  setViewportStatus: (degraded: string | null, info: ViewportInfo | null) => void;
  /** 读取当前 engine-host 的后端/能力；仅展示状态，不修改 forge.toml 或切换后端。 */
  refreshRenderBackend: (workspaceId: string | null) => Promise<void>;
}

export const useEditorStore = create<EditorState>((set, get) => {
  /**
   * 统一错误出口:写入 lastError。`toast` = 用户主动触发的动作(Play/存取场景等)失败时
   * 同时弹错误 toast——此前 Play 失败(如图加载失败)只写 lastError 而界面毫无反馈;
   * 轮询类调用(refreshSummary 等)不弹,免刷屏。
   */
  async function run(fn: () => Promise<void>, opts: { toast?: string } = {}): Promise<void> {
    try {
      await fn();
      set({ lastError: null });
    } catch (err) {
      const message = (err as Error).message;
      set({ lastError: message });
      if (opts.toast) useToastStore.getState().push('error', `${opts.toast}:${message}`);
    }
  }

  /** 变更成功后局部刷新实体列表 */
  let entityRequest = 0;
  let summaryRequest = 0;
  async function reload(): Promise<void> {
    const workspaceId = readActiveWorkspaceId();
    const request = ++entityRequest;
    const data = await callTool<{ entities: EntityData[] }>('entity_list');
    if (readActiveWorkspaceId() === workspaceId && request === entityRequest) set((state) => ({ entities: data.entities, selectedId: data.entities.some((e) => e.id === state.selectedId) ? state.selectedId : null, selectedIds: state.selectedIds.filter((id) => data.entities.some((e) => e.id === id)) }));
  }

  return {
    identityPersisted: null,
    sceneGuid: null,
    hostEpoch: null,
    contentRevision: null,
    entities: [],
    selectedId: null,
    selectedIds: [],
    playState: 'edit',
    stats: null,
    sceneName: '',
    scenePath: null,
    componentTypes: [],
    lastError: null,

    gizmo: 'translate',
    centerTab: 'viewport',
    sceneMode: '3d',
    chatPrefill: null,

    editorPanes: loadPanePref(),

    camera: null,
    viewportDegraded: null,
    viewportInfo: null,
    renderBackendInfo: null,
    renderCapabilities: null,
    renderConfigMismatch: null,
    renderStatusError: null,
    renderStatusWorkspaceId: null,
    renderStatusLoaded: false,

    loadEntities: () => run(reload),

    refreshSummary: () =>
      run(async () => {
        const workspaceId = readActiveWorkspaceId();
        const request = ++summaryRequest;
        const s = await callTool<SceneSummary>('scene_summary');
        if (workspaceId !== readActiveWorkspaceId() || request !== summaryRequest) return;
        if (s.hostEpoch === get().hostEpoch && s.sceneGuid === get().sceneGuid && typeof s.contentRevision === 'number' && typeof get().contentRevision === 'number' && s.contentRevision < get().contentRevision!) return;
        const changed = (s.contentRevision !== undefined && s.contentRevision !== get().contentRevision) || (s.hostEpoch !== undefined && s.hostEpoch !== get().hostEpoch) || (s.sceneGuid !== undefined && s.sceneGuid !== get().sceneGuid);
        set({ sceneGuid: s.sceneGuid ?? null, hostEpoch: s.hostEpoch ?? null, contentRevision: s.contentRevision ?? null, identityPersisted: s.identityPersisted ?? null });
        set({ stats: s.render, sceneName: s.name, playState: s.playState, sceneMode: s.mode === '2d' ? '2d' : '3d' });
        // F9(D1):entityCount 与本地清单漂移 = 外部(MCP/agent)实体变更 → 真实 entity_list 重拉。
        // 不伪造同步:漂移只作触发信号,面板数据始终来自后端 entity_list 实返。
        if (changed || s.entityCount !== get().entities.length) await reload();
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

    runPlaytest: (matrixRef) =>
      run(async () => {
        const r = await apiPost<PlaytestReport>('/api/forge/playtest/run', { matrixRef });
        const push = useToastStore.getState().push;
        push(
          r.ok ? 'success' : 'error',
          `${matrixRef} — ${r.ok ? 'PASS' : 'FAIL'} ${r.passed}/${r.passed + r.failed} (${r.durationMs}ms)`,
        );
        // 失败用例逐条如实抛出(名称 + kind + detail),不折叠成一句「若干失败」
        for (const c of r.cases) {
          if (!c.pass) push('error', `✗ ${c.name} [${c.kind}] ${c.detail}`);
        }
      }),

    selectEntity: (id, additive = false) => set((state) => {
      const selectedIds = id === null ? [] : additive ? state.selectedIds.includes(id) ? state.selectedIds.filter((value) => value !== id) : [...state.selectedIds, id] : [id];
      return { selectedIds, selectedId: selectedIds.at(-1) ?? null };
    }),

    createEntity: (name) =>
      run(async () => {
        const finalName = name ?? `Entity ${get().entities.length + 1}`;
        const r = await callTool<{ id: number; entity: EntityData }>('entity_create', {
          name: finalName,
        });
        set((s) => ({ entities: [...s.entities, r.entity], selectedId: r.id, selectedIds: [r.id] }));
      }),

    destroyEntity: (id) =>
      run(async () => {
        await callTool('entity_destroy', { id });
        set((s) => ({
          entities: s.entities.filter((e) => e.id !== id),
          selectedId: s.selectedId === id ? null : s.selectedId,
          selectedIds: s.selectedIds.filter((value) => value !== id),
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

    setComponentProps: (id, type, props) =>
      run(async () => {
        await callTool('component_set', { id, type, props });
        set((s) => ({
          entities: s.entities.map((e) =>
            e.id === id
              ? { ...e, components: e.components.map((c) => (c.type === type ? { ...c, props } : c)) }
              : e,
          ),
        }));
      }),

    setCategory: (id, category) =>
      run(async () => {
        const entity = get().entities.find((e) => e.id === id);
        if (!entity) return;
        const has = entity.components.some((c) => c.type === 'Category');
        if (has) {
          await callTool('component_set', { id, type: 'Category', props: { category } });
        } else {
          await callTool('component_add', {
            id,
            type: 'Category',
            props: { category },
          });
        }
        await reload();
      }),

    playEnter: () =>
      run(
        async () => {
          const r = await callTool<{ state: PlayState }>('play_enter');
          set({ playState: r.state });
          await reload(); // play 态列表为运行态克隆
        },
        { toast: 'Play 失败' },
      ),

    playPause: () =>
      run(
        async () => {
          const r = await callTool<{ state: PlayState }>('play_pause');
          set({ playState: r.state });
        },
        { toast: 'Pause 失败' },
      ),

    playResume: () =>
      run(
        async () => {
          const r = await callTool<{ state: PlayState }>('play_resume');
          set({ playState: r.state });
        },
        { toast: 'Resume 失败' },
      ),

    playStep: () =>
      run(
        async () => {
          const r = await callTool<{ state: PlayState }>('play_step');
          set({ playState: r.state });
        },
        { toast: 'Step 失败' },
      ),

    playExit: () =>
      run(
        async () => {
          const r = await callTool<{ state: PlayState }>('play_exit');
          set({ playState: r.state });
          await reload(); // 回到编辑态
        },
        { toast: 'Stop 失败' },
      ),

    undo: () =>
      run(async () => {
        await apiPost('/api/forge/editor/undo', { workspaceId: readActiveWorkspaceId() });
        await reload();
      }),

    redo: () =>
      run(async () => {
        await apiPost('/api/forge/editor/redo', { workspaceId: readActiveWorkspaceId() });
        await reload();
      }),

    saveScene: () =>
      run(
        async () => {
          const r = await callTool<{ path: string; bytes: number; savedCopy?: boolean }>('scene_save');
          if (r.savedCopy) useToastStore.getState().push('success', `已保存场景副本：${r.path}`);
          else set({ scenePath: r.path });
        },
        { toast: '保存场景失败' },
      ),

    loadScene: () =>
      run(
        async () => {
          const path = get().scenePath ?? 'data/scene.rxscene';
          await callTool('scene_load', { path });
          await reload();
          const s = await callTool<SceneSummary>('scene_summary');
          set({ stats: s.render, sceneName: s.name, playState: s.playState, sceneMode: s.mode === '2d' ? '2d' : '3d' });
        },
        { toast: '加载场景失败' },
      ),

    openScenePath: (path) =>
      run(
        async () => {
          if (get().playState !== 'edit') {
            await callTool('play_exit');
          }
          await callTool('scene_load', { path });
          await reload();
          const s = await callTool<SceneSummary>('scene_summary');
          set({
            stats: s.render,
            sceneName: s.name,
            playState: s.playState,
            scenePath: path,
            selectedId: null,
            selectedIds: [],
            sceneMode: s.mode === '2d' ? '2d' : '3d',
          });
          await get().loadCamera();
        },
        { toast: `打开场景失败(${path})` },
      ),

    ensureDefaultScene: () =>
      run(async () => {
        const s = await callTool<SceneSummary>('scene_summary');
        if (s.entityCount > 0) {
          set({ stats: s.render, sceneName: s.name, playState: s.playState, sceneMode: s.mode === '2d' ? '2d' : '3d' });
          return;
        }
        // 空场景兜底装 demo 迷宫;非 demo 项目(如 PvZ)没有这条路径,静默留空——
        // 用户从 Assets 面板双击 .rxscene 打开自己的场景,不把 demo 的缺省当错误弹出。
        try {
          await callTool('scene_load', { path: 'Content/Scenes/maze.rxscene' });
        } catch {
          return;
        }
        await reload();
        const s2 = await callTool<SceneSummary>('scene_summary');
        set({ stats: s2.render, sceneName: s2.name, playState: s2.playState, scenePath: 'Content/Scenes/maze.rxscene', sceneMode: s2.mode === '2d' ? '2d' : '3d' });
      }),

    setGizmo: (g) => set({ gizmo: g }),
    setCenterTab: (t) => set({ centerTab: t }),

    toggleEditorPane: (which) =>
      set((s) => {
        const next = { ...s.editorPanes, [which]: !s.editorPanes[which] };
        persistPanePref(next);
        return { editorPanes: next };
      }),

    prefillChat: (text) => set({ chatPrefill: text }),
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

    setCameraLocal: (patch) =>
      set((s) => {
        if (!s.camera) return {};
        const next = { ...s.camera, ...patch };
        // 与服务端 viewport.setCamera 同套钳制,避免本地/引擎两份相机态漂移。
        next.pitch = Math.min(89, Math.max(-89, next.pitch));
        next.dist = Math.min(500, Math.max(0.2, next.dist));
        next.fovY = Math.min(120, Math.max(10, next.fovY));
        next.orthoSize = Math.min(1000, Math.max(0.01, next.orthoSize));
        return { camera: next };
      }),

    orbitCamera: async (dxPx, dyPx) => {
      const c = get().camera;
      if (!c || c.ortho) return; // F-GAME-3:2D 正交视口禁用环绕(正视 XY 平面不倾斜)
      // Alt+左键拖拽:右拖 → 方位角减(视线右移),下拖 → 俯仰角增(视线上移)
      await get().updateCamera({ yaw: c.yaw - dxPx * 0.35, pitch: c.pitch + dyPx * 0.35 });
    },

    panCamera: async (dxPx, dyPx, viewH) => {
      const c = get().camera;
      if (!c) return;
      // 视野中心(target)沿相机右/上轴平移;右拖 → 视野左移(内容跟手)。
      const wpp = worldPerPixel(c, viewH);
      const { right, up } = cameraBasis(c);
      await get().updateCamera({
        target: [
          c.target[0] - right[0] * dxPx * wpp + up[0] * dyPx * wpp,
          c.target[1] - right[1] * dxPx * wpp + up[1] * dyPx * wpp,
          c.target[2] - right[2] * dxPx * wpp + up[2] * dyPx * wpp,
        ],
      });
    },

    panCameraLocal: (dxPx, dyPx, viewH) => {
      const c = get().camera;
      if (!c) return;
      const wpp = worldPerPixel(c, viewH);
      const { right, up } = cameraBasis(c);
      get().setCameraLocal({
        target: [
          c.target[0] - right[0] * dxPx * wpp + up[0] * dyPx * wpp,
          c.target[1] - right[1] * dxPx * wpp + up[1] * dyPx * wpp,
          c.target[2] - right[2] * dxPx * wpp + up[2] * dyPx * wpp,
        ],
      });
    },

    zoomCamera: async (wheelDeltaY) => {
      const c = get().camera;
      if (!c) return;
      // F-GAME-3:正交缩放调 orthoSize(滚轮上=放大=半高缩);透视沿视轴 dolly。
      if (c.ortho) {
        await get().updateCamera({ orthoSize: c.orthoSize * Math.pow(1.0015, wheelDeltaY) });
      } else {
        await get().updateCamera({ dist: c.dist * Math.pow(1.0015, wheelDeltaY) });
      }
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
        get().selectEntity(r.hit ? (r.entityId ?? null) : null);
        if (r.hit) useWorkbenchStore.getState().setRightTab('properties');
      }),

    gizmoDragSelected: (dxPx, dyPx, viewH, snap) =>
      run(async () => {
        const { selectedId, gizmo, camera, entities } = get();
        if (selectedId == null || !camera) return;
        const e = entities.find((x) => x.id === selectedId);
        if (!e) return;
        const t = e.transform;
        if (gizmo === 'translate') {
          // 相机平面拖动:像素→世界按投影模式换算(F-GAME-3 正交分支)
          const wpp = worldPerPixel(camera, viewH);
          const { right, up } = cameraBasis(camera);
          const t0 = t.translation;
          const next = [
            t0[0] + right[0] * dxPx * wpp - up[0] * dyPx * wpp,
            t0[1] + right[1] * dxPx * wpp - up[1] * dyPx * wpp,
            t0[2] + right[2] * dxPx * wpp - up[2] * dyPx * wpp,
          ];
          // F-GAME-3:2D 模式平移吸附 0.5 网格(07 §2 规范;Ctrl 临时禁用),z 保持不动。
          if (get().sceneMode === '2d' && snap !== false) {
            next[0] = Math.round(next[0] * 2) / 2;
            next[1] = Math.round(next[1] * 2) / 2;
          }
          await get().setTransform(selectedId, { translation: next });
        } else if (gizmo === 'rotate') {
          // F-GAME-3:2D 模式绕视线轴 Z(XY 平面内旋转);3D 模式绕世界 Y。dx 像素 → 0.5°/px
          const rad = (dxPx * 0.5 * Math.PI) / 360;
          const q = get().sceneMode === '2d'
            ? [0, 0, Math.sin(rad), Math.cos(rad)]
            : [0, Math.sin(rad), 0, Math.cos(rad)];
          await get().setTransform(selectedId, { rotation: quatMul(q, t.rotation) });
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

    refreshRenderBackend: async (workspaceId) => {
      const cached = get();
      const sameWorkspace = cached.renderStatusLoaded && cached.renderStatusWorkspaceId === workspaceId;
      const infoTask = callTool<RenderBackendInfo>('render_backend_info');
      const capsTask = sameWorkspace
        ? Promise.resolve(cached.renderCapabilities)
        : callTool<RenderCapabilities>('render_capabilities');
      const configTask = getForgeRenderConfig(workspaceId).catch(() => null);
      const [infoResult, capsResult, configResult] = await Promise.allSettled([infoTask, capsTask, configTask]);
      // MCP 请求已带工作区作用域；旧工作区慢响应不能覆盖新工作区状态。
      if (readActiveWorkspaceId() !== workspaceId) return;

      const info = infoResult.status === 'fulfilled'
        ? infoResult.value
        : sameWorkspace ? cached.renderBackendInfo : null;
      const capabilities = capsResult.status === 'fulfilled'
        ? capsResult.value
        : sameWorkspace ? cached.renderCapabilities : null;
      const config = configResult.status === 'fulfilled' ? configResult.value : null;
      const errors = [infoResult, capsResult]
        .filter((result) => result.status === 'rejected')
        .map((result) => result.reason instanceof Error ? result.reason.message : String(result.reason));
      const loaded = info !== null && capabilities !== null;
      set({
        renderBackendInfo: info,
        renderCapabilities: capabilities,
        renderConfigMismatch: info && config ? renderConfigMismatch(info, config) : null,
        renderStatusError: errors.length > 0 ? errors.join(' · ') : null,
        renderStatusWorkspaceId: workspaceId,
        renderStatusLoaded: loaded,
      });
    },
  };
});
