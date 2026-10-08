import { act, cleanup, fireEvent, render, screen, within } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import ProposalsTab from '@/components/workbench/ProposalsTab';
import TodoTab from '@/components/workbench/TodoTab';
import Workbench from '@/components/shell/Workbench';
import { useChatStore } from '@/lib/chatStore';
import { useComposerPrefillStore } from '@/lib/composerStore';
import { useGoalStore } from '@/lib/goalStore';
import { useSessionStore } from '@/lib/sessionStore';
import { useUltraPlanStore } from '@/lib/ultraPlanStore';
import { demoTabId, demoTabTitle, useWorkbenchStore } from '@/lib/workbenchStore';

/**
 * F7 wave.5 workbench tabs:Todo(四列看板分列)+ 提案(列表渲染/impact 展开/批准拒绝接线)。
 * D-035:Plan 页签已改为计划文件页(按 path 多开),用例迁到 planTab.test.tsx。
 * D-044:Demo 页签(按流程 id 多开、带会话 id)+ Workbench 分支;页签内部行为见 demoTab.test.tsx。
 */

const initialChat = useChatStore.getState();
const initialPrefill = useComposerPrefillStore.getState();
const initialGoal = useGoalStore.getState();
const initialSessions = useSessionStore.getState();
const initialWorkbench = useWorkbenchStore.getState();
const initialUltra = useUltraPlanStore.getState();

beforeEach(() => {
  useUltraPlanStore.setState(initialUltra, true);
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

describe('Demo tab(D-044)', () => {
  const UP = 'up_a1';
  const TOKEN = '0123456789abcdef0123456789abcdef';

  it('openDemo:id=demo:<流程 id>,带流程 id 与会话 id;同一流程单例;标题后补;可关闭', () => {
    expect(demoTabId(UP)).toBe('demo:up_a1');
    expect(demoTabTitle('')).toBe('Demo');
    expect(demoTabTitle('  塔防 ')).toBe('Demo · 塔防');

    const wb = () => useWorkbenchStore.getState();
    wb().openTab('todo');
    wb().openDemo(UP, 'sess_1', '');
    expect(wb().activeTabId).toBe('demo:up_a1');
    expect(wb().tabs.find((t) => t.id === 'demo:up_a1')).toEqual({
      id: 'demo:up_a1',
      kind: 'demo',
      title: 'Demo',
      upId: UP,
      sessionId: 'sess_1',
    });
    expect(wb().rightTab).toBe('files');

    // 切走后再开:不重复,激活,补上标题
    wb().activateTab('todo');
    wb().openDemo(UP, 'sess_1', '塔防');
    expect(wb().tabs.filter((t) => t.kind === 'demo')).toHaveLength(1);
    expect(wb().activeTabId).toBe('demo:up_a1');
    expect(wb().tabs.find((t) => t.id === 'demo:up_a1')?.title).toBe('Demo · 塔防');
    // 空标题不覆盖已有标题
    wb().openDemo(UP, 'sess_1', '');
    expect(wb().tabs.find((t) => t.id === 'demo:up_a1')?.title).toBe('Demo · 塔防');

    // 另一个流程是另一个页签
    wb().openDemo('up_b2', 'sess_2', '跑酷');
    expect(wb().tabs.map((t) => t.id)).toEqual(['todo', 'demo:up_a1', 'demo:up_b2']);

    wb().closeTab('demo:up_b2');
    expect(wb().tabs.map((t) => t.id)).toEqual(['todo', 'demo:up_a1']);
    expect(wb().activeTabId).toBe('demo:up_a1');
  });

  it('Workbench demo 分支:tabbar 显示页签,正文挂 DemoTab 并加载 iframe', async () => {
    useSessionStore.setState({ activeSessionId: 'sess_1' });
    const state = {
      id: UP, token: TOKEN, slug: 'td', dir: '.forge/ultraplan/td', title: '塔防', workspaceId: null,
      stage: 'demo_review', phase: 'waiting', running: null, lastError: null, questionnaireRev: 1,
      demoIteration: 1, demoVerified: true, demoNote: null, planPath: null, planRev: 0, planHash: null,
      productionRunId: null, acceptanceRound: 0, createdAt: '', updatedAt: '',
    };
    useUltraPlanStore.getState().hydrate(state, 'sess_1');
    vi.stubGlobal(
      'fetch',
      vi.fn(async () => ({
        ok: true,
        status: 200,
        json: async () => ({ ultraplan: state, demo: { port: 41000, token: TOKEN, entry: 'index.html' } }),
      }) as Response),
    );
    useWorkbenchStore.getState().openDemo(UP, 'sess_1', '塔防');
    render(<Workbench />);
    expect(screen.getByTestId('workbench-tab-demo:up_a1')).toHaveTextContent('Demo · 塔防');
    expect(screen.getByTestId('demo-tab')).toBeInTheDocument();
    const frame = await screen.findByTestId('demo-tab-frame');
    expect(frame).toHaveAttribute('src', `http://127.0.0.1:41000/u/${TOKEN}/?v=1`);
    // 关掉页签即卸载 iframe
    fireEvent.click(screen.getByLabelText('关闭 Demo · 塔防'));
    expect(screen.queryByTestId('demo-tab')).not.toBeInTheDocument();
    expect(document.querySelector('iframe')).toBeNull();
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

  it('旧状态别名显示对应圆圈并分列,失败重试事件把卡片移回制作中后完成', () => {
    useChatStore.setState({
      todos: [
        { id: 'queued-task', title: '待办项', status: 'queued' },
        { id: 'running-task', title: '制作项', status: 'in_progress' },
        { id: 'failed-task', title: '重试项', status: 'blocked' },
        { id: 'done-task', title: '完成项', status: 'done' },
      ],
    });
    render(<TodoTab />);
    const states = [
      ['Backlog', 'queued-task', '待办', 'pending'],
      ['Running', 'running-task', '制作中', 'running'],
      ['Review', 'failed-task', '失败', 'failed'],
      ['Done', 'done-task', '完成', 'completed'],
    ];
    for (const [column, id, label, status] of states) {
      const card = within(screen.getByTestId(`todo-col-${column}`)).getByTestId(`todo-card-${id}`);
      expect(within(card).getByRole('img', { name: label })).toHaveAttribute('data-todo-status', status);
    }

    const update = (status: string, seq: number) => act(() => useChatStore.getState().applyEvent({
      id: `e-retry-${seq}`,
      sessionId: 'sess_1',
      seq,
      type: 'todo.updated',
      ts: '2026-09-03T10:00:02.000Z',
      payload: { id: 'failed-task', status },
    }));
    update('running', 1);
    expect(within(screen.getByTestId('todo-col-Review')).queryByTestId('todo-card-failed-task')).toBeNull();
    const retryCard = within(screen.getByTestId('todo-col-Running')).getByTestId('todo-card-failed-task');
    expect(retryCard).toHaveTextContent('重试项');
    expect(within(retryCard).getByRole('img', { name: '制作中' })).toHaveAttribute('data-todo-status', 'running');

    update('completed', 2);
    expect(within(screen.getByTestId('todo-col-Running')).queryByTestId('todo-card-failed-task')).toBeNull();
    const doneCard = within(screen.getByTestId('todo-col-Done')).getByTestId('todo-card-failed-task');
    expect(within(doneCard).getByRole('img', { name: '完成' })).toHaveAttribute('data-todo-status', 'completed');
    expect(screen.getAllByTestId('todo-card-failed-task')).toHaveLength(1);
  });

  it('来源标记取 todo.source(计划/用户),没有来源的不画;不再给每张卡写死「铸」', () => {
    useChatStore.setState({
      todos: [
        { id: 'p1', title: '计划项', status: 'queued', source: 'plan' },
        { id: 'u1', title: '手动项', status: 'queued', source: 'user' },
        { id: 'a1', title: 'Agent 项', status: 'queued' },
      ],
    });
    render(<TodoTab />);
    expect(screen.getByTestId('todo-source-p1')).toHaveTextContent('计划');
    expect(screen.getByTestId('todo-source-u1')).toHaveTextContent('用户');
    expect(screen.queryByTestId('todo-source-a1')).not.toBeInTheDocument();
    expect(screen.getByTestId('todo-card-a1')).not.toHaveTextContent('铸');
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
