import { create } from 'zustand';

/**
 * F7 wave.3 workbench tab 体系 + 三栏宽/折叠态。
 * 栏宽持久化 forge:paneSizes(参考 keys::PANE_SIZES,语义照搬);
 * clamp:侧栏 200–360(默认 256)/ 对话列 300–560(默认 360)/ Inspector 240–420(默认 288)。
 */

/**
 * F7 wave.5:内建 tab 种类(editor=游戏编辑器;todo/proposals=本仓语义适配页)。
 * F11(D-025):store=资产商店、skills=Skill 管理——两个大类不新增常驻面板(I-3 冻结),
 * 沿用既有 workbench tab 承载,入口在 Sidebar 导航按钮。
 * F-GAME-4:sprite-editor=精灵编辑器(.rxsprite;入口在资产面板右键/双击,
 * 「当前编辑哪个 sprite」在 spriteStore,tab 本身仍是无 payload 单例)。
 * D-035:plan 退出单例集——计划是工作区文件(.forge/plans/<名>.plan.md),按 path 多开,
 * 与 file tab 同形态(dirty 拦截/草稿暂存全套复用)。
 */
export type BuiltinTabKind = 'editor' | 'goal' | 'todo' | 'proposals' | 'store' | 'skills' | 'sprite-editor';
/** 工作区 tab:内建页 + 按路径多开的文件编辑器与计划页(Cursor 式)。 */
export type TabKind = BuiltinTabKind | 'file' | 'plan';

export interface WorkbenchTab {
  id: string;
  kind: TabKind;
  title: string;
  /** kind=file|plan 时为工作区相对路径。 */
  path?: string;
  /** F9:未保存改动标记(文件编辑器写入;tabbar 圆点 + 关闭拦截消费)。 */
  dirty?: boolean;
}

/** 内建 tab 元信息(id=kind,单例)。 */
export const BUILTIN_TABS: Record<Exclude<BuiltinTabKind, 'editor'>, { title: string }> = {
  goal: { title: 'Goal' },
  todo: { title: 'Todo' },
  proposals: { title: '提案' },
  store: { title: '资产商店' },
  skills: { title: 'Skill 管理' },
  'sprite-editor': { title: '精灵编辑器' },
};

/** 文件预览 tab id(同一 path 单例)。 */
export function fileTabId(path: string): string {
  return `file:${path}`;
}

export function fileTabTitle(path: string): string {
  const name = path.replace(/\\/g, '/').split('/').pop();
  return name && name !== '' ? name : path;
}

/** D-035:计划 tab id(同一计划文件单例;与 file tab 分开,同一文件两种视图互不顶掉)。 */
export function planTabId(path: string): string {
  return `plan:${path}`;
}

/** 计划 tab 标题:去掉目录与 `.plan.md` 后缀(真名由 front matter 给,加载后回填)。 */
export function planTabTitle(path: string): string {
  const name = fileTabTitle(path);
  return name.replace(/\.plan\.md$/i, '') || 'Plan';
}

export type PaneKind = 'sessions' | 'chat' | 'inspector';

/**
 * 右栏承载页(2026-08-24):工作区文件树 / 场景层级 / 实体属性三选一。
 * 编辑器 tab 激活时层级接管右栏(编辑器内不再另开层级/属性列,省出宽度给视口),
 * 切走/关掉编辑器回落文件树;手动切换在下次 tab 变更前保持。
 */
export type RightTab = 'files' | 'hierarchy' | 'properties' | 'asset';

function rightTabFor(kind: TabKind | null | undefined): RightTab {
  return kind === 'editor' ? 'hierarchy' : 'files';
}

/** 底部面板 tab(参考 BottomPanelTab;Terminal/Problems 无后端面不落,RD-F7-002)。 */
export type BottomTab = 'logs' | 'output' | 'metrics';

export const BOTTOM_CLAMP = { min: 120, max: 520, def: 260 } as const;

export const PANE_CLAMP: Record<PaneKind, { min: number; max: number; def: number }> = {
  sessions: { min: 200, max: 360, def: 256 },
  chat: { min: 300, max: 560, def: 360 },
  inspector: { min: 240, max: 420, def: 288 },
};

const PANE_KEY = 'forge:paneSizes';

/** 缩小态浮窗的落点(相对壳主体区左上角,px)。 */
export interface MiniPos {
  x: number;
  y: number;
}

interface PanePersist {
  w: Record<PaneKind, number>;
  collapsed: Record<PaneKind, boolean>;
  chatMini: boolean;
  sessionsBeforeMini: boolean;
  miniPos: MiniPos | null;
}

function clampW(kind: PaneKind, w: number): number {
  const c = PANE_CLAMP[kind];
  return Math.min(c.max, Math.max(c.min, Math.round(w)));
}

function readMiniPos(v: unknown): MiniPos | null {
  if (typeof v !== 'object' || v === null) return null;
  const { x, y } = v as Partial<MiniPos>;
  if (typeof x !== 'number' || typeof y !== 'number') return null;
  if (!Number.isFinite(x) || !Number.isFinite(y)) return null;
  return { x: Math.round(x), y: Math.round(y) };
}

function loadPanes(): PanePersist {
  const def: PanePersist = {
    w: { sessions: PANE_CLAMP.sessions.def, chat: PANE_CLAMP.chat.def, inspector: PANE_CLAMP.inspector.def },
    collapsed: { sessions: false, chat: false, inspector: false },
    chatMini: false,
    sessionsBeforeMini: false,
    miniPos: null,
  };
  try {
    const raw = globalThis.localStorage?.getItem(PANE_KEY);
    if (!raw) return def;
    const parsed = JSON.parse(raw) as Partial<PanePersist>;
    return {
      w: {
        sessions: clampW('sessions', parsed.w?.sessions ?? def.w.sessions),
        chat: clampW('chat', parsed.w?.chat ?? def.w.chat),
        inspector: clampW('inspector', parsed.w?.inspector ?? def.w.inspector),
      },
      collapsed: {
        sessions: parsed.collapsed?.sessions === true,
        chat: parsed.collapsed?.chat === true,
        inspector: parsed.collapsed?.inspector === true,
      },
      chatMini: parsed.chatMini === true,
      sessionsBeforeMini: parsed.sessionsBeforeMini === true,
      miniPos: readMiniPos(parsed.miniPos),
    };
  } catch {
    return def;
  }
}

function persistPanes(s: {
  paneW: Record<PaneKind, number>;
  collapsed: Record<PaneKind, boolean>;
  chatMini: boolean;
  sessionsBeforeMini: boolean;
  miniPos: MiniPos | null;
}): void {
  try {
    globalThis.localStorage?.setItem(
      PANE_KEY,
      JSON.stringify({
        w: s.paneW,
        collapsed: s.collapsed,
        chatMini: s.chatMini,
        sessionsBeforeMini: s.sessionsBeforeMini,
        miniPos: s.miniPos,
      }),
    );
  } catch {
    // 写不进静默
  }
}

interface WorkbenchState {
  tabs: WorkbenchTab[];
  activeTabId: string | null;
  /** F9:待确认关闭的 dirty tab(closeTab 拦截后挂此,编辑器内联确认条接管)。 */
  pendingCloseTabId: string | null;
  /** 右栏当前页(随激活 tab 自动切换,可手动覆盖)。 */
  rightTab: RightTab;
  paneW: Record<PaneKind, number>;
  collapsed: Record<PaneKind, boolean>;
  /**
   * 对话缩小态(2026-08-25 用户拍板):对话脱离三栏流,收成贴主区左下角的浮窗,
   * 主区拿回整片宽度;缩小时顺手收起会话栏,还原时把会话栏放回缩小前的样子。
   */
  chatMini: boolean;
  /** 进缩小态前的会话栏折叠态,供还原时回填。 */
  sessionsBeforeMini: boolean;
  /**
   * 浮窗被拖到的落点(相对主体区左上角);null = 没搬过,仍贴主区左下角随会话栏让位。
   * 一旦搬过就按落点定死(不再跟会话栏走),还原/再缩小都回到这个位置。
   */
  miniPos: MiniPos | null;
  /** 底部面板开关(Ctrl+J;持久化 forge:bottomPanel)。 */
  bottomOpen: boolean;
  /** 底部面板高(clamp 120–520,默认 260;持久化)。 */
  bottomH: number;
  bottomTab: BottomTab;
  /**
   * 全屏对话主页(Codex 式,2026-08-24 用户拍板):workbench 没有 tab 时对话接管整屏。
   * 手动「进入工作台」置 true 暂避;开任一 tab 复位,关光 tab 后重新回主页。
   * 不持久化——tabs 本身也不持久化,刷新即回主页当落地页。
   */
  homeDismissed: boolean;

  openEditor: () => void;
  openTab: (kind: BuiltinTabKind) => void;
  /** 工作区文件 → 个人工作区 tab(已开则激活)。 */
  openFile: (path: string) => void;
  /** D-035:计划文件 → Plan tab(已开则激活;plan.created/updated 事件与命令面板共用)。 */
  openPlan: (path: string) => void;
  /** tab 标题回填(计划页加载出 front matter 真名后改 tabbar 文案)。 */
  setTabTitle: (id: string, title: string) => void;
  /** 关闭 tab(F9:dirty tab 先拦截 → 激活 + 挂 pendingCloseTabId,不直接关)。 */
  closeTab: (id: string) => void;
  /** 无条件关闭(dirty 确认后/干净 tab;原 closeTab 语义)。 */
  forceCloseTab: (id: string) => void;
  /** 取消 dirty 关闭确认。 */
  cancelCloseTab: () => void;
  /** F9:文件编辑器同步未保存标记。 */
  setTabDirty: (id: string, dirty: boolean) => void;
  activateTab: (id: string) => void;
  setRightTab: (t: RightTab) => void;
  setPaneW: (kind: PaneKind, w: number) => void;
  togglePane: (kind: PaneKind) => void;
  /** 进/出对话缩小态(连带会话栏收起与回填)。 */
  setChatMini: (v: boolean) => void;
  toggleChatMini: () => void;
  /** 挪浮窗;传 null = 归位到默认左下角。调用方负责 clamp 进可视区。 */
  setMiniPos: (pos: MiniPos | null) => void;
  toggleBottom: () => void;
  setBottomH: (h: number) => void;
  setBottomTab: (t: BottomTab) => void;
  setHomeDismissed: (v: boolean) => void;
}

const BOTTOM_KEY = 'forge:bottomPanel';

interface BottomPersist {
  open: boolean;
  h: number;
  tab: BottomTab;
}

function clampBottomH(h: number): number {
  return Math.min(BOTTOM_CLAMP.max, Math.max(BOTTOM_CLAMP.min, Math.round(h)));
}

function loadBottom(): BottomPersist {
  const def: BottomPersist = { open: false, h: BOTTOM_CLAMP.def, tab: 'logs' };
  try {
    const raw = globalThis.localStorage?.getItem(BOTTOM_KEY);
    if (!raw) return def;
    const parsed = JSON.parse(raw) as Partial<BottomPersist>;
    return {
      open: parsed.open === true,
      h: clampBottomH(typeof parsed.h === 'number' ? parsed.h : def.h),
      tab: parsed.tab === 'output' || parsed.tab === 'metrics' ? parsed.tab : 'logs',
    };
  } catch {
    return def;
  }
}

function persistBottom(s: { bottomOpen: boolean; bottomH: number; bottomTab: BottomTab }): void {
  try {
    globalThis.localStorage?.setItem(
      BOTTOM_KEY,
      JSON.stringify({ open: s.bottomOpen, h: s.bottomH, tab: s.bottomTab }),
    );
  } catch {
    // 写不进静默
  }
}

export const useWorkbenchStore = create<WorkbenchState>((set, get) => {
  const panes = loadPanes();
  const bottom = loadBottom();
  return {
    tabs: [],
    activeTabId: null,
    pendingCloseTabId: null,
    rightTab: 'files',
    paneW: panes.w,
    collapsed: panes.collapsed,
    chatMini: panes.chatMini,
    sessionsBeforeMini: panes.sessionsBeforeMini,
    miniPos: panes.miniPos,
    bottomOpen: bottom.open,
    bottomH: bottom.h,
    bottomTab: bottom.tab,
    homeDismissed: false,

    openEditor: () => {
      get().openTab('editor');
    },

    openTab: (kind) => {
      const { tabs } = get();
      const title = kind === 'editor' ? '编辑器' : BUILTIN_TABS[kind].title;
      const rightTab = rightTabFor(kind);
      if (!tabs.some((t) => t.id === kind)) {
        set({ tabs: [...tabs, { id: kind, kind, title }], activeTabId: kind, rightTab, homeDismissed: false });
      } else {
        set({ activeTabId: kind, rightTab, homeDismissed: false });
      }
    },

    openFile: (path) => {
      const id = fileTabId(path);
      const { tabs } = get();
      if (!tabs.some((t) => t.id === id)) {
        set({
          tabs: [...tabs, { id, kind: 'file', title: fileTabTitle(path), path }],
          activeTabId: id,
          rightTab: 'files',
          homeDismissed: false,
        });
      } else {
        set({ activeTabId: id, rightTab: 'files', homeDismissed: false });
      }
    },

    openPlan: (path) => {
      const id = planTabId(path);
      const { tabs } = get();
      if (!tabs.some((t) => t.id === id)) {
        set({
          tabs: [...tabs, { id, kind: 'plan', title: planTabTitle(path), path }],
          activeTabId: id,
          rightTab: 'files',
          homeDismissed: false,
        });
      } else {
        set({ activeTabId: id, rightTab: 'files', homeDismissed: false });
      }
    },

    setTabTitle: (id, title) => {
      const t = title.trim();
      if (t === '') return;
      set((st) =>
        st.tabs.some((x) => x.id === id && x.title !== t)
          ? { tabs: st.tabs.map((x) => (x.id === id ? { ...x, title: t } : x)) }
          : {},
      );
    },

    closeTab: (id) => {
      const tab = get().tabs.find((t) => t.id === id);
      if (tab?.dirty === true) {
        // dirty 拦截:激活该 tab 并挂确认(编辑器内联确认条:保存/放弃/取消)。
        set({ activeTabId: id, rightTab: rightTabFor(tab.kind), pendingCloseTabId: id });
        return;
      }
      get().forceCloseTab(id);
    },

    forceCloseTab: (id) => {
      const { tabs, activeTabId, pendingCloseTabId } = get();
      const idx = tabs.findIndex((t) => t.id === id);
      const next = tabs.filter((t) => t.id !== id);
      let nextActive = activeTabId;
      if (activeTabId === id) {
        nextActive = next.length === 0 ? null : (next[Math.min(idx, next.length - 1)]?.id ?? null);
      }
      set({
        tabs: next,
        activeTabId: nextActive,
        rightTab: rightTabFor(next.find((t) => t.id === nextActive)?.kind ?? null),
        pendingCloseTabId: pendingCloseTabId === id ? null : pendingCloseTabId,
      });
    },

    cancelCloseTab: () => set({ pendingCloseTabId: null }),

    setTabDirty: (id, dirty) => {
      const { tabs } = get();
      const tab = tabs.find((t) => t.id === id);
      if (!tab || (tab.dirty === true) === dirty) return;
      set({ tabs: tabs.map((t) => (t.id === id ? { ...t, dirty } : t)) });
    },

    activateTab: (id) => {
      const tab = get().tabs.find((t) => t.id === id);
      if (tab) set({ activeTabId: id, rightTab: rightTabFor(tab.kind) });
    },

    setRightTab: (t) => set({ rightTab: t }),

    setPaneW: (kind, w) => {
      set((st) => {
        const paneW = { ...st.paneW, [kind]: clampW(kind, w) };
        const next = { ...st, paneW };
        persistPanes(next);
        return { paneW };
      });
    },

    togglePane: (kind) => {
      set((st) => {
        const collapsed = { ...st.collapsed, [kind]: !st.collapsed[kind] };
        const next = { ...st, collapsed };
        persistPanes(next);
        return { collapsed };
      });
    },

    setChatMini: (v) => {
      set((st) => {
        if (st.chatMini === v) return {};
        const sessionsBeforeMini = v ? st.collapsed.sessions : st.sessionsBeforeMini;
        // 缩小态下对话必须可见(命令面板可能在对话栏已折叠时进来),故一并放开 chat。
        const collapsed = {
          ...st.collapsed,
          chat: false,
          sessions: v ? true : st.sessionsBeforeMini,
        };
        const next = { ...st, chatMini: v, sessionsBeforeMini, collapsed };
        persistPanes(next);
        return { chatMini: v, sessionsBeforeMini, collapsed };
      });
    },

    toggleChatMini: () => get().setChatMini(!get().chatMini),

    setMiniPos: (pos) => {
      set((st) => {
        const miniPos = pos === null ? null : { x: Math.round(pos.x), y: Math.round(pos.y) };
        if (st.miniPos?.x === miniPos?.x && st.miniPos?.y === miniPos?.y) return {};
        persistPanes({ ...st, miniPos });
        return { miniPos };
      });
    },

    toggleBottom: () => {
      set((st) => {
        const next = { ...st, bottomOpen: !st.bottomOpen };
        persistBottom(next);
        return { bottomOpen: next.bottomOpen };
      });
    },

    setBottomH: (h) => {
      set((st) => {
        const bottomH = clampBottomH(h);
        const next = { ...st, bottomH };
        persistBottom(next);
        return { bottomH };
      });
    },

    setBottomTab: (t) => {
      set((st) => {
        const next = { ...st, bottomTab: t };
        persistBottom(next);
        return { bottomTab: t };
      });
    },

    setHomeDismissed: (v) => set({ homeDismissed: v }),
  };
});

/** 全屏对话主页是否接管整屏(workbench 无 tab 且未手动暂避)。 */
export function useHomeMode(): boolean {
  return useWorkbenchStore((st) => st.tabs.length === 0 && !st.homeDismissed);
}
