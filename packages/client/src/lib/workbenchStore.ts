import { create } from 'zustand';

/**
 * F7 wave.3 workbench tab 体系 + 三栏宽/折叠态。
 * 栏宽持久化 forge:paneSizes(参考 keys::PANE_SIZES,语义照搬);
 * clamp:侧栏 200–360(默认 256)/ 对话列 300–560(默认 360)/ Inspector 240–420(默认 288)。
 */

/** F7 wave.5:内建 tab 种类(editor=游戏编辑器;plan/todo/proposals=本仓语义适配页)。 */
export type TabKind = 'editor' | 'plan' | 'todo' | 'proposals';

export interface WorkbenchTab {
  id: string;
  kind: TabKind;
  title: string;
}

/** 内建 tab 元信息(id=kind,单例)。 */
export const BUILTIN_TABS: Record<Exclude<TabKind, 'editor'>, { title: string }> = {
  plan: { title: 'Plan' },
  todo: { title: 'Todo' },
  proposals: { title: '提案' },
};

export type PaneKind = 'sessions' | 'chat' | 'inspector';

/** 底部面板 tab(参考 BottomPanelTab;Terminal/Problems 无后端面不落,RD-F7-002)。 */
export type BottomTab = 'logs' | 'output' | 'metrics';

export const BOTTOM_CLAMP = { min: 120, max: 520, def: 260 } as const;

export const PANE_CLAMP: Record<PaneKind, { min: number; max: number; def: number }> = {
  sessions: { min: 200, max: 360, def: 256 },
  chat: { min: 300, max: 560, def: 360 },
  inspector: { min: 240, max: 420, def: 288 },
};

const PANE_KEY = 'forge:paneSizes';

interface PanePersist {
  w: Record<PaneKind, number>;
  collapsed: Record<PaneKind, boolean>;
}

function clampW(kind: PaneKind, w: number): number {
  const c = PANE_CLAMP[kind];
  return Math.min(c.max, Math.max(c.min, Math.round(w)));
}

function loadPanes(): PanePersist {
  const def: PanePersist = {
    w: { sessions: PANE_CLAMP.sessions.def, chat: PANE_CLAMP.chat.def, inspector: PANE_CLAMP.inspector.def },
    collapsed: { sessions: false, chat: false, inspector: false },
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
    };
  } catch {
    return def;
  }
}

function persistPanes(s: { paneW: Record<PaneKind, number>; collapsed: Record<PaneKind, boolean> }): void {
  try {
    globalThis.localStorage?.setItem(PANE_KEY, JSON.stringify({ w: s.paneW, collapsed: s.collapsed }));
  } catch {
    // 写不进静默
  }
}

interface WorkbenchState {
  tabs: WorkbenchTab[];
  activeTabId: string | null;
  paneW: Record<PaneKind, number>;
  collapsed: Record<PaneKind, boolean>;
  /** 底部面板开关(Ctrl+J;持久化 forge:bottomPanel)。 */
  bottomOpen: boolean;
  /** 底部面板高(clamp 120–520,默认 260;持久化)。 */
  bottomH: number;
  bottomTab: BottomTab;

  openEditor: () => void;
  openTab: (kind: TabKind) => void;
  closeTab: (id: string) => void;
  activateTab: (id: string) => void;
  setPaneW: (kind: PaneKind, w: number) => void;
  togglePane: (kind: PaneKind) => void;
  toggleBottom: () => void;
  setBottomH: (h: number) => void;
  setBottomTab: (t: BottomTab) => void;
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
    paneW: panes.w,
    collapsed: panes.collapsed,
    bottomOpen: bottom.open,
    bottomH: bottom.h,
    bottomTab: bottom.tab,

    openEditor: () => {
      get().openTab('editor');
    },

    openTab: (kind) => {
      const { tabs } = get();
      const title = kind === 'editor' ? '编辑器' : BUILTIN_TABS[kind].title;
      if (!tabs.some((t) => t.id === kind)) {
        set({ tabs: [...tabs, { id: kind, kind, title }], activeTabId: kind });
      } else {
        set({ activeTabId: kind });
      }
    },

    closeTab: (id) => {
      const { tabs, activeTabId } = get();
      const idx = tabs.findIndex((t) => t.id === id);
      const next = tabs.filter((t) => t.id !== id);
      let nextActive = activeTabId;
      if (activeTabId === id) {
        nextActive = next.length === 0 ? null : (next[Math.min(idx, next.length - 1)]?.id ?? null);
      }
      set({ tabs: next, activeTabId: nextActive });
    },

    activateTab: (id) => {
      if (get().tabs.some((t) => t.id === id)) set({ activeTabId: id });
    },

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
  };
});
