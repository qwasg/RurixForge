import { act, cleanup, fireEvent, render, screen } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import PlanTab, { lastPlanText } from '@/components/workbench/PlanTab';
import ProposalsTab from '@/components/workbench/ProposalsTab';
import TodoTab from '@/components/workbench/TodoTab';
import { useChatStore, type ChatMsg } from '@/lib/chatStore';
import { useComposerPrefillStore } from '@/lib/composerStore';

/**
 * F7 wave.5 workbench tabs:Plan(plan 空态/有稿/开始 Build 预填)+ Todo(四列看板分列)
 * + 提案(列表渲染/impact 展开/批准拒绝接线)。
 */

const initialChat = useChatStore.getState();
const initialPrefill = useComposerPrefillStore.getState();

beforeEach(() => {
  useChatStore.setState(initialChat, true);
  useChatStore.getState().reset();
  useComposerPrefillStore.setState(initialPrefill, true);
  globalThis.localStorage?.clear();
});

afterEach(() => {
  cleanup();
  vi.unstubAllGlobals();
});

function msg(partial: Partial<ChatMsg> & Pick<ChatMsg, 'id' | 'role'>): ChatMsg {
  return { text: '', blocks: [], time: '', ...partial };
}

describe('Plan tab', () => {
  it('空态:无 plan 模式消息 → 「尚无计划」+ 开始 Build 禁用', () => {
    render(<PlanTab />);
    expect(screen.getByText('尚无计划')).toBeInTheDocument();
    expect(screen.getByTestId('plan-start-build')).toBeDisabled();
  });

  it('lastPlanText:取最近 plan 模式 turn 的 assistant 终稿', () => {
    const messages: ChatMsg[] = [
      msg({ id: 'u1', role: 'user', text: '做个迷宫', mode: 'build', runId: 'r1' }),
      msg({ id: 'a1', role: 'assistant', runId: 'r1', blocks: [{ kind: 'text', text: 'build 答复', final: true }] }),
      msg({ id: 'u2', role: 'user', text: '规划一下', mode: 'plan', runId: 'r2' }),
      msg({ id: 'a2', role: 'assistant', runId: 'r2', blocks: [{ kind: 'text', text: '# 计划\n第一步', final: true }] }),
    ];
    expect(lastPlanText(messages)).toBe('# 计划\n第一步');
    // 非 plan 模式不取
    expect(lastPlanText(messages.slice(0, 2))).toBeNull();
  });

  it('有稿:渲染终稿 Markdown + To-dos 表;开始 Build → composer 预填 build+「执行上述计划」', () => {
    useChatStore.setState({
      messages: [
        msg({ id: 'u2', role: 'user', text: '规划一下', mode: 'plan', runId: 'r2' }),
        msg({ id: 'a2', role: 'assistant', runId: 'r2', blocks: [{ kind: 'text', text: '计划正文', final: true }] }),
      ],
      todos: [
        { id: 'todo_1', title: '搭场景', status: 'completed' },
        { id: 'todo_2', title: '写逻辑', status: 'queued' },
      ],
    });
    render(<PlanTab />);
    expect(screen.getByText('计划正文')).toBeInTheDocument();
    expect(screen.getByTestId('plan-todo-table')).toHaveTextContent('2 To-dos');
    // 完成划线
    expect(screen.getByTestId('plan-todo-todo_1').querySelector('.line-through')).not.toBeNull();
    const btn = screen.getByTestId('plan-start-build');
    expect(btn).toBeEnabled();
    fireEvent.click(btn);
    // 预填已写入(Composer 挂载时才消费 clear;此处只验 store 面)
    expect(useComposerPrefillStore.getState().draft).toBe('执行上述计划');
    expect(useComposerPrefillStore.getState().mode).toBe('build');
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
