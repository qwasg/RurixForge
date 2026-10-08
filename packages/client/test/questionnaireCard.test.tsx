import { act, cleanup, fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import UltraPlanBlock from '@/components/chat/ultraplan/UltraPlanBlock';
import {
  answerStatus,
  buildAnswers,
  delegateRest,
  multiBounds,
  normalizeQuestionnaire,
  sanitizeDraft,
} from '@/components/chat/ultraplan/questionnaireModel';
import { useChatStore } from '@/lib/chatStore';
import { useSessionStore } from '@/lib/sessionStore';
import { useToastStore } from '@/lib/toastStore';
import type { ChatBlock } from '@/lib/timeline';
import {
  draftKey,
  loadDraft,
  saveDraft,
  useUltraPlanStore,
  type UltraPlanAnswers,
  type UltraPlanState,
} from '@/lib/ultraPlanStore';

/**
 * D-044 问卷卡:四种题型的控件、必答门槛、「交给你决定」与「其余全部交给你决定」、「其他」自填、
 * multi 的数量界、分节导航、提交请求体(契约 §2)、草稿(localStorage)重挂恢复、只读形态、被拒回显。
 * 后端未实现:ask:execute / GET …/ultraplan 全部用 fetch 桩,请求体对着 wire 契约断言。
 */

type Ultra = Extract<ChatBlock, { kind: 'ultraplan' }>;

const SID = 'sess_1';
const UP = 'up_a1';
const ASK_URL = `/api/forge/sessions/${SID}/ask:execute`;

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
    questionnaireRev: 1,
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

/** 两节五题,四种题型各至少一道;q_name 必答且不许委托,q_note 选答。 */
const questionnaire = {
  title: '塔防问卷',
  understanding: '## 我的理解\n做一个**塔防**小游戏',
  sections: [
    {
      id: 's1',
      title: '玩法',
      questions: [
        {
          id: 'q_view',
          kind: 'single',
          question: '用什么视角?',
          help: '决定相机与美术方向',
          options: [
            { id: 'top', label: '俯视', description: '经典塔防视角', recommended: true },
            { id: 'side', label: '横版' },
          ],
          allowOther: true,
        },
        {
          id: 'q_towers',
          kind: 'multi',
          question: '要哪些塔?',
          options: [
            { id: 'arrow', label: '箭塔' },
            { id: 'cannon', label: '炮塔' },
            { id: 'ice', label: '冰塔' },
            { id: 'fire', label: '火塔' },
          ],
          min: 2,
          max: 3,
          allowOther: true,
        },
      ],
    },
    {
      id: 's2',
      title: '节奏',
      questions: [
        { id: 'q_diff', kind: 'scale', question: '难度?', min: 1, max: 5, scaleLabels: ['休闲', '硬核'] },
        { id: 'q_note', kind: 'text', question: '还有什么想法?' },
        { id: 'q_name', kind: 'text', question: '游戏叫什么?', required: true, allowDelegate: false },
      ],
    },
  ],
};

function card(patch: Partial<Ultra> = {}): Ultra {
  return {
    kind: 'ultraplan',
    step: 'questionnaire',
    upId: UP,
    rev: 1,
    payload: { runId: 'run_d', id: UP, rev: 1, path: '.forge/ultraplan/td-a1b2/questionnaire.json', questionnaire },
    ...patch,
  };
}

interface Call {
  url: string;
  method: string;
  body: unknown;
}
type Reply = { status?: number; body: unknown };
type Handler = (call: Call) => Reply | Promise<Reply>;

let calls: Call[] = [];

function stubFetch(handler: Handler) {
  vi.stubGlobal(
    'fetch',
    vi.fn(async (url: unknown, init?: RequestInit) => {
      const call: Call = {
        url: String(url),
        method: init?.method ?? 'GET',
        body: init?.body ? JSON.parse(String(init.body)) : undefined,
      };
      calls.push(call);
      const reply = await handler(call);
      const status = reply.status ?? 200;
      return { ok: status < 400, status, json: async () => reply.body } as Response;
    }),
  );
}

const asks = () => calls.filter((c) => c.url === ASK_URL);
const click = (testId: string) => fireEvent.click(screen.getByTestId(testId));
const typeInto = (testId: string, value: string) =>
  fireEvent.change(screen.getByTestId(testId), { target: { value } });

/** 把五道题全部答好(停在第二节)。 */
function fillAll() {
  click('q-q_view-opt-top');
  click('q-q_towers-opt-ice');
  click('q-q_towers-opt-arrow');
  click('questionnaire-next');
  click('q-q_diff-scale-4');
  typeInto('q-q_name-text', '守城');
}

const initialChat = useChatStore.getState();
const initialSessions = useSessionStore.getState();
const initialToasts = useToastStore.getState();
const initialUltra = useUltraPlanStore.getState();

beforeEach(() => {
  calls = [];
  localStorage.clear();
  useUltraPlanStore.setState(initialUltra, true);
  useChatStore.setState(initialChat, true);
  // chat reset() 连带清 ultraPlanStore 的闭包态(在途槽 / 代次)
  useChatStore.getState().reset();
  useSessionStore.setState(initialSessions, true);
  useToastStore.setState(initialToasts, true);
  useSessionStore.setState({ activeSessionId: SID });
  useUltraPlanStore.getState().hydrate(flow(), SID);
  stubFetch((call) =>
    call.url === ASK_URL ? { body: { run: { id: 'run_s', status: 'completed' } } } : { body: { ultraplan: flow() } },
  );
});

afterEach(() => {
  cleanup();
  vi.unstubAllGlobals();
});

describe('<QuestionnaireCard /> 表单', () => {
  it('头部标题 + 进度;「项目理解」可折叠;四种题型各出对应控件', () => {
    render(<UltraPlanBlock block={card()} />);
    const root = screen.getByTestId('questionnaire-card');
    expect(root).toHaveAttribute('data-mode', 'form');
    expect(root).toHaveAttribute('data-interactive', '1');
    expect(root).toHaveTextContent('塔防问卷');
    expect(screen.getByTestId('questionnaire-progress')).toHaveTextContent('已答 0/5');

    // 项目理解:markdown 渲染(标题标记不外露),可收起
    const understanding = screen.getByTestId('questionnaire-understanding');
    expect(understanding).toHaveTextContent('我的理解');
    expect(understanding.textContent).not.toContain('##');
    expect(understanding.textContent).not.toContain('**');
    const toggle = screen.getByTestId('questionnaire-understanding-toggle');
    expect(toggle).toHaveAttribute('aria-expanded', 'true');
    fireEvent.click(toggle);
    expect(screen.queryByTestId('questionnaire-understanding')).not.toBeInTheDocument();
    expect(toggle).toHaveAttribute('aria-expanded', 'false');

    // 一次只展开一节:第一节两题在,第二节的题不在
    expect(screen.getByTestId('questionnaire-section-title')).toHaveTextContent('玩法');
    expect(screen.queryByTestId('q-q_diff')).not.toBeInTheDocument();

    // single:radiogroup + 选项卡(label / 描述 / 推荐)+ 说明 + 必答星标
    const single = screen.getByTestId('q-q_view');
    expect(single).toHaveTextContent('用什么视角?');
    expect(single).toHaveTextContent('*');
    expect(single).toHaveTextContent('决定相机与美术方向');
    const radios = within(single).getAllByRole('radio');
    expect(radios).toHaveLength(2);
    expect(within(single).getByRole('radiogroup')).toBeInTheDocument();
    expect(screen.getByTestId('q-q_view-opt-top')).toHaveTextContent('俯视');
    expect(screen.getByTestId('q-q_view-opt-top')).toHaveTextContent('经典塔防视角');
    expect(screen.getByTestId('q-q_view-opt-top-recommended')).toHaveTextContent('推荐');
    expect(screen.queryByTestId('q-q_view-opt-side-recommended')).not.toBeInTheDocument();
    expect(screen.getByTestId('q-q_view-other')).toBeInTheDocument();
    expect(screen.getByTestId('q-q_view-delegate')).toHaveTextContent('交给你决定');

    // multi:checkbox 语义 + 数量界说明
    const multi = screen.getByTestId('q-q_towers');
    expect(within(multi).getAllByRole('checkbox')).toHaveLength(4);
    expect(screen.getByTestId('q-q_towers-bounds')).toHaveTextContent('选 2–3 项');

    // 第二节:scale 分段按钮(两端标签)+ text 文本框;选答题无星标,不许委托的题没有委托钮
    click('questionnaire-next');
    expect(screen.getByTestId('questionnaire-section-title')).toHaveTextContent('节奏');
    const scale = screen.getByTestId('q-q_diff');
    expect(within(scale).getAllByRole('radio').map((el) => el.textContent)).toEqual(['1', '2', '3', '4', '5']);
    expect(screen.getByTestId('q-q_diff-scale-label-min')).toHaveTextContent('休闲');
    expect(screen.getByTestId('q-q_diff-scale-label-max')).toHaveTextContent('硬核');
    expect(screen.getByTestId('q-q_note-text').tagName).toBe('TEXTAREA');
    expect(screen.getByTestId('q-q_note')).not.toHaveTextContent('*');
    expect(screen.getByTestId('q-q_name')).toHaveTextContent('*');
    expect(screen.getByTestId('q-q_note-delegate')).toBeInTheDocument();
    expect(screen.queryByTestId('q-q_name-delegate')).not.toBeInTheDocument();
  });

  it('必答门槛:没答完不能提交,并指出哪一节还有缺口;答完即可提交', () => {
    render(<UltraPlanBlock block={card()} />);
    const submit = screen.getByTestId('questionnaire-submit');
    expect(submit).toBeDisabled();
    expect(screen.getByTestId('questionnaire-gap')).toHaveTextContent('第 1 节「玩法」还有 2 题未完成,另有 1 节待补');
    expect(submit).toHaveAttribute('title', expect.stringContaining('第 1 节'));

    click('q-q_view-opt-top');
    expect(screen.getByTestId('q-q_view-opt-top')).toHaveAttribute('aria-checked', 'true');
    // 单选:再点另一项即换选
    click('q-q_view-opt-side');
    expect(screen.getByTestId('q-q_view-opt-top')).toHaveAttribute('aria-checked', 'false');
    expect(screen.getByTestId('q-q_view-opt-side')).toHaveAttribute('aria-checked', 'true');
    expect(screen.getByTestId('questionnaire-progress')).toHaveTextContent('已答 1/5');
    expect(screen.getByTestId('questionnaire-gap')).toHaveTextContent('第 1 节「玩法」还有 1 题未完成');

    click('q-q_towers-opt-arrow');
    click('q-q_towers-opt-ice');
    // 第一节补齐 → 缺口指向第二节(选答的 q_note 不算缺口)
    expect(screen.getByTestId('questionnaire-gap')).toHaveTextContent('第 2 节「节奏」还有 2 题未完成');
    expect(screen.getByTestId('questionnaire-section-0')).toHaveAttribute('data-state', 'done');
    expect(screen.getByTestId('questionnaire-section-1')).toHaveAttribute('data-state', 'todo');
    expect(submit).toBeDisabled();

    // 点缺口提示直接跳到那一节
    click('questionnaire-gap');
    expect(screen.getByTestId('questionnaire-section-title')).toHaveTextContent('节奏');
    click('q-q_diff-scale-4');
    expect(screen.getByTestId('q-q_diff-scale-4')).toHaveAttribute('aria-checked', 'true');
    expect(submit).toBeDisabled();
    // 只有空白的文本不算作答
    typeInto('q-q_name-text', '   ');
    expect(submit).toBeDisabled();
    typeInto('q-q_name-text', '守城');
    expect(screen.queryByTestId('questionnaire-gap')).not.toBeInTheDocument();
    expect(screen.getByTestId('questionnaire-progress')).toHaveTextContent('已答 4/5');
    expect(submit).toBeEnabled();
  });

  it('「交给你决定」与具体答案互斥;再点一次取消', () => {
    render(<UltraPlanBlock block={card()} />);
    click('q-q_view-opt-top');
    const delegate = screen.getByTestId('q-q_view-delegate');
    expect(delegate).toHaveAttribute('aria-pressed', 'false');
    fireEvent.click(delegate);
    expect(delegate).toHaveAttribute('aria-pressed', 'true');
    expect(screen.getByTestId('q-q_view-opt-top')).toHaveAttribute('aria-checked', 'false');
    expect(screen.getByTestId('q-q_view')).toHaveAttribute('data-status', 'ok');
    expect(loadDraft(UP, 1)).toEqual({ q_view: { delegate: true } });
    // 选了具体答案 → 委托自动取消
    click('q-q_view-opt-side');
    expect(delegate).toHaveAttribute('aria-pressed', 'false');
    expect(loadDraft(UP, 1)).toEqual({ q_view: { choice: ['side'] } });
    // 委托 → 再点取消 → 回到未答
    fireEvent.click(delegate);
    fireEvent.click(delegate);
    expect(screen.getByTestId('q-q_view')).toHaveAttribute('data-status', 'empty');
    expect(loadDraft(UP, 1)).toBeNull();
  });

  it('「其余全部交给你决定」:只填没答好且允许委托的题;不许委托的必答题仍挡着提交', async () => {
    render(<UltraPlanBlock block={card()} />);
    click('q-q_view-opt-top');
    click('questionnaire-delegate-rest');
    // 已答的不动;其余可委托的(含选答题)全部委托;q_name 不许委托,留空
    expect(loadDraft(UP, 1)).toEqual({
      q_view: { choice: ['top'] },
      q_towers: { delegate: true },
      q_diff: { delegate: true },
      q_note: { delegate: true },
    });
    expect(screen.getByTestId('q-q_towers-delegate')).toHaveAttribute('aria-pressed', 'true');
    expect(screen.getByTestId('questionnaire-progress')).toHaveTextContent('已答 4/5');
    expect(screen.getByTestId('questionnaire-delegate-rest')).toBeDisabled();
    expect(screen.getByTestId('questionnaire-submit')).toBeDisabled();
    expect(screen.getByTestId('questionnaire-gap')).toHaveTextContent('第 2 节「节奏」还有 1 题未完成');

    click('questionnaire-gap');
    typeInto('q-q_name-text', '守城');
    click('questionnaire-submit');
    await waitFor(() => expect(asks()).toHaveLength(1));
    expect(asks()[0].body).toEqual({
      userInput: '',
      mode: 'ultraplan',
      ultraplan: {
        id: UP,
        rev: 1,
        action: 'answer',
        answers: {
          q_view: { choice: ['top'] },
          q_towers: { delegate: true },
          q_diff: { delegate: true },
          q_note: { delegate: true },
          q_name: { text: '守城' },
        },
      },
    });
  });

  it('实现技术栈必须明确选择:批量委托后仍阻止提交,提交保留后端选项', async () => {
    const stackQuestionnaire = {
      title: '实现技术栈确认',
      understanding: '2D 默认使用 Godot,3D 可选择 Godot 或 Rurix。',
      sections: [{
        id: 'implementation',
        title: '实现方式',
        questions: [
          {
            id: 'implementation_stack',
            kind: 'single',
            question: '请选择游戏维度与实现后端',
            required: true,
            allowDelegate: false,
            allowOther: false,
            options: [
              { id: '2d_godot', label: '2D · Godot', recommended: true },
              { id: '3d_godot', label: '3D · Godot' },
              { id: '3d_rurix', label: '3D · Rurix' },
            ],
          },
          questionnaire.sections[0].questions[0],
        ],
      }],
    };
    render(<UltraPlanBlock block={card({ payload: { id: UP, rev: 1, questionnaire: stackQuestionnaire } })} />);
    expect(within(screen.getByTestId('q-implementation_stack')).getAllByRole('radio')).toHaveLength(3);
    expect(screen.queryByTestId('q-implementation_stack-delegate')).not.toBeInTheDocument();
    expect(screen.queryByTestId('q-implementation_stack-other')).not.toBeInTheDocument();

    click('questionnaire-delegate-rest');
    expect(loadDraft(UP, 1)).toEqual({ q_view: { delegate: true } });
    expect(screen.getByTestId('q-implementation_stack')).toHaveAttribute('data-status', 'empty');
    expect(screen.getByTestId('questionnaire-submit')).toBeDisabled();
    click('questionnaire-submit');
    expect(asks()).toHaveLength(0);

    click('q-implementation_stack-opt-3d_rurix');
    expect(screen.getByTestId('q-implementation_stack-opt-3d_rurix')).toHaveAttribute('aria-checked', 'true');
    expect(screen.getByTestId('questionnaire-submit')).toBeEnabled();
    click('questionnaire-submit');
    await waitFor(() => expect(asks()).toHaveLength(1));
    expect(asks()[0].body).toEqual({
      userInput: '',
      mode: 'ultraplan',
      ultraplan: {
        id: UP,
        rev: 1,
        action: 'answer',
        answers: {
          implementation_stack: { choice: ['3d_rurix'] },
          q_view: { delegate: true },
        },
      },
    });
  });

  it('「其他」:single 与选项二选一;multi 与已选项并存并计入数量', () => {
    render(<UltraPlanBlock block={card()} />);
    click('q-q_view-opt-top');
    typeInto('q-q_view-other', '等距视角');
    expect(screen.getByTestId('q-q_view-opt-top')).toHaveAttribute('aria-checked', 'false');
    expect(screen.getByTestId('q-q_view')).toHaveAttribute('data-status', 'ok');
    expect(loadDraft(UP, 1)).toEqual({ q_view: { other: '等距视角' } });
    // 再选回选项 → 「其他」清空
    click('q-q_view-opt-side');
    expect(screen.getByTestId('q-q_view-other')).toHaveValue('');
    expect(loadDraft(UP, 1)).toEqual({ q_view: { choice: ['side'] } });
    // 清空「其他」且没有选项 → 这题回到未答
    typeInto('q-q_view-other', '等距');
    typeInto('q-q_view-other', '');
    expect(screen.getByTestId('q-q_view')).toHaveAttribute('data-status', 'empty');

    // multi:一个选项 + 「其他」= 2 项,满足 min=2
    click('q-q_towers-opt-arrow');
    expect(screen.getByTestId('q-q_towers')).toHaveAttribute('data-status', 'invalid');
    typeInto('q-q_towers-other', '毒塔');
    expect(screen.getByTestId('q-q_towers')).toHaveAttribute('data-status', 'ok');
    expect(screen.getByTestId('q-q_towers-bounds')).toHaveTextContent('已选 2');
    expect(loadDraft(UP, 1)).toEqual({ q_towers: { choice: ['arrow'], other: '毒塔' } });
    // 委托后「其他」一并清掉(互斥)
    click('q-q_towers-delegate');
    expect(screen.getByTestId('q-q_towers-other')).toHaveValue('');
    expect(loadDraft(UP, 1)).toEqual({ q_towers: { delegate: true } });
  });

  it('multi 数量界:不足 min 是缺口;选满 max 后其余项与「其他」不可再选,取消一项即恢复', () => {
    render(<UltraPlanBlock block={card()} />);
    click('q-q_view-opt-top');
    click('q-q_towers-opt-arrow');
    expect(screen.getByTestId('q-q_towers-opt-arrow')).toHaveAttribute('aria-checked', 'true');
    // 只选 1 项 < min=2:算缺口
    expect(screen.getByTestId('questionnaire-gap')).toHaveTextContent('第 1 节「玩法」还有 1 题未完成');
    click('q-q_towers-opt-cannon');
    expect(screen.getByTestId('q-q_towers')).toHaveAttribute('data-status', 'ok');
    click('q-q_towers-opt-ice');
    // 选满 3 项
    const fire = screen.getByTestId('q-q_towers-opt-fire');
    expect(fire).toBeDisabled();
    expect(fire).toHaveAttribute('title', '最多选 3 项');
    expect(screen.getByTestId('q-q_towers-other')).toBeDisabled();
    fireEvent.click(fire);
    expect(loadDraft(UP, 1)?.q_towers).toEqual({ choice: ['arrow', 'cannon', 'ice'] });
    // 已选中的仍可取消;取消后其余项恢复
    expect(screen.getByTestId('q-q_towers-opt-ice')).toBeEnabled();
    click('q-q_towers-opt-ice');
    expect(fire).toBeEnabled();
    expect(screen.getByTestId('q-q_towers-other')).toBeEnabled();
    // 全部取消 → 这题的草稿条目整条删除
    click('q-q_towers-opt-arrow');
    click('q-q_towers-opt-cannon');
    expect(loadDraft(UP, 1)).toEqual({ q_view: { choice: ['top'] } });
  });

  it('分节导航:上一节 / 下一节 + 分节圆点;首尾禁用;只有一节时不出导航', () => {
    const { unmount } = render(<UltraPlanBlock block={card()} />);
    const prev = screen.getByTestId('questionnaire-prev');
    const next = screen.getByTestId('questionnaire-next');
    expect(prev).toBeDisabled();
    expect(next).toBeEnabled();
    expect(screen.getByTestId('questionnaire-section-0')).toHaveAttribute('aria-current', 'step');
    fireEvent.click(next);
    expect(screen.getByTestId('questionnaire-section-title')).toHaveTextContent('节奏');
    expect(next).toBeDisabled();
    expect(prev).toBeEnabled();
    expect(screen.getByTestId('questionnaire-section-1')).toHaveAttribute('aria-current', 'step');
    expect(screen.getByTestId('questionnaire-section-0')).not.toHaveAttribute('aria-current');
    fireEvent.click(prev);
    expect(screen.getByTestId('q-q_view')).toBeInTheDocument();
    // 圆点直达
    click('questionnaire-section-1');
    expect(screen.getByTestId('q-q_diff')).toBeInTheDocument();
    unmount();

    const single = { ...questionnaire, sections: [questionnaire.sections[0]] };
    render(<UltraPlanBlock block={card({ payload: { id: UP, rev: 1, questionnaire: single } })} />);
    expect(screen.queryByTestId('questionnaire-prev')).not.toBeInTheDocument();
    expect(screen.queryByTestId('questionnaire-next')).not.toBeInTheDocument();
    expect(screen.getByTestId('questionnaire-gap')).toHaveTextContent('还有 2 题未完成');
  });

  it('键盘:radiogroup 内方向键移动并选中', () => {
    render(<UltraPlanBlock block={card()} />);
    const top = screen.getByTestId('q-q_view-opt-top');
    const side = screen.getByTestId('q-q_view-opt-side');
    top.focus();
    fireEvent.keyDown(top, { key: 'ArrowDown' });
    expect(document.activeElement).toBe(side);
    expect(side).toHaveAttribute('aria-checked', 'true');
    // 末项再往下回到首项
    fireEvent.keyDown(side, { key: 'ArrowRight' });
    expect(document.activeElement).toBe(top);
    expect(top).toHaveAttribute('aria-checked', 'true');
    fireEvent.keyDown(top, { key: 'ArrowUp' });
    expect(side).toHaveAttribute('aria-checked', 'true');
  });

  it('scale 范围过大时退成数字输入框,越界值不算作答', () => {
    const wide = {
      title: '宽量表',
      understanding: '',
      sections: [{ id: 's1', title: '数值', questions: [{ id: 'q_hp', kind: 'scale', question: '血量?', min: 0, max: 100 }] }],
    };
    render(<UltraPlanBlock block={card({ payload: { id: UP, rev: 1, questionnaire: wide } })} />);
    expect(screen.queryByTestId('q-q_hp-scale-0')).not.toBeInTheDocument();
    typeInto('q-q_hp-scale-input', '250');
    expect(screen.getByTestId('q-q_hp')).toHaveAttribute('data-status', 'empty');
    typeInto('q-q_hp-scale-input', '60');
    expect(loadDraft(UP, 1)).toEqual({ q_hp: { scale: 60 } });
    expect(screen.getByTestId('questionnaire-submit')).toBeEnabled();
  });
});

describe('<QuestionnaireCard /> 提交', () => {
  it('请求体 = {mode:"ultraplan", ultraplan:{id, rev, action:"answer", answers}};提交中禁用;受理后清草稿', async () => {
    let releasePost!: () => void;
    stubFetch((call) =>
      call.url === ASK_URL
        ? new Promise<Reply>((resolve) => {
            releasePost = () => resolve({ body: { run: { id: 'run_s', status: 'completed' } } });
          })
        : { body: { ultraplan: flow() } },
    );
    render(<UltraPlanBlock block={card()} />);
    fillAll();
    typeInto('q-q_note-text', '  多一点特效  ');
    click('questionnaire-prev');
    typeInto('q-q_towers-other', ' 毒塔 ');
    expect(localStorage.getItem(draftKey(UP, 1))).not.toBeNull();

    const submit = screen.getByTestId('questionnaire-submit');
    expect(submit).toBeEnabled();
    fireEvent.click(submit);
    await waitFor(() => expect(asks()).toHaveLength(1));
    expect(asks()[0].method).toBe('POST');
    // choice 按问卷里的选项顺序(点选顺序是 ice → arrow);自由文本去首尾空白
    expect(asks()[0].body).toEqual({
      userInput: '',
      mode: 'ultraplan',
      ultraplan: {
        id: UP,
        rev: 1,
        action: 'answer',
        answers: {
          q_view: { choice: ['top'] },
          q_towers: { choice: ['arrow', 'ice'], other: '毒塔' },
          q_diff: { scale: 4 },
          q_note: { text: '多一点特效' },
          q_name: { text: '守城' },
        },
      },
    });

    // 在途:提交钮转「提交中…」,表单整体禁用,重复点击不重发
    expect(submit).toBeDisabled();
    expect(submit).toHaveTextContent('提交中…');
    expect(screen.getByTestId('q-q_view-opt-side')).toBeDisabled();
    expect(screen.getByTestId('questionnaire-delegate-rest')).toBeDisabled();
    fireEvent.click(submit);
    expect(asks()).toHaveLength(1);
    // 在途期间草稿还在(被拒时要留给用户改)
    expect(loadDraft(UP, 1)).not.toBeNull();

    // 受理信号 = 带同一 {id, action, rev} 的 composer.user.message 回显(POST 要等整轮跑完才回)
    act(() => useUltraPlanStore.getState().noteUserMessage({ id: UP, action: 'answer', rev: 1 }));
    await waitFor(() => expect(screen.getByTestId('questionnaire-note')).toHaveTextContent('已提交'));
    expect(loadDraft(UP, 1)).toBeNull();
    expect(submit).toBeDisabled();
    expect(screen.queryByTestId('questionnaire-error')).not.toBeInTheDocument();
    await act(async () => {
      releasePost();
      await Promise.resolve();
    });
    expect(asks()).toHaveLength(1);
  });

  it('被拒:文案就地显示(warn 色,不用 danger),点名的题所在节被翻到并标出;草稿保留、可再次提交', async () => {
    stubFetch((call) =>
      call.url === ASK_URL
        ? {
            status: 400,
            body: {
              error: { code: 'ULTRAPLAN_ANSWERS_INVALID', message: '「难度」的取值超出范围', details: { questionId: 'q_diff' } },
            },
          }
        : { body: { ultraplan: flow() } },
    );
    render(<UltraPlanBlock block={card()} />);
    fillAll();
    click('questionnaire-prev');
    click('questionnaire-submit');
    const error = await screen.findByTestId('questionnaire-error');
    expect(error).toHaveTextContent('「难度」的取值超出范围');
    expect(error).toHaveAttribute('role', 'alert');
    expect(error.className).toContain('text-warn');
    expect(error.className).not.toContain('danger');
    // 翻到 q_diff 所在的第二节并标出这道题
    expect(screen.getByTestId('questionnaire-section-title')).toHaveTextContent('节奏');
    expect(screen.getByTestId('q-q_diff')).toHaveAttribute('data-flagged', '1');
    expect(screen.getByTestId('q-q_name')).not.toHaveAttribute('data-flagged');
    // 重新可填可提交;草稿没被清
    expect(screen.getByTestId('questionnaire-submit')).toBeEnabled();
    expect(screen.getByTestId('questionnaire-submit')).toHaveTextContent('提交');
    expect(loadDraft(UP, 1)).toMatchObject({ q_diff: { scale: 4 }, q_name: { text: '守城' } });
    // 一改答案,旧的报错随即收起
    click('q-q_diff-scale-3');
    expect(screen.queryByTestId('questionnaire-error')).not.toBeInTheDocument();
    click('questionnaire-submit');
    await waitFor(() => expect(asks()).toHaveLength(2));
  });

  it('409 阶段已变:报错不留在卡上——重拉后这张卡按新阶段转只读', async () => {
    stubFetch((call) =>
      call.url === ASK_URL
        ? {
            status: 409,
            body: {
              error: {
                code: 'ULTRAPLAN_STAGE_MISMATCH',
                message: '流程已进入 Demo 阶段',
                details: { stage: 'demo_review', allowed: ['approve_demo', 'revise_demo'] },
              },
            },
          }
        : { body: { ultraplan: flow({ stage: 'demo_review', demoIteration: 1 }) } },
    );
    render(<UltraPlanBlock block={card()} />);
    fillAll();
    click('questionnaire-submit');
    await waitFor(() =>
      expect(screen.getByTestId('questionnaire-card')).toHaveAttribute('data-mode', 'readonly'),
    );
    expect(screen.getByTestId('questionnaire-note')).toHaveTextContent('流程已进入「Demo」阶段');
    expect(screen.queryByTestId('questionnaire-submit')).not.toBeInTheDocument();
    const toast = useToastStore.getState().items.find((t) => t.title === '流程已进入 Demo 阶段');
    expect(toast?.kind).toBe('warning');
  });
});

describe('<QuestionnaireCard /> 草稿', () => {
  it('每次改动即存;重挂(切会话 / 刷新)后恢复,并落到第一处没填完的那一节', () => {
    const first = render(<UltraPlanBlock block={card()} />);
    click('q-q_view-opt-top');
    click('q-q_towers-opt-arrow');
    click('q-q_towers-opt-fire');
    click('questionnaire-next');
    typeInto('q-q_note-text', '想要 Boss 战');
    expect(JSON.parse(localStorage.getItem(draftKey(UP, 1)) ?? 'null')).toEqual({
      q_view: { choice: ['top'] },
      q_towers: { choice: ['arrow', 'fire'] },
      q_note: { text: '想要 Boss 战' },
    });
    first.unmount();

    render(<UltraPlanBlock block={card()} />);
    // 第一节已答完 → 直接落在第二节
    expect(screen.getByTestId('questionnaire-section-title')).toHaveTextContent('节奏');
    expect(screen.getByTestId('q-q_note-text')).toHaveValue('想要 Boss 战');
    expect(screen.getByTestId('questionnaire-progress')).toHaveTextContent('已答 3/5');
    click('questionnaire-prev');
    expect(screen.getByTestId('q-q_view-opt-top')).toHaveAttribute('aria-checked', 'true');
    expect(screen.getByTestId('q-q_towers-opt-arrow')).toHaveAttribute('aria-checked', 'true');
    expect(screen.getByTestId('q-q_towers-opt-fire')).toHaveAttribute('aria-checked', 'true');
    expect(screen.getByTestId('q-q_towers-opt-ice')).toHaveAttribute('aria-checked', 'false');
  });

  it('草稿按 id + rev 隔离;认不出的题 / 选项 / 越界值不带进表单', () => {
    // 别的版本、别的流程的草稿互不串
    saveDraft(UP, 2, { q_view: { choice: ['side'] } });
    saveDraft('up_other', 1, { q_view: { choice: ['side'] } });
    saveDraft(UP, 1, {
      q_view: { choice: ['gone'] },
      q_towers: { choice: ['arrow', 'gone', 'arrow'] },
      q_diff: { scale: 9 },
      q_removed: { text: '旧题' },
    } as UltraPlanAnswers);
    render(<UltraPlanBlock block={card()} />);
    expect(screen.getByTestId('q-q_view-opt-side')).toHaveAttribute('aria-checked', 'false');
    expect(screen.getByTestId('q-q_view')).toHaveAttribute('data-status', 'empty');
    expect(screen.getByTestId('q-q_towers-opt-arrow')).toHaveAttribute('aria-checked', 'true');
    expect(screen.getByTestId('q-q_towers-bounds')).toHaveTextContent('已选 1');
    click('questionnaire-next');
    expect(screen.getByTestId('q-q_diff')).toHaveAttribute('data-status', 'empty');
    expect(screen.getByTestId('questionnaire-progress')).toHaveTextContent('已答 0/5');
  });
});

describe('<QuestionnaireCard /> 只读形态', () => {
  const submittedAnswers: UltraPlanAnswers = {
    q_view: { choice: ['top'] },
    q_towers: { choice: ['arrow', 'ice'], other: '毒塔' },
    q_diff: { delegate: true },
    q_name: { text: '守城' },
  };

  it('已提交:回显选项 label / 「由 AI 决定」/ 未填,不再有任何表单控件', () => {
    useUltraPlanStore.getState().hydrate(flow({ stage: 'demo_review', demoIteration: 1 }), SID);
    render(
      <UltraPlanBlock
        block={card({ submitted: { runId: 'run_s', id: UP, rev: 1, answers: submittedAnswers, delegated: 1 } })}
      />,
    );
    expect(screen.getByTestId('ultraplan-block')).toHaveAttribute('data-submitted', '1');
    const root = screen.getByTestId('questionnaire-card');
    expect(root).toHaveAttribute('data-mode', 'submitted');
    expect(screen.getByTestId('questionnaire-submitted')).toHaveTextContent('已提交 · 共 5 题,其中 1 题由 AI 决定');
    expect(screen.getByTestId('q-q_view-submitted')).toHaveTextContent('俯视');
    expect(screen.getByTestId('q-q_towers-submitted')).toHaveTextContent('箭塔、冰塔、其他:毒塔');
    expect(screen.getByTestId('q-q_diff-submitted')).toHaveTextContent('由 AI 决定');
    expect(screen.getByTestId('q-q_note-submitted')).toHaveTextContent('未填');
    expect(screen.getByTestId('q-q_name-submitted')).toHaveTextContent('守城');
    expect(screen.queryByTestId('questionnaire-submit')).not.toBeInTheDocument();
    expect(screen.queryByTestId('q-q_view-opt-top')).not.toBeInTheDocument();
    expect(within(root).queryAllByRole('radio')).toHaveLength(0);
    // 「项目理解」默认收起,仍可展开回看
    expect(screen.queryByTestId('questionnaire-understanding')).not.toBeInTheDocument();
    click('questionnaire-understanding-toggle');
    expect(screen.getByTestId('questionnaire-understanding')).toHaveTextContent('我的理解');
  });

  it('已提交优先于一切:即便阶段还停在问卷关口(上一轮失败)也只读回显', () => {
    useUltraPlanStore.getState().hydrate(flow({ phase: 'failed' }), SID);
    render(<UltraPlanBlock block={card({ submitted: { id: UP, rev: 1, answers: submittedAnswers } })} />);
    expect(screen.getByTestId('questionnaire-card')).toHaveAttribute('data-mode', 'submitted');
    expect(screen.queryByTestId('questionnaire-submit')).not.toBeInTheDocument();
  });

  it('旧版问卷:「已有更新的问卷」', () => {
    useUltraPlanStore.getState().hydrate(flow({ questionnaireRev: 2 }), SID);
    render(<UltraPlanBlock block={card()} />);
    expect(screen.getByTestId('questionnaire-card')).toHaveAttribute('data-mode', 'readonly');
    expect(screen.getByTestId('questionnaire-note')).toHaveTextContent('已有更新的问卷');
    expect(screen.queryByTestId('questionnaire-submit')).not.toBeInTheDocument();
    expect(screen.queryByTestId('q-q_view-opt-top')).not.toBeInTheDocument();
  });

  it('别的流程 / 没有流程(fork 出来的会话、已重新开始)/ store 里挂的是别的会话的流程:说明归属', () => {
    useUltraPlanStore.getState().hydrate(flow({ id: 'up_new' }), SID);
    const { rerender } = render(<UltraPlanBlock block={card()} />);
    expect(screen.getByTestId('questionnaire-note')).toHaveTextContent('此流程属于其他会话');

    act(() => useUltraPlanStore.getState().hydrate(null, SID));
    rerender(<UltraPlanBlock block={card()} />);
    expect(screen.getByTestId('questionnaire-note')).toHaveTextContent('此流程属于其他会话');

    act(() => useUltraPlanStore.getState().hydrate(flow(), 'sess_other'));
    rerender(<UltraPlanBlock block={card()} />);
    expect(screen.getByTestId('questionnaire-card')).toHaveAttribute('data-mode', 'readonly');
    expect(screen.queryByTestId('questionnaire-submit')).not.toBeInTheDocument();
  });

  it('关口已过但没有提交记录(历史被截断):说明流程所在阶段', () => {
    useUltraPlanStore.getState().hydrate(flow({ stage: 'plan_review', demoIteration: 1, planRev: 1 }), SID);
    render(<UltraPlanBlock block={card()} />);
    expect(screen.getByTestId('questionnaire-note')).toHaveTextContent('流程已进入「计划」阶段');
  });

  it('卡片比本地阶段新(建卡事件先到、阶段重拉未回):显示同步中,重拉回来即可填写', () => {
    useUltraPlanStore
      .getState()
      .hydrate(flow({ stage: 'discovery', questionnaireRev: 0, phase: 'running', running: 'discovery' }), SID);
    render(<UltraPlanBlock block={card()} />);
    expect(screen.getByTestId('questionnaire-card')).toHaveAttribute('data-mode', 'readonly');
    expect(screen.getByTestId('questionnaire-note')).toHaveTextContent('正在同步流程状态');
    act(() => useUltraPlanStore.getState().hydrate(flow(), SID));
    expect(screen.getByTestId('questionnaire-card')).toHaveAttribute('data-mode', 'form');
    expect(screen.getByTestId('questionnaire-card')).toHaveAttribute('data-interactive', '1');
  });

  it('有任务正在运行:表单原样留着但整体禁用并说明;任务结束即恢复,草稿不丢', () => {
    render(<UltraPlanBlock block={card()} />);
    click('q-q_view-opt-top');
    act(() => useChatStore.setState({ activeRunId: 'run_9' }));
    expect(screen.getByTestId('questionnaire-note')).toHaveTextContent('有任务正在运行');
    expect(screen.getByTestId('questionnaire-card')).not.toHaveAttribute('data-interactive');
    expect(screen.getByTestId('q-q_view-opt-side')).toBeDisabled();
    expect(screen.getByTestId('q-q_view-other')).toBeDisabled();
    expect(screen.getByTestId('q-q_view-delegate')).toBeDisabled();
    expect(screen.getByTestId('questionnaire-delegate-rest')).toBeDisabled();
    expect(screen.getByTestId('questionnaire-submit')).toBeDisabled();
    fireEvent.click(screen.getByTestId('q-q_view-opt-side'));
    expect(screen.getByTestId('q-q_view-opt-top')).toHaveAttribute('aria-checked', 'true');
    // 翻节只是看,不受影响
    expect(screen.getByTestId('questionnaire-next')).toBeEnabled();

    act(() => useChatStore.setState({ activeRunId: null }));
    expect(screen.queryByTestId('questionnaire-note')).not.toBeInTheDocument();
    expect(screen.getByTestId('q-q_view-opt-side')).toBeEnabled();
    expect(screen.getByTestId('q-q_view-opt-top')).toHaveAttribute('aria-checked', 'true');

    // 流程自己在跑(phase=running)同理
    act(() => useUltraPlanStore.getState().hydrate(flow({ phase: 'running', running: 'discovery' }), SID));
    expect(screen.getByTestId('questionnaire-note')).toHaveTextContent('有任务正在运行');
    expect(screen.getByTestId('questionnaire-submit')).toBeDisabled();
  });

  it('Codex 可以填写问卷;代理不是 coding 时表单禁用', () => {
    const session = (agentKind: string, agentEngine: 'local' | 'codex') => ({
      id: SID, title: 'x', status: 'idle', agentKind, agentEngine,
      selectedModelId: 'mock', thinkingEnabled: false, reasoningEffort: null,
      contextOptionId: null, webSearchEnabled: false, activeRunId: null,
      createdAt: '', updatedAt: '', pinned: false, titleManuallySet: false,
    });
    useSessionStore.setState({ sessions: [session('coding', 'codex')] });
    render(<UltraPlanBlock block={card()} />);
    expect(screen.queryByTestId('questionnaire-note')).not.toBeInTheDocument();
    expect(screen.getByTestId('q-q_view-opt-top')).toBeEnabled();
    act(() => useSessionStore.setState({ sessions: [session('general', 'local')] }));
    expect(screen.getByTestId('questionnaire-note')).toHaveTextContent('当前代理类型不支持 UltraPlan');
    act(() => useSessionStore.setState({ sessions: [session('coding', 'local')] }));
    expect(screen.queryByTestId('questionnaire-note')).not.toBeInTheDocument();
    expect(screen.getByTestId('q-q_view-opt-top')).toBeEnabled();
  });

  it('载荷残缺(没有问卷 / 没有可答的题)不抛,给出说明', () => {
    const { unmount } = render(<UltraPlanBlock block={card({ payload: { id: UP, rev: 1 } })} />);
    expect(screen.getByTestId('questionnaire-note')).toHaveTextContent('问卷内容缺失');
    unmount();
    render(
      <UltraPlanBlock
        block={card({ payload: { id: UP, rev: 1, questionnaire: { title: '空问卷', sections: [{ id: 's', title: 't', questions: [{ id: 'x', kind: 'nope' }] }] } } })}
      />,
    );
    expect(screen.getByTestId('questionnaire-card')).toHaveTextContent('空问卷');
    expect(screen.getByTestId('questionnaire-note')).toHaveTextContent('问卷内容缺失');
  });
});

describe('questionnaireModel(纯逻辑)', () => {
  const q = normalizeQuestionnaire(questionnaire)!;
  const byId = (id: string) => q.sections.flatMap((s) => s.questions).find((x) => x.id === id)!;

  it('normalizeQuestionnaire:丢掉不成形的节 / 题 / 选项与重复 id;非对象回 null', () => {
    expect(normalizeQuestionnaire(null)).toBeNull();
    expect(normalizeQuestionnaire('x')).toBeNull();
    const out = normalizeQuestionnaire({
      title: 7,
      sections: [
        'junk',
        { id: 's1', title: '一', questions: [
          { id: 'a', kind: 'single', question: 'A?', options: [{ id: 'o1', label: '甲' }, { id: 'o1', label: '重复' }, { label: '无 id' }] },
          { id: 'a', kind: 'text', question: '重复 id' },
          { id: 'b', kind: 'single', question: '没有选项也不许自填' },
          { id: 'c', kind: 'multi', question: '只许自填', allowOther: true },
          { kind: 'text', question: '无 id' },
        ] },
        { id: 's2', title: '空节', questions: [] },
      ],
    })!;
    expect(out.title).toBe('');
    expect(out.sections).toHaveLength(1);
    expect(out.sections[0].questions.map((x) => x.id)).toEqual(['a', 'c']);
    expect(out.sections[0].questions[0].options).toEqual([{ id: 'o1', label: '甲' }]);
  });

  it('answerStatus / multiBounds:缺省必答、缺省 1..5、「其他」计入数量', () => {
    expect(answerStatus(byId('q_view'), undefined)).toBe('empty');
    expect(answerStatus(byId('q_view'), { choice: ['top'] })).toBe('ok');
    expect(answerStatus(byId('q_view'), { other: '  ' })).toBe('empty');
    expect(answerStatus(byId('q_view'), { other: '等距' })).toBe('ok');
    expect(answerStatus(byId('q_name'), { delegate: true })).toBe('empty'); // 不许委托
    expect(multiBounds(byId('q_towers'))).toEqual({ min: 2, max: 3 });
    expect(answerStatus(byId('q_towers'), { choice: ['arrow'] })).toBe('invalid');
    expect(answerStatus(byId('q_towers'), { choice: ['arrow', 'cannon', 'ice', 'fire'] })).toBe('invalid');
    expect(answerStatus(byId('q_diff'), { scale: 0 })).toBe('empty');
    expect(answerStatus(byId('q_diff'), { scale: 2.5 })).toBe('empty');
    expect(answerStatus(byId('q_diff'), { scale: 5 })).toBe('ok');
    // 没写 min / max 的 multi:1..(选项数 + 其他)
    const loose = normalizeQuestionnaire({
      sections: [{ id: 's', title: 't', questions: [{ id: 'm', kind: 'multi', question: '?', options: [{ id: 'a', label: 'A' }, { id: 'b', label: 'B' }] }] }],
    })!.sections[0].questions[0];
    expect(multiBounds(loose)).toEqual({ min: 1, max: 2 });
  });

  it('buildAnswers / sanitizeDraft / delegateRest:只出契约字段,委托与具体答案互斥', () => {
    const draft = sanitizeDraft(q, {
      q_view: { choice: ['top'], other: '会被丢掉', delegate: false },
      q_towers: { delegate: true, choice: ['arrow'] },
      q_diff: { scale: 3, text: '多余字段' },
      q_note: { text: '   ' },
    });
    expect(draft).toEqual({
      q_view: { choice: ['top'] },
      q_towers: { delegate: true },
      q_diff: { scale: 3 },
    });
    expect(delegateRest(q, draft)).toEqual({ ...draft, q_note: { delegate: true } });
    expect(buildAnswers(q, { ...draft, q_name: { text: ' 守城 ' } })).toEqual({
      q_view: { choice: ['top'] },
      q_towers: { delegate: true },
      q_diff: { scale: 3 },
      q_name: { text: '守城' },
    });
  });
});
