import { beforeEach, describe, expect, it } from 'vitest';
import { useAppStore } from '@/lib/store';

// zustand store 为模块级单例:每个用例前重置回初始态(含 actions)
const initialState = useAppStore.getState();
beforeEach(() => {
  useAppStore.setState(initialState, true);
});

describe('useAppStore', () => {
  it('openAgent:切换到 agent 视图、记录 activeAgentId 并关闭搜索面板', () => {
    useAppStore.getState().setPaletteOpen(true);
    useAppStore.getState().openAgent('a-cindy');
    const s = useAppStore.getState();
    expect(s.route).toBe('agent');
    expect(s.activeAgentId).toBe('a-cindy');
    expect(s.paletteOpen).toBe(false);
  });

  it('goHome:回到 home 视图并清空 activeAgentId', () => {
    useAppStore.getState().openAgent('a-cindy');
    useAppStore.getState().goHome();
    const s = useAppStore.getState();
    expect(s.route).toBe('home');
    expect(s.activeAgentId).toBeNull();
  });

  it('pinAgent / unpinAgent:工作区会话固定到 Pinned 并可取消', () => {
    const s0 = useAppStore.getState();
    const target = s0.workspaces
      .flatMap((w) => w.agents)
      .find((a) => !s0.pinnedAgents.some((p) => p.id === a.id));
    expect(target).toBeDefined();

    useAppStore.getState().pinAgent(target!.id);
    expect(useAppStore.getState().pinnedAgents.map((a) => a.id)).toContain(target!.id);

    useAppStore.getState().unpinAgent(target!.id);
    expect(useAppStore.getState().pinnedAgents.map((a) => a.id)).not.toContain(target!.id);
  });

  it('archiveAgent:同时从 Pinned 与所在工作区移除', () => {
    const pinned = useAppStore.getState().pinnedAgents[0];
    expect(pinned).toBeDefined();
    useAppStore.getState().archiveAgent(pinned.id);
    const s = useAppStore.getState();
    expect(s.pinnedAgents.some((a) => a.id === pinned.id)).toBe(false);
    expect(s.workspaces.flatMap((w) => w.agents).some((a) => a.id === pinned.id)).toBe(false);
  });
});
