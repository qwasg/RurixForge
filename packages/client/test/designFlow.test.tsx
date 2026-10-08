import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import DesignBlock, { designBlockReason } from '@/components/chat/design/DesignBlock';
import { COMPOSER_MODES, modesForKind } from '@/components/chat/composerModes';
import { useChatStore } from '@/lib/chatStore';
import {
  designFileUrl,
  designPhaseText,
  normalizeDesignState,
  useDesignFlowStore,
  type DesignState,
} from '@/lib/designFlowStore';
import { useSessionStore } from '@/lib/sessionStore';
import { useToastStore } from '@/lib/toastStore';
import type { ForgeEventWire } from '@/lib/chatStore';
import type { ChatBlock } from '@/lib/timeline';

/**
 * D-045 Design 流程前端:状态归一、模式注册、design.* 事件建卡与回填决定、
 * 审阅卡的选中 / 采用 / 修改请求体、只读与禁用原因、验收卡的对比件。后端以 fetch 桩代替。
 */

type Block = Extract<ChatBlock, { kind: 'design' }>;

const SID = 'sess_d';
const FLOW = 'dz_1';
const DIR = '.forge/design/menu-ab12';
const ASK_URL = `/api/forge/sessions/${SID}/ask:execute`;

function flow(patch: Partial<DesignState> = {}): DesignState {
  return {
    id: FLOW,
    slug: 'menu-ab12',
    dir: DIR,
    title: '赛博朋克主菜单',
    workspaceId: null,
    stage: 'design_review',
    phase: 'waiting',
    running: null,
    lastError: null,
    designRev: 1,
    candidates: [0, 2],
    selected: 0,
    designType: 'ui',
    aspect: 'landscape',
    approved: null,
    replicationRound: 0,
    layoutReady: false,
    assetsReady: false,
    scenePath: null,
    verifyCount: 0,
    lastVerify: null,
    passed: null,
    ...patch,
  };
}

function review(patch: Partial<Block> = {}): Block {
  return {
    kind: 'design',
    step: 'review',
    flowId: FLOW,
    rev: 1,
    payload: {
      id: FLOW,
      rev: 1,
      summary: '两种配色方向',
      width: 1536,
      height: 1024,
      candidates: [
        { index: 0, path: `${DIR}/rounds/r1/c0.png`, prompt: 'neon menu' },
        { index: 2, path: `${DIR}/rounds/r1/c2.png`, prompt: 'dark menu' },
      ],
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

function stubFetch(status = 200) {
  vi.stubGlobal(
    'fetch',
    vi.fn(async (url: unknown, init?: RequestInit) => {
      const call: Call = {
        url: String(url),
        method: init?.method ?? 'GET',
        body: init?.body ? JSON.parse(String(init.body)) : undefined,
      };
      calls.push(call);
      const reply =
        call.method === 'GET'
          ? { design: useDesignFlowStore.getState().state }
          : status < 400
            ? { ok: true, run: { id: 'run_x', status: 'completed' } }
            : { error: { code: 'DESIGN_STAGE_MISMATCH', message: '版本已过期' } };
      const code = call.method === 'GET' ? 200 : status;
      return { ok: code < 400, status: code, json: async () => reply } as Response;
    }),
  );
}

let seq = 0;
function evt(type: string, payload: Record<string, unknown>): ForgeEventWire {
  seq += 1;
  return { id: `e_${seq}`, sessionId: SID, seq, type, ts: '2026-10-06T08:00:00.000Z', payload };
}

const initialChat = useChatStore.getState();
const initialSessions = useSessionStore.getState();
const initialToasts = useToastStore.getState();
const initialDesign = useDesignFlowStore.getState();

beforeEach(() => {
  calls = [];
  useDesignFlowStore.setState(initialDesign, true);
  useChatStore.setState(initialChat, true);
  useChatStore.getState().reset();
  useSessionStore.setState(initialSessions, true);
  useToastStore.setState(initialToasts, true);
  useSessionStore.setState({ activeSessionId: SID });
  useDesignFlowStore.getState().hydrate(flow(), SID);
  stubFetch();
});

afterEach(() => {
  cleanup();
  vi.unstubAllGlobals();
});

describe('designFlowStore 归一与文案', () => {
  it('后端状态按「可能缺字段」读;未知阶段回落 concept', () => {
    const s = normalizeDesignState({ id: 'dz', stage: 'weird', candidates: [1, 'x', 3], lastVerify: { n: 2, passed: true } });
    expect(s?.stage).toBe('concept');
    expect(s?.candidates).toEqual([1, 3]);
    expect(s?.lastVerify).toEqual({ n: 2, passed: true, globalSsim: 0, failedElements: 0 });
    expect(normalizeDesignState({ id: '' })).toBeNull();
    expect(designPhaseText(flow({ phase: 'running', running: 'replication' }))).toContain('复刻');
    expect(designFileUrl('s 1', 'a/b.png')).toBe('/api/forge/sessions/s%201/design/file?path=a%2Fb.png');
  });

  it('Design 模式对 coding 代理与两种引擎都可选,通用 / 文档代理不给', () => {
    expect(COMPOSER_MODES.some((m) => m.id === 'design')).toBe(true);
    expect(modesForKind('coding', 'local').some((m) => m.id === 'design')).toBe(true);
    expect(modesForKind('coding', 'codex').some((m) => m.id === 'design')).toBe(true);
    expect(modesForKind('general', 'local').some((m) => m.id === 'design')).toBe(false);
  });
});

describe('chatStore:design.* 建卡与回填', () => {
  it('review.ready 建审阅卡;重复到达原地替换;decision 回填只读', () => {
    const s = useChatStore.getState();
    s.applyEvent(evt('agent.started', { runId: 'run_1' }));
    const payload = { ...review().payload, runId: 'run_1' };
    s.applyEvent(evt('design.review.ready', payload));
    s.applyEvent(evt('design.review.ready', payload));
    const blocks = () =>
      useChatStore.getState().messages.flatMap((m) => m.blocks).filter((b): b is Block => b.kind === 'design');
    expect(blocks()).toHaveLength(1);
    expect(blocks()[0]).toMatchObject({ step: 'review', flowId: FLOW, rev: 1 });
    s.applyEvent(evt('design.decision', { id: FLOW, rev: 1, action: 'approve_design', candidate: 2, runId: 'run_2' }));
    expect(blocks()[0].submitted).toMatchObject({ action: 'approve_design', candidate: 2 });
    s.applyEvent(evt('design.verify.result', { id: FLOW, n: 1, passed: false, runId: 'run_1' }));
    expect(blocks().map((b) => b.step)).toEqual(['review', 'verify']);
  });
});

describe('<DesignBlock /> 审阅卡', () => {
  it('列出候选图(经 design/file 取图),默认选中状态里的那张', () => {
    render(<DesignBlock block={review()} />);
    expect(screen.getByTestId('design-block')).toHaveAttribute('data-step', 'review');
    const imgs = screen.getAllByRole('img');
    expect(imgs[0].getAttribute('src')).toBe(designFileUrl(SID, `${DIR}/rounds/r1/c0.png`));
    expect(screen.getByTestId('design-candidate-0')).toHaveAttribute('data-selected', '1');
    expect(screen.getByTestId('design-candidate-2')).toHaveAttribute('data-selected', '0');
    expect(screen.getByTestId('design-review-card')).toHaveTextContent('两种配色方向');
  });

  it('点候选 = 持久化选中;采用 → ask:execute{mode:design, design:{id,rev,action,candidate}}', async () => {
    render(<DesignBlock block={review()} />);
    fireEvent.click(screen.getByTestId('design-candidate-2'));
    await waitFor(() => expect(calls.some((c) => c.url.endsWith('/design/select'))).toBe(true));
    expect(calls.find((c) => c.url.endsWith('/design/select'))?.body).toEqual({ id: FLOW, rev: 1, candidate: 2 });
    fireEvent.click(screen.getByTestId('design-approve'));
    await waitFor(() => expect(calls.some((c) => c.url === ASK_URL)).toBe(true));
    expect(calls.find((c) => c.url === ASK_URL)?.body).toEqual({
      userInput: '',
      mode: 'design',
      design: { id: FLOW, rev: 1, action: 'approve_design', candidate: 2 },
    });
  });

  it('提出修改须写意见,带选中候选与正文', async () => {
    render(<DesignBlock block={review()} />);
    fireEvent.click(screen.getByTestId('design-revise-toggle'));
    expect(screen.getByTestId('design-revise-submit')).toBeDisabled();
    fireEvent.change(screen.getByTestId('design-revise-input'), { target: { value: '标题再大一点' } });
    fireEvent.click(screen.getByTestId('design-revise-submit'));
    await waitFor(() => expect(calls.some((c) => c.url === ASK_URL)).toBe(true));
    expect(calls.find((c) => c.url === ASK_URL)?.body).toMatchObject({
      userInput: '标题再大一点',
      design: { action: 'revise_design', candidate: 0, rev: 1 },
    });
  });

  it('已决定的卡只读;过期批次 / 运行中禁用并写明原因', () => {
    render(<DesignBlock block={review({ submitted: { action: 'revise_design', candidate: 0, feedback: '更亮' } })} />);
    expect(screen.getByTestId('design-review-decided')).toHaveTextContent('已提出修改 #0:更亮');
    expect(screen.queryByTestId('design-approve')).not.toBeInTheDocument();
    cleanup();
    useDesignFlowStore.getState().hydrate(flow({ designRev: 2 }), SID);
    render(<DesignBlock block={review()} />);
    expect(screen.getByTestId('design-approve')).toBeDisabled();
    expect(screen.getByTestId('design-review-note')).toHaveTextContent('这一批设计稿已处理');
  });

  it('designBlockReason 优先级:不属于当前流程 → 阶段不对 → 运行中 → 有 run → 在途', () => {
    const base = { activeRunId: null, pending: false, unsupported: null };
    const b = { flowId: FLOW, step: 'review' as const, rev: 1 };
    expect(designBlockReason(b, { ...base, flow: null })).toContain('不属于当前流程');
    expect(designBlockReason(b, { ...base, flow: flow({ stage: 'replication' }) })).toBe('这一批设计稿已处理');
    expect(designBlockReason(b, { ...base, flow: flow({ phase: 'running', running: 'revise' }) })).toContain('修改');
    expect(designBlockReason(b, { ...base, flow: flow(), activeRunId: 'r' })).toBe('有任务正在运行');
    expect(designBlockReason(b, { ...base, flow: flow(), pending: true })).toBe('上一个操作还在提交中');
    expect(designBlockReason(b, { ...base, flow: flow() })).toBeNull();
  });

  it('被拒的动作就地报错并弹提示', async () => {
    stubFetch(409);
    render(<DesignBlock block={review()} />);
    fireEvent.click(screen.getByTestId('design-approve'));
    await waitFor(() => expect(screen.getByRole('alert')).toHaveTextContent('流程状态已变化'));
    expect(useToastStore.getState().items.length).toBeGreaterThan(0);
  });
});

describe('<DesignBlock /> 验收卡', () => {
  it('滑块对比引擎截帧与定稿;可切差异图;列出未对上的元素', () => {
    const shots = {
      frame: { path: `${DIR}/verify/1/frame.png` },
      mockup: { path: `${DIR}/verify/1/mockup.png` },
      diff: { path: `${DIR}/verify/1/diff.png` },
    };
    render(
      <DesignBlock
        block={{
          kind: 'design',
          step: 'verify',
          flowId: FLOW,
          rev: 1,
          payload: { id: FLOW, n: 1, passed: false, failed: ['btn_start'], global: { ssim: 0.8, colorDiff: 9, ssimMin: 0.85, colorDiffMax: 18 }, screenshots: shots },
        }}
      />,
    );
    expect(screen.getByTestId('design-verify-card')).toHaveTextContent('未通过');
    expect(screen.getByTestId('design-verify-failed')).toHaveTextContent('btn_start');
    expect(screen.getByTestId('design-compare')).toBeInTheDocument();
    fireEvent.click(screen.getByTestId('design-diff-toggle'));
    expect(screen.getByAltText('差异热力图').getAttribute('src')).toBe(designFileUrl(SID, shots.diff.path));
  });
});
