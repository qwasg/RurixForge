import { act, cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import DemoTab, { DEMO_SANDBOX, resolveDemoView } from '@/components/workbench/DemoTab';
import { useChatStore, type ChatMsg } from '@/lib/chatStore';
import { usePlanStore } from '@/lib/planStore';
import { useSessionStore } from '@/lib/sessionStore';
import { useToastStore } from '@/lib/toastStore';
import { useUltraPlanStore, type UltraPlanState } from '@/lib/ultraPlanStore';
import { useWorkbenchStore } from '@/lib/workbenchStore';

/**
 * D-044 Demo 页签:iframe 沙箱 / allow / referrer 属性与 src 形状(应用在 127.0.0.1 与 localhost
 * 两种主机名下取相反的回环名)、坐标取不到或 URL 断言不过时的错误态(warn 色,带码)、
 * 通过 / 提出修改 / 回到上一版的请求体(契约 §2 / §3)、别的会话 / 有任务在跑 / 关口不对时禁用、
 * 未自动验证的措辞、停止 / 重新加载、iframe 出错后重新解析坐标。
 * 后端 W2 并行开发中:GET …/ultraplan、ask:execute、rollback_demo 全部用 fetch 桩。
 */

const SID = 'sess_1';
const SID2 = 'sess_2';
const UP = 'up_a1';
const TOKEN = '0123456789abcdef0123456789abcdef';
const PORT = 45123;
const ASK_URL = `/api/forge/sessions/${SID}/ask:execute`;
const ROLLBACK_URL = `/api/forge/sessions/${SID}/ultraplan/rollback_demo`;
const faceUrl = (sid: string) => `/api/forge/sessions/${sid}/ultraplan`;

function flow(patch: Partial<UltraPlanState> = {}): UltraPlanState {
  return {
    id: UP,
    token: TOKEN,
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

interface Face {
  ultraplan: UltraPlanState | null;
  demo: { port: number; token: string; entry: string } | null;
  demoError?: { code: string; message: string };
}

const demo = (patch: Partial<{ port: number; token: string }> = {}) => ({
  port: PORT,
  token: TOKEN,
  entry: 'index.html',
  ...patch,
});

interface Call {
  url: string;
  method: string;
  body: unknown;
  hasBody: boolean;
}
type Reply = { status?: number; body: unknown };
type Handler = (call: Call) => Reply | Promise<Reply>;

let calls: Call[] = [];
/** 各会话 GET …/ultraplan 的当前应答。 */
let faces: Record<string, Face> = {};

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
      let reply: Reply;
      if (call.method === 'GET') {
        const sid = Object.keys(faces).find((s) => call.url === faceUrl(s));
        reply = sid ? { body: faces[sid] } : { status: 404, body: { error: { code: 'NOT_FOUND', message: 'nf' } } };
      } else {
        reply = await handler(call);
      }
      const status = reply.status ?? 200;
      return { ok: status < 400, status, json: async () => reply.body } as Response;
    }),
  );
}

const posts = (url: string) => calls.filter((c) => c.method === 'POST' && c.url === url);
const gets = (sid: string) => calls.filter((c) => c.method === 'GET' && c.url === faceUrl(sid));

/** 当前会话 = SID,流程挂上(等同快照回填,不带 Demo 坐标);GET 应答同步。 */
function mount(state: UltraPlanState | null, face: Partial<Face> = {}) {
  faces[SID] = { ultraplan: state, demo: state ? demo() : null, ...face };
  useUltraPlanStore.getState().hydrate(state, SID);
}

function renderTab(sessionId: string = SID) {
  return render(<DemoTab upId={UP} sessionId={sessionId} tabId={`demo:${UP}`} title="Demo · 塔防" />);
}

const jsdomWindow = () =>
  (globalThis as unknown as { jsdom: { reconfigure: (o: { url: string }) => void } }).jsdom;
const ORIGINAL_URL = location.href;

const initialChat = useChatStore.getState();
const initialSessions = useSessionStore.getState();
const initialToasts = useToastStore.getState();
const initialUltra = useUltraPlanStore.getState();
const initialWorkbench = useWorkbenchStore.getState();

beforeEach(() => {
  calls = [];
  faces = {};
  useUltraPlanStore.setState(initialUltra, true);
  useChatStore.setState(initialChat, true);
  // chat reset() 连带清 ultraPlanStore 的闭包态(在途槽 / 代次)
  useChatStore.getState().reset();
  useSessionStore.setState(initialSessions, true);
  useToastStore.setState(initialToasts, true);
  useWorkbenchStore.setState(initialWorkbench, true);
  usePlanStore.getState().reset();
  useSessionStore.setState({ activeSessionId: SID });
  stubFetch();
});

afterEach(() => {
  cleanup();
  vi.unstubAllGlobals();
  if (location.href !== ORIGINAL_URL) jsdomWindow().reconfigure({ url: ORIGINAL_URL });
});

describe('<DemoTab /> iframe 与地址', () => {
  it('应用在 localhost:iframe 指向 127.0.0.1(跨站);沙箱 / allow / referrer 属性按契约;挂载即解析坐标', async () => {
    expect(location.hostname).toBe('localhost');
    mount(flow());
    renderTab();
    const frame = await screen.findByTestId('demo-tab-frame');
    expect(frame.tagName).toBe('IFRAME');
    expect(frame).toHaveAttribute('src', `http://127.0.0.1:${PORT}/u/${TOKEN}/?v=2`);
    expect(frame).toHaveAttribute('sandbox', 'allow-scripts allow-same-origin allow-pointer-lock');
    expect(DEMO_SANDBOX).toBe('allow-scripts allow-same-origin allow-pointer-lock');
    // 不给 allow-popups / allow-top-navigation / allow-forms / allow-modals
    expect(frame.getAttribute('sandbox')).not.toMatch(/popups|top-navigation|forms|modals/);
    expect(frame.getAttribute('allow')).toBe('');
    expect(frame).toHaveAttribute('referrerpolicy', 'no-referrer');
    expect(frame.getAttribute('title')).toContain('塔防');
    expect(frame.getAttribute('title')).toContain('第 2 版');
    // 挂载时走 store.refresh:GET …/ultraplan 一次,坐标落 store
    expect(gets(SID)).toHaveLength(1);
    expect(useUltraPlanStore.getState().demo).toEqual(demo());

    // 在系统浏览器打开:同一地址,新窗口 + noopener noreferrer(Electron 把跨源 http 交给系统浏览器)
    const external = screen.getByTestId('demo-tab-external');
    expect(external.tagName).toBe('A');
    expect(external).toHaveAttribute('href', `http://127.0.0.1:${PORT}/u/${TOKEN}/?v=2`);
    expect(external).toHaveAttribute('target', '_blank');
    expect(external).toHaveAttribute('rel', 'noopener noreferrer');

    expect(screen.getByTestId('demo-tab-iteration')).toHaveTextContent('第 2 版');
    expect(screen.getByTestId('demo-tab-manual-note')).toHaveTextContent('自动验证包含键鼠操作，请试玩确认手感与整体体验');
    expect(screen.getByTestId('demo-tab-reset-note')).toHaveTextContent('切走页签会重置 Demo');
  });

  it('应用在 127.0.0.1:iframe 改指 localhost(与应用不同的回环名)', async () => {
    jsdomWindow().reconfigure({ url: 'http://127.0.0.1:3080/' });
    expect(location.origin).toBe('http://127.0.0.1:3080');
    mount(flow({ demoIteration: 5 }));
    renderTab();
    const frame = await screen.findByTestId('demo-tab-frame');
    expect(frame).toHaveAttribute('src', `http://localhost:${PORT}/u/${TOKEN}/?v=5`);
    expect(screen.getByTestId('demo-tab-external')).toHaveAttribute('href', `http://localhost:${PORT}/u/${TOKEN}/?v=5`);
  });

  it('首次解析回来前显示「正在获取 Demo 地址」,不误报「没有 Demo」', async () => {
    let release: () => void = () => undefined;
    const gate = new Promise<void>((resolve) => {
      release = resolve;
    });
    mount(flow());
    const base = vi.mocked(fetch);
    vi.stubGlobal(
      'fetch',
      vi.fn(async (url: unknown, init?: RequestInit) => {
        await gate;
        return base(url as string, init);
      }),
    );
    renderTab();
    expect(screen.getByTestId('demo-tab-loading')).toHaveTextContent('正在获取 Demo 地址');
    expect(screen.queryByTestId('demo-tab-error')).not.toBeInTheDocument();
    expect(screen.queryByTestId('demo-tab-frame')).not.toBeInTheDocument();
    await act(async () => release());
    expect(await screen.findByTestId('demo-tab-frame')).toBeInTheDocument();
  });
});

describe('<DemoTab /> 错误态(I-5,warn 色,带码)', () => {
  const errorCode = async () => (await screen.findByTestId('demo-tab-error')).getAttribute('data-code');

  it('Demo 宿主起不来:demo=null + demoError → DEMO_HOST_UNAVAILABLE,不出 iframe,系统浏览器钮禁用', async () => {
    mount(flow(), { demo: null, demoError: { code: 'DEMO_HOST_UNAVAILABLE', message: '端口绑定失败:拒绝访问' } });
    renderTab();
    expect(await errorCode()).toBe('DEMO_HOST_UNAVAILABLE');
    const panel = screen.getByTestId('demo-tab-error');
    expect(panel).toHaveTextContent('端口绑定失败:拒绝访问');
    expect(panel).toHaveTextContent('DEMO_HOST_UNAVAILABLE');
    expect(panel.innerHTML).toContain('text-warn');
    expect(panel.innerHTML).not.toContain('danger');
    expect(screen.queryByTestId('demo-tab-frame')).not.toBeInTheDocument();
    expect(screen.getByTestId('demo-tab-external').tagName).toBe('BUTTON');
    expect(screen.getByTestId('demo-tab-external')).toBeDisabled();
    expect(screen.getByTestId('demo-tab-stop')).toBeDisabled();
  });

  it('URL 断言不过(端口越界 / token 与流程不符)→ DEMO_URL_REJECTED,绝不把可疑地址塞进 iframe', async () => {
    mount(flow(), { demo: demo({ port: 70000 }) });
    const { unmount } = renderTab();
    expect(await errorCode()).toBe('DEMO_URL_REJECTED');
    expect(screen.queryByTestId('demo-tab-frame')).not.toBeInTheDocument();
    unmount();

    mount(flow(), { demo: demo({ token: 'ffffffffffffffffffffffffffffffff' }) });
    renderTab();
    expect(await errorCode()).toBe('DEMO_URL_REJECTED');
    expect(document.querySelector('iframe')).toBeNull();
  });

  it('流程已不存在(重新开始)→ ULTRAPLAN_FLOW_GONE;还没有 Demo → DEMO_MISSING', async () => {
    mount(null);
    const { unmount } = renderTab();
    expect(await errorCode()).toBe('ULTRAPLAN_FLOW_GONE');
    unmount();

    mount(flow({ id: 'up_other' }));
    const second = renderTab();
    expect(await errorCode()).toBe('ULTRAPLAN_FLOW_GONE');
    second.unmount();

    mount(flow(), { demo: null });
    renderTab();
    expect(await errorCode()).toBe('DEMO_MISSING');
  });

  it('页面不是经回环地址打开的(远程网关)→ DEMO_HOST_LOCAL_ONLY', async () => {
    jsdomWindow().reconfigure({ url: 'http://192.168.1.20:3080/' });
    mount(flow());
    renderTab();
    expect(await errorCode()).toBe('DEMO_HOST_LOCAL_ONLY');
    expect(document.querySelector('iframe')).toBeNull();
  });

  it('resolveDemoView:只经 demoUrl() 出地址;与应用同源的地址被拒', () => {
    const face = { state: flow(), demo: demo(), demoError: null };
    const base = { upId: UP, loading: false, face };
    expect(resolveDemoView({ ...base, locationHostname: 'localhost', locationOrigin: 'http://localhost:5173' })).toEqual({
      kind: 'ready',
      url: `http://127.0.0.1:${PORT}/u/${TOKEN}/?v=2`,
      flow: flow(),
    });
    // 应用 origin 恰好等于拼出的 Demo 源(allow-same-origin 下沙箱形同虚设)→ 断言拒绝
    expect(
      resolveDemoView({ ...base, locationHostname: '[::1]', locationOrigin: `http://127.0.0.1:${PORT}` }),
    ).toMatchObject({ kind: 'error', error: { code: 'DEMO_URL_REJECTED' } });
    expect(resolveDemoView({ ...base, face: null, locationHostname: 'localhost', locationOrigin: '' })).toMatchObject({
      kind: 'error',
      error: { code: 'DEMO_INFO_UNAVAILABLE' },
    });
    expect(
      resolveDemoView({ ...base, face: { ...face, state: flow({ demoIteration: 0 }) }, locationHostname: 'localhost', locationOrigin: '' }),
    ).toMatchObject({ kind: 'error', error: { code: 'DEMO_MISSING' } });
  });

  it('GET 流程信息失败保留实际错误,不误报流程不存在', async () => {
    mount(flow());
    vi.stubGlobal('fetch', vi.fn(async () => ({
      ok: false, status: 502,
      json: async () => ({ error: { code: 'AGENTD_OFFLINE', message: '引擎未连接' } }),
    })));
    renderTab();
    expect(await errorCode()).toBe('AGENTD_OFFLINE');
    expect(screen.getByTestId('demo-tab-error')).toHaveTextContent('引擎未连接');
    expect(screen.queryByTestId('demo-tab-frame')).not.toBeInTheDocument();
  });
});

describe('<DemoTab /> 动作与请求体', () => {
  it('通过 Demo → ask:execute {mode:ultraplan, ultraplan:{id, rev=demoIteration, action:approve_demo}},userInput 为空', async () => {
    mount(flow());
    renderTab();
    await screen.findByTestId('demo-tab-frame');
    const approve = screen.getByTestId('demo-tab-approve');
    expect(approve).toHaveTextContent('通过 Demo');
    expect(approve).toBeEnabled();
    fireEvent.click(approve);
    await waitFor(() => expect(posts(ASK_URL)).toHaveLength(1));
    expect(posts(ASK_URL)[0].body).toEqual({
      userInput: '',
      mode: 'ultraplan',
      ultraplan: { id: UP, rev: 2, action: 'approve_demo' },
    });
    expect(await screen.findByTestId('demo-tab-decision-note')).toHaveTextContent('已提交,等待处理');
  });

  it('提出修改:空意见不能提交;提交 → revise_demo,意见作 userInput(去首尾空白)', async () => {
    mount(flow());
    renderTab();
    await screen.findByTestId('demo-tab-frame');
    fireEvent.click(screen.getByTestId('demo-tab-revise-toggle'));
    const submit = screen.getByTestId('demo-tab-revise-submit');
    expect(submit).toBeDisabled();
    expect(submit).toHaveAttribute('title', '请先填写修改意见');
    fireEvent.change(screen.getByTestId('demo-tab-revise-input'), { target: { value: '   ' } });
    expect(submit).toBeDisabled();
    fireEvent.click(submit);
    expect(posts(ASK_URL)).toHaveLength(0);

    fireEvent.change(screen.getByTestId('demo-tab-revise-input'), { target: { value: '  敌人再慢一点,加暂停键 ' } });
    expect(submit).toBeEnabled();
    fireEvent.click(submit);
    await waitFor(() => expect(posts(ASK_URL)).toHaveLength(1));
    expect(posts(ASK_URL)[0].body).toEqual({
      userInput: '敌人再慢一点,加暂停键',
      mode: 'ultraplan',
      ultraplan: { id: UP, rev: 2, action: 'revise_demo' },
    });
    // 受理后收起文本框
    await waitFor(() => expect(screen.queryByTestId('demo-tab-revise-input')).not.toBeInTheDocument());
  });

  it('被拒(409):原因就地显示(warn 色),store 同时 warning toast', async () => {
    stubFetch(() => ({
      status: 409,
      body: {
        error: {
          code: 'ULTRAPLAN_STAGE_MISMATCH',
          message: 'Demo 已不是当前版',
          details: { stage: 'demo_review', allowed: ['approve_demo', 'revise_demo'] },
        },
      },
    }));
    mount(flow());
    renderTab();
    await screen.findByTestId('demo-tab-frame');
    fireEvent.click(screen.getByTestId('demo-tab-approve'));
    const error = await screen.findByTestId('demo-tab-decision-error');
    expect(error).toHaveTextContent('Demo 已不是当前版');
    expect(error.className).toContain('text-warn');
    expect(error.className).not.toContain('danger');
    expect(useToastStore.getState().items.some((t) => t.kind === 'warning' && t.title === 'Demo 已不是当前版')).toBe(
      true,
    );
    await waitFor(() => expect(screen.getByTestId('demo-tab-approve')).toBeEnabled());
  });

  it('回到上一版:仅第 2 版起出现;就地确认,取消不发;确认带当前flow/demo版本,iframe 换到新一版', async () => {
    stubFetch((call) => {
      if (call.url === ROLLBACK_URL) {
        const next = flow({ demoIteration: 3, demoVerified: false, demoNote: '已回退到第 1 版(未重新验证)' });
        faces[SID] = { ultraplan: next, demo: demo() };
        return { body: { ultraplan: next } };
      }
      return { body: { run: { id: 'run_s', status: 'completed' } } };
    });
    mount(flow());
    renderTab();
    const first = await screen.findByTestId('demo-tab-frame');
    const rollback = screen.getByTestId('demo-tab-rollback');
    expect(rollback).toHaveTextContent('回到上一版');
    fireEvent.click(rollback);
    expect(screen.getByTestId('demo-tab-rollback-confirm')).toHaveTextContent('不会重新自动验证');
    fireEvent.click(screen.getByTestId('demo-tab-rollback-cancel'));
    expect(screen.queryByTestId('demo-tab-rollback-confirm')).not.toBeInTheDocument();
    expect(posts(ROLLBACK_URL)).toHaveLength(0);

    fireEvent.click(rollback);
    const ok = screen.getByTestId('demo-tab-rollback-ok');
    expect(ok.className).toContain('text-warn');
    expect(ok.className).not.toContain('danger');
    fireEvent.click(ok);
    await waitFor(() => expect(posts(ROLLBACK_URL)).toHaveLength(1));
    expect(posts(ROLLBACK_URL)[0].body).toEqual({ id: UP, rev: 2 });
    await waitFor(() => expect(screen.getByTestId('demo-tab-iteration')).toHaveTextContent('第 3 版'));
    const next = screen.getByTestId('demo-tab-frame');
    expect(next).toHaveAttribute('src', `http://127.0.0.1:${PORT}/u/${TOKEN}/?v=3`);
    // 按迭代号换 key:是一个新的 iframe 元素
    expect(next).not.toBe(first);
    expect(screen.getByTestId('demo-tab-verified')).toHaveTextContent('未自动验证');
    expect(screen.getByTestId('demo-tab-approve')).toHaveTextContent('未自动验证,仍然通过');
    expect(screen.queryByTestId('demo-tab-rollback-confirm')).not.toBeInTheDocument();
    expect(posts(ASK_URL)).toHaveLength(0);
  });

  it('第 1 版没有「回到上一版」', async () => {
    mount(flow({ demoIteration: 1 }));
    renderTab();
    await screen.findByTestId('demo-tab-frame');
    expect(screen.queryByTestId('demo-tab-rollback')).not.toBeInTheDocument();
  });
});

describe('<DemoTab /> 禁用条件(可看不可点)', () => {
  const actionButtons = () => [
    screen.getByTestId('demo-tab-approve'),
    screen.getByTestId('demo-tab-revise-toggle'),
    screen.getByTestId('demo-tab-rollback'),
  ];

  it('页签属于别的会话:照样加载那个会话的 Demo(不动 store),动作全部禁用并说明「该 Demo 属于另一个会话」', async () => {
    faces[SID] = { ultraplan: flow(), demo: demo() };
    faces[SID2] = { ultraplan: null, demo: null };
    useSessionStore.setState({ activeSessionId: SID2 });
    useUltraPlanStore.getState().hydrate(null, SID2);
    renderTab(SID);
    const frame = await screen.findByTestId('demo-tab-frame');
    expect(frame).toHaveAttribute('src', `http://127.0.0.1:${PORT}/u/${TOKEN}/?v=2`);
    expect(screen.getByTestId('demo-tab')).toHaveAttribute('data-own', '0');
    expect(gets(SID)).toHaveLength(1);
    // 只读取回,不落 store(store 仍是当前会话的)
    expect(useUltraPlanStore.getState().sessionId).toBe(SID2);
    expect(useUltraPlanStore.getState().state).toBeNull();
    for (const button of actionButtons()) {
      expect(button).toBeDisabled();
      expect(button).toHaveAttribute('title', '该 Demo 属于另一个会话');
    }
    expect(screen.getByTestId('demo-tab-decision-note')).toHaveTextContent('该 Demo 属于另一个会话');
    fireEvent.click(screen.getByTestId('demo-tab-approve'));
    expect(calls.filter((c) => c.method === 'POST')).toHaveLength(0);
    // 查看类工具照常
    expect(screen.getByTestId('demo-tab-reload')).toBeEnabled();
    expect(screen.getByTestId('demo-tab-stop')).toBeEnabled();
  });

  it('有任务在跑 / 流程在跑 / 关口不对:禁用并各自说明;恢复后可点', async () => {
    mount(flow());
    renderTab();
    await screen.findByTestId('demo-tab-frame');
    for (const button of actionButtons()) expect(button).toBeEnabled();

    act(() => useChatStore.setState({ activeRunId: 'run_9' }));
    for (const button of actionButtons()) {
      expect(button).toBeDisabled();
      expect(button).toHaveAttribute('title', '有任务正在运行');
    }
    act(() => useChatStore.setState({ activeRunId: null }));
    for (const button of actionButtons()) expect(button).toBeEnabled();

    act(() => mount(flow({ phase: 'running', running: 'planning' })));
    expect(screen.getByTestId('demo-tab-approve')).toBeDisabled();
    expect(screen.getByTestId('demo-tab-approve').getAttribute('title')).toContain('流程正在处理');

    act(() => mount(flow({ stage: 'plan_review', planPath: '.forge/plans/td-a1b2.plan.md', planRev: 1 })));
    for (const button of actionButtons()) {
      expect(button).toBeDisabled();
      expect(button).toHaveAttribute('title', '流程已进入「计划」阶段');
    }
    // 过了关口仍能看
    expect(screen.getByTestId('demo-tab-frame')).toBeInTheDocument();
  });

  it('流程换了(重新开始后的新流程)→ 该 Demo 不属于当前流程,不加载新流程的 Demo', async () => {
    mount(flow({ id: 'up_new' }));
    renderTab();
    expect(await screen.findByTestId('demo-tab-error')).toHaveAttribute('data-code', 'ULTRAPLAN_FLOW_GONE');
    expect(screen.getByTestId('demo-tab-approve')).toBeDisabled();
    expect(screen.getByTestId('demo-tab-approve').getAttribute('title')).toContain('不属于当前流程');
  });
});

describe('<DemoTab /> 验证徽标与探测报错', () => {
  it('未自动验证:徽标「未自动验证」(说明在悬停提示,warn 色),通过钮读作「未自动验证,仍然通过」', async () => {
    mount(flow({ demoVerified: false, demoNote: 'WEB_PROBE_UNAVAILABLE:没有找到 Edge / Chrome' }));
    renderTab();
    await screen.findByTestId('demo-tab-frame');
    const badge = screen.getByTestId('demo-tab-verified');
    expect(badge).toHaveTextContent('未自动验证');
    expect(badge).toHaveAttribute('data-verified', '0');
    expect(badge).toHaveAttribute('title', 'WEB_PROBE_UNAVAILABLE:没有找到 Edge / Chrome');
    expect(badge.className).toContain('text-warn');
    expect(badge.className).not.toContain('danger');
    expect(screen.getByTestId('demo-tab-approve')).toHaveTextContent('未自动验证,仍然通过');
    expect(screen.getByTestId('demo-tab-approve')).toHaveAttribute('data-verified', '0');

    act(() => mount(flow({ demoVerified: true })));
    expect(screen.getByTestId('demo-tab-verified')).toHaveTextContent('已自动验证');
    expect(screen.getByTestId('demo-tab-approve')).toHaveTextContent('通过 Demo');
    expect(screen.getByTestId('demo-tab-approve')).not.toHaveTextContent('仍然');
  });

  it('探测报错取自本会话这一版的 Demo 卡,默认收起,可展开', async () => {
    const msg: ChatMsg = {
      id: 'assistant-1',
      role: 'assistant',
      text: '',
      blocks: [
        {
          kind: 'ultraplan',
          step: 'demo',
          upId: UP,
          rev: 2,
          payload: { id: UP, iteration: 2, verified: false, probe: { ok: false, errors: ['TypeError: x is undefined', 'CSP: blocked https://cdn.example'] } },
        },
      ],
      status: 'completed',
      time: '10:24',
      runId: 'run_s',
    };
    useChatStore.setState({ messages: [msg] });
    mount(flow({ demoVerified: false }));
    renderTab();
    await screen.findByTestId('demo-tab-frame');
    const toggle = screen.getByTestId('demo-tab-probe-errors-toggle');
    expect(toggle).toHaveTextContent('探测报错 2 条');
    expect(screen.queryByTestId('demo-tab-probe-errors-list')).not.toBeInTheDocument();
    fireEvent.click(toggle);
    const list = screen.getByTestId('demo-tab-probe-errors-list');
    expect(list).toHaveTextContent('TypeError: x is undefined');
    expect(list).toHaveTextContent('CSP: blocked https://cdn.example');
  });
});

describe('<DemoTab /> 停止 / 重新加载 / 出错重解析', () => {
  it('停止 = 卸载 iframe 并提示「已停止,点重新加载恢复」;重新加载 = 重新解析 + 新 iframe', async () => {
    mount(flow());
    renderTab();
    const first = await screen.findByTestId('demo-tab-frame');
    expect(gets(SID)).toHaveLength(1);
    fireEvent.click(screen.getByTestId('demo-tab-stop'));
    expect(screen.queryByTestId('demo-tab-frame')).not.toBeInTheDocument();
    expect(document.querySelector('iframe')).toBeNull();
    expect(screen.getByTestId('demo-tab-stopped')).toHaveTextContent('已停止,点重新加载恢复');
    expect(screen.getByTestId('demo-tab-stop')).toBeDisabled();

    fireEvent.click(screen.getByTestId('demo-tab-reload'));
    const again = await screen.findByTestId('demo-tab-frame');
    expect(again).not.toBe(first);
    expect(screen.queryByTestId('demo-tab-stopped')).not.toBeInTheDocument();
    await waitFor(() => expect(gets(SID)).toHaveLength(2));

    // 运行中直接重新加载也换一个新 iframe(从头开始)
    fireEvent.click(screen.getByTestId('demo-tab-reload'));
    await waitFor(() => expect(screen.getByTestId('demo-tab-frame')).not.toBe(again));
  });

  it('iframe 加载出错 → 重新解析坐标;端口变了(引擎重启)src 随之更新', async () => {
    mount(flow());
    renderTab();
    const frame = await screen.findByTestId('demo-tab-frame');
    expect(gets(SID)).toHaveLength(1);
    faces[SID] = { ultraplan: flow(), demo: demo({ port: 50999 }) };
    fireEvent.error(frame);
    await waitFor(() => expect(gets(SID)).toHaveLength(2));
    await waitFor(() =>
      expect(screen.getByTestId('demo-tab-frame')).toHaveAttribute('src', `http://127.0.0.1:50999/u/${TOKEN}/?v=2`),
    );
  });

  it('流程标题回填 tabbar 文案', async () => {
    useWorkbenchStore.getState().openDemo(UP, SID, '');
    expect(useWorkbenchStore.getState().tabs[0].title).toBe('Demo');
    mount(flow({ title: '星际塔防' }));
    renderTab();
    await screen.findByTestId('demo-tab-frame');
    await waitFor(() => expect(useWorkbenchStore.getState().tabs[0].title).toBe('Demo · 星际塔防'));
  });
});
