import { act, cleanup, fireEvent, render, screen } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import ProposalsTab from '@/components/workbench/ProposalsTab';
import TodoTab from '@/components/workbench/TodoTab';
import Workbench from '@/components/shell/Workbench';
import { useChatStore } from '@/lib/chatStore';
import { useComposerPrefillStore } from '@/lib/composerStore';
import { useGoalStore } from '@/lib/goalStore';
import { useSessionStore } from '@/lib/sessionStore';
import { useWorkbenchStore } from '@/lib/workbenchStore';

/**
 * F7 wave.5 workbench tabs:Todo(四列看板分列)+ 提案(列表渲染/impact 展开/批准拒绝接线)。
 * D-035:Plan 页签已改为计划文件页(按 path 多开),用例迁到 planTab.test.tsx。
 */

const initialChat = useChatStore.getState();
const initialPrefill = useComposerPrefillStore.getState();
const initialGoal = useGoalStore.getState();
const initialSessions = useSessionStore.getState();
const initialWorkbench = useWorkbenchStore.getState();

beforeEach(() => {
  useChatStore.setState(initialChat, true);
  useChatStore.getState().reset();
  useComposerPrefillStore.setState(initialPrefill, true);
  useGoalStore.setState(initialGoal, true);
  useGoalStore.getState().reset();
  useSessionStore.setState(initialSessions, true);
  useWorkbenchStore.setState(initialWorkbench, true);
  globalThis.localStorage?.clear();
});

afterEach(() => {
  cleanup();
  vi.unstubAllGlobals();
});

describe('Goal tab', () => {
  it('Workbench goal 分支显示目标、预算与状态操作', () => {
    const pauseGoal = vi.fn(async () => null);
    useSessionStore.setState({
      activeSessionId: 'sess_goal',
      sessions: [{
        id: 'sess_goal', title: '目标会话', status: 'idle', agentKind: 'coding', agentEngine: 'codex',
        selectedModelId: null, thinkingEnabled: false, reasoningEffort: null,
        contextOptionId: null, webSearchEnabled: false, activeRunId: null,
        createdAt: '', updatedAt: '', pinned: false, titleManuallySet: false,
      }],
    });
    useGoalStore.setState({
      sessionId: 'sess_goal',
      goal: {
        sessionId: 'sess_goal', objective: '完成垂直切片', status: 'active', engine: 'codex',
        tokenBudget: 50_000, tokensUsed: 12_000, timeUsedSeconds: 90, turns: 3,
      },
      history: [{ status: 'active', ts: '2026-09-04T10:00:00.000Z', tokensUsed: 12_000 }],
      loadGoal: vi.fn(async () => undefined),
      pauseGoal,
    });
    useWorkbenchStore.setState({
      tabs: [{ id: 'goal', kind: 'goal', title: 'Goal' }],
      activeTabId: 'goal',
    });
    render(<Workbench />);
    expect(screen.getByTestId('workbench-tab-goal')).toBeInTheDocument();
    expect(screen.getByTestId('goal-objective')).toHaveValue('完成垂直切片');
    expect(screen.getByTestId('goal-metrics')).toHaveTextContent('12,000');
    fireEvent.click(screen.getByTestId('goal-pause'));
    expect(pauseGoal).toHaveBeenCalled();
  });
});

describe('Todo tab', () => {
  it('四列看板分列:Backlog=queued / Running=running / Review=failed / Done=completed;卡含 description', () => {
    useChatStore.setState({
      todos: [
        { id: 't1', title: '排队中', status: 'queued', description: '先取证再动手\n第二行描述' },
        { id: 't2', title: '执行中', status: 'running' },
        { id: 't3', title: '失败了', status: 'failed' },
        { id: 't4', title: '完成了', status: 'completed' },
      ],
    });
    render(<TodoTab />);
    expect(screen.getByTestId('todo-col-Backlog')).toHaveTextContent('排队中');
    expect(screen.getByTestId('todo-col-Running')).toHaveTextContent('执行中');
    expect(screen.getByTestId('todo-col-Review')).toHaveTextContent('失败了');
    expect(screen.getByTestId('todo-col-Done')).toHaveTextContent('完成了');
    // 描述两行截断渲染(内容在)
    expect(screen.getByTestId('todo-card-t1')).toHaveTextContent('先取证再动手');
    // 列计数
    expect(screen.getByTestId('todo-col-Done')).toHaveTextContent('1');
  });

  it('空态:暂无待办', () => {
    render(<TodoTab />);
    expect(screen.getByText('暂无待办。')).toBeInTheDocument();
  });
});

describe('提案 tab', () => {
  it('列表渲染 + impact 展开 + 批准接线(PATCH action=approve 后状态翻转)', async () => {
    const calls: Array<{ url: string; method: string; body: string }> = [];
    let status = 'pending';
    vi.stubGlobal(
      'fetch',
      vi.fn(async (url: unknown, init?: { method?: string; body?: string }) => {
        const u = String(url);
        const method = init?.method ?? 'GET';
        calls.push({ url: u, method, body: init?.body ?? '' });
        if (u.startsWith('/api/forge/proposals/') && method === 'PATCH') {
          status = JSON.parse(init?.body ?? '{}').action === 'approve' ? 'approved' : 'rejected';
          return { ok: true, status: 200, json: async () => ({ id: 'prop_1', status }) } as Response;
        }
        return {
          ok: true,
          status: 200,
          json: async () => ({
            proposals: [
              {
                id: 'prop_1',
                kind: 'asset.delete',
                summary: 'force 删除 1 个资产',
                impact: { assets: ['Meshes/x.gltf'] },
                status,
                createdBy: { sessionId: 'http', tool: 'asset_delete' },
              },
            ],
          }),
        } as Response;
      }),
    );
    render(<ProposalsTab />);
    // 列表行(kind/summary/status)
    const row = await screen.findByTestId('proposal-row-prop_1');
    expect(row).toHaveTextContent('asset.delete');
    expect(row).toHaveTextContent('force 删除 1 个资产');
    expect(screen.getByTestId('proposal-status-prop_1')).toHaveTextContent('pending');
    // impact 展开(pretty JSON 含资产路径)
    fireEvent.click(screen.getByTestId('proposal-expand-prop_1'));
    expect(screen.getByTestId('proposal-payload-prop_1')).toHaveTextContent('Meshes/x.gltf');
    // 批准 → PATCH {action:approve} → 重载后 approved
    fireEvent.click(screen.getByTestId('proposal-approve-prop_1'));
    await act(async () => {
      await Promise.resolve();
    });
    const patch = calls.find((c) => c.method === 'PATCH');
    expect(patch).toBeDefined();
    expect(patch!.url).toBe('/api/forge/proposals/prop_1');
    expect(JSON.parse(patch!.body)).toEqual({ action: 'approve' });
    expect(await screen.findByTestId('proposal-status-prop_1')).toHaveTextContent('approved');
  });

  it('拒绝接线(PATCH action=reject)', async () => {
    const bodies: string[] = [];
    vi.stubGlobal(
      'fetch',
      vi.fn(async (url: unknown, init?: { method?: string; body?: string }) => {
        const u = String(url);
        if (u.startsWith('/api/forge/proposals/') && init?.method === 'PATCH') {
          bodies.push(init.body ?? '');
          return { ok: true, status: 200, json: async () => ({ id: 'prop_2', status: 'rejected' }) } as Response;
        }
        return {
          ok: true,
          status: 200,
          json: async () => ({
            proposals: [{ id: 'prop_2', kind: 'asset.cleanup', summary: '整理', impact: {}, status: 'pending' }],
          }),
        } as Response;
      }),
    );
    render(<ProposalsTab />);
    fireEvent.click(await screen.findByTestId('proposal-reject-prop_2'));
    await act(async () => {
      await Promise.resolve();
    });
    expect(bodies.length).toBe(1);
    expect(JSON.parse(bodies[0])).toEqual({ action: 'reject' });
  });

  it('空列表:暂无提案', async () => {
    vi.stubGlobal(
      'fetch',
      vi.fn(async () => ({ ok: true, status: 200, json: async () => ({ proposals: [] }) }) as Response),
    );
    render(<ProposalsTab />);
    expect(await screen.findByTestId('proposals-empty')).toHaveTextContent('暂无提案');
  });
});
