import { act, cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import UltraPlanBlock from '@/components/chat/ultraplan/UltraPlanBlock';
import { useChatStore } from '@/lib/chatStore';
import { usePlanStore } from '@/lib/planStore';
import { useSessionStore } from '@/lib/sessionStore';
import { useToastStore } from '@/lib/toastStore';
import type { ChatBlock } from '@/lib/timeline';
import { useUltraPlanStore, type UltraPlanState } from '@/lib/ultraPlanStore';
import { useWorkbenchStore } from '@/lib/workbenchStore';

/**
 * D-044 Demo 卡:迭代号、自动验证徽标 + 说明、探测报错折叠、探测截图只显示路径(不拉取)、
 * 「打开 Demo」开页签、可操作时的通过 / 提出修改请求体(契约 §2)、未自动验证的措辞、
 * 已提交与各种只读形态的说明、有任务在跑时禁用。后端未实现:fetch 桩。
 */

type Ultra = Extract<ChatBlock, { kind: 'ultraplan' }>;

const SID = 'sess_1';
const UP = 'up_a1';
const ASK_URL = `/api/forge/sessions/${SID}/ask:execute`;
const SHOT = '.forge/ultraplan/td-a1b2/probe/iter-2.png';

function flow(patch: Partial<UltraPlanState> = {}): UltraPlanState {
  return {
    id: UP,
    token: '0123456789abcdef0123456789abcdef',
    slug: 'td-a1b2',
    dir: '.forge/ultraplan/td-a1b2',
    title: '塔防',
    workspaceId: null,
    stage: 'demo_review',
    phase: 'waiting',
    running: null,
    lastError: null,
    questionnaireRev: 1,
    demoIteration: 2,
    demoVerified: true,
    demoNote: null,
    planPath: null,
    planRev: 0,
    planHash: null,
    productionRunId: null,
    acceptanceRound: 0,
    createdAt: '2026-09-30T08:00:00Z',
    updatedAt: '2026-09-30T08:00:00Z',
    ...patch,
  };
}

function card(patch: Partial<Ultra> = {}, payload: Record<string, unknown> = {}): Ultra {
  return {
    kind: 'ultraplan',
    step: 'demo',
    upId: UP,
    rev: 2,
    payload: {
      runId: 'run_s',
      id: UP,
      iteration: 2,
      entry: 'index.html',
      verified: true,
      probe: { ok: true, errors: [], screenshot: SHOT },
      ...payload,
    },
    ...patch,
  };
}

interface Call {
  url: string;
  method: string;
  body: unknown;
}

let calls: Call[] = [];

function stubFetch(status = 200, body: unknown = { run: { id: 'run_s', status: 'completed' } }) {
  vi.stubGlobal(
    'fetch',
    vi.fn(async (url: unknown, init?: RequestInit) => {
      const call: Call = {
        url: String(url),
        method: init?.method ?? 'GET',
        body: init?.body ? JSON.parse(String(init.body)) : undefined,
      };
      calls.push(call);
      const reply = call.method === 'GET' ? { ultraplan: useUltraPlanStore.getState().state } : body;
      const code = call.method === 'GET' ? 200 : status;
      return { ok: code < 400, status: code, json: async () => reply } as Response;
    }),
  );
}

const asks = () => calls.filter((c) => c.url === ASK_URL);

const initialChat = useChatStore.getState();
const initialSessions = useSessionStore.getState();
const initialToasts = useToastStore.getState();
const initialUltra = useUltraPlanStore.getState();
const initialWorkbench = useWorkbenchStore.getState();

beforeEach(() => {
  calls = [];
  useUltraPlanStore.setState(initialUltra, true);
  useChatStore.setState(initialChat, true);
  useChatStore.getState().reset();
  useSessionStore.setState(initialSessions, true);
  useToastStore.setState(initialToasts, true);
  useWorkbenchStore.setState(initialWorkbench, true);
  usePlanStore.getState().reset();
  useSessionStore.setState({ activeSessionId: SID });
  useUltraPlanStore.getState().hydrate(flow(), SID);
  stubFetch();
});

afterEach(() => {
  cleanup();
  vi.unstubAllGlobals();
});

describe('<DemoCard /> 展示', () => {
  it('经 UltraPlanBlock 分发;迭代号、已自动验证徽标、截图只显示路径(不拉取)、手动试玩提醒', () => {
    render(<UltraPlanBlock block={card()} />);
    expect(screen.getByTestId('ultraplan-block')).toHaveAttribute('data-step', 'demo');
    const root = screen.getByTestId('demo-card');
    expect(root).toHaveTextContent('Demo · 第 2 版');
    const badge = screen.getByTestId('demo-card-verified');
    expect(badge).toHaveTextContent('已自动验证');
    expect(badge).toHaveAttribute('data-verified', '1');
    expect(screen.getByTestId('demo-card-screenshot')).toHaveTextContent(SHOT);
    expect(root.querySelector('img')).toBeNull();
    expect(calls.some((c) => c.url.includes('iter-2.png'))).toBe(false);
    expect(root).toHaveTextContent('自动验证包含键鼠操作，请试玩确认手感与整体体验');
    expect(screen.queryByTestId('demo-card-probe-errors')).not.toBeInTheDocument();
  });

  it('未自动验证:徽标 warn 色 + 说明(悬停与正文都有);探测报错可展开;回退来源与探测不可用', () => {
    render(
      <UltraPlanBlock
        block={card(
          { rev: 2 },
          {
            verified: false,
            note: '已回退到第 1 版(未重新验证)',
            rollbackOf: 1,
            probe: { ok: false, errors: ['ReferenceError: foo'], unavailable: true },
          },
        )}
      />,
    );
    const badge = screen.getByTestId('demo-card-verified');
    expect(badge).toHaveTextContent('未自动验证');
    expect(badge).toHaveAttribute('title', '已回退到第 1 版(未重新验证)');
    expect(badge.className).toContain('text-warn');
    expect(badge.className).not.toContain('danger');
    expect(screen.getByTestId('demo-card-note')).toHaveTextContent('已回退到第 1 版(未重新验证)');
    const root = screen.getByTestId('demo-card');
    expect(root).toHaveTextContent('由第 1 版回退');
    expect(root).toHaveTextContent('探测环境不可用');
    expect(screen.queryByTestId('demo-card-screenshot')).not.toBeInTheDocument();
    fireEvent.click(screen.getByTestId('demo-card-probe-errors-toggle'));
    expect(screen.getByTestId('demo-card-probe-errors-list')).toHaveTextContent('ReferenceError: foo');
    expect(screen.getByTestId('demo-card-approve')).toHaveTextContent('未自动验证,仍然通过');
  });

  it('「打开 Demo」开(或激活)当前流程的 Demo 页签,带会话 id;不发请求', () => {
    render(<UltraPlanBlock block={card()} />);
    fireEvent.click(screen.getByTestId('demo-card-open'));
    const wb = useWorkbenchStore.getState();
    expect(wb.activeTabId).toBe(`demo:${UP}`);
    expect(wb.tabs).toEqual([{ id: `demo:${UP}`, kind: 'demo', title: 'Demo · 塔防', upId: UP, sessionId: SID }]);
    fireEvent.click(screen.getByTestId('demo-card-open'));
    expect(useWorkbenchStore.getState().tabs).toHaveLength(1);
    expect(calls.filter((c) => c.method === 'POST')).toHaveLength(0);
  });
});

describe('<DemoCard /> 可操作', () => {
  it('通过 Demo → approve_demo(卡自己的 id + rev)', async () => {
    render(<UltraPlanBlock block={card()} />);
    expect(screen.getByTestId('demo-card')).toHaveAttribute('data-interactive', '1');
    const approve = screen.getByTestId('demo-card-approve');
    expect(approve).toHaveTextContent('通过 Demo');
    fireEvent.click(approve);
    await waitFor(() => expect(asks()).toHaveLength(1));
    expect(asks()[0].body).toEqual({
      userInput: '',
      mode: 'ultraplan',
      ultraplan: { id: UP, rev: 2, action: 'approve_demo' },
    });
    expect(await screen.findByTestId('demo-card-decision-note')).toHaveTextContent('已提交,等待处理');
  });

  it('提出修改 → revise_demo,意见作 userInput;空意见不发', async () => {
    render(<UltraPlanBlock block={card()} />);
    fireEvent.click(screen.getByTestId('demo-card-revise-toggle'));
    expect(screen.getByTestId('demo-card-revise-submit')).toBeDisabled();
    fireEvent.change(screen.getByTestId('demo-card-revise-input'), { target: { value: '加一个暂停键' } });
    fireEvent.click(screen.getByTestId('demo-card-revise-submit'));
    await waitFor(() => expect(asks()).toHaveLength(1));
    expect(asks()[0].body).toEqual({
      userInput: '加一个暂停键',
      mode: 'ultraplan',
      ultraplan: { id: UP, rev: 2, action: 'revise_demo' },
    });
  });

  it('有任务在跑:控件留着但禁用,title 说明原因;结束后恢复', () => {
    useChatStore.setState({ activeRunId: 'run_9' });
    render(<UltraPlanBlock block={card()} />);
    expect(screen.getByTestId('demo-card')).not.toHaveAttribute('data-interactive');
    for (const id of ['demo-card-approve', 'demo-card-revise-toggle']) {
      expect(screen.getByTestId(id)).toBeDisabled();
      expect(screen.getByTestId(id)).toHaveAttribute('title', '有任务正在运行');
    }
    fireEvent.click(screen.getByTestId('demo-card-approve'));
    expect(asks()).toHaveLength(0);
    act(() => useChatStore.setState({ activeRunId: null }));
    expect(screen.getByTestId('demo-card-approve')).toBeEnabled();
    expect(screen.getByTestId('demo-card')).toHaveAttribute('data-interactive', '1');
  });

  it('流程在跑(phase=running):禁用并说明', () => {
    useUltraPlanStore.getState().hydrate(flow({ phase: 'running', running: 'spec_demo' }), SID);
    render(<UltraPlanBlock block={card()} />);
    expect(screen.getByTestId('demo-card-approve')).toBeDisabled();
    expect(screen.getByTestId('demo-card-approve').getAttribute('title')).toContain('正在构建 Demo');
  });
});

describe('<DemoCard /> 已提交与只读', () => {
  it('已提交:通过 → 「已通过」;要求修改 → 「已要求修改:<意见>」;没有动作', () => {
    const { rerender } = render(
      <UltraPlanBlock block={card({ submitted: { runId: 'run_p', id: UP, iteration: 2, decision: 'approve' } })} />,
    );
    expect(screen.getByTestId('demo-card-submitted')).toHaveTextContent('已通过');
    expect(screen.queryByTestId('demo-card-approve')).not.toBeInTheDocument();
    rerender(
      <UltraPlanBlock
        block={card({ submitted: { runId: 'run_r', id: UP, iteration: 2, decision: 'revise', feedback: '敌人太快' } })}
      />,
    );
    expect(screen.getByTestId('demo-card-submitted')).toHaveTextContent('已要求修改:敌人太快');
    expect(screen.queryByTestId('demo-card-revise-toggle')).not.toBeInTheDocument();
    // 已提交的卡仍可打开 Demo 看
    expect(screen.getByTestId('demo-card-open')).toBeInTheDocument();
  });

  it('旧版 / 关口已过 / 别的流程:只读并给原因', () => {
    const { rerender } = render(<UltraPlanBlock block={card({ rev: 1 }, { iteration: 1 })} />);
    expect(screen.getByTestId('demo-card-readonly')).toHaveTextContent('已有更新的 Demo(第 2 版)');
    expect(screen.queryByTestId('demo-card-approve')).not.toBeInTheDocument();

    act(() => useUltraPlanStore.getState().hydrate(flow({ stage: 'plan_review', planRev: 1 }), SID));
    rerender(<UltraPlanBlock block={card()} />);
    expect(screen.getByTestId('demo-card-readonly')).toHaveTextContent('流程已进入「计划」阶段');

    rerender(<UltraPlanBlock block={card({ upId: 'up_old' })} />);
    expect(screen.getByTestId('demo-card-readonly')).toHaveTextContent('此流程属于其他会话,或已被重新开始');
    // 别的流程的卡不给「打开 Demo」(它的 Demo 已不在服务)
    expect(screen.queryByTestId('demo-card-open')).not.toBeInTheDocument();
  });

  it('没有会话 / 流程(回放残留):只读、不抛', () => {
    useUltraPlanStore.getState().hydrate(null, SID);
    render(<UltraPlanBlock block={card({}, { verified: 'yes', probe: 'bad' })} />);
    expect(screen.getByTestId('demo-card-readonly')).toBeInTheDocument();
    expect(screen.getByTestId('demo-card-verified')).toHaveTextContent('未自动验证');
    expect(screen.queryByTestId('demo-card-open')).not.toBeInTheDocument();
  });
});
