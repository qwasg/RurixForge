import { act, cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import UltraPlanBar, { contextAction } from '@/components/chat/UltraPlanBar';
import { useChatStore } from '@/lib/chatStore';
import { useSessionStore } from '@/lib/sessionStore';
import { useToastStore } from '@/lib/toastStore';
import { useUltraPlanStore, type UltraPlanState } from '@/lib/ultraPlanStore';
import { useWorkbenchStore } from '@/lib/workbenchStore';

/**
 * D-044 UltraPlan 状态条:何时出现、六步进度、阶段说明(在跑 / 在等 / 失败)、按关口给的唯一
 * 上下文动作及其请求体(契约 §2 / §3)、「重新开始」二次确认、有任务在跑时全部禁用、
 * 引擎 / 代理不支持时只读。后端未实现:ask:execute 与 …/ultraplan 全部用 fetch 桩。
 */

const SID = 'sess_1';
const UP = 'up_a1';
const BASE = `/api/forge/sessions/${SID}`;
const ASK_URL = `${BASE}/ask:execute`;
const FACE_URL = `${BASE}/ultraplan`;
const RESTART_URL = `${BASE}/ultraplan/restart`;
const PLAN_PATH = '.forge/plans/td-a1b2.plan.md';

function flow(patch: Partial<UltraPlanState> = {}): UltraPlanState {
  return {
    id: UP,
    token: '0123456789abcdef0123456789abcdef',
    slug: 'td-a1b2',
    dir: '.forge/ultraplan/td-a1b2',
    title: '塔防',
    workspaceId: null,
    stage: 'questionnaire',
    phase: 'waiting',
    running: null,
    lastError: null,
    questionnaireRev: 2,
    demoIteration: 0,
    demoVerified: false,
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

/** 计划已出(plan_review 及之后的关口共用的字段)。 */
const planned: Partial<UltraPlanState> = { demoIteration: 1, demoVerified: true, planPath: PLAN_PATH, planRev: 3 };

interface Call {
  url: string;
  method: string;
  body: unknown;
  hasBody: boolean;
}
type Reply = { status?: number; body: unknown };
type Handler = (call: Call) => Reply | Promise<Reply>;

let calls: Call[] = [];
/** GET …/ultraplan 的当前应答(被拒 / 重新开始后 store 会重拉)。 */
let face: { ultraplan: UltraPlanState | null; answers?: unknown } = { ultraplan: null };

function stubFetch(handler: Handler = () => ({ body: { run: { id: 'run_s', status: 'completed' } } })) {
  vi.stubGlobal(
    'fetch',
    vi.fn(async (url: unknown, init?: RequestInit) => {
      const call: Call = {
        url: String(url),
        method: init?.method ?? 'GET',
        body: init?.body ? JSON.parse(String(init.body)) : undefined,
        hasBody: init?.body !== undefined,
      };
      calls.push(call);
      const reply = call.method === 'GET' ? { body: face } : await handler(call);
      const status = reply.status ?? 200;
      return { ok: status < 400, status, json: async () => reply.body } as Response;
    }),
  );
}

const posts = (url: string) => calls.filter((c) => c.method === 'POST' && c.url === url);

/** 把流程挂到当前会话上(等同快照回填);face 同步,免得重拉把它冲掉。 */
function mount(state: UltraPlanState | null, sessionId: string = SID) {
  face = { ultraplan: sessionId === SID ? state : null };
  useUltraPlanStore.getState().hydrate(state, sessionId);
}

const session = (agentKind: string, agentEngine: 'local' | 'codex') => ({
  id: SID, title: 'x', status: 'idle', agentKind, agentEngine,
  selectedModelId: 'mock', thinkingEnabled: false, reasoningEffort: null,
  contextOptionId: null, webSearchEnabled: false, activeRunId: null,
  createdAt: '', updatedAt: '', pinned: false, titleManuallySet: false,
});

const initialChat = useChatStore.getState();
const initialSessions = useSessionStore.getState();
const initialToasts = useToastStore.getState();
const initialUltra = useUltraPlanStore.getState();

beforeEach(() => {
  calls = [];
  face = { ultraplan: null };
  useUltraPlanStore.setState(initialUltra, true);
  useChatStore.setState(initialChat, true);
  // chat reset() 连带清 ultraPlanStore 的闭包态(在途槽 / 代次)
  useChatStore.getState().reset();
  useSessionStore.setState(initialSessions, true);
  useToastStore.setState(initialToasts, true);
  useSessionStore.setState({ activeSessionId: SID });
  stubFetch();
});

afterEach(() => {
  cleanup();
  vi.unstubAllGlobals();
});

describe('<UltraPlanBar /> 何时出现', () => {
  it('没有流程 / 流程属于别的会话 / 已完成:不渲染;完成态下失败仍显示', () => {
    const { rerender } = render(<UltraPlanBar />);
    expect(screen.queryByTestId('ultraplan-bar')).not.toBeInTheDocument();

    // store 里挂着别的会话的流程(切会话的瞬间)→ 当作没有
    act(() => mount(flow(), 'sess_other'));
    rerender(<UltraPlanBar />);
    expect(screen.queryByTestId('ultraplan-bar')).not.toBeInTheDocument();

    act(() => mount(flow()));
    const bar = screen.getByTestId('ultraplan-bar');
    expect(bar).toHaveAttribute('data-stage', 'questionnaire');
    expect(bar).toHaveAttribute('data-phase', 'waiting');

    act(() => mount(flow({ ...planned, stage: 'done', acceptanceRound: 1 })));
    expect(screen.queryByTestId('ultraplan-bar')).not.toBeInTheDocument();

    act(() =>
      mount(flow({ ...planned, stage: 'done', phase: 'failed', lastError: { code: 'X', message: '收尾失败' } })),
    );
    expect(screen.getByTestId('ultraplan-bar')).toHaveAttribute('data-phase', 'failed');
  });
});

describe('<UltraPlanBar /> 进度与阶段说明', () => {
  it('六步:需求 → 问卷 → Demo → 计划 → 制作 → 验收;当前步高亮,之前为已过、之后为未到', () => {
    mount(flow({ stage: 'demo_review', demoIteration: 1 }));
    render(<UltraPlanBar />);
    const steps = screen.getByTestId('ultraplan-bar-steps');
    const items = Array.from(steps.querySelectorAll('[data-testid^="ultraplan-step-"]'));
    expect(items.map((el) => el.textContent)).toEqual(['需求', '问卷', 'Demo', '计划', '制作', '验收']);
    expect(items.map((el) => el.getAttribute('data-state'))).toEqual([
      'done',
      'done',
      'current',
      'todo',
      'todo',
      'todo',
    ]);
    const current = screen.getByTestId('ultraplan-step-demo_review');
    expect(current).toHaveAttribute('aria-current', 'step');
    expect(current.className).toContain('text-acc');
    expect(screen.getByTestId('ultraplan-step-questionnaire')).not.toHaveAttribute('aria-current');
  });

  it('在等 / 在跑 / 失败三种说明;失败用 warn 色(不用 danger),原因在悬停提示里', () => {
    mount(flow());
    render(<UltraPlanBar />);
    const phase = screen.getByTestId('ultraplan-bar-phase');
    expect(phase).toHaveTextContent('等待你填写问卷');
    expect(phase.querySelector('.animate-spin')).toBeNull();
    expect(phase).not.toHaveAttribute('title');

    act(() => mount(flow({ phase: 'running', running: 'spec_demo' })));
    expect(phase).toHaveTextContent('正在构建 Demo…');
    expect(phase.querySelector('.animate-spin')).not.toBeNull();
    // 在跑时没有上下文动作
    expect(screen.queryByTestId('ultraplan-bar-action')).not.toBeInTheDocument();

    act(() =>
      mount(
        flow({
          phase: 'failed',
          lastError: { code: 'ULTRAPLAN_DEMO_BUILD_FAILED', message: '构建子代理超时' },
        }),
      ),
    );
    expect(phase).toHaveTextContent('失败 · Demo 构建中断');
    expect(phase).toHaveAttribute('title', '构建子代理超时');
    expect(phase.className).toContain('text-warn');
    expect(phase.className).not.toContain('danger');
    expect(phase.querySelector('.animate-spin')).toBeNull();
    // 当前步同样转 warn 色
    const current = screen.getByTestId('ultraplan-step-questionnaire');
    expect(current.className).toContain('text-warn');
    expect(current.className).not.toContain('danger');
  });

  it('「深度思考 · <effort>」:只在实时 ultraplan.stage(running)带了思考规格时显示', () => {
    mount(flow({ stage: 'discovery', questionnaireRev: 0, phase: 'running', running: 'discovery' }));
    render(<UltraPlanBar />);
    expect(screen.queryByTestId('ultraplan-bar-thinking')).not.toBeInTheDocument();

    const stage = (payload: Record<string, unknown>) =>
      act(() =>
        useUltraPlanStore.getState().applyLiveEvent({
          sessionId: SID,
          type: 'ultraplan.stage',
          payload: { id: UP, stage: 'discovery', running: 'discovery', ...payload },
        }),
      );
    stage({ phase: 'running', effort: 'high', thinkingForced: true });
    expect(screen.getByTestId('ultraplan-bar-thinking')).toHaveTextContent('深度思考 · high');
    // 只强制了思考、没有档位
    stage({ phase: 'running', effort: null, thinkingForced: true });
    expect(screen.getByTestId('ultraplan-bar-thinking')).toHaveTextContent('深度思考');
    expect(screen.getByTestId('ultraplan-bar-thinking')).not.toHaveTextContent('·');
    // 这一轮没带思考规格 / turn 结束 → 收起
    stage({ phase: 'running' });
    expect(screen.queryByTestId('ultraplan-bar-thinking')).not.toBeInTheDocument();
    stage({ phase: 'running', effort: 'max' });
    expect(screen.getByTestId('ultraplan-bar-thinking')).toHaveTextContent('深度思考 · max');
    stage({ phase: 'waiting' });
    expect(screen.queryByTestId('ultraplan-bar-thinking')).not.toBeInTheDocument();
  });

  it('实时 ultraplan.notice 汇成一个提示角标(原文在悬停提示里)', () => {
    mount(flow({ stage: 'discovery', questionnaireRev: 0 }));
    render(<UltraPlanBar />);
    expect(screen.queryByTestId('ultraplan-bar-notice')).not.toBeInTheDocument();
    act(() =>
      useUltraPlanStore.getState().applyLiveEvent({
        sessionId: SID,
        type: 'ultraplan.notice',
        payload: { runId: 'run_d', id: UP, code: 'THINKING_UNAVAILABLE', message: '当前模型没有思考档位' },
      }),
    );
    const notice = screen.getByTestId('ultraplan-bar-notice');
    expect(notice).toHaveTextContent('提示 1');
    expect(notice).toHaveAttribute('title', '当前模型没有思考档位');
  });
});

describe('<UltraPlanBar /> 上下文动作', () => {
  // 6b 起 Demo 关口有「打开 Demo」(见下一条),从这张「没有动作」的清单里移出。
  it('等待中的问卷 / 计划 / 验收关口:没有上下文动作,只有「重新开始」', () => {
    mount(flow());
    render(<UltraPlanBar />);
    for (const state of [
      flow(),
      flow({ stage: 'discovery', questionnaireRev: 0 }),
      flow({ ...planned, stage: 'plan_review' }),
      flow({ ...planned, stage: 'acceptance', acceptanceRound: 1 }),
    ]) {
      act(() => mount(state));
      expect(screen.queryByTestId('ultraplan-bar-action')).not.toBeInTheDocument();
      expect(screen.getByTestId('ultraplan-bar-restart')).toBeEnabled();
    }
  });

  it('Demo 关口(等待 / 失败):「打开 Demo」开当前流程的 Demo 页签,不发请求;有任务在跑也可点;流程在跑时不给', () => {
    useWorkbenchStore.setState({ tabs: [], activeTabId: null });
    mount(flow({ stage: 'demo_review', demoIteration: 2 }));
    render(<UltraPlanBar />);
    const action = screen.getByTestId('ultraplan-bar-action');
    expect(action).toHaveTextContent('打开 Demo');
    expect(action).toHaveAttribute('data-action', 'open-demo');
    // 只是看:会话里有任务在跑也不禁用
    act(() => useChatStore.setState({ activeRunId: 'run_9' }));
    expect(action).toBeEnabled();
    fireEvent.click(action);
    const wb = useWorkbenchStore.getState();
    expect(wb.activeTabId).toBe(`demo:${UP}`);
    expect(wb.tabs).toEqual([
      { id: `demo:${UP}`, kind: 'demo', title: 'Demo · 塔防', upId: UP, sessionId: SID },
    ]);
    expect(calls.filter((c) => c.method === 'POST')).toHaveLength(0);
    // 「重新开始」照常受「有任务在跑」约束
    expect(screen.getByTestId('ultraplan-bar-restart')).toBeDisabled();

    act(() => useChatStore.setState({ activeRunId: null }));
    act(() => mount(flow({ stage: 'demo_review', demoIteration: 2, phase: 'failed', lastError: { code: 'X', message: '改版失败' } })));
    expect(screen.getByTestId('ultraplan-bar-action')).toHaveAttribute('data-action', 'open-demo');
    act(() => mount(flow({ stage: 'demo_review', demoIteration: 2, phase: 'running', running: 'planning' })));
    expect(screen.queryByTestId('ultraplan-bar-action')).not.toBeInTheDocument();
  });

  it('问卷关口失败 → 重试 = 不带答案重发 answer(后端复用已存答案)', async () => {
    mount(flow({ phase: 'failed', lastError: { code: 'ULTRAPLAN_DEMO_BUILD_FAILED', message: '构建失败' } }));
    render(<UltraPlanBar />);
    const action = screen.getByTestId('ultraplan-bar-action');
    expect(action).toHaveTextContent('重试');
    expect(action).toHaveAttribute('data-action', 'retry-answer');
    fireEvent.click(action);
    await waitFor(() => expect(posts(ASK_URL)).toHaveLength(1));
    expect(posts(ASK_URL)[0].body).toEqual({
      userInput: '',
      mode: 'ultraplan',
      ultraplan: { id: UP, rev: 2, action: 'answer' },
    });
  });

  it('制作关口:等待中 → 继续制作;失败 → 重试;都是 team 模式的 resume_production(不带 rev)', async () => {
    mount(flow({ ...planned, stage: 'production', productionRunId: 'run_p' }));
    render(<UltraPlanBar />);
    const resume = screen.getByTestId('ultraplan-bar-action');
    expect(resume).toHaveTextContent('继续制作');
    expect(resume).toHaveAttribute('data-action', 'resume');
    fireEvent.click(resume);
    await waitFor(() => expect(posts(ASK_URL)).toHaveLength(1));
    const body = { userInput: '', mode: 'team', ultraplan: { id: UP, action: 'resume_production' } };
    expect(posts(ASK_URL)[0].body).toEqual(body);

    act(() =>
      mount(
        flow({
          ...planned,
          stage: 'production',
          phase: 'failed',
          lastError: { code: 'ULTRAPLAN_PRODUCTION_INCOMPLETE', message: '还有 2 个任务未完成' },
        }),
      ),
    );
    await waitFor(() => expect(screen.getByTestId('ultraplan-bar-action')).toBeEnabled());
    const retry = screen.getByTestId('ultraplan-bar-action');
    expect(retry).toHaveTextContent('重试');
    expect(retry).toHaveAttribute('data-action', 'retry-resume');
    expect(screen.getByTestId('ultraplan-bar-phase')).toHaveTextContent('失败 · 制作中断');
    fireEvent.click(retry);
    await waitFor(() => expect(posts(ASK_URL)).toHaveLength(2));
    expect(posts(ASK_URL)[1].body).toEqual(body);
  });

  it('计划关口失败且断的是制作起步 → 重试 = start_production(team 模式 + planRev + planPath)', async () => {
    mount(flow({ ...planned, stage: 'plan_review', phase: 'failed', running: 'production' }));
    render(<UltraPlanBar />);
    const action = screen.getByTestId('ultraplan-bar-action');
    expect(action).toHaveAttribute('data-action', 'retry-start');
    fireEvent.click(action);
    await waitFor(() => expect(posts(ASK_URL)).toHaveLength(1));
    expect(posts(ASK_URL)[0].body).toEqual({
      userInput: '',
      mode: 'team',
      ultraplan: { id: UP, rev: 3, action: 'start_production' },
      planPath: PLAN_PATH,
    });
  });

  it('contextAction:只有「重发哪个动作」没有歧义时才给重试', () => {
    const failed = (patch: Partial<UltraPlanState>) => flow({ phase: 'failed', ...patch });
    // 问卷关口:断在 Demo 构建 → 重试;断在重出问卷(要用户原文)→ 不给
    expect(contextAction(failed({ running: 'spec_demo' }), false)?.id).toBe('retry-answer');
    expect(contextAction(failed({ lastError: { code: 'ULTRAPLAN_SPEC_MISSING', message: '' } }), false)?.id).toBe(
      'retry-answer',
    );
    expect(contextAction(failed({ running: 'discovery' }), true)).toBeNull();
    // 看不出断在哪:答案已落库才给(否则让用户在问卷卡上重新提交)
    expect(contextAction(failed({ lastError: { code: 'PROVIDER_ERROR', message: '' } }), true)?.id).toBe(
      'retry-answer',
    );
    expect(contextAction(failed({ lastError: { code: 'PROVIDER_ERROR', message: '' } }), false)).toBeNull();
    // 计划关口:断在改计划(要修改意见原文)→ 不给;绝不把它重试成开始制作
    expect(contextAction(failed({ ...planned, stage: 'plan_review', running: 'planning' }), true)).toBeNull();
    expect(contextAction(failed({ ...planned, stage: 'plan_review' }), true)).toBeNull();
    expect(contextAction(failed({ ...planned, stage: 'plan_review', running: 'production' }), true)?.id).toBe(
      'retry-start',
    );
    // 其余关口的后续动作在各自卡片上;Demo 关口失败后给「打开 Demo」(重试要修改意见原文,在页签 / 卡片上)
    expect(contextAction(failed({ stage: 'discovery', questionnaireRev: 0 }), false)).toBeNull();
    expect(contextAction(failed({ stage: 'demo_review', demoIteration: 1 }), true)?.id).toBe('open-demo');
    expect(contextAction(flow({ stage: 'demo_review', demoIteration: 1, phase: 'running', running: 'planning' }), true)).toBeNull();
    expect(contextAction(failed({ ...planned, stage: 'acceptance', acceptanceRound: 1 }), true)).toBeNull();
    expect(contextAction(failed({ ...planned, stage: 'production' }), false)?.id).toBe('retry-resume');
    // 在跑时什么都不给
    expect(contextAction(flow({ ...planned, stage: 'production', phase: 'running', running: 'production' }), true)).toBeNull();
    expect(contextAction(flow({ ...planned, stage: 'production' }), false)).toEqual({ id: 'resume', label: '继续制作' });
  });

  it('问卷关口失败、看不出断在哪:重拉到已存答案后才出现重试', async () => {
    mount(flow({ phase: 'failed', lastError: { code: 'PROVIDER_ERROR', message: '上游 500' } }));
    render(<UltraPlanBar />);
    expect(screen.queryByTestId('ultraplan-bar-action')).not.toBeInTheDocument();
    face = { ...face, answers: { q_view: { choice: ['top'] } } };
    await act(async () => {
      await useUltraPlanStore.getState().refresh(SID);
    });
    expect(screen.getByTestId('ultraplan-bar-action')).toHaveAttribute('data-action', 'retry-answer');
  });

  it('被拒(409):不留在途态,原因走 warning toast,并重拉真实阶段', async () => {
    stubFetch(() => ({
      status: 409,
      body: {
        error: {
          code: 'ULTRAPLAN_STAGE_MISMATCH',
          message: '流程已进入验收阶段',
          details: { stage: 'acceptance', allowed: ['fix_production'] },
        },
      },
    }));
    mount(flow({ ...planned, stage: 'production' }));
    // 真实阶段其实已经到了验收(本地过期)
    face = { ultraplan: flow({ ...planned, stage: 'acceptance', acceptanceRound: 1 }) };
    render(<UltraPlanBar />);
    fireEvent.click(screen.getByTestId('ultraplan-bar-action'));
    await waitFor(() => expect(screen.getByTestId('ultraplan-bar')).toHaveAttribute('data-stage', 'acceptance'));
    expect(screen.queryByTestId('ultraplan-bar-action')).not.toBeInTheDocument();
    expect(useUltraPlanStore.getState().pending).toBeNull();
    const toast = useToastStore.getState().items.find((t) => t.title === '流程已进入验收阶段');
    expect(toast?.kind).toBe('warning');
    await waitFor(() => expect(screen.getByTestId('ultraplan-bar-restart')).toBeEnabled());
  });

  it('auto 权限下制作需要确认(ULTRAPLAN_NEEDS_BYPASS):就地问一句,确认后带 acknowledgeApprovals 重发', async () => {
    stubFetch((call) => {
      const ack = (call.body as { ultraplan?: { acknowledgeApprovals?: boolean } }).ultraplan?.acknowledgeApprovals;
      return ack
        ? { body: { run: { id: 'run_p', status: 'completed' } } }
        : { status: 409, body: { error: { code: 'ULTRAPLAN_NEEDS_BYPASS', message: '当前权限下制作需要逐项审批' } } };
    });
    mount(flow({ ...planned, stage: 'production' }));
    render(<UltraPlanBar />);
    fireEvent.click(screen.getByTestId('ultraplan-bar-action'));
    const confirm = await screen.findByTestId('ultraplan-bar-confirm');
    expect(confirm).toHaveAttribute('data-kind', 'ack');
    expect(confirm).toHaveTextContent('逐项确认');
    // 取消:不重发
    fireEvent.click(screen.getByTestId('ultraplan-bar-ack-cancel'));
    expect(screen.queryByTestId('ultraplan-bar-confirm')).not.toBeInTheDocument();
    expect(posts(ASK_URL)).toHaveLength(1);

    await waitFor(() => expect(screen.getByTestId('ultraplan-bar-action')).toBeEnabled());
    fireEvent.click(screen.getByTestId('ultraplan-bar-action'));
    await waitFor(() => expect(posts(ASK_URL)).toHaveLength(2));
    await waitFor(() => expect(screen.getByTestId('ultraplan-bar-ack-confirm')).toBeEnabled());
    fireEvent.click(screen.getByTestId('ultraplan-bar-ack-confirm'));
    await waitFor(() => expect(posts(ASK_URL)).toHaveLength(3));
    expect(posts(ASK_URL)[2].body).toEqual({
      userInput: '',
      mode: 'team',
      ultraplan: { id: UP, action: 'resume_production', acknowledgeApprovals: true },
    });
    await waitFor(() => expect(screen.queryByTestId('ultraplan-bar-confirm')).not.toBeInTheDocument());
  });
});

describe('<UltraPlanBar /> 重新开始', () => {
  it('点开先就地确认;取消不发请求;确认 → POST …/ultraplan/restart(无请求体)→ 流程清空、状态条收起', async () => {
    mount(flow());
    render(<UltraPlanBar />);
    const restart = screen.getByTestId('ultraplan-bar-restart');
    expect(restart).toHaveTextContent('重新开始');
    expect(restart).toHaveAttribute('aria-expanded', 'false');
    fireEvent.click(restart);
    expect(restart).toHaveAttribute('aria-expanded', 'true');
    const confirm = screen.getByTestId('ultraplan-bar-confirm');
    expect(confirm).toHaveAttribute('data-kind', 'restart');
    expect(confirm).toHaveTextContent('清除当前流程进度');
    expect(posts(RESTART_URL)).toHaveLength(0);

    fireEvent.click(screen.getByTestId('ultraplan-bar-restart-cancel'));
    expect(screen.queryByTestId('ultraplan-bar-confirm')).not.toBeInTheDocument();
    expect(posts(RESTART_URL)).toHaveLength(0);
    // 再点一次「重新开始」本身也能收起
    fireEvent.click(restart);
    fireEvent.click(restart);
    expect(screen.queryByTestId('ultraplan-bar-confirm')).not.toBeInTheDocument();

    fireEvent.click(restart);
    const ok = screen.getByTestId('ultraplan-bar-restart-confirm');
    // 确认钮是 warn 色,不用 danger 红
    expect(ok.className).toContain('text-warn');
    expect(ok.className).not.toContain('danger');
    face = { ultraplan: null };
    fireEvent.click(ok);
    await waitFor(() => expect(posts(RESTART_URL)).toHaveLength(1));
    expect(posts(RESTART_URL)[0].hasBody).toBe(false);
    await waitFor(() => expect(screen.queryByTestId('ultraplan-bar')).not.toBeInTheDocument());
    expect(useUltraPlanStore.getState().state).toBeNull();
    expect(posts(ASK_URL)).toHaveLength(0);
  });

  it('重新开始被拒(SESSION_BUSY):流程原样保留,提示「有任务正在运行」', async () => {
    stubFetch(() => ({ status: 409, body: { error: { code: 'SESSION_BUSY', message: 'session busy' } } }));
    mount(flow());
    render(<UltraPlanBar />);
    fireEvent.click(screen.getByTestId('ultraplan-bar-restart'));
    fireEvent.click(screen.getByTestId('ultraplan-bar-restart-confirm'));
    await waitFor(() => expect(posts(RESTART_URL)).toHaveLength(1));
    await waitFor(() => expect(screen.queryByTestId('ultraplan-bar-confirm')).not.toBeInTheDocument());
    expect(screen.getByTestId('ultraplan-bar')).toHaveAttribute('data-stage', 'questionnaire');
    expect(useToastStore.getState().items.some((t) => t.kind === 'warning' && t.title.includes('有任务正在运行'))).toBe(
      true,
    );
  });

  it('流程往前走了:挂着的确认随之作废', () => {
    mount(flow());
    render(<UltraPlanBar />);
    fireEvent.click(screen.getByTestId('ultraplan-bar-restart'));
    expect(screen.getByTestId('ultraplan-bar-confirm')).toBeInTheDocument();
    act(() => mount(flow({ stage: 'demo_review', demoIteration: 1 })));
    expect(screen.queryByTestId('ultraplan-bar-confirm')).not.toBeInTheDocument();
  });
});

describe('<UltraPlanBar /> 禁用与只读', () => {
  it('有任务在跑(activeRunId 非空):动作与重新开始全部禁用,title「有任务正在运行」,点击不发请求', () => {
    mount(flow({ ...planned, stage: 'production' }));
    useChatStore.setState({ activeRunId: 'run_9' });
    render(<UltraPlanBar />);
    const action = screen.getByTestId('ultraplan-bar-action');
    const restart = screen.getByTestId('ultraplan-bar-restart');
    expect(action).toBeDisabled();
    expect(action).toHaveAttribute('title', '有任务正在运行');
    expect(restart).toBeDisabled();
    expect(restart).toHaveAttribute('title', '有任务正在运行');
    fireEvent.click(action);
    fireEvent.click(restart);
    expect(screen.queryByTestId('ultraplan-bar-confirm')).not.toBeInTheDocument();
    expect(calls.filter((c) => c.method === 'POST')).toHaveLength(0);

    // 任务结束即恢复
    act(() => useChatStore.setState({ activeRunId: null }));
    expect(action).toBeEnabled();
    expect(action).not.toHaveAttribute('title');
    expect(restart).toBeEnabled();
  });

  it('确认行开着时来了任务:确认钮同样禁用', () => {
    mount(flow());
    render(<UltraPlanBar />);
    fireEvent.click(screen.getByTestId('ultraplan-bar-restart'));
    act(() => useChatStore.setState({ activeRunId: 'run_9' }));
    const ok = screen.getByTestId('ultraplan-bar-restart-confirm');
    expect(ok).toBeDisabled();
    expect(ok).toHaveAttribute('title', '有任务正在运行');
    fireEvent.click(ok);
    expect(posts(RESTART_URL)).toHaveLength(0);
  });

  it('上一个动作还在提交中:其余动作禁用', () => {
    mount(flow({ ...planned, stage: 'production' }));
    render(<UltraPlanBar />);
    act(() => useUltraPlanStore.setState({ pending: { action: 'resume_production', rev: null } }));
    expect(screen.getByTestId('ultraplan-bar-action')).toBeDisabled();
    expect(screen.getByTestId('ultraplan-bar-action')).toHaveAttribute('title', '上一个操作还在提交中');
    expect(screen.getByTestId('ultraplan-bar-restart')).toBeDisabled();
  });

  it('提交在途时切会话:原会话没回来的请求不把新会话的状态条卡住', async () => {
    const SID2 = 'sess_2';
    const UP2 = 'up_b2';
    const ASK_URL2 = `/api/forge/sessions/${SID2}/ask:execute`;
    // 两个会话的 ask:execute 都挂着(HTTP 响应要到整轮结束才回),各自手动放行
    const release: Record<string, () => void> = {};
    stubFetch(
      (call) =>
        new Promise<Reply>((resolve) => {
          release[call.url] = () => resolve({ body: { run: { id: 'run_s', status: 'completed' } } });
        }),
    );
    mount(flow({ ...planned, stage: 'production' }));
    render(<UltraPlanBar />);
    fireEvent.click(screen.getByTestId('ultraplan-bar-action'));
    await waitFor(() => expect(posts(ASK_URL)).toHaveLength(1));
    expect(screen.getByTestId('ultraplan-bar-action')).toBeDisabled();
    expect(screen.getByTestId('ultraplan-bar-action')).toHaveAttribute('title', '上一个操作还在提交中');

    // 切会话:chat reset()(连带清 ultraPlanStore)→ 选中另一个会话 → 快照回填它自己的流程。
    // 状态条随 Composer 留在原地不重挂。
    const other = flow({ ...planned, id: UP2, stage: 'production' });
    face = { ultraplan: other };
    await act(async () => {
      useChatStore.getState().reset();
      useSessionStore.setState({ activeSessionId: SID2 });
      useUltraPlanStore.getState().hydrate(other, SID2);
    });
    expect(useUltraPlanStore.getState().pending).toBeNull();
    const action = screen.getByTestId('ultraplan-bar-action');
    expect(action).toBeEnabled();
    expect(action).not.toHaveAttribute('title');
    expect(screen.getByTestId('ultraplan-bar-restart')).toBeEnabled();

    // 新会话的动作发得出去,且落在新会话自己的地址上
    fireEvent.click(action);
    await waitFor(() => expect(posts(ASK_URL2)).toHaveLength(1));
    expect(posts(ASK_URL2)[0].body).toEqual({
      userInput: '',
      mode: 'team',
      ultraplan: { id: UP2, action: 'resume_production' },
    });
    expect(action).toBeDisabled();

    // 原会话那一轮此时才结束:不动新会话的在途态,也不弹提示
    await act(async () => {
      release[ASK_URL]();
    });
    expect(useUltraPlanStore.getState().pending).toEqual({ action: 'resume_production', rev: null });
    expect(action).toBeDisabled();
    expect(useToastStore.getState().items).toHaveLength(0);

    await act(async () => {
      release[ASK_URL2]();
    });
    await waitFor(() => expect(screen.getByTestId('ultraplan-bar-action')).toBeEnabled());
    expect(posts(ASK_URL)).toHaveLength(1);
    expect(posts(ASK_URL2)).toHaveLength(1);
  });

  it('Codex 可以继续制作;代理不是 coding 时整条只读', () => {
    useSessionStore.setState({ sessions: [session('coding', 'codex')] });
    mount(flow({ ...planned, stage: 'production' }));
    render(<UltraPlanBar />);
    // 进度与阶段说明照常
    expect(screen.getByTestId('ultraplan-step-production')).toHaveAttribute('data-state', 'current');
    expect(screen.queryByTestId('ultraplan-bar-readonly')).not.toBeInTheDocument();
    expect(screen.getByTestId('ultraplan-bar-action')).toBeEnabled();
    expect(screen.getByTestId('ultraplan-bar-restart')).toBeEnabled();

    act(() => useSessionStore.setState({ sessions: [session('document', 'local')] }));
    expect(screen.getByTestId('ultraplan-bar-readonly')).toHaveTextContent('当前代理类型不支持 UltraPlan');
    expect(screen.queryByTestId('ultraplan-bar-restart')).not.toBeInTheDocument();

    act(() => useSessionStore.setState({ sessions: [session('coding', 'local')] }));
    expect(screen.queryByTestId('ultraplan-bar-readonly')).not.toBeInTheDocument();
    expect(screen.getByTestId('ultraplan-bar-action')).toBeEnabled();
    expect(screen.getByTestId('ultraplan-bar-restart')).toBeEnabled();
  });
});
