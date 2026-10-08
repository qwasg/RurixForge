import { act, cleanup, fireEvent, render, screen } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import HomeRecommendations from '@/components/shell/HomeRecommendations';
import { useSessionStore } from '@/lib/sessionStore';
import { useWorkspaceStore } from '@/lib/workspaceStore';
import { useComposerPrefillStore } from '@/lib/composerStore';
import { useOverlayStore } from '@/lib/overlayStore';

const sessions = useSessionStore.getState();
const workspaces = useWorkspaceStore.getState();
const prefill = useComposerPrefillStore.getState();
const overlay = useOverlayStore.getState();
const face = (label: string, mode = 'ask') => ({
  source: 'agent-capabilities',
  context: { gameMode: '2d' },
  recommendations: [{ id: 'actual', label, desc: '来自当前工作区', draft: `${label}的实际提示词`, mode, image: 'debug' }],
});
const response = (value: unknown) => ({ ok: true, status: 200, json: async () => value }) as Response;

beforeEach(() => {
  useSessionStore.setState({ ...sessions, activeSessionId: null, sessions: [], draftAgentEngine: 'local' }, true);
  useWorkspaceStore.setState({ ...workspaces, activeWorkspaceId: 'ws_a' }, true);
  useComposerPrefillStore.setState(prefill, true);
  useOverlayStore.setState(overlay, true);
});
afterEach(() => {
  cleanup(); vi.unstubAllGlobals();
  useSessionStore.setState(sessions, true); useWorkspaceStore.setState(workspaces, true);
  useComposerPrefillStore.setState(prefill, true); useOverlayStore.setState(overlay, true);
});

describe('后台推荐同步', () => {
  it('携带工作区和引擎，点击使用后台原始提示词与模式', async () => {
    const fetch = vi.fn(async (_url: string) => response(face('检查真正的场景')));
    vi.stubGlobal('fetch', fetch);
    render(<HomeRecommendations />);
    const card = await screen.findByTestId('home-quick-actual');
    expect(fetch.mock.calls[0]?.[0]).toContain('workspaceId=ws_a');
    expect(fetch.mock.calls[0]?.[0]).toContain('agentEngine=local');
    fireEvent.click(card);
    expect(useComposerPrefillStore.getState()).toMatchObject({ draft: '检查真正的场景的实际提示词', mode: 'ask' });
  });

  it('切换工作区后丢弃旧响应，切换引擎后重新查询', async () => {
    let finishOld!: (response: Response) => void;
    const fetch = vi.fn().mockImplementationOnce(() => new Promise<Response>((resolve) => { finishOld = resolve; }))
      .mockResolvedValueOnce(response(face('工作区 B'))).mockResolvedValueOnce(response(face('Codex 推荐', 'plan')));
    vi.stubGlobal('fetch', fetch);
    render(<HomeRecommendations />);
    act(() => useWorkspaceStore.setState({ activeWorkspaceId: 'ws_b' }));
    expect(await screen.findByTestId('home-quick-actual')).toHaveTextContent('工作区 B');
    await act(async () => finishOld(response(face('过时的 A'))));
    expect(screen.queryByText('过时的 A')).not.toBeInTheDocument();
    act(() => useSessionStore.setState({ draftAgentEngine: 'codex' }));
    expect(await screen.findByText('Codex 推荐')).toBeInTheDocument();
    expect(fetch.mock.calls[2]?.[0]).toContain('agentEngine=codex');
  });

  it('失败后显示可重试状态，设置关闭后重新读取权限对应的推荐', async () => {
    const fetch = vi.fn().mockRejectedValueOnce(new Error('offline')).mockResolvedValueOnce(response(face('恢复后的推荐')))
      .mockResolvedValue(response(face('只读推荐')));
    vi.stubGlobal('fetch', fetch);
    render(<HomeRecommendations />);
    expect(await screen.findByText('暂时无法获取推荐')).toBeInTheDocument();
    fireEvent.click(screen.getByRole('button', { name: '重试' }));
    expect(await screen.findByText('恢复后的推荐')).toBeInTheDocument();
    act(() => useOverlayStore.getState().open('settings'));
    expect(await screen.findByText('只读推荐')).toBeInTheDocument();
    act(() => useOverlayStore.getState().close('settings'));
    await vi.waitFor(() => expect(fetch).toHaveBeenCalledTimes(4));
  });
});
