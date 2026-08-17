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
  };
});
