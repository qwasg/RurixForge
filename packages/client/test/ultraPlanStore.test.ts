import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { usePlanStore } from '@/lib/planStore';
import { useSessionStore } from '@/lib/sessionStore';
import { useToastStore } from '@/lib/toastStore';
import type { ChatBlock } from '@/lib/timeline';
import {
  CLIENT_ERROR,
  actionErrorText,
  cardInteractive,
  clearDraft,
  demoUrl,
  demoUrlAllowed,
  draftKey,
  isLoopbackHost,
  loadDraft,
  phaseText,
  saveDraft,
  stageIndex,
  stageLabel,
  useUltraPlanStore,
  type UltraPlanAnswers,
  type UltraPlanState,
} from '@/lib/ultraPlanStore';

/**
 * D-044 ultraPlanStore:状态只从快照 / GET 来;act() 自己 POST ask:execute,受理信号是
 * composer.user.message 回显而非 HTTP 返回;REST 动作;草稿;Demo 地址断言;卡片可操作真值表。
 * 后端未实现,全部对着 wire 契约用 fetch 桩测。
 */

const SID = 'sess_1';
const UP_ID = 'up_a1';
const TOKEN = '0123456789abcdef0123456789abcdef';
const ULTRA_URL = `/api/forge/sessions/${SID}/ultraplan`;
const ASK_URL = `/api/forge/sessions/${SID}/ask:execute`;

function flow(patch: Partial<UltraPlanState> = {}): UltraPlanState {
  return {
    id: UP_ID,
    token: TOKEN,
    slug: 'tower-defense-a1b2',
    dir: '.forge/ultraplan/tower-defense-a1b2',
    title: '塔防小游戏',
    workspaceId: null,
    stage: 'questionnaire',
    phase: 'waiting',
    running: null,
    lastError: null,
    questionnaireRev: 2,
    demoIteration: 3,
    demoVerified: true,
    demoNote: null,
    planPath: '.forge/plans/tower-defense-a1b2.plan.md',
    planRev: 1,
    planHash: 'abc',
    productionRunId: null,
    acceptanceRound: 4,
    createdAt: '2026-09-30T08:00:00Z',
    updatedAt: '2026-09-30T08:05:00Z',
    ...patch,
  };
}

interface Call {
  url: string;
  method: string;
  /** 解析后的 JSON 请求体;无请求体为 undefined。 */
  body: unknown;
  hasBody: boolean;
}

type Reply = { status?: number; body: unknown };
type Handler = (call: Call) => Reply | Promise<Reply>;

let calls: Call[] = [];

function stubFetch(handler: Handler) {
  vi.stubGlobal(
    'fetch',
    vi.fn(async (url: unknown, init?: RequestInit) => {
      const hasBody = init?.body !== undefined && init.body !== null;
      const call: Call = {
        url: String(url),
        method: init?.method ?? 'GET',
        body: hasBody ? JSON.parse(String(init?.body)) : undefined,
        hasBody,
      };
      calls.push(call);
      const reply = await handler(call);
      const status = reply.status ?? 200;
      return { ok: status < 400, status, json: async () => reply.body } as Response;
    }),
  );
}

function errorReply(status: number, code: string, message: string, details?: unknown): Reply {
  return { status, body: { error: { code, message, ...(details ? { details } : {}) } } };
}

const gets = () => calls.filter((c) => c.method === 'GET' && c.url === ULTRA_URL);
const asks = () => calls.filter((c) => c.url === ASK_URL);

const initialUltra = useUltraPlanStore.getState();
const initialSessions = useSessionStore.getState();
const initialToasts = useToastStore.getState();

beforeEach(() => {
  calls = [];
  localStorage.clear();
  useUltraPlanStore.setState(initialUltra, true);
  // 在途请求槽 / 代次在 store 闭包外,须经 reset() 清。
  useUltraPlanStore.getState().reset();
  usePlanStore.getState().reset();
  useSessionStore.setState(initialSessions, true);
  useToastStore.setState(initialToasts, true);
  useSessionStore.setState({ activeSessionId: SID });
  stubFetch(() => ({ body: { ultraplan: null } }));
});

afterEach(() => {
  vi.unstubAllGlobals();
});

describe('ultraPlanStore hydrate / refresh', () => {
  it('hydrate:归一会话字段并记下所属会话;null 清空流程', () => {
    const s = useUltraPlanStore.getState();
    s.hydrate(flow());
    expect(useUltraPlanStore.getState().state).toEqual(flow());
    expect(useUltraPlanStore.getState().sessionId).toBe(SID);
    // 缺字段的旧 / 残缺对象不让 undefined 漏进版本比较
    s.hydrate({ id: 'up_min', stage: 'demo_review' }, 'sess_9');
    expect(useUltraPlanStore.getState().state).toMatchObject({
      id: 'up_min',
      stage: 'demo_review',
      phase: 'waiting',
      running: null,
      questionnaireRev: 0,
      demoIteration: 0,
      planRev: 0,
      acceptanceRound: 0,
      demoVerified: false,
      planPath: null,
    });
    expect(useUltraPlanStore.getState().sessionId).toBe('sess_9');
    s.hydrate(null);
    expect(useUltraPlanStore.getState().state).toBeNull();
    // 没有 id 的对象不算流程
    s.hydrate({ stage: 'discovery' });
    expect(useUltraPlanStore.getState().state).toBeNull();
  });

  it('hydrate:同一流程保留已拉到的附属数据;换流程 / 无流程一并清掉', () => {
    const questionnaire = { title: '问卷', understanding: '', sections: [] };
    useUltraPlanStore.getState().hydrate(flow());
    useUltraPlanStore.setState({ questionnaire, demo: { port: 41000, token: TOKEN, entry: 'index.html' } });
    useUltraPlanStore.getState().hydrate(flow({ stage: 'demo_review' }));
    expect(useUltraPlanStore.getState().questionnaire).toEqual(questionnaire);
    expect(useUltraPlanStore.getState().demo).not.toBeNull();
    useUltraPlanStore.getState().hydrate(flow({ id: 'up_other' }));
    expect(useUltraPlanStore.getState().questionnaire).toBeNull();
    expect(useUltraPlanStore.getState().demo).toBeNull();
  });

  it('refresh:GET …/ultraplan 回填 state / demo / 问卷 / 答案 / checks / acceptance', async () => {
    const questionnaire = {
      title: '塔防问卷',
      understanding: '## 理解',
      sections: [{ id: 's1', title: '玩法', questions: [{ id: 'q1', kind: 'text', question: '主题?' }] }],
    };
    const answers: UltraPlanAnswers = { q1: { text: '科幻' } };
    const checks = {
      automated: [{ id: 'a1', kind: 'gameplay', steps: '放塔', expected: '敌人掉血' }],
      manual: [{ id: 'm1', title: '手感', steps: '玩一局', expected: '不卡顿' }],
    };
    const acceptance = { rounds: [{ round: 1, results: [{ id: 'm1', status: 'pass' }], failed: [] }] };
    stubFetch(() => ({
      body: {
        ultraplan: flow({ stage: 'demo_review' }),
        demo: { port: 41234, token: TOKEN, entry: 'index.html' },
        questionnaire,
        answers,
        checks,
        acceptance,
      },
    }));
    await useUltraPlanStore.getState().refresh(SID);
    expect(gets()).toHaveLength(1);
    const st = useUltraPlanStore.getState();
    expect(st.sessionId).toBe(SID);
    expect(st.state).toEqual(flow({ stage: 'demo_review' }));
    expect(st.demo).toEqual({ port: 41234, token: TOKEN, entry: 'index.html' });
    expect(st.demoError).toBeNull();
    expect(st.questionnaire).toEqual(questionnaire);
    expect(st.answers).toEqual(answers);
    expect(st.checks).toEqual(checks);
    expect(st.acceptance).toEqual(acceptance);
  });

  it('refresh:Demo 监听起不来 → demo=null + demoError;流程被清 → 全部落空', async () => {
    stubFetch(() => ({
      body: {
        ultraplan: flow({ stage: 'demo_review' }),
        demo: null,
        demoError: { code: 'DEMO_HOST_UNAVAILABLE', message: '端口绑定失败' },
      },
    }));
    await useUltraPlanStore.getState().refresh();
    expect(useUltraPlanStore.getState().demo).toBeNull();
    expect(useUltraPlanStore.getState().demoError).toEqual({
      code: 'DEMO_HOST_UNAVAILABLE',
      message: '端口绑定失败',
    });
    stubFetch(() => ({ body: { ultraplan: null, demo: { port: 1, token: TOKEN, entry: 'index.html' } } }));
    await useUltraPlanStore.getState().refresh();
    expect(useUltraPlanStore.getState().state).toBeNull();
    expect(useUltraPlanStore.getState().demo).toBeNull();
    expect(useUltraPlanStore.getState().demoError).toBeNull();
  });

  it('refresh:在途时再来的调用合并成结束后补拉一次(一串事件不打 N 个 GET)', async () => {
    const releases: Array<() => void> = [];
    let served = 0;
    stubFetch(
      () =>
        new Promise<Reply>((resolve) => {
          served += 1;
          const stage = served === 1 ? 'questionnaire' : 'demo_review';
          releases.push(() => resolve({ body: { ultraplan: flow({ stage }) } }));
        }),
    );
    const s = useUltraPlanStore.getState();
    const first = s.refresh(SID);
    const second = s.refresh(SID);
    const third = s.refresh(SID);
    expect(second).toBe(first);
    expect(third).toBe(first);
    expect(gets()).toHaveLength(1);
    releases[0]();
    await vi.waitFor(() => expect(gets()).toHaveLength(2));
    releases[1]();
    await first;
    expect(gets()).toHaveLength(2);
    expect(useUltraPlanStore.getState().state?.stage).toBe('demo_review');
  });

  it('refresh:回来时已切走会话 / 已 reset → 陈旧结果不落;请求失败保留快照回填的状态', async () => {
    let release!: () => void;
    stubFetch(
      () =>
        new Promise<Reply>((resolve) => {
          release = () => resolve({ body: { ultraplan: flow({ stage: 'done' }) } });
        }),
    );
    const stale = useUltraPlanStore.getState().refresh(SID);
    useUltraPlanStore.getState().reset();
    useSessionStore.setState({ activeSessionId: 'sess_2' });
    release();
    await stale;
    expect(useUltraPlanStore.getState().state).toBeNull();

    useSessionStore.setState({ activeSessionId: SID });
    useUltraPlanStore.getState().hydrate(flow());
    vi.stubGlobal('fetch', vi.fn(async () => Promise.reject(new TypeError('offline'))));
    await useUltraPlanStore.getState().refresh(SID);
    expect(useUltraPlanStore.getState().state).toEqual(flow());
    expect(useToastStore.getState().items).toHaveLength(0);
  });
});

describe('ultraPlanStore.act 请求体(契约 §2 表)', () => {
  beforeEach(() => {
    stubFetch((call) =>
      call.url === ASK_URL ? { body: { run: { id: 'run_1', status: 'completed' } } } : { body: { ultraplan: flow() } },
    );
    useUltraPlanStore.getState().hydrate(flow());
  });

  it('answer:mode=ultraplan,rev=questionnaireRev,带 answers,userInput 为空', async () => {
    const answers: UltraPlanAnswers = {
      q1: { choice: ['opt_a'] },
      q2: { delegate: true },
      q3: { scale: 4 },
    };
    const result = await useUltraPlanStore.getState().submitAnswers(answers);
    expect(result).toEqual({ ok: true });
    expect(asks()).toHaveLength(1);
    expect(asks()[0].method).toBe('POST');
    expect(asks()[0].body).toEqual({
      userInput: '',
      mode: 'ultraplan',
      ultraplan: { id: UP_ID, rev: 2, action: 'answer', answers },
    });
    expect(useUltraPlanStore.getState().pending).toBeNull();
  });

  it('answer 重试:answers 省略(后端复用已存答案)', async () => {
    await useUltraPlanStore.getState().submitAnswers();
    expect(asks()[0].body).toEqual({
      userInput: '',
      mode: 'ultraplan',
      ultraplan: { id: UP_ID, rev: 2, action: 'answer' },
    });
  });

  it('approve_demo / revise_demo:rev=demoIteration;revise 的意见走 userInput', async () => {
    useUltraPlanStore.getState().hydrate(flow({ stage: 'demo_review' }));
    await useUltraPlanStore.getState().approveDemo();
    await useUltraPlanStore.getState().reviseDemo('  跳跃手感太飘  ');
    expect(asks().map((c) => c.body)).toEqual([
      { userInput: '', mode: 'ultraplan', ultraplan: { id: UP_ID, rev: 3, action: 'approve_demo' } },
      { userInput: '跳跃手感太飘', mode: 'ultraplan', ultraplan: { id: UP_ID, rev: 3, action: 'revise_demo' } },
    ]);
  });

  it('revise_plan:mode=ultraplan,rev=planRev', async () => {
    useUltraPlanStore.getState().hydrate(flow({ stage: 'plan_review' }));
    await useUltraPlanStore.getState().revisePlan('把 Boss 战提前');
    expect(asks()[0].body).toEqual({
      userInput: '把 Boss 战提前',
      mode: 'ultraplan',
      ultraplan: { id: UP_ID, rev: 1, action: 'revise_plan' },
    });
  });

  it('start_production:mode=team,rev=planRev,带 planPath;确认逐项审批才带 acknowledgeApprovals', async () => {
    useUltraPlanStore.getState().hydrate(flow({ stage: 'plan_review' }));
    await useUltraPlanStore.getState().startProduction();
    await useUltraPlanStore.getState().startProduction(true);
    expect(asks().map((c) => c.body)).toEqual([
      {
        userInput: '',
        mode: 'team',
        ultraplan: { id: UP_ID, rev: 1, action: 'start_production' },
        planPath: '.forge/plans/tower-defense-a1b2.plan.md',
      },
      {
        userInput: '',
        mode: 'team',
        ultraplan: { id: UP_ID, rev: 1, action: 'start_production', acknowledgeApprovals: true },
        planPath: '.forge/plans/tower-defense-a1b2.plan.md',
      },
    ]);
  });

  it('resume_production:mode=team 且不带 rev;fix_production:rev=acceptanceRound', async () => {
    useUltraPlanStore.getState().hydrate(flow({ stage: 'production', phase: 'failed' }));
    await useUltraPlanStore.getState().resumeProduction();
    useUltraPlanStore.getState().hydrate(flow({ stage: 'acceptance' }));
    await useUltraPlanStore.getState().fixProduction(true);
    expect(asks().map((c) => c.body)).toEqual([
      { userInput: '', mode: 'team', ultraplan: { id: UP_ID, action: 'resume_production' } },
      {
        userInput: '',
        mode: 'team',
        ultraplan: { id: UP_ID, rev: 4, action: 'fix_production', acknowledgeApprovals: true },
      },
    ]);
  });

  it('target 覆盖:卡片 / 页签带自己那一版的 id + rev(过期与否交后端 409)', async () => {
    useUltraPlanStore.getState().hydrate(flow({ stage: 'demo_review' }));
    await useUltraPlanStore.getState().approveDemo({ id: 'up_other', rev: 1 });
    expect(asks()[0].body).toEqual({
      userInput: '',
      mode: 'ultraplan',
      ultraplan: { id: 'up_other', rev: 1, action: 'approve_demo' },
    });
  });

  it('本地即可判定的失败不发请求:无流程 / 无会话 / 修改意见为空', async () => {
    const blank = await useUltraPlanStore.getState().reviseDemo('   ');
    expect(blank).toEqual({ ok: false, code: 'INVALID_INPUT', message: '请先填写修改意见', details: null });
    useUltraPlanStore.getState().hydrate(null);
    const noFlow = await useUltraPlanStore.getState().approveDemo();
    expect(noFlow).toMatchObject({ ok: false, code: CLIENT_ERROR.noFlow });
    useUltraPlanStore.getState().hydrate(flow());
    useSessionStore.setState({ activeSessionId: null });
    const noSession = await useUltraPlanStore.getState().approveDemo();
    expect(noSession).toMatchObject({ ok: false, code: CLIENT_ERROR.noSession });
    expect(asks()).toHaveLength(0);
    expect(useUltraPlanStore.getState().pending).toBeNull();
  });
});

describe('ultraPlanStore.act 在途 → 受理 / 被拒', () => {
  it('受理信号是 composer.user.message 回显,不等 POST(整轮跑完才回)', async () => {
    let releasePost!: () => void;
    stubFetch((call) =>
      call.url === ASK_URL
        ? new Promise<Reply>((resolve) => {
            releasePost = () => resolve({ body: { run: { id: 'run_1', status: 'completed' } } });
          })
        : { body: { ultraplan: flow() } },
    );
    useUltraPlanStore.getState().hydrate(flow());
    saveDraft(UP_ID, 2, { q1: { text: '草稿' } });
    let settled: unknown = null;
    const pending = useUltraPlanStore
      .getState()
      .submitAnswers({ q1: { text: '定稿' } })
      .then((r) => {
        settled = r;
        return r;
      });
    expect(useUltraPlanStore.getState().pending).toEqual({ action: 'answer', rev: 2 });
    expect(useUltraPlanStore.getState().lastActionError).toBeNull();

    // 在途期间再点一次:本地拒,不重复发
    const dup = await useUltraPlanStore.getState().approveDemo();
    expect(dup).toMatchObject({ ok: false, code: CLIENT_ERROR.pending });
    expect(asks()).toHaveLength(1);

    // 不相干的回显(别的动作 / 别的流程 / 别的版本)不算受理
    const s = useUltraPlanStore.getState();
    s.noteUserMessage({ id: UP_ID, action: 'approve_demo', rev: 2 });
    s.noteUserMessage({ id: 'up_other', action: 'answer', rev: 2 });
    s.noteUserMessage({ id: UP_ID, action: 'answer', rev: 1 });
    await Promise.resolve();
    expect(useUltraPlanStore.getState().pending).not.toBeNull();
    expect(settled).toBeNull();
    expect(loadDraft(UP_ID, 2)).toEqual({ q1: { text: '草稿' } });

    s.noteUserMessage({ id: UP_ID, action: 'answer', rev: 2 });
    expect(useUltraPlanStore.getState().pending).toBeNull();
    await expect(pending).resolves.toEqual({ ok: true });
    // 答案已入库 → 本地草稿作废
    expect(loadDraft(UP_ID, 2)).toBeNull();

    // 整轮结束后 POST 才回:不再改任何状态
    releasePost();
    await Promise.resolve();
    await Promise.resolve();
    expect(useUltraPlanStore.getState().pending).toBeNull();
    expect(useUltraPlanStore.getState().lastActionError).toBeNull();
    expect(useToastStore.getState().items).toHaveLength(0);
  });

  it('resume_production 回显不带 rev 也能对上', async () => {
    stubFetch((call) =>
      call.url === ASK_URL ? new Promise<Reply>(() => undefined) : { body: { ultraplan: flow() } },
    );
    useUltraPlanStore.getState().hydrate(flow({ stage: 'production', phase: 'failed' }));
    const pending = useUltraPlanStore.getState().resumeProduction();
    expect(useUltraPlanStore.getState().pending).toEqual({ action: 'resume_production', rev: null });
    useUltraPlanStore.getState().noteUserMessage({ id: UP_ID, action: 'resume_production', rev: null });
    await expect(pending).resolves.toEqual({ ok: true });
    expect(useUltraPlanStore.getState().pending).toBeNull();
  });

  it('没见到回显但 POST 成功返回 → 在途态随之清掉', async () => {
    stubFetch((call) =>
      call.url === ASK_URL ? { body: { run: { id: 'run_1' } } } : { body: { ultraplan: flow() } },
    );
    useUltraPlanStore.getState().hydrate(flow());
    saveDraft(UP_ID, 2, { q1: { text: '草稿' } });
    await expect(useUltraPlanStore.getState().submitAnswers({ q1: { text: 'x' } })).resolves.toEqual({ ok: true });
    expect(useUltraPlanStore.getState().pending).toBeNull();
    expect(loadDraft(UP_ID, 2)).toBeNull();
  });

  it('409 ULTRAPLAN_STAGE_MISMATCH:清在途、记 lastActionError(含 details)、warning toast、重拉阶段', async () => {
    const details = { stage: 'production', allowed: ['resume_production'] };
    stubFetch((call) =>
      call.url === ASK_URL
        ? errorReply(409, 'ULTRAPLAN_STAGE_MISMATCH', '流程已进入制作阶段', details)
        : { body: { ultraplan: flow({ stage: 'production', phase: 'running', running: 'production' }) } },
    );
    useUltraPlanStore.getState().hydrate(flow({ stage: 'demo_review' }));
    saveDraft(UP_ID, 2, { q1: { text: '草稿' } });
    const result = await useUltraPlanStore.getState().approveDemo();
    expect(result).toEqual({
      ok: false,
      code: 'ULTRAPLAN_STAGE_MISMATCH',
      message: '流程已进入制作阶段',
      details,
    });
    const st = useUltraPlanStore.getState();
    expect(st.pending).toBeNull();
    expect(st.lastActionError).toEqual({
      code: 'ULTRAPLAN_STAGE_MISMATCH',
      message: '流程已进入制作阶段',
      details,
    });
    const toast = useToastStore.getState().items.find((t) => t.title === '流程已进入制作阶段');
    expect(toast?.kind).toBe('warning');
    // 被拒后重拉:本地阶段换成后端的真实阶段
    await vi.waitFor(() => expect(useUltraPlanStore.getState().state?.stage).toBe('production'));
    expect(gets()).toHaveLength(1);
    // approve_demo 发起时点亮的「正在调研」随拒绝复位
    expect(usePlanStore.getState().planning).toBe(false);
    // 下一次动作发起即清掉上次的错误
    stubFetch(() => new Promise<Reply>(() => undefined));
    void useUltraPlanStore.getState().resumeProduction();
    expect(useUltraPlanStore.getState().lastActionError).toBeNull();
  });

  it('400 ULTRAPLAN_ANSWERS_INVALID 带 questionId;草稿保留供用户改', async () => {
    stubFetch((call) =>
      call.url === ASK_URL
        ? errorReply(400, 'ULTRAPLAN_ANSWERS_INVALID', '第 3 题必答', { questionId: 'q3' })
        : { body: { ultraplan: flow() } },
    );
    useUltraPlanStore.getState().hydrate(flow());
    saveDraft(UP_ID, 2, { q1: { text: '草稿' } });
    const result = await useUltraPlanStore.getState().submitAnswers({ q1: { text: '草稿' } });
    expect(result).toMatchObject({ ok: false, code: 'ULTRAPLAN_ANSWERS_INVALID', details: { questionId: 'q3' } });
    expect(loadDraft(UP_ID, 2)).toEqual({ q1: { text: '草稿' } });
  });

  it('SESSION_BUSY / 网络失败:结构化返回,提示语不沿用「后台回执」那句', async () => {
    stubFetch((call) =>
      call.url === ASK_URL ? errorReply(409, 'SESSION_BUSY', '会话已有运行中的 run(run_9)') : { body: { ultraplan: flow() } },
    );
    useUltraPlanStore.getState().hydrate(flow({ stage: 'demo_review' }));
    const busy = await useUltraPlanStore.getState().approveDemo();
    expect(busy).toMatchObject({ ok: false, code: 'SESSION_BUSY', details: null });
    expect(useToastStore.getState().items.some((t) => t.title.includes('有任务正在运行'))).toBe(true);
    expect(useToastStore.getState().items.some((t) => t.title.includes('后台回执'))).toBe(false);

    vi.stubGlobal('fetch', vi.fn(async () => Promise.reject(new TypeError('Failed to fetch'))));
    const offline = await useUltraPlanStore.getState().approveDemo();
    expect(offline).toMatchObject({ ok: false, code: 'NETWORK' });
    expect(useUltraPlanStore.getState().pending).toBeNull();
  });

  it('approve_demo / revise_plan 起的是 Planning turn:发起即点亮 planStore.planning', () => {
    stubFetch(() => new Promise<Reply>(() => undefined));
    useUltraPlanStore.getState().hydrate(flow({ stage: 'demo_review' }));
    void useUltraPlanStore.getState().approveDemo();
    expect(usePlanStore.getState().planning).toBe(true);
    usePlanStore.getState().reset();
    useUltraPlanStore.getState().reset();
    useUltraPlanStore.getState().hydrate(flow({ stage: 'questionnaire' }));
    void useUltraPlanStore.getState().submitAnswers({});
    expect(usePlanStore.getState().planning).toBe(false);
  });

  it('在途时切会话(reset):在途 Promise 当场以 CLIENT_SESSION_CHANGED 结掉;POST 之后回来不污染新会话的 store', async () => {
    let releasePost!: () => void;
    stubFetch((call) =>
      call.url === ASK_URL
        ? new Promise<Reply>((resolve) => {
            releasePost = () => resolve(errorReply(409, 'ULTRAPLAN_STAGE_MISMATCH', '过期'));
          })
        : call.method === 'POST'
          ? new Promise<Reply>(() => undefined)
          : { body: { ultraplan: null } },
    );
    useUltraPlanStore.getState().hydrate(flow({ stage: 'demo_review' }));
    const pending = useUltraPlanStore.getState().approveDemo();
    useUltraPlanStore.getState().reset();
    expect(useUltraPlanStore.getState().pending).toBeNull();
    // POST 还挂着(整轮结束才回):调用方不陪着等,也不弹提示 / 不记错误
    await expect(pending).resolves.toEqual({
      ok: false,
      code: CLIENT_ERROR.sessionChanged,
      message: expect.any(String),
      details: null,
    });
    expect(CLIENT_ERROR.sessionChanged).toBe('CLIENT_SESSION_CHANGED');
    expect(useUltraPlanStore.getState().lastActionError).toBeNull();
    expect(useToastStore.getState().items).toHaveLength(0);

    // 新会话发起自己的动作后,原会话的 POST 才回来:不清新会话的在途态,不记错误、不弹提示、不重拉
    useSessionStore.setState({ activeSessionId: 'sess_2' });
    useUltraPlanStore.getState().hydrate(flow({ id: 'up_b2', stage: 'production' }), 'sess_2');
    const next = useUltraPlanStore.getState().resumeProduction();
    expect(useUltraPlanStore.getState().pending).toEqual({ action: 'resume_production', rev: null });
    releasePost();
    await new Promise((resolve) => setTimeout(resolve, 0));
    expect(useUltraPlanStore.getState().pending).toEqual({ action: 'resume_production', rev: null });
    expect(useUltraPlanStore.getState().lastActionError).toBeNull();
    expect(useToastStore.getState().items).toHaveLength(0);
    expect(calls.filter((c) => c.method === 'GET')).toHaveLength(0);
    // 新会话的在途槽没被原会话的响应顶掉:它自己的回显照常结它
    useUltraPlanStore.getState().noteUserMessage({ id: 'up_b2', action: 'resume_production', rev: null });
    await expect(next).resolves.toEqual({ ok: true });
    expect(useUltraPlanStore.getState().pending).toBeNull();
  });
});

describe('ultraPlanStore REST 动作(契约 §3)', () => {
  it('restart:POST …/ultraplan/restart 无请求体;成功即清空流程', async () => {
    stubFetch((call) => (call.method === 'POST' ? { body: { ok: true } } : { body: { ultraplan: null } }));
    useUltraPlanStore.getState().hydrate(flow({ stage: 'plan_review' }));
    const result = await useUltraPlanStore.getState().restart();
    expect(result).toEqual({ ok: true });
    const post = calls.find((c) => c.method === 'POST');
    expect(post?.url).toBe(`${ULTRA_URL}/restart`);
    expect(post?.hasBody).toBe(false);
    const st = useUltraPlanStore.getState();
    expect(st.state).toBeNull();
    expect(st.pending).toBeNull();
    expect(st.sessionId).toBe(SID);
  });

  it('submitAcceptance:body绑定flow/plan/round;回 next / failed 并以响应里的 ultraplan 更新阶段', async () => {
    const results = [
      { id: 'm1', status: 'pass' as const },
      { id: 'm2', status: 'fail' as const, note: '第二关卡死' },
      { id: 'm3', status: 'skip' as const },
    ];
    stubFetch((call) =>
      call.method === 'POST'
        ? { body: { ultraplan: flow({ stage: 'acceptance', phase: 'waiting' }), next: 'fix', failed: ['m2'] } }
        : { body: { ultraplan: flow({ stage: 'acceptance' }) } },
    );
    useUltraPlanStore.getState().hydrate(flow({ stage: 'acceptance' }));
    const outcome = await useUltraPlanStore.getState().submitAcceptance(results);
    expect(outcome).toEqual({ ok: true, next: 'fix', failed: ['m2'] });
    const post = calls.find((c) => c.method === 'POST');
    expect(post?.url).toBe(`${ULTRA_URL}/acceptance`);
    expect(post?.body).toEqual({ id: UP_ID, rev: 1, round: 4, results });
    // 动作成功后的对账重拉先落定,再换桩
    await useUltraPlanStore.getState().refresh(SID);

    // 全过 → done,无 LLM turn
    stubFetch((call) =>
      call.method === 'POST'
        ? { body: { ultraplan: flow({ stage: 'done' }), next: 'done', failed: [] } }
        : { body: { ultraplan: flow({ stage: 'done' }) } },
    );
    const done = await useUltraPlanStore.getState().submitAcceptance([{ id: 'm1', status: 'pass' }]);
    expect(done).toEqual({ ok: true, next: 'done', failed: [] });
    expect(useUltraPlanStore.getState().state?.stage).toBe('done');
  });

  it('submitAcceptance:400 ULTRAPLAN_ACCEPTANCE_INVALID 如实返回', async () => {
    stubFetch((call) =>
      call.method === 'POST'
        ? errorReply(400, 'ULTRAPLAN_ACCEPTANCE_INVALID', 'fail 项必须填写说明')
        : { body: { ultraplan: flow({ stage: 'acceptance' }) } },
    );
    useUltraPlanStore.getState().hydrate(flow({ stage: 'acceptance' }));
    const outcome = await useUltraPlanStore.getState().submitAcceptance([{ id: 'm2', status: 'fail' }]);
    expect(outcome).toEqual({
      ok: false,
      code: 'ULTRAPLAN_ACCEPTANCE_INVALID',
      message: 'fail 项必须填写说明',
      details: null,
    });
    expect(useUltraPlanStore.getState().lastActionError?.code).toBe('ULTRAPLAN_ACCEPTANCE_INVALID');
    expect(useUltraPlanStore.getState().pending).toBeNull();
  });

  it('rollbackDemo:POST绑定flow/demo版本;阶段取响应体;无快照 409 如实返回', async () => {
    const rolled = flow({
      stage: 'demo_review',
      demoIteration: 4,
      demoVerified: false,
      demoNote: '已回退到第 2 版(未重新验证)',
    });
    stubFetch((call) => (call.method === 'POST' ? { body: { ultraplan: rolled } } : { body: { ultraplan: rolled } }));
    useUltraPlanStore.getState().hydrate(flow({ stage: 'demo_review' }));
    await expect(useUltraPlanStore.getState().rollbackDemo()).resolves.toEqual({ ok: true });
    const post = calls.find((c) => c.method === 'POST');
    expect(post?.url).toBe(`${ULTRA_URL}/rollback_demo`);
    expect(post?.body).toEqual({ id: UP_ID, rev: 3 });
    expect(useUltraPlanStore.getState().state).toMatchObject({ demoIteration: 4, demoVerified: false });

    stubFetch((call) =>
      call.method === 'POST'
        ? errorReply(409, 'ULTRAPLAN_ARTIFACTS_MISSING', '没有可回退的 Demo 版本')
        : { body: { ultraplan: rolled } },
    );
    const missing = await useUltraPlanStore.getState().rollbackDemo();
    expect(missing).toMatchObject({ ok: false, code: 'ULTRAPLAN_ARTIFACTS_MISSING' });
    expect(useToastStore.getState().items.some((t) => t.title === '没有可回退的 Demo 版本')).toBe(true);
  });

  it('REST 动作在有在途动作 / 无会话时本地拒', async () => {
    stubFetch(() => new Promise<Reply>(() => undefined));
    useUltraPlanStore.getState().hydrate(flow({ stage: 'demo_review' }));
    void useUltraPlanStore.getState().approveDemo();
    await expect(useUltraPlanStore.getState().restart()).resolves.toMatchObject({
      ok: false,
      code: CLIENT_ERROR.pending,
    });
    useUltraPlanStore.getState().reset();
    useSessionStore.setState({ activeSessionId: null });
    await expect(useUltraPlanStore.getState().rollbackDemo()).resolves.toMatchObject({
      ok: false,
      code: CLIENT_ERROR.noSession,
    });
  });
});

describe('ultraPlanStore 实时事件', () => {
  it('ultraplan.stage:记本轮生效的思考规格并触发重拉;turn 结束即清', async () => {
    stubFetch(() => ({ body: { ultraplan: flow({ phase: 'running', running: 'discovery' }) } }));
    const s = useUltraPlanStore.getState();
    s.applyLiveEvent({
      sessionId: SID,
      type: 'ultraplan.stage',
      payload: { id: UP_ID, stage: 'discovery', phase: 'running', running: 'discovery', effort: 'high', thinkingForced: true },
    });
    expect(useUltraPlanStore.getState().live).toEqual({ effort: 'high', thinkingForced: true });
    await vi.waitFor(() => expect(useUltraPlanStore.getState().state?.phase).toBe('running'));
    expect(gets()).toHaveLength(1);
    s.applyLiveEvent({
      sessionId: SID,
      type: 'ultraplan.stage',
      payload: { id: UP_ID, stage: 'questionnaire', phase: 'waiting', running: null },
    });
    expect(useUltraPlanStore.getState().live).toBeNull();
  });

  it('ultraplan.notice 去重入列;started / cleared 清空;别的会话 / 非 ultraplan 事件不理', () => {
    const s = useUltraPlanStore.getState();
    const notice = {
      sessionId: SID,
      type: 'ultraplan.notice',
      payload: { runId: 'run_1', id: UP_ID, code: 'THINKING_UNAVAILABLE', message: '当前模型没有思考选项' },
    };
    s.applyLiveEvent(notice);
    s.applyLiveEvent(notice);
    expect(useUltraPlanStore.getState().notices).toEqual([
      { id: UP_ID, runId: 'run_1', code: 'THINKING_UNAVAILABLE', message: '当前模型没有思考选项' },
    ]);
    const before = calls.length;
    s.applyLiveEvent({ ...notice, sessionId: 'sess_other' });
    s.applyLiveEvent({ sessionId: SID, type: 'agent.started', payload: {} });
    expect(calls.length).toBe(before);
    s.applyLiveEvent({ sessionId: SID, type: 'ultraplan.cleared', payload: { id: UP_ID } });
    expect(useUltraPlanStore.getState().notices).toEqual([]);
  });

  it('reset:流程 / 在途 / 错误 / 提示全部清空', () => {
    useUltraPlanStore.getState().hydrate(flow());
    useUltraPlanStore.setState({
      pending: { action: 'answer', rev: 2 },
      lastActionError: { code: 'X', message: 'y' },
      live: { effort: 'high', thinkingForced: true },
    });
    useUltraPlanStore.getState().reset();
    expect(useUltraPlanStore.getState()).toMatchObject({
      sessionId: null,
      state: null,
      demo: null,
      questionnaire: null,
      pending: null,
      lastActionError: null,
      live: null,
      notices: [],
    });
  });
});

describe('问卷草稿(localStorage)', () => {
  it('存 / 取 / 清;键按 id + rev 隔离', () => {
    const answers: UltraPlanAnswers = { q1: { choice: ['a'], other: '别的' }, q2: { delegate: true } };
    expect(draftKey(UP_ID, 2)).toBe(`forge:ultraplan:draft:${UP_ID}:2`);
    expect(loadDraft(UP_ID, 2)).toBeNull();
    saveDraft(UP_ID, 2, answers);
    expect(localStorage.getItem(`forge:ultraplan:draft:${UP_ID}:2`)).toBe(JSON.stringify(answers));
    expect(loadDraft(UP_ID, 2)).toEqual(answers);
    // 重出的问卷(rev 3)与别的流程互不串
    expect(loadDraft(UP_ID, 3)).toBeNull();
    expect(loadDraft('up_other', 2)).toBeNull();
    clearDraft(UP_ID, 2);
    expect(loadDraft(UP_ID, 2)).toBeNull();
  });

  it('损坏的草稿 / 非对象 → null,不抛', () => {
    localStorage.setItem(draftKey(UP_ID, 1), '{oops');
    expect(loadDraft(UP_ID, 1)).toBeNull();
    localStorage.setItem(draftKey(UP_ID, 1), '[1,2]');
    expect(loadDraft(UP_ID, 1)).toBeNull();
  });

  it('localStorage 不存在 / 访问即抛 / 写满:静默退化,不抛', () => {
    vi.stubGlobal('localStorage', undefined);
    expect(() => saveDraft(UP_ID, 1, { q1: { text: 'x' } })).not.toThrow();
    expect(loadDraft(UP_ID, 1)).toBeNull();
    expect(() => clearDraft(UP_ID, 1)).not.toThrow();

    vi.stubGlobal('localStorage', {
      getItem: () => {
        throw new Error('denied');
      },
      setItem: () => {
        throw new Error('QuotaExceededError');
      },
      removeItem: () => {
        throw new Error('denied');
      },
    });
    expect(() => saveDraft(UP_ID, 1, { q1: { text: 'x' } })).not.toThrow();
    expect(loadDraft(UP_ID, 1)).toBeNull();
    expect(() => clearDraft(UP_ID, 1)).not.toThrow();
  });
});

describe('demoUrl(契约 §8)', () => {
  const demo = { port: 41234, token: TOKEN, entry: 'index.html' };

  it('取与应用不同的回环主机名,路径 /u/<token>/,追加 ?v=<iteration>', () => {
    // 生产:应用在 127.0.0.1 → Demo 走 localhost(跨站,独立渲染进程)
    expect(demoUrl(demo, '127.0.0.1', 3, 'http://127.0.0.1:3080')).toBe(
      `http://localhost:41234/u/${TOKEN}/?v=3`,
    );
    // Vite dev:应用在 localhost → Demo 走 127.0.0.1
    expect(demoUrl(demo, 'localhost', 1, 'http://localhost:5173')).toBe(
      `http://127.0.0.1:41234/u/${TOKEN}/?v=1`,
    );
    expect(demoUrl(demo, '[::1]', 0, 'http://[::1]:3080')).toBe(`http://127.0.0.1:41234/u/${TOKEN}/?v=0`);
    // 第四参缺省取 window.location.origin(jsdom:http://localhost:3000)
    expect(demoUrl(demo, location.hostname, 2)).toBe(`http://127.0.0.1:41234/u/${TOKEN}/?v=2`);
  });

  it('没有 Demo 坐标 / 入参不成形 → null', () => {
    expect(demoUrl(null, '127.0.0.1', 1, 'http://127.0.0.1:3080')).toBeNull();
    expect(demoUrl({ ...demo, token: '' }, '127.0.0.1', 1, 'http://127.0.0.1:3080')).toBeNull();
    expect(demoUrl({ ...demo, port: 0 }, '127.0.0.1', 1, 'http://127.0.0.1:3080')).toBeNull();
    expect(demoUrl({ ...demo, port: 70000 }, '127.0.0.1', 1, 'http://127.0.0.1:3080')).toBeNull();
    expect(demoUrl({ ...demo, port: 412.5 }, '127.0.0.1', 1, 'http://127.0.0.1:3080')).toBeNull();
    expect(demoUrl(demo, '127.0.0.1', -1, 'http://127.0.0.1:3080')).toBeNull();
    expect(demoUrl(demo, '127.0.0.1', 1.5, 'http://127.0.0.1:3080')).toBeNull();
  });

  it('与应用同源 → null(iframe 带 allow-same-origin,同源等于没有沙箱)', () => {
    expect(demoUrl(demo, '127.0.0.1', 1, 'http://localhost:41234')).toBeNull();
    expect(demoUrl(demo, 'localhost', 1, 'http://127.0.0.1:41234')).toBeNull();
    expect(demoUrlAllowed(`http://localhost:41234/u/${TOKEN}/`, TOKEN, 'http://localhost:41234')).toBe(false);
  });

  it('token 夹带路径穿越 / 查询 / 片段 → 路径断言不成立 → null', () => {
    for (const token of ['../../api/forge', 'a/../../b', 'abc?x=1', 'abc#frag', '%2e%2e/%2e%2e', 'a\\..\\..']) {
      expect(demoUrl({ ...demo, token }, '127.0.0.1', 1, 'http://127.0.0.1:3080')).toBeNull();
    }
    expect(demoUrlAllowed('http://localhost:41234/api/forge/sessions', TOKEN, 'http://127.0.0.1:3080')).toBe(false);
    expect(demoUrlAllowed(`http://localhost:41234/u/${TOKEN}`, TOKEN, 'http://127.0.0.1:3080')).toBe(false);
    expect(demoUrlAllowed(`http://localhost:41234/u/${TOKEN}x/`, TOKEN, 'http://127.0.0.1:3080')).toBe(false);
  });

  it('非回环主机 / 非 http / 带凭据 / 不成 URL → 断言不成立', () => {
    const origin = 'http://127.0.0.1:3080';
    expect(demoUrlAllowed(`http://localhost:41234/u/${TOKEN}/`, TOKEN, origin)).toBe(true);
    expect(demoUrlAllowed(`http://[::1]:41234/u/${TOKEN}/`, TOKEN, origin)).toBe(true);
    expect(demoUrlAllowed(`http://evil.example:41234/u/${TOKEN}/`, TOKEN, origin)).toBe(false);
    expect(demoUrlAllowed(`http://192.168.1.20:41234/u/${TOKEN}/`, TOKEN, origin)).toBe(false);
    expect(demoUrlAllowed(`http://localhost.evil.example/u/${TOKEN}/`, TOKEN, origin)).toBe(false);
    // 端口位夹带 @ → 真正的主机变成 evil.example
    expect(demoUrlAllowed(`http://localhost:80@evil.example/u/${TOKEN}/`, TOKEN, origin)).toBe(false);
    expect(demoUrlAllowed(`http://user@localhost:41234/u/${TOKEN}/`, TOKEN, origin)).toBe(false);
    expect(demoUrlAllowed(`https://localhost:41234/u/${TOKEN}/`, TOKEN, origin)).toBe(false);
    expect(demoUrlAllowed(`file:///u/${TOKEN}/`, TOKEN, origin)).toBe(false);
    expect(demoUrlAllowed('not a url', TOKEN, origin)).toBe(false);
    expect(demoUrlAllowed('http://localhost:41234/u//', '', origin)).toBe(false);
    expect(isLoopbackHost('LOCALHOST')).toBe(true);
    expect(isLoopbackHost('127.0.0.2')).toBe(false);
  });
});

describe('cardInteractive 真值表(契约 §9)', () => {
  type Ultra = Extract<ChatBlock, { kind: 'ultraplan' }>;
  const card = (step: Ultra['step'], rev: number, patch: Partial<Ultra> = {}): Ultra => ({
    kind: 'ultraplan',
    step,
    upId: UP_ID,
    rev,
    payload: {},
    ...patch,
  });

  it('四种关口卡:流程 / 关口 / 版本全对、没在跑、没提交过 → 可操作', () => {
    expect(cardInteractive(card('questionnaire', 2), flow({ stage: 'questionnaire' }), null)).toBe(true);
    expect(cardInteractive(card('demo', 3), flow({ stage: 'demo_review' }), null)).toBe(true);
    expect(cardInteractive(card('plan', 1), flow({ stage: 'plan_review' }), null)).toBe(true);
    expect(cardInteractive(card('acceptance', 4), flow({ stage: 'acceptance' }), null)).toBe(true);
    // 上一轮失败(phase=failed)不等于在跑:卡片仍可操作
    expect(cardInteractive(card('demo', 3), flow({ stage: 'demo_review', phase: 'failed' }), null)).toBe(true);
  });

  it('任一条件不成立 → 只读', () => {
    const state = flow({ stage: 'questionnaire' });
    const live = card('questionnaire', 2);
    // 无流程(已重新开始 / fork 出来的会话)
    expect(cardInteractive(live, null, null)).toBe(false);
    // 别的流程的卡
    expect(cardInteractive(card('questionnaire', 2, { upId: 'up_old' }), state, null)).toBe(false);
    // 关口已过
    expect(cardInteractive(live, flow({ stage: 'demo_review' }), null)).toBe(false);
    // 同关口下的旧版(问卷重出过)
    expect(cardInteractive(card('questionnaire', 1), state, null)).toBe(false);
    // 流程正在跑
    expect(cardInteractive(live, flow({ stage: 'questionnaire', phase: 'running', running: 'spec_demo' }), null)).toBe(false);
    // 会话里有别的 run 在跑(activeRunId 非空)
    expect(cardInteractive(live, state, 'run_9')).toBe(false);
    // 已提交过
    expect(cardInteractive(card('questionnaire', 2, { submitted: { answers: {} } }), state, null)).toBe(false);
  });

  it('step 与关口逐一对应:错位的卡只读', () => {
    const stages = ['discovery', 'questionnaire', 'demo_review', 'plan_review', 'production', 'acceptance', 'done'] as const;
    const expected: Record<Ultra['step'], string> = {
      questionnaire: 'questionnaire',
      demo: 'demo_review',
      plan: 'plan_review',
      acceptance: 'acceptance',
      done: 'done',
    };
    const revOf: Record<Ultra['step'], number> = { questionnaire: 2, demo: 3, plan: 1, acceptance: 4, done: 0 };
    for (const step of Object.keys(expected) as Array<Ultra['step']>) {
      for (const stage of stages) {
        expect(cardInteractive(card(step, revOf[step]), flow({ stage }), null)).toBe(stage === expected[step]);
      }
    }
  });

  it('done 卡没有版本号:不比 rev', () => {
    expect(cardInteractive(card('done', 0), flow({ stage: 'done' }), null)).toBe(true);
    expect(cardInteractive(card('done', 0, { upId: 'up_old' }), flow({ stage: 'done' }), null)).toBe(false);
  });
});

describe('阶段文案', () => {
  it('stageIndex / stageLabel:七阶段顺序与短名;未知阶段不硬造', () => {
    expect(
      ['discovery', 'questionnaire', 'demo_review', 'plan_review', 'production', 'acceptance', 'done'].map(stageIndex),
    ).toEqual([0, 1, 2, 3, 4, 5, 6]);
    expect(
      ['discovery', 'questionnaire', 'demo_review', 'plan_review', 'production', 'acceptance', 'done'].map(stageLabel),
    ).toEqual(['需求', '问卷', 'Demo', '计划', '制作', '验收', '完成']);
    expect(stageIndex('nope')).toBe(-1);
    expect(stageIndex(null)).toBe(-1);
    expect(stageLabel('nope')).toBe('');
  });

  it('phaseText:在跑什么 / 在等什么 / 断在哪', () => {
    expect(phaseText(null)).toBe('');
    expect(phaseText(flow({ phase: 'running', running: 'spec_demo' }))).toBe('正在构建 Demo…');
    expect(phaseText(flow({ phase: 'running', running: 'discovery' }))).toBe('正在理解需求并起草问卷…');
    expect(phaseText(flow({ phase: 'running', running: 'planning' }))).toBe('正在编写制作计划…');
    expect(phaseText(flow({ phase: 'running', running: 'production' }))).toBe('正在制作…');
    expect(phaseText(flow({ phase: 'running', running: null }))).toBe('正在处理…');
    expect(phaseText(flow({ stage: 'questionnaire' }))).toBe('等待你填写问卷');
    expect(phaseText(flow({ stage: 'demo_review' }))).toBe('等待你试玩 Demo');
    expect(phaseText(flow({ stage: 'plan_review' }))).toBe('等待你确认计划');
    expect(phaseText(flow({ stage: 'acceptance' }))).toBe('等待你验收');
    expect(phaseText(flow({ stage: 'done' }))).toBe('已完成');
    expect(phaseText(flow({ stage: 'production', phase: 'failed' }))).toBe('制作中断');
    expect(
      phaseText(
        flow({
          stage: 'questionnaire',
          phase: 'failed',
          lastError: { code: 'ULTRAPLAN_DEMO_BUILD_FAILED', message: 'x' },
        }),
      ),
    ).toBe('Demo 构建中断');
    expect(phaseText(flow({ stage: 'demo_review', phase: 'failed', running: 'planning' }))).toBe('计划编写中断');
    expect(phaseText(flow({ stage: 'discovery', phase: 'failed' }))).toBe('上一步未完成');
  });

  it('actionErrorText:后端文案优先;空文案回落错误码', () => {
    expect(actionErrorText('ULTRAPLAN_PLAN_EDITED', '计划已被手动修改')).toBe('计划已被手动修改');
    expect(actionErrorText('ULTRAPLAN_PLAN_EDITED', '  ')).toBe('ULTRAPLAN_PLAN_EDITED');
    expect(actionErrorText('SESSION_BUSY', '会话已有运行中的 run')).toBe('有任务正在运行,请等它结束后再操作');
  });
});
