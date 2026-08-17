import { create } from 'zustand';
import { PINNED_AGENTS, WORKSPACES } from './mock';
import type { SidebarAgent, Workspace } from './types';

export type Route = 'home' | 'agent' | 'automations' | 'customize';
export type RightPanelView = 'menu' | 'changes' | 'browser' | 'terminal' | 'files';

interface AppState {
  route: Route;
  /** 当前打开的会话 id(侧栏点击 / 搜索面板跳转) */
  activeAgentId: string | null;
  sidebarVisible: boolean;
  paletteOpen: boolean;
  rightPanelOpen: boolean;
  rightPanelView: RightPanelView;
  /** 侧栏数据(固定 / 工作区),pin / archive 会真实增删 */
  pinnedAgents: SidebarAgent[];
  workspaces: Workspace[];

  goHome: () => void;
  openAgent: (id: string | null) => void;
  openAutomations: () => void;
  openCustomize: () => void;
  toggleSidebar: () => void;
  setPaletteOpen: (open: boolean) => void;
  toggleRightPanel: () => void;
  setRightPanelView: (view: RightPanelView) => void;
  /** 工作区会话固定到 Pinned */
  pinAgent: (id: string) => void;
  /** 取消 Pinned */
  unpinAgent: (id: string) => void;
  /** 归档:从 Pinned 与所在工作区移除 */
  archiveAgent: (id: string) => void;
  /** 归档全部 Pinned(Pinned 区标题上的 Archive 按钮) */
  archiveAllPinned: () => void;
}

export const useAppStore = create<AppState>((set) => ({
  route: 'home',
  activeAgentId: null,
  sidebarVisible: true,
  paletteOpen: false,
  rightPanelOpen: true,
  rightPanelView: 'menu',
  pinnedAgents: PINNED_AGENTS,
  workspaces: WORKSPACES,

  goHome: () => set({ route: 'home', activeAgentId: null }),
  openAgent: (id) => set({ route: 'agent', activeAgentId: id, paletteOpen: false }),
  openAutomations: () => set({ route: 'automations' }),
  openCustomize: () => set({ route: 'customize' }),
  toggleSidebar: () => set((s) => ({ sidebarVisible: !s.sidebarVisible })),
  setPaletteOpen: (open) => set({ paletteOpen: open }),
  toggleRightPanel: () => set((s) => ({ rightPanelOpen: !s.rightPanelOpen })),
  setRightPanelView: (view) => set({ rightPanelView: view, rightPanelOpen: true }),

  pinAgent: (id) =>
    set((s) => {
      if (s.pinnedAgents.some((a) => a.id === id)) return s;
      const agent = s.workspaces.flatMap((w) => w.agents).find((a) => a.id === id);
      return agent ? { pinnedAgents: [...s.pinnedAgents, agent] } : s;
    }),
  unpinAgent: (id) => set((s) => ({ pinnedAgents: s.pinnedAgents.filter((a) => a.id !== id) })),
  archiveAgent: (id) =>
    set((s) => ({
      pinnedAgents: s.pinnedAgents.filter((a) => a.id !== id),
      workspaces: s.workspaces.map((w) => ({
        ...w,
        agents: w.agents.filter((a) => a.id !== id),
      })),
    })),
  archiveAllPinned: () => set({ pinnedAgents: [] }),
}));
