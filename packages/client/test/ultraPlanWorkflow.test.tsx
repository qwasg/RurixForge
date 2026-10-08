import { act, cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import UltraPlanBlock from '@/components/chat/ultraplan/UltraPlanBlock';
import { useChatStore } from '@/lib/chatStore';
import { useSessionStore } from '@/lib/sessionStore';
import { useWorkbenchStore } from '@/lib/workbenchStore';
import { useEditorStore } from '@/lib/editorStore';
import { useWorkspaceStore } from '@/lib/workspaceStore';
import { normalizeUltraPlanState, useUltraPlanStore, type UltraPlanState } from '@/lib/ultraPlanStore';
import type { ChatBlock } from '@/lib/timeline';

type Ultra = Extract<ChatBlock, { kind: 'ultraplan' }>;
const SID = 'sess_flow';
const UP = 'up_flow';
const PATH = '.forge/plans/tower.plan.md';
const initialChat = useChatStore.getState();
const initialSession = useSessionStore.getState();
const initialWorkbench = useWorkbenchStore.getState();
const initialUltra = useUltraPlanStore.getState();
const initialEditor = useEditorStore.getState();
const initialWorkspace = useWorkspaceStore.getState();
const delivery = { entry: 'Content/Scenes/main.rxscene', controls: '方向键移动，R 重开' };
const checks = [
  { id: 'win', title: '击败敌人获胜', steps: '开始游戏并击败全部敌人', expected: '出现胜利结算' },
  { id: 'sound', title: '声音反馈', steps: '点击攻击按钮', expected: '播放攻击音效', required: false },
];
let state: UltraPlanState;
let posts: { url: string; body: Record<string, any> }[];
let error: { code: string; message: string } | null;

function card(step: Ultra['step'], overrides: Partial<Ultra> = {}): Ultra {
  return { kind: 'ultraplan', upId: UP, step, rev: step === 'acceptance' ? 2 : 3, payload: step === 'plan'
    ? { planPath: PATH, name: '塔防 MVP', overview: '完成建塔与敌人波次循环', gameMode: '2d', renderBackend: 'godot', roles: ['场景', '玩法', '验证'], taskCount: 6, automated: 3, manual: 2 }
    : step === 'acceptance' ? { manual: checks } : { dir: '.forge/ultraplan/tower', planPath: PATH }, ...overrides };
}

function mount(stage: UltraPlanState['stage'], patch: Partial<UltraPlanState> = {}) {
  state = normalizeUltraPlanState({ id: UP, stage, phase: 'waiting', planPath: PATH, planRev: 3, acceptanceRound: 2, renderBackend: 'godot', ...patch })!;
  useUltraPlanStore.getState().hydrate(state, SID);
}

beforeEach(() => {
  useUltraPlanStore.setState(initialUltra, true);
  useUltraPlanStore.getState().reset();
  useChatStore.setState({ ...initialChat, activeRunId: null }, true);
  useSessionStore.setState({ ...initialSession, activeSessionId: SID, sessions: [{
    id: SID, title: '塔防', status: 'idle', agentKind: 'coding', agentEngine: 'codex', selectedModelId: null,
    thinkingEnabled: false, reasoningEffort: null, contextOptionId: null, webSearchEnabled: false,
    activeRunId: null, createdAt: '', updatedAt: '', pinned: false, titleManuallySet: false,
  }] }, true);
  useWorkbenchStore.setState(initialWorkbench, true);
  useWorkspaceStore.setState({ ...initialWorkspace, activeWorkspaceId: null }, true);
  useEditorStore.setState({ ...initialEditor, lastError: null, scenePath: null,
    openScenePath: vi.fn(async (path: string) => { useEditorStore.setState({ scenePath: path, lastError: null }); }),
    playEnter: vi.fn(async () => {}),
  }, true);
  localStorage.clear(); posts = []; error = null;
  mount('plan_review');
  vi.stubGlobal('fetch', vi.fn(async (url: unknown, init?: RequestInit) => {
    if ((init?.method ?? 'GET') === 'GET') return { ok: true, status: 200, json: async () => ({ ultraplan: state, delivery }) };
    const body = JSON.parse(String(init?.body ?? '{}'));
    posts.push({ url: String(url), body });
    if (error) return { ok: false, status: 409, json: async () => ({ error }) };
    if (String(url).endsWith('/acceptance')) {
      const failed = body.results.filter((r: { status: string }) => r.status === 'fail').map((r: { id: string }) => r.id);
      state = { ...state, stage: failed.length ? 'acceptance' : 'done' };
      return { ok: true, status: 200, json: async () => ({ ultraplan: state, next: failed.length ? 'fix' : 'done', failed }) };
    }
    return { ok: true, status: 200, json: async () => ({ run: { id: 'run_1', status: 'completed' } }) };
  }));
});

afterEach(() => { cleanup(); vi.unstubAllGlobals(); });

describe('UltraPlan 计划确认', () => {
  it('显示技术后端与分工,确认通过 team + start_production 开工', async () => {
    render(<UltraPlanBlock block={card('plan')} />);
    expect(screen.getByTestId('ultraplan-plan-card')).toHaveTextContent('游戏后端: godot');
    expect(screen.getByTestId('ultraplan-plan-card')).toHaveTextContent('场景、玩法、验证');
    fireEvent.click(screen.getByTestId('ultraplan-plan-open'));
    expect(useWorkbenchStore.getState().tabs[0]).toMatchObject({ path: PATH, upId: UP, sessionId: SID });
    fireEvent.click(screen.getByTestId('ultraplan-plan-start'));
    await waitFor(() => expect(posts).toHaveLength(1));
    expect(posts[0].body).toEqual({ userInput: '', mode: 'team', planPath: PATH, ultraplan: { id: UP, rev: 3, action: 'start_production' } });
    await waitFor(() => expect(screen.getByTestId('ultraplan-plan-start')).toBeDisabled());
  });

  it('修改计划需要意见,保持当前 id 与版本', async () => {
    render(<UltraPlanBlock block={card('plan')} />);
    fireEvent.click(screen.getByTestId('ultraplan-plan-revise-toggle'));
    expect(screen.getByTestId('ultraplan-plan-revise-submit')).toBeDisabled();
    fireEvent.change(screen.getByTestId('ultraplan-plan-feedback'), { target: { value: '增加键鼠回归验证' } });
    fireEvent.click(screen.getByTestId('ultraplan-plan-revise-submit'));
    await waitFor(() => expect(posts).toHaveLength(1));
    expect(posts[0].body).toEqual({ userInput: '增加键鼠回归验证', mode: 'ultraplan', ultraplan: { id: UP, rev: 3, action: 'revise_plan' } });
  });

  it('逐项审批需要明确确认,不会自行 bypass', async () => {
    error = { code: 'ULTRAPLAN_NEEDS_BYPASS', message: '请确认逐项审批' };
    render(<UltraPlanBlock block={card('plan')} />);
    fireEvent.click(screen.getByTestId('ultraplan-plan-start'));
    await screen.findByTestId('ultraplan-plan-approval');
    expect(posts).toHaveLength(1);
    expect(posts[0].body.ultraplan.acknowledgeApprovals).toBeUndefined();
    error = null;
    fireEvent.click(screen.getByTestId('ultraplan-plan-ack-confirm'));
    await waitFor(() => expect(posts).toHaveLength(2));
    expect(posts[1].body.ultraplan.acknowledgeApprovals).toBe(true);
  });

  it('旧版、运行中与其他会话卡片都不能触发制作', () => {
    const { rerender } = render(<UltraPlanBlock block={card('plan', { rev: 2 })} />);
    expect(screen.getByTestId('ultraplan-plan-start')).toBeDisabled();
    rerender(<UltraPlanBlock block={card('plan')} />);
    act(() => useChatStore.setState({ activeRunId: 'running' }));
    expect(screen.getByTestId('ultraplan-plan-start')).toBeDisabled();
    act(() => { useChatStore.setState({ activeRunId: null }); useSessionStore.setState({ activeSessionId: 'other' }); });
    expect(screen.getByTestId('ultraplan-plan-start')).toBeDisabled();
    fireEvent.click(screen.getByTestId('ultraplan-plan-start'));
    expect(posts).toHaveLength(0);
  });

  it('计划文件变更拒绝就地呈现,不变成普通 Build', async () => {
    error = { code: 'ULTRAPLAN_PLAN_CHANGED', message: '计划文件已改变,请重新生成并确认' };
    render(<UltraPlanBlock block={card('plan')} />);
    fireEvent.click(screen.getByTestId('ultraplan-plan-start'));
    expect(await screen.findByTestId('ultraplan-plan-error')).toHaveTextContent('计划文件已改变');
    expect(posts[0].body.mode).toBe('team');
  });
});

describe('UltraPlan 人工验收与自动修复', () => {
  beforeEach(() => mount('acceptance'));

  it('必要项不能跳过,可选跳过和未通过都需要原因,草稿刷新保留', () => {
    const { unmount } = render(<UltraPlanBlock block={card('acceptance')} />);
    expect(screen.getByTestId('acceptance-win-skip')).toBeDisabled();
    expect(screen.getByTestId('acceptance-win')).toHaveTextContent('开始游戏并击败全部敌人');
    expect(screen.getByTestId('acceptance-win')).toHaveTextContent('出现胜利结算');
    fireEvent.click(screen.getByTestId('acceptance-win-fail'));
    fireEvent.click(screen.getByTestId('acceptance-sound-skip'));
    expect(screen.getByTestId('acceptance-submit')).toBeDisabled();
    fireEvent.change(screen.getByTestId('acceptance-win-note'), { target: { value: '没有结算界面' } });
    expect(screen.getByTestId('acceptance-submit')).toBeDisabled();
    fireEvent.change(screen.getByTestId('acceptance-sound-note'), { target: { value: '本机静音' } });
    expect(screen.getByTestId('acceptance-submit')).toBeEnabled();
    unmount(); render(<UltraPlanBlock block={card('acceptance')} />);
    expect(screen.getByTestId('acceptance-win-fail')).toHaveAttribute('aria-pressed', 'true');
    expect(screen.getByTestId('acceptance-win-note')).toHaveValue('没有结算界面');
    expect(screen.getByTestId('acceptance-submit')).toBeEnabled();
  });

  it('提交失败项自动发 fix_production,不要求再手动点一次', async () => {
    render(<UltraPlanBlock block={card('acceptance')} />);
    fireEvent.click(screen.getByTestId('acceptance-win-fail'));
    fireEvent.change(screen.getByTestId('acceptance-win-note'), { target: { value: '结算卡死' } });
    fireEvent.click(screen.getByTestId('acceptance-sound-pass'));
    fireEvent.click(screen.getByTestId('acceptance-submit'));
    await waitFor(() => expect(posts).toHaveLength(2));
    expect(posts[0].body).toEqual({ id: UP, rev: 3, round: 2, results: [{ id: 'win', status: 'fail', note: '结算卡死' }, { id: 'sound', status: 'pass' }] });
    expect(posts[1].body).toEqual({ userInput: '', mode: 'team', ultraplan: { id: UP, rev: 2, action: 'fix_production' } });
    expect(screen.getByTestId('acceptance-submitted')).toHaveTextContent('未通过 1');
    expect(localStorage.getItem(`forge:ultraplan:acceptance:${UP}:2`)).toBeNull();
  });

  it('全部通过直接完成,不再开制作轮', async () => {
    const { rerender } = render(<UltraPlanBlock block={card('acceptance')} />);
    fireEvent.click(screen.getByTestId('acceptance-win-pass'));
    fireEvent.click(screen.getByTestId('acceptance-sound-pass'));
    fireEvent.click(screen.getByTestId('acceptance-submit'));
    await waitFor(() => expect(useUltraPlanStore.getState().state?.stage).toBe('done'));
    expect(posts).toHaveLength(1);
    rerender(<UltraPlanBlock block={card('done')} />);
    expect(screen.getByTestId('ultraplan-done-card')).toHaveTextContent('游戏源码和资产保留在项目工作区');
    fireEvent.click(screen.getByRole('button', { name: '打开游戏' }));
    await waitFor(() => expect(useWorkbenchStore.getState().activeTabId).toBe('editor'));
    expect(useEditorStore.getState().openScenePath).toHaveBeenCalledWith(delivery.entry);
    expect(useEditorStore.getState().playEnter).toHaveBeenCalledOnce();
  });

  it('已提交的失败轮刷新后可恢复修复,不重新提交验收', async () => {
    render(<UltraPlanBlock block={card('acceptance', { submitted: { results: [{ id: 'win', status: 'fail', note: '卡住' }, { id: 'sound', status: 'pass' }] } })} />);
    expect(screen.queryByTestId('acceptance-submit')).not.toBeInTheDocument();
    fireEvent.click(screen.getByTestId('acceptance-retry-fix'));
    await waitFor(() => expect(posts).toHaveLength(1));
    expect(posts[0].body.ultraplan.action).toBe('fix_production');
  });

  it('自动修复被逐项审批门拦住时等用户确认,不重复提交验收', async () => {
    const originalFetch = globalThis.fetch;
    vi.stubGlobal('fetch', vi.fn(async (url: unknown, init?: RequestInit) => {
      if (String(url).endsWith('/ask:execute')) error = { code: 'ULTRAPLAN_NEEDS_BYPASS', message: '制作期间需要逐项审批' };
      return originalFetch(url as RequestInfo, init);
    }));
    render(<UltraPlanBlock block={card('acceptance')} />);
    fireEvent.click(screen.getByTestId('acceptance-win-fail'));
    fireEvent.change(screen.getByTestId('acceptance-win-note'), { target: { value: '关卡卡死' } });
    fireEvent.click(screen.getByTestId('acceptance-sound-pass'));
    fireEvent.click(screen.getByTestId('acceptance-submit'));
    await screen.findByTestId('acceptance-approval');
    expect(posts).toHaveLength(2);
    expect(posts[1].body.ultraplan.acknowledgeApprovals).toBeUndefined();
    error = null;
    vi.stubGlobal('fetch', originalFetch);
    fireEvent.click(screen.getByTestId('acceptance-ack-confirm'));
    await waitFor(() => expect(posts).toHaveLength(3));
    expect(posts[2].body.ultraplan).toEqual({ id: UP, rev: 2, action: 'fix_production', acknowledgeApprovals: true });
    expect(posts.filter((p) => p.url.endsWith('/acceptance'))).toHaveLength(1);
  });

  it('验收提交过程中切换会话不向新会话派发修复', async () => {
    let release!: () => void;
    const originalFetch = globalThis.fetch;
    vi.stubGlobal('fetch', vi.fn(async (url: unknown, init?: RequestInit) => {
      if (String(url).endsWith('/acceptance')) await new Promise<void>((resolve) => { release = resolve; });
      return originalFetch(url as RequestInfo, init);
    }));
    render(<UltraPlanBlock block={card('acceptance')} />);
    fireEvent.click(screen.getByTestId('acceptance-win-fail'));
    fireEvent.change(screen.getByTestId('acceptance-win-note'), { target: { value: '卡住' } });
    fireEvent.click(screen.getByTestId('acceptance-sound-pass'));
    fireEvent.click(screen.getByTestId('acceptance-submit'));
    act(() => useSessionStore.setState({ activeSessionId: 'other' }));
    await act(async () => { release(); });
    expect(posts).toHaveLength(1);
    expect(posts[0].url).toContain(SID);
    expect(posts.some((p) => p.url.endsWith('/ask:execute'))).toBe(false);
  });

  it('过期验收轮只读,不会覆盖新的结果', () => {
    render(<UltraPlanBlock block={card('acceptance', { rev: 1 })} />);
    expect(screen.getByTestId('acceptance-win-pass')).toBeDisabled();
    expect(screen.getByTestId('acceptance-submit')).toBeDisabled();
    expect(posts).toHaveLength(0);
  });
});

describe('UltraPlan 交付试玩', () => {
  beforeEach(() => { mount('acceptance'); useUltraPlanStore.setState({ delivery }); });

  it('先加载批准入口再Play，并显示操作说明和证据入口', async () => {
    render(<UltraPlanBlock block={card('acceptance')} />);
    expect(screen.getByTestId('ultraplan-delivery')).toHaveTextContent(delivery.controls);
    fireEvent.click(screen.getByTestId('ultraplan-play-game'));
    await waitFor(() => expect(useEditorStore.getState().playEnter).toHaveBeenCalledOnce());
    expect(useEditorStore.getState().openScenePath).toHaveBeenCalledWith(delivery.entry);
  });

  it('场景加载失败时不进入Play，显示原始错误', async () => {
    useEditorStore.setState({ openScenePath: vi.fn(async () => { useEditorStore.setState({ lastError: 'SCENE_LOAD_FAILED: 文件不存在' }); }) });
    render(<UltraPlanBlock block={card('acceptance')} />);
    fireEvent.click(screen.getByTestId('ultraplan-play-game'));
    expect(await screen.findByTestId('ultraplan-play-error')).toHaveTextContent('SCENE_LOAD_FAILED');
    expect(useEditorStore.getState().playEnter).not.toHaveBeenCalled();
  });

  it('工作区不匹配或加载期间切换会话不会Play另一个项目', async () => {
    useWorkspaceStore.setState({ activeWorkspaceId: 'other-project' });
    const { rerender } = render(<UltraPlanBlock block={card('acceptance')} />);
    expect(screen.getByTestId('ultraplan-play-game')).toBeDisabled();
    act(() => useWorkspaceStore.setState({ activeWorkspaceId: null }));
    let release!: () => void;
    useEditorStore.setState({ openScenePath: vi.fn(async () => { await new Promise<void>((resolve) => { release = resolve; }); useEditorStore.setState({ scenePath: delivery.entry }); }) });
    rerender(<UltraPlanBlock block={card('acceptance')} />);
    fireEvent.click(screen.getByTestId('ultraplan-play-game'));
    act(() => useSessionStore.setState({ activeSessionId: 'other-session' }));
    await act(async () => release());
    expect(useEditorStore.getState().playEnter).not.toHaveBeenCalled();
  });
});
